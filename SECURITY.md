# Security policy

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report them privately through GitHub: go to the **Security** tab of this repo and click **Report a vulnerability** ([direct link](https://github.com/nicedreamzapp/trinidad-head/security/advisories/new)). Only the maintainer sees the report.

Helpful details: what you found, how to reproduce it, the release or commit, and your OS and version. If it is triggered by printed output, a small file or command that reproduces it helps a lot.

I aim to reply within 3 days and to ship a fix or a clear plan within 30 days, depending on severity. You will be credited in the advisory unless you ask not to be.

## Supported versions

Fixes go into the latest release and `main`. Please update before reporting if you are on an older version.

## What matters most here

The parts where a mistake could hurt someone:

- the per-window control socket in `crates/app/src/mac/control.rs` (`~/.trinidad-head/<token>.sock`) and the `mac-tools/th-type` helper, which can type text and keys into a shell
- the self-updater in `crates/app/src/update.rs`, which checks git at launch and rebuilds the app when `main` has moved
- the terminal engine in `crates/core-vt/src/lib.rs`, which parses everything programs print, including output from files and servers you don't control
- the shell hosts in `crates/pty/` (ConPTY on Windows, a Unix pty on the Mac)
- release packaging and signing in `scripts/build-mac-app.sh`, `scripts/package-release.sh` and `pc-tools/package-release.ps1`
