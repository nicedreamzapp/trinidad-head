//! Outside control of one window, and the list of open windows.
//!
//! Every window has a token (`TRINIDAD_HEAD_TOKEN`, handed to its shell too) and listens on
//! `~/.trinidad-head/<token>.sock`. One command per connection:
//!   `ping`          → `ok`
//!   `type <text>`   → the text goes to the shell as if pasted (bracketed when the program asked)
//!   `key enter` / `key escape` / `key tab` / `key ctrl-c`
//! This is how dictation and iMessage mode type into a window in the background, the way
//! Ghostty's `input text` / `send key` did.
//!
//! `~/.trinidad-head/windows/<pid>` holds each open window's glow theme, so every window can
//! pick a color no other open window uses.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pty::Pty;

use super::Shared;

fn base() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(home).join(".trinidad-head")
}

/// This window's token: the one we were launched with, or a new one.
pub fn token() -> String {
    if let Ok(t) = std::env::var("TRINIDAD_HEAD_TOKEN") {
        let t: String = t.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if !t.is_empty() {
            return t;
        }
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("th-{}-{:x}", std::process::id(), now.as_nanos())
}

pub fn socket_path(token: &str) -> PathBuf {
    base().join(format!("{token}.sock"))
}

fn windows_dir() -> PathBuf {
    base().join("windows")
}

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

/// Glow themes used by other open windows. Files left by windows that are gone are removed.
pub fn glows_in_use() -> Vec<usize> {
    let me = std::process::id();
    let mut used = Vec::new();
    let Ok(dir) = std::fs::read_dir(windows_dir()) else { return used };
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        if !alive(pid) {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        if let Some(g) = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("glow=").map(str::to_string)))
            .and_then(|g| g.trim().parse::<usize>().ok())
        {
            used.push(g);
        }
    }
    used
}

pub fn register(glow: usize, token: &str) {
    let dir = windows_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(std::process::id().to_string()), format!("glow={glow}\ntoken={token}\n"));
    prune_sockets();
}

/// Remove sockets left by windows that were killed before they could clean up.
fn prune_sockets() {
    let _ = glows_in_use(); // drops entries of dead windows
    let mut live = Vec::new();
    if let Ok(dir) = std::fs::read_dir(windows_dir()) {
        for e in dir.flatten() {
            if let Ok(t) = std::fs::read_to_string(e.path()) {
                live.extend(t.lines().filter_map(|l| l.strip_prefix("token=").map(str::to_string)));
            }
        }
    }
    let Ok(dir) = std::fs::read_dir(base()) else { return };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(tok) = name.strip_suffix(".sock") {
            if !live.iter().any(|l| l == tok) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Remove this window's socket and registry entry.
pub fn cleanup(token: &str) {
    let _ = std::fs::remove_file(socket_path(token));
    let _ = std::fs::remove_file(windows_dir().join(std::process::id().to_string()));
}

/// Bytes for one control command, or an error message.
fn bytes_for(cmd: &str, bracketed: bool) -> Result<Vec<u8>, String> {
    let (verb, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match verb {
        "type" => {
            let text = rest.replace("\r\n", "\r").replace('\n', "\r");
            let mut out = Vec::with_capacity(text.len() + 12);
            if bracketed {
                out.extend_from_slice(b"\x1b[200~");
            }
            out.extend_from_slice(text.as_bytes());
            if bracketed {
                out.extend_from_slice(b"\x1b[201~");
            }
            Ok(out)
        }
        "key" => match rest.trim() {
            "enter" | "return" => Ok(b"\r".to_vec()),
            "escape" | "esc" => Ok(b"\x1b".to_vec()),
            "tab" => Ok(b"\t".to_vec()),
            "ctrl-c" => Ok(b"\x03".to_vec()),
            other => Err(format!("unknown key {other}")),
        },
        other => Err(format!("unknown command {other}")),
    }
}

/// Listen for control commands on this window's socket (runs on its own thread).
pub fn listen(token: &str, shared: Arc<Mutex<Shared>>, pty: Arc<Mutex<Pty>>) {
    let path = socket_path(token);
    let _ = std::fs::create_dir_all(base());
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("trinidad-head: control socket {path:?}: {e}");
            return;
        }
    };
    // Only this user may type into the window.
    unsafe {
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap_or_default();
        libc::chmod(c.as_ptr(), 0o600);
    }
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            let cmd = String::from_utf8_lossy(&buf);
            let cmd = cmd.strip_suffix('\n').unwrap_or(&cmd);
            let reply = if cmd.trim() == "ping" {
                "ok".to_string()
            } else {
                let bracketed = shared.lock().unwrap().term.bracketed_paste;
                match bytes_for(cmd, bracketed) {
                    Ok(bytes) => match pty.lock().unwrap().write(&bytes) {
                        Ok(()) => "ok".to_string(),
                        Err(e) => format!("error {e}"),
                    },
                    Err(e) => format!("error {e}"),
                }
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::bytes_for;

    #[test]
    fn commands() {
        assert_eq!(bytes_for("type hi\nthere", false).unwrap(), b"hi\rthere");
        assert_eq!(bytes_for("type hi", true).unwrap(), b"\x1b[200~hi\x1b[201~");
        assert_eq!(bytes_for("key enter", false).unwrap(), b"\r");
        assert_eq!(bytes_for("key escape", false).unwrap(), b"\x1b");
        assert!(bytes_for("launch rockets", false).is_err());
    }
}
