//! Glow themes for the window rim, and the tiny settings file that remembers the choice.

/// A neon rim: four colors swept diagonally around the window (0xRRGGBB).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glow {
    pub name: &'static str,
    pub colors: [u32; 4],
}

impl Glow {
    /// The color used for the cursor and highlights.
    pub fn accent(&self) -> u32 {
        self.colors[0]
    }
}

pub const GLOWS: [Glow; 8] = [
    Glow { name: "aurora", colors: [0x3FD8FF, 0x5B6CFF, 0xB44DFF, 0xFF4FD8] },
    Glow { name: "ember", colors: [0xFFB45A, 0xFF7A1A, 0xFF4D1A, 0xFF9A3C] },
    Glow { name: "tide", colors: [0x7FFFD0, 0x14D6A0, 0x10A8C8, 0x2FE0FF] },
    Glow { name: "rose", colors: [0xFF9AC8, 0xFF4F9A, 0xD9368B, 0xFF7AB0] },
    Glow { name: "ocean", colors: [0x6FE6FF, 0x2F9BFF, 0x1B4DFF, 0x00C8FF] },
    Glow { name: "gold", colors: [0xFFE27A, 0xF5B82E, 0xE0901A, 0xFFD24D] },
    Glow { name: "violet", colors: [0xC9A2FF, 0x8A4DFF, 0x6A2FE0, 0xB07BFF] },
    Glow { name: "lime", colors: [0xD4FF7A, 0x8FE03A, 0x4CC23A, 0xB8F55A] },
];

/// The glow for a new window: the saved default if no other open window uses it, otherwise
/// the next color (in list order) that no open window is using. With every color taken,
/// colors repeat, starting again from the default.
pub fn pick_glow(default: usize, in_use: &[usize]) -> usize {
    let n = GLOWS.len();
    let default = default % n;
    (0..n)
        .map(|k| (default + k) % n)
        .find(|g| !in_use.contains(g))
        .unwrap_or((default + in_use.len()) % n)
}

/// Background of the window body (near-black glass).
pub const BODY: u32 = 0x090B18;
pub const BODY_OPACITY: f32 = 0.98;
/// Tints washed over the body from the top-left and bottom-right corners.
pub const TINT_A: u32 = 0x1C2658;
pub const TINT_B: u32 = 0x3A1450;
pub const TEXT: u32 = 0xD7DBE3;

/// Claude Code's "your message" bar. The PC's Claude theme (~/.claude/themes/trinidad-head.json)
/// paints it this exact color; cells on it get green (4E9F6B, Matt picked option C), bold, taller text on a see-through pill (alpha .26) so Matt's own
/// prompts stand out when scrolling back (2026-09-17, his pick: sample 8).
pub const USER_BAR: (u8, u8, u8) = (0xC4, 0xD6, 0xE6);
pub const USER_TEXT: u32 = 0x4E9F6B;
/// The pill is see-through glass tinted with the bar color (Matt: "too white, more clear").
pub const USER_BAR_ALPHA: f32 = 0.26;
/// How much bigger Matt's prompt text is drawn: one size up (Matt, 2026-09-17). Widths shrink
/// back toward the grid only if a long line would otherwise run past the pill.
pub const USER_TEXT_SCALE: f32 = 1.18;
pub const ICON: u32 = 0xA9B0BD;

pub fn by_name(name: &str) -> usize {
    GLOWS.iter().position(|g| g.name == name.trim()).unwrap_or(0)
}

/// Reads `glow=<name>` from the settings text; anything else is ignored.
pub fn parse_settings(text: &str) -> usize {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("glow="))
        .last()
        .map(by_name)
        .unwrap_or(0)
}

#[cfg_attr(windows, allow(dead_code))]
pub fn settings_text(glow: usize) -> String {
    format!("# Trinidad Head settings\nglow={}\n", GLOWS[glow % GLOWS.len()].name)
}

/// Claude's own "your message" bar colors (dark theme, plus its hover shade). Treated as the
/// prompt bar too, so the pill and green text show up even before Claude picks up our theme.
pub const CLAUDE_DEFAULT_BARS: [(u8, u8, u8); 2] = [(55, 55, 55), (70, 70, 70)];

pub fn is_prompt_bar(rgb: (u8, u8, u8), user_bar: (u8, u8, u8)) -> bool {
    rgb == user_bar || CLAUDE_DEFAULT_BARS.contains(&rgb)
}

/// The prompt-bar color from Claude's theme file text, so the two never drift apart.
pub fn user_bar_from_theme(text: &str) -> Option<(u8, u8, u8)> {
    let key = text.find("\"userMessageBackground\"")?;
    let rest = &text[key..];
    let open = rest.find("rgb(")? + 4;
    let close = rest[open..].find(')')? + open;
    let mut parts = rest[open..close].split(',').map(|p| p.trim().parse::<u8>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bar_color_from_claude_theme() {
        let t = r#"{"overrides": {"userMessageBackground": "rgb(196, 214,230)", "userMessageBackgroundHover": "rgb(1,2,3)"}}"#;
        assert_eq!(user_bar_from_theme(t), Some((196, 214, 230)));
        assert_eq!(user_bar_from_theme("{}"), None);
    }

    #[test]
    fn each_window_gets_its_own_color() {
        assert_eq!(pick_glow(0, &[]), 0);
        assert_eq!(pick_glow(0, &[0]), 1);
        assert_eq!(pick_glow(0, &[0, 1, 3]), 2);
        assert_eq!(pick_glow(2, &[2]), 3);
        let all: Vec<usize> = (0..GLOWS.len()).collect();
        assert!(pick_glow(0, &all) < GLOWS.len());
    }

    #[test]
    fn round_trips_and_defaults() {
        assert_eq!(parse_settings(""), 0);
        assert_eq!(parse_settings("glow=tide"), 2);
        assert_eq!(parse_settings(&settings_text(3)), 3);
        assert_eq!(parse_settings("glow=nonsense"), 0);
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    #[test]
    fn glow_names_are_unique_and_found_by_name() {
        for (i, g) in GLOWS.iter().enumerate() {
            assert_eq!(by_name(g.name), i);
            assert_eq!(by_name(&format!("  {}\n", g.name)), i, "surrounding whitespace is ignored");
            assert_eq!(g.accent(), g.colors[0]);
            assert!(g.colors.iter().all(|&c| c <= 0xFF_FFFF));
        }
        assert_eq!(by_name("Aurora"), 0, "an unknown (or wrongly cased) name falls back to the first");
        assert_eq!(by_name("ember"), 1);
    }

    #[test]
    fn the_last_glow_line_wins_and_comments_are_ignored() {
        assert_eq!(parse_settings("# glow=rose\nglow=gold\n  glow=lime  \nother=1"), 7);
        assert_eq!(parse_settings("glow=\n"), 0);
        assert_eq!(parse_settings("GLOW=tide"), 0, "the key is lower case");
    }

    #[test]
    fn settings_text_wraps_an_out_of_range_index() {
        assert_eq!(parse_settings(&settings_text(GLOWS.len() + 2)), 2);
        assert!(settings_text(0).starts_with("# "), "the file explains itself");
    }

    #[test]
    fn pick_glow_wraps_the_default_and_repeats_once_all_are_taken() {
        assert_eq!(pick_glow(GLOWS.len() + 1, &[]), 1);
        assert_eq!(pick_glow(7, &[7]), 0, "the search wraps past the end of the list");
        let all: Vec<usize> = (0..GLOWS.len()).collect();
        assert_eq!(pick_glow(3, &all), 3, "with every color taken, colors repeat from the default");
        let more: Vec<usize> = (0..GLOWS.len() + 2).map(|i| i % GLOWS.len()).collect();
        assert_eq!(pick_glow(0, &more), 2);
    }

    #[test]
    fn pick_glow_ignores_indexes_from_other_windows_that_are_out_of_range() {
        assert_eq!(pick_glow(0, &[99, 1000]), 0);
    }

    #[test]
    fn prompt_bar_colors() {
        assert!(is_prompt_bar(USER_BAR, USER_BAR));
        assert!(is_prompt_bar((55, 55, 55), USER_BAR));
        assert!(is_prompt_bar((70, 70, 70), (1, 2, 3)));
        assert!(!is_prompt_bar((56, 55, 55), USER_BAR));
        assert!(is_prompt_bar((1, 2, 3), (1, 2, 3)));
    }

    #[test]
    fn theme_parsing_rejects_bad_values() {
        let t = |v: &str| user_bar_from_theme(&format!(r#"{{"userMessageBackground": "{v}"}}"#));
        assert_eq!(t("rgb(0,0,0)"), Some((0, 0, 0)));
        assert_eq!(t("rgb( 255 , 1 , 2 )"), Some((255, 1, 2)));
        assert_eq!(t("rgb(256,0,0)"), None, "out of range for a byte");
        assert_eq!(t("rgb(1,2)"), None, "too few channels");
        assert_eq!(t("rgb(a,b,c)"), None);
        assert_eq!(t("rgb(1,2,3"), None, "no closing paren");
        assert_eq!(user_bar_from_theme(r#"{"userMessageBackgroundHover": "rgb(1,2,3)"}"#), None, "only the exact key");
    }
}
