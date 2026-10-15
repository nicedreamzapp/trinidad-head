//! The byte-level parser: UTF-8, escape sequences, CSI parameters, OSC strings, and input that
//! is broken, split across reads, or hostile.

mod common;

use common::{screen, term};
use core_vt::{Attrs, Color, Terminal};

#[test]
fn every_split_point_of_a_sequence_gives_the_same_screen() {
    // Output arrives in reads of any size, so a sequence can be cut anywhere. The screen must
    // not depend on where.
    let bytes = "a\x1b[1;31mé中\x1b]0;tïtle\x07\x1b[2;3Hz\x1b[38;2;10;20;30mq".as_bytes();
    let whole = term(10, 3, bytes);
    for cut in 0..=bytes.len() {
        let mut t = Terminal::new(10, 3);
        t.feed(&bytes[..cut]);
        t.feed(&bytes[cut..]);
        assert_eq!(screen(&t), screen(&whole), "cut at {cut}");
        assert_eq!(t.title, whole.title, "cut at {cut}");
        assert_eq!(t.cursor(), whole.cursor(), "cut at {cut}");
        for r in 0..3 {
            assert_eq!(t.line(r, 0), whole.line(r, 0), "row {r}, cut at {cut}");
        }
    }
}

#[test]
fn one_byte_at_a_time_matches_one_read() {
    let bytes = "héllo \x1b[7mwörld\x1b[0m 🌊!\r\n\x1b[Knext".as_bytes();
    let whole = term(20, 3, bytes);
    let mut t = Terminal::new(20, 3);
    for b in bytes {
        t.feed(std::slice::from_ref(b));
    }
    assert_eq!(screen(&t), screen(&whole));
}

#[test]
fn invalid_utf8_becomes_replacement_characters() {
    // A stray continuation byte, a lead byte cut short by ASCII, and bytes that never start
    // a character.
    let t = term(10, 1, b"a\x80b\xC3c\xFFd");
    assert_eq!(t.row_text(0), "a\u{FFFD}b\u{FFFD}c\u{FFFD}d");
}

#[test]
fn a_lead_byte_cut_short_by_another_lead_byte_still_decodes_the_second() {
    let t = term(10, 1, b"\xC3\xC3\xA9");
    assert_eq!(t.row_text(0), "\u{FFFD}é");
}

#[test]
fn four_byte_characters_decode() {
    let t = term(10, 1, "🌊x".as_bytes());
    let l = t.line(0, 0);
    assert_eq!(l[0].ch, '🌊');
    assert!(l[0].wide && l[1].spacer);
    assert_eq!(l[2].ch, 'x');
}

#[test]
fn control_characters_are_not_printed() {
    // NUL, BEL, SO, SI, DEL and friends move nothing and draw nothing.
    let t = term(10, 1, b"a\x00\x07\x0e\x0f\x7fb");
    assert_eq!(t.row_text(0), "ab");
}

#[test]
fn can_and_sub_abort_a_sequence() {
    // CAN (0x18) and SUB (0x1A) cancel a sequence in progress; what follows is plain text.
    let t = term(20, 1, b"\x1b[31\x18mX\x1b[1\x1aY");
    assert_eq!(t.row_text(0), "mXY");
    assert_eq!(t.line(0, 0)[1].attrs, Attrs::default());
}

#[test]
fn escape_restarts_a_sequence() {
    // An ESC in the middle of a CSI abandons it and starts over.
    let t = term(10, 1, b"\x1b[3\x1b[32mG");
    assert_eq!(t.row_text(0), "G");
    assert_eq!(t.line(0, 0)[0].attrs.fg, Color::Indexed(2));
}

#[test]
fn control_characters_inside_csi_still_act() {
    // A carriage return inside a CSI is carried out, then the CSI finishes.
    let t = term(10, 1, b"abc\x1b[\r2CX");
    assert_eq!(t.row_text(0), "abX");
}

#[test]
fn charset_designations_are_swallowed() {
    let t = term(10, 1, b"\x1b(Ba\x1b)0b\x1b#8c");
    assert_eq!(t.row_text(0), "abc");
}

#[test]
fn dcs_apc_pm_sos_strings_are_ignored_until_st_or_bel() {
    let t = term(20, 1, b"a\x1bPq#0;1;2\x1b\\b\x1b_hidden\x07c\x1b^pm\x1b\\d\x1bXsos\x1b\\e");
    assert_eq!(t.row_text(0), "abcde");
}

#[test]
fn an_escape_inside_an_ignored_string_that_is_not_st_keeps_ignoring() {
    let t = term(20, 1, b"\x1bPab\x1bxcd\x1b\\Z");
    assert_eq!(t.row_text(0), "Z");
}

#[test]
fn osc_ends_on_bel_or_st() {
    let mut t = term(10, 1, b"\x1b]2;with st\x1b\\");
    assert_eq!(t.title, "with st");
    t.feed(b"\x1b]0;with bel\x07");
    assert_eq!(t.title, "with bel");
}

#[test]
fn osc_title_keeps_unicode_and_semicolons() {
    let t = term(10, 1, "\x1b]0;π; a;b 🌊\x07".as_bytes());
    assert_eq!(t.title, "π; a;b 🌊");
}

#[test]
fn osc_cut_short_by_another_escape_still_runs_that_escape() {
    // ESC that is not followed by '\' ends the OSC and is read as the start of a new escape.
    let t = term(10, 2, b"\x1b]0;t\x1b[2;1HX");
    assert_eq!(t.title, "t");
    assert_eq!(t.row_text(1), "X");
}

#[test]
fn unknown_osc_codes_change_nothing() {
    let t = term(10, 1, b"\x1b]7;file://host/tmp\x07\x1b]8;;https://x.io\x07link\x1b]8;;\x07");
    assert_eq!(t.title, "");
    assert_eq!(t.row_text(0), "link");
}

#[test]
fn a_huge_osc_is_capped_and_does_not_leak_into_the_screen() {
    let mut bytes = b"\x1b]0;".to_vec();
    bytes.extend(std::iter::repeat_n(b'x', 100_000));
    bytes.extend_from_slice(b"\x07ok");
    let t = term(10, 1, &bytes);
    assert!(t.title.len() <= 4096, "title kept to the cap, got {}", t.title.len());
    assert_eq!(t.row_text(0), "ok");
}

#[test]
fn huge_parameters_saturate_instead_of_overflowing() {
    let mut t = term(10, 5, b"\x1b[99999999999999999999;99999999999999999999H");
    assert_eq!(t.cursor(), (4, 9));
    t.feed(b"\x1b[4294967296A\x1b[99999999999D");
    assert_eq!(t.cursor(), (0, 0));
    t.feed(b"\x1b[99999@\x1b[99999P\x1b[99999X\x1b[99999L\x1b[99999M\x1b[99999S\x1b[99999T");
    assert_eq!(screen(&t), vec![""; 5]);
}

#[test]
fn zero_and_missing_parameters_mean_one() {
    let mut t = term(10, 5, b"\x1b[3;3H\x1b[0A");
    assert_eq!(t.cursor(), (1, 2));
    t.feed(b"\x1b[C");
    assert_eq!(t.cursor(), (1, 3));
    t.feed(b"\x1b[;5H");
    assert_eq!(t.cursor(), (0, 4));
}

#[test]
fn unknown_final_bytes_are_ignored() {
    let t = term(10, 1, b"a\x1b[5yb\x1b[?99zc\x1b[>1;2td");
    assert_eq!(t.row_text(0), "abcd");
}

#[test]
fn every_byte_value_is_survivable() {
    // Throw all 256 byte values at it, several times over, in every state the parser has.
    let mut t = Terminal::new(7, 3);
    let all: Vec<u8> = (0..=255u8).collect();
    for prefix in [&b""[..], b"\x1b", b"\x1b[", b"\x1b]", b"\x1bP", b"\x1b[?", b"\x1b(", b"\xE4"] {
        t.feed(prefix);
        t.feed(&all);
    }
    let (r, c) = t.cursor();
    assert!(r < t.rows() && c < t.cols());
}

#[test]
fn pseudo_random_input_never_panics_and_keeps_the_cursor_on_screen() {
    // A small fixed-seed generator: the same bytes every run, no extra crates.
    let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    // Biased toward the bytes that drive the parser so the sequences get deep.
    let alphabet = b"\x1b[]?;:0123456789HJKmrhlABCDsu\r\n\x08\t\x07\\PMDE7c@LMXST abc\xC3\xA9\xE4\xB8\xAD";
    for (cols, rows) in [(1, 1), (2, 2), (5, 3), (80, 24)] {
        let mut t = Terminal::new(cols, rows);
        for _ in 0..200 {
            let len = (next() % 64) as usize;
            let chunk: Vec<u8> = (0..len).map(|_| alphabet[(next() % alphabet.len() as u64) as usize]).collect();
            t.feed(&chunk);
            if next() % 17 == 0 {
                t.resize((next() % 40 + 1) as usize, (next() % 20 + 1) as usize);
            }
            let (r, c) = t.cursor();
            assert!(r < t.rows() && c < t.cols(), "cursor ({r},{c}) off a {}x{} screen", t.cols(), t.rows());
            assert_eq!(t.line(t.rows() - 1, 0).len(), t.cols());
            let _ = t.take_responses();
            let _ = t.text_between((0, 0), (t.total_lines(), t.cols()));
        }
    }
}
