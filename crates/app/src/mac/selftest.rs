//! Automated checks that drive one Trinidad Head window with its own synthetic events.
//!
//! `TRINIDAD_HEAD_SELFTEST=<mode>` turns it on. Events are handed straight to this window's
//! NSWindow (never posted system-wide), so the real cursor and other apps are untouched.
//! Results go to `TRINIDAD_HEAD_RESULTS` as PASS/FAIL lines.
//!
//! Modes:
//! - `full`: resize, move, buttons, sidebar, selection, copy/paste, keys, input methods.
//!   Expects the window's command to write its raw stdin to `TRINIDAD_HEAD_CAPTURE`.
//! - `claude`: a real Claude Code session: renders, survives resizes, quits on Ctrl-C twice.
//! - `version`: `claude --version` prints a version.
//! - `stress`: a big output flood finishes, with a sane frame rate.

use std::cell::{Cell, RefCell};
use std::io::Write;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::Instant;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::msg_send;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypeString};
use objc2_foundation::{NSNotFound, NSPoint, NSRange, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSUInteger, NSURL};

use super::{THWindow, TermView};
use crate::layout::{Button, Hit};
use crate::theme::GLOWS;

/// A line to write if the app quits while a close/quit check is armed.
static ON_QUIT: Mutex<Option<String>> = Mutex::new(None);

thread_local! {
    /// When the app started, for timing the stress run from the first byte of output.
    static STARTED: Cell<Option<Instant>> = const { Cell::new(None) };
}

fn results_path() -> Option<std::path::PathBuf> {
    std::env::var_os("TRINIDAD_HEAD_RESULTS").map(std::path::PathBuf::from)
}

fn append(line: &str) {
    if let Some(p) = results_path() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn on_terminate() {
    if let Some(line) = ON_QUIT.lock().unwrap().take() {
        append(&line);
        let secs = STARTED.with(|t| t.get()).map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        append(&format!("NOTE quit {secs:.1}s after the last check started"));
        append("DONE");
    }
}

struct Ctx {
    window: Retained<THWindow>,
    view: Retained<TermView>,
    mark: Cell<u64>,
    base: Cell<NSRect>,
    before: Cell<(NSRect, usize, usize)>,
    settings: RefCell<Option<Option<String>>>,
    glow0: Cell<usize>,
    t0: Cell<Option<Instant>>,
    frames0: Cell<u64>,
    fails: Cell<u32>,
}

enum Step {
    Act(f64, Box<dyn Fn(&Ctx)>),
    /// Check `cond` every `interval` seconds up to `tries` times, then call `then(met)`.
    Poll(f64, u32, Box<dyn Fn(&Ctx) -> bool>, Box<dyn Fn(&Ctx, bool)>),
}

fn act(wait: f64, f: impl Fn(&Ctx) + 'static) -> Step {
    Step::Act(wait, Box::new(f))
}

fn poll(interval: f64, tries: u32, cond: impl Fn(&Ctx) -> bool + 'static, then: impl Fn(&Ctx, bool) + 'static) -> Step {
    Step::Poll(interval, tries, Box::new(cond), Box::new(then))
}

fn later(wait: f64, f: impl Fn() + 'static) {
    let block = RcBlock::new(move |_t: NonNull<NSTimer>| f());
    unsafe {
        let _ = NSTimer::scheduledTimerWithTimeInterval_repeats_block(wait, false, &block);
    }
}

/// Write a PNG of the whole window (glow included) a few seconds after launch.
pub fn shot_later(view: Retained<TermView>, path: std::path::PathBuf) {
    later(3.0, move || {
        let rect = view.bounds();
        if let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(rect) {
            view.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
            let data = unsafe {
                rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new())
            };
            if let Some(data) = data {
                let _ = std::fs::write(&path, data.to_vec());
            }
        }
    });
}

fn run(ctx: Rc<Ctx>, steps: Rc<Vec<Step>>, i: usize, tries_left: u32) {
    let Some(step) = steps.get(i) else { return };
    match step {
        Step::Act(wait, f) => {
            f(&ctx);
            let wait = *wait;
            later(wait, move || run(ctx.clone(), steps.clone(), i + 1, 0));
        }
        Step::Poll(interval, tries, cond, then) => {
            let left = if tries_left == 0 { *tries } else { tries_left };
            if cond(&ctx) {
                then(&ctx, true);
                later(0.1, move || run(ctx.clone(), steps.clone(), i + 1, 0));
            } else if left <= 1 {
                then(&ctx, false);
                later(0.1, move || run(ctx.clone(), steps.clone(), i + 1, 0));
            } else {
                let interval = *interval;
                later(interval, move || run(ctx.clone(), steps.clone(), i, left - 1));
            }
        }
    }
}

impl Ctx {
    fn check(&self, name: &str, ok: bool, detail: impl AsRef<str>) {
        if ok {
            append(&format!("PASS {name}"));
        } else {
            self.fails.set(self.fails.get() + 1);
            append(&format!("FAIL {name}: {}", detail.as_ref()));
        }
    }

    fn feed(&self, bytes: &[u8]) {
        self.view.shared().lock().unwrap().term.feed(bytes);
        self.view.setNeedsDisplay(true);
    }

    fn screen_text(&self) -> String {
        let shared = self.view.shared();
        let s = shared.lock().unwrap();
        (0..s.term.rows()).map(|r| s.term.row_text(r)).collect::<Vec<_>>().join("\n")
    }

    fn term_size(&self) -> (usize, usize) {
        let shared = self.view.shared();
        let s = shared.lock().unwrap();
        (s.term.cols(), s.term.rows())
    }

    fn capture_len() -> u64 {
        std::env::var_os("TRINIDAD_HEAD_CAPTURE")
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0)
    }

    fn mark(&self) {
        self.mark.set(Self::capture_len());
    }

    fn captured(&self) -> Vec<u8> {
        let Some(p) = std::env::var_os("TRINIDAD_HEAD_CAPTURE") else { return Vec::new() };
        let all = std::fs::read(p).unwrap_or_default();
        all.get(self.mark.get() as usize..).map(|b| b.to_vec()).unwrap_or_default()
    }

    fn frame(&self) -> NSRect {
        self.window.frame()
    }

    /// View point (top-left origin) to screen point (bottom-left origin).
    fn to_screen(&self, x: f64, y: f64) -> NSPoint {
        let f = self.frame();
        NSPoint::new(f.origin.x + x, f.origin.y + f.size.height - y)
    }

    fn mouse_at_screen(&self, t: NSEventType, p: NSPoint, flags: NSEventModifierFlags, clicks: isize) {
        let f = self.frame();
        let loc = NSPoint::new(p.x - f.origin.x, p.y - f.origin.y);
        let e = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            t,
            loc,
            flags,
            0.0,
            self.window.windowNumber(),
            None,
            0,
            clicks,
            1.0,
        );
        if let Some(e) = e {
            self.window.sendEvent(&e);
        }
    }

    fn mouse(&self, t: NSEventType, x: f64, y: f64, flags: NSEventModifierFlags) {
        self.mouse_at_screen(t, self.to_screen(x, y), flags, 1);
    }

    fn click(&self, x: f64, y: f64) {
        self.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
        self.mouse(NSEventType::LeftMouseUp, x, y, NSEventModifierFlags::empty());
    }

    fn click_button(&self, b: Button) {
        let (x, y, _) = self.view.layout().button(b);
        self.click(x as f64, y as f64);
    }

    /// Centre of a screen cell, in view coordinates.
    fn cell_point(&self, row: usize, col: usize) -> (f64, f64) {
        let l = self.view.layout();
        let (cw, ch) = self.view.cell_size();
        (l.text.l as f64 + (col as f64 + 0.5) * cw, l.text.t as f64 + (row as f64 + 0.5) * ch)
    }

    fn drag_cells(&self, from: (usize, usize), to: (usize, usize), flags: NSEventModifierFlags) {
        let (x0, y0) = self.cell_point(from.0, from.1);
        let (x1, y1) = self.cell_point(to.0, to.1);
        self.mouse(NSEventType::LeftMouseDown, x0, y0, flags);
        self.mouse(NSEventType::LeftMouseDragged, (x0 + x1) / 2.0, (y0 + y1) / 2.0, flags);
        self.mouse(NSEventType::LeftMouseDragged, x1, y1, flags);
        self.mouse(NSEventType::LeftMouseUp, x1, y1, flags);
    }

    fn key(&self, chars: &str, bare: &str, flags: NSEventModifierFlags, code: u16) {
        let e = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            flags,
            0.0,
            self.window.windowNumber(),
            None,
            &NSString::from_str(chars),
            &NSString::from_str(bare),
            false,
            code,
        );
        if let Some(e) = e {
            self.window.sendEvent(&e);
        }
    }

    fn scroll(&self, notches: i32) {
        use objc2_core_graphics::{CGEvent, CGScrollEventUnit};
        if let Some(cg) = CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 1, notches, 0, 0) {
            if let Some(e) = NSEvent::eventWithCGEvent(&cg) {
                // Straight to the view: the window would route a wheel event to wherever the
                // real cursor is, which may not be over this window.
                let _: () = unsafe { msg_send![&*self.view, scrollWheel: &*e] };
            }
        }
    }

    /// A picture of the window exactly as it looks right now, so a person can look at the
    /// frame instead of taking a passing check's word for it.
    fn shot(&self, name: &str) {
        let rect = self.view.bounds();
        if let Some(rep) = self.view.bitmapImageRepForCachingDisplayInRect(rect) {
            self.view.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
            if let Some(data) = unsafe {
                rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new())
            } {
                let dir = std::env::var("TRINIDAD_HEAD_SHOTS").unwrap_or_else(|_| "/tmp".into());
                let _ = std::fs::write(format!("{dir}/{name}.png"), data.to_vec());
            }
        }
    }

    /// Rows the window actually painted a highlight on in the last frame.
    fn highlighted_rows(&self) -> usize {
        self.view.painted_sel_rows()
    }

    fn has_sel(&self) -> bool {
        self.view.has_selection()
    }

    fn set_pasteboard(&self, text: &str) {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString });
    }

    fn pasteboard(&self) -> String {
        NSPasteboard::generalPasteboard()
            .stringForType(unsafe { NSPasteboardTypeString })
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    fn settings_path() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Library/Application Support/TrinidadHead/settings.txt"))
    }

    fn close_window(&self) {
        self.window.close();
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 2.0
}

fn show(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if (0x20..0x7f).contains(&b) { (b as char).to_string() } else { format!("\\x{b:02x}") }).collect()
}

/// Every process started under this app, for diagnosing a session that won't end.
fn descendants() -> String {
    let out = std::process::Command::new("/bin/ps").args(["-axo", "pid=,ppid=,stat=,command="]).output();
    let Ok(out) = out else { return String::new() };
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let rows: Vec<(u32, u32, String)> = text
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            Some((pid, ppid, l.chars().take(160).collect()))
        })
        .collect();
    let mut keep = vec![std::process::id()];
    let mut lines = Vec::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (pid, ppid, line) in &rows {
            if keep.contains(ppid) && !keep.contains(pid) {
                keep.push(*pid);
                lines.push(line.clone());
                changed = true;
            }
        }
    }
    lines.join("\n")
}

fn no_garble(text: &str) -> bool {
    !text.contains('\u{1b}') && !text.contains('\u{FFFD}') && !text.contains("[?")
}

pub fn start(mode: &str, window: Retained<THWindow>, view: Retained<TermView>) {
    append(&format!("START {mode} pid {}", std::process::id()));
    STARTED.with(|t| t.set(Some(Instant::now())));
    let ctx = Rc::new(Ctx {
        window,
        view,
        mark: Cell::new(0),
        base: Cell::new(NSRect::ZERO),
        before: Cell::new((NSRect::ZERO, 0, 0)),
        settings: RefCell::new(None),
        glow0: Cell::new(0),
        t0: Cell::new(None),
        frames0: Cell::new(0),
        fails: Cell::new(0),
    });
    let steps = match mode {
        "full" => full_steps(),
        "claude" => claude_steps(),
        "version" => version_steps(),
        "stress" => stress_steps(),
        "program" | "program-chat" => program_steps(),
        "claude-select" => claude_select_steps(),
        "prompt-edit" => prompt_edit_steps(),
        "claude-edit" => claude_edit_steps(),
        other => {
            append(&format!("FAIL unknown self-test mode {other}"));
            return;
        }
    };
    let steps = Rc::new(steps);
    later(1.5, move || run(ctx.clone(), steps.clone(), 0, 0));
}

fn place_window(c: &Ctx, w: f64, h: f64) {
    let Some(screen) = c.window.screen() else { return };
    let v = screen.visibleFrame();
    let frame = NSRect::new(
        NSPoint::new(v.origin.x + (v.size.width - w) / 2.0, v.origin.y + (v.size.height - h) / 2.0),
        NSSize::new(w, h),
    );
    c.window.setFrame_display(frame, true);
    c.base.set(frame);
}

type EdgePick = fn(&crate::layout::Layout) -> (f32, f32);

fn resize_steps(steps: &mut Vec<Step>, name: &'static str, pick: EdgePick, dx: f64, dy: f64) {
    steps.push(act(0.5, move |c| {
        c.window.setFrame_display(c.base.get(), true);
        let (cols, rows) = c.term_size();
        c.before.set((c.frame(), cols, rows));
        let l = c.view.layout();
        let (x, y) = pick(&l);
        let hit = l.hit(x, y);
        let start = c.to_screen(x as f64, y as f64);
        let none = NSEventModifierFlags::empty();
        c.mouse_at_screen(NSEventType::LeftMouseDown, start, none, 1);
        // Screen y runs upward, so a downward drag in the view is a negative screen delta.
        for k in 1..=4 {
            let f = k as f64 / 4.0;
            let p = NSPoint::new(start.x + dx * f, start.y - dy * f);
            c.mouse_at_screen(NSEventType::LeftMouseDragged, p, none, 1);
        }
        let end = NSPoint::new(start.x + dx, start.y - dy);
        c.mouse_at_screen(NSEventType::LeftMouseUp, end, none, 1);
        if hit == Hit::Client || hit == Hit::Caption {
            append(&format!("NOTE {name}: start point hit {hit:?}"));
        }
    }));
    steps.push(act(0.1, move |c| {
        let (f0, cols0, rows0) = c.before.get();
        let f1 = c.frame();
        let (cols1, rows1) = c.term_size();
        let (gw, gh) = (f1.size.width - f0.size.width, f1.size.height - f0.size.height);
        let want_w = dx.abs();
        let want_h = dy.abs();
        let mut ok = near(gw, want_w) && near(gh, want_h);
        if want_w > 0.0 {
            ok &= cols1 > cols0;
        }
        if want_h > 0.0 {
            ok &= rows1 > rows0;
        }
        // Edges on the left/bottom must keep the opposite edge still.
        if dx < 0.0 {
            ok &= near(f1.origin.x + f1.size.width, f0.origin.x + f0.size.width);
        }
        c.check(
            &format!("resize by dragging {name}"),
            ok,
            format!(
                "size {:.0}x{:.0} -> {:.0}x{:.0}, grid {cols0}x{rows0} -> {cols1}x{rows1}",
                f0.size.width, f0.size.height, f1.size.width, f1.size.height
            ),
        );
    }));
}

fn full_steps() -> Vec<Step> {
    let mut s: Vec<Step> = Vec::new();
    let none = NSEventModifierFlags::empty();

    s.push(act(0.8, |c| {
        place_window(c, 900.0, 620.0);
        *c.settings.borrow_mut() = Some(Ctx::settings_path().and_then(|p| std::fs::read_to_string(p).ok()));
        c.glow0.set(c.view.glow());
    }));

    // 1. Resize from every edge and the grip.
    resize_steps(&mut s, "the right edge", |l| (l.body.r - 2.0, l.body.cy()), 40.0, 0.0);
    resize_steps(&mut s, "the left edge", |l| (l.body.l + 2.0, l.body.cy()), -40.0, 0.0);
    resize_steps(&mut s, "the top edge", |l| (l.body.cx(), l.body.t + 2.0), 0.0, -40.0);
    resize_steps(&mut s, "the bottom edge", |l| (l.body.cx(), l.body.b - 2.0), 0.0, 40.0);
    resize_steps(&mut s, "the /// grip", |l| l.grip(), 40.0, 40.0);

    // 2. Move: the top strip is the drag handle; the move itself is done along the same path.
    s.push(act(0.4, |c| {
        c.window.setFrame_display(c.base.get(), true);
        let l = c.view.layout();
        let top = l.hit(l.body.cx(), l.body.t + 22.0);
        let mid = l.hit(l.body.cx(), l.body.cy());
        c.check("top strip is the window's drag handle", top == Hit::Caption, format!("top strip hit {top:?}"));
        c.check("middle of the window is terminal, not a handle", mid == Hit::Client, format!("middle hit {mid:?}"));
        let f0 = c.frame();
        for k in 1..=5 {
            c.window.setFrameOrigin(NSPoint::new(f0.origin.x + 10.0 * k as f64, f0.origin.y - 10.0 * k as f64));
        }
        c.before.set((f0, 0, 0));
    }));
    s.push(act(0.2, |c| {
        let f0 = c.before.get().0;
        let f1 = c.frame();
        c.check(
            "window moves and keeps its size",
            near(f1.origin.x, f0.origin.x + 50.0) && near(f1.origin.y, f0.origin.y - 50.0) && near(f1.size.width, f0.size.width),
            format!("origin {:?} -> {:?}", f0.origin, f1.origin),
        );
        c.window.setFrame_display(c.base.get(), true);
    }));

    // 3. Buttons: yellow minimizes, green zooms and restores (red is the very last step).
    s.push(act(1.5, |c| c.click_button(Button::Minimize)));
    s.push(act(0.1, |c| {
        c.check("yellow button minimizes", c.window.isMiniaturized(), "window is not minimized");
        c.window.deminiaturize(None);
    }));
    s.push(act(1.5, |_| {}));
    s.push(act(0.3, |c| {
        c.check("minimized window comes back", !c.window.isMiniaturized() && c.window.isVisible(), "still minimized");
        c.window.makeKeyAndOrderFront(None);
        c.click_button(Button::Zoom);
    }));
    s.push(act(0.8, |c| {
        let full = c.window.screen().map(|s| s.visibleFrame()).unwrap_or(NSRect::ZERO);
        let f = c.frame();
        c.check(
            "green button fills the screen",
            near(f.size.width, full.size.width) && near(f.size.height, full.size.height) && c.view.layout().maximized,
            format!("frame {:.0}x{:.0}, screen {:.0}x{:.0}", f.size.width, f.size.height, full.size.width, full.size.height),
        );
        c.click_button(Button::Zoom);
    }));
    s.push(act(0.8, |c| {
        let (f, b) = (c.frame(), c.base.get());
        c.check(
            "green button again restores the size",
            near(f.size.width, b.size.width) && near(f.size.height, b.size.height) && !c.view.layout().maximized,
            format!("frame {:.0}x{:.0}, expected {:.0}x{:.0}", f.size.width, f.size.height, b.size.width, b.size.height),
        );
        c.click_button(Button::Glow);
    }));

    // 4. Sidebar: the glow button cycles themes and saves the choice; it is the only sidebar button.
    s.push(act(0.4, |c| {
        let want = (c.glow0.get() + 1) % GLOWS.len();
        let saved = Ctx::settings_path().and_then(|p| std::fs::read_to_string(p).ok());
        c.check(
            "glow button switches the color theme",
            c.view.glow() == want,
            format!("theme {} (wanted {want})", c.view.glow()),
        );
        let reg = std::env::var("HOME")
            .map(|h| std::path::PathBuf::from(h).join(".trinidad-head/windows").join(std::process::id().to_string()))
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        c.check("glow button updates this window's entry", reg.contains(&format!("glow={want}\n")), format!("registry {reg:?}"));
        c.check(
            "glow button leaves the saved default alone",
            saved == c.settings.borrow().clone().flatten(),
            format!("settings.txt now {saved:?}"),
        );
        for _ in 1..GLOWS.len() {
            c.click_button(Button::Glow);
        }
    }));
    s.push(act(0.4, |c| {
        c.check("glow button cycles back to the start", c.view.glow() == c.glow0.get(), format!("theme {}", c.view.glow()));
        // Put Matt's saved setting back exactly as it was.
        if let (Some(p), Some(orig)) = (Ctx::settings_path(), c.settings.borrow().clone()) {
            match orig {
                Some(text) => {
                    let _ = std::fs::write(p, text);
                }
                None => {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
    }));
    s.push(act(0.3, |c| {
        let (x, y, _) = c.view.layout().button(Button::Folder);
        c.check("the sidebar has no terminal or folder icons", c.view.layout().button_at(x, y).is_none(), "folder button still there");
    }));

    // Drag and drop: a dropped file arrives as an escaped path, bracketed like a paste.
    s.push(act(0.3, |c| {
        c.feed(b"\x1b[?2004h");
        c.mark();
        let pb = NSPasteboard::pasteboardWithUniqueName();
        pb.clearContents();
        let url = NSURL::fileURLWithPath(&NSString::from_str("/tmp/Screen Shot (1).png"));
        let s = url.absoluteString().unwrap();
        pb.setString_forType(&s, unsafe { NSPasteboardTypeFileURL });
        let ok = c.view.drop_pasteboard(&pb);
        let _: () = unsafe { msg_send![&*pb, releaseGlobally] };
        c.check("a dropped file is accepted", ok, "drop refused");
    }));
    s.push(act(0.3, |c| {
        let want = b"\x1b[200~/tmp/Screen\\ Shot\\ \\(1\\).png \x1b[201~".to_vec();
        c.check("a dropped file is typed as an escaped path", c.captured() == want, show(&c.captured()));
        c.feed(b"\x1b[?2004l");
    }));

    // 5-6. Selection and copy/paste.
    s.push(act(0.3, |c| {
        c.feed(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[2J\x1b[5;1HSELECT-ME-12345\x1b[7;1HSHIFT-SELECTED\x1b[10;1H");
        c.set_pasteboard("before");
        c.mark();
        c.drag_cells((4, 0), (4, 14), NSEventModifierFlags::empty());
    }));
    s.push(act(0.3, |c| {
        let got = c.pasteboard();
        c.check("dragging selects text without copying it", got == "before", format!("clipboard {got:?}"));
        c.check("a plain drag sends nothing to the program", c.captured().is_empty(), show(&c.captured()));
        c.drag_cells((4, 0), (4, 14), NSEventModifierFlags::empty());
        c.set_pasteboard("zzz");
        c.key("c", "c", NSEventModifierFlags::Command, 8);
    }));
    s.push(act(0.3, |c| {
        let got = c.pasteboard();
        c.check("Cmd+C copies the selection", got == "SELECT-ME-12345", format!("clipboard {got:?}"));
        c.feed(b"\x1b[?1002h\x1b[?1006h");
        c.set_pasteboard("before");
        c.mark();
        c.drag_cells((6, 0), (6, 13), NSEventModifierFlags::Shift);
        c.key("c", "c", NSEventModifierFlags::Command, 8);
    }));
    s.push(act(0.3, |c| {
        let got = c.pasteboard();
        c.check("Shift+drag selects even when the program wants the mouse", got == "SHIFT-SELECTED", format!("clipboard {got:?}"));
        c.check("Shift+drag is not sent to the program", c.captured().is_empty(), show(&c.captured()));
        c.mark();
        let (x, y) = c.cell_point(2, 3);
        c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
        c.mouse(NSEventType::LeftMouseUp, x, y, NSEventModifierFlags::empty());
    }));
    s.push(act(0.3, |c| {
        let got = c.captured();
        c.check("a click goes to the program when it asks (SGR)", got == b"\x1b[<0;4;3M\x1b[<0;4;3m", show(&got));
        c.feed(b"\x1b[2J\x1b[5;1HDRAG-ME-TOO");
        c.mark();
        c.set_pasteboard("before");
        c.drag_cells((4, 0), (4, 10), NSEventModifierFlags::empty());
    }));
    s.push(act(0.3, |c| {
        c.check("a drag selects even when the program wants the mouse", c.has_sel(), "no selection");
        c.check("a drag is not sent to the program", c.captured().is_empty(), show(&c.captured()));
        c.check("a drag does not copy by itself", c.pasteboard() == "before", c.pasteboard());
        c.feed(b"\x1b[7;1Hsee https://example.com/page now");
    }));
    // The right-click menu is modal: a timer that also runs while it tracks reads it, then
    // closes it.
    s.push(act(0.3, |c| {
        let (x, y) = c.cell_point(6, 10);
        let view = c.view.clone();
        let seen: Rc<RefCell<Option<(bool, Vec<(String, bool)>)>>> = Rc::new(RefCell::new(None));
        let seen2 = seen.clone();
        let block = RcBlock::new(move |_t: NonNull<NSTimer>| {
            let mode = NSRunLoop::currentRunLoop().currentMode().map(|m| m.to_string()).unwrap_or_default();
            *seen2.borrow_mut() = Some((mode.contains("EventTracking"), view.menu_state().1));
            view.cancel_menu();
        });
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.5, false, &block) };
        unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        let before = c.view.menu_state().0;
        c.mouse(NSEventType::RightMouseDown, x, y, NSEventModifierFlags::empty());
        c.mouse(NSEventType::RightMouseUp, x, y, NSEventModifierFlags::empty());
        let after = c.view.menu_state().0;
        c.check("right-click opens the menu", after == before + 1, format!("menus {before} -> {after}"));
        let got = seen.borrow().clone();
        match got {
            Some((tracking, items)) => {
                c.check("the menu stays open on screen until closed", tracking, "menu was not tracking");
                let titles: Vec<&str> = items.iter().map(|(t, _)| t.as_str()).collect();
                c.check(
                    "over a link the menu offers Open Link and Copy Link",
                    titles.starts_with(&["Open Link", "Copy Link"]),
                    format!("{items:?}"),
                );
                c.check(
                    "the menu's Copy is enabled after a drag-selection",
                    items.iter().any(|(t, e)| t == "Copy" && *e),
                    format!("{items:?}"),
                );
            }
            None => c.check("the menu stays open on screen until closed", false, "timer never ran"),
        }
    }));
    s.push(act(0.3, |c| {
        c.mark();
        let (x, y) = c.cell_point(6, 12);
        c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::Command);
        c.mouse(NSEventType::LeftMouseUp, x, y, NSEventModifierFlags::Command);
    }));
    s.push(act(0.3, |c| {
        let got = c.view.link_opened();
        c.check("Cmd+click opens the link", got.as_deref() == Some("https://example.com/page"), format!("{got:?}"));
        c.check("Cmd+click is not sent to the program", c.captured().is_empty(), show(&c.captured()));
        let before = c.view.menu_state().0;
        let block = RcBlock::new({
            let view = c.view.clone();
            move |_t: NonNull<NSTimer>| view.cancel_menu()
        });
        let timer = unsafe { NSTimer::timerWithTimeInterval_repeats_block(0.4, false, &block) };
        unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        let (x, y) = c.cell_point(2, 2);
        c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::Control);
        c.mouse(NSEventType::LeftMouseUp, x, y, NSEventModifierFlags::Control);
        let after = c.view.menu_state().0;
        c.check("Control-click opens the menu", after == before + 1, format!("menus {before} -> {after}"));
        // Long dictation text: drawn wrapped inside the window. A picture goes next to the
        // results for a look.
        c.feed(b"\x1b[2J\x1b[10;1H> ");
        let long = "this is a long dictated sentence that keeps going well past the right edge of the window so it has to wrap onto the next lines instead of running off the side";
        let _: () = unsafe {
            msg_send![&*c.view, setMarkedText: &*NSString::from_str(long), selectedRange: NSRange::new(0, 0), replacementRange: NSRange::new(NSNotFound as usize, 0)]
        };
        c.view.display();
        if let Some(dir) = results_path().and_then(|p| p.parent().map(|d| d.join("dictation.png"))) {
            let rect = c.view.bounds();
            if let Some(rep) = c.view.bitmapImageRepForCachingDisplayInRect(rect) {
                c.view.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
                let data = unsafe {
                    rep.representationUsingType_properties(objc2_app_kit::NSBitmapImageFileType::PNG, &objc2_foundation::NSDictionary::new())
                };
                if let Some(data) = data {
                    let _ = std::fs::write(&dir, data.to_vec());
                    append(&format!("NOTE dictation picture {}", dir.display()));
                }
            }
        }
        let _: () = unsafe { msg_send![&*c.view, unmarkText] };
        c.mark();
        c.scroll(1);
        c.scroll(-1);
    }));
    s.push(act(0.3, |c| {
        let got = String::from_utf8_lossy(&c.captured()).into_owned();
        c.check(
            "the wheel goes to the program as buttons 64/65",
            got.contains("\x1b[<64;") && got.contains("\x1b[<65;"),
            show(got.as_bytes()),
        );
        c.set_pasteboard("RIGHT-CLICK");
        c.mark();
        // The right-click menu itself is modal, so pick its Paste item's action directly.
        let _: () = unsafe { msg_send![&*c.view, paste: None::<&AnyObject>] };
    }));
    s.push(act(0.3, |c| {
        let got = c.captured();
        c.check("the right-click menu's Paste pastes (even when the program wants the mouse)", got == b"RIGHT-CLICK", show(&got));
        c.feed(b"\x1b[?1002l\x1b[?1006l\x1b[?2004h");
        c.set_pasteboard("PASTE-ONE\nline2");
        c.mark();
        c.key("v", "v", NSEventModifierFlags::Command, 9);
    }));
    s.push(act(0.3, |c| {
        let got = c.captured();
        c.check(
            "Cmd+V pastes with bracketed paste",
            got == b"\x1b[200~PASTE-ONE\rline2\x1b[201~",
            show(&got),
        );
        c.feed(b"\x1b[?2004l");
        c.mark();
        c.key("v", "v", NSEventModifierFlags::Command, 9);
    }));
    s.push(act(0.3, |c| {
        let got = c.captured();
        c.check("Cmd+V pastes plain when bracketed paste is off", got == b"PASTE-ONE\rline2", show(&got));
    }));

    // 6b. A drag held past the top or bottom edge keeps scrolling on its own, so a selection
    // can run further than the window shows without resizing it.
    // (line number of the first row on screen when the drag began, rows, scroll offset)
    let dragged: Rc<Cell<(usize, usize, usize)>> = Rc::new(Cell::new((0, 0, 0)));
    fn line_no(text: &str) -> usize {
        text.rsplit('-').next().and_then(|n| n.trim().parse().ok()).unwrap_or(0)
    }
    {
        let rec = dragged.clone();
        s.push(act(0.3, move |c| {
            c.feed(b"\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?1006l\x1b[?2004l\x1b[2J\x1b[H");
            let (_cols, rows) = c.term_size();
            // Three screens of numbered lines, so two screens end up in the scrollback.
            let mut out = String::new();
            for i in 1..=rows * 3 {
                out.push_str(&format!("AUTOSCROLL-LINE-{i:03}\r\n"));
            }
            c.feed(out.as_bytes());
            let first = line_no(&c.view.shared().lock().unwrap().term.row_text(0));
            rec.set((first, rows, 0));
            // Press on the bottom row, then hold the pointer above the text area without
            // moving it again: nothing but the autoscroll can grow this selection now.
            let (x, y) = c.cell_point(rows - 1, 0);
            let top = c.view.layout().text.t as f64 - 20.0;
            c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
            c.mouse(NSEventType::LeftMouseDragged, x, y - 8.0, NSEventModifierFlags::empty());
            c.mouse(NSEventType::LeftMouseDragged, x, top, NSEventModifierFlags::empty());
        }));
    }
    {
        let rec = dragged.clone();
        s.push(act(0.9, move |c| {
            let (first, rows, _) = rec.get();
            let off = c.view.scroll_offset();
            c.check("a drag held above the top edge scrolls back by itself", off > 0, format!("scroll offset {off}"));
            let (x, _) = c.cell_point(rows - 1, 0);
            let top = c.view.layout().text.t as f64 - 20.0;
            c.mouse(NSEventType::LeftMouseUp, x, top, NSEventModifierFlags::empty());
            c.set_pasteboard("before");
            c.key("c", "c", NSEventModifierFlags::Command, 8);
            rec.set((first, rows, c.view.scroll_offset()));
        }));
    }
    {
        let rec = dragged.clone();
        s.push(act(0.3, move |c| {
            let (first, rows, _) = rec.get();
            let got = c.pasteboard();
            let lines = got.lines().count();
            c.check(
                "the copy holds more text than the window was showing",
                lines > rows,
                format!("copied {lines} lines; the screen holds {rows}"),
            );
            let top = line_no(got.lines().next().unwrap_or(""));
            c.check(
                "the copy starts above the first line that was on screen",
                top > 0 && top < first,
                format!("copy starts at line {top}; the screen started at line {first}"),
            );
        }));
    }
    {
        let rec = dragged.clone();
        s.push(act(0.5, move |c| {
            let (first, rows, was) = rec.get();
            let off = c.view.scroll_offset();
            c.check(
                "the autoscroll stops when the button comes up",
                off == was,
                format!("scroll offset drifted {was} -> {off} after the release"),
            );
            // Now the other way: from the top of the history, hold a drag below the bottom
            // edge and the view must come forward to gather the rest.
            c.scroll(40);
            let back = c.view.scroll_offset();
            rec.set((first, rows, back));
            let (x, y) = c.cell_point(0, 0);
            let bottom = c.view.layout().text.b as f64 + 20.0;
            c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
            c.mouse(NSEventType::LeftMouseDragged, x, y + 8.0, NSEventModifierFlags::empty());
            c.mouse(NSEventType::LeftMouseDragged, x, bottom, NSEventModifierFlags::empty());
        }));
    }
    {
        let rec = dragged.clone();
        s.push(act(0.9, move |c| {
            let (_first, rows, was) = rec.get();
            let off = c.view.scroll_offset();
            c.check(
                "a drag held below the bottom edge scrolls forward by itself",
                off < was,
                format!("scroll offset {was} -> {off}"),
            );
            let (x, _) = c.cell_point(0, 0);
            let bottom = c.view.layout().text.b as f64 + 20.0;
            c.mouse(NSEventType::LeftMouseUp, x, bottom, NSEventModifierFlags::empty());
            c.set_pasteboard("before");
            c.key("c", "c", NSEventModifierFlags::Command, 8);
            rec.set((0, rows, off));
        }));
    }
    {
        let rec = dragged.clone();
        s.push(act(0.3, move |c| {
            let (_f, rows, _off) = rec.get();
            let got = c.pasteboard();
            let lines = got.lines().count();
            c.check(
                "dragging downward gathers more than the window was showing",
                lines > rows,
                format!("copied {lines} lines; the screen holds {rows}"),
            );
            c.feed(b"\x1b[2J\x1b[H");
        }));
    }

    // 7. Keys.
    let keys: Vec<(&'static str, &'static str, &'static str, NSEventModifierFlags, u16, &'static [u8])> = vec![
        ("letter a", "a", "a", none, 0, b"a"),
        ("Return", "\r", "\r", none, 36, b"\r"),
        ("Backspace", "\u{7f}", "\u{7f}", none, 51, b"\x7f"),
        ("Tab", "\t", "\t", none, 48, b"\t"),
        ("Escape", "\u{1b}", "\u{1b}", none, 53, b"\x1b"),
        ("Up arrow", "\u{F700}", "\u{F700}", NSEventModifierFlags::Function, 126, b"\x1b[A"),
        ("Left arrow", "\u{F702}", "\u{F702}", NSEventModifierFlags::Function, 123, b"\x1b[D"),
        ("Shift+Right arrow", "\u{F703}", "\u{F703}", NSEventModifierFlags::Shift.union(NSEventModifierFlags::Function), 124, b"\x1b[1;2C"),
        ("Home", "\u{F729}", "\u{F729}", NSEventModifierFlags::Function, 115, b"\x1b[H"),
        ("Delete (forward)", "\u{F728}", "\u{F728}", NSEventModifierFlags::Function, 117, b"\x1b[3~"),
        ("F1", "\u{F704}", "\u{F704}", NSEventModifierFlags::Function, 122, b"\x1bOP"),
        ("F5", "\u{F708}", "\u{F708}", NSEventModifierFlags::Function, 96, b"\x1b[15~"),
        ("F12", "\u{F70F}", "\u{F70F}", NSEventModifierFlags::Function, 111, b"\x1b[24~"),
        ("Option as Meta (Option+x)", "\u{2248}", "x", NSEventModifierFlags::Option, 7, b"\x1bx"),
        ("Ctrl+C", "\u{3}", "c", NSEventModifierFlags::Control, 8, b"\x03"),
        ("Ctrl+D", "\u{4}", "d", NSEventModifierFlags::Control, 2, b"\x04"),
        ("Shift+Tab", "\u{19}", "\t", NSEventModifierFlags::Shift, 48, b"\x1b[Z"),
    ];
    for (name, chars, bare, flags, code, want) in keys {
        s.push(act(0.25, move |c| {
            c.mark();
            c.key(chars, bare, flags, code);
        }));
        s.push(act(0.02, move |c| {
            let got = c.captured();
            c.check(&format!("key: {name}"), got == want, format!("sent {}, wanted {}", show(&got), show(want)));
        }));
    }
    s.push(act(0.3, |c| {
        c.feed(b"\x1b[?1h");
        c.mark();
        c.key("\u{F700}", "\u{F700}", NSEventModifierFlags::Function, 126);
    }));
    s.push(act(0.02, |c| {
        let got = c.captured();
        c.check("key: Up arrow in application-cursor mode", got == b"\x1bOA", show(&got));
        c.feed(b"\x1b[?1l");
    }));

    // 8. Dictation / input-method path, straight into the view.
    s.push(act(0.3, |c| {
        c.mark();
        let range = NSRange::new(NSNotFound as NSUInteger, 0);
        let text = NSString::from_str("héllo 👋");
        let any: &AnyObject = text.as_ref();
        let _: () = unsafe { msg_send![&*c.view, insertText: any, replacementRange: range] };
    }));
    s.push(act(0.1, |c| {
        let got = c.captured();
        c.check("dictated text arrives as UTF-8", got == "héllo 👋".as_bytes(), show(&got));
        c.mark();
        let range = NSRange::new(NSNotFound as NSUInteger, 0);
        let sel = NSRange::new(2, 0);
        let text = NSString::from_str("ni");
        let any: &AnyObject = text.as_ref();
        let _: () = unsafe { msg_send![&*c.view, setMarkedText: any, selectedRange: sel, replacementRange: range] };
    }));
    s.push(act(0.1, |c| {
        let marked: bool = unsafe { msg_send![&*c.view, hasMarkedText] };
        c.check("input method shows composing text", marked, "no marked text");
        c.check("composing text is not sent early", c.captured().is_empty(), show(&c.captured()));
        let range = NSRange::new(NSNotFound as NSUInteger, 0);
        let text = NSString::from_str("你");
        let any: &AnyObject = text.as_ref();
        let _: () = unsafe { msg_send![&*c.view, insertText: any, replacementRange: range] };
    }));
    s.push(act(0.2, |c| {
        let marked: bool = unsafe { msg_send![&*c.view, hasMarkedText] };
        let got = c.captured();
        c.check("input method commits the final character", got == "你".as_bytes() && !marked, show(&got));
        let rect: NSRect = unsafe {
            msg_send![&*c.view, firstRectForCharacterRange: NSRange::new(0, 0), actualRange: std::ptr::null_mut::<NSRange>()]
        };
        let f = c.frame();
        let inside = rect.origin.x >= f.origin.x && rect.origin.x <= f.origin.x + f.size.width
            && rect.origin.y >= f.origin.y && rect.origin.y <= f.origin.y + f.size.height;
        c.check("dictation popup is placed on the window", inside, format!("rect {rect:?}, window {f:?}"));
    }));

    // Last: red closes the window, which quits this test app.
    s.push(act(0.5, |c| {
        let summary = if c.fails.get() == 0 { "all checks above passed".to_string() } else { format!("{} checks failed", c.fails.get()) };
        append(&format!("NOTE {summary}"));
        *ON_QUIT.lock().unwrap() = Some("PASS red button closes the window and quits".into());
        c.window.makeKeyAndOrderFront(None);
        c.click_button(Button::Close);
    }));
    s.push(act(2.0, |c| {
        // Still here: the close didn't happen.
        ON_QUIT.lock().unwrap().take();
        c.check("red button closes the window and quits", false, "window still open");
        append("DONE");
        c.close_window();
    }));
    s
}

/// The same selecting-past-the-edge job as `program_steps`, but against REAL Claude Code
/// instead of a stand-in, because the stand-in is only ever our idea of what Claude does.
/// `/help` prints a long page locally, so the transcript grows past one screen without ever
/// calling the model.
fn claude_select_steps() -> Vec<Step> {
    let mut s: Vec<Step> = Vec::new();
    s.push(act(0.2, |c| place_window(c, 900.0, 520.0)));
    s.push(poll(
        0.5,
        40,
        |c| {
            let t = c.screen_text();
            claude_main_ui(&t) || t.contains("trust this folder")
        },
        |c, _| {
            if c.screen_text().contains("trust this folder") {
                append("NOTE Claude asked to trust the test folder; answered yes");
                c.key("\u{F701}", "\u{F701}", NSEventModifierFlags::Function, 125);
                c.key("\r", "\r", NSEventModifierFlags::empty(), 36);
            }
        },
    ));
    s.push(poll(
        0.5,
        40,
        |c| claude_main_ui(&c.screen_text()),
        |c, ok| c.check("Claude Code is up for the selection test", ok, c.screen_text()),
    ));
    // Fill the transcript with local output. Claude's `!` shell mode runs a command and puts
    // its output in the transcript, so this costs no tokens. (`/help` is no good here: it opens
    // an overlay that owns the screen instead of adding anything to scroll back through.)
    for _ in 0..3 {
        s.push(act(1.0, |c| c.key("!", "!", NSEventModifierFlags::empty(), 18)));
        s.push(act(0.5, |c| {
            c.set_pasteboard("ls -1 /usr/share/man/man1 | head -40");
            c.key("v", "v", NSEventModifierFlags::Command, 9);
        }));
        s.push(act(0.5, |c| c.key("\r", "\r", NSEventModifierFlags::empty(), 36)));
    }
    s.push(act(2.0, |c| {
        let (_cols, rows) = c.term_size();
        append(&format!("NOTE Claude window is {rows} rows"));
        c.set_pasteboard("before");
        // Press on the last row of text and hold the pointer above the top edge.
        let (x, y) = c.cell_point(rows - 1, 0);
        let top = c.view.layout().text.t as f64 - 20.0;
        c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
        c.mouse(NSEventType::LeftMouseDragged, x, y - 8.0, NSEventModifierFlags::empty());
        c.mouse(NSEventType::LeftMouseDragged, x, top, NSEventModifierFlags::empty());
    }));
    s.push(act(0.4, |c| {
        let (mode, _) = c.view.mouse_mode_for_test();
        let (alt, back) = c.view.screen_shape_for_test();
        append(&format!("NOTE at the drag: mouse tracking {mode}, alt screen {alt}, scrollback {back}, selecting {}, selection {}", c.view.selecting_for_test(), c.has_sel()));
        std::fs::write("/tmp/th_claude_select_screen.txt", c.screen_text()).ok();
    }));
    s.push(poll(0.1, 20, |c| c.view.scroll_offset() > 0, |c, ok| {
        let (kept, ticks) = c.view.history_state();
        c.check("the window starts scrolling Claude's text as soon as the drag passes the edge", ok, format!("offset {} after {ticks} ticks", c.view.scroll_offset()));
        append(&format!("NOTE window holds {kept} rows of Claude above the screen"));
    }));
    s.push(act(1.5, |_| {}));
    s.push(act(0.1, |c| {
        let (_cols, rows) = c.term_size();
        let (kept, ticks) = c.view.history_state();
        append(&format!("NOTE {kept} rows kept, offset {} over {ticks} ticks", c.view.scroll_offset()));
        append(&format!("NOTE after the hold: selecting {}, selection {}", c.view.selecting_for_test(), c.has_sel()));
        c.check("the selection survived Claude Code repainting", c.has_sel(), "no selection");
        let (x, _) = c.cell_point(rows - 1, 0);
        let top = c.view.layout().text.t as f64 - 20.0;
        c.mouse(NSEventType::LeftMouseUp, x, top, NSEventModifierFlags::empty());
        c.key("c", "c", NSEventModifierFlags::Command, 8);
    }));
    s.push(act(0.5, |c| {
        let (_cols, rows) = c.term_size();
        let got = c.pasteboard();
        let lines: Vec<&str> = got.lines().collect();
        c.check(
            "the copy reaches past the screen Claude was showing",
            lines.len() > rows,
            format!("copied {} lines; the screen holds {rows}", lines.len()),
        );
        // The prompt box is pinned to the bottom and never scrolls, so it is on screen for
        // every repaint. Banking whole screens is exactly what put it in the copy over and over.
        let boxes = lines.iter().filter(|l| l.contains('\u{276F}')).count();
        c.check(
            "the pinned prompt box is not copied once per repaint",
            boxes <= 2,
            format!("the prompt row appears {boxes} times in {} lines", lines.len()),
        );
        let solid: Vec<&&str> = lines.iter().filter(|l| l.trim().len() > 8).collect();
        let repeat = (0..solid.len().saturating_sub(8)).find(|&i| solid[i..i + 4] == solid[i + 4..i + 8]);
        c.check(
            "no block of the copy repeats straight after itself",
            repeat.is_none(),
            match repeat {
                Some(i) => format!("lines {}-{} repeat: {:?}", i, i + 4, &solid[i..i + 4]),
                None => String::new(),
            },
        );
        append("DONE");
        c.close_window();
    }));
    s
}

/// Where `word` first appears on screen, as (row, first col, last col).
fn find_word(c: &Ctx, word: &str) -> Option<(usize, usize, usize)> {
    let shared = c.view.shared();
    let s = shared.lock().unwrap();
    for r in 0..s.term.rows() {
        let cells: Vec<char> = s.term.line(r, 0).iter().filter(|x| !x.spacer).map(|x| x.ch).collect();
        let line: String = cells.iter().collect();
        if let Some(byte) = line.find(word) {
            let col = line[..byte].chars().count();
            return Some((r, col, col + word.chars().count() - 1));
        }
    }
    None
}

/// Drag across `from`..=`to` the way a hand does, then let go.
fn highlight(c: &Ctx, from: (usize, usize), to: (usize, usize)) {
    c.drag_cells(from, to, NSEventModifierFlags::empty());
}

fn highlight_word(c: &Ctx, word: &str) -> bool {
    match find_word(c, word) {
        Some((r, a, b)) => {
            highlight(c, (r, a), (r, b));
            true
        }
        None => {
            append(&format!("NOTE {word} is not on screen:\n{}", c.screen_text()));
            false
        }
    }
}

fn backspace(c: &Ctx) {
    c.key("\u{7f}", "\u{7f}", NSEventModifierFlags::empty(), 51);
}

/// Deleting a highlight inside a program's input box, against `scripts/prompt-child.py`: a
/// stand-in for Claude Code's prompt that writes its exact text to PROMPT_CHILD_OUT, so every
/// check reads the program's own buffer.
fn prompt_edit_steps() -> Vec<Step> {
    fn buffer() -> String {
        std::env::var_os("PROMPT_CHILD_OUT").and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default()
    }
    fn expect(name: &'static str, want: impl Fn() -> String + 'static) -> Step {
        let want = Rc::new(want);
        let w2 = want.clone();
        poll(0.1, 30, move |_| buffer() == want(), move |c, ok| c.check(name, ok, format!("buffer {:?}, wanted {:?}", buffer(), w2())))
    }
    let long: &'static str = "w01 w02 w03 w04 w05 w06 w07 w08 w09 w10 w11 w12 w13 w14 w15 w16 w17 w18 w19 w20 \
w21 w22 w23 w24 w25 w26 w27 w28 w29 w30 w31 w32 w33 w34 w35 w36 w37 w38 w39 w40";
    let mut s: Vec<Step> = Vec::new();
    s.push(act(0.2, |c| place_window(c, 700.0, 420.0)));
    s.push(poll(0.2, 40, |c| c.screen_text().contains('❯'), |c, ok| c.check("the stand-in prompt painted", ok, c.screen_text())));
    s.push(act(0.3, |c| {
        c.set_pasteboard("alpha bravo charlie delta echo");
        c.key("v", "v", NSEventModifierFlags::Command, 9);
    }));
    s.push(expect("text pasted into the prompt", || "alpha bravo charlie delta echo".into()));

    // Backspace with nothing highlighted is still just Backspace.
    s.push(act(0.3, backspace));
    s.push(expect("Backspace with no highlight deletes one character", || "alpha bravo charlie delta ech".into()));

    s.push(act(0.3, |c| {
        highlight_word(c, "bravo");
    }));
    s.push(act(0.3, |c| {
        c.check("dragging over the prompt text highlights it", c.has_sel(), "no selection");
        backspace(c);
    }));
    s.push(expect("highlight a word, Backspace deletes the word", || "alpha  charlie delta ech".into()));
    s.push(act(0.1, |c| {
        c.check("the highlight is gone after the delete", !c.has_sel(), "still highlighted");
        c.check("the delete ran through the prompt path", c.view.cuts_for_test() == 1, format!("{} cuts", c.view.cuts_for_test()));
    }));

    // Typing over a highlight replaces it.
    s.push(act(0.3, |c| {
        highlight_word(c, "charlie");
    }));
    s.push(act(0.3, |c| c.key("X", "x", NSEventModifierFlags::Shift, 7)));
    s.push(expect("typing over a highlight replaces it", || "alpha  X delta ech".into()));

    // Fast typing right behind it lands after the replacement, in order.
    s.push(act(0.3, |c| {
        highlight_word(c, "delta");
    }));
    s.push(act(0.0, |c| {
        c.key("a", "a", NSEventModifierFlags::empty(), 0);
        c.key("b", "b", NSEventModifierFlags::empty(), 11);
        c.key("c", "c", NSEventModifierFlags::empty(), 8);
    }));
    s.push(expect("keys typed while a highlight is being replaced all land, in order", || "alpha  X abc ech".into()));

    // Pasting over a highlight replaces it.
    s.push(act(0.3, |c| {
        highlight_word(c, "ech");
    }));
    s.push(act(0.3, |c| {
        c.set_pasteboard("PASTED");
        c.key("v", "v", NSEventModifierFlags::Command, 9);
    }));
    s.push(expect("pasting over a highlight replaces it", || "alpha  X abc PASTED".into()));

    // Forward Delete does the same as Backspace.
    s.push(act(0.3, |c| {
        highlight_word(c, "abc");
    }));
    s.push(act(0.3, |c| c.key("\u{F728}", "\u{F728}", NSEventModifierFlags::Function, 117)));
    s.push(expect("highlight, forward Delete deletes it", || "alpha  X  PASTED".into()));

    // Cmd+X copies and deletes.
    s.push(act(0.3, |c| {
        highlight_word(c, "PASTED");
    }));
    s.push(act(0.3, |c| c.key("x", "x", NSEventModifierFlags::Command, 7)));
    s.push(expect("Cmd+X takes the highlight out of the prompt", || "alpha  X  ".into()));
    s.push(act(0.1, |c| c.check("Cmd+X put it on the clipboard", c.pasteboard() == "PASTED", c.pasteboard())));

    // A highlight out in the transcript is not the prompt's: Backspace stays one character.
    s.push(act(0.3, |c| {
        highlight_word(c, "TRANSCRIPT-LINE-1");
    }));
    s.push(act(0.3, backspace));
    s.push(expect("a highlight outside the prompt leaves the prompt alone (one Backspace)", || "alpha  X ".into()));

    // Highlight everything from the ❯ mark to past the end of the text: all of it goes.
    s.push(act(0.3, |c| {
        let Some((r, _, _)) = find_word(c, "alpha") else { return };
        let (cols, _) = c.term_size();
        highlight(c, (r, 0), (r, cols - 1));
    }));
    s.push(act(0.3, backspace));
    s.push(expect("highlighting the whole line, mark included, and Backspace empties the prompt", String::new));

    // Text that wraps onto several rows: highlight from a word on one row to a word on the next.
    s.push(act(0.3, move |c| {
        c.set_pasteboard(long);
        c.key("v", "v", NSEventModifierFlags::Command, 9);
    }));
    s.push(expect("a long text wraps in the prompt", move || long.into()));
    let picked: Rc<RefCell<(String, String)>> = Rc::new(RefCell::new((String::new(), String::new())));
    {
        let picked = picked.clone();
        s.push(act(0.3, move |c| {
            // The first word on the second text row, and one a few words before it.
            let Some((r1, _, _)) = find_word(c, "w01") else { return };
            let shared = c.view.shared();
            let row2: String = shared.lock().unwrap().term.row_text(r1 + 1);
            let Some(first2) = row2.split_whitespace().next().map(str::to_string) else { return };
            let n: usize = first2[1..].parse().unwrap_or(10);
            let a = format!("w{:02}", n.saturating_sub(3));
            let b = format!("w{:02}", n + 2);
            append(&format!("NOTE wrap test: highlighting {a} (row {r1}) through {b} (row {})", r1 + 1));
            let (Some(pa), Some(pb)) = (find_word(c, &a), find_word(c, &b)) else { return };
            highlight(c, (pa.0, pa.1), (pb.0, pb.2));
            *picked.borrow_mut() = (a, b);
        }));
    }
    s.push(act(0.3, backspace));
    {
        let picked = picked.clone();
        s.push(poll(
            0.1,
            30,
            {
                let picked = picked.clone();
                move |_| {
                    let (a, b) = picked.borrow().clone();
                    !a.is_empty() && buffer() == cut_words(long, &a, &b)
                }
            },
            move |c, ok| {
                let (a, b) = picked.borrow().clone();
                c.check("a highlight across a wrapped line deletes exactly what was highlighted", ok, format!("buffer {:?}, wanted {:?}", buffer(), cut_words(long, &a, &b)));
            },
        ));
    }
    s.push(act(0.2, |c| {
        c.shot("prompt-edit-final");
        append("DONE");
        c.close_window();
    }));
    s
}

/// `text` with everything from the start of word `a` to the end of word `b` removed.
fn cut_words(text: &str, a: &str, b: &str) -> String {
    let (Some(i), Some(j)) = (text.find(a), text.find(b)) else { return String::from("?") };
    format!("{}{}", &text[..i], &text[j + b.len()..])
}

/// The same deletes against REAL Claude Code. Nothing is ever submitted, so no tokens.
fn claude_edit_steps() -> Vec<Step> {
    fn prompt_row(c: &Ctx) -> String {
        // Claude writes a no-break space after the mark.
        c.screen_text().lines().find(|l| l.starts_with('\u{276F}')).unwrap_or("").replace('\u{a0}', " ")
    }
    let mut s: Vec<Step> = Vec::new();
    s.push(act(0.2, |c| place_window(c, 900.0, 520.0)));
    s.push(poll(
        0.5,
        40,
        |c| {
            let t = c.screen_text();
            claude_main_ui(&t) || t.contains("trust this folder")
        },
        |c, _| {
            if c.screen_text().contains("trust this folder") {
                append("NOTE Claude asked to trust the test folder; answered yes");
                c.key("\u{F701}", "\u{F701}", NSEventModifierFlags::Function, 125);
                c.key("\r", "\r", NSEventModifierFlags::empty(), 36);
            }
        },
    ));
    s.push(poll(0.5, 40, |c| claude_main_ui(&c.screen_text()), |c, ok| c.check("Claude Code is up for the edit test", ok, c.screen_text())));
    s.push(act(1.0, |c| {
        for ch in "alpha bravo charlie delta".chars() {
            let t = ch.to_string();
            c.key(&t, &t, NSEventModifierFlags::empty(), 0);
        }
    }));
    s.push(poll(0.2, 20, |c| prompt_row(c).contains("alpha bravo charlie delta"), |c, ok| c.check("typed into Claude's prompt", ok, prompt_row(c))));
    s.push(act(0.3, |c| {
        highlight_word(c, "bravo");
    }));
    s.push(act(0.3, |c| {
        let shared = c.view.shared();
        let cursor = shared.lock().unwrap().term.cursor();
        append(&format!("NOTE before Backspace: selection {:?}, caret {cursor:?}", c.view.selection_for_test()));
        backspace(c);
    }));
    s.push(act(1.0, |c| append(&format!("NOTE cuts after Backspace: {}", c.view.cuts_for_test()))));
    s.push(poll(
        0.1,
        30,
        |c| prompt_row(c).trim_end() == "\u{276F} alpha  charlie delta",
        |c, ok| c.check("in Claude Code, highlight a word + Backspace deletes the word", ok, prompt_row(c)),
    ));
    s.push(act(0.3, |c| {
        highlight_word(c, "charlie");
    }));
    s.push(act(0.3, |c| c.key("Z", "z", NSEventModifierFlags::Shift, 6)));
    s.push(poll(
        0.1,
        30,
        |c| prompt_row(c).trim_end() == "\u{276F} alpha  Z delta",
        |c, ok| c.check("in Claude Code, typing over a highlight replaces it", ok, prompt_row(c)),
    ));
    s.push(act(0.3, |c| {
        let Some((r, _, _)) = find_word(c, "alpha") else { return };
        let (cols, _) = c.term_size();
        highlight(c, (r, 0), (r, cols - 1));
    }));
    s.push(act(0.3, backspace));
    s.push(poll(
        0.1,
        30,
        |c| !c.screen_text().contains("alpha") && !c.screen_text().contains("delta"),
        |c, ok| c.check("in Claude Code, highlighting the whole prompt + Backspace empties it", ok, prompt_row(c)),
    ));
    // A prompt long enough to wrap: highlight from a word on the first row to one on the next.
    let long: &'static str = "w01 w02 w03 w04 w05 w06 w07 w08 w09 w10 w11 w12 w13 w14 w15 w16 w17 w18 w19 w20 \
w21 w22 w23 w24 w25 w26 w27 w28 w29 w30 w31 w32 w33 w34 w35 w36 w37 w38 w39 w40";
    fn prompt_words(c: &Ctx) -> Vec<String> {
        let t = c.screen_text();
        let lines: Vec<&str> = t.lines().collect();
        let Some(i) = lines.iter().position(|l| l.starts_with('\u{276F}')) else { return Vec::new() };
        let mut out = Vec::new();
        for l in &lines[i..] {
            if l.starts_with('\u{2500}') {
                break;
            }
            out.extend(l.chars().skip(2).collect::<String>().split_whitespace().map(str::to_string));
        }
        out
    }
    s.push(act(0.5, move |c| {
        for ch in long.chars() {
            let t = ch.to_string();
            c.key(&t, &t, NSEventModifierFlags::empty(), 0);
        }
    }));
    s.push(poll(0.2, 30, move |c| prompt_words(c).join(" ") == long, move |c, ok| c.check("a long prompt wraps in Claude", ok, prompt_words(c).join(" "))));
    let picked: Rc<RefCell<(String, String)>> = Rc::new(RefCell::new((String::new(), String::new())));
    {
        let picked = picked.clone();
        s.push(act(0.3, move |c| {
            let Some((r1, _, _)) = find_word(c, "w01") else { return };
            let row2: String = c.view.shared().lock().unwrap().term.row_text(r1 + 1);
            let Some(first2) = row2.split_whitespace().next().map(str::to_string) else { return };
            let n: usize = first2[1..].parse().unwrap_or(10);
            let a = format!("w{:02}", n.saturating_sub(3));
            let b = format!("w{:02}", n + 2);
            append(&format!("NOTE Claude wrap test: highlighting {a} through {b}"));
            let (Some(pa), Some(pb)) = (find_word(c, &a), find_word(c, &b)) else { return };
            highlight(c, (pa.0, pa.1), (pb.0, pb.2));
            *picked.borrow_mut() = (a, b);
        }));
    }
    s.push(act(0.3, backspace));
    {
        let p2 = picked.clone();
        s.push(poll(
            0.1,
            30,
            move |c| {
                let (a, b) = p2.borrow().clone();
                let want: Vec<String> = cut_words(long, &a, &b).split_whitespace().map(str::to_string).collect();
                !a.is_empty() && prompt_words(c) == want
            },
            move |c, ok| {
                let (a, b) = picked.borrow().clone();
                c.check("in Claude Code, a highlight across a wrapped line deletes exactly that", ok, format!("prompt {:?}, wanted {:?}", prompt_words(c).join(" "), cut_words(long, &a, &b)));
            },
        ));
    }
    s.push(act(0.2, |c| {
        c.shot("claude-edit-final");
        append("DONE");
        c.close_window();
    }));
    s
}

fn claude_main_ui(t: &str) -> bool {
    t.contains("Claude Code v") && t.contains('❯') && !t.contains("Accessing workspace")
}

fn claude_steps() -> Vec<Step> {
    let mut s: Vec<Step> = Vec::new();
    s.push(act(0.2, |c| place_window(c, 1000.0, 680.0)));
    // First run in the dedicated test folder: Claude asks whether to trust it. Say yes once.
    s.push(poll(
        0.5,
        40,
        |c| {
            let t = c.screen_text();
            claude_main_ui(&t) || t.contains("trust this folder")
        },
        |c, _| {
            let t = c.screen_text();
            if t.contains("trust this folder") {
                append("NOTE Claude asked to trust the test folder; answered yes");
                c.key("\u{F701}", "\u{F701}", NSEventModifierFlags::Function, 125);
                c.key("\r", "\r", NSEventModifierFlags::empty(), 36);
            }
        },
    ));
    s.push(poll(
        0.5,
        40,
        |c| claude_main_ui(&c.screen_text()),
        |c, ok| {
            let t = c.screen_text();
            c.check("Claude Code draws its screen", ok, format!("screen:\n{t}"));
            c.check("Claude Code screen has no stray escape codes", no_garble(&t), format!("screen:\n{t}"));
            let alt = c.view.shared().lock().unwrap().term.in_alt_screen();
            append(&format!("NOTE Claude Code alternate screen: {alt}"));
            std::fs::write("/tmp/th_selftest_claude_screen1.txt", &t).ok();
        },
    ));
    s.push(act(1.0, |c| {
        let f = c.frame();
        c.window.setFrame_display(NSRect::new(f.origin, NSSize::new(760.0, 480.0)), true);
    }));
    s.push(act(1.5, |c| {
        let f = c.frame();
        c.window.setFrame_display(NSRect::new(f.origin, NSSize::new(1150.0, 720.0)), true);
    }));
    s.push(act(2.5, |c| {
        let t = c.screen_text();
        let (cols, rows) = c.term_size();
        let l = c.view.layout();
        let (cw, ch) = c.view.cell_size();
        let want = ((l.text.w() as f64 / cw).floor() as usize, (l.text.h() as f64 / ch).floor() as usize);
        c.check("grid follows the window after resizing Claude Code", (cols, rows) == want, format!("{cols}x{rows} vs {want:?}"));
        c.check("Claude Code redraws cleanly after resizing", no_garble(&t) && claude_main_ui(&t), format!("screen:\n{t}"));
        std::fs::write("/tmp/th_selftest_claude_screen2.txt", &t).ok();
    }));
    // Let it settle after the resizes, then press Ctrl-C like a person: once, wait for the hint,
    // then again.
    s.push(act(3.0, |_| {}));
    s.push(act(0.0, |c| c.key("\u{3}", "c", NSEventModifierFlags::Control, 8)));
    s.push(poll(
        0.1,
        30,
        |c| c.screen_text().contains("again to exit"),
        |c, ok| {
            let t = c.screen_text();
            c.check("Claude Code shows the Ctrl-C hint after the first press", ok, format!("screen:\n{t}"));
            *ON_QUIT.lock().unwrap() = Some("PASS Claude Code quits on Ctrl-C twice".into());
            STARTED.with(|t| t.set(Some(Instant::now())));
        },
    ));
    // Ctrl-C reaches Claude (the hint above proves it). Whether a second press quits is up to
    // Claude: on this Mac it doesn't even on a bare pty with no terminal app involved, so it is
    // only noted. What the terminal owns is checked instead: closing the window ends Claude.
    s.push(act(0.0, |c| {
        STARTED.with(|t| t.set(Some(Instant::now())));
        c.key("\u{3}", "c", NSEventModifierFlags::Control, 8);
    }));
    s.push(act(4.0, |c| {
        *ON_QUIT.lock().unwrap() = None;
        append(&format!("NOTE Claude still running 4s after a second Ctrl-C: {}", !descendants().is_empty()));
        *ON_QUIT.lock().unwrap() = Some("PASS red button closes a window that is running Claude Code".into());
        c.window.makeKeyAndOrderFront(None);
        c.click_button(Button::Close);
    }));
    s.push(act(5.0, |c| {
        ON_QUIT.lock().unwrap().take();
        c.check("red button closes a window that is running Claude Code", false, format!("still open; processes:\n{}", descendants()));
        append("DONE");
        c.close_window();
    }));
    s
}

/// Selecting past the edge inside a program that owns the screen. The window runs
/// `scripts/fullscreen-child.py`, which behaves like Claude Code: alternate screen, mouse
/// tracking on, 300 lines it keeps to itself and paints one screen of. None of that text is in
/// our scrollback, so reaching past the edge means gathering the program's rows and holding
/// them still — Matt's complaint was that the old way scrolled the program under his hand,
/// which flashed, moved the text he was pointing at and ended up copying the whole window.
fn program_steps() -> Vec<Step> {
    let start: Rc<Cell<(usize, usize)>> = Rc::new(Cell::new((0, 0)));
    let mut s: Vec<Step> = Vec::new();

    fn line_no(text: &str) -> usize {
        text.rsplit('-').next().and_then(|n| n.trim().parse().ok()).unwrap_or(0)
    }
    fn top_line(c: &Ctx) -> usize {
        line_no(c.screen_text().lines().next().unwrap_or(""))
    }

    // Wait for the program to paint, then press on the bottom row and hold the pointer above
    // the top edge without moving it again.
    {
        let rec = start.clone();
        s.push(poll(
            0.5,
            40,
            |c| c.screen_text().contains("PROGRAM-LINE-300"),
            move |c, ok| {
                c.check("the full-screen program painted", ok, c.screen_text());
                let (_cols, rows) = c.term_size();
                rec.set((top_line(c), rows));
                c.set_pasteboard("before");
                // Press in the MIDDLE of the screen, at the end of that row, and drag up: the
                // rows below the finger must stay out of it. Anchoring to the whole screen
                // instead — the old bug — copies the window.
                let (cols, _) = c.term_size();
                let (x, y) = c.cell_point(rows / 2, cols - 1);
                let top = c.view.layout().text.t as f64 - 20.0;
                c.mouse(NSEventType::LeftMouseDown, x, y, NSEventModifierFlags::empty());
                c.mouse(NSEventType::LeftMouseDragged, x, y - 8.0, NSEventModifierFlags::empty());
                c.mouse(NSEventType::LeftMouseDragged, x, top, NSEventModifierFlags::empty());
            },
        ));
    }
    // The drag is held past the edge. The window must start moving AT ONCE — Matt's complaint
    // was a pause and then a jump — and it must move ITSELF, not scroll the program.
    s.push(poll(0.1, 20, |c| c.view.scroll_offset() > 0, |c, ok| {
        let (kept, ticks) = c.view.history_state();
        c.check("the window starts scrolling as soon as the drag passes the edge", ok, format!("offset {} after {ticks} ticks", c.view.scroll_offset()));
        append(&format!("NOTE window holds {kept} rows above the screen"));
    }));
    {
        let rec = start.clone();
        s.push(act(1.2, move |c| {
            let (first, _rows) = rec.get();
            let now = top_line(c);
            c.check(
                "the program is left alone while he drags",
                now == first,
                format!("its top line went {first} -> {now}"),
            );
            c.check("the window kept scrolling while the button was held", c.view.scroll_offset() > 2, format!("offset {}", c.view.scroll_offset()));
            let (kept, _) = c.view.history_state();
            append(&format!("NOTE selection {:?}, offset {}, kept {kept}, rows {}", c.view.selection_for_test(), c.view.scroll_offset(), c.term_size().1));
            c.check("the rows he is dragging over are highlighted", c.highlighted_rows() > 0, format!("{} rows painted", c.highlighted_rows()));
        }));
    }
    // Further back still: the highlight has to hold all the way, not just at the start.
    {
        let rec = start.clone();
        s.push(act(2.0, move |c| {
            let _ = rec.get();
            let (kept, _) = c.view.history_state();
            append(&format!("NOTE deep: selection {:?}, offset {}, kept {kept}, painted {}", c.view.selection_for_test(), c.view.scroll_offset(), c.highlighted_rows()));
            c.shot("drag-held");
            c.check(
                "the highlight holds as it keeps scrolling back",
                c.highlighted_rows() > 0,
                format!("{} rows painted at offset {}", c.highlighted_rows(), c.view.scroll_offset()),
            );
        }));
    }
    {
        let rec = start.clone();
        s.push(act(0.4, move |c| {
            let (first, rows) = rec.get();
            let (x, _) = c.cell_point(rows / 2, 0);
            let top = c.view.layout().text.t as f64 - 20.0;
            c.mouse(NSEventType::LeftMouseUp, x, top, NSEventModifierFlags::empty());
            c.shot("after-release");
            c.key("c", "c", NSEventModifierFlags::Command, 8);
            std::thread::sleep(std::time::Duration::from_millis(250));
            c.shot("after-copy");
            c.check("the highlight stays after copying", c.highlighted_rows() > 0, format!("{} rows painted", c.highlighted_rows()));
            let got = c.pasteboard();
            let lines: Vec<&str> = got.lines().filter(|l| l.starts_with("PROGRAM-LINE-")).collect();
            c.check(
                "the copy holds more than the program was showing",
                lines.len() > rows,
                format!("copied {} program lines; the screen holds {rows}", lines.len()),
            );
            let nums: Vec<usize> = lines.iter().map(|l| line_no(l)).collect();
            c.check(
                "the copy is every line in order, none repeated or skipped",
                nums.windows(2).all(|w| w[1] == w[0] + 1),
                format!("{:?}...{:?}", &nums[..nums.len().min(4)], &nums[nums.len().saturating_sub(4)..]),
            );
            c.check(
                "the copy reaches back past where the screen started",
                nums.first().is_some_and(|&n| n < first),
                format!("copy starts at {:?}; the screen started at {first}", nums.first()),
            );
            // He pressed halfway down the screen, so that is where the copy ends. The old way
            // re-anchored to the top-left corner and handed back the whole window.
            let anchor_line = first + rows / 2;
            c.check(
                "the copy stops where he put his finger down",
                nums.last().is_some_and(|&n| n == anchor_line),
                format!("copy ends at {:?}; he pressed on line {anchor_line}", nums.last()),
            );
            c.check(
                "nothing below his finger is copied",
                nums.iter().all(|&n| n <= anchor_line),
                format!("copied up to {:?}, past line {anchor_line}", nums.iter().max()),
            );
        }));
    }
    // Typing goes back to the live screen, and the program is untouched by all of it.
    s.push(act(0.3, |c| c.key("x", "x", NSEventModifierFlags::empty(), 7)));
    {
        let rec = start.clone();
        s.push(act(0.4, move |c| {
            let (first, _rows) = rec.get();
            c.check("typing snaps back to the live screen", c.view.scroll_offset() == 0, format!("offset {}", c.view.scroll_offset()));
            c.check(
                "and the program was never scrolled by any of it",
                top_line(c) == first,
                format!("top line {first} -> {}", top_line(c)),
            );
            append("DONE");
            c.close_window();
        }));
    }
    s
}

fn version_steps() -> Vec<Step> {
    vec![poll(
        0.5,
        30,
        |c| {
            let t = c.screen_text();
            t.split(|ch: char| !(ch.is_ascii_digit() || ch == '.')).any(|w| w.matches('.').count() == 2 && w.len() >= 5)
        },
        |c, ok| {
            let t = c.screen_text();
            c.check("claude --version prints a version", ok, format!("screen:\n{t}"));
            append(&format!("NOTE version screen: {}", t.trim().lines().next().unwrap_or("")));
            append("DONE");
            c.close_window();
        },
    )]
}

fn stress_steps() -> Vec<Step> {
    vec![
        // The flood starts with the window, so time it from launch.
        act(0.0, |c| {
            c.t0.set(STARTED.with(|t| t.get()));
            c.frames0.set(0);
        }),
        poll(
            0.5,
            180,
            |c| c.screen_text().contains("STRESS-DONE"),
            |c, ok| {
                let secs = c.t0.get().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
                let frames = c.view.frames() - c.frames0.get();
                let fps = frames as f64 / secs.max(0.001);
                c.check("big output flood finishes", ok, format!("not done after {secs:.0}s"));
                c.check(
                    "frame rate stays sane during the flood",
                    fps >= 10.0,
                    format!("{frames} frames in {secs:.1}s"),
                );
                let t = c.screen_text();
                c.check("screen is clean after the flood", no_garble(&t), format!("screen:\n{t}"));
                append(&format!("NOTE flood took {secs:.1}s, {frames} frames ({fps:.0} fps)"));
                append("DONE");
                c.close_window();
            },
        ),
    ]
}
