//! Helpers shared by the core-vt integration tests.

#![allow(dead_code)]

use core_vt::Terminal;

/// A terminal with `bytes` already fed to it.
pub fn term(cols: usize, rows: usize, bytes: &[u8]) -> Terminal {
    let mut t = Terminal::new(cols, rows);
    t.feed(bytes);
    t
}

/// Every live row as text, trailing spaces trimmed.
pub fn screen(t: &Terminal) -> Vec<String> {
    (0..t.rows()).map(|r| t.row_text(r)).collect()
}

/// Every history row (scrollback, or the alternate-screen recording) as text.
pub fn history(t: &Terminal) -> Vec<String> {
    (0..t.history_len())
        .map(|i| t.abs_line(i).iter().filter(|c| !c.spacer).map(|c| c.ch).collect::<String>().trim_end().to_string())
        .collect()
}
