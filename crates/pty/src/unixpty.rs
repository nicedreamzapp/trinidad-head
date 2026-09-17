use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};

/// A running shell attached to a Unix pseudo terminal.
pub struct Pty {
    master: File,
    output: Option<File>,
    child: Child,
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows.max(1), ws_col: cols.max(1), ws_xpixel: 0, ws_ypixel: 0 }
}

impl Pty {
    /// Start `command_line` through the user's login shell, or an interactive login shell when
    /// it is empty, in a `cols` x `rows` terminal.
    pub fn spawn(command_line: &str, cwd: Option<&str>, cols: u16, rows: u16) -> io::Result<Pty> {
        let (mut master_fd, mut slave_fd) = (0, 0);
        let mut ws = winsize(cols, rows);
        let rc = unsafe {
            libc::openpty(&mut master_fd, &mut slave_fd, std::ptr::null_mut(), std::ptr::null_mut(), &mut ws)
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { OwnedFd::from_raw_fd(master_fd) };
        let slave = unsafe { OwnedFd::from_raw_fd(slave_fd) };
        unsafe {
            // Our end of the pty must not leak into the child.
            libc::fcntl(master.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC);
        }

        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/zsh".into());
        let mut cmd = Command::new(&shell);
        cmd.arg("-l");
        if !command_line.trim().is_empty() {
            cmd.arg("-c").arg(command_line);
        }
        // Claude Code session markers must never leak into a new window: a claude started
        // there would think it's a child session and stop saving its transcript.
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("CLAUDE") {
                cmd.env_remove(&k);
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        cmd.current_dir(cwd.unwrap_or(&home))
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "TrinidadHead")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        unsafe {
            cmd.pre_exec(|| {
                // New session with the pty as its controlling terminal, so job control and
                // Ctrl+C reach the right processes.
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn()?;

        let master = File::from(master);
        let output = master.try_clone()?;
        Ok(Pty { master, output: Some(output), child })
    }

    /// The shell's output stream. Call once and read it on its own thread.
    pub fn take_output(&mut self) -> Option<File> {
        self.output.take()
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> io::Result<()> {
        let ws = winsize(cols, rows);
        let rc = unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// `Some(exit_code)` once the shell has exited.
    pub fn exit_code(&mut self) -> Option<u32> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code().unwrap_or(1) as u32),
            _ => None,
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let pid = self.child.id() as libc::pid_t;
        unsafe {
            // The shell leads its own process group; hang up on the whole group.
            libc::kill(-pid, libc::SIGHUP);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::Pty;
    use std::io::Read;

    #[test]
    fn runs_a_command_and_reads_its_output() {
        let mut pty = Pty::spawn("printf 'hi from pty'; stty size", None, 90, 30).unwrap();
        let mut out = pty.take_output().unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            let s = String::from_utf8_lossy(&buf);
            if s.contains("hi from pty") && s.contains("30 90") {
                break;
            }
        }
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("hi from pty"), "{s}");
        assert!(s.contains("30 90"), "{s}");
    }
}
