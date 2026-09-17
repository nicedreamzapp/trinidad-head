//! pty: runs a shell behind a pseudo console and hands back its byte streams.
//!
//! Windows uses ConPTY; macOS and Linux use a Unix pty. WSL and SSH plug in behind the same
//! `Pty` shape later.

#[cfg(windows)]
mod conpty;
#[cfg(windows)]
pub use conpty::Pty;
#[cfg(unix)]
mod unixpty;
#[cfg(unix)]
pub use unixpty::Pty;

/// On Unix an empty command means "the user's interactive login shell".
#[cfg(unix)]
pub fn default_shell() -> String {
    String::new()
}

/// The shell to start when the user didn't name one: PowerShell 7 if installed, else Windows PowerShell.
#[cfg(windows)]
pub fn default_shell() -> String {
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
    let pwsh = format!(r"{pf}\PowerShell\7\pwsh.exe");
    if std::path::Path::new(&pwsh).exists() {
        format!("\"{pwsh}\" -NoLogo")
    } else {
        "powershell.exe -NoLogo".into()
    }
}
