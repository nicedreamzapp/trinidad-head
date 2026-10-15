//! Splitting our own program name off the raw Windows command line.

/// The raw command line minus the program name (quoted or not) and the spaces after it.
pub fn tail(line: &str) -> &str {
    let line = line.trim_start();
    let rest = if let Some(stripped) = line.strip_prefix('"') {
        match stripped.find('"') {
            Some(end) => &stripped[end + 1..],
            None => "",
        }
    } else {
        match line.find([' ', '\t']) {
            Some(end) => &line[end..],
            None => "",
        }
    };
    rest.trim_start()
}

#[cfg(test)]
mod tests {
    use super::tail;

    #[test]
    fn keeps_quotes_in_the_command() {
        assert_eq!(tail(r#""C:\Program Files\th.exe" cmd /c "C:\a b\x.bat""#), r#"cmd /c "C:\a b\x.bat""#);
        assert_eq!(tail(r#"th.exe   powershell.exe"#), "powershell.exe");
        assert_eq!(tail(r#""C:\th.exe""#), "");
        assert_eq!(tail("th.exe"), "");
    }
}

#[cfg(test)]
mod more_tests {
    use super::tail;

    #[test]
    fn leading_spaces_and_tabs_are_skipped() {
        assert_eq!(tail("   th.exe\t\t-NoLogo"), "-NoLogo");
        assert_eq!(tail("\tth.exe cmd"), "cmd");
    }

    #[test]
    fn an_unclosed_quote_leaves_nothing() {
        assert_eq!(tail(r#""C:\Program Files\th.exe cmd"#), "");
    }

    #[test]
    fn an_empty_line_is_empty() {
        assert_eq!(tail(""), "");
        assert_eq!(tail("    "), "");
    }

    #[test]
    fn a_quoted_program_name_needs_no_space_after_it() {
        assert_eq!(tail(r#""th.exe"cmd /k"#), "cmd /k");
    }

    #[test]
    fn spaces_inside_the_command_are_kept() {
        assert_eq!(tail("th.exe  wsl  -d  Ubuntu  "), "wsl  -d  Ubuntu  ");
    }
}
