---
title: Trinidad Head credits and licenses
description: Who built Trinidad Head and the upstream projects it relies on, including Rust, Microsoft's windows crates, the objc2 crates, unicode-width, llvm-mingw and Anthropic's Claude Code.
---

# Credits

## Trinidad Head

Trinidad Head is built by **Matt Macosko**
([project page](https://nicedreamzwholesale.com/software/trinidad-head/)). Some commits in the
history are co-authored with Claude, Anthropic's AI model, and are marked that way in git.

Licensed under either the [MIT License](https://github.com/nicedreamzapp/trinidad-head/blob/main/LICENSE-MIT)
or the [Apache License 2.0](https://github.com/nicedreamzapp/trinidad-head/blob/main/LICENSE-APACHE),
at your option.

## Upstream projects

Trinidad Head's terminal engine, pty host and windows are its own code. It builds on these
projects, which belong to their own authors:

| Project | Used for |
|---|---|
| [Rust](https://www.rust-lang.org/) and Cargo | The language and build system. |
| [`windows`](https://crates.io/crates/windows), `windows-core`, `windows-numerics` (Microsoft) | Win32, ConPTY, Direct3D 11, DirectComposition, Direct2D and DirectWrite bindings on Windows. |
| [`objc2`](https://crates.io/crates/objc2), `objc2-foundation`, `objc2-app-kit`, `objc2-core-graphics`, `objc2-core-foundation`, `block2`, `dispatch2` | AppKit, CoreGraphics and Grand Central Dispatch bindings on macOS. |
| [`libc`](https://crates.io/crates/libc) | `openpty`, signals, `flock` and other Unix calls. |
| [`unicode-width`](https://crates.io/crates/unicode-width) | How many cells a character takes. |
| [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (Martin Storsjö) | The Windows toolchain and resource compiler used to build without Visual Studio. |
| Microsoft Windows pseudo console (ConPTY) | Running shells on Windows. |
| Apple AppKit, CoreGraphics, `codesign`, `notarytool` | The Mac window and the signed download. |
| Cascadia Mono / Code, Consolas, Menlo | Fonts the app asks the system for; not bundled. |

## Programs it is made to host

- [Claude Code](https://www.anthropic.com/claude-code) is Anthropic's tool. Trinidad Head only hosts
  it; several features are tuned to how Claude Code draws its screen.
- [Claude Code Local](https://github.com/nicedreamzapp/claude-code-local) is a separate project by
  the same author for running Claude Code against local models.

## Prior art

[Ghostty](https://ghostty.org/) was the terminal this one replaced on the author's Mac. Early
research looked at building on its engine before choosing to write everything from scratch, and
the Mac helper `th-open` understands the `.ghostty` and `.command` files that the author's Ghostty
launcher used. No Ghostty code is included.

## This website

Built with [MkDocs](https://www.mkdocs.org/) and
[Material for MkDocs](https://squidfunk.github.io/mkdocs-material/).
