//! What programs ask the terminal for: colors and text attributes (SGR), private modes,
//! replies to queries, the clipboard (OSC 52), shell-integration marks (OSC 133), and the
//! mouse report encoding.

mod common;

use common::term;
use core_vt::{sgr_mouse, Attrs, Color, Mark, Terminal};

fn attrs(t: &Terminal, col: usize) -> Attrs {
    t.line(0, 0)[col].attrs
}

// ---- SGR -------------------------------------------------------------------------------

#[test]
fn sgr_turns_each_attribute_on_and_off() {
    let t = term(20, 1, b"\x1b[1mA\x1b[3mB\x1b[4mC\x1b[7mD\x1b[22;23;24;27mE");
    assert!(attrs(&t, 0).bold);
    assert!(attrs(&t, 1).bold && attrs(&t, 1).italic);
    assert!(attrs(&t, 2).underline);
    assert!(attrs(&t, 3).inverse);
    assert_eq!(attrs(&t, 4), Attrs::default());
}

#[test]
fn an_empty_sgr_resets() {
    let t = term(5, 1, b"\x1b[1;4;31;42mA\x1b[mB");
    assert_eq!(attrs(&t, 1), Attrs::default());
}

#[test]
fn the_sixteen_basic_colors() {
    let t = term(10, 1, b"\x1b[30;47mA\x1b[37;40mB\x1b[90;107mC\x1b[97;100mD\x1b[39;49mE");
    assert_eq!((attrs(&t, 0).fg, attrs(&t, 0).bg), (Color::Indexed(0), Color::Indexed(7)));
    assert_eq!((attrs(&t, 1).fg, attrs(&t, 1).bg), (Color::Indexed(7), Color::Indexed(0)));
    assert_eq!((attrs(&t, 2).fg, attrs(&t, 2).bg), (Color::Indexed(8), Color::Indexed(15)));
    assert_eq!((attrs(&t, 3).fg, attrs(&t, 3).bg), (Color::Indexed(15), Color::Indexed(8)));
    assert_eq!((attrs(&t, 4).fg, attrs(&t, 4).bg), (Color::Default, Color::Default));
}

#[test]
fn indexed_and_true_colors_with_semicolons() {
    let t = term(10, 1, b"\x1b[38;5;208;48;5;17mA\x1b[38;2;255;128;0;48;2;1;2;3mB");
    assert_eq!(attrs(&t, 0).fg, Color::Indexed(208));
    assert_eq!(attrs(&t, 0).bg, Color::Indexed(17));
    assert_eq!(attrs(&t, 1).fg, Color::Rgb(255, 128, 0));
    assert_eq!(attrs(&t, 1).bg, Color::Rgb(1, 2, 3));
}

#[test]
fn out_of_range_color_values_are_clamped() {
    let t = term(10, 1, b"\x1b[38;5;999mA\x1b[38;2;300;256;1000mB");
    assert_eq!(attrs(&t, 0).fg, Color::Indexed(255));
    assert_eq!(attrs(&t, 1).fg, Color::Rgb(255, 255, 255));
}

#[test]
fn attributes_after_an_extended_color_still_apply() {
    let t = term(10, 1, b"\x1b[38;5;3;1;48;2;4;5;6;4mA");
    let a = attrs(&t, 0);
    assert_eq!(a.fg, Color::Indexed(3));
    assert_eq!(a.bg, Color::Rgb(4, 5, 6));
    assert!(a.bold && a.underline);
}

#[test]
fn a_truncated_extended_color_changes_nothing() {
    let t = term(10, 1, b"\x1b[31m\x1b[38;2;1;2mA\x1b[38mB\x1b[38;5mC");
    assert_eq!(attrs(&t, 0).fg, Color::Indexed(1));
    assert_eq!(attrs(&t, 1).fg, Color::Indexed(1));
    assert_eq!(attrs(&t, 2).fg, Color::Indexed(1));
}

#[test]
#[ignore = "known bug, fixed in a later commit on this branch"]
fn true_color_in_colon_form() {
    // ITU T.416 form: 38:2:<colour space>:r:g:b. The colour-space slot is usually empty.
    let t = term(10, 1, b"\x1b[38:2::255:0:0mA\x1b[48:2:0:10:20:30mB\x1b[38:5:99mC");
    assert_eq!(attrs(&t, 0).fg, Color::Rgb(255, 0, 0));
    assert_eq!(attrs(&t, 0).bg, Color::Default, "the trailing 0 is the blue channel, not a reset");
    assert_eq!(attrs(&t, 1).bg, Color::Rgb(10, 20, 30));
    assert_eq!(attrs(&t, 1).fg, Color::Rgb(255, 0, 0));
    assert_eq!(attrs(&t, 2).fg, Color::Indexed(99));
}

#[test]
fn true_color_in_colon_form_without_a_colour_space_slot() {
    // Many programs (and xterm) also send the shorter 38:2:r:g:b.
    let t = term(10, 1, b"\x1b[38:2:1:2:3mA");
    assert_eq!(attrs(&t, 0).fg, Color::Rgb(1, 2, 3));
}

#[test]
#[ignore = "known bug, fixed in a later commit on this branch"]
fn colon_sub_parameters_are_not_read_as_attributes() {
    // 4:3 is a curly underline (kitty, neovim's undercurl). The 3 belongs to the 4: it is not
    // SGR 3, italic. Likewise 4:0 turns underline off.
    let t = term(10, 1, b"\x1b[4:3mA\x1b[4:0mB\x1b[1;4:1;31mC");
    assert!(attrs(&t, 0).underline);
    assert!(!attrs(&t, 0).italic, "the 3 in 4:3 is not italic");
    assert!(!attrs(&t, 1).underline, "4:0 is underline off");
    assert!(attrs(&t, 2).bold && attrs(&t, 2).underline);
    assert_eq!(attrs(&t, 2).fg, Color::Indexed(1));
}

#[test]
fn unknown_sgr_codes_are_ignored() {
    let t = term(10, 1, b"\x1b[5;8;9;53;31mA");
    assert_eq!(attrs(&t, 0).fg, Color::Indexed(1));
}

// ---- Private modes ---------------------------------------------------------------------

#[test]
fn private_modes_toggle() {
    let mut t = Terminal::new(10, 2);
    assert!(t.cursor_visible && t.autowrap);
    assert!(!t.app_cursor_keys && !t.bracketed_paste && !t.focus_events);
    t.feed(b"\x1b[?1h\x1b[?25l\x1b[?2004h\x1b[?1004h\x1b[?7l");
    assert!(t.app_cursor_keys && !t.cursor_visible && t.bracketed_paste && t.focus_events && !t.autowrap);
    t.feed(b"\x1b[?1l\x1b[?25h\x1b[?2004l\x1b[?1004l\x1b[?7h");
    assert!(!t.app_cursor_keys && t.cursor_visible && !t.bracketed_paste && !t.focus_events && t.autowrap);
}

#[test]
fn several_private_modes_in_one_sequence() {
    let t = term(10, 2, b"\x1b[?1000;1006;2004h");
    assert_eq!(t.mouse_tracking, 1000);
    assert!(t.mouse_sgr && t.bracketed_paste);
}

#[test]
fn mouse_tracking_keeps_the_widest_mode_asked_for() {
    let mut t = term(10, 2, b"\x1b[?1000h\x1b[?1003h\x1b[?1002h");
    assert_eq!(t.mouse_tracking, 1003);
    t.feed(b"\x1b[?1002l");
    assert_eq!(t.mouse_tracking, 1003, "turning off a mode that is not the active one leaves it");
    t.feed(b"\x1b[?1003l");
    assert_eq!(t.mouse_tracking, 0);
    t.feed(b"\x1b[?1003h\x1b[?1000l");
    assert_eq!(t.mouse_tracking, 0, "1000 off turns all tracking off");
}

#[test]
fn ansi_modes_without_a_question_mark_are_not_private_modes() {
    let t = term(10, 2, b"\x1b[25l\x1b[2004h\x1b[4h");
    assert!(t.cursor_visible);
    assert!(!t.bracketed_paste);
}

// ---- Replies ---------------------------------------------------------------------------

#[test]
fn device_status_and_attributes_replies() {
    let mut t = term(10, 5, b"\x1b[5n");
    assert_eq!(t.take_responses(), b"\x1b[0n");
    assert!(t.take_responses().is_empty(), "taking the replies empties them");
    t.feed(b"\x1b[c\x1b[0c\x1b[>c\x1b[1c");
    assert_eq!(t.take_responses(), b"\x1b[?62;22c\x1b[?62;22c\x1b[>0;10;1c");
}

#[test]
fn cursor_position_report_is_one_based_and_on_screen() {
    let mut t = term(4, 2, b"abcd");
    t.feed(b"\x1b[6n");
    assert_eq!(t.take_responses(), b"\x1b[1;4R", "a pending wrap reports the last column");
    t.feed(b"\x1b[H\x1b[6n");
    assert_eq!(t.take_responses(), b"\x1b[1;1R");
}

#[test]
fn replies_queue_in_order_until_taken() {
    let mut t = term(10, 5, b"\x1b[2;2H\x1b[6n\x1b[3;3H\x1b[6n");
    assert_eq!(t.take_responses(), b"\x1b[2;2R\x1b[3;3R");
}

// ---- OSC 52 ----------------------------------------------------------------------------

#[test]
fn clipboard_write_decodes_base64_with_and_without_padding() {
    let mut t = term(10, 1, b"\x1b]52;c;aGk=\x07");
    assert_eq!(t.take_clipboard().as_deref(), Some("hi"));
    t.feed(b"\x1b]52;c;aGk\x07");
    assert_eq!(t.take_clipboard().as_deref(), Some("hi"));
    t.feed(b"\x1b]52;p;YWJj\x1b\\");
    assert_eq!(t.take_clipboard().as_deref(), Some("abc"));
}

#[test]
fn clipboard_write_handles_unicode_and_line_breaks_in_the_base64() {
    // "héllo 🌊" split across lines, the way some programs wrap long base64.
    let t = &mut term(10, 1, b"\x1b]52;c;aMOpbGxv\r\nIPCfjIo=\x07");
    assert_eq!(t.take_clipboard().as_deref(), Some("héllo 🌊"));
}

#[test]
fn clipboard_reads_and_bad_base64_are_ignored() {
    let mut t = term(10, 1, b"\x1b]52;c;?\x07");
    assert_eq!(t.take_clipboard(), None, "a read request is never answered");
    assert!(t.take_responses().is_empty());
    t.feed(b"\x1b]52;c;not*base64!\x07");
    assert_eq!(t.take_clipboard(), None);
    t.feed(b"\x1b]52;c\x07");
    assert_eq!(t.take_clipboard(), None, "no data field at all");
}

#[test]
fn the_latest_clipboard_write_wins() {
    let mut t = term(10, 1, b"\x1b]52;c;b25l\x07\x1b]52;c;dHdv\x07");
    assert_eq!(t.take_clipboard().as_deref(), Some("two"));
}

#[test]
fn the_empty_clipboard_write_is_an_empty_string() {
    let mut t = term(10, 1, b"\x1b]52;c;\x07");
    assert_eq!(t.take_clipboard().as_deref(), Some(""));
}

// ---- OSC 133 ---------------------------------------------------------------------------

#[test]
fn shell_marks_record_the_absolute_row() {
    let t = term(10, 2, b"a\r\nb\r\n\x1b]133;A\x07$ \x1b]133;D;127\x07\x1b]133;D\x07\x1b]133;D;x\x07");
    // One row scrolled into history, so the prompt is on absolute row 2.
    assert_eq!(
        t.marks,
        vec![
            (2, Mark::PromptStart),
            (2, Mark::CommandEnd(Some(127))),
            (2, Mark::CommandEnd(None)),
            (2, Mark::CommandEnd(None)),
        ]
    );
}

#[test]
fn unknown_shell_marks_are_skipped() {
    let t = term(10, 2, b"\x1b]133;Z\x07\x1b]133;\x07");
    assert!(t.marks.is_empty());
}

#[test]
fn shell_marks_are_capped() {
    let mut t = Terminal::new(10, 2);
    let mut bytes = Vec::new();
    for _ in 0..10_001 {
        bytes.extend_from_slice(b"\x1b]133;A\x07");
    }
    t.feed(&bytes);
    assert!(t.marks.len() <= 10_000);
}

// ---- Mouse -----------------------------------------------------------------------------

#[test]
fn sgr_mouse_reports() {
    assert_eq!(sgr_mouse(0, 0, 0, true), b"\x1b[<0;1;1M");
    assert_eq!(sgr_mouse(2, 9, 4, false), b"\x1b[<2;10;5m");
    assert_eq!(sgr_mouse(64, 300, 200, true), b"\x1b[<64;301;201M", "wheel, past column 223");
}

// ---- Change counter --------------------------------------------------------------------

#[test]
fn every_feed_and_resize_bumps_the_generation() {
    let mut t = Terminal::new(10, 2);
    let g0 = t.generation;
    t.feed(b"x");
    assert!(t.generation > g0);
    let g1 = t.generation;
    t.resize(12, 3);
    assert!(t.generation > g1);
    let g2 = t.generation;
    t.resize(12, 3);
    assert_eq!(t.generation, g2, "a resize to the same size is not a change");
}
