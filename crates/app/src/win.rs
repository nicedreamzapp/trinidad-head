use std::cell::RefCell;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_vt::{Attrs, Cell, Color, Terminal};
use pty::Pty;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_GRADIENT_STOP, D2D_RECT_F, D2D1_BORDER_MODE_SOFT,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, ID2D1Effect, ID2D1SolidColorBrush, D2D1_BUFFER_PRECISION_8BPC_UNORM,
    D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT, D2D1_COLOR_SPACE_SRGB, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAUSSIANBLUR_PROP_BORDER_MODE, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PROPERTY_TYPE_ENUM, D2D1_PROPERTY_TYPE_FLOAT, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, ScreenToClient, PAINTSTRUCT};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, ShellExecuteW, HDROP};
use windows::Win32::UI::HiDpi::{
    GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_CONTROL,
    VK_DELETE, VK_DOWN, VK_END, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9,
    VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, IsZoomed, KillTimer, SetTimer,
    LoadCursorW, PostMessageW, PostQuitMessage, RegisterClassW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, MSG, NCCALCSIZE_PARAMS, SM_CXFRAME,
    SM_CXPADDEDBORDER, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE,
    WHEEL_DELTA, WM_APP, WM_CHAR, WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_RBUTTONUP, WM_TIMER,
    WM_SETFOCUS, WM_SIZE, WM_SYSCHAR, WM_SYSKEYDOWN, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_OVERLAPPEDWINDOW,
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, SetForegroundWindow, TrackPopupMenu, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_DROPFILES,
};
use windows_numerics::{Matrix3x2, Vector2};

use crate::gfx::Gfx;
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED};
use windows::Win32::Graphics::Direct2D::{ID2D1Factory1, ID2D1Geometry, ID2D1LinearGradientBrush, ID2D1PathGeometry1};
use crate::layout::{Button, Hit, Layout, GLASS};
use crate::theme::{self, GLOWS};

const WM_TERM_OUTPUT: u32 = WM_APP + 1;
/// Ticks while a drag-selection is held past the top or bottom edge of the text.
const AUTOSCROLL_TIMER: usize = 1;
const WM_TERM_EXITED: u32 = WM_APP + 2;
const CF_UNICODETEXT: u32 = 13;
// One size up from 13 (Matt, 2026-09-17: terminal text has always been too small to read).
const FONT_DIP: f32 = 15.0;
const APP_NAME: &str = "Trinidad Head";

/// State shared between the window thread and the shell-reader thread.
struct Shared {
    term: Terminal,
    last_output: Option<Instant>,
}

/// The window's shape, rim brush and blurred bloom for one size and color theme.
#[derive(Clone)]
struct ChromeCache {
    key: (u32, u32, usize, bool),
    bloom: ID2D1Effect,
    shape: ID2D1Geometry,
    /// The sunken window inside the glass, a hairline just outside it, the pale edge just
    /// inside the rim, and the path the top sheen follows.
    window: ID2D1Geometry,
    window_edge: ID2D1Geometry,
    edge: ID2D1Geometry,
    sheen: ID2D1Geometry,
    rim: ID2D1LinearGradientBrush,
}

struct App {
    hwnd: HWND,
    shared: Arc<Mutex<Shared>>,
    pty: Arc<Mutex<Pty>>,
    gfx: Option<Gfx>,
    brush: Option<ID2D1SolidColorBrush>,
    glow_cache: Option<ChromeCache>,
    dwrite: IDWriteFactory,
    formats: Vec<IDWriteTextFormat>, // regular, bold, italic, bold-italic
    icon_format: Option<IDWriteTextFormat>,
    icon_small: Option<IDWriteTextFormat>,
    font_family: Vec<u16>,
    icon_family: Vec<u16>,
    cell_w: f32,
    cell_h: f32,
    layout: Layout,
    glow: usize,
    /// This window's entry in the open-windows registry (holds its glow color).
    registry_file: Option<std::path::PathBuf>,
    scroll_offset: usize,
    focused: bool,
    hover: Option<Button>,
    pressed: Option<Button>,
    tracking_mouse: bool,
    high_surrogate: Option<u16>,
    /// Selected text as (line, column) points; lines count from the top of history.
    sel: Option<((usize, usize), (usize, usize))>,
    /// Claude's prompt-bar color, read from its theme file at startup.
    user_bar: (u8, u8, u8),
    selecting: bool,
    skip_char: bool,
    /// A button press we reported to the program (Claude's fullscreen view wants the mouse).
    mouse_reported: bool,
    /// A press the program wants, held back until we know it is a click (sent to the program)
    /// and not a drag (a text selection here): screen cell, text point.
    pending_press: Option<((usize, usize), (usize, usize))>,
    /// A drag-selection held past the top or bottom edge keeps scrolling on a timer.
    /// `drag_pt` is where the pointer last was, so each tick can stretch the selection to it
    /// without waiting for the mouse to move again.
    autoscroll: bool,
    drag_pt: (f32, f32),
    /// Autoscroll ticks so far (the self-test reads it out of the screen dump).
    autoscroll_ticks: u64,
    meter: crate::latency::Meter,
    last_title: Instant,
    started: Instant,
    first_frame_logged: bool,
    dump_path: Option<std::path::PathBuf>,
    last_dump: Instant,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

static PAINT_QUEUED: AtomicBool = AtomicBool::new(false);
/// Shell output arrived while a handler of ours was still running (see `wndproc`).
static MISSED_OUTPUT: AtomicBool = AtomicBool::new(false);

pub fn run() {
    let started = Instant::now();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    // Tell programs in the shell we draw full 24-bit color (Claude Code checks this).
    std::env::set_var("COLORTERM", "truecolor");
    std::env::set_var("TERM_PROGRAM", "TrinidadHead");
    // Everything after our own name is the command to run, passed through exactly as typed so
    // quoted paths with spaces survive (shortcuts rely on this).
    let command = unsafe { windows::Win32::System::Environment::GetCommandLineW().to_string() }
        .ok()
        .map(|line| crate::cmdline::tail(&line).to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(pty::default_shell);
    let data_dir = std::env::var("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("TrinidadHead")).ok();
    if let Some(d) = &data_dir {
        let _ = std::fs::create_dir_all(d);
    }
    let log = data_dir
        .as_ref()
        .and_then(|d| std::fs::OpenOptions::new().create(true).append(true).open(d.join("latency.log")).ok());
    let settings_path = data_dir.as_ref().map(|d| d.join("settings.txt"));
    let default_glow = settings_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|t| theme::parse_settings(&t))
        .unwrap_or(0);
    // Every open window gets its own glow color: look at the others first.
    let registry = data_dir.as_ref().map(|d| d.join("windows"));
    let glow = theme::pick_glow(default_glow, &registry.as_deref().map(live_window_glows).unwrap_or_default());
    let registry_file = registry.as_ref().map(|d| d.join(std::process::id().to_string()));
    if let Some(f) = &registry_file {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, glow.to_string());
    }
    // Test hook: TRINIDAD_HEAD_DUMP=path writes the visible screen there a few times a second.
    let dump_path = std::env::var_os("TRINIDAD_HEAD_DUMP").map(std::path::PathBuf::from);

    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let class = w!("TrinidadHeadWindow");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            // Icon resource 1, embedded by build.rs.
            hIcon: windows::Win32::UI::WindowsAndMessaging::LoadIconW(Some(instance.into()), PCWSTR(1 as *const u16))
                .unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let title = wide(APP_NAME);
        // No redirection bitmap: DirectComposition supplies every pixel, transparent ones included.
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            class,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1080,
            700,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create window");
        // Our shape is drawn, not clipped: Windows must not round or outline it.
        let pref = DWMWCP_DONOTROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        // Windows 11 outlines every window with a thin line; ours has its own rim.
        let none = DWMWA_COLOR_NONE;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &none as *const _ as *const std::ffi::c_void,
            std::mem::size_of_val(&none) as u32,
        );
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER);

        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("dwrite");
        let font_family = pick_font(&dwrite, &["Cascadia Mono", "Cascadia Code", "Consolas", "Courier New"]);
        let icon_family = pick_font(&dwrite, &["Segoe Fluent Icons", "Segoe MDL2 Assets"]);

        let pty = match Pty::spawn(&command, None, 80, 24) {
            Ok(p) => p,
            Err(e) => {
                let msg = wide(&format!("Could not start the shell:\n{command}\n\n{e}"));
                windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    Some(hwnd),
                    PCWSTR(msg.as_ptr()),
                    w!("Trinidad Head"),
                    Default::default(),
                );
                return;
            }
        };

        let mut app = App {
            hwnd,
            shared: Arc::new(Mutex::new(Shared { term: Terminal::new(80, 24), last_output: None })),
            pty: Arc::new(Mutex::new(pty)),
            gfx: None,
            brush: None,
            glow_cache: None,
            dwrite,
            formats: Vec::new(),
            icon_format: None,
            icon_small: None,
            font_family,
            icon_family,
            cell_w: 8.0,
            cell_h: 16.0,
            layout: Layout::new(1080.0, 700.0, 1.0, false),
            glow,
            registry_file: registry_file.clone(),
            scroll_offset: 0,
            focused: true,
            hover: None,
            pressed: None,
            tracking_mouse: false,
            high_surrogate: None,
            sel: None,
            user_bar: std::env::var("USERPROFILE")
                .ok()
                .and_then(|h| std::fs::read_to_string(std::path::Path::new(&h).join(".claude/themes/trinidad-head.json")).ok())
                .and_then(|t| theme::user_bar_from_theme(&t))
                .unwrap_or(theme::USER_BAR),
            selecting: false,
            skip_char: false,
            mouse_reported: false,
            pending_press: None,
            autoscroll: false,
            drag_pt: (0.0, 0.0),
            autoscroll_ticks: 0,
            meter: crate::latency::Meter::new(log),
            last_title: Instant::now(),
            started,
            first_frame_logged: false,
            dump_path,
            last_dump: Instant::now(),
        };
        app.make_fonts();
        app.fit_to_window();

        // Shell-reader thread: parse output off the UI thread, then poke the window once.
        let output = app.pty.lock().unwrap().take_output().expect("pty output");
        let shared = app.shared.clone();
        let pty_for_reader = app.pty.clone();
        let hwnd_raw = hwnd.0 as isize;
        std::thread::spawn(move || reader_loop(output, shared, pty_for_reader, hwnd_raw));

        // Files dropped on the window arrive as WM_DROPFILES.
        DragAcceptFiles(hwnd, true);
        APP.with(|a| *a.borrow_mut() = Some(app));
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| a.borrow_mut().take());
    }
    if let Some(f) = &registry_file {
        let _ = std::fs::remove_file(f);
    }
}

/// Glow colors used by other Trinidad Head windows that are still open. Entries left behind
/// by windows that crashed are cleaned up here.
fn live_window_glows(dir: &std::path::Path) -> Vec<usize> {
    use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    const STILL_ACTIVE: u32 = 259;
    let mut used = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return used };
    for e in entries.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let alive = unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) => {
                    let mut code = 0u32;
                    let ok = GetExitCodeProcess(h, &mut code).is_ok() && code == STILL_ACTIVE;
                    let _ = windows::Win32::Foundation::CloseHandle(h);
                    ok
                }
                Err(_) => false,
            }
        };
        if !alive {
            let _ = std::fs::remove_file(e.path());
            continue;
        }
        if let Some(g) = std::fs::read_to_string(e.path()).ok().and_then(|t| t.trim().parse::<usize>().ok()) {
            used.push(g);
        }
    }
    used
}

fn reader_loop(mut output: std::fs::File, shared: Arc<Mutex<Shared>>, pty: Arc<Mutex<Pty>>, hwnd_raw: isize) {
    let hwnd = HWND(hwnd_raw as *mut _);
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
                if !PAINT_QUEUED.swap(true, Ordering::SeqCst) {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_TERM_OUTPUT, WPARAM(0), LPARAM(0));
                    }
                }
            }
        }
    }
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_TERM_EXITED, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // The whole window is ours to draw; Windows keeps no title bar or border.
        WM_NCCALCSIZE if wparam.0 != 0 => {
            if IsZoomed(hwnd).as_bool() {
                // Maximized windows hang over the screen edge by the frame size; pull the content back in.
                let params = &mut *(lparam.0 as *mut NCCALCSIZE_PARAMS);
                let dpi = GetDpiForWindow(hwnd);
                let f = GetSystemMetricsForDpi(SM_CXFRAME, dpi) + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                let r = &mut params.rgrc[0];
                r.left += f;
                r.top += f;
                r.right -= f;
                r.bottom -= f;
            }
            return LRESULT(0);
        }
        WM_NCACTIVATE => return LRESULT(1),
        WM_NCHITTEST => return LRESULT(hit_test(hwnd, lparam) as isize),
        _ => {}
    }
    let handled = APP.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else {
            // We're inside one of our own handlers (a menu's or a drop's message loop). Shell
            // output can't be drawn now; note it so the outer handler asks again when done,
            // otherwise the window stops repainting new output.
            if msg == WM_TERM_OUTPUT {
                MISSED_OUTPUT.store(true, Ordering::SeqCst);
                return Some(LRESULT(0));
            }
            return None;
        };
        let app = guard.as_mut()?;
        let r = app.handle(msg, wparam, lparam);
        if MISSED_OUTPUT.swap(false, Ordering::SeqCst) {
            PAINT_QUEUED.store(true, Ordering::SeqCst);
            let _ = PostMessageW(Some(hwnd), WM_TERM_OUTPUT, WPARAM(0), LPARAM(0));
        }
        r
    });
    match handled {
        Some(r) => r,
        None => {
            if msg == WM_DESTROY {
                PostQuitMessage(0);
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

impl App {
    /// Returns None to fall through to the default window procedure.
    fn handle(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        unsafe {
            match msg {
                WM_TERM_OUTPUT => {
                    PAINT_QUEUED.store(false, Ordering::SeqCst);
                    // A program copied something (Claude's fullscreen view does this on select).
                    let clip = self.shared.lock().unwrap().term.take_clipboard();
                    if let Some(text) = clip {
                        set_clipboard_text(self.hwnd, &text);
                    }
                    self.render();
                    Some(LRESULT(0))
                }
                WM_TERM_EXITED => {
                    let _ = DestroyWindow(self.hwnd);
                    Some(LRESULT(0))
                }
                WM_PAINT => {
                    let mut ps = PAINTSTRUCT::default();
                    BeginPaint(self.hwnd, &mut ps);
                    self.render();
                    let _ = EndPaint(self.hwnd, &ps);
                    Some(LRESULT(0))
                }
                WM_SIZE => {
                    self.fit_to_window();
                    Some(LRESULT(0))
                }
                WM_DPICHANGED => {
                    let r = &*(lparam.0 as *const RECT);
                    let _ = SetWindowPos(
                        self.hwnd,
                        None,
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                    );
                    self.make_fonts();
                    self.fit_to_window();
                    Some(LRESULT(0))
                }
                WM_SETFOCUS | WM_KILLFOCUS => {
                    self.focused = msg == WM_SETFOCUS;
                    if self.shared.lock().unwrap().term.focus_events {
                        let _ = self.pty.lock().unwrap().write(if self.focused { b"\x1b[I" } else { b"\x1b[O" });
                    }
                    self.render();
                    Some(LRESULT(0))
                }
                WM_CHAR | WM_SYSCHAR => {
                    if std::mem::take(&mut self.skip_char) {
                        return Some(LRESULT(0));
                    }
                    self.on_char(wparam.0 as u16, msg == WM_SYSCHAR);
                    Some(LRESULT(0))
                }
                WM_KEYDOWN | WM_SYSKEYDOWN => {
                    if self.on_key(VIRTUAL_KEY(wparam.0 as u16)) {
                        Some(LRESULT(0))
                    } else {
                        None // let TranslateMessage produce WM_CHAR, and Alt+F4 still close
                    }
                }
                WM_MOUSEWHEEL => {
                    let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
                    if self.mouse_mode() > 0 {
                        // Wheel events go to the program as buttons 64 (up) / 65 (down).
                        let mut pt = POINT { x: (lparam.0 & 0xFFFF) as u16 as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32 };
                        let _ = ScreenToClient(self.hwnd, &mut pt);
                        let notches = (delta / WHEEL_DELTA as i32).unsigned_abs().max(1);
                        for _ in 0..notches {
                            self.report_mouse(if delta > 0 { 64 } else { 65 }, pt.x as f32, pt.y as f32, true);
                        }
                        return Some(LRESULT(0));
                    }
                    let lines = (delta / WHEEL_DELTA as i32) * 3;
                    let max = self.shared.lock().unwrap().term.scrollback_len() as i32;
                    self.scroll_offset = (self.scroll_offset as i32 + lines).clamp(0, max) as usize;
                    self.render();
                    Some(LRESULT(0))
                }
                WM_RBUTTONUP => {
                    let (x, y) = xy(lparam);
                    self.context_menu(x, y);
                    Some(LRESULT(0))
                }
                WM_DROPFILES => {
                    self.drop_files(HDROP(wparam.0 as *mut _));
                    Some(LRESULT(0))
                }
                WM_TIMER if wparam.0 == AUTOSCROLL_TIMER => {
                    self.autoscroll_step();
                    Some(LRESULT(0))
                }
                WM_MOUSEMOVE => {
                    if !self.tracking_mouse {
                        let mut tme = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: self.hwnd,
                            dwHoverTime: 0,
                        };
                        self.tracking_mouse = TrackMouseEvent(&mut tme).is_ok();
                    }
                    let (x, y) = xy(lparam);
                    if let Some((cell, p)) = self.pending_press {
                        // Moved off the pressed cell: a drag, so select text here.
                        if self.screen_cell(x, y) != cell {
                            self.pending_press = None;
                            self.sel = Some((p, self.cell_at(x, y)));
                            self.selecting = true;
                            self.drag_pt = (x, y);
                            self.update_autoscroll();
                            self.render();
                        }
                        return Some(LRESULT(0));
                    }
                    if self.mouse_reported {
                        if self.mouse_mode() >= 1002 {
                            self.report_mouse(32, x, y, true);
                        }
                        return Some(LRESULT(0));
                    }
                    if self.mouse_mode() == 1003 && self.layout.text.contains(x, y) {
                        self.report_mouse(35, x, y, true);
                    }
                    if self.selecting {
                        let p = self.cell_at(x, y);
                        self.drag_pt = (x, y);
                        if let Some(sel) = self.sel.as_mut() {
                            sel.1 = p;
                        }
                        // Dragged past the top or bottom edge: keep scrolling until the
                        // button comes up.
                        self.update_autoscroll();
                        self.render();
                        return Some(LRESULT(0));
                    }
                    let hover = self.layout.button_at(x, y);
                    if hover != self.hover {
                        self.hover = hover;
                        self.render();
                    }
                    Some(LRESULT(0))
                }
                WM_MOUSELEAVE => {
                    self.tracking_mouse = false;
                    if self.hover.is_some() {
                        self.hover = None;
                        self.render();
                    }
                    Some(LRESULT(0))
                }
                WM_LBUTTONDOWN => {
                    let (x, y) = xy(lparam);
                    self.pressed = self.layout.button_at(x, y);
                    const MK_CONTROL: usize = 0x0008;
                    let ctrl = wparam.0 & MK_CONTROL != 0;
                    let link = if ctrl && self.pressed.is_none() { self.link_at(x, y) } else { None };
                    if self.pressed.is_some() {
                        SetCapture(self.hwnd);
                        self.render();
                    } else if let Some(link) = link {
                        // Ctrl+click opens a link.
                        self.open_link(&link);
                    } else if self.mouse_mode() > 0 && self.layout.text.contains(x, y) {
                        // The program handles clicks (Claude's fullscreen view), but a drag still
                        // selects text here. Hold the press until we know which it is.
                        self.pending_press = Some((self.screen_cell(x, y), self.cell_at(x, y)));
                        self.sel = None;
                        SetCapture(self.hwnd);
                        self.render();
                    } else if self.layout.in_body(x, y) {
                        // Start selecting text.
                        let p = self.cell_at(x, y);
                        self.sel = Some((p, p));
                        self.selecting = true;
                        self.drag_pt = (x, y);
                        SetCapture(self.hwnd);
                        self.render();
                    }
                    Some(LRESULT(0))
                }
                WM_LBUTTONUP => {
                    let (x, y) = xy(lparam);
                    self.stop_autoscroll();
                    if let Some((cell, _)) = self.pending_press.take() {
                        // It never moved off its cell: a plain click, which the program gets.
                        let _ = ReleaseCapture();
                        self.report_mouse_cell(0, cell, true);
                        self.report_mouse_cell(0, cell, false);
                        return Some(LRESULT(0));
                    }
                    if self.mouse_reported {
                        self.mouse_reported = false;
                        let _ = ReleaseCapture();
                        self.report_mouse(0, x, y, false);
                        return Some(LRESULT(0));
                    }
                    if self.selecting {
                        self.selecting = false;
                        let _ = ReleaseCapture();
                        // Nothing is copied until Copy is picked (right-click menu or Ctrl+C).
                        if matches!(self.sel, Some((a, b)) if a == b) {
                            self.sel = None;
                        }
                        self.render();
                        return Some(LRESULT(0));
                    }
                    if let Some(b) = self.pressed.take() {
                        let _ = ReleaseCapture();
                        if self.layout.button_at(x, y) == Some(b) {
                            self.press(b);
                        }
                        self.render();
                    }
                    Some(LRESULT(0))
                }
                _ => None,
            }
        }
    }

    fn press(&mut self, b: Button) {
        unsafe {
            match b {
                Button::Close => {
                    let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
                Button::Minimize => {
                    let _ = ShowWindow(self.hwnd, SW_MINIMIZE);
                }
                Button::Zoom => {
                    let cmd = if IsZoomed(self.hwnd).as_bool() { SW_RESTORE } else { SW_MAXIMIZE };
                    let _ = ShowWindow(self.hwnd, cmd);
                }
                Button::Terminal => {}
                Button::Folder => {
                    let home = std::env::var("USERPROFILE").unwrap_or_else(|_| "C:\\".into());
                    let _ = std::process::Command::new("explorer.exe").arg(home).spawn();
                }
                Button::Glow => {
                    // Changes this window only; other windows and the default stay as they are.
                    self.glow = (self.glow + 1) % GLOWS.len();
                    self.glow_cache = None;
                    if let Some(p) = &self.registry_file {
                        let _ = std::fs::write(p, self.glow.to_string());
                    }
                }
            }
        }
    }

    /// Lines to scroll per tick while a drag is held past an edge of the text area:
    /// none while the pointer is inside it, then faster the further out it goes.
    /// Positive runs back up into history, negative comes forward toward the live screen.
    fn autoscroll_lines(&self, y: f32) -> i64 {
        let t = self.layout.text;
        let over = if y < t.t {
            t.t - y
        } else if y >= t.b {
            -(y - t.b + 1.0)
        } else {
            return 0;
        };
        let speed = (1.0 + over.abs() / self.cell_h.max(1.0)).min(8.0) as i64;
        if over > 0.0 {
            speed
        } else {
            -speed
        }
    }

    /// Start or stop the drag-autoscroll for wherever the pointer is now.
    fn update_autoscroll(&mut self) {
        let want = self.selecting && self.autoscroll_lines(self.drag_pt.1) != 0;
        if want == self.autoscroll {
            return;
        }
        if want {
            unsafe { SetTimer(Some(self.hwnd), AUTOSCROLL_TIMER, 50, None) };
            self.autoscroll = true;
        } else {
            self.stop_autoscroll();
        }
    }

    fn stop_autoscroll(&mut self) {
        if self.autoscroll {
            let _ = unsafe { KillTimer(Some(self.hwnd), AUTOSCROLL_TIMER) };
            self.autoscroll = false;
        }
    }

    /// One tick of a held drag: move the view a few lines, then stretch the selection to the
    /// pointer. The end point is clamped to the visible grid, so scrolling is what lets the
    /// selection reach text that was never on screen.
    fn autoscroll_step(&mut self) {
        self.autoscroll_ticks += 1;
        let (x, y) = self.drag_pt;
        let lines = self.autoscroll_lines(y);
        if !self.selecting || lines == 0 {
            self.stop_autoscroll();
            return;
        }
        let max = self.shared.lock().unwrap().term.scrollback_len() as i64;
        let before = self.scroll_offset;
        self.scroll_offset = (before as i64 + lines).clamp(0, max) as usize;
        let p = self.cell_at(x, y);
        let stretched = match self.sel.as_mut() {
            Some(sel) if sel.1 != p => {
                sel.1 = p;
                true
            }
            _ => false,
        };
        if self.scroll_offset != before || stretched {
            self.render();
        }
    }

    /// The (line, column) under a pixel, counting lines from the top of history.
    fn cell_at(&self, x: f32, y: f32) -> (usize, usize) {
        let t = self.layout.text;
        let st = self.shared.lock().unwrap();
        let term = &st.term;
        let offset = self.scroll_offset.min(term.scrollback_len());
        let row = ((y - t.t) / self.cell_h).floor().clamp(0.0, (term.rows() - 1) as f32) as usize;
        let col = ((x - t.l) / self.cell_w).floor().clamp(0.0, (term.cols() - 1) as f32) as usize;
        (term.scrollback_len() - offset + row, col)
    }

    /// The program's mouse mode, unless Shift is held (Shift always means "let me select").
    fn mouse_mode(&self) -> u16 {
        let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
        if shift {
            return 0;
        }
        self.shared.lock().unwrap().term.mouse_tracking
    }

    /// The on-screen (column, row) under a pixel, as mouse reports count them.
    fn screen_cell(&self, x: f32, y: f32) -> (usize, usize) {
        let t = self.layout.text;
        let (cols, rows) = {
            let st = self.shared.lock().unwrap();
            (st.term.cols(), st.term.rows())
        };
        let col = ((x - t.l) / self.cell_w).floor().clamp(0.0, (cols - 1) as f32) as usize;
        let row = ((y - t.t) / self.cell_h).floor().clamp(0.0, (rows - 1) as f32) as usize;
        (col, row)
    }

    fn report_mouse(&mut self, button: u8, x: f32, y: f32, pressed: bool) {
        let cell = self.screen_cell(x, y);
        self.report_mouse_cell(button, cell, pressed);
    }

    fn report_mouse_cell(&mut self, button: u8, (col, row): (usize, usize), pressed: bool) {
        let sgr = self.shared.lock().unwrap().term.mouse_sgr;
        let bytes = if sgr {
            core_vt::sgr_mouse(button, col, row, pressed)
        } else {
            // Legacy X10 encoding: release is button 3, coordinates offset by 32.
            let b = if pressed { button } else { 3 | (button & !3) };
            vec![0x1b, b'[', b'M', 32 + b, (33 + col.min(222)) as u8, (33 + row.min(222)) as u8]
        };
        let _ = self.pty.lock().unwrap().write(&bytes);
    }

    fn copy_selection(&mut self) {
        let Some((a, b)) = self.sel else { return };
        let text = self.shared.lock().unwrap().term.text_between(a, b);
        if !text.is_empty() {
            unsafe { set_clipboard_text(self.hwnd, &text) };
        }
    }

    /// Right-click menu: Open Link / Copy Link over a link, then Copy, Paste, Select All.
    /// Nothing is copied or pasted until one is picked.
    fn context_menu(&mut self, x: f32, y: f32) {
        const OPEN: usize = 1;
        const COPY_LINK: usize = 2;
        const COPY: usize = 3;
        const PASTE: usize = 4;
        const SELECT_ALL: usize = 5;
        let link = self.link_at(x, y);
        let has_sel = matches!(self.sel, Some((a, b)) if a != b);
        let has_clip = unsafe { IsClipboardFormatAvailable(CF_UNICODETEXT).is_ok() };
        let mut items: Vec<(usize, &str, bool)> = Vec::new();
        if link.is_some() {
            items.push((OPEN, "Open Link", true));
            items.push((COPY_LINK, "Copy Link", true));
            items.push((0, "", false));
        }
        items.push((COPY, "Copy", has_sel));
        items.push((PASTE, "Paste", has_clip));
        items.push((SELECT_ALL, "Select All", true));
        // Test hook: the menu's items, next to the screen dump.
        if let Some(p) = &self.dump_path {
            let list: Vec<String> = items.iter().map(|(_, t, on)| format!("{t}={}", *on as u8)).collect();
            let _ = std::fs::write(p.with_extension("menu"), list.join("\n"));
        }
        let cmd = unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            for (id, title, enabled) in &items {
                if *id == 0 {
                    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
                    continue;
                }
                let t = wide(title);
                let flags = if *enabled { MF_STRING } else { MF_STRING | MF_GRAYED };
                let _ = AppendMenuW(menu, flags, *id, PCWSTR(t.as_ptr()));
            }
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            // The menu runs its own message loop; our handler is busy until it returns, so
            // output that arrives meanwhile is picked up below.
            let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, self.hwnd, None).0 as usize;
            let _ = DestroyMenu(menu);
            cmd
        };
        match cmd {
            OPEN => {
                if let Some(l) = &link {
                    self.open_link(l);
                }
            }
            COPY_LINK => {
                if let Some(l) = &link {
                    unsafe { set_clipboard_text(self.hwnd, l) };
                }
            }
            COPY => {
                self.copy_selection();
                self.sel = None;
            }
            PASTE => self.paste(),
            SELECT_ALL => {
                let st = self.shared.lock().unwrap();
                let (total, cols) = (st.term.total_lines(), st.term.cols());
                drop(st);
                self.sel = Some(((0, 0), (total.saturating_sub(1), cols.saturating_sub(1))));
            }
            _ => {}
        }
        self.render();
    }

    /// The web link under a pixel in the text area, following it across wrapped rows.
    fn link_at(&self, x: f32, y: f32) -> Option<String> {
        if !self.layout.text.contains(x, y) {
            return None;
        }
        let (line_i, col) = self.cell_at(x, y);
        let st = self.shared.lock().unwrap();
        let term = &st.term;
        let chars = |i: usize| -> Vec<char> {
            term.abs_line(i).iter().map(|c| if c.spacer { '\0' } else { c.ch }).collect()
        };
        let joins = |a: &[char], b: &[char]| {
            a.len() >= term.cols()
                && a.last().is_some_and(|c| !c.is_whitespace() && *c != '\0')
                && b.first().is_some_and(|c| !c.is_whitespace() && *c != '\0')
        };
        let mut first = line_i;
        while first > 0 && line_i - first < 4 && joins(&chars(first - 1), &chars(first)) {
            first -= 1;
        }
        let mut last = line_i;
        while last + 1 < term.total_lines() && last - line_i < 4 && joins(&chars(last), &chars(last + 1)) {
            last += 1;
        }
        let mut all = Vec::new();
        let mut at = col;
        for i in first..=last {
            let c = chars(i);
            if i < line_i {
                at += c.len();
            }
            all.extend(c);
        }
        crate::textutil::url_at(&all, at)
    }

    fn open_link(&self, link: &str) {
        if let Some(p) = &self.dump_path {
            let _ = std::fs::write(p.with_extension("link"), link);
        }
        if std::env::var_os("TRINIDAD_HEAD_SELFTEST").is_some() {
            return;
        }
        let url = wide(link);
        unsafe {
            ShellExecuteW(Some(self.hwnd), w!("open"), PCWSTR(url.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
        }
    }

    /// Files dropped on the window are typed in as paths (quoted when they hold spaces, like
    /// Windows Terminal), which Claude Code turns into attached images.
    fn drop_files(&mut self, drop: HDROP) {
        let mut paths = Vec::new();
        unsafe {
            let n = DragQueryFileW(drop, u32::MAX, None);
            for i in 0..n {
                let len = DragQueryFileW(drop, i, None) as usize;
                let mut buf = vec![0u16; len + 1];
                let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
                let path = String::from_utf16_lossy(&buf[..got]);
                paths.push(if path.contains(' ') { format!("\"{path}\"") } else { path });
            }
            DragFinish(drop);
        }
        if paths.is_empty() {
            return;
        }
        self.paste_text(&(paths.join(" ") + " "));
        unsafe {
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.sel = None;
        self.meter.key(Instant::now());
        self.scroll_offset = 0;
        let _ = self.pty.lock().unwrap().write(bytes);
    }

    fn on_char(&mut self, unit: u16, alt: bool) {
        let ch = if (0xD800..0xDC00).contains(&unit) {
            self.high_surrogate = Some(unit);
            return;
        } else if (0xDC00..0xE000).contains(&unit) {
            let Some(high) = self.high_surrogate.take() else { return };
            char::decode_utf16([high, unit]).next().and_then(|r| r.ok())
        } else {
            char::from_u32(unit as u32)
        };
        let Some(ch) = ch else { return };
        let mut out = Vec::with_capacity(8);
        if alt {
            out.push(0x1b);
        }
        match ch {
            '\u{8}' => out.push(0x7f),  // Backspace
            '\u{7f}' => out.push(0x08), // Ctrl+Backspace
            _ => {
                let mut b = [0u8; 4];
                out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
            }
        }
        self.send(&out);
    }

    /// Keys that don't produce characters. Returns true if handled.
    fn on_key(&mut self, vk: VIRTUAL_KEY) -> bool {
        let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
        let (shift, alt, ctrl) = (down(VK_SHIFT), down(VK_MENU), down(VK_CONTROL));
        let rows = self.shared.lock().unwrap().term.rows();

        if shift && (vk == VK_PRIOR || vk == VK_NEXT) {
            let max = self.shared.lock().unwrap().term.scrollback_len();
            self.scroll_offset = if vk == VK_PRIOR {
                (self.scroll_offset + rows.saturating_sub(1)).min(max)
            } else {
                self.scroll_offset.saturating_sub(rows.saturating_sub(1))
            };
            self.render();
            return true;
        }
        // Ctrl+C copies when something is selected, otherwise it goes to the shell as usual.
        if ctrl && vk.0 == b'C' as u16 && (shift || self.sel.is_some()) {
            self.copy_selection();
            self.sel = None;
            self.skip_char = true;
            self.render();
            return true;
        }
        if ctrl && shift && vk.0 == b'V' as u16 {
            self.skip_char = true;
            self.paste();
            return true;
        }
        if ctrl && vk == VK_SPACE {
            self.send(&[0]);
            return true;
        }

        let app_keys = self.shared.lock().unwrap().term.app_cursor_keys;
        let m = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
        let cursor = |c: char| -> Vec<u8> {
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else if app_keys {
                format!("\x1bO{c}").into_bytes()
            } else {
                format!("\x1b[{c}").into_bytes()
            }
        };
        let tilde = |n: u8| -> Vec<u8> {
            if m > 1 {
                format!("\x1b[{n};{m}~").into_bytes()
            } else {
                format!("\x1b[{n}~").into_bytes()
            }
        };
        let ss3 = |c: char| -> Vec<u8> {
            if m > 1 {
                format!("\x1b[1;{m}{c}").into_bytes()
            } else {
                format!("\x1bO{c}").into_bytes()
            }
        };
        let seq = match vk {
            VK_UP => cursor('A'),
            VK_DOWN => cursor('B'),
            VK_RIGHT => cursor('C'),
            VK_LEFT => cursor('D'),
            VK_HOME => cursor('H'),
            VK_END => cursor('F'),
            VK_INSERT => tilde(2),
            VK_DELETE => tilde(3),
            VK_PRIOR => tilde(5),
            VK_NEXT => tilde(6),
            VK_F1 => ss3('P'),
            VK_F2 => ss3('Q'),
            VK_F3 => ss3('R'),
            VK_F4 if !alt => ss3('S'),
            VK_F5 => tilde(15),
            VK_F6 => tilde(17),
            VK_F7 => tilde(18),
            VK_F8 => tilde(19),
            VK_F9 => tilde(20),
            VK_F10 => tilde(21),
            VK_F11 => tilde(23),
            VK_F12 => tilde(24),
            _ => return false,
        };
        self.send(&seq);
        true
    }

    fn paste(&mut self) {
        let text = unsafe { clipboard_text() };
        let Some(text) = text else { return };
        self.paste_text(&text);
    }

    fn paste_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bracketed = self.shared.lock().unwrap().term.bracketed_paste;
        let mut out = Vec::with_capacity(text.len() + 12);
        if bracketed {
            out.extend_from_slice(b"\x1b[200~");
        }
        out.extend_from_slice(text.as_bytes());
        if bracketed {
            out.extend_from_slice(b"\x1b[201~");
        }
        self.send(&out);
    }

    fn dpi_scale(&self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd) as f32 / 96.0 }
    }

    fn make_fonts(&mut self) {
        let s = self.dpi_scale();
        let family = PCWSTR(self.font_family.as_ptr());
        let mut formats = Vec::new();
        unsafe {
            for (weight, style) in [
                (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL),
                (DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_NORMAL),
                (DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_ITALIC),
                (DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_STYLE_ITALIC),
            ] {
                let f = self
                    .dwrite
                    .CreateTextFormat(family, None, weight, style, DWRITE_FONT_STRETCH_NORMAL, FONT_DIP * s, w!("en-us"))
                    .expect("text format");
                let _ = f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                formats.push(f);
            }
            // Cell size comes from the font's real advance, so runs of text line up with the grid.
            let probe: Vec<u16> = "MMMMMMMMMM".encode_utf16().collect();
            if let Ok(layout) = self.dwrite.CreateTextLayout(&probe, &formats[0], 10000.0, 10000.0) {
                let mut m = DWRITE_TEXT_METRICS::default();
                if layout.GetMetrics(&mut m).is_ok() {
                    self.cell_w = m.widthIncludingTrailingWhitespace / 10.0;
                    self.cell_h = (m.height * 1.08).ceil();
                }
            }
            let icon = |size: f32| {
                let f = self
                    .dwrite
                    .CreateTextFormat(
                        PCWSTR(self.icon_family.as_ptr()),
                        None,
                        DWRITE_FONT_WEIGHT_NORMAL,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size * s,
                        w!("en-us"),
                    )
                    .ok()?;
                let _ = f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                Some(f)
            };
            self.icon_format = icon(15.0);
            self.icon_small = icon(10.0);
        }
        self.formats = formats;
    }

    fn client_size(&self) -> (u32, u32) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut r);
        }
        ((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32)
    }

    fn fit_to_window(&mut self) {
        let (w, h) = self.client_size();
        let maximized = unsafe { IsZoomed(self.hwnd).as_bool() };
        self.layout = Layout::new(w as f32, h as f32, self.dpi_scale(), maximized);
        let text = self.layout.text;
        let cols = (text.w() / self.cell_w).floor().max(2.0) as usize;
        let rows = (text.h() / self.cell_h).floor().max(1.0) as usize;
        {
            let mut s = self.shared.lock().unwrap();
            if s.term.cols() != cols || s.term.rows() != rows {
                s.term.resize(cols, rows);
                let _ = self.pty.lock().unwrap().resize(cols as u16, rows as u16);
            }
        }
        if let Some(g) = self.gfx.as_mut() {
            if g.resize(w, h).is_err() {
                self.drop_gpu();
            }
        }
        self.render();
    }

    fn drop_gpu(&mut self) {
        self.gfx = None;
        self.brush = None;
        self.glow_cache = None;
    }

    fn ensure_gpu(&mut self) -> bool {
        if self.gfx.is_none() {
            let (w, h) = self.client_size();
            match Gfx::new(self.hwnd, w, h) {
                Ok(g) => self.gfx = Some(g),
                Err(_) => return false,
            }
        }
        let g = self.gfx.as_mut().unwrap();
        if g.bind().is_err() {
            self.drop_gpu();
            return false;
        }
        if self.brush.is_none() {
            unsafe {
                self.brush = g.dc.CreateSolidColorBrush(&rgb(theme::TEXT), None).ok();
            }
        }
        self.brush.is_some()
    }

    /// Shape and glow depend only on the window size and color theme, so they're built once and
    /// reused every frame.
    fn chrome(&mut self) -> Option<ChromeCache> {
        let l = self.layout;
        let key = (l.window.r as u32, l.window.b as u32, self.glow, l.maximized);
        if let Some(c) = &self.glow_cache {
            if c.key == key {
                return Some(c.clone());
            }
        }
        let g = self.gfx.as_ref()?;
        let dc = g.dc.clone();
        let factory = g.factory.clone();
        let s = l.scale;
        unsafe {
            // Look 06 is smooth glass, not the old rippled outline.
            let glass = GLASS * s;
            let shape = blob_path(&factory, &l, 0.0, 0.0)?;
            let window = blob_path(&factory, &l, glass, 0.0)?;
            let window_edge = blob_path(&factory, &l, glass - 0.5 * s, 0.0)?;
            let edge = blob_path(&factory, &l, 1.6 * s, 0.0)?;
            let sheen = blob_path(&factory, &l, glass * 0.45, 0.0)?;
            let rim = self.rim_brush(1.0)?;

            // Faint glow: the rim drawn thick plus a soft pool of light under the window, then
            // blurred. Small enough to fade out inside the 26 px margin.
            let list = dc.CreateCommandList().ok()?;
            let old = dc.GetTarget().ok();
            dc.SetTarget(&list);
            dc.BeginDraw();
            if !l.maximized {
                dc.DrawGeometry(&shape, &rim, 7.0 * s, None);
                rim.SetOpacity(0.35);
                dc.FillEllipse(
                    &ellipse_xy(l.body.cx(), l.body.b + 10.0 * s, l.body.w() * 0.36, 5.0 * s),
                    &rim,
                );
                rim.SetOpacity(1.0);
            }
            let _ = dc.EndDraw(None, None);
            let _ = list.Close();
            dc.SetTarget(old.as_ref());

            let blur = dc.CreateEffect(&CLSID_D2D1GaussianBlur).ok()?;
            blur.SetInput(0, &list, true);
            let dev: f32 = 5.0 * s;
            let _ = blur.SetValue(
                D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32,
                D2D1_PROPERTY_TYPE_FLOAT,
                &dev.to_le_bytes(),
            );
            let _ = blur.SetValue(
                D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32,
                D2D1_PROPERTY_TYPE_ENUM,
                &(D2D1_BORDER_MODE_SOFT.0 as u32).to_le_bytes(),
            );
            let cache = ChromeCache {
                key,
                bloom: blur,
                shape: shape.cast().ok()?,
                window: window.cast().ok()?,
                window_edge: window_edge.cast().ok()?,
                edge: edge.cast().ok()?,
                sheen: sheen.cast().ok()?,
                rim,
            };
            self.glow_cache = Some(cache.clone());
            Some(cache)
        }
    }

    /// Look 06's rim: the glow's light color hot at both ends, dimmer across the middle.
    fn rim_brush(&self, strength: f32) -> Option<ID2D1LinearGradientBrush> {
        let glow = GLOWS[self.glow];
        let c = |hex: u32, a: f32| D2D1_COLOR_F { a: strength * a, ..rgb(hex) };
        let [c0, c1, _, c3] = glow.colors;
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: c(c0, 1.0) },
            D2D1_GRADIENT_STOP { position: 0.12, color: c(c1, 1.0) },
            D2D1_GRADIENT_STOP { position: 0.36, color: c(c1, 0.45) },
            D2D1_GRADIENT_STOP { position: 0.64, color: c(c1, 0.45) },
            D2D1_GRADIENT_STOP { position: 0.88, color: c(c3, 1.0) },
            D2D1_GRADIENT_STOP { position: 1.0, color: c(c0, 1.0) },
        ];
        let b = self.layout.body;
        self.linear_brush(&stops, (b.l, 0.0), (b.r, 0.0))
    }

    fn linear_brush(&self, stops: &[D2D1_GRADIENT_STOP], from: (f32, f32), to: (f32, f32)) -> Option<ID2D1LinearGradientBrush> {
        let g = self.gfx.as_ref()?;
        unsafe {
            let coll = g
                .dc
                .CreateGradientStopCollection(
                    stops,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_COLOR_SPACE_SRGB,
                    D2D1_BUFFER_PRECISION_8BPC_UNORM,
                    D2D1_EXTEND_MODE_CLAMP,
                    D2D1_COLOR_INTERPOLATION_MODE_STRAIGHT,
                )
                .ok()?;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: Vector2 { X: from.0, Y: from.1 },
                endPoint: Vector2 { X: to.0, Y: to.1 },
            };
            g.dc.CreateLinearGradientBrush(&props, None, &coll).ok()
        }
    }

    fn render(&mut self) {
        if !self.ensure_gpu() {
            return;
        }
        let chrome = self.chrome();
        let dc = self.gfx.as_ref().unwrap().dc.clone();
        let brush = self.brush.clone().unwrap();
        let l = self.layout;
        let s = l.scale;
        let (cw, ch) = (self.cell_w, self.cell_h);
        let glow = GLOWS[self.glow];

        let shared = self.shared.clone();
        let st = shared.lock().unwrap();
        let term = &st.term;
        let offset = self.scroll_offset.min(term.scrollback_len());
        let default_fg = rgb(theme::TEXT);

        unsafe {
            dc.BeginDraw();
            dc.Clear(Some(&D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 }));

            // Look 06 (Matt, 2026-09-17): thick black glass, a thin neon rim that burns brightest
            // at the left and right ends, a faint glow, and a glossy sheen along the top.
            let strength = if self.focused { 1.0 } else { 0.5 };
            // 1. Faint glow and the light pool under the window.
            if let Some(c) = &chrome {
                if !l.maximized {
                    if let Ok(img) = c.bloom.GetOutput() {
                        let _ = dc.PushLayer(
                            &windows::Win32::Graphics::Direct2D::D2D1_LAYER_PARAMETERS1 {
                                contentBounds: D2D_RECT_F { left: -1e6, top: -1e6, right: 1e6, bottom: 1e6 },
                                opacity: 0.2 + 0.25 * strength,
                                ..Default::default()
                            },
                            None,
                        );
                        dc.DrawImage(&img, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                        dc.PopLayer();
                    }
                }
            }

            if let Some(c) = &chrome {
                let b = l.body;
                let glass = GLASS * s;
                let stop = |p: f32, hex: u32, a: f32| D2D1_GRADIENT_STOP { position: p, color: D2D1_COLOR_F { a, ..rgb(hex) } };
                // 2. The glass: charcoal at the top fading to black.
                let body = [stop(0.0, 0x2A2A31, 0.98), stop(0.22, 0x0E0E12, 0.98), stop(0.8, 0x08080B, 0.98), stop(1.0, 0x16110D, 0.98)];
                if let Some(w) = self.linear_brush(&body, (0.0, b.t), (0.0, b.b)) {
                    let _ = dc.FillGeometry(&c.shape, &w, None);
                }
                // Colored light caught inside the glass at both ends.
                c.rim.SetOpacity(0.22 * strength);
                let _ = dc.DrawGeometry(&c.shape, &c.rim, glass * 2.4, None);
                // The sunken window.
                brush.SetColor(&D2D1_COLOR_F { a: theme::BODY_OPACITY, ..rgb(theme::BODY) });
                let _ = dc.FillGeometry(&c.window, &brush, None);
                let wash = [
                    stop(0.0, theme::TINT_A, 0.2),
                    stop(0.45, theme::TINT_A, 0.0),
                    stop(0.62, theme::TINT_B, 0.0),
                    stop(1.0, theme::TINT_B, 0.18),
                ];
                if let Some(w) = self.linear_brush(&wash, (b.l, b.t), (b.r, b.b)) {
                    let _ = dc.FillGeometry(&c.window, &w, None);
                }
                // Its edge sits in shadow, with a hairline of glass light.
                for (w, a) in [(5.0, 0.55), (2.0, 0.8)] {
                    brush.SetColor(&D2D1_COLOR_F { a, ..rgb(0x000000) });
                    let _ = dc.DrawGeometry(&c.window, &brush, w * s, None);
                }
                brush.SetColor(&D2D1_COLOR_F { a: 0.07, ..rgb(0xFFFFFF) });
                let _ = dc.DrawGeometry(&c.window_edge, &brush, 1.0 * s, None);

                // 3. The neon rim: a soft band and a hot core line, then a pale inner hairline.
                for (w, a) in [(4.5, 0.45), (1.8, 1.0)] {
                    c.rim.SetOpacity(a * strength);
                    let _ = dc.DrawGeometry(&c.shape, &c.rim, w * s, None);
                }
                c.rim.SetOpacity(1.0);
                let pale = [stop(0.0, 0xFFE9D2, 0.0), stop(0.5, 0xFFE9D2, 0.55 * strength), stop(1.0, 0xFFE9D2, 0.0)];
                if let Some(w) = self.linear_brush(&pale, (0.0, b.t), (0.0, b.b)) {
                    let _ = dc.DrawGeometry(&c.edge, &w, 0.8 * s, None);
                }
                // The top of the tube is turned away from the light.
                let shade = [stop(0.0, 0x000000, 0.5), stop(1.0, 0x000000, 0.0)];
                if let Some(w) = self.linear_brush(&shade, (0.0, b.t), (0.0, b.t + b.h() * 0.4)) {
                    let _ = dc.DrawGeometry(&c.shape, &w, 5.0 * s, None);
                }

                // 4. Gloss: a soft sheen across the top of the glass.
                let gloss = [stop(0.0, 0xFFFFFF, 0.16), stop(1.0, 0xFFFFFF, 0.0)];
                if let Some(w) = self.linear_brush(&gloss, (0.0, b.t), (0.0, b.t + l.radius * 0.9)) {
                    let _ = dc.DrawGeometry(&c.sheen, &w, glass * 0.5, None);
                }
            }

            // 5. Glass pills: window buttons, window controls, sidebar.
            let pill = |r: crate::layout::Rect| D2D1_ROUNDED_RECT {
                rect: D2D_RECT_F { left: r.l, top: r.t, right: r.r, bottom: r.b },
                radiusX: r.w().min(r.h()) / 2.0,
                radiusY: r.w().min(r.h()) / 2.0,
            };
            for r in [l.lights_pill, l.sidebar] {
                brush.SetColor(&D2D1_COLOR_F { a: 0.07, ..rgb(0xFFFFFF) });
                dc.FillRoundedRectangle(&pill(r), &brush);
                let edge = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: D2D1_COLOR_F { a: 0.45, ..rgb(0xFFFFFF) } },
                    D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { a: 0.08, ..rgb(0xFFFFFF) } },
                ];
                if let Some(e) = self.linear_brush(&edge, (r.l, r.t), (r.l, r.b)) {
                    dc.DrawRoundedRectangle(&pill(r), &e, 1.0, None);
                }
            }
            // Window buttons, macOS style: red closes, yellow minimizes, green maximizes.
            for (b, color, glyph) in [
                (Button::Close, 0xFF5F57, '\u{E8BB}'),
                (Button::Minimize, 0xFEBC2E, '\u{E921}'),
                (Button::Zoom, 0x28C840, if l.maximized { '\u{E73F}' } else { '\u{E740}' }),
            ] {
                let (x, y, r) = l.button(b);
                let lit = self.focused || self.hover.is_some();
                let mut c = rgb(if lit { color } else { 0x5A5E68 });
                if self.pressed == Some(b) {
                    c = D2D1_COLOR_F { r: c.r * 0.75, g: c.g * 0.75, b: c.b * 0.75, a: 1.0 };
                }
                brush.SetColor(&c);
                dc.FillEllipse(&ellipse(x, y, r), &brush);
                // Like macOS, the symbols appear when the pointer is over any of the three.
                if matches!(self.hover, Some(Button::Close | Button::Minimize | Button::Zoom)) {
                    brush.SetColor(&D2D1_COLOR_F { a: 0.6, ..rgb(0x000000) });
                    self.icon(&dc, &brush, glyph, x, y, true);
                }
            }
            // Sidebar: only the buttons this build shows (the glow color).
            for (b, glyph) in [(Button::Terminal, '\u{E756}'), (Button::Folder, '\u{E8B7}'), (Button::Glow, '\u{E790}')] {
                if !crate::layout::SIDEBAR.contains(&b) {
                    continue;
                }
                let (x, y, r) = l.button(b);
                if b == Button::Terminal || self.hover == Some(b) {
                    let a = if b == Button::Terminal { 0.10 } else { 0.07 };
                    brush.SetColor(&D2D1_COLOR_F { a, ..rgb(0xFFFFFF) });
                    dc.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: D2D_RECT_F { left: x - r, top: y - r, right: x + r, bottom: y + r },
                            radiusX: 9.0 * s,
                            radiusY: 9.0 * s,
                        },
                        &brush,
                    );
                }
                let color = match b {
                    Button::Terminal => rgb(0xFFFFFF),
                    Button::Glow => rgb(glow.accent()),
                    _ => rgb(if self.hover == Some(b) { 0xFFFFFF } else { theme::ICON }),
                };
                brush.SetColor(&color);
                self.icon(&dc, &brush, glyph, x, y, false);
            }

            // Resize grip: three short diagonal strokes, like the corner of a window you can drag.
            if !l.maximized {
                let (gx, gy) = l.grip();
                brush.SetColor(&D2D1_COLOR_F { a: if self.focused { 0.8 } else { 0.45 }, ..rgb(0xE6E9FF) });
                for i in 0..3 {
                    let o = (i as f32 - 1.0) * 6.0 * s;
                    let len = (9.0 - 2.5 * i as f32) * s;
                    // Each stroke runs up and to the right, stacked toward the corner.
                    let (cx, cy) = (gx + o, gy + o);
                    let _ = dc.DrawLine(
                        Vector2 { X: cx - len, Y: cy + len },
                        Vector2 { X: cx + len, Y: cy - len },
                        &brush,
                        2.0 * s,
                        None,
                    );
                }
            }

            // 5. Terminal text.
            let (left, top) = (l.text.l, l.text.t);
            let mut text: Vec<u16> = Vec::with_capacity(term.cols() * 2);
            let sel = self.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
            // Claude's prompt bar: drawn as one rounded pill per block of rows instead of
            // square cell backgrounds (Matt, 2026-09-17).
            let (ur, ug, ub) = self.user_bar;
            let is_bar = |c: Color| matches!(c, Color::Rgb(r, g, b) if theme::is_prompt_bar((r, g, b), (ur, ug, ub)));
            let bar_span = |row: usize| -> Option<(usize, usize)> {
                let line = term.line(row, offset);
                let first = line.iter().position(|c| is_bar(c.attrs.bg))?;
                let last = line.iter().rposition(|c| is_bar(c.attrs.bg))?;
                Some((first, last + 1))
            };
            // Per row inside a pill: (pill text left edge, horizontal scale) for the bigger text.
            let mut row_scale: Vec<Option<(f32, f32)>> = vec![None; term.rows()];
            let text_end = |row: usize, from: usize| -> usize {
                let line = term.line(row, offset);
                line.iter()
                    .enumerate()
                    .rev()
                    .find(|(i, c)| *i >= from && is_bar(c.attrs.bg) && c.ch != ' ' && !c.spacer)
                    .map(|(i, _)| i + 1)
                    .unwrap_or(from)
            };
            let mut row = 0;
            while row < term.rows() {
                let Some(mut span) = bar_span(row) else {
                    row += 1;
                    continue;
                };
                let start_row = row;
                row += 1;
                while row < term.rows() {
                    match bar_span(row) {
                        Some(s) => {
                            span = (span.0.min(s.0), span.1.max(s.1));
                            row += 1;
                        }
                        None => break,
                    }
                }
                let used = (start_row..row).map(|r| text_end(r, span.0)).max().unwrap_or(span.0);
                let room = (span.1 - span.0) as f32 - 1.0;
                let need = (used - span.0).max(1) as f32;
                let sx = theme::USER_TEXT_SCALE.min(room / need).max(1.0);
                for r in start_row..row {
                    row_scale[r] = Some((left + span.0 as f32 * cw, sx));
                }
                let t = top + start_row as f32 * ch;
                let b = top + row as f32 * ch;
                let r = ((b - t) / 2.0).min(16.0 * s);
                let pill = D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: left + span.0 as f32 * cw - 4.0 * s, top: t + 1.0, right: left + span.1 as f32 * cw + 4.0 * s, bottom: b - 1.0 },
                    radiusX: r,
                    radiusY: r,
                };
                brush.SetColor(&D2D1_COLOR_F { a: theme::USER_BAR_ALPHA, ..rgb((ur as u32) << 16 | (ug as u32) << 8 | ub as u32) });
                dc.FillRoundedRectangle(&pill, &brush);
                // A faint glassy edge so the pill still reads as a shape.
                brush.SetColor(&D2D1_COLOR_F { a: 0.35, ..rgb((ur as u32) << 16 | (ug as u32) << 8 | ub as u32) });
                dc.DrawRoundedRectangle(&pill, &brush, 1.0, None);
            }
            for row in 0..term.rows() {
                let line = term.line(row, offset);
                let y = top + row as f32 * ch;
                if let Some((sa, sb)) = sel {
                    let abs = term.scrollback_len() - offset + row;
                    if abs >= sa.0 && abs <= sb.0 {
                        let c0 = if abs == sa.0 { sa.1 } else { 0 };
                        let c1 = if abs == sb.0 { sb.1 + 1 } else { term.cols() };
                        brush.SetColor(&D2D1_COLOR_F { a: 0.35, ..rgb(glow.colors[1]) });
                        dc.FillRectangle(
                            &D2D_RECT_F { left: left + c0 as f32 * cw, top: y, right: left + c1 as f32 * cw, bottom: y + ch },
                            &brush,
                        );
                    }
                }
                let mut col = 0;
                while col < line.len() {
                    let start = col;
                    let attrs = line[col].attrs;
                    let wide = line[col].wide;
                    text.clear();
                    // A run: same attributes, no wide characters inside (those are placed one by one).
                    while col < line.len() && line[col].attrs == attrs && (col == start || !line[col].wide) {
                        let c: &Cell = &line[col];
                        if !c.spacer {
                            let mut b = [0u16; 2];
                            text.extend_from_slice(c.ch.encode_utf16(&mut b));
                        }
                        col += 1;
                        if wide {
                            if col < line.len() && line[col].spacer {
                                col += 1;
                            }
                            break;
                        }
                    }
                    let (mut fg, bg) = colors(&attrs, default_fg);
                    let (ur, ug, ub) = self.user_bar;
                    let user_bar = matches!(attrs.bg, Color::Rgb(r, g, b) if theme::is_prompt_bar((r, g, b), (ur, ug, ub)));
                    let mut run_attrs = attrs;
                    if user_bar {
                        fg = rgb(theme::USER_TEXT);
                        run_attrs.bold = true;
                    }
                    let x0 = left + start as f32 * cw;
                    let x1 = left + col as f32 * cw;
                    if let Some(bg) = bg.filter(|_| !user_bar) {
                        brush.SetColor(&bg);
                        dc.FillRectangle(&D2D_RECT_F { left: x0, top: y, right: x1, bottom: y + ch }, &brush);
                    }
                    if text.iter().any(|&u| u != b' ' as u16) {
                        brush.SetColor(&fg);
                        if user_bar {
                            // One size bigger, grown from the pill's left edge and the row's middle.
                            let (pl, sx) = row_scale[row].unwrap_or((x0, 1.0));
                            let sy = theme::USER_TEXT_SCALE;
                            let cy = y + ch / 2.0;
                            dc.SetTransform(&Matrix3x2 { M11: sx, M12: 0.0, M21: 0.0, M22: sy, M31: pl - sx * pl, M32: cy - sy * cy });
                            self.draw_text(&dc, &brush, &text, &run_attrs, x0, y, x1 + cw);
                            dc.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
                        } else {
                            self.draw_text(&dc, &brush, &text, &attrs, x0, y, x1 + cw);
                        }
                    }
                    if attrs.underline {
                        brush.SetColor(&fg);
                        dc.FillRectangle(&D2D_RECT_F { left: x0, top: y + ch - 2.0, right: x1, bottom: y + ch - 1.0 }, &brush);
                    }
                }
            }

            // Cursor: a block in the glow color when focused, an outline when not.
            if term.cursor_visible && offset == 0 {
                let (cr, cc) = term.cursor();
                let x = left + cc as f32 * cw;
                let y = top + cr as f32 * ch;
                let cell = term.line(cr, 0)[cc];
                let width = if cell.wide { 2.0 * cw } else { cw };
                let rect = D2D_RECT_F { left: x, top: y + 1.0, right: x + width, bottom: y + ch - 1.0 };
                brush.SetColor(&rgb(glow.accent()));
                if self.focused {
                    dc.FillRectangle(&rect, &brush);
                    if cell.ch != ' ' {
                        let mut b = [0u16; 2];
                        let t: Vec<u16> = cell.ch.encode_utf16(&mut b).to_vec();
                        brush.SetColor(&rgb(theme::BODY));
                        self.draw_text(&dc, &brush, &t, &cell.attrs, x, y, x + width + cw);
                    }
                } else {
                    dc.DrawRectangle(&rect, &brush, 1.0, None);
                }
            }

            // Scrolled back: a thin bar on the right shows where we are.
            if offset > 0 {
                let total = (term.scrollback_len() + term.rows()) as f32;
                let t = l.text.t + (term.scrollback_len() - offset) as f32 / total * l.text.h();
                let len = (term.rows() as f32 / total * l.text.h()).max(10.0 * s);
                brush.SetColor(&D2D1_COLOR_F { a: 0.5, ..rgb(glow.accent()) });
                dc.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: l.text.r + 10.0 * s, top: t, right: l.text.r + 13.0 * s, bottom: t + len },
                        radiusX: 1.5 * s,
                        radiusY: 1.5 * s,
                    },
                    &brush,
                );
            }

            let ended = dc.EndDraw(None, None);
            let presented = self.gfx.as_mut().unwrap().present();
            let now = Instant::now();
            if ended.is_err() || presented.is_err() {
                // Device lost (driver reset, remote session change): rebuild next frame.
                drop(st);
                self.drop_gpu();
                return;
            }
            self.meter.frame(st.last_output, now);

            if !self.first_frame_logged && st.last_output.is_some() {
                self.first_frame_logged = true;
                if let Ok(d) = std::env::var("LOCALAPPDATA") {
                    let _ = std::fs::write(
                        std::path::Path::new(&d).join("TrinidadHead").join("startup.log"),
                        format!("first shell output on screen after {:.0} ms\n", (now - self.started).as_secs_f64() * 1000.0),
                    );
                }
            }

            if let Some(path) = &self.dump_path {
                if now - self.last_dump > Duration::from_millis(250) {
                    self.last_dump = now;
                    let mut out = String::new();
                    for r in 0..term.rows() {
                        out.push_str(&term.row_text(r));
                        out.push('\n');
                    }
                    let (cr, cc) = term.cursor();
                    out.push_str(&format!("--\ncursor {cr},{cc} size {}x{} title {:?}\n", term.cols(), term.rows(), term.title));
                    // Background colors on screen, to check what programs actually paint.
                    let mut bgs: Vec<String> = Vec::new();
                    for r in 0..term.rows() {
                        for c in term.line(r, 0) {
                            let name = format!("{:?}", c.attrs.bg);
                            if !bgs.contains(&name) {
                                bgs.push(name);
                            }
                        }
                    }
                    out.push_str(&format!("backgrounds {}\n", bgs.join(" ")));
                    out.push_str(&format!(
                        "scroll {} of {} autoscroll {} ticks, drag {:.0},{:.0}, text {:.0},{:.0},{:.0},{:.0}\n",
                        self.scroll_offset,
                        term.scrollback_len(),
                        self.autoscroll_ticks,
                        self.drag_pt.0,
                        self.drag_pt.1,
                        self.layout.text.l,
                        self.layout.text.t,
                        self.layout.text.r,
                        self.layout.text.b,
                    ));
                    if let Some((p50, p95, n)) = self.meter.stats() {
                        out.push_str(&format!("typing delay median {p50:.1} ms, p95 {p95:.1} ms over {n} keys\n"));
                    }
                    let _ = std::fs::write(path, out);
                }
            }

            if now - self.last_title > Duration::from_millis(500) {
                self.last_title = now;
                let shell_title = if term.title.is_empty() { APP_NAME.to_string() } else { term.title.clone() };
                let title = match self.meter.stats() {
                    Some((p50, p95, _)) => format!("{shell_title}  ·  typing delay {p50:.0} ms (p95 {p95:.0} ms)"),
                    None => shell_title,
                };
                let t = wide(&title);
                let _ = SetWindowTextW(self.hwnd, PCWSTR(t.as_ptr()));
            }
        }
    }

    fn icon(&self, dc: &windows::Win32::Graphics::Direct2D::ID2D1DeviceContext, brush: &ID2D1SolidColorBrush, glyph: char, x: f32, y: f32, small: bool) {
        let Some(f) = (if small { &self.icon_small } else { &self.icon_format }) else { return };
        let mut b = [0u16; 2];
        let t = glyph.encode_utf16(&mut b);
        let r = 20.0 * self.layout.scale;
        unsafe {
            dc.DrawText(
                t,
                f,
                &D2D_RECT_F { left: x - r, top: y - r, right: x + r, bottom: y + r },
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        dc: &windows::Win32::Graphics::Direct2D::ID2D1DeviceContext,
        brush: &ID2D1SolidColorBrush,
        text: &[u16],
        attrs: &Attrs,
        x: f32,
        y: f32,
        right: f32,
    ) {
        let idx = attrs.bold as usize + 2 * attrs.italic as usize;
        let rect = D2D_RECT_F { left: x, top: y, right, bottom: y + self.cell_h };
        unsafe {
            dc.DrawText(
                text,
                &self.formats[idx],
                &rect,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}

fn ellipse_xy(x: f32, y: f32, rx: f32, ry: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: rx, radiusY: ry }
}

/// The window outline: a rounded capsule whose long edges ripple gently, like poured glass.
/// `inset` shrinks it (for inner highlights); `wobble` is the ripple height in pixels.
unsafe fn blob_path(factory: &ID2D1Factory1, l: &Layout, inset: f32, wobble: f32) -> Option<ID2D1PathGeometry1> {
    let path = factory.CreatePathGeometry().ok()?;
    let sink = path.Open().ok()?;
    let pts = crate::layout::outline(l, inset, wobble, 480);
    sink.BeginFigure(Vector2 { X: pts[0].0, Y: pts[0].1 }, D2D1_FIGURE_BEGIN_FILLED);
    for &(x, y) in &pts[1..] {
        sink.AddLine(Vector2 { X: x, Y: y });
    }
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close().ok()?;
    Some(path)
}

fn ellipse(x: f32, y: f32, r: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE { point: Vector2 { X: x, Y: y }, radiusX: r, radiusY: r }
}

fn xy(lparam: LPARAM) -> (f32, f32) {
    ((lparam.0 & 0xFFFF) as u16 as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32)
}

/// Resize edges live in the glow margin; the top strip drags the window.
unsafe fn hit_test(hwnd: HWND, lparam: LPARAM) -> u32 {
    let (sx, sy) = xy(lparam);
    let mut pt = POINT { x: sx as i32, y: sy as i32 };
    let _ = ScreenToClient(hwnd, &mut pt);
    let mut r = RECT::default();
    let _ = GetClientRect(hwnd, &mut r);
    let s = GetDpiForWindow(hwnd) as f32 / 96.0;
    let layout = Layout::new(r.right as f32, r.bottom as f32, s, IsZoomed(hwnd).as_bool());
    match layout.hit(pt.x as f32, pt.y as f32) {
        Hit::Client => 1,
        Hit::Caption => 2,
        Hit::Left => 10,
        Hit::Right => 11,
        Hit::Top => 12,
        Hit::TopLeft => 13,
        Hit::TopRight => 14,
        Hit::Bottom => 15,
        Hit::BottomLeft => 16,
        Hit::BottomRight => 17,
    }
}

fn pick_font(dwrite: &IDWriteFactory, names: &[&str]) -> Vec<u16> {
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut coll, false).is_ok() {
            if let Some(coll) = coll {
                for name in names {
                    let w = wide(name);
                    let mut index = 0u32;
                    let mut exists = windows::core::BOOL(0);
                    if coll.FindFamilyName(PCWSTR(w.as_ptr()), &mut index, &mut exists).is_ok() && exists.as_bool() {
                        return w;
                    }
                }
            }
        }
    }
    wide(names.last().copied().unwrap_or("Consolas"))
}

unsafe fn set_clipboard_text(hwnd: HWND, text: &str) {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::DataExchange::{EmptyClipboard, SetClipboardData};
    use windows::Win32::System::Memory::{GlobalAlloc, GMEM_MOVEABLE};
    let wide: Vec<u16> = text.replace('\n', "\r\n").encode_utf16().chain(Some(0)).collect();
    if OpenClipboard(Some(hwnd)).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    if let Ok(mem) = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2) {
        let p = GlobalLock(mem) as *mut u16;
        if !p.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(mem);
            // On success the clipboard owns the memory.
            let _ = SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0)));
        }
    }
    let _ = CloseClipboard();
}

unsafe fn clipboard_text() -> Option<String> {
    OpenClipboard(None).ok()?;
    let result = (|| {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let hg = HGLOBAL(h.0);
        let p = GlobalLock(hg) as *const u16;
        if p.is_null() {
            return None;
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, len));
        let _ = GlobalUnlock(hg);
        Some(s)
    })();
    let _ = CloseClipboard();
    result
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn rgb(hex: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a: 1.0,
    }
}

/// A palette tuned for the dark glass: brighter blues and greens than Windows' defaults.
const PALETTE: [u32; 16] = [
    0x1B1E26, 0xFF5C57, 0x5AF78E, 0xF3F99D, 0x57C7FF, 0xFF6AC1, 0x9AEDFE, 0xD7DBE3, 0x686F7D, 0xFF7A75, 0x7CFFA6,
    0xFFFFB0, 0x7FD3FF, 0xFF8AD0, 0xB8F4FF, 0xFFFFFF,
];

fn indexed(i: u8) -> D2D1_COLOR_F {
    match i {
        0..=15 => rgb(PALETTE[i as usize]),
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            rgb(level(i / 36) << 16 | level((i / 6) % 6) << 8 | level(i % 6))
        }
        _ => {
            let v = 8 + (i as u32 - 232) * 10;
            rgb(v << 16 | v << 8 | v)
        }
    }
}

/// Foreground, and a background only when the cell has one (default background is the glass).
fn colors(a: &Attrs, default_fg: D2D1_COLOR_F) -> (D2D1_COLOR_F, Option<D2D1_COLOR_F>) {
    let resolve = |c: Color, bright: bool| match c {
        Color::Default => None,
        Color::Indexed(i) if bright && i < 8 => Some(indexed(i + 8)),
        Color::Indexed(i) => Some(indexed(i)),
        Color::Rgb(r, g, b) => Some(rgb((r as u32) << 16 | (g as u32) << 8 | b as u32)),
    };
    let fg = resolve(a.fg, a.bold);
    let bg = resolve(a.bg, false);
    if a.inverse {
        (bg.unwrap_or(rgb(theme::BODY)), Some(fg.unwrap_or(default_fg)))
    } else {
        (faint(fg.unwrap_or(default_fg), a.dim), bg)
    }
}

/// SGR 2 text is drawn at a third of its brightness, so Claude Code's ghost
/// suggestion reads as a hint instead of competing with what Matt typed.
fn faint(c: D2D1_COLOR_F, dim: bool) -> D2D1_COLOR_F {
    if !dim {
        return c;
    }
    const DIM: f32 = 1.0 / 3.0;
    D2D1_COLOR_F { r: c.r * DIM, g: c.g * DIM, b: c.b * DIM, a: c.a }
}
