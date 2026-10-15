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

#[cfg(test)]
mod more_tests {
    use super::Pty;
    use std::io::Read;
    use std::time::{Duration, Instant};

    /// Read the shell's output until `want` shows up, the stream ends, or 10 seconds pass.
    fn read_until(out: &mut std::fs::File, want: &str) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
            if String::from_utf8_lossy(&buf).contains(want) {
                break;
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    #[test]
    fn what_is_written_reaches_the_program() {
        let mut pty = Pty::spawn("cat", None, 80, 24).unwrap();
        let mut out = pty.take_output().unwrap();
        pty.write(b"ping from the test\n").unwrap();
        let s = read_until(&mut out, "ping from the test");
        assert!(s.contains("ping from the test"), "{s}");
    }

    #[test]
    fn the_output_stream_is_handed_out_once() {
        let mut pty = Pty::spawn("true", None, 80, 24).unwrap();
        assert!(pty.take_output().is_some());
        assert!(pty.take_output().is_none());
    }

    #[test]
    fn resizing_reaches_the_program() {
        let mut pty = Pty::spawn("sleep 0.5; stty size", None, 80, 24).unwrap();
        let mut out = pty.take_output().unwrap();
        pty.resize(132, 43).unwrap();
        let s = read_until(&mut out, "43 132");
        assert!(s.contains("43 132"), "{s}");
    }

    #[test]
    fn a_zero_size_is_treated_as_one_by_one() {
        let mut pty = Pty::spawn("stty size", None, 0, 0).unwrap();
        let mut out = pty.take_output().unwrap();
        let s = read_until(&mut out, "1 1");
        assert!(s.contains("1 1"), "{s}");
    }

    #[test]
    fn reports_the_exit_code() {
        let mut pty = Pty::spawn("exit 3", None, 80, 24).unwrap();
        let _out = pty.take_output();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut code = None;
        while Instant::now() < deadline && code.is_none() {
            code = pty.exit_code();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(code, Some(3));
    }

    #[test]
    fn the_program_sees_a_color_terminal() {
        let mut pty = Pty::spawn("echo \"[$TERM $COLORTERM $TERM_PROGRAM]\"", None, 80, 24).unwrap();
        let mut out = pty.take_output().unwrap();
        let s = read_until(&mut out, "TrinidadHead]");
        assert!(s.contains("[xterm-256color truecolor TrinidadHead]"), "{s}");
    }

    #[test]
    fn claude_code_session_variables_do_not_leak_into_the_shell() {
        // A claude started in a new window must not think it is a child session.
        std::env::set_var("CLAUDE_TH_LEAK_TEST", "leaked");
        let mut pty = Pty::spawn("echo \"[${CLAUDE_TH_LEAK_TEST-gone}]\"", None, 80, 24).unwrap();
        let mut out = pty.take_output().unwrap();
        let s = read_until(&mut out, "[gone]");
        std::env::remove_var("CLAUDE_TH_LEAK_TEST");
        assert!(s.contains("[gone]") && !s.contains("leaked"), "{s}");
    }

    #[test]
    fn starts_in_the_folder_asked_for() {
        let dir = std::env::temp_dir().canonicalize().unwrap();
        let mut pty = Pty::spawn("pwd -P", Some(dir.to_str().unwrap()), 200, 24).unwrap();
        let mut out = pty.take_output().unwrap();
        let want = dir.to_string_lossy().into_owned();
        let s = read_until(&mut out, &want);
        assert!(s.contains(&want), "{s}");
    }

    #[test]
    fn dropping_it_ends_the_shell() {
        let pty = Pty::spawn("sleep 30", None, 80, 24).unwrap();
        let pid = pty.pid() as libc::pid_t;
        assert!(pid > 0);
        drop(pty);
        // Killed and reaped: the pid no longer names a process of ours.
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
    }
}
