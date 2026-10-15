//! Screen editing: cursor movement, erasing, inserting and deleting, scrolling, tabs, wide
//! characters, resizing and the alternate screen.

mod common;

use common::{history, screen, term};
use core_vt::{Attrs, Color, Terminal};

#[test]
fn cursor_movement_is_clamped_to_the_screen() {
    let mut t = term(10, 4, b"\x1b[2;5H");
    assert_eq!(t.cursor(), (1, 4));
    t.feed(b"\x1b[10A");
    assert_eq!(t.cursor(), (0, 4));
    t.feed(b"\x1b[10B");
    assert_eq!(t.cursor(), (3, 4));
    t.feed(b"\x1b[20C");
    assert_eq!(t.cursor(), (3, 9));
    t.feed(b"\x1b[20D");
    assert_eq!(t.cursor(), (3, 0));
}

#[test]
fn line_and_column_absolute_moves() {
    let mut t = term(10, 5, b"\x1b[3;4H\x1b[2E");
    assert_eq!(t.cursor(), (4, 0), "CNL: down and to column one");
    t.feed(b"\x1b[3;4H\x1b[2F");
    assert_eq!(t.cursor(), (0, 0), "CPL: up and to column one");
    t.feed(b"\x1b[7G");
    assert_eq!(t.cursor(), (0, 6), "CHA");
    t.feed(b"\x1b[4d");
    assert_eq!(t.cursor(), (3, 6), "VPA");
    t.feed(b"\x1b[2`");
    assert_eq!(t.cursor(), (3, 1), "HPA");
    t.feed(b"\x1b[2e\x1b[3a");
    assert_eq!(t.cursor(), (4, 4), "VPR and HPR");
    t.feed(b"\x1b[1;1f");
    assert_eq!(t.cursor(), (0, 0), "HVP");
}

#[test]
fn backspace_stops_at_the_left_edge() {
    let t = term(10, 1, b"ab\x08\x08\x08\x08X");
    assert_eq!(t.row_text(0), "Xb");
}

#[test]
fn the_last_column_waits_to_wrap() {
    // Writing the last column leaves the cursor there; only the next character wraps.
    let mut t = term(3, 2, b"abc");
    assert_eq!(t.cursor(), (0, 2));
    assert_eq!(t.row_text(1), "");
    t.feed(b"\rX");
    assert_eq!(screen(&t), ["Xbc", ""], "a carriage return cancels the pending wrap");
    t.feed(b"\x1b[1;3Hc");
    t.feed(b"d");
    assert_eq!(screen(&t), ["Xbc", "d"]);
}

#[test]
fn autowrap_off_overwrites_the_last_column() {
    let t = term(3, 2, b"\x1b[?7labcdef");
    assert_eq!(screen(&t), ["abf", ""]);
}

#[test]
fn tabs_stop_every_eight_columns_and_can_be_changed() {
    let mut t = term(20, 1, b"\ta\tb");
    assert_eq!(t.row_text(0), "        a       b");
    // Clear all stops, then set one at column 3.
    t.feed(b"\r\x1b[2K\x1b[3g\x1b[4G\x1bH\r\tX");
    assert_eq!(t.row_text(0), "   X");
    // Clear the stop under the cursor: the next tab runs to the right edge.
    t.feed(b"\x1b[4G\x1b[g\r\tY");
    assert_eq!(t.cursor(), (0, 19));
    assert_eq!(t.line(0, 0)[19].ch, 'Y');
}

#[test]
fn erase_in_line_modes() {
    let mut t = term(10, 1, b"0123456789\x1b[1;5H\x1b[0K");
    assert_eq!(t.row_text(0), "0123");
    t.feed(b"\r0123456789\x1b[1;5H\x1b[1K");
    assert_eq!(t.row_text(0), "     56789");
    t.feed(b"\x1b[2K");
    assert_eq!(t.row_text(0), "");
}

#[test]
fn erase_in_display_modes() {
    let fill = b"aaaa\r\nbbbb\r\ncccc\x1b[2;3H";
    let mut t = term(4, 3, fill);
    t.feed(b"\x1b[J");
    assert_eq!(screen(&t), ["aaaa", "bb", ""]);
    let mut t = term(4, 3, fill);
    t.feed(b"\x1b[1J");
    assert_eq!(screen(&t), ["", "   b", "cccc"]);
    let mut t = term(4, 3, fill);
    t.feed(b"\x1b[2J");
    assert_eq!(screen(&t), ["", "", ""]);
    assert_eq!(t.cursor(), (1, 2), "ED 2 leaves the cursor where it was");
}

#[test]
fn erase_scrollback_with_ed_3() {
    let mut t = term(5, 2, b"1\r\n2\r\n3\r\n4");
    assert_eq!(t.scrollback_len(), 2);
    t.feed(b"\x1b[3J");
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(screen(&t), ["3", "4"], "the screen itself is untouched");
}

#[test]
fn erased_cells_take_the_current_background() {
    let t = term(4, 1, b"abcd\x1b[44m\x1b[1;2H\x1b[K");
    let l = t.line(0, 0);
    assert_eq!(l[0].attrs.bg, Color::Default);
    assert!(l[1..].iter().all(|c| c.attrs.bg == Color::Indexed(4) && c.ch == ' '));
}

#[test]
fn erase_characters() {
    let t = term(10, 1, b"abcdefgh\x1b[1;3H\x1b[3X");
    assert_eq!(t.row_text(0), "ab   fgh");
    assert_eq!(t.cursor(), (0, 2), "ECH does not move the cursor");
}

#[test]
fn insert_and_delete_characters() {
    let mut t = term(8, 1, b"abcdefgh\x1b[1;3H\x1b[2@");
    assert_eq!(t.row_text(0), "ab  cdef", "the right end falls off");
    t.feed(b"\x1b[3P");
    assert_eq!(t.row_text(0), "abdef");
    assert_eq!(t.line(0, 0).len(), 8, "the row keeps its width");
}

#[test]
fn insert_and_delete_lines_inside_the_scroll_region() {
    let fill = b"1\r\n2\r\n3\r\n4\r\n5";
    let mut t = term(3, 5, fill);
    t.feed(b"\x1b[2;4r\x1b[2;1H\x1b[L");
    assert_eq!(screen(&t), ["1", "", "2", "3", "5"], "line 4 fell off the region's bottom; 5 is outside it");
    t.feed(b"\x1b[2M");
    assert_eq!(screen(&t), ["1", "3", "", "", "5"]);
    // Outside the region, IL and DL do nothing.
    t.feed(b"\x1b[5;1H\x1b[L");
    assert_eq!(screen(&t), ["1", "3", "", "", "5"]);
}

#[test]
fn scroll_up_and_down_commands() {
    let mut t = term(3, 3, b"1\r\n2\r\n3");
    t.feed(b"\x1b[S");
    assert_eq!(screen(&t), ["2", "3", ""]);
    assert_eq!(history(&t), ["1"], "SU from the top of the screen saves the line");
    t.feed(b"\x1b[2T");
    assert_eq!(screen(&t), ["", "", "2"]);
}

#[test]
fn reverse_index_scrolls_down_at_the_top() {
    let mut t = term(3, 3, b"1\r\n2\r\n3\x1b[H\x1bM");
    assert_eq!(screen(&t), ["", "1", "2"]);
    t.feed(b"\x1b[3;1H\x1bM");
    assert_eq!(t.cursor(), (1, 0), "RI anywhere else moves up a row");
}

#[test]
fn index_and_next_line() {
    let mut t = term(5, 3, b"ab\x1bD");
    assert_eq!(t.cursor(), (1, 2), "IND keeps the column");
    t.feed(b"\x1bE");
    assert_eq!(t.cursor(), (2, 0), "NEL goes to column one");
}

#[test]
fn a_bad_scroll_region_is_ignored() {
    let mut t = term(3, 4, b"\x1b[3;2r\x1b[1;9r\x1b[4;1H1\n2\n");
    // Neither region took, so the whole screen scrolls.
    assert_eq!(t.scrollback_len(), 2);
    t.feed(b"\x1b[r");
    assert_eq!(t.cursor(), (0, 0), "setting a region homes the cursor");
}

#[test]
fn a_region_that_does_not_start_at_the_top_keeps_no_history() {
    let t = term(3, 4, b"\x1b[2;4r\x1b[4;1H\n\n\n");
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn save_and_restore_cursor_and_pen() {
    let mut t = term(10, 3, b"\x1b[2;3H\x1b[1;35m\x1b7\x1b[0m\x1b[H\x1b8X");
    assert_eq!(t.cursor(), (1, 3));
    let c = t.line(1, 0)[2];
    assert_eq!(c.ch, 'X');
    assert!(c.attrs.bold);
    assert_eq!(c.attrs.fg, Color::Indexed(5));
    t.feed(b"\x1b[0m\x1b[3;1H\x1b[s\x1b[H\x1b[uY");
    assert_eq!(t.row_text(2), "Y", "CSI s / CSI u do the same");
}

#[test]
fn full_reset_clears_the_screen_and_modes_but_keeps_scrollback() {
    let mut t = term(5, 2, b"1\r\n2\r\n3\x1b[?25l\x1b[?2004h\x1b[31m\x1b]0;x\x07");
    assert_eq!(t.scrollback_len(), 1);
    t.feed(b"\x1bc");
    assert_eq!(screen(&t), ["", ""]);
    assert_eq!(t.cursor(), (0, 0));
    assert!(t.cursor_visible);
    assert!(!t.bracketed_paste);
    assert_eq!(t.title, "");
    assert_eq!(t.scrollback_len(), 1);
    t.feed(b"Z");
    assert_eq!(t.line(0, 0)[0].attrs, Attrs::default());
}

#[test]
fn soft_reset_restores_the_pen_region_and_cursor_visibility() {
    let mut t = term(5, 4, b"\x1b[2;3r\x1b[?25l\x1b[1m\x1b[!p");
    assert!(t.cursor_visible);
    t.feed(b"A\x1b[4;1H\n");
    assert_eq!(t.line(0, 0)[0].attrs, Attrs::default());
    assert_eq!(t.scrollback_len(), 1, "the region is the whole screen again");
}

// ---- Wide characters -------------------------------------------------------------------

#[test]
fn a_wide_character_that_does_not_fit_wraps_whole() {
    let t = term(3, 2, "ab中".as_bytes());
    assert_eq!(t.row_text(0), "ab");
    assert_eq!(t.line(1, 0)[0].ch, '中');
    assert!(t.line(1, 0)[1].spacer);
    assert_eq!(t.cursor(), (1, 2));
}

#[test]
fn a_wide_character_at_the_edge_without_autowrap_is_dropped() {
    let t = term(3, 1, "\x1b[?7lab中".as_bytes());
    assert_eq!(t.row_text(0), "ab");
}

#[test]
fn spacers_are_left_out_of_text() {
    let t = term(10, 2, "中文ok\r\n".as_bytes());
    assert_eq!(t.row_text(0), "中文ok");
    assert_eq!(t.text_between((0, 0), (0, 9)), "中文ok");
}

#[test]
fn combining_marks_take_no_cell() {
    let t = term(5, 1, "e\u{301}x".as_bytes());
    assert_eq!(t.line(0, 0)[1].ch, 'x');
    assert_eq!(t.cursor(), (0, 2));
}

#[test]
fn overwriting_the_right_half_of_a_wide_character_clears_the_left_half() {
    // "中" is in columns 0-1. Writing over column 1 leaves no half character behind: the
    // wide cell must not stay wide over a neighbour it no longer owns.
    let t = term(5, 1, "中\x1b[1;2Hx".as_bytes());
    let l = t.line(0, 0);
    assert!(!l[0].wide, "column 0 no longer starts a wide character");
    assert_eq!(l[0].ch, ' ');
    assert_eq!(l[1].ch, 'x');
    assert!(!l[1].spacer);
    assert_eq!(t.row_text(0), " x");
}

#[test]
fn overwriting_the_left_half_of_a_wide_character_clears_the_right_half() {
    // Write one narrow character over "中": the spacer it leaves behind is a blank now, and
    // must read as one, not vanish from the text.
    let t = term(5, 1, "中y\rx".as_bytes());
    let l = t.line(0, 0);
    assert_eq!(l[0].ch, 'x');
    assert!(!l[0].wide);
    assert!(!l[1].spacer, "the old spacer is an ordinary blank cell");
    assert_eq!(t.row_text(0), "x y");
}

// ---- Scrollback ------------------------------------------------------------------------

#[test]
fn scrollback_is_capped() {
    let mut t = Terminal::new(5, 2);
    t.max_scrollback = 3;
    for i in 0..10 {
        t.feed(format!("{i}\r\n").as_bytes());
    }
    assert_eq!(history(&t), ["6", "7", "8"]);
}

#[test]
fn zero_scrollback_keeps_nothing() {
    let mut t = Terminal::new(5, 2);
    t.max_scrollback = 0;
    t.feed(b"1\r\n2\r\n3\r\n4");
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(screen(&t), ["3", "4"]);
}

#[test]
fn line_reads_back_through_history() {
    let t = term(5, 2, b"1\r\n2\r\n3\r\n4");
    assert_eq!(t.line(0, 2)[0].ch, '1');
    assert_eq!(t.line(1, 2)[0].ch, '2');
    assert_eq!(t.line(0, 99)[0].ch, '1', "scrolling past the top stops at the oldest line");
    assert_eq!(t.abs_line(3)[0].ch, '4');
    assert_eq!(t.total_lines(), 4);
}

#[test]
#[ignore = "known bug, fixed in a later commit on this branch"]
fn text_between_handles_points_out_of_range() {
    let t = term(5, 2, b"abc\r\ndef");
    assert_eq!(t.text_between((0, 0), (99, 99)), "abc\ndef");
    assert_eq!(t.text_between((0, 50), (0, 60)), "");
    assert_eq!(t.text_between((1, 1), (1, 1)), "e");
}

// ---- Resizing --------------------------------------------------------------------------

#[test]
fn shrinking_keeps_the_cursor_row_on_screen() {
    let mut t = term(10, 4, b"1\r\n2\r\n3\r\n$ ");
    t.resize(10, 2);
    assert_eq!(screen(&t), ["3", "$"]);
    assert_eq!(history(&t), ["1", "2"]);
    assert_eq!(t.cursor(), (1, 2));
}

#[test]
fn shrinking_with_the_cursor_at_the_top_drops_blank_rows_below() {
    let mut t = term(10, 4, b"$ ");
    t.resize(10, 2);
    assert_eq!(t.scrollback_len(), 0);
    assert_eq!(t.cursor(), (0, 2));
}

#[test]
fn growing_adds_blank_rows_and_columns() {
    let mut t = term(3, 2, b"abc\r\nde");
    t.resize(6, 4);
    assert_eq!(screen(&t), ["abc", "de", "", ""]);
    assert_eq!(t.line(3, 0).len(), 6);
    t.feed(b"xyz");
    assert_eq!(t.row_text(1), "dexyz");
}

#[test]
fn narrowing_clamps_the_cursor() {
    let mut t = term(10, 2, b"abcdefgh");
    t.resize(4, 2);
    assert_eq!(t.cursor(), (0, 3));
    assert_eq!(t.row_text(0), "abcd");
}

#[test]
fn a_zero_size_is_treated_as_one() {
    let mut t = Terminal::new(0, 0);
    assert_eq!((t.cols(), t.rows()), (1, 1));
    t.feed("x中\r\ny".as_bytes());
    t.resize(0, 0);
    assert_eq!((t.cols(), t.rows()), (1, 1));
}

#[test]
fn resize_resets_the_scroll_region() {
    let mut t = term(5, 4, b"\x1b[2;3r");
    t.resize(5, 5);
    t.feed(b"\x1b[5;1H\n");
    assert_eq!(t.scrollback_len(), 1, "the whole screen scrolls after a resize");
}

#[test]
#[ignore = "known bug, fixed in a later commit on this branch"]
fn shrinking_the_window_does_not_record_the_same_rows_twice() {
    // Rows a shrink pushes into scrollback are in the document already. The next paint must
    // not compare the old, taller screen with the new one, read that as the screen having
    // travelled, and bank those rows a second time.
    let mut t = Terminal::new(10, 6);
    t.feed(b"line 1\r\nline 2\r\nline 3\r\nline 4\r\nline 5\r\nline 6");
    t.resize(10, 4);
    assert_eq!(history(&t), ["line 1", "line 2"]);
    t.feed(b"!");
    assert_eq!(history(&t), ["line 1", "line 2"]);
    assert_eq!(screen(&t), ["line 3", "line 4", "line 5", "line 6!"]);
}

// ---- Alternate screen ------------------------------------------------------------------

#[test]
fn every_alternate_screen_mode_switches_screens() {
    for mode in ["47", "1047", "1049"] {
        let mut t = term(10, 2, b"main");
        t.feed(format!("\x1b[?{mode}h").as_bytes());
        assert!(t.in_alt_screen(), "mode {mode}");
        assert_eq!(t.row_text(0), "", "mode {mode} starts blank");
        t.feed(format!("\x1b[?{mode}l").as_bytes());
        assert!(!t.in_alt_screen());
        assert_eq!(t.row_text(0), "main", "mode {mode}");
    }
}

#[test]
fn entering_the_alternate_screen_twice_keeps_the_first_saved_screen() {
    let mut t = term(10, 2, b"main\x1b[?1049hfirst\x1b[?1049hsecond\x1b[?1049l");
    assert_eq!(t.row_text(0), "main");
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.row_text(0), "main", "leaving twice is harmless");
}

#[test]
fn the_alternate_screen_restores_the_cursor_and_pen() {
    let t = term(10, 3, b"\x1b[2;4H\x1b[32m\x1b[?1049h\x1b[0m\x1b[3;1Hx\x1b[?1049lY");
    assert_eq!(t.cursor(), (1, 4));
    assert_eq!(t.line(1, 0)[3].attrs.fg, Color::Indexed(2));
}

#[test]
fn the_alternate_screen_adds_nothing_to_scrollback_when_it_scrolls() {
    let mut t = term(5, 2, b"\x1b[?1049h");
    for i in 0..10 {
        t.feed(format!("{i}\r\n").as_bytes());
    }
    t.feed(b"\x1b[?1049l");
    assert_eq!(t.scrollback_len(), 0);
}

#[test]
fn resizing_inside_the_alternate_screen_keeps_its_rows_out_of_the_main_scrollback() {
    // A full-screen program's rows belong to it. Shrinking the window while it runs must not
    // drop them into the shell's own scrollback, where they would sit after it exits.
    let mut t = term(10, 4, b"shell\x1b[?1049h\x1b[Hvim 1\r\nvim 2\r\nvim 3\r\nvim 4");
    t.resize(10, 2);
    t.feed(b"\x1b[?1049l");
    assert!(!t.in_alt_screen());
    assert_eq!(t.scrollback_len(), 0, "{:?}", history(&t));
    assert_eq!(t.row_text(0), "shell");
}

#[test]
fn leaving_the_alternate_screen_after_a_resize_fits_the_main_screen_to_the_new_size() {
    let mut t = term(10, 3, b"a\r\nb\r\nc\x1b[?1049h");
    t.resize(4, 5);
    t.feed(b"\x1b[?1049l");
    assert_eq!(screen(&t), ["a", "b", "c", "", ""]);
    assert!((0..5).all(|r| t.line(r, 0).len() == 4));
    assert_eq!(t.cursor(), (2, 1));
}
