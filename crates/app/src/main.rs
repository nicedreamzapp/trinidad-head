//! The terminal window.
//!
//! First piece: one Win32 window running one shell through ConPTY, drawn with Direct2D +
//! DirectWrite, with a built-in typing-delay meter shown in the title bar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod cmdline;
#[cfg_attr(not(windows), allow(dead_code))]
mod latency;
#[cfg(windows)]
mod gfx;
#[cfg_attr(not(windows), allow(dead_code))]
mod layout;
#[cfg_attr(not(windows), allow(dead_code))]
mod theme;
#[allow(dead_code)]
mod textutil;
#[cfg(windows)]
mod win;
#[cfg(target_os = "macos")]
mod mac;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(target_os = "macos")]
fn main() {
    mac::run();
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("trinidad-head: there is no window for this OS yet");
}
