# Trinidad Head

Trinidad Head is a headland on the Northern California coast, tied to the mainland by a strip of
sand. This terminal is meant to join things the same way: Windows and Linux in one window, and
people and AI agents working on the same live sessions.

Trinidad Head is written from scratch in Rust and runs on **Windows and macOS** from one codebase.
It has no Electron and no borrowed terminal engine. It's young but in daily use.

![A Trinidad Head window: dark glass with a neon rim](docs/images/window.jpg)

![Three windows open at once, each with its own glow color](docs/images/colors.jpg)

## What works now

- **Its own terminal engine** (`core-vt`) turns shell output into a screen of cells: cursor
  movement, erase, scroll regions, 16/256/true color, wide characters, the alternate screen,
  window titles, and OSC 133 shell-integration marks. It has no UI and no OS code, so a web or
  phone viewer can reuse it.
- **Shell host** (`pty`): ConPTY on Windows (PowerShell 7 if installed, otherwise Windows
  PowerShell) and a Unix pty on macOS (your login shell), or any command you pass.
- **Its own window chrome on both systems:** a borderless, see-through window drawn pixel by
  pixel. Windows uses Direct2D, DirectWrite and DirectComposition; the Mac uses AppKit,
  CoreGraphics and CoreText.
  - rounded glass body with a rippled edge and a neon rim that glows
  - red/yellow/green window buttons, a small sidebar, and a "///" resize corner
- **A different color for every window.** Eight glow themes; each new window picks one no other
  open window is using.
- **Your own prompts stand out.** Claude Code's "your message" bar is drawn as a soft pill,
  with bigger green text, so it's easy to find your questions when you scroll back.
- **Works with full-screen terminal apps.** Mouse reporting (SGR), OSC 52 copy, bracketed paste
  and focus events. Shift+drag always selects, and right-click always pastes.
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

## Build (macOS)

```
scripts/build-mac-app.sh                       # builds ~/Applications/Trinidad Head.app
open -n -a "Trinidad Head" --args htop         # each launch is its own window
```

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
crates/pty       shell host (ConPTY on Windows, Unix pty on macOS)
crates/app       the window (binary: trinidad-head); src/mac for macOS
mac-tools/       helpers for launching and typing into windows on macOS
docs/            plan and research
```

© 2026 Matt Macosko. All rights reserved for now; a license will be chosen before the first release.
