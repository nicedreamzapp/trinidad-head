//! Embeds the app icon on Windows using the MinGW resource compiler that ships with llvm-mingw.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=app.rc");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("app_res.o");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "x86_64".into());
    let tool = format!("{arch}-w64-mingw32-windres");
    match Command::new(&tool).args(["app.rc", "-O", "coff", "-o"]).arg(&out).status() {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", out.display()),
        // No resource compiler (e.g. a check build on another OS): build without an icon.
        _ => println!("cargo:warning={tool} not found; building without the app icon"),
    }
}
