//! Embeds the app icon on Windows using the MinGW resource compiler that ships with llvm-mingw.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    // The commit this binary was built from, so it can tell whether main has moved on.
    let sha = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=TH_GIT_SHA={sha}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads/main");

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
