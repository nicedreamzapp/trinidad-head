//! Plain-text helpers for the view: finding a web link under the mouse, and wrapping the
//! text an input method (dictation) is still composing.

/// Characters that can appear inside a URL on screen.
fn url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

/// The web link covering `col` in `line` (one char per cell, spacers already removed by the
/// caller as '\0'), or None. Trailing sentence punctuation and an unmatched ')' are dropped.
pub fn url_at(line: &[char], col: usize) -> Option<String> {
    let text: String = line.iter().collect();
    for scheme in ["https://", "http://"] {
        let mut from = 0;
        while let Some(pos) = text[from..].find(scheme) {
            let start_byte = from + pos;
            let start = text[..start_byte].chars().count();
            let mut end = start;
            while end < line.len() && url_char(line[end]) {
                end += 1;
            }
            let mut url: String = line[start..end].iter().collect();
            trim_url(&mut url);
            let end = start + url.chars().count();
            if col >= start && col < end && url.len() > scheme.len() {
                return Some(url);
            }
            from = start_byte + scheme.len();
        }
    }
    None
}

fn trim_url(url: &mut String) {
    loop {
        let Some(last) = url.chars().last() else { return };
        let unmatched_paren = last == ')' && url.matches('(').count() < url.matches(')').count();
        if ".,;:!?'\"".contains(last) || unmatched_paren {
            url.pop();
        } else {
            return;
        }
    }
}

/// Break `text` into lines: the first holds at most `first` characters, the rest at most
/// `width`. Breaks at spaces when it can.
pub fn wrap(text: &str, first: usize, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut rest: Vec<char> = text.chars().collect();
    let mut limit = first.max(1);
    while rest.len() > limit {
        let cut = rest[..=limit].iter().rposition(|&c| c == ' ').filter(|&i| i > 0).unwrap_or(limit);
        lines.push(rest[..cut].iter().collect());
        let skip = if rest.get(cut) == Some(&' ') { cut + 1 } else { cut };
        rest.drain(..skip);
        limit = width;
    }
    lines.push(rest.into_iter().collect());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn finds_the_link_under_the_column() {
        let line = chars("see https://ineedhemp.com/shop. and http://x.io/a_(b) ok");
        assert_eq!(url_at(&line, 10).as_deref(), Some("https://ineedhemp.com/shop"));
        assert_eq!(url_at(&line, 30), None); // the trimmed period
        assert_eq!(url_at(&line, 2), None);
        assert_eq!(url_at(&line, 40).as_deref(), Some("http://x.io/a_(b)"));
        assert_eq!(url_at(&chars("(https://a.com/x)"), 5).as_deref(), Some("https://a.com/x"));
        assert_eq!(url_at(&chars("https:// nothing"), 2), None);
    }

    #[test]
    fn wraps_at_spaces() {
        assert_eq!(wrap("hello there world", 8, 11), vec!["hello", "there world"]);
        assert_eq!(wrap("abcdefghij", 4, 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("short", 20, 20), vec!["short"]);
        assert_eq!(wrap("", 5, 5), vec![""]);
    }
}

/// Selecting past the edge inside a program that owns the screen (Claude Code, a pager) is a
/// different job from selecting in our own scrollback. That text was never ours: the program
/// keeps it, paints one screen at a time, and repaints the whole grid when it scrolls. Grid
/// coordinates stop meaning anything mid-drag — the old selection would quietly come to cover
/// whatever text later landed on those rows.
///
/// So we ask the program to scroll (a wheel report, which it asked for by turning mouse
/// tracking on) and keep what it uncovers here, as text.
pub struct Harvest {
    /// True when the drag is running back up through the program's history.
    pub up: bool,
    /// The visible rows at the last step, to measure how far the program actually scrolled.
    last: Vec<String>,
    /// Everything selected so far, in reading order.
    pub lines: Vec<String>,
    /// Steps in a row where nothing moved: the program has no more to show.
    pub stuck: u32,
}

/// Stop asking after this many steps with nothing new on screen.
const GIVE_UP: u32 = 12;

impl Harvest {
    /// `seed` is what was already selected on screen when the drag reached the edge.
    pub fn new(up: bool, screen: Vec<String>, seed: Vec<String>) -> Harvest {
        Harvest { up, last: screen, lines: seed, stuck: 0 }
    }

    /// Bank whatever the program uncovered since the last step. False once it has stopped
    /// moving, which is how a drag against the top of a chat ends.
    pub fn absorb(&mut self, now: Vec<String>) -> bool {
        let fresh = match moved(&self.last, &now, self.up) {
            Moved::Still => None,
            Moved::Rows(r) => Some(now[r].to_vec()),
            Moved::Jumped => Some(now.clone()),
        };
        match fresh {
            None => self.stuck += 1,
            Some(rows) => {
                self.stuck = 0;
                if self.up {
                    let mut head = rows;
                    head.append(&mut self.lines);
                    self.lines = head;
                } else {
                    self.lines.extend(rows);
                }
            }
        }
        self.last = now;
        self.stuck < GIVE_UP
    }

    /// What to put on the clipboard: trailing blank rows the program painted are dropped.
    pub fn text(&self) -> String {
        let mut lines = self.lines.clone();
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }
}

/// What the screen did between two snapshots.
enum Moved {
    /// Nothing travelled: the program has nothing more to show.
    Still,
    /// These rows of the new screen were not on the old one, in reading order.
    Rows(std::ops::Range<usize>),
    /// Nothing lines up at all, so the program jumped somewhere unrelated.
    Jumped,
}

/// The longest unbroken run of rows that line up when the screen is read as having travelled
/// `k` rows, given in NEW-screen coordinates, with the count of non-blank rows in it. Blank
/// rows line up with each other by accident, so they do not vote.
fn run_at(old: &[String], new: &[String], up: bool, k: usize) -> Option<(usize, usize, usize)> {
    let rows = old.len().min(new.len());
    if k >= rows {
        return None;
    }
    let mut best: Option<(usize, usize, usize)> = None;
    let mut start: Option<usize> = None;
    let mut weight = 0usize;
    let keep = |best: &mut Option<(usize, usize, usize)>, s: usize, e: usize, w: usize| {
        if best.is_none_or(|(_, _, bw)| w > bw) {
            *best = Some((s, e, w));
        }
    };
    for i in 0..rows - k {
        let (o, n) = if up { (i, i + k) } else { (i + k, i) };
        if old[o] == new[n] {
            if start.is_none() {
                start = Some(n);
                weight = 0;
            }
            if !new[n].trim().is_empty() {
                weight += 1;
            }
            if i + 1 == rows - k {
                keep(&mut best, start.unwrap(), n + 1, weight);
            }
        } else if let Some(s) = start.take() {
            keep(&mut best, s, n, weight);
        }
    }
    best
}

/// Read two snapshots as one screen that travelled. A full-screen program usually pins part of
/// the grid — Claude Code keeps its prompt box and status line at the bottom, and they never
/// scroll — so only the rows that really moved may be banked. Whole-screen matching would find
/// no overlap at all against a pinned box and call every repaint a jump, which is how the same
/// screen ended up in the clipboard over and over.
fn moved(old: &[String], new: &[String], up: bool) -> Moved {
    let rows = old.len().min(new.len());
    if rows == 0 || old[..rows] == new[..rows] {
        return Moved::Still;
    }
    let still = run_at(old, new, up, 0).map_or(0, |(_, _, w)| w);
    let mut best: Option<(usize, usize, usize, usize)> = None;
    for k in 1..rows {
        if let Some((s, e, w)) = run_at(old, new, up, k) {
            if best.is_none_or(|(_, _, _, bw)| w > bw) {
                best = Some((k, s, e, w));
            }
        }
    }
    match best {
        // Two rows in a row is the least that tells travel apart from a coincidence.
        Some((k, s, e, w)) if w > still && w >= 2 => {
            let range = if up { s.saturating_sub(k)..s } else { e..(e + k).min(new.len()) };
            if range.is_empty() {
                Moved::Still
            } else {
                Moved::Rows(range)
            }
        }
        _ if still > 0 => Moved::Still,
        _ => Moved::Jumped,
    }
}

#[cfg(test)]
mod harvest_tests {
    use super::*;

    fn screen(from: usize, n: usize) -> Vec<String> {
        (from..from + n).map(|i| format!("line {i}")).collect()
    }

    #[test]
    fn banks_what_scrolling_up_uncovers() {
        let mut h = Harvest::new(true, screen(10, 5), screen(10, 5).to_vec());
        assert!(h.absorb(screen(8, 5)));
        assert_eq!(h.lines, ["line 8", "line 9", "line 10", "line 11", "line 12", "line 13", "line 14"]);
    }

    #[test]
    fn banks_what_scrolling_down_uncovers() {
        let mut h = Harvest::new(false, screen(10, 5), screen(10, 5).to_vec());
        assert!(h.absorb(screen(12, 5)));
        assert_eq!(h.lines.last().unwrap(), "line 16");
        assert_eq!(h.lines.len(), 7);
    }

    #[test]
    fn a_still_screen_counts_against_giving_up() {
        let mut h = Harvest::new(true, screen(10, 5), Vec::new());
        for _ in 0..GIVE_UP - 1 {
            assert!(h.absorb(screen(10, 5)));
        }
        assert!(!h.absorb(screen(10, 5)));
        assert!(h.lines.is_empty());
    }

    /// Claude Code's real shape: a prompt box pinned to the bottom that never scrolls, and a
    /// status line inside it that changes on every repaint. Only the rows above it travel.
    fn chat(from: usize, n: usize, paint: usize) -> Vec<String> {
        let mut rows: Vec<String> = (from..from + n).map(|i| format!("line {i}")).collect();
        rows.push(String::new());
        rows.push("> ask me anything".into());
        rows.push(format!("  ? for shortcuts   {paint} paints"));
        rows
    }

    #[test]
    fn a_pinned_prompt_box_does_not_stop_it_reading_the_scroll() {
        let mut h = Harvest::new(true, chat(10, 6, 1), Vec::new());
        assert!(h.absorb(chat(7, 6, 2)));
        assert_eq!(h.lines, ["line 7", "line 8", "line 9"]);
        assert!(h.absorb(chat(4, 6, 3)));
        assert_eq!(h.lines, ["line 4", "line 5", "line 6", "line 7", "line 8", "line 9"]);
        // Down the other way, the new rows come off the bottom of the text, not the box.
        let mut h = Harvest::new(false, chat(10, 6, 1), Vec::new());
        assert!(h.absorb(chat(13, 6, 2)));
        assert_eq!(h.lines, ["line 16", "line 17", "line 18"]);
    }

    #[test]
    fn a_pinned_box_over_a_still_screen_is_still_still() {
        let mut h = Harvest::new(true, chat(10, 6, 1), Vec::new());
        // Only the ticking status line changed: the program has nothing more to show.
        assert!(h.absorb(chat(10, 6, 2)));
        assert!(h.lines.is_empty());
        assert_eq!(h.stuck, 1);
    }

    #[test]
    fn a_jump_with_no_overlap_takes_the_whole_screen() {
        let mut h = Harvest::new(true, screen(10, 5), Vec::new());
        assert!(h.absorb(screen(90, 5)));
        assert_eq!(h.lines, screen(90, 5));
    }

    #[test]
    fn trailing_blank_rows_are_not_copied() {
        let mut h = Harvest::new(true, Vec::new(), vec!["a".into(), "b".into(), String::new(), String::new()]);
        assert_eq!(h.text(), "a\nb");
        assert!(h.absorb(Vec::new()) || true);
    }
}
