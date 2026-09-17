//! The terminal window.
//!
//! First piece: one Win32 window running one shell through ConPTY, drawn with Direct2D +
//! DirectWrite, with a built-in typing-delay meter shown in the title bar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code))]
mod latency;
#[cfg(windows)]
mod win;

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("our-terminal: only the Windows window exists so far");
}
