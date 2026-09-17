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
