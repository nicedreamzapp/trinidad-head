//! One Dock icon for every Trinidad Head window.
//!
//! Each window is its own process (`open -n`), and every process that is a regular app gets its
//! own Dock tile, so eight windows meant eight icons, and any window that was killed rather than
//! closed left its tile behind. Now only one process, the lead, is a regular app. The others run
//! as accessory apps: their windows look and type exactly the same, they just have no tile.
//!
//! The lead is whoever holds an exclusive lock on `~/.trinidad-head/dock.lock`. The kernel drops
//! the lock when the process ends, however it ends, so when the lead's window closes (or is
//! killed) the next waiting window gets the lock and puts the icon back. The Info.plist starts
//! every launch as an accessory (LSUIElement), so no tile flashes up before this decides.
//!
//! Clicking the icon brings every Trinidad Head window forward, not just the lead's.

use std::fs::File;
use std::os::fd::AsRawFd;
use std::sync::OnceLock;

use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSRunningApplication};
use objc2_foundation::NSString;

static LOCK: OnceLock<File> = OnceLock::new();

const BUNDLE_ID: &str = "com.nicedreamz.trinidadhead";

fn lock_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    std::path::PathBuf::from(home).join(".trinidad-head").join("dock.lock")
}

/// Take the icon if nobody has it; otherwise wait in the background and take it when the lead
/// goes away. Self-test windows never take it, so a test run can't leave a tile in the Dock.
pub fn start(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    if std::env::var_os("TRINIDAD_HEAD_SELFTEST").is_some() {
        return;
    }
    let path = lock_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path) else {
        // No lock file means no way to agree on a lead: keep the old one-icon-per-window rule
        // rather than leave a window with no icon at all.
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        return;
    };
    let fd = file.as_raw_fd();
    let _ = LOCK.set(file);
    if unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        return;
    }
    std::thread::spawn(move || {
        if unsafe { libc::flock(fd, libc::LOCK_EX) } == 0 {
            dispatch2::DispatchQueue::main().exec_async(|| {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).setActivationPolicy(NSApplicationActivationPolicy::Regular);
                }
            });
        }
    });
}

/// The Dock icon was clicked: bring every Trinidad Head window forward, this one last so it
/// ends up on top with the keyboard.
pub fn bring_all_forward() {
    let me = std::process::id() as libc::pid_t;
    let apps = NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(BUNDLE_ID));
    let mut own = None;
    for app in apps.iter() {
        if app.processIdentifier() == me {
            own = Some(app);
        } else {
            #[allow(deprecated)]
            app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
    }
    if let Some(app) = own {
        #[allow(deprecated)]
        app.activateWithOptions(
            NSApplicationActivationOptions::ActivateAllWindows | NSApplicationActivationOptions::ActivateIgnoringOtherApps,
        );
    }
}
