//! Turning Mac key presses into the bytes a terminal program expects.

/// Modifier state for one key press.
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

/// Bytes for keys that don't produce ordinary text: arrows, Home/End, function keys and the
/// like. `ch` is the first character of the event's `charactersIgnoringModifiers`, where AppKit
/// reports those keys as private-use characters (0xF700...). Returns None for everything else.
pub fn special(ch: char, m: Mods, app_cursor: bool) -> Option<Vec<u8>> {
    let code = 1 + m.shift as u8 + 2 * m.alt as u8 + 4 * m.ctrl as u8;
    let cursor = |c: char| -> Vec<u8> {
        if code > 1 {
            format!("\x1b[1;{code}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if code > 1 {
            format!("\x1b[{n};{code}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let ss3 = |c: char| -> Vec<u8> {
        if code > 1 {
            format!("\x1b[1;{code}{c}").into_bytes()
        } else {
            format!("\x1bO{c}").into_bytes()
        }
    };
    Some(match ch as u32 {
        0xF700 => cursor('A'),
        0xF701 => cursor('B'),
        0xF703 => cursor('C'),
        0xF702 => cursor('D'),
        0xF729 => cursor('H'),
        0xF72B => cursor('F'),
        0xF727 => tilde(2),
        0xF728 => tilde(3),
        0xF72C => tilde(5),
        0xF72D => tilde(6),
        0xF704 => ss3('P'),
        0xF705 => ss3('Q'),
        0xF706 => ss3('R'),
        0xF707 => ss3('S'),
        0xF708 => tilde(15),
        0xF709 => tilde(17),
        0xF70A => tilde(18),
        0xF70B => tilde(19),
        0xF70C => tilde(20),
        0xF70D => tilde(21),
        0xF70E => tilde(23),
        0xF70F => tilde(24),
        // Shift+Tab
        0x19 => b"\x1b[Z".to_vec(),
        _ => return None,
    })
}

/// Bytes for a control key (Return, Tab, Delete, Escape, Ctrl+letter) given the event's
/// `characters`, or None when it's ordinary text that should go through the input system.
pub fn control(chars: &str, m: Mods) -> Option<Vec<u8>> {
    let mut it = chars.chars();
    let c = it.next()?;
    if it.next().is_some() {
        return None;
    }
    let base: Vec<u8> = match c as u32 {
        0x0D => vec![0x0D],
        // Keypad Enter arrives as 0x03, the same code Ctrl+C makes.
        0x03 if !m.ctrl => vec![0x0D],
        0x09 => vec![0x09],
        0x7F => {
            if m.ctrl {
                vec![0x08]
            } else {
                vec![0x7F]
            }
        }
        0x1B => vec![0x1B],
        // Shift+Tab (AppKit reports it as the "backtab" control character).
        0x19 => return Some(b"\x1b[Z".to_vec()),
        n if n < 0x20 => vec![n as u8],
        _ if m.ctrl => {
            // Ctrl+Space and friends that AppKit didn't already turn into a control code.
            match c {
                ' ' | '@' | '2' => vec![0],
                c if c.is_ascii_alphabetic() => vec![(c.to_ascii_lowercase() as u8) & 0x1F],
                _ => return None,
            }
        }
        _ => return None,
    };
    if m.alt {
        let mut v = vec![0x1B];
        v.extend(base);
        Some(v)
    } else {
        Some(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_and_modifiers() {
        assert_eq!(special('\u{F700}', Mods::default(), false).unwrap(), b"\x1b[A");
        assert_eq!(special('\u{F700}', Mods::default(), true).unwrap(), b"\x1bOA");
        let shift = Mods { shift: true, ..Default::default() };
        assert_eq!(special('\u{F703}', shift, false).unwrap(), b"\x1b[1;2C");
        assert_eq!(special('\u{F728}', Mods::default(), false).unwrap(), b"\x1b[3~");
        assert!(special('a', Mods::default(), false).is_none());
    }

    #[test]
    fn control_keys() {
        assert_eq!(control("\r", Mods::default()).unwrap(), b"\r");
        assert_eq!(control("\u{7f}", Mods::default()).unwrap(), b"\x7f");
        assert_eq!(control("\u{3}", Mods::default()).unwrap(), b"\r");
        assert_eq!(control("\u{3}", Mods { ctrl: true, ..Default::default() }).unwrap(), b"\x03");
        assert_eq!(control("c", Mods { ctrl: true, ..Default::default() }).unwrap(), b"\x03");
        assert_eq!(control("\u{1b}", Mods { alt: true, ..Default::default() }).unwrap(), b"\x1b\x1b");
        assert_eq!(control("\u{19}", Mods { shift: true, ..Default::default() }).unwrap(), b"\x1b[Z");
        assert!(control("a", Mods::default()).is_none());
        assert!(control("ab", Mods::default()).is_none());
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;

    const NONE: Mods = Mods { shift: false, alt: false, ctrl: false };

    #[test]
    fn modifier_codes_follow_xterm() {
        // 1 + shift + 2*alt + 4*ctrl.
        let up = '\u{F700}';
        assert_eq!(special(up, Mods { alt: true, ..NONE }, false).unwrap(), b"\x1b[1;3A");
        assert_eq!(special(up, Mods { ctrl: true, ..NONE }, false).unwrap(), b"\x1b[1;5A");
        assert_eq!(special(up, Mods { shift: true, alt: true, ctrl: true }, false).unwrap(), b"\x1b[1;8A");
        // A modifier wins over application cursor mode.
        assert_eq!(special(up, Mods { ctrl: true, ..NONE }, true).unwrap(), b"\x1b[1;5A");
    }

    #[test]
    fn every_arrow_and_home_end() {
        for (ch, fin) in [('\u{F700}', 'A'), ('\u{F701}', 'B'), ('\u{F703}', 'C'), ('\u{F702}', 'D'), ('\u{F729}', 'H'), ('\u{F72B}', 'F')] {
            assert_eq!(special(ch, NONE, false).unwrap(), format!("\x1b[{fin}").into_bytes());
            assert_eq!(special(ch, NONE, true).unwrap(), format!("\x1bO{fin}").into_bytes());
        }
    }

    #[test]
    fn function_keys() {
        assert_eq!(special('\u{F704}', NONE, false).unwrap(), b"\x1bOP", "F1");
        assert_eq!(special('\u{F707}', NONE, false).unwrap(), b"\x1bOS", "F4");
        assert_eq!(special('\u{F704}', Mods { shift: true, ..NONE }, false).unwrap(), b"\x1b[1;2P");
        assert_eq!(special('\u{F708}', NONE, false).unwrap(), b"\x1b[15~", "F5");
        assert_eq!(special('\u{F70F}', NONE, false).unwrap(), b"\x1b[24~", "F12");
        assert_eq!(special('\u{F70F}', Mods { ctrl: true, ..NONE }, false).unwrap(), b"\x1b[24;5~");
    }

    #[test]
    fn insert_delete_page_keys() {
        assert_eq!(special('\u{F727}', NONE, false).unwrap(), b"\x1b[2~");
        assert_eq!(special('\u{F72C}', NONE, false).unwrap(), b"\x1b[5~");
        assert_eq!(special('\u{F72D}', NONE, false).unwrap(), b"\x1b[6~");
        assert_eq!(special('\u{F728}', Mods { shift: true, ..NONE }, false).unwrap(), b"\x1b[3;2~");
    }

    #[test]
    fn backtab_is_the_same_from_either_path() {
        assert_eq!(special('\u{19}', NONE, false), control("\u{19}", NONE));
    }

    #[test]
    fn control_letters_and_specials() {
        let ctrl = Mods { ctrl: true, ..NONE };
        assert_eq!(control("a", ctrl).unwrap(), [0x01]);
        assert_eq!(control("Z", ctrl).unwrap(), [0x1A]);
        assert_eq!(control(" ", ctrl).unwrap(), [0x00]);
        assert_eq!(control("@", ctrl).unwrap(), [0x00]);
        assert_eq!(control("2", ctrl).unwrap(), [0x00]);
        assert!(control("é", ctrl).is_none());
        assert!(control("", NONE).is_none());
    }

    #[test]
    fn ctrl_delete_is_backspace_and_alt_prefixes_escape() {
        assert_eq!(control("\u{7f}", Mods { ctrl: true, ..NONE }).unwrap(), [0x08]);
        assert_eq!(control("\u{7f}", Mods { alt: true, ..NONE }).unwrap(), [0x1b, 0x7f]);
        assert_eq!(control("b", Mods { ctrl: true, alt: true, ..NONE }).unwrap(), [0x1b, 0x02]);
        assert_eq!(control("\t", NONE).unwrap(), b"\t");
    }

    #[test]
    fn control_codes_appkit_already_made_pass_through() {
        assert_eq!(control("\u{1}", NONE).unwrap(), [0x01]);
        assert_eq!(control("\u{1f}", NONE).unwrap(), [0x1f]);
    }
}
