---
title: Trinidad Head FAQ
description: Answers to common questions about Trinidad Head, the open source Rust terminal for Windows and Mac. Linux support, tabs, Ghostty, privacy, Claude Code, local models and licensing.
---

# FAQ

### What is Trinidad Head?

A terminal app for Windows and macOS, written from scratch in Rust. It runs your shell in a window
with a glass look and a glowing rim, gives each open window its own color, and has a few features
aimed at long Claude Code sessions. See the [home page](index.md).

### Why the name?

From the README: Trinidad Head is a headland on the Northern California coast, tied to the
mainland by a strip of sand. The terminal is meant to tie things together the same way.

### Is it free?

Yes. The source is on GitHub under your choice of the MIT License or the Apache License 2.0, and
the downloads are on the releases page.

### Does it run on Linux?

No. The engine and the Unix pty compile there, but there is no window, and the program prints
`trinidad-head: there is no window for this OS yet`.

### Does it have tabs or split panes?

No. One shell per window. Open more windows instead; each gets its own color.

### Is it built on Ghostty, Alacritty or another terminal?

No. The early research (in `docs/RESEARCH.md` in the repo) looked at building on Ghostty's engine,
and the project then chose to write everything from scratch. The Rust dependencies are platform
bindings (Microsoft's `windows` crates, the `objc2` crates for AppKit), `libc`, and
`unicode-width`. See [Credits](credits.md).

### Does it work with Claude Code?

Yes. Trinidad Head only hosts Claude Code, which is Anthropic's tool. Features aimed at it:
your prompts drawn in a bubble, faint ghost suggestions drawn dim, selecting and copying inside
its full-screen view, editing its prompt like a text box, dropped files typed as paths it can
attach, and OSC 52 so Claude Code's own copy command reaches the system clipboard.

### Does it work with local AI models?

It is a terminal, so any command line tool runs in it. The README recommends it for local
models and pairs it with [Claude Code Local](https://github.com/nicedreamzapp/claude-code-local),
a separate project. Giving each model's window its own color is the main help.

### Does it send any data over the network?

The only network use in the source is the self-updater running `git fetch origin main` on a git
clone of this repo, and only if such a clone is on the machine. There is no telemetry code. Links
open only when you Cmd/Ctrl+click them or pick **Open Link**.

### Where are the settings?

There is one: the default glow color, in a text file. See [Configuration](configuration.md).

### How do I change the font or font size?

You can't without changing the code. The font list and sizes are constants (15 DIPs on Windows,
14 pt Menlo on the Mac).

### Is the download signed?

The Mac download is signed with a Developer ID certificate and notarized by Apple. The Windows
download is not code-signed, so SmartScreen warns on first launch.

### Why does each Mac window show up as a separate process?

Each launch with `open -n` is its own process with its own shell. That keeps windows independent:
closing or crashing one does not touch the others. They still share one Dock icon.

### What happens to programs when I close a window?

On the Mac the shell's whole process group gets `SIGHUP`. On Windows the pseudo console is closed
(`ClosePseudoConsole`). TODO-VERIFY: exactly what Windows does to child processes still running at
that point has not been tested here. Sessions do not outlive the window; the old plan in
`docs/PLAN.md` notes that this is a decision, not a missing feature.

### What does the number in the Windows title mean?

`typing delay N ms (p95 M ms)` is the median and 95th percentile time from a key press to the frame
that shows the echo, over the last 200 keys. It does not include the monitor's own delay. See
[How it works](how-it-works.md#typing-delay-meter).

### How can I help?

Report problems or ideas in [GitHub issues](https://github.com/nicedreamzapp/trinidad-head/issues).
Per the README, contributions are dual-licensed MIT OR Apache-2.0 unless you say otherwise.
