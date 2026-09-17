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
