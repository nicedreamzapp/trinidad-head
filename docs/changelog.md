---
title: Trinidad Head changelog and release history
description: Release history of the Trinidad Head terminal, from git tags and commit history. Versions 0.1.0 and 0.2.0 and the changes merged since.
---

# Changelog

Built from the repo's git tags, commit history and the text of the two GitHub releases. Dates are
commit dates; release publish dates are noted where they differ.

## Unreleased (since v0.2.0)

Changes on `main` after the `v0.2.0` tag. The version in `Cargo.toml` is still 0.2.0.

- Mac download: Developer ID signature, hardened runtime, notarized and stapled
  (`5a6a89d`, 2026-09-20).
- Windows: select and copy out of Claude Code through the recorded document instead of a harvest
  of the screen; paint the highlight from that document too; the mouse wheel follows the Mac's rule
  while a selection is live (`9421b17`, `8a7035a`, `51c32f3`, 2026-09-21).
- Keep the scrolled-past document for Claude Code, which never takes the alternate screen
  (`c57ce54`, 2026-09-21).
- Windows: the self-updater's git and cargo no longer open console windows (`2effa47`,
  2026-09-22).
- Mac: microphone and speech recognition usage strings in `Info.plist`, so programs that dictate
  can ask for them (`d3f0a5e`, 2026-09-22).
- Mac: one Dock icon for every window (`174a284`, 2026-09-22).
- README rewrite (`f0f86dd`, merged in pull request 1 on 2026-09-27).

## v0.2.0 (tagged 2026-09-20, published 2026-09-21)

The first version with downloads: a universal Mac app zip and a Windows exe zip.

- **Edit Claude Code's prompt like a text box.** Highlight words in the prompt and press Backspace
  to delete them, or type or paste over them, even across wrapped lines. Mac and Windows.
- **Copy more than the window shows.** Drag to select, even inside Claude Code, and keep dragging
  past the top or bottom edge to keep scrolling.
- Trinidad Head keeps a document of what a full-screen program has shown, so selecting in it is
  ordinary.
- Copy out of Claude Code holds the screen still instead of scrolling under the mouse.
- **Updates itself** from a git clone when `main` moves (see
  [How it works](how-it-works.md#self-updater)).
- Copy the right rows when a program pins a box to the bottom of the screen.
- Fixed a crash where a plain drag could read the pressed cell after the view changed.
- Build: never ad hoc sign the Mac app by accident; when signing fails over SSH, sign in a window
  on the Mac itself.
- Self-tests clean up after themselves (dead Dock icons, hidden test windows on the PC).
- License: MIT OR Apache-2.0 (`7685b59`, 2026-09-19).
- Release packaging scripts for both downloads; version set to 0.2.0 (`dc63468`).

## v0.1.0 (tagged 2026-09-17, published 2026-09-19)

First tagged release, source only.

- Dark glass windows with a neon rim and a different color for every open window ("look 06"
  window chrome, both systems).
- Your own prompts in a Claude Code chat sit in their own bubble.
- Faint text (SGR 2) draws at a third of its brightness, so Claude Code's ghost suggestion reads as
  a hint.
- Drag and drop files onto the window (typed as paths, so Claude Code attaches images) and
  dropped text.
- Right-click menu with Open Link, Copy Link, Copy, Paste and Select All; selecting no longer
  copies by itself.
- Cmd+click (Mac) and Ctrl+click (Windows) open links.
- A drag selects text even when Claude Code has the mouse.
- Mac: dictation with Control pressed twice, and input methods.
- Sidebar trimmed to the glow button.

The history before `v0.1.0` in this repo starts on 2026-09-17 with 8 commits up to the tag.
