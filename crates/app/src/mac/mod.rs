//! The macOS window: a borderless, see-through NSWindow whose every pixel (glass body, neon
//! rim, window buttons, sidebar) is drawn by us, matching the Windows version.

mod control;
mod dock;
mod keys;
mod paint;
mod selftest;
mod view;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use core_vt::Terminal;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSColor, NSMenu,
    NSMenuItem, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{ns_string, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize};
use pty::Pty;

pub(crate) use view::TermView;

/// State shared between the main thread and the shell-reader thread.
pub(crate) struct Shared {
    pub term: Terminal,
    pub last_output: Option<Instant>,
}

static REDRAW_QUEUED: AtomicBool = AtomicBool::new(false);
static TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// This window's control token.
pub(crate) fn window_token() -> &'static str {
    TOKEN.get().map(String::as_str).unwrap_or("")
}

define_class!(
    // A borderless window can't normally take keyboard focus; this one can.
    #[unsafe(super(NSWindow))]
    #[thread_kind = MainThreadOnly]
    #[name = "TrinidadHeadWindow"]
    pub(crate) struct THWindow;

    impl THWindow {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }

        #[unsafe(method(becomeKeyWindow))]
        fn become_key(&self) {
            let _: () = unsafe { msg_send![super(self), becomeKeyWindow] };
            focus_changed(true);
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            let _: () = unsafe { msg_send![super(self), resignKeyWindow] };
            focus_changed(false);
        }
    }
);

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TrinidadHeadAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate(&self, _app: &NSApplication) -> bool {
            true
        }

        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _visible: bool) -> bool {
            dock::bring_all_forward();
            false
        }

        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _note: &objc2_foundation::NSNotification) {
            selftest::on_terminate();
            control::cleanup(window_token());
            // Hang up on the shell and everything it runs (the process group it leads).
            let pid = SHELL_PID.load(Ordering::Relaxed);
            if pid > 0 {
                unsafe {
                    libc::kill(-pid, libc::SIGHUP);
                }
            }
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    static VIEW: std::cell::RefCell<Option<Retained<TermView>>> = const { std::cell::RefCell::new(None) };
}

/// Ask the view to repaint on the main thread (safe to call from any thread).
pub(crate) fn request_redraw() {
    if !REDRAW_QUEUED.swap(true, Ordering::SeqCst) {
        dispatch2::DispatchQueue::main().exec_async(|| {
            REDRAW_QUEUED.store(false, Ordering::SeqCst);
            redraw_now();
        });
    }
}

fn redraw_now() {
    VIEW.with(|v| {
        if let Some(v) = v.borrow().as_ref() {
            v.setNeedsDisplay(true);
        }
    });
}

fn focus_changed(focused: bool) {
    VIEW.with(|v| {
        if let Some(v) = v.borrow().as_ref() {
            v.focus_changed(focused);
            v.setNeedsDisplay(true);
        }
    });
}

fn build_menu(mtm: MainThreadMarker, app: &NSApplication) {
    unsafe {
        let bar = NSMenu::new(mtm);
        let app_item = NSMenuItem::new(mtm);
        bar.addItem(&app_item);
        let app_menu = NSMenu::new(mtm);
        let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            ns_string!("Quit Trinidad Head"),
            Some(sel!(terminate:)),
            ns_string!("q"),
        );
        app_menu.addItem(&quit);
        app_item.setSubmenu(Some(&app_menu));
        app.setMainMenu(Some(&bar));
    }
}

pub fn run() {
    // Before any window or shell exists: a test asking "would you update?" must not
    // leave a child process holding the pipe it is reading.
    if std::env::var_os("TRINIDAD_HEAD_UPDATE_NOW").is_some() {
        crate::update::run_once_and_exit();
    }
    let started = Instant::now();
    let mtm = MainThreadMarker::new().expect("must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    dock::start(mtm);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    build_menu(mtm, &app);

    // Everything after our own name is the command to run through the login shell.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    let command = args.join(" ");

    // The window's control token, handed to the shell so tools inside can find this window.
    let token = control::token();
    let _ = TOKEN.set(token.clone());
    // A line to type into the shell once it starts (Ghostty Run's .ghostty/.command files).
    let initial_input = std::env::var("TRINIDAD_HEAD_INPUT").ok();
    unsafe {
        std::env::set_var("TRINIDAD_HEAD_TOKEN", &token);
        std::env::remove_var("TRINIDAD_HEAD_INPUT");
    }

    let pty = match Pty::spawn(&command, None, 80, 24) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("trinidad-head: could not start the shell: {e}");
            return;
        }
    };
    SHELL_PID.store(pty.pid() as i32, Ordering::Relaxed);
    let shared = Arc::new(Mutex::new(Shared { term: Terminal::new(80, 24), last_output: None }));
    let pty = Arc::new(Mutex::new(pty));
    if let Some(line) = initial_input.filter(|l| !l.is_empty()) {
        // The shell reads this once it is ready, like typing ahead.
        let _ = pty.lock().unwrap().write(format!("{line}\r").as_bytes());
    }
    control::listen(&token, shared.clone(), pty.clone());

    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1080.0, 700.0));
    let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::Resizable | NSWindowStyleMask::Miniaturizable;
    let window: Retained<THWindow> = unsafe {
        msg_send![
            THWindow::alloc(mtm),
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: false
        ]
    };
    unsafe { window.setReleasedWhenClosed(false) };
    window.setOpaque(false);
    window.setBackgroundColor(Some(&NSColor::clearColor()));
    window.setHasShadow(false);
    window.setAcceptsMouseMovedEvents(true);
    window.setMinSize(NSSize::new(420.0, 280.0));
    window.center();

    let view = TermView::new(mtm, frame, shared.clone(), pty.clone(), started);
    window.setContentView(Some(&view));
    window.makeFirstResponder(Some(&view));
    VIEW.with(|v| *v.borrow_mut() = Some(view.clone()));
    view.fit();

    // Shell-reader thread: parse output off the main thread, then ask for one repaint.
    let output = pty.lock().unwrap().take_output().expect("pty output");
    let pty_for_reader = pty.clone();
    std::thread::spawn(move || reader_loop(output, shared, pty_for_reader));
    // Close when the shell itself ends, even if something it started (a background helper, an
    // MCP server) still holds the terminal open and the output never reaches end-of-file.
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let exited = pty.lock().unwrap().exit_code().is_some();
        if exited {
            // Give the last output a moment to be read and drawn.
            std::thread::sleep(std::time::Duration::from_millis(300));
            close_window_soon();
            break;
        }
    });

    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    quit_on_signals();
    // Fetch and rebuild in the background if main has moved: the next window is the new one.
    crate::update::spawn_check();
    if let Ok(mode) = std::env::var("TRINIDAD_HEAD_SELFTEST") {
        selftest::start(&mode, window.retain(), view.clone());
    }
    if let Some(path) = std::env::var_os("TRINIDAD_HEAD_SHOT") {
        // Test hook: a picture of the window, drawn by the view itself (no Screen Recording needed).
        selftest::shot_later(view.clone(), std::path::PathBuf::from(path));
    }
    app.run();
}

fn reader_loop(mut output: std::fs::File, shared: Arc<Mutex<Shared>>, pty: Arc<Mutex<Pty>>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match output.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let (responses, clip) = {
                    let mut s = shared.lock().unwrap();
                    s.term.feed(&buf[..n]);
                    s.last_output = Some(Instant::now());
                    (s.term.take_responses(), s.term.take_clipboard())
                };
                // A program asked to copy text (OSC 52), e.g. Claude Code's copy command.
                if let Some(text) = clip {
                    dispatch2::DispatchQueue::main().exec_async(move || {
                        let pb = objc2_app_kit::NSPasteboard::generalPasteboard();
                        pb.clearContents();
                        pb.setString_forType(
                            &objc2_foundation::NSString::from_str(&text),
                            unsafe { objc2_app_kit::NSPasteboardTypeString },
                        );
                    });
                }
                if !responses.is_empty() {
                    let _ = pty.lock().unwrap().write(&responses);
                }
                request_redraw();
            }
        }
    }
    close_window_soon();
}

static SHELL_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

static SIGNAL_PIPE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_signal(_sig: libc::c_int) {
    let fd = SIGNAL_PIPE.load(Ordering::Relaxed);
    if fd >= 0 {
        unsafe {
            libc::write(fd, b"x".as_ptr() as *const libc::c_void, 1);
        }
    }
}

/// SIGTERM / SIGHUP / SIGINT close the window the normal way, so the shell is hung up on and
/// the control socket and window entry are removed.
fn quit_on_signals() {
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    SIGNAL_PIPE.store(fds[1], Ordering::Relaxed);
    unsafe {
        for sig in [libc::SIGTERM, libc::SIGHUP, libc::SIGINT] {
            libc::signal(sig, on_signal as *const () as libc::sighandler_t);
        }
    }
    let read_fd = fds[0];
    std::thread::spawn(move || {
        let mut b = [0u8; 1];
        if unsafe { libc::read(read_fd, b.as_mut_ptr() as *mut libc::c_void, 1) } == 1 {
            dispatch2::DispatchQueue::main().exec_async(|| {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).terminate(None);
                }
            });
            // If the main thread is stuck, don't hang around forever.
            std::thread::sleep(std::time::Duration::from_secs(5));
            control::cleanup(window_token());
            std::process::exit(0);
        }
    });
}

/// Close the window (which quits the app) on the main thread.
fn close_window_soon() {
    dispatch2::DispatchQueue::main().exec_async(|| {
        VIEW.with(|v| {
            if let Some(v) = v.borrow().as_ref() {
                if let Some(w) = v.window() {
                    w.close();
                }
            }
        });
    });
}
