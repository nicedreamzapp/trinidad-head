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
