//! Deleting highlighted text inside a program's own input box, the way a text box would.
//!
//! Claude Code's prompt is Claude's text, not ours: the terminal only sees the cells Claude
//! paints, so it cannot cut words out of it directly. It does what a person would instead:
//! click just after the highlight (Claude moves its caret there), then press Backspace once
//! per character, watching Claude's caret to know when it has reached the start of the
//! highlight. Anything typed while that runs waits and follows it, so typing over a highlight
//! replaces it.
//!
//! Only a highlight that sits wholly inside the input box counts: the rows between the two
//! horizontal rules around the caret, with the text starting two columns in (`❯ ` on the
//! first row, an indent on the rest). A highlight anywhere else is left to the old behaviour.

use std::time::{Duration, Instant};

use core_vt::Cell;

/// Where the text of the box starts on every row, after `❯ ` or the indent.
const TEXT_COL: usize = 2;

/// What to delete: the caret positions just before and just after the highlight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cut {
    pub start: (usize, usize),
    pub end: (usize, usize),
}

/// Blank for the prompt's own lead-in. Claude puts a no-break space after `❯`.
fn blank(ch: char) -> bool {
    ch == ' ' || ch == '\u{a0}'
}

fn is_rule(row: &[Cell]) -> bool {
    let mut any = false;
    for c in row {
        match c.ch {
            '─' => any = true,
            ' ' => {}
            _ => return false,
        }
    }
    any
}

/// The rows of the input box around the caret (first, last), if the caret is in one.
fn input_box(rows: &[Vec<Cell>], cursor: (usize, usize)) -> Option<(usize, usize)> {
    let r = cursor.0;
    if r >= rows.len() || is_rule(&rows[r]) {
        return None;
    }
    let top = (0..r).rev().find(|&i| is_rule(&rows[i]))? + 1;
    let bottom = (r + 1..rows.len()).find(|&i| is_rule(&rows[i]))? - 1;
    let first = &rows[top];
    // The first row is `❯ text`: a prompt mark, a space, then the text.
    let mark = first.first().map(|c| c.ch).unwrap_or(' ');
    if blank(mark) || !first.get(1).is_some_and(|c| blank(c.ch)) {
        return None;
    }
    // Every row after it is indented to line up with the text.
    let indented = rows[top + 1..=bottom].iter().all(|row| row.iter().take(TEXT_COL).all(|c| blank(c.ch)));
    indented.then_some((top, bottom))
}

/// One past the last character of text on a row of the box. Faint cells are Claude's greyed
/// suggestion, which is not text anyone typed.
fn text_end(row: &[Cell]) -> usize {
    row.iter()
        .enumerate()
        .rev()
        .find(|(i, c)| *i >= TEXT_COL && !c.spacer && c.ch != ' ' && !c.attrs.dim)
        .map(|(i, _)| i + 1)
        .unwrap_or(TEXT_COL)
}

/// The cut a highlight from `a` to `b` (screen cells, `b` included) asks for, if the whole
/// highlight is inside the input box the caret sits in and covers at least one character.
pub fn plan(rows: &[Vec<Cell>], cursor: (usize, usize), a: (usize, usize), b: (usize, usize)) -> Option<Cut> {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    let (top, bottom) = input_box(rows, cursor)?;
    if a.0 < top || b.0 > bottom {
        return None;
    }
    let start = (a.0, a.1.clamp(TEXT_COL, text_end(&rows[a.0])));
    // A highlight that runs on past the text ends where the program puts the caret for a
    // click out there: the true end of the line, trailing spaces included, which only the
    // program knows about. So the click goes where the hand let go.
    let last = rows[b.0].len().saturating_sub(1);
    let end = (b.0, if b.1 + 1 > text_end(&rows[b.0]) { b.1.min(last) } else { (b.1 + 1).max(TEXT_COL) });
    // It has to cover at least one character anyone can see.
    let covers = start < (b.0, text_end(&rows[b.0]));
    (start < end && covers).then_some(Cut { start, end })
}

/// Characters painted between two caret positions. Where a line wraps, the space it broke at
/// (or the newline) is not painted, so this can come up short by one per row boundary — never
/// over. `run` makes up the difference by watching the caret.
pub fn painted_chars(rows: &[Vec<Cell>], from: (usize, usize), to: (usize, usize)) -> usize {
    let mut n = 0;
    for r in from.0..=to.0.min(rows.len().saturating_sub(1)) {
        let row = &rows[r];
        let lo = if r == from.0 { from.1 } else { TEXT_COL };
        let hi = if r == to.0 { to.1 } else { text_end(row) };
        n += row.iter().take(hi).skip(lo).filter(|c| !c.spacer).count();
    }
    n
}

/// What `run` needs from the window, so the Mac and Windows builds can share it.
pub trait Screen {
    /// The caret and the live screen rows, read together.
    fn snapshot(&self) -> ((usize, usize), Vec<Vec<Cell>>);
    fn write(&self, bytes: &[u8]);
    /// A left click at a screen cell, in whatever mouse encoding the program asked for.
    fn click(&self, cell: (usize, usize));
}

const POLL: Duration = Duration::from_millis(4);

/// Wait until the caret sits at `want`, or has stopped moving for `settle` after moving at
/// least once, or `limit` runs out. Returns where it ended up.
fn wait_caret(s: &dyn Screen, from: (usize, usize), want: (usize, usize), settle: Duration, limit: Duration) -> (usize, usize) {
    let t0 = Instant::now();
    let mut last = from;
    let mut moved_at: Option<Instant> = None;
    loop {
        let (now, _) = s.snapshot();
        if now == want {
            return now;
        }
        if now != last {
            last = now;
            moved_at = Some(Instant::now());
        }
        if moved_at.is_some_and(|t| t.elapsed() >= settle) || t0.elapsed() >= limit {
            return last;
        }
        std::thread::sleep(POLL);
    }
}

/// Delete the text of `cut` from the program's input box. Returns whether anything was sent
/// besides the click (false means the program never moved its caret, so nothing was deleted).
pub fn run(s: &dyn Screen, cut: Cut) -> bool {
    let (before, _) = s.snapshot();
    s.click(cut.end);
    let caret = wait_caret(s, before, cut.end, Duration::from_millis(60), Duration::from_millis(500));
    let (_, rows) = s.snapshot();
    // The caret has to be in the box and after the start of the highlight, or the program did
    // not take the click the way Claude does. Deleting blind from there could eat the wrong text.
    let Some((top, bottom)) = input_box(&rows, caret) else { return false };
    if caret <= cut.start || cut.start.0 < top || caret.0 > bottom {
        return false;
    }
    let n = painted_chars(&rows, cut.start, caret);
    if n > 0 {
        s.write(&vec![0x7f; n]);
    }
    let mut at = wait_caret(s, caret, cut.start, Duration::from_millis(150), Duration::from_millis(2000));
    // Make up the characters a wrap hid: at most one per row boundary, one press at a time,
    // and only while the caret is still past the start.
    for _ in 0..(caret.0 - cut.start.0) {
        if at <= cut.start {
            break;
        }
        s.write(b"\x7f");
        let next = wait_caret(s, at, cut.start, Duration::from_millis(40), Duration::from_millis(400));
        if next == at {
            break;
        }
        at = next;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn row(text: &str, cols: usize) -> Vec<Cell> {
        let mut v: Vec<Cell> = text.chars().map(|ch| Cell { ch, ..Cell::default() }).collect();
        v.resize(cols, Cell::default());
        v
    }

    fn screen(lines: &[&str]) -> Vec<Vec<Cell>> {
        lines.iter().map(|l| row(l, 40)).collect()
    }

    const RULE: &str = "────────────────────────────────────────";

    #[test]
    fn a_word_in_the_prompt() {
        let rows = screen(&["hello", RULE, "❯ alpha bravo charlie", RULE, "  status"]);
        // "bravo" is columns 8..=12.
        let cut = plan(&rows, (2, 21), (2, 8), (2, 12)).unwrap();
        assert_eq!(cut, Cut { start: (2, 8), end: (2, 13) });
        assert_eq!(painted_chars(&rows, cut.start, cut.end), 5);
    }

    #[test]
    fn highlight_over_the_mark_and_past_the_end_takes_the_whole_line() {
        let rows = screen(&[RULE, "❯ alpha bravo", RULE]);
        let cut = plan(&rows, (1, 13), (1, 0), (1, 39)).unwrap();
        assert_eq!(cut, Cut { start: (1, 2), end: (1, 39) });
        // The count is taken from where the caret really lands, the end of the text.
        assert_eq!(painted_chars(&rows, cut.start, (1, 13)), 11);
    }

    #[test]
    fn wrapped_rows_count_short_never_over() {
        let rows = screen(&[RULE, "❯ one two three", "  four five", RULE]);
        let cut = plan(&rows, (2, 11), (1, 6), (2, 5)).unwrap();
        // "two three" + (hidden space) + "four" = 14 real characters, 13 painted.
        assert_eq!(painted_chars(&rows, cut.start, cut.end), 13);
    }

    #[test]
    fn claude_puts_a_no_break_space_after_the_mark() {
        let rows = screen(&[RULE, "❯\u{a0}alpha bravo", RULE]);
        assert_eq!(plan(&rows, (1, 13), (1, 8), (1, 12)), Some(Cut { start: (1, 8), end: (1, 13) }));
    }

    #[test]
    fn outside_the_box_is_left_alone() {
        let rows = screen(&["transcript text", RULE, "❯ typed", RULE]);
        assert_eq!(plan(&rows, (2, 7), (0, 0), (0, 5)), None);
        assert_eq!(plan(&rows, (2, 7), (0, 3), (2, 4)), None);
        // No box at all: a plain shell.
        let shell = screen(&["$ echo hi", "hi", "$ "]);
        assert_eq!(plan(&shell, (2, 2), (0, 2), (0, 5)), None);
    }

    #[test]
    fn empty_or_suggestion_only_prompt_has_nothing_to_cut() {
        let mut rows = screen(&[RULE, "❯ try this", RULE]);
        for c in rows[1].iter_mut().skip(2) {
            c.attrs.dim = true;
        }
        assert_eq!(plan(&rows, (1, 2), (1, 2), (1, 9)), None);
    }

    /// A pretend Claude prompt: text with a caret, wrapped at `width`, clicks move the caret.
    struct Fake {
        text: RefCell<Vec<char>>,
        caret: RefCell<usize>,
        width: usize,
        clicks: RefCell<u32>,
    }

    impl Fake {
        fn new(text: &str, width: usize) -> Fake {
            let t: Vec<char> = text.chars().collect();
            let n = t.len();
            Fake { text: RefCell::new(t), caret: RefCell::new(n), width, clicks: RefCell::new(0) }
        }
        /// Rows of (start offset, chars) after word wrapping, the way Claude breaks lines:
        /// at a space, which is then not painted.
        fn layout(&self) -> Vec<(usize, Vec<char>)> {
            let t = self.text.borrow();
            let w = self.width - TEXT_COL;
            let mut out = Vec::new();
            let mut i = 0;
            loop {
                if t.len() - i <= w {
                    out.push((i, t[i..].to_vec()));
                    return out;
                }
                let cut = (i..=i + w).rev().find(|&k| t[k] == ' ' && k > i).unwrap_or(i + w);
                out.push((i, t[i..cut].to_vec()));
                i = if t.get(cut) == Some(&' ') { cut + 1 } else { cut };
            }
        }
        fn pos(&self, off: usize) -> (usize, usize) {
            let lay = self.layout();
            for (r, (s, chars)) in lay.iter().enumerate() {
                let next = lay.get(r + 1).map(|x| x.0).unwrap_or(usize::MAX);
                if off >= *s && (off < next || r == lay.len() - 1) && off <= s + chars.len() {
                    return (r + 1, TEXT_COL + off - s);
                }
            }
            let (s, chars) = lay.last().unwrap();
            (lay.len(), TEXT_COL + chars.len().min(off - s))
        }
    }

    impl Screen for Fake {
        fn snapshot(&self) -> ((usize, usize), Vec<Vec<Cell>>) {
            let lay = self.layout();
            let mut lines = vec![RULE.to_string()];
            for (r, (_, chars)) in lay.iter().enumerate() {
                let lead = if r == 0 { "❯ " } else { "  " };
                lines.push(format!("{lead}{}", chars.iter().collect::<String>()));
            }
            lines.push(RULE.to_string());
            (self.pos(*self.caret.borrow()), lines.iter().map(|l| row(l, self.width)).collect())
        }
        fn write(&self, bytes: &[u8]) {
            for &b in bytes {
                if b == 0x7f {
                    let mut c = self.caret.borrow_mut();
                    if *c > 0 {
                        *c -= 1;
                        self.text.borrow_mut().remove(*c);
                    }
                }
            }
        }
        fn click(&self, cell: (usize, usize)) {
            *self.clicks.borrow_mut() += 1;
            let lay = self.layout();
            let (s, chars) = &lay[cell.0 - 1];
            *self.caret.borrow_mut() = s + (cell.1 - TEXT_COL).min(chars.len());
        }
    }

    fn cut_between(f: &Fake, a: (usize, usize), b: (usize, usize)) -> String {
        let (cur, rows) = f.snapshot();
        let cut = plan(&rows, cur, a, b).unwrap();
        assert!(run(f, cut));
        f.text.borrow().iter().collect()
    }

    #[test]
    fn deletes_one_word() {
        let f = Fake::new("alpha bravo charlie", 40);
        assert_eq!(cut_between(&f, (1, 8), (1, 13)), "alpha charlie");
    }

    #[test]
    fn deletes_across_a_wrap() {
        // width 20: rows "one two three four" / "five six seven"
        let f = Fake::new("one two three four five six seven", 20);
        let (_, rows) = f.snapshot();
        assert!(rows[1].iter().map(|c| c.ch).collect::<String>().starts_with("❯ one two three four"));
        // From "three" on row 1 to "six" on row 2.
        assert_eq!(cut_between(&f, (1, 10), (2, 9)), "one two  seven");
    }

    #[test]
    fn deletes_everything_when_all_rows_are_highlighted() {
        let f = Fake::new("one two three four five six seven eight nine", 20);
        let (_, rows) = f.snapshot();
        let last = rows.len() - 2;
        assert_eq!(cut_between(&f, (1, 0), (last, 19)), "");
    }

    #[test]
    fn trailing_spaces_go_too_when_the_highlight_runs_past_the_end() {
        let f = Fake::new("typed  ", 40);
        assert_eq!(cut_between(&f, (1, 0), (1, 39)), "");
    }

    #[test]
    fn keeps_text_after_the_highlight() {
        let f = Fake::new("keep this and that", 40);
        assert_eq!(cut_between(&f, (1, 2), (1, 6)), "this and that");
    }
}
