//! The macOS window: a borderless, see-through NSWindow whose every pixel (glass body, neon
//! rim, window buttons, sidebar) is drawn by us, matching the Windows version.

mod keys;
mod paint;
mod view;

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use core_vt::Terminal;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType, NSColor, NSMenu,
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
            redraw_now();
        }

        #[unsafe(method(resignKeyWindow))]
        fn resign_key(&self) {
            let _: () = unsafe { msg_send![super(self), resignKeyWindow] };
            redraw_now();
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
    let started = Instant::now();
    let mtm = MainThreadMarker::new().expect("must start on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    build_menu(mtm, &app);

    // Everything after our own name is the command to run through the login shell.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    let command = args.join(" ");

    let pty = match Pty::spawn(&command, None, 80, 24) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("trinidad-head: could not start the shell: {e}");
            return;
        }
    };
    let shared = Arc::new(Mutex::new(Shared { term: Terminal::new(80, 24), last_output: None }));
    let pty = Arc::new(Mutex::new(pty));

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
    std::thread::spawn(move || reader_loop(output, shared, pty));

    window.makeKeyAndOrderFront(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    app.run();
}

fn reader_loop(mut output: std::fs::File, shared: Arc<Mutex<Shared>>, pty: Arc<Mutex<Pty>>) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match output.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let responses = {
                    let mut s = shared.lock().unwrap();
                    s.term.feed(&buf[..n]);
                    s.last_output = Some(Instant::now());
                    s.term.take_responses()
                };
                if !responses.is_empty() {
                    let _ = pty.lock().unwrap().write(&responses);
                }
                request_redraw();
            }
        }
    }
    // The shell ended: close the window, which quits the app.
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
