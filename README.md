# Trinidad Head

Trinidad Head is a headland on the Northern California coast, tied to the mainland by a strip of
sand. This terminal is meant to join things the same way: Windows and Linux in one window, and
people and AI agents working on the same live sessions.

Trinidad Head is written from scratch in Rust. Windows comes first, and a Mac version will follow. It has
no Electron and no borrowed terminal engine. It's early: the first window runs today, and most of
the plan below is still ahead.

## What works now (v0.1)

- **Its own terminal engine** (`core-vt`) turns shell output into a screen of cells: cursor
  movement, erase, scroll regions, 16/256/true color, wide characters, the alternate screen,
  window titles, and OSC 133 shell-integration marks. It has no UI and no OS code, so a web or
  phone viewer can reuse it.
- **Windows ConPTY host** (`pty`) runs PowerShell 7 if it's installed, otherwise Windows
  PowerShell, or any command you pass.
- **A native window** (`trinidad-head`) draws with Direct2D and DirectWrite and is styled like a
  macOS terminal:
  - dark glass: #191d27 at 95% over an acrylic blur
  - 16 px rounded corners
  - red/yellow/green window buttons and a centered title
- **A built-in typing-delay meter.** The title bar shows the median and p95 time from key press
  to finished frame. It doesn't include the monitor's own delay.
- Scrollback with the mouse wheel and Shift+PgUp/PgDn, right-click or Ctrl+Shift+V paste with
  bracketed paste, Alt as Meta, and function and navigation keys with modifiers.

## Where it's going

See [docs/PLAN.md](docs/PLAN.md). In short:

- **Sessions outlive the window.** A background service owns every shell, so a crash or a closed
  window doesn't lose your work, and a second window, a phone or a web page can attach to the same
  session.
- **Linux and Windows as one system.** WSL and Windows shells run side by side with shared paths,
  clipboard and history.
- **Output is recorded as structured blocks, not only pixels.** Every command becomes a record of
  the command, its output and its exit code that tools and AI can search.
- **AI agents are users with rules.** Agents can only see their own tabs, risky commands can wait
  for approval, and everything agents type goes into an audit log. It isn't tied to one model
  provider.
- **Accessible from day one:** a real UI Automation tree for screen readers.
- **Plugins:** sandboxed WebAssembly.

[docs/RESEARCH.md](docs/RESEARCH.md) collects the pain points of existing terminals that shaped
these choices.

## Build (Windows)

Trinidad Head builds without Visual Studio, using the LLVM-based MinGW toolchain:

```
rustup default stable-x86_64-pc-windows-gnullvm
# put llvm-mingw (https://github.com/mstorsjo/llvm-mingw) on PATH
cargo build --release
target\release\trinidad-head.exe            # default shell
target\release\trinidad-head.exe cmd.exe    # or any command
```

The engine's tests run on any OS: `cargo test -p core-vt`.

## Layout

```
crates/core-vt   terminal parser + screen state (portable)
crates/pty       pseudo-console host (Windows ConPTY today)
crates/app       the window (binary: trinidad-head)
docs/            plan and research
```

© 2026 Matt Macosko. All rights reserved for now; a license will be chosen before the first release.
