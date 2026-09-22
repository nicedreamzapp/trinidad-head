//! Keeping itself up to date.
//!
//! Every machine that runs Trinidad Head also has the repo and the Rust toolchain, so there is
//! nothing to download and no second copy of the binary to sign or trust: at launch it asks
//! git whether main has moved, and if it has, it runs the same build script a person would.
//! The window already open keeps the build it started with — the next one Matt opens is the
//! new one. That is the whole point: he should not have to be told to update three computers.
//!
//! It never touches a repo with local changes, never runs during a self-test, and a failed
//! build leaves the installed app exactly as it was.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The commit this binary was built from (baked in by build.rs).
pub const BUILT_FROM: &str = env!("TH_GIT_SHA");

fn log_path() -> Option<PathBuf> {
    Some(state_dir()?.join("update.log"))
}

#[cfg(target_os = "macos")]
fn state_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/TrinidadHead"))
}

#[cfg(windows)]
fn state_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("TrinidadHead"))
}

fn note(line: &str) {
    if let Some(p) = log_path() {
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            let _ = writeln!(f, "{} {line}", now());
        }
    }
}

fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("[{secs}]")
}

/// Where this machine keeps the repo. TRINIDAD_HEAD_REPO wins, then the usual spots.
fn repo() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("TRINIDAD_HEAD_REPO") {
        let p = PathBuf::from(p);
        return is_repo(&p).then_some(p);
    }
    let mut guesses: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        guesses.push(PathBuf::from(&home).join("Desktop/PROJECTS/trinidad-head"));
        guesses.push(PathBuf::from(&home).join("dev/trinidad-head"));
        guesses.push(PathBuf::from(&home).join("trinidad-head"));
    }
    guesses.into_iter().find(|p| is_repo(p))
}

fn is_repo(p: &Path) -> bool {
    p.join(".git").exists() && p.join("crates/app/Cargo.toml").exists()
}

/// Windows hands a console program its own console when the parent has none, and Trinidad Head
/// is a windowed program with no console, so every git and every cargo the updater runs would
/// otherwise flash up a terminal window of its own — Windows Terminal, if that is the default.
/// The update is meant to be invisible; this keeps it that way.
#[cfg(windows)]
fn quiet(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
fn quiet(cmd: &mut Command) -> &mut Command {
    cmd
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let out = quiet(Command::new("git").current_dir(dir).args(args)).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Test hook: run the check on this thread, say what happened and exit. With
/// TRINIDAD_HEAD_UPDATE_DRYRUN it stops short of building, so a test can prove the decision
/// without a twenty-second build and a real install.
pub fn run_once_and_exit() -> ! {
    let outcome = match check() {
        Ok(Outcome::UpToDate) => "up to date".to_string(),
        Ok(Outcome::WouldInstall(sha)) => format!("would install {}", short(&sha)),
        Ok(Outcome::Installed(sha)) => format!("installed {}", short(&sha)),
        Err(why) => format!("skipped: {why}"),
    };
    println!("{outcome} (this build came from {})", short(BUILT_FROM));
    std::process::exit(0)
}

pub enum Outcome {
    UpToDate,
    WouldInstall(String),
    Installed(String),
}

/// Check for a newer main and, if there is one, build and install it. Runs off the main
/// thread; call it once at startup.
pub fn spawn_check() {
    if std::env::var_os("TRINIDAD_HEAD_SELFTEST").is_some() || std::env::var_os("TRINIDAD_HEAD_NO_UPDATE").is_some() {
        return;
    }
    std::thread::spawn(|| match check() {
        Err(why) => note(&format!("skipped: {why}")),
        Ok(Outcome::Installed(sha)) => note(&format!("installed {}; the next window you open is the new one", short(&sha))),
        Ok(_) => {}
    });
}

fn check() -> Result<Outcome, String> {
    let dir = repo().ok_or("no repo on this machine")?;
    // Never build over someone's half-finished work.
    let dirty = git(&dir, &["status", "--porcelain"]).ok_or("git status failed")?;
    if !dirty.is_empty() {
        return Err("the repo has local changes".into());
    }
    git(&dir, &["fetch", "--quiet", "origin", "main"]).ok_or("cannot reach origin")?;
    let head = git(&dir, &["rev-parse", "origin/main"]).ok_or("no origin/main")?;
    if head == BUILT_FROM {
        return Ok(Outcome::UpToDate);
    }
    let branch = git(&dir, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if branch != "main" {
        return Err(format!("on branch {branch}, not main"));
    }
    note(&format!("main moved to {}, was built from {}", short(&head), short(BUILT_FROM)));
    git(&dir, &["merge", "--ff-only", "origin/main"]).ok_or("cannot fast-forward to origin/main")?;
    if std::env::var_os("TRINIDAD_HEAD_UPDATE_DRYRUN").is_some() {
        return Ok(Outcome::WouldInstall(head));
    }
    build_and_install(&dir)?;
    Ok(Outcome::Installed(head))
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

#[cfg(target_os = "macos")]
fn build_and_install(dir: &Path) -> Result<(), String> {
    // build-mac-app.sh signs with the Apple Development certificate. We are inside the
    // logged-in session here (the app was launched by the person), so the keychain is open
    // and the signature — and the app's permissions with it — survive.
    run(Command::new("bash").current_dir(dir).arg("scripts/build-mac-app.sh"))
}

#[cfg(windows)]
fn build_and_install(dir: &Path) -> Result<(), String> {
    run(Command::new("cargo").current_dir(dir).args(["build", "--release"]))?;
    run(Command::new("powershell")
        .current_dir(dir)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "pc-tools\\install_th.ps1"]))
}

fn run(cmd: &mut Command) -> Result<(), String> {
    // quiet() here covers the whole build: cargo gets a console with no window, and rustc and
    // the linker under it inherit that console rather than each opening one of their own.
    let out = quiet(cmd).output().map_err(|e| format!("{e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let tail: String = String::from_utf8_lossy(&out.stderr).lines().rev().take(3).collect::<Vec<_>>().join(" | ");
        Err(format!("build failed: {tail}"))
    }
}
