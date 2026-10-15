---
title: Install Trinidad Head on Mac or Windows
description: How to install the Trinidad Head terminal from the signed Mac download or the Windows zip, or build it from source with Rust on macOS and Windows.
---

# Install

Trinidad Head runs on **macOS 14 or newer** (Apple Silicon and Intel) and on **Windows**. The
README lists Windows 10 and 11. There is no Linux window.

## Download

| System | File | What to do |
|---|---|---|
| macOS | [Trinidad-Head-mac.zip](https://github.com/nicedreamzapp/trinidad-head/releases/latest/download/Trinidad-Head-mac.zip) | Unzip, drag **Trinidad Head** into Applications, open it. |
| Windows | [Trinidad-Head-windows.zip](https://github.com/nicedreamzapp/trinidad-head/releases/latest/download/Trinidad-Head-windows.zip) | Unzip and double-click **Trinidad Head.exe**. |

**Mac.** The download is one universal app (built for both `aarch64-apple-darwin` and
`x86_64-apple-darwin` and joined with `lipo`), signed with a Developer ID certificate, notarized
and stapled by `scripts/package-release.sh`, so it opens without a warning.

**Windows.** The zip holds a single exe and nothing else. It is linked with the C runtime built in
(`+crt-static` in `.cargo/config.toml`), so no extra DLLs are needed. It is not code-signed, so if
a blue "Windows protected your PC" box shows, click **More info**, then **Run anyway**.

All releases are listed on the [releases page](https://github.com/nicedreamzapp/trinidad-head/releases).

## Build from source on a Mac

You need the Rust toolchain. The build script looks for cargo at `~/.cargo/bin/cargo` unless you
set `CARGO`. It also uses `sips`, `iconutil` and `codesign`, which ship with macOS.

```bash
git clone https://github.com/nicedreamzapp/trinidad-head.git
cd trinidad-head
scripts/build-mac-app.sh              # puts "Trinidad Head" in ~/Applications
open -n -a "Trinidad Head"            # each launch opens its own window
```

The script signs the app with the first **Apple Development** certificate in your keychain. If you
have none, it stops with:

```text
no Apple Development certificate on this Mac.
Set TH_SIGN_IDENTITY, or TH_ALLOW_ADHOC=1 to accept losing the app's permissions.
```

Run `TH_ALLOW_ADHOC=1 scripts/build-mac-app.sh` to sign ad hoc instead. An ad hoc signature
changes with every build, so macOS forgets permissions you granted the app (Full Disk Access,
Accessibility, Screen Recording) each time you rebuild. Other build options are listed under
[Configuration](configuration.md#build-script-variables).

### Optional helper commands (Mac)

`mac-tools/install.sh` copies four small helpers into `~/Scripts/trinidad-head`:

| Command | What it does |
|---|---|
| `th-open <file.command or file.ghostty>...` | Opens each file in its own new window and prints the window id (`th:<token>`). |
| `th-type [--escape-first] [--no-enter] <token> <text>` | Types text into a window in the background, then presses Return. |
| `th-alive <token>` | Prints `yes` if that window is open, else `no`. |
| `th-self-id` | Inside a Trinidad Head window, prints this window's id. |

See [How it works](how-it-works.md#control-socket-mac) for the socket these use.

## Build from source on Windows

No Visual Studio is needed. The project builds with the `gnullvm` Rust target and
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw).

```powershell
rustup default stable-x86_64-pc-windows-gnullvm
# add llvm-mingw to PATH, then:
cargo build --release
target\release\trinidad-head.exe
```

`build.rs` embeds the app icon with `x86_64-w64-mingw32-windres` from llvm-mingw. If that tool
is not on `PATH`, the build still succeeds and prints
`x86_64-w64-mingw32-windres not found; building without the app icon`.

To make the release zip, run:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File pc-tools\package-release.ps1
```

which writes `dist\Trinidad-Head-windows.zip`.

!!! note "pc-tools/install_th.ps1"
    This script copies a fresh build into `%LOCALAPPDATA%\Programs\TrinidadHead`. Its source path
    is hard-coded to the author's own clone location, so on another machine edit `$src` first.

## Linux

The code compiles the engine and Unix pty on Linux, but there is no window. Running the binary
prints `trinidad-head: there is no window for this OS yet`.

## Updating

There is no download updater. The app updates itself only when a git clone of this repo is on the
same machine in one of the places it looks, and then it rebuilds from that clone. See
[the self-updater](how-it-works.md#self-updater) for the rules it follows, and
[`TRINIDAD_HEAD_NO_UPDATE`](configuration.md#environment-variables) to turn it off.
