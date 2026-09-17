use std::cell::RefCell;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use core_vt::{Attrs, Cell, Color, Terminal};
use pty::Pty;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_IMMEDIATELY, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE,
    D2D1_FEATURE_LEVEL_DEFAULT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED,
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3,
    VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_HOME, VK_INSERT, VK_LEFT, VK_MENU, VK_NEXT, VK_PRIOR, VK_RIGHT,
    VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW, LoadCursorW,
    PostMessageW, PostQuitMessage, RegisterClassW, SetWindowTextW, TranslateMessage, CS_HREDRAW, CS_VREDRAW,
    CW_USEDEFAULT, IDC_IBEAM, MSG, WHEEL_DELTA, WINDOW_EX_STYLE, WM_APP, WM_CHAR, WM_DESTROY, WM_DPICHANGED,
    WM_KEYDOWN, WM_KILLFOCUS, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONUP, WM_SETFOCUS, WM_SIZE, WM_SYSCHAR,
    WM_SYSKEYDOWN, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const WM_TERM_OUTPUT: u32 = WM_APP + 1;
const WM_TERM_EXITED: u32 = WM_APP + 2;
const CF_UNICODETEXT: u32 = 13;
const PAD: f32 = 6.0;
const FONT_PT: f32 = 11.0;
const APP_NAME: &str = "Our Terminal";

/// State shared between the window thread and the shell-reader thread.
struct Shared {
    term: Terminal,
    last_output: Option<Instant>,
}

struct App {
    hwnd: HWND,
    shared: Arc<Mutex<Shared>>,
    pty: Arc<Mutex<Pty>>,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    target: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    formats: Vec<IDWriteTextFormat>, // regular, bold, italic, bold-italic
    font_family: Vec<u16>,
    cell_w: f32,
    cell_h: f32,
    baseline_fix: f32,
    scroll_offset: usize,
    focused: bool,
    high_surrogate: Option<u16>,
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

pub fn run() {
    let started = Instant::now();
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = if args.is_empty() { pty::default_shell() } else { args.join(" ") };
    let data_dir = std::env::var("LOCALAPPDATA")
        .map(|d| std::path::PathBuf::from(d).join("OurTerminal"))
        .ok();
    if let Some(d) = &data_dir {
        let _ = std::fs::create_dir_all(d);
    }
    let log = data_dir.as_ref().and_then(|d| {
        std::fs::OpenOptions::new().create(true).append(true).open(d.join("latency.log")).ok()
    });
    // Test hook: OUR_TERMINAL_DUMP=path writes the visible screen there a few times a second.
    let dump_path = std::env::var_os("OUR_TERMINAL_DUMP").map(std::path::PathBuf::from);

    unsafe {
        let instance = GetModuleHandleW(None).expect("module handle");
        let class = w!("OurTerminalWindow");
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_IBEAM).unwrap_or_default(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let title = wide(APP_NAME);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1000,
            640,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .expect("create window");

        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).expect("d2d");
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).expect("dwrite");
        let font_family = pick_font(&dwrite);

        let mut app = App {
            hwnd,
            shared: Arc::new(Mutex::new(Shared { term: Terminal::new(80, 24), last_output: None })),
            // Placeholder until the real size is known; replaced just below.
            pty: Arc::new(Mutex::new(match Pty::spawn(&command, None, 80, 24) {
                Ok(p) => p,
                Err(e) => {
                    let msg = wide(&format!("Could not start the shell:\n{command}\n\n{e}"));
                    windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        Some(hwnd),
                        PCWSTR(msg.as_ptr()),
                        w!("Our Terminal"),
                        Default::default(),
                    );
                    return;
                }
            })),
            d2d,
            dwrite,
            target: None,
            brush: None,
            formats: Vec::new(),
            font_family,
            cell_w: 8.0,
            cell_h: 16.0,
            baseline_fix: 0.0,
            scroll_offset: 0,
            focused: true,
            high_surrogate: None,
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

        APP.with(|a| *a.borrow_mut() = Some(app));

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        APP.with(|a| a.borrow_mut().take());
    }
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
    let handled = APP.with(|cell| {
        let Ok(mut guard) = cell.try_borrow_mut() else { return None };
        let app = guard.as_mut()?;
        app.handle(msg, wparam, lparam)
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
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
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
                    self.render();
                    Some(LRESULT(0))
                }
                WM_CHAR | WM_SYSCHAR => {
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
                    let lines = (delta / WHEEL_DELTA as i32) * 3;
                    let max = self.shared.lock().unwrap().term.scrollback_len() as i32;
                    self.scroll_offset = (self.scroll_offset as i32 + lines).clamp(0, max) as usize;
                    self.render();
                    Some(LRESULT(0))
                }
                WM_RBUTTONUP => {
                    self.paste();
                    Some(LRESULT(0))
                }
                _ => None,
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) {
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
            '\u{8}' => out.push(0x7f),   // Backspace
            '\u{7f}' => out.push(0x08),  // Ctrl+Backspace
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
        if ctrl && shift && vk.0 == b'V' as u16 {
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
        let size = FONT_PT * 96.0 / 72.0 * self.dpi_scale();
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
                    .CreateTextFormat(family, None, weight, style, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us"))
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
                    self.cell_h = m.height.ceil();
                    self.baseline_fix = 0.0;
                }
            }
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
        let cols = (((w as f32) - 2.0 * PAD) / self.cell_w).floor().max(2.0) as usize;
        let rows = (((h as f32) - 2.0 * PAD) / self.cell_h).floor().max(1.0) as usize;
        {
            let mut s = self.shared.lock().unwrap();
            if s.term.cols() != cols || s.term.rows() != rows {
                s.term.resize(cols, rows);
                let _ = self.pty.lock().unwrap().resize(cols as u16, rows as u16);
            }
        }
        if let Some(t) = &self.target {
            unsafe {
                if t.Resize(&D2D_SIZE_U { width: w, height: h }).is_err() {
                    self.target = None;
                }
            }
        }
        self.render();
    }

    fn ensure_target(&mut self) -> bool {
        if self.target.is_some() {
            return true;
        }
        let (w, h) = self.client_size();
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.hwnd,
            pixelSize: D2D_SIZE_U { width: w, height: h },
            presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
        };
        unsafe {
            let Ok(t) = self.d2d.CreateHwndRenderTarget(&props, &hprops) else { return false };
            t.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
            let Ok(b) = t.CreateSolidColorBrush(&rgb(0xCCCCCC), None) else { return false };
            self.brush = Some(b);
            self.target = Some(t);
        }
        true
    }

    fn render(&mut self) {
        if !self.ensure_target() {
            return;
        }
        let target = self.target.clone().unwrap();
        let brush = self.brush.clone().unwrap();
        let (cw, ch) = (self.cell_w, self.cell_h);

        let shared = self.shared.clone();
        let s = shared.lock().unwrap();
        let term = &s.term;
        let offset = self.scroll_offset.min(term.scrollback_len());
        let default_bg = rgb(0x0C0C0C);
        let default_fg = rgb(0xCCCCCC);

        unsafe {
            target.BeginDraw();
            target.Clear(Some(&default_bg));

            let mut text: Vec<u16> = Vec::with_capacity(term.cols() * 2);
            for row in 0..term.rows() {
                let line = term.line(row, offset);
                let y = PAD + row as f32 * ch;
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
                    let (fg, bg) = colors(&attrs, default_fg, default_bg);
                    let x0 = PAD + start as f32 * cw;
                    let x1 = PAD + col as f32 * cw;
                    if bg != default_bg {
                        brush.SetColor(&bg);
                        target.FillRectangle(&D2D_RECT_F { left: x0, top: y, right: x1, bottom: y + ch }, &brush);
                    }
                    if text.iter().any(|&u| u != b' ' as u16) {
                        brush.SetColor(&fg);
                        self.draw_text(&target, &brush, &text, &attrs, x0, y, x1 + cw);
                    }
                    if attrs.underline {
                        brush.SetColor(&fg);
                        target.FillRectangle(
                            &D2D_RECT_F { left: x0, top: y + ch - 2.0, right: x1, bottom: y + ch - 1.0 },
                            &brush,
                        );
                    }
                }
            }

            // Cursor: solid block when focused, outline when not.
            if term.cursor_visible && offset == 0 {
                let (cr, cc) = term.cursor();
                let x = PAD + cc as f32 * cw;
                let y = PAD + cr as f32 * ch;
                let cell = term.line(cr, 0)[cc];
                let width = if cell.wide { 2.0 * cw } else { cw };
                brush.SetColor(&rgb(0xFFFFFF));
                let rect = D2D_RECT_F { left: x, top: y, right: x + width, bottom: y + ch };
                if self.focused {
                    target.FillRectangle(&rect, &brush);
                    if cell.ch != ' ' {
                        let mut b = [0u16; 2];
                        let t: Vec<u16> = cell.ch.encode_utf16(&mut b).to_vec();
                        brush.SetColor(&default_bg);
                        self.draw_text(&target, &brush, &t, &cell.attrs, x, y, x + width + cw);
                    }
                } else {
                    target.DrawRectangle(&rect, &brush, 1.0, None);
                }
            }

            // Scrolled back: a thin bar on the right shows where we are.
            if offset > 0 {
                let (w, h) = self.client_size();
                let total = (term.scrollback_len() + term.rows()) as f32;
                let top = (term.scrollback_len() - offset) as f32 / total * h as f32;
                let len = term.rows() as f32 / total * h as f32;
                brush.SetColor(&rgb(0x606060));
                target.FillRectangle(
                    &D2D_RECT_F { left: w as f32 - 4.0, top, right: w as f32, bottom: top + len.max(8.0) },
                    &brush,
                );
            }

            let result = target.EndDraw(None, None);
            let now = Instant::now();
            if result.is_err() {
                // Device lost (driver reset, remote session change): rebuild next frame.
                self.target = None;
                self.brush = None;
            }
            self.meter.frame(s.last_output, now);

            if !self.first_frame_logged && s.last_output.is_some() {
                self.first_frame_logged = true;
                if let Ok(d) = std::env::var("LOCALAPPDATA") {
                    let _ = std::fs::write(
                        std::path::Path::new(&d).join("OurTerminal").join("startup.log"),
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

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &self,
        target: &ID2D1HwndRenderTarget,
        brush: &ID2D1SolidColorBrush,
        text: &[u16],
        attrs: &Attrs,
        x: f32,
        y: f32,
        right: f32,
    ) {
        let idx = attrs.bold as usize + 2 * attrs.italic as usize;
        let rect = D2D_RECT_F { left: x, top: y + self.baseline_fix, right, bottom: y + self.cell_h };
        unsafe {
            target.DrawText(
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

fn pick_font(dwrite: &IDWriteFactory) -> Vec<u16> {
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut coll, false).is_ok() {
            if let Some(coll) = coll {
                for name in ["Cascadia Mono", "Cascadia Code", "Consolas", "Courier New"] {
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
    wide("Consolas")
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

/// Windows Terminal's "Campbell" scheme for the first 16 colors.
const PALETTE: [u32; 16] = [
    0x0C0C0C, 0xC50F1F, 0x13A10E, 0xC19C00, 0x0037DA, 0x881798, 0x3A96DD, 0xCCCCCC, 0x767676, 0xE74856, 0x16C60C,
    0xF9F1A5, 0x3B78FF, 0xB4009E, 0x61D6D6, 0xF2F2F2,
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

fn colors(a: &Attrs, default_fg: D2D1_COLOR_F, default_bg: D2D1_COLOR_F) -> (D2D1_COLOR_F, D2D1_COLOR_F) {
    let resolve = |c: Color, default: D2D1_COLOR_F, bright: bool| match c {
        Color::Default => default,
        Color::Indexed(i) if bright && i < 8 => indexed(i + 8),
        Color::Indexed(i) => indexed(i),
        Color::Rgb(r, g, b) => rgb((r as u32) << 16 | (g as u32) << 8 | b as u32),
    };
    let fg = resolve(a.fg, default_fg, a.bold);
    let bg = resolve(a.bg, default_bg, false);
    if a.inverse {
        (bg, fg)
    } else {
        (fg, bg)
    }
}

#[allow(dead_code)]
fn _assert_interfaces(t: &ID2D1HwndRenderTarget) {
    let _ = t.cast::<windows::Win32::Graphics::Direct2D::ID2D1RenderTarget>();
}
