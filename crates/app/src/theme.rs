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

pub const GLOWS: [Glow; 4] = [
    Glow { name: "aurora", colors: [0x3FD8FF, 0x5B6CFF, 0xB44DFF, 0xFF4FD8] },
    Glow { name: "ember", colors: [0xFFB45A, 0xFF7A1A, 0xFF4D1A, 0xFF9A3C] },
    Glow { name: "ocean", colors: [0x6FE6FF, 0x2F9BFF, 0x1B4DFF, 0x00C8FF] },
    Glow { name: "tide", colors: [0x7FFFD0, 0x14D6A0, 0x10A8C8, 0x2FE0FF] },
];

/// Background of the window body (near-black glass).
pub const BODY: u32 = 0x090B18;
pub const BODY_OPACITY: f32 = 0.98;
/// Tints washed over the body from the top-left and bottom-right corners.
pub const TINT_A: u32 = 0x1C2658;
pub const TINT_B: u32 = 0x3A1450;
pub const TEXT: u32 = 0xD7DBE3;

/// Claude Code's "your message" bar. The PC's Claude theme (~/.claude/themes/trinidad-head.json)
/// paints it this exact color; cells on it get dark green, bold, taller text so Matt's own
/// prompts stand out when scrolling back (2026-09-17, his pick: sample 8).
pub const USER_BAR: (u8, u8, u8) = (0xC4, 0xD6, 0xE6);
pub const USER_TEXT: u32 = 0x0B3D20;
/// How much taller Matt's prompt text is drawn (width stays on the grid).
pub const USER_TEXT_STRETCH: f32 = 1.22;
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

pub fn settings_text(glow: usize) -> String {
    format!("# Trinidad Head settings\nglow={}\n", GLOWS[glow % GLOWS.len()].name)
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
    fn round_trips_and_defaults() {
        assert_eq!(parse_settings(""), 0);
        assert_eq!(parse_settings("glow=ocean"), 2);
        assert_eq!(parse_settings(&settings_text(3)), 3);
        assert_eq!(parse_settings("glow=nonsense"), 0);
    }
}
