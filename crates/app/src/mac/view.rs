//! The terminal view: draws the chrome and text, and turns mouse and keyboard input into
//! bytes for the shell.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use core_vt::{Attrs, Cell, Color};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSColor, NSCompositingOperation, NSCursor, NSCursorFrameResizeDirections,
    NSCursorFrameResizePosition, NSEvent, NSEventModifierFlags, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSGraphicsContext, NSImage, NSImageSymbolConfiguration, NSMenu, NSMenuItem, NSPasteboard,
    NSPasteboardTypeFileURL, NSPasteboardTypeString, NSResponder, NSApplication, NSStringDrawing, NSTextInputClient, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSWorkspace,
};
use objc2_core_graphics::CGContext;
use objc2_foundation::{
    ns_string, NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSNotFound, NSObjectProtocol, NSPoint, NSRange,
    NSRangePointer, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSUInteger, NSURL,
};
use pty::Pty;

use super::keys::{self, Mods};
use super::paint::{self, rgba};
use super::Shared;
use crate::latency::Meter;
use crate::layout::{Button, Hit, Layout};
use crate::textutil::Harvest;
use crate::theme::{self, GLOWS};

const FONT_SIZE: f64 = 14.0;

pub struct ViewState {
    shared: Arc<Mutex<Shared>>,
    pty: Arc<Mutex<Pty>>,
    layout: Layout,
    fonts: Vec<Retained<NSFont>>, // regular, bold, italic, bold-italic
    cell_w: f64,
    cell_h: f64,
    glow: usize,
    scroll_offset: usize,
    scroll_accum: f64,
    hover: Option<Button>,
    pressed: Option<Button>,
    sel: Option<((usize, usize), (usize, usize))>,
    selecting: bool,
    resizing: Option<(Hit, NSPoint, NSRect)>,
    zoom_restore: Option<NSRect>,
    marked: String,
    user_bar: (u8, u8, u8),
    meter: Meter,
    started: Instant,
    first_frame_logged: bool,
    dump_path: Option<std::path::PathBuf>,
    last_dump: Instant,
    tracking: Option<Retained<NSTrackingArea>>,
    /// A press was sent to the program (mouse reporting), so drags and the release go there too.
    mouse_reported: bool,
    last_mouse_cell: Option<(usize, usize)>,
    /// Frames drawn so far (the self-test uses it to measure frame rate).
    frames: u64,
    /// The folder button opened a folder (self-test check).
    folder_opened: bool,
    /// A press the program wants, held back until we know it is a click (sent to the program)
    /// and not a drag (a text selection here): screen cell, modifier bits, text point.
    pending_press: Option<((usize, usize), u8, (usize, usize))>,
    /// The right-click menu currently or last shown, and the link it was opened over.
    menu: Option<Retained<NSMenu>>,
    menu_link: Option<String>,
    /// Right-click menus shown so far, and the last link opened (self-test checks).
    menus_shown: u32,
    link_opened: Option<String>,
    /// A drag-selection held past the top or bottom edge keeps scrolling on this timer.
    /// `drag_pt` is where the pointer last was, in view coordinates, so each tick can
    /// stretch the selection to it without waiting for the mouse to move again.
    autoscroll: Option<Retained<NSTimer>>,
    drag_pt: (f32, f32),
    /// Autoscroll ticks so far (the self-test reads it, to tell a stalled timer from a
    /// program that simply had nothing more to show).
    ticks: u64,
    /// Set while a drag runs past an edge inside a program that owns the screen.
    harvest: Option<Harvest>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder))]
    #[thread_kind = MainThreadOnly]
    #[name = "TrinidadHeadTermView"]
    #[ivars = RefCell<ViewState>]
    pub struct TermView;

    impl TermView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            self.draw();
        }

        #[unsafe(method(setFrameSize:))]
        fn set_frame_size(&self, size: NSSize) {
            let _: () = unsafe { msg_send![super(self), setFrameSize: size] };
            self.fit();
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let old = self.ivars().borrow_mut().tracking.take();
            if let Some(old) = old {
                self.removeTrackingArea(&old);
            }
            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    self.bounds(),
                    options,
                    Some(self),
                    None,
                )
            };
            self.addTrackingArea(&area);
            self.ivars().borrow_mut().tracking = Some(area);
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            self.on_key_down(event);
        }

        // Drag and drop: files dropped on the window are typed in as shell-escaped paths
        // (what Claude Code turns into an attached image), dropped text is pasted.
        #[unsafe(method(draggingEntered:))]
        fn dragging_entered(&self, info: &AnyObject) -> NSUInteger {
            drop_operation(info)
        }

        #[unsafe(method(draggingUpdated:))]
        fn dragging_updated(&self, info: &AnyObject) -> NSUInteger {
            drop_operation(info)
        }

        #[unsafe(method(prepareForDragOperation:))]
        fn prepare_for_drag_operation(&self, _info: &AnyObject) -> bool {
            true
        }

        #[unsafe(method(performDragOperation:))]
        fn perform_drag_operation(&self, info: &AnyObject) -> bool {
            let pb: Retained<NSPasteboard> = unsafe { msg_send![info, draggingPasteboard] };
            let ok = self.drop_pasteboard(&pb);
            if ok {
                if let Some(w) = self.window() {
                    w.makeKeyAndOrderFront(None);
                    w.makeFirstResponder(Some(self));
                }
                NSApplication::sharedApplication(self.mtm()).activate();
            }
            ok
        }

        // Dictation asks Accessibility whether a text field has focus; without these it says
        // there is nowhere to type.
        #[unsafe(method(isAccessibilityElement))]
        fn is_accessibility_element(&self) -> bool {
            true
        }

        #[unsafe(method_id(accessibilityRole))]
        fn accessibility_role(&self) -> Retained<NSString> {
            NSString::from_str("AXTextArea")
        }

        #[unsafe(method_id(accessibilityValue))]
        fn accessibility_value(&self) -> Option<Retained<AnyObject>> {
            let text = {
                let st = self.ivars().borrow();
                let s = st.shared.lock().unwrap();
                (0..s.term.rows()).map(|r| s.term.row_text(r)).collect::<Vec<_>>().join("\n")
            };
            Some(Retained::into_super(Retained::into_super(NSString::from_str(&text))))
        }

        #[unsafe(method(accessibilitySelectedTextRange))]
        fn accessibility_selected_range(&self) -> NSRange {
            NSRange::new(0, 0)
        }

        #[unsafe(method(isAccessibilityFocused))]
        fn is_accessibility_focused(&self) -> bool {
            let me = self as *const Self as *const NSResponder;
            self.window().is_some_and(|w| w.firstResponder().is_some_and(|r| std::ptr::eq(&*r, me)))
        }

        // A lone modifier press only reaches the input system if we pass it on; without this,
        // the "press Control twice" dictation shortcut never fires.
        #[unsafe(method(flagsChanged:))]
        fn flags_changed(&self, event: &NSEvent) {
            let ctx: Option<Retained<AnyObject>> = unsafe { msg_send![self, inputContext] };
            if let Some(ctx) = ctx {
                let _: bool = unsafe { msg_send![&*ctx, handleEvent: event] };
            }
            let _: () = unsafe { msg_send![super(self), flagsChanged: event] };
        }

        #[unsafe(method(performKeyEquivalent:))]
        fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
            self.on_command_key(event)
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.on_mouse_down(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.on_mouse_dragged(event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.on_mouse_up(event);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.on_mouse_moved(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let changed = self.ivars().borrow_mut().hover.take().is_some();
            if changed {
                self.setNeedsDisplay(true);
            }
            NSCursor::arrowCursor().set();
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.context_menu(event);
        }

        #[unsafe(method(copy:))]
        fn copy_action(&self, _sender: Option<&AnyObject>) {
            self.copy_and_clear();
        }

        #[unsafe(method(openLink:))]
        fn open_link_action(&self, _sender: Option<&AnyObject>) {
            let link = self.ivars().borrow().menu_link.clone();
            if let Some(link) = link {
                self.open_link(&link);
            }
        }

        #[unsafe(method(copyLink:))]
        fn copy_link_action(&self, _sender: Option<&AnyObject>) {
            let link = self.ivars().borrow().menu_link.clone();
            if let Some(link) = link {
                let pb = NSPasteboard::generalPasteboard();
                pb.clearContents();
                pb.setString_forType(&NSString::from_str(&link), unsafe { NSPasteboardTypeString });
            }
        }

        #[unsafe(method(paste:))]
        fn paste_action(&self, _sender: Option<&AnyObject>) {
            self.paste();
        }

        #[unsafe(method(selectAll:))]
        fn select_all_action(&self, _sender: Option<&AnyObject>) {
            self.select_all();
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.on_scroll(event);
        }

        #[unsafe(method(autoscrollTick:))]
        fn autoscroll_tick(&self, _timer: Option<&NSTimer>) {
            self.autoscroll_step();
        }

        // Older-style entry point some input methods still use.
        #[unsafe(method(insertText:))]
        fn insert_text_legacy(&self, text: &AnyObject) {
            self.insert(text);
        }
    }

    unsafe impl NSObjectProtocol for TermView {}

    unsafe impl NSTextInputClient for TermView {
        #[unsafe(method(insertText:replacementRange:))]
        fn insert_text(&self, text: &AnyObject, _range: NSRange) {
            self.insert(text);
        }

        #[unsafe(method(doCommandBySelector:))]
        fn do_command(&self, selector: Sel) {
            self.command(selector);
        }

        #[unsafe(method(setMarkedText:selectedRange:replacementRange:))]
        fn set_marked_text(&self, text: &AnyObject, _sel: NSRange, _rep: NSRange) {
            self.ivars().borrow_mut().marked = string_of(text);
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(unmarkText))]
        fn unmark_text(&self) {
            self.ivars().borrow_mut().marked.clear();
            self.setNeedsDisplay(true);
        }

        #[unsafe(method(selectedRange))]
        fn selected_range(&self) -> NSRange {
            // An empty caret, not "no selection": dictation won't start without an insertion point.
            NSRange::new(0, 0)
        }

        #[unsafe(method(markedRange))]
        fn marked_range(&self) -> NSRange {
            let len = self.ivars().borrow().marked.encode_utf16().count();
            if len == 0 {
                NSRange::new(NSNotFound as NSUInteger, 0)
            } else {
                NSRange::new(0, len)
            }
        }

        #[unsafe(method(hasMarkedText))]
        fn has_marked_text(&self) -> bool {
            !self.ivars().borrow().marked.is_empty()
        }

        #[unsafe(method_id(attributedSubstringForProposedRange:actualRange:))]
        fn attributed_substring(&self, _range: NSRange, _actual: NSRangePointer) -> Option<Retained<NSAttributedString>> {
            None
        }

        #[unsafe(method_id(validAttributesForMarkedText))]
        fn valid_attributes(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        #[unsafe(method(firstRectForCharacterRange:actualRange:))]
        fn first_rect(&self, _range: NSRange, _actual: NSRangePointer) -> NSRect {
            // Where dictation and input-method popups should appear: at the cursor.
            let rect = self.cursor_rect();
            match self.window() {
                Some(w) => w.convertRectToScreen(self.convertRect_toView(rect, None)),
                None => rect,
            }
        }

        #[unsafe(method(characterIndexForPoint:))]
        fn character_index(&self, _point: NSPoint) -> NSUInteger {
            NSNotFound as NSUInteger
        }
    }
);

/// Copy (1) when the drag carries files or text, otherwise refuse it (0).
fn drop_operation(info: &AnyObject) -> NSUInteger {
    let pb: Retained<NSPasteboard> = unsafe { msg_send![info, draggingPasteboard] };
    let types = unsafe { NSArray::from_slice(&[NSPasteboardTypeFileURL, NSPasteboardTypeString]) };
    if pb.availableTypeFromArray(&types).is_some() { 1 } else { 0 }
}

/// Backslash-escape a path the way macOS terminals do when a file is dropped on them.
pub fn shell_escape(path: &str) -> String {
    let mut out = String::with_capacity(path.len() + 8);
    for ch in path.chars() {
        if "\\ ()[]{}<>\"'`!#$&;|*?\t".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn string_of(obj: &AnyObject) -> String {
    if let Some(s) = obj.downcast_ref::<NSString>() {
        return s.to_string();
    }
    if let Some(a) = obj.downcast_ref::<NSAttributedString>() {
        return a.string().to_string();
    }
    String::new()
}

fn ns_color(c: [f64; 4]) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c[0], c[1], c[2], c[3])
}

fn cell_color(c: Color, bright: bool) -> Option<[f64; 4]> {
    match c {
        Color::Default => None,
        Color::Indexed(i) if bright && i < 8 => Some(indexed(i + 8)),
        Color::Indexed(i) => Some(indexed(i)),
        Color::Rgb(r, g, b) => Some([r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0, 1.0]),
    }
}

/// Same palette as the Windows build.
const PALETTE: [u32; 16] = [
    0x1B1E26, 0xFF5C57, 0x5AF78E, 0xF3F99D, 0x57C7FF, 0xFF6AC1, 0x9AEDFE, 0xD7DBE3, 0x686F7D, 0xFF7A75, 0x7CFFA6,
    0xFFFFB0, 0x7FD3FF, 0xFF8AD0, 0xB8F4FF, 0xFFFFFF,
];

fn indexed(i: u8) -> [f64; 4] {
    match i {
        0..=15 => rgba(PALETTE[i as usize], 1.0),
        16..=231 => {
            let i = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v as u32 * 40 };
            rgba(level(i / 36) << 16 | level((i / 6) % 6) << 8 | level(i % 6), 1.0)
        }
        _ => {
            let v = 8 + (i as u32 - 232) * 10;
            rgba(v << 16 | v << 8 | v, 1.0)
        }
    }
}

impl TermView {
    pub fn new(
        mtm: MainThreadMarker,
        frame: NSRect,
        shared: Arc<Mutex<Shared>>,
        pty: Arc<Mutex<Pty>>,
        started: Instant,
    ) -> Retained<Self> {
        let support = std::env::var("HOME")
            .ok()
            .map(|h| std::path::PathBuf::from(h).join("Library/Application Support/TrinidadHead"));
        if let Some(d) = &support {
            let _ = std::fs::create_dir_all(d);
        }
        let log = support
            .as_ref()
            .and_then(|d| std::fs::OpenOptions::new().create(true).append(true).open(d.join("latency.log")).ok());
        let settings_path = support.as_ref().map(|d| d.join("settings.txt"));
        // The saved theme is the starting point; each open window then takes a color no other
        // open window is using.
        let default_glow = settings_path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| theme::parse_settings(&t))
            .unwrap_or(0);
        let glow = theme::pick_glow(default_glow, &super::control::glows_in_use());
        super::control::register(glow, super::window_token());
        let user_bar = std::env::var("HOME")
            .ok()
            .and_then(|h| std::fs::read_to_string(std::path::Path::new(&h).join(".claude/themes/trinidad-head.json")).ok())
            .and_then(|t| theme::user_bar_from_theme(&t))
            .unwrap_or(theme::USER_BAR);

        let font = |name: &str| {
            NSFont::fontWithName_size(&NSString::from_str(name), FONT_SIZE)
                .unwrap_or_else(|| NSFont::monospacedSystemFontOfSize_weight(FONT_SIZE, 0.0))
        };
        let fonts = vec![font("Menlo-Regular"), font("Menlo-Bold"), font("Menlo-Italic"), font("Menlo-BoldItalic")];
        let regular = &fonts[0];
        let cell_w = unsafe {
            let attrs = NSDictionary::from_slices(&[NSFontAttributeName], &[regular.as_ref() as &AnyObject]);
            ns_string!("MMMMMMMMMM").sizeWithAttributes(Some(&attrs)).width / 10.0
        };
        let cell_h = ((regular.ascender() - regular.descender() + regular.leading()) * 1.08).ceil();

        let state = ViewState {
            shared,
            pty,
            layout: Layout::new(frame.size.width as f32, frame.size.height as f32, 1.0, false),
            fonts,
            cell_w,
            cell_h,
            glow,
            scroll_offset: 0,
            scroll_accum: 0.0,
            hover: None,
            pressed: None,
            sel: None,
            selecting: false,
            resizing: None,
            zoom_restore: None,
            marked: String::new(),
            user_bar,
            meter: Meter::new(log),
            started,
            first_frame_logged: false,
            dump_path: std::env::var_os("TRINIDAD_HEAD_DUMP").map(std::path::PathBuf::from).or_else(|| {
                // Test hook for launcher checks: while ~/.trinidad-head/dump-all exists, every new
                // window writes its screen text to ~/.trinidad-head/dumps/<pid>.txt.
                let base = std::path::PathBuf::from(std::env::var_os("HOME")?).join(".trinidad-head");
                base.join("dump-all").exists().then(|| {
                    let _ = std::fs::create_dir_all(base.join("dumps"));
                    base.join("dumps").join(format!("{}.txt", std::process::id()))
                })
            }),
            last_dump: Instant::now(),
            tracking: None,
            mouse_reported: false,
            last_mouse_cell: None,
            frames: 0,
            folder_opened: false,
            pending_press: None,
            menu: None,
            menu_link: None,
            menus_shown: 0,
            link_opened: None,
            autoscroll: None,
            drag_pt: (0.0, 0.0),
            ticks: 0,
            harvest: None,
        };
        let this = Self::alloc(mtm).set_ivars(RefCell::new(state));
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        let types = unsafe { NSArray::from_slice(&[NSPasteboardTypeFileURL, NSPasteboardTypeString]) };
        this.registerForDraggedTypes(&types);
        this
    }

    /// Recompute the layout and the terminal size from the view size.
    pub fn fit(&self) {
        let size = self.bounds().size;
        let mut st = self.ivars().borrow_mut();
        let zoomed = st.zoom_restore.is_some();
        st.layout = Layout::new(size.width as f32, size.height as f32, 1.0, zoomed);
        let text = st.layout.text;
        let cols = (text.w() as f64 / st.cell_w).floor().max(2.0) as usize;
        let rows = (text.h() as f64 / st.cell_h).floor().max(1.0) as usize;
        let mut s = st.shared.lock().unwrap();
        if s.term.cols() != cols || s.term.rows() != rows {
            s.term.resize(cols, rows);
            let _ = st.pty.lock().unwrap().resize(cols as u16, rows as u16);
        }
        drop(s);
        drop(st);
        self.setNeedsDisplay(true);
    }

    fn focused(&self) -> bool {
        self.window().map(|w| w.isKeyWindow()).unwrap_or(false)
    }

    fn cursor_rect(&self) -> NSRect {
        let st = self.ivars().borrow();
        let (r, c) = st.shared.lock().unwrap().term.cursor();
        let x = st.layout.text.l as f64 + c as f64 * st.cell_w;
        let y = st.layout.text.t as f64 + r as f64 * st.cell_h;
        NSRect::new(NSPoint::new(x, y), NSSize::new(st.cell_w, st.cell_h))
    }

    fn send(&self, bytes: &[u8]) {
        let mut st = self.ivars().borrow_mut();
        st.sel = None;
        st.harvest = None;
        st.scroll_offset = 0;
        st.meter.key(Instant::now());
        let _ = st.pty.lock().unwrap().write(bytes);
    }

    fn insert(&self, text: &AnyObject) {
        let s = string_of(text);
        self.ivars().borrow_mut().marked.clear();
        if !s.is_empty() {
            self.send(s.as_bytes());
        }
        self.setNeedsDisplay(true);
    }

    /// Editing commands the input system sends instead of text (mostly already handled in
    /// keyDown; this keeps AppKit from beeping for the rest).
    fn command(&self, selector: Sel) {
        let bytes: &[u8] = if selector == sel!(insertNewline:) {
            b"\r"
        } else if selector == sel!(insertTab:) {
            b"\t"
        } else if selector == sel!(deleteBackward:) {
            b"\x7f"
        } else if selector == sel!(cancelOperation:) {
            b"\x1b"
        } else {
            return;
        };
        self.send(bytes);
    }

    fn on_key_down(&self, event: &NSEvent) {
        let flags = event.modifierFlags();
        if flags.contains(NSEventModifierFlags::Command) {
            // Normally handled by performKeyEquivalent before keyDown; this covers events
            // delivered straight to the window.
            self.on_command_key(event);
            return;
        }
        let mods = Mods {
            shift: flags.contains(NSEventModifierFlags::Shift),
            alt: flags.contains(NSEventModifierFlags::Option),
            ctrl: flags.contains(NSEventModifierFlags::Control),
        };
        let chars = event.characters().map(|s| s.to_string()).unwrap_or_default();
        let bare = event.charactersIgnoringModifiers().map(|s| s.to_string()).unwrap_or_default();
        let has_marked = !self.ivars().borrow().marked.is_empty();

        if !has_marked {
            let app_cursor = {
                let st = self.ivars().borrow();
                let v = st.shared.lock().unwrap().term.app_cursor_keys;
                v
            };
            // Shift+PageUp/Down scroll the history, like the Windows build.
            if let Some(first) = bare.chars().next() {
                if mods.shift && (first == '\u{F72C}' || first == '\u{F72D}') {
                    self.scroll_page(first == '\u{F72C}');
                    return;
                }
                if let Some(seq) = keys::special(first, mods, app_cursor) {
                    self.send(&seq);
                    return;
                }
            }
            if let Some(seq) = keys::control(&chars, mods) {
                self.send(&seq);
                return;
            }
            // Option works as Meta: ESC + the plain key.
            if mods.alt && !mods.ctrl && !bare.is_empty() {
                let mut v = vec![0x1b];
                v.extend_from_slice(bare.as_bytes());
                self.send(&v);
                return;
            }
        }
        // Ordinary typing goes through the input system so dictation and input methods work.
        let events = NSArray::from_slice(&[event]);
        self.interpretKeyEvents(&events);
    }

    fn on_command_key(&self, event: &NSEvent) -> bool {
        let flags = event.modifierFlags();
        if !flags.contains(NSEventModifierFlags::Command) {
            return false;
        }
        let key = event.charactersIgnoringModifiers().map(|s| s.to_string().to_lowercase()).unwrap_or_default();
        match key.as_str() {
            "c" => self.copy_and_clear(),
            "v" => self.paste(),
            "w" => {
                if let Some(w) = self.window() {
                    w.close();
                }
            }
            "m" => {
                if let Some(w) = self.window() {
                    w.miniaturize(None);
                }
            }
            "a" => self.select_all(),
            _ => return false,
        }
        true
    }

    fn copy_and_clear(&self) {
        self.copy_selection();
        let mut st = self.ivars().borrow_mut();
        st.sel = None;
        st.harvest = None;
        drop(st);
        self.setNeedsDisplay(true);
    }

    fn select_all(&self) {
        self.ivars().borrow_mut().harvest = None;
        let total = {
            let st = self.ivars().borrow();
            let s = st.shared.lock().unwrap();
            (s.term.total_lines(), s.term.cols())
        };
        self.ivars().borrow_mut().sel = Some(((0, 0), (total.0.saturating_sub(1), total.1.saturating_sub(1))));
        self.setNeedsDisplay(true);
    }

    /// Right-click (or Control-click) menu: Open Link / Copy Link over a link, then Copy,
    /// Paste, Select All. Nothing is copied or pasted until one is picked.
    fn context_menu(&self, event: &NSEvent) {
        let (x, y) = self.point(event);
        let link = self.link_at(x, y);
        let menu = self.build_menu(link);
        {
            let mut st = self.ivars().borrow_mut();
            st.menu = Some(menu.clone());
            st.menus_shown += 1;
        }
        NSMenu::popUpContextMenu_withEvent_forView(&menu, event, self);
    }

    fn build_menu(&self, link: Option<String>) -> Retained<NSMenu> {
        let mtm = self.mtm();
        let has_sel = matches!(self.ivars().borrow().sel, Some((a, b)) if a != b);
        let has_clip = NSPasteboard::generalPasteboard().stringForType(unsafe { NSPasteboardTypeString }).is_some();
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let mut items = Vec::new();
        if link.is_some() {
            items.push((ns_string!("Open Link"), Some(sel!(openLink:)), true));
            items.push((ns_string!("Copy Link"), Some(sel!(copyLink:)), true));
            items.push((ns_string!(""), None, false));
        }
        items.push((ns_string!("Copy"), Some(sel!(copy:)), has_sel));
        items.push((ns_string!("Paste"), Some(sel!(paste:)), has_clip));
        items.push((ns_string!("Select All"), Some(sel!(selectAll:)), true));
        for (title, action, enabled) in items {
            let Some(action) = action else {
                menu.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            };
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(mtm.alloc(), title, Some(action), ns_string!(""))
            };
            unsafe { item.setTarget(Some(self)) };
            item.setEnabled(enabled);
            menu.addItem(&item);
        }
        self.ivars().borrow_mut().menu_link = link;
        menu
    }

    /// The last right-click menu's items as (title, enabled), and how many menus were shown.
    pub fn menu_state(&self) -> (u32, Vec<(String, bool)>) {
        let st = self.ivars().borrow();
        let items = st
            .menu
            .as_ref()
            .map(|m| m.itemArray().iter().map(|i| (i.title().to_string(), i.isEnabled())).collect())
            .unwrap_or_default();
        (st.menus_shown, items)
    }

    pub fn cancel_menu(&self) {
        let menu = self.ivars().borrow().menu.clone();
        if let Some(m) = menu {
            m.cancelTrackingWithoutAnimation();
        }
    }

    pub fn link_opened(&self) -> Option<String> {
        self.ivars().borrow().link_opened.clone()
    }

    /// Test hooks: what the view thinks the screen and the drag are doing.
    pub(super) fn mouse_mode_for_test(&self) -> (u16, bool) {
        self.mouse_mode()
    }

    pub(super) fn screen_shape_for_test(&self) -> (bool, usize) {
        let st = self.ivars().borrow();
        let s = st.shared.lock().unwrap();
        (s.term.in_alt_screen(), s.term.scrollback_len())
    }

    pub(super) fn selecting_for_test(&self) -> bool {
        self.ivars().borrow().selecting
    }

    /// Lines gathered from a full-screen program, ticks where nothing moved, and autoscroll
    /// ticks in total (the self-test reads all three).
    pub(super) fn harvest_state(&self) -> (usize, u32, u64) {
        let st = self.ivars().borrow();
        let (n, stuck) = st.harvest.as_ref().map(|h| (h.lines.len(), h.stuck)).unwrap_or((0, 0));
        (n, stuck, st.ticks)
    }

    pub fn has_selection(&self) -> bool {
        let st = self.ivars().borrow();
        st.harvest.as_ref().is_some_and(|h| !h.lines.is_empty()) || matches!(st.sel, Some((a, b)) if a != b)
    }

    /// How many lines the view is scrolled back from the live screen (the self-test reads it).
    pub(super) fn scroll_offset(&self) -> usize {
        self.ivars().borrow().scroll_offset
    }

    /// The web link under a point in the text area, following it across wrapped rows.
    fn link_at(&self, x: f32, y: f32) -> Option<String> {
        if !self.ivars().borrow().layout.text.contains(x, y) {
            return None;
        }
        let (line_i, col) = self.cell_at(x, y);
        let st = self.ivars().borrow();
        let s = st.shared.lock().unwrap();
        let term = &s.term;
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
        self.ivars().borrow_mut().link_opened = Some(link.to_string());
        if std::env::var_os("TRINIDAD_HEAD_SELFTEST").is_some() {
            return;
        }
        if let Some(url) = NSURL::URLWithString(&NSString::from_str(link)) {
            NSWorkspace::sharedWorkspace().openURL(&url);
        }
    }

    fn paste(&self) {
        let Some(text) = NSPasteboard::generalPasteboard().stringForType(unsafe { NSPasteboardTypeString }) else {
            return;
        };
        self.paste_text(&text.to_string());
    }

    /// Type what a drag carries: files as escaped paths separated by spaces, else its text.
    pub fn drop_pasteboard(&self, pb: &NSPasteboard) -> bool {
        let mut paths = Vec::new();
        for item in pb.pasteboardItems().map(|a| a.to_vec()).unwrap_or_default() {
            let Some(s) = item.stringForType(unsafe { NSPasteboardTypeFileURL }) else { continue };
            // Finder hands over file reference URLs (file:///.file/id=...); resolve them.
            let path = NSURL::URLWithString(&s)
                .and_then(|u| u.filePathURL())
                .and_then(|u| u.path());
            if let Some(path) = path {
                paths.push(shell_escape(&path.to_string()));
            }
        }
        if !paths.is_empty() {
            self.paste_text(&(paths.join(" ") + " "));
            return true;
        }
        match pb.stringForType(unsafe { NSPasteboardTypeString }) {
            Some(text) => {
                self.paste_text(&text.to_string());
                true
            }
            None => false,
        }
    }

    fn paste_text(&self, text: &str) {
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        let bracketed = {
            let st = self.ivars().borrow();
            let v = st.shared.lock().unwrap().term.bracketed_paste;
            v
        };
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

    fn copy_selection(&self) {
        let text = {
            let st = self.ivars().borrow();
            if let Some(h) = st.harvest.as_ref() {
                // Gathered out of a full-screen program a screen at a time.
                h.text()
            } else {
                let Some((a, b)) = st.sel else { return };
                let s = st.shared.lock().unwrap();
                s.term.text_between(a, b)
            }
        };
        if text.is_empty() {
            return;
        }
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        pb.setString_forType(&NSString::from_str(&text), unsafe { NSPasteboardTypeString });
    }

    fn point(&self, event: &NSEvent) -> (f32, f32) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        (p.x as f32, p.y as f32)
    }

    fn cell_at(&self, x: f32, y: f32) -> (usize, usize) {
        let st = self.ivars().borrow();
        let t = st.layout.text;
        let s = st.shared.lock().unwrap();
        let offset = st.scroll_offset.min(s.term.scrollback_len());
        let row = ((y - t.t) as f64 / st.cell_h).floor().clamp(0.0, (s.term.rows() - 1) as f64) as usize;
        let col = ((x - t.l) as f64 / st.cell_w).floor().clamp(0.0, (s.term.cols() - 1) as f64) as usize;
        (s.term.scrollback_len() - offset + row, col)
    }

    /// The screen cell (row, col) under a point, clamped to the grid.
    fn screen_cell(&self, x: f32, y: f32) -> (usize, usize) {
        let st = self.ivars().borrow();
        let t = st.layout.text;
        let s = st.shared.lock().unwrap();
        let row = ((y - t.t) as f64 / st.cell_h).floor().clamp(0.0, (s.term.rows() - 1) as f64) as usize;
        let col = ((x - t.l) as f64 / st.cell_w).floor().clamp(0.0, (s.term.cols() - 1) as f64) as usize;
        (row, col)
    }

    /// Whether the program in the terminal wants mouse events (and at what level).
    fn mouse_mode(&self) -> (u16, bool) {
        let st = self.ivars().borrow();
        let s = st.shared.lock().unwrap();
        (s.term.mouse_tracking, s.term.mouse_sgr)
    }

    fn mouse_mods(event: &NSEvent) -> u8 {
        let f = event.modifierFlags();
        (if f.contains(NSEventModifierFlags::Shift) { 4 } else { 0 })
            | (if f.contains(NSEventModifierFlags::Option) { 8 } else { 0 })
            | (if f.contains(NSEventModifierFlags::Control) { 16 } else { 0 })
    }

    /// Send one mouse report in whichever encoding the program asked for.
    fn report_mouse(&self, button: u8, cell: (usize, usize), pressed: bool) {
        let (_, sgr) = self.mouse_mode();
        let bytes = if sgr {
            core_vt::sgr_mouse(button, cell.1, cell.0, pressed)
        } else {
            // The old X10 form can't say which button was released, and stops at column 223.
            let b = if pressed { button } else { (button & !3) | 3 };
            let enc = |v: usize| (32 + (v + 1).min(223)) as u8;
            vec![0x1b, b'[', b'M', 32 + b, enc(cell.1), enc(cell.0)]
        };
        let st = self.ivars().borrow();
        let _ = st.pty.lock().unwrap().write(&bytes);
    }

    /// Focus in/out reports, when the program asked for them.
    pub fn focus_changed(&self, focused: bool) {
        let st = self.ivars().borrow();
        let wants = st.shared.lock().unwrap().term.focus_events;
        if wants {
            let _ = st.pty.lock().unwrap().write(if focused { b"\x1b[I" } else { b"\x1b[O" });
        }
    }

    fn on_mouse_down(&self, event: &NSEvent) {
        let (x, y) = self.point(event);
        let (hit, button, in_body) = {
            let st = self.ivars().borrow();
            (st.layout.hit(x, y), st.layout.button_at(x, y), st.layout.in_body(x, y))
        };
        if let Some(b) = button {
            self.ivars().borrow_mut().pressed = Some(b);
            self.setNeedsDisplay(true);
            return;
        }
        match hit {
            Hit::Caption => {
                if event.clickCount() == 2 {
                    self.toggle_zoom();
                } else if let Some(w) = self.window() {
                    w.performWindowDragWithEvent(event);
                }
            }
            Hit::Client => {
                let flags = event.modifierFlags();
                let shift = flags.contains(NSEventModifierFlags::Shift);
                let in_text = self.ivars().borrow().layout.text.contains(x, y);
                if in_text && flags.contains(NSEventModifierFlags::Command) {
                    // Cmd+click opens a link.
                    if let Some(link) = self.link_at(x, y) {
                        self.open_link(&link);
                    }
                    return;
                }
                if flags.contains(NSEventModifierFlags::Control) {
                    // Control-click is a right-click on a Mac.
                    self.context_menu(event);
                    return;
                }
                if in_text && !shift && self.mouse_mode().0 != 0 {
                    // The program handles clicks (e.g. Claude Code's full-screen view), but a
                    // drag still selects text here. Hold the press until we know which it is.
                    let cell = self.screen_cell(x, y);
                    let p = self.cell_at(x, y);
                    let mut st = self.ivars().borrow_mut();
                    st.pending_press = Some((cell, Self::mouse_mods(event), p));
                    st.sel = None;
                    st.harvest = None;
                    drop(st);
                    self.setNeedsDisplay(true);
                    return;
                }
                if in_body {
                    let p = self.cell_at(x, y);
                    let mut st = self.ivars().borrow_mut();
                    st.sel = Some((p, p));
                    st.selecting = true;
                    st.drag_pt = (x, y);
                    st.harvest = None;
                    drop(st);
                    self.setNeedsDisplay(true);
                }
            }
            edge => {
                if let Some(w) = self.window() {
                    let start = w.convertPointToScreen(event.locationInWindow());
                    self.ivars().borrow_mut().resizing = Some((edge, start, w.frame()));
                }
            }
        }
    }

    /// Whether the program on screen owns what we would be scrolling to. Claude Code and any
    /// other full-screen program keep their own history and ask for the mouse; the rows above
    /// and below are theirs, not in our scrollback.
    fn program_owns_screen(&self) -> bool {
        let st = self.ivars().borrow();
        let s = st.shared.lock().unwrap();
        s.term.mouse_tracking != 0 && (s.term.in_alt_screen() || s.term.scrollback_len() == 0)
    }

    fn visible_rows(&self) -> Vec<String> {
        let st = self.ivars().borrow();
        let s = st.shared.lock().unwrap();
        (0..s.term.rows()).map(|r| s.term.row_text(r)).collect()
    }

    /// The selected text as lines, for seeding a harvest with what is already on screen.
    fn selection_lines(&self) -> Vec<String> {
        let st = self.ivars().borrow();
        let Some((a, b)) = st.sel else { return Vec::new() };
        let s = st.shared.lock().unwrap();
        s.term.text_between(a, b).lines().map(|l| l.to_string()).collect()
    }

    /// While harvesting, everything on screen is part of the selection: the anchor scrolled
    /// away long ago, so highlighting a range from it would be a lie.
    fn select_whole_screen(&self) {
        let mut st = self.ivars().borrow_mut();
        let (base, rows, cols) = {
            let s = st.shared.lock().unwrap();
            (s.term.scrollback_len(), s.term.rows(), s.term.cols())
        };
        st.sel = Some(((base, 0), (base + rows.saturating_sub(1), cols.saturating_sub(1))));
    }

    /// One step of selecting through a full-screen program: bank whatever it uncovered since
    /// last time, then ask it for one more notch. Returns false when it has stopped moving.
    fn harvest_step(&self, up: bool) -> bool {
        let now = self.visible_rows();
        let rows = now.len();
        if rows == 0 {
            return false;
        }
        let fresh = self.ivars().borrow().harvest.is_none();
        let seed = if fresh { self.selection_lines() } else { Vec::new() };
        let keep_going = {
            let mut st = self.ivars().borrow_mut();
            match st.harvest.as_mut() {
                None => {
                    st.harvest = Some(Harvest::new(up, now, seed));
                    true
                }
                Some(h) => h.absorb(now),
            }
        };
        self.select_whole_screen();
        if keep_going {
            // The program asked for the mouse, so a wheel report is what it expects.
            let cell = self.screen_cell(self.ivars().borrow().drag_pt.0, self.ivars().borrow().drag_pt.1);
            self.report_mouse(if up { 64 } else { 65 }, cell, true);
        }
        keep_going
    }

    /// Lines to scroll per tick while a drag is held past an edge of the text area:
    /// none while the pointer is inside it, then faster the further out it goes.
    /// Positive runs back up into history, negative comes forward toward the live screen.
    fn autoscroll_lines(&self, y: f32) -> i64 {
        let st = self.ivars().borrow();
        let t = st.layout.text;
        let cell = st.cell_h.max(1.0) as f32;
        let over = if y < t.t {
            t.t - y
        } else if y >= t.b {
            -(y - t.b + 1.0)
        } else {
            return 0;
        };
        let speed = (1.0 + over.abs() / cell).min(8.0) as i64;
        if over > 0.0 {
            speed
        } else {
            -speed
        }
    }

    /// Start or stop the drag-autoscroll for wherever the pointer is now.
    fn update_autoscroll(&self) {
        let (want, running) = {
            let st = self.ivars().borrow();
            (st.selecting, st.autoscroll.is_some())
        };
        let want = want && self.autoscroll_lines(self.ivars().borrow().drag_pt.1) != 0;
        if want == running {
            return;
        }
        if !want {
            self.stop_autoscroll();
            return;
        }
        let timer = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(0.05, self, sel!(autoscrollTick:), None, true)
        };
        unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
        self.ivars().borrow_mut().autoscroll = Some(timer);
    }

    fn stop_autoscroll(&self) {
        let timer = self.ivars().borrow_mut().autoscroll.take();
        if let Some(t) = timer {
            t.invalidate();
        }
    }

    /// One tick of a held drag: move the view a few lines, then stretch the selection to the
    /// pointer. The end point is clamped to the visible grid, so scrolling is what lets the
    /// selection reach text that was never on screen.
    fn autoscroll_step(&self) {
        self.ivars().borrow_mut().ticks += 1;
        let (selecting, (x, y)) = {
            let st = self.ivars().borrow();
            (st.selecting, st.drag_pt)
        };
        let lines = self.autoscroll_lines(y);
        if !selecting || lines == 0 {
            self.stop_autoscroll();
            return;
        }
        if self.program_owns_screen() {
            // The rows we are reaching for belong to the program, not to our scrollback.
            if !self.harvest_step(lines > 0) {
                self.stop_autoscroll();
            }
            self.setNeedsDisplay(true);
            return;
        }
        let moved = {
            let mut st = self.ivars().borrow_mut();
            let max = st.shared.lock().unwrap().term.scrollback_len() as i64;
            let before = st.scroll_offset;
            st.scroll_offset = (before as i64 + lines).clamp(0, max) as usize;
            st.scroll_offset != before
        };
        let p = self.cell_at(x, y);
        let stretched = {
            let mut st = self.ivars().borrow_mut();
            match st.sel.as_mut() {
                Some(sel) if sel.1 != p => {
                    sel.1 = p;
                    true
                }
                _ => false,
            }
        };
        if moved || stretched {
            self.setNeedsDisplay(true);
        }
    }

    fn on_mouse_dragged(&self, event: &NSEvent) {
        let resizing = self.ivars().borrow().resizing;
        if let Some((edge, start, frame)) = resizing {
            // From the event itself (not the live cursor), so scripted drags work too.
            let Some(win) = self.window() else { return };
            let now = win.convertPointToScreen(event.locationInWindow());
            let (dx, dy) = (now.x - start.x, now.y - start.y);
            let (min_w, min_h) = (420.0, 280.0);
            let mut f = frame;
            // Screen coordinates run bottom-up: the window's top edge is origin.y + height.
            let (left, right, top, bottom) = match edge {
                Hit::Left => (true, false, false, false),
                Hit::Right => (false, true, false, false),
                Hit::Top => (false, false, true, false),
                Hit::Bottom => (false, false, false, true),
                Hit::TopLeft => (true, false, true, false),
                Hit::TopRight => (false, true, true, false),
                Hit::BottomLeft => (true, false, false, true),
                Hit::BottomRight => (false, true, false, true),
                _ => (false, false, false, false),
            };
            if right {
                f.size.width = (frame.size.width + dx).max(min_w);
            }
            if left {
                let w = (frame.size.width - dx).max(min_w);
                f.origin.x = frame.origin.x + frame.size.width - w;
                f.size.width = w;
            }
            if top {
                f.size.height = (frame.size.height + dy).max(min_h);
            }
            if bottom {
                let h = (frame.size.height - dy).max(min_h);
                f.origin.y = frame.origin.y + frame.size.height - h;
                f.size.height = h;
            }
            if let Some(w) = self.window() {
                w.setFrame_display(f, true);
            }
            return;
        }
        let pending = self.ivars().borrow().pending_press;
        if let Some((cell, _, p)) = pending {
            let (x, y) = self.point(event);
            if self.screen_cell(x, y) != cell {
                let q = self.cell_at(x, y);
                let mut st = self.ivars().borrow_mut();
                st.pending_press = None;
                st.sel = Some((p, q));
                st.selecting = true;
                st.drag_pt = (x, y);
                drop(st);
                self.update_autoscroll();
                self.setNeedsDisplay(true);
            }
            return;
        }
        if self.ivars().borrow().mouse_reported {
            let (x, y) = self.point(event);
            let cell = self.screen_cell(x, y);
            let (mode, _) = self.mouse_mode();
            let moved = self.ivars().borrow().last_mouse_cell != Some(cell);
            if mode >= 1002 && moved {
                self.report_mouse(32 | Self::mouse_mods(event), cell, true);
                self.ivars().borrow_mut().last_mouse_cell = Some(cell);
            }
            return;
        }
        if self.ivars().borrow().selecting {
            let (x, y) = self.point(event);
            let p = self.cell_at(x, y);
            {
                let mut st = self.ivars().borrow_mut();
                st.drag_pt = (x, y);
                if let Some(sel) = st.sel.as_mut() {
                    sel.1 = p;
                }
            }
            // Dragged past the top or bottom edge: keep scrolling until the button comes up.
            self.update_autoscroll();
            self.setNeedsDisplay(true);
        }
    }

    fn on_mouse_up(&self, event: &NSEvent) {
        let (x, y) = self.point(event);
        self.stop_autoscroll();
        if self.ivars().borrow_mut().resizing.take().is_some() {
            return;
        }
        let pending = self.ivars().borrow_mut().pending_press.take();
        if let Some((cell, mods, _)) = pending {
            // It never moved off its cell: a plain click, which the program gets.
            self.report_mouse(mods, cell, true);
            self.report_mouse(mods, cell, false);
            return;
        }
        if std::mem::take(&mut self.ivars().borrow_mut().mouse_reported) {
            let cell = self.screen_cell(x, y);
            self.report_mouse(Self::mouse_mods(event), cell, false);
            return;
        }
        let selecting = std::mem::take(&mut self.ivars().borrow_mut().selecting);
        if selecting {
            let (sel, harvested) = {
                let st = self.ivars().borrow();
                (st.sel, st.harvest.is_some())
            };
            // The selection stays on screen; copying waits for Cmd+C or the right-click menu.
            if !harvested && !matches!(sel, Some((a, b)) if a != b) {
                self.ivars().borrow_mut().sel = None;
            }
            self.setNeedsDisplay(true);
            return;
        }
        let pressed = self.ivars().borrow_mut().pressed.take();
        if let Some(b) = pressed {
            let still = self.ivars().borrow().layout.button_at(x, y) == Some(b);
            self.setNeedsDisplay(true);
            if still {
                self.press(b);
            }
        }
    }

    fn press(&self, b: Button) {
        match b {
            Button::Close => {
                if let Some(w) = self.window() {
                    w.close();
                }
            }
            Button::Minimize => {
                if let Some(w) = self.window() {
                    w.miniaturize(None);
                }
            }
            Button::Zoom => self.toggle_zoom(),
            Button::Terminal => {}
            Button::Folder => {
                // TRINIDAD_HEAD_FOLDER lets the self-test open a throwaway folder instead of ~.
                let dir = std::env::var("TRINIDAD_HEAD_FOLDER").or_else(|_| std::env::var("HOME"));
                if let Ok(dir) = dir {
                    let url = NSURL::fileURLWithPath(&NSString::from_str(&dir));
                    let ok = NSWorkspace::sharedWorkspace().openURL(&url);
                    self.ivars().borrow_mut().folder_opened = ok;
                }
            }
            Button::Glow => {
                // Changes this window only; the saved default for new windows stays put.
                let mut st = self.ivars().borrow_mut();
                st.glow = (st.glow + 1) % GLOWS.len();
                super::control::register(st.glow, super::window_token());
                drop(st);
                self.setNeedsDisplay(true);
            }
        }
    }

    fn toggle_zoom(&self) {
        let Some(w) = self.window() else { return };
        let restore = self.ivars().borrow_mut().zoom_restore.take();
        match restore {
            Some(frame) => w.setFrame_display(frame, true),
            None => {
                let Some(screen) = w.screen() else { return };
                self.ivars().borrow_mut().zoom_restore = Some(w.frame());
                w.setFrame_display(screen.visibleFrame(), true);
            }
        }
        self.fit();
    }

    fn on_mouse_moved(&self, event: &NSEvent) {
        let (x, y) = self.point(event);
        let (hover, hit, in_text) = {
            let st = self.ivars().borrow();
            (st.layout.button_at(x, y), st.layout.hit(x, y), st.layout.text.contains(x, y))
        };
        if in_text && self.mouse_mode().0 == 1003 {
            let cell = self.screen_cell(x, y);
            if self.ivars().borrow().last_mouse_cell != Some(cell) {
                self.report_mouse(35 | Self::mouse_mods(event), cell, true);
                self.ivars().borrow_mut().last_mouse_cell = Some(cell);
            }
        }
        let changed = {
            let mut st = self.ivars().borrow_mut();
            let c = st.hover != hover;
            st.hover = hover;
            c
        };
        if changed {
            self.setNeedsDisplay(true);
        }
        let pos = match hit {
            Hit::Left => Some(NSCursorFrameResizePosition::Left),
            Hit::Right => Some(NSCursorFrameResizePosition::Right),
            Hit::Top => Some(NSCursorFrameResizePosition::Top),
            Hit::Bottom => Some(NSCursorFrameResizePosition::Bottom),
            Hit::TopLeft => Some(NSCursorFrameResizePosition::TopLeft),
            Hit::TopRight => Some(NSCursorFrameResizePosition::TopRight),
            Hit::BottomLeft => Some(NSCursorFrameResizePosition::BottomLeft),
            Hit::BottomRight => Some(NSCursorFrameResizePosition::BottomRight),
            _ => None,
        };
        let cursor = match pos {
            Some(p) => NSCursor::frameResizeCursorFromPosition_inDirections(p, NSCursorFrameResizeDirections::All),
            None if in_text
                && hover.is_none()
                && event.modifierFlags().contains(NSEventModifierFlags::Command)
                && self.link_at(x, y).is_some() =>
            {
                NSCursor::pointingHandCursor()
            }
            None if in_text && hover.is_none() => NSCursor::IBeamCursor(),
            None => NSCursor::arrowCursor(),
        };
        cursor.set();
    }

    fn on_scroll(&self, event: &NSEvent) {
        // Wheel deltas round to zero on slow clicks; any movement counts as whole notches.
        fn notch(delta: f64) -> f64 {
            if delta == 0.0 {
                0.0
            } else {
                delta.signum() * delta.abs().round().max(1.0)
            }
        }
        let shift = event.modifierFlags().contains(NSEventModifierFlags::Shift);
        let (mode, _) = self.mouse_mode();
        let (x, y) = self.point(event);
        if self.ivars().borrow().selecting && self.program_owns_screen() {
            // Scrolling by hand in the middle of a selection: the program repaints, so bank
            // what it uncovers instead of letting the selection slide onto new text.
            let up = event.scrollingDeltaY() > 0.0;
            self.ivars().borrow_mut().drag_pt = (x, y);
            self.harvest_step(up);
            self.setNeedsDisplay(true);
            return;
        }
        if mode != 0 && !shift {
            // Wheel goes to the program as buttons 64 (up) and 65 (down).
            let cell = self.screen_cell(x, y);
            let steps = {
                let mut st = self.ivars().borrow_mut();
                let delta = event.scrollingDeltaY();
                if event.hasPreciseScrollingDeltas() {
                    st.scroll_accum += delta / st.cell_h;
                    let whole = st.scroll_accum.trunc();
                    st.scroll_accum -= whole;
                    whole
                } else {
                    // A mouse wheel click can report as little as 0.1; still one notch.
                    notch(delta)
                }
            };
            let button = if steps > 0.0 { 64 } else { 65 };
            for _ in 0..(steps.abs() as usize).min(10) {
                self.report_mouse(button | Self::mouse_mods(event), cell, true);
            }
            return;
        }
        let mut st = self.ivars().borrow_mut();
        let delta = event.scrollingDeltaY();
        let lines = if event.hasPreciseScrollingDeltas() {
            st.scroll_accum += delta / st.cell_h;
            let whole = st.scroll_accum.trunc();
            st.scroll_accum -= whole;
            whole
        } else {
            notch(delta) * 3.0
        };
        if lines == 0.0 {
            return;
        }
        let max = st.shared.lock().unwrap().term.scrollback_len() as f64;
        st.scroll_offset = (st.scroll_offset as f64 + lines).clamp(0.0, max) as usize;
        drop(st);
        self.setNeedsDisplay(true);
    }

    fn scroll_page(&self, up: bool) {
        let mut st = self.ivars().borrow_mut();
        let (rows, max) = {
            let s = st.shared.lock().unwrap();
            (s.term.rows(), s.term.scrollback_len())
        };
        let page = rows.saturating_sub(1);
        st.scroll_offset = if up { (st.scroll_offset + page).min(max) } else { st.scroll_offset.saturating_sub(page) };
        drop(st);
        self.setNeedsDisplay(true);
    }

    pub(super) fn layout(&self) -> Layout {
        self.ivars().borrow().layout
    }

    pub(super) fn shared(&self) -> Arc<Mutex<Shared>> {
        self.ivars().borrow().shared.clone()
    }

    pub(super) fn glow(&self) -> usize {
        self.ivars().borrow().glow
    }

    pub(super) fn cell_size(&self) -> (f64, f64) {
        let st = self.ivars().borrow();
        (st.cell_w, st.cell_h)
    }

    pub(super) fn frames(&self) -> u64 {
        self.ivars().borrow().frames
    }

    #[allow(dead_code)]
    pub(super) fn folder_opened(&self) -> bool {
        self.ivars().borrow().folder_opened
    }

    fn draw(&self) {
        self.ivars().borrow_mut().frames += 1;
        let focused = self.focused();
        let Some(ctx) = NSGraphicsContext::currentContext() else { return };
        let cg = ctx.CGContext();
        let size = self.bounds().size;
        let mut st = self.ivars().borrow_mut();
        let l = st.layout;
        let glow = GLOWS[st.glow];
        paint::clear(&cg, size.width, size.height);
        paint::chrome(
            &cg,
            &l,
            &paint::ChromeState { glow: st.glow, focused, hover: st.hover, pressed: st.pressed },
        );
        self.draw_sidebar_icons(&st, focused);

        let shared = st.shared.clone();
        let s = shared.lock().unwrap();
        let term = &s.term;
        let (cw, ch) = (st.cell_w, st.cell_h);
        let (left, top) = (l.text.l as f64, l.text.t as f64);
        let offset = st.scroll_offset.min(term.scrollback_len());
        let default_fg = rgba(theme::TEXT, 1.0);
        let sel = st.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
        let (ur, ug, ub) = st.user_bar;
        let is_bar = |c: Color| matches!(c, Color::Rgb(r, g, b) if theme::is_prompt_bar((r, g, b), (ur, ug, ub)));
        let bar_hex = (ur as u32) << 16 | (ug as u32) << 8 | ub as u32;

        // Claude's prompt bar: one rounded see-through pill per block of rows, with the text
        // inside drawn one size bigger (same rules as the Windows build).
        let bar_span = |row: usize| -> Option<(usize, usize)> {
            let line = term.line(row, offset);
            let first = line.iter().position(|c| is_bar(c.attrs.bg))?;
            let last = line.iter().rposition(|c| is_bar(c.attrs.bg))?;
            Some((first, last + 1))
        };
        let text_end = |row: usize, from: usize| -> usize {
            let line = term.line(row, offset);
            line.iter()
                .enumerate()
                .rev()
                .find(|(i, c)| *i >= from && is_bar(c.attrs.bg) && c.ch != ' ' && !c.spacer)
                .map(|(i, _)| i + 1)
                .unwrap_or(from)
        };
        let mut row_scale: Vec<Option<(f64, f64)>> = vec![None; term.rows()];
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
                    Some(sp) => {
                        span = (span.0.min(sp.0), span.1.max(sp.1));
                        row += 1;
                    }
                    None => break,
                }
            }
            let used = (start_row..row).map(|r| text_end(r, span.0)).max().unwrap_or(span.0);
            let room = (span.1 - span.0) as f64 - 1.0;
            let need = (used - span.0).max(1) as f64;
            let sx = (theme::USER_TEXT_SCALE as f64).min(room / need).max(1.0);
            for r in start_row..row {
                row_scale[r] = Some((left + span.0 as f64 * cw, sx));
            }
            let t = top + start_row as f64 * ch;
            let b = top + row as f64 * ch;
            let radius = ((b - t) / 2.0).min(16.0);
            let pill = crate::layout::Rect {
                l: (left + span.0 as f64 * cw - 4.0) as f32,
                t: (t + 1.0) as f32,
                r: (left + span.1 as f64 * cw + 4.0) as f32,
                b: (b - 1.0) as f32,
            };
            paint::rounded_fill(&cg, pill, radius, rgba(bar_hex, theme::USER_BAR_ALPHA as f64));
            paint::rounded_stroke(&cg, pill, radius, 1.0, rgba(bar_hex, 0.35));
        }

        for row in 0..term.rows() {
            let line = term.line(row, offset);
            let y = top + row as f64 * ch;
            if let Some((sa, sb)) = sel {
                let abs = term.scrollback_len() - offset + row;
                if abs >= sa.0 && abs <= sb.0 {
                    let c0 = if abs == sa.0 { sa.1 } else { 0 };
                    let c1 = if abs == sb.0 { sb.1 + 1 } else { term.cols() };
                    paint::fill_rect(&cg, left + c0 as f64 * cw, y, left + c1 as f64 * cw, y + ch, rgba(glow.colors[1], 0.35));
                }
            }
            let mut col = 0;
            while col < line.len() {
                let start = col;
                let attrs = line[col].attrs;
                let wide = line[col].wide;
                let mut text = String::new();
                while col < line.len() && line[col].attrs == attrs && (col == start || !line[col].wide) {
                    let c: &Cell = &line[col];
                    if !c.spacer {
                        text.push(c.ch);
                    }
                    col += 1;
                    if wide {
                        if col < line.len() && line[col].spacer {
                            col += 1;
                        }
                        break;
                    }
                }
                let user_bar = matches!(attrs.bg, Color::Rgb(r, g, b) if theme::is_prompt_bar((r, g, b), (ur, ug, ub)));
                let (mut fg, bg) = colors(&attrs, default_fg);
                let mut run_attrs = attrs;
                if user_bar {
                    fg = rgba(theme::USER_TEXT, 1.0);
                    run_attrs.bold = true;
                }
                let x0 = left + start as f64 * cw;
                let x1 = left + col as f64 * cw;
                if let Some(bg) = bg.filter(|_| !user_bar) {
                    paint::fill_rect(&cg, x0, y, x1, y + ch, bg);
                }
                if text.chars().any(|c| c != ' ') {
                    if user_bar {
                        // One size bigger, grown from the pill's left edge and the row's middle.
                        let (pl, sx) = row_scale[row].unwrap_or((x0, 1.0));
                        let sy = theme::USER_TEXT_SCALE as f64;
                        let cy = y + ch / 2.0;
                        CGContext::save_g_state(Some(&cg));
                        CGContext::translate_ctm(Some(&cg), pl, cy);
                        CGContext::scale_ctm(Some(&cg), sx, sy);
                        CGContext::translate_ctm(Some(&cg), -pl, -cy);
                        draw_run(&st.fonts, &text, &run_attrs, fg, x0, y);
                        CGContext::restore_g_state(Some(&cg));
                    } else {
                        draw_run(&st.fonts, &text, &attrs, fg, x0, y);
                    }
                }
                if attrs.underline {
                    paint::fill_rect(&cg, x0, y + ch - 2.0, x1, y + ch - 1.0, fg);
                }
            }
        }

        // Cursor in the glow color; input-method text shows at the cursor while composing.
        if term.cursor_visible && offset == 0 {
            let (cr, cc) = term.cursor();
            let x = left + cc as f64 * cw;
            let y = top + cr as f64 * ch;
            let cell = term.line(cr, 0)[cc];
            let width = if cell.wide { 2.0 * cw } else { cw };
            let accent = rgba(glow.accent(), 1.0);
            if !st.marked.is_empty() {
                // Dictation / input-method text wraps inside the window instead of running off
                // the edge: the first line starts at the cursor, the rest at the left margin,
                // and the block moves up if it would drop below the bottom.
                let cols = term.cols().max(1);
                let room = cols - cc.min(cols);
                let (start_col, first) = if room >= 12 { (cc, room) } else { (0, cols) };
                let start_row = if room >= 12 { cr } else { cr + 1 };
                let lines = crate::textutil::wrap(&st.marked, first, cols);
                let last_row = start_row + lines.len();
                let shift = last_row.saturating_sub(term.rows());
                let base = start_row.saturating_sub(shift);
                let body = rgba(theme::BODY, 1.0);
                for (i, line) in lines.iter().enumerate() {
                    let lx = left + if i == 0 { start_col } else { 0 } as f64 * cw;
                    let ly = top + (base + i) as f64 * ch;
                    let w = line.chars().count().max(1) as f64 * cw;
                    let full = left + cols as f64 * cw;
                    paint::fill_rect(&cg, lx, ly, if i + 1 < lines.len() { full } else { lx + w }, ly + ch, body);
                    draw_run(&st.fonts, line, &Attrs::default(), default_fg, lx, ly);
                    paint::fill_rect(&cg, lx, ly + ch - 2.0, lx + w, ly + ch - 1.0, accent);
                }
            } else if focused {
                paint::fill_rect(&cg, x, y + 1.0, x + width, y + ch - 1.0, accent);
                if cell.ch != ' ' {
                    draw_run(&st.fonts, &cell.ch.to_string(), &cell.attrs, rgba(theme::BODY, 1.0), x, y);
                }
            } else {
                paint::stroke_rect(&cg, x, y + 1.0, x + width, y + ch - 1.0, accent);
            }
        }

        // Scrolled back: a thin bar on the right shows where we are.
        if offset > 0 {
            let total = (term.scrollback_len() + term.rows()) as f64;
            let th = l.text.h() as f64;
            let t = top + (term.scrollback_len() - offset) as f64 / total * th;
            let len = (term.rows() as f64 / total * th).max(10.0);
            let bar = crate::layout::Rect::new(l.text.r + 10.0, t as f32, 3.0, len as f32);
            paint::rounded_fill(&cg, bar, 1.5, rgba(glow.accent(), 0.5));
        }

        let now = Instant::now();
        let last_output = s.last_output;
        // Test hook: write every frame (throttling could leave the last frame unwritten).
        let dump = st.dump_path.is_some();
        let dump_text = if dump {
            let mut out = String::new();
            for r in 0..term.rows() {
                out.push_str(&term.row_text(r));
                out.push('\n');
            }
            let (cr, cc) = term.cursor();
            out.push_str(&format!("--\ncursor {cr},{cc} size {}x{} title {:?}\n", term.cols(), term.rows(), term.title));
            Some(out)
        } else {
            None
        };
        drop(s);
        st.meter.frame(last_output, now);
        if !st.first_frame_logged && last_output.is_some() {
            st.first_frame_logged = true;
            if let Some(home) = std::env::var_os("HOME") {
                let p = std::path::Path::new(&home).join("Library/Application Support/TrinidadHead/startup.log");
                let _ = std::fs::write(
                    p,
                    format!("first shell output on screen after {:.0} ms\n", (now - st.started).as_secs_f64() * 1000.0),
                );
            }
        }
        if let (Some(mut out), Some(path)) = (dump_text, st.dump_path.clone()) {
            st.last_dump = now;
            if let Some((p50, p95, n)) = st.meter.stats() {
                out.push_str(&format!("typing delay median {p50:.1} ms, p95 {p95:.1} ms over {n} keys\n"));
            }
            let _ = std::fs::write(path, out);
        }
    }

    fn draw_sidebar_icons(&self, st: &ViewState, focused: bool) {
        let l = st.layout;
        let glow = GLOWS[st.glow];
        let _ = focused;
        for (b, symbol) in [(Button::Terminal, "terminal"), (Button::Folder, "folder"), (Button::Glow, "paintpalette")] {
            if !crate::layout::SIDEBAR.contains(&b) {
                continue;
            }
            let (x, y, r) = l.button(b);
            let (x, y, r) = (x as f64, y as f64, r as f64);
            if b == Button::Terminal || st.hover == Some(b) {
                let a = if b == Button::Terminal { 0.10 } else { 0.07 };
                let Some(ctx) = NSGraphicsContext::currentContext() else { return };
                let cg = ctx.CGContext();
                let rect = crate::layout::Rect::new((x - r) as f32, (y - r) as f32, (2.0 * r) as f32, (2.0 * r) as f32);
                paint::rounded_fill(&cg, rect, 9.0, rgba(0xFFFFFF, a));
            }
            let color = match b {
                Button::Terminal => rgba(0xFFFFFF, 1.0),
                Button::Glow => rgba(glow.accent(), 1.0),
                _ => rgba(if st.hover == Some(b) { 0xFFFFFF } else { theme::ICON }, 1.0),
            };
            let Some(image) =
                NSImage::imageWithSystemSymbolName_accessibilityDescription(&NSString::from_str(symbol), None)
            else {
                continue;
            };
            let palette = NSArray::from_retained_slice(&[ns_color(color)]);
            let config = NSImageSymbolConfiguration::configurationWithPaletteColors(&palette);
            let image = image.imageWithSymbolConfiguration(&config).unwrap_or(image);
            let size = image.size();
            let scale = (15.0 / size.width.max(size.height)).min(1.2);
            let (w, h) = (size.width * scale, size.height * scale);
            let rect = NSRect::new(NSPoint::new(x - w / 2.0, y - h / 2.0), NSSize::new(w, h));
            unsafe {
                image.drawInRect_fromRect_operation_fraction_respectFlipped_hints(
                    rect,
                    NSRect::ZERO,
                    NSCompositingOperation::SourceOver,
                    1.0,
                    true,
                    None,
                );
            }
        }
    }
}

fn colors(a: &Attrs, default_fg: [f64; 4]) -> ([f64; 4], Option<[f64; 4]>) {
    let fg = cell_color(a.fg, a.bold);
    let bg = cell_color(a.bg, false);
    if a.inverse {
        (bg.unwrap_or(rgba(theme::BODY, 1.0)), Some(fg.unwrap_or(default_fg)))
    } else {
        (faint(fg.unwrap_or(default_fg), a.dim), bg)
    }
}

/// SGR 2 text is drawn at a third of its brightness, so Claude Code's ghost
/// suggestion reads as a hint instead of competing with what Matt typed.
fn faint(c: [f64; 4], dim: bool) -> [f64; 4] {
    if dim { [c[0] * DIM, c[1] * DIM, c[2] * DIM, c[3]] } else { c }
}

const DIM: f64 = 1.0 / 3.0;

fn draw_run(fonts: &[Retained<NSFont>], text: &str, attrs: &Attrs, color: [f64; 4], x: f64, y: f64) {
    let font = &fonts[attrs.bold as usize + 2 * attrs.italic as usize];
    let color = ns_color(color);
    unsafe {
        let dict: Retained<NSDictionary<NSAttributedStringKey, AnyObject>> = NSDictionary::from_slices(
            &[NSFontAttributeName, NSForegroundColorAttributeName],
            &[font.as_ref() as &AnyObject, color.as_ref() as &AnyObject],
        );
        NSString::from_str(text).drawAtPoint_withAttributes(NSPoint::new(x, y), Some(&dict));
    }
}
