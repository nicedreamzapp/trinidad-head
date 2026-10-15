---
title: How Trinidad Head works, terminal engine, pty and window architecture
description: The architecture of the Trinidad Head terminal, read from its Rust source. The core-vt engine, ConPTY and Unix pty host, Direct2D and AppKit windows, prompt editing, the control socket and the self-updater.
---

# How it works

Trinidad Head is a Cargo workspace of three crates (`Cargo.toml`), all at version 0.2.0 and
licensed MIT OR Apache-2.0.

| Crate | Path | Job | OS code? |
|---|---|---|---|
| `core-vt` | `crates/core-vt` | Turns the bytes a shell prints into a screen of cells, plus scrollback | None |
| `pty` | `crates/pty` | Runs a shell behind a pseudo console and hands back its byte streams | ConPTY on Windows, `openpty` on Unix |
| `trinidad-head` | `crates/app` | The window: drawing, input, selection, clipboard | Win32 + Direct2D on Windows, AppKit + CoreGraphics on macOS |

```text
 keyboard / mouse
        |
        v
 +-------------+   bytes    +-----------+   bytes   +-------+
 |   window    | ---------> |    pty    | --------> | shell |
 | (app crate) |            | ConPTY or |           +-------+
 |             | <--------- | Unix pty  | <---------   output
 +-------------+  redraw    +-----------+
        ^                         |
        |   screen of cells       | reader thread
        +---------- core-vt <-----+
```

## The loop

1. At startup the window spawns the shell through `pty::Pty::spawn` at 80 x 24, then resizes it
   to fit.
2. A **reader thread** reads the shell's output in 64 KiB chunks and feeds it to
   `core_vt::Terminal::feed` under a mutex. Replies the terminal owes the shell (cursor position
   reports, device attributes) are written straight back.
3. The reader then asks the window for one repaint. On Windows it posts a private message
   (`WM_APP + 1`); on the Mac it queues a block on the main dispatch queue. A flag makes sure only
   one repaint is queued at a time.
4. Keys, pastes and mouse reports are turned into bytes in the window code and written to the pty.
5. When the shell exits, the window closes. On the Mac a watcher thread also checks every 200 ms
   whether the shell process itself has ended, because a background helper can hold the terminal
   open after the shell is gone.

## core-vt, the engine

`crates/core-vt/src/lib.rs` has no UI and no OS code, so the README notes it could later back
other views. It depends only on `unicode-width`.

What it handles, from `csi_dispatch`, `private_modes`, `sgr` and `osc_end`:

- Cursor movement, erase, insert and delete of characters and lines, scroll regions, tab stops,
  save and restore cursor.
- Colors: 16 colors, 256 indexed colors and 24-bit RGB. Bold, faint, italic, underline, inverse.
- Private modes: application cursor keys (1), autowrap (7), cursor visibility (25), alternate
  screen (47, 1047, 1049), bracketed paste (2004), mouse tracking (1000, 1002, 1003), SGR mouse
  encoding (1006), focus events (1004).
- OSC 0 and 2 (window title), OSC 52 (a program copying to the clipboard; reads with `?` are
  ignored), OSC 133 shell-integration marks (prompt, command, output, exit code), kept but not yet
  shown anywhere.
- Wide characters (CJK, most emoji) take two cells. Combining marks are not drawn yet.
- DCS, SOS, PM and APC strings are read and thrown away.
- Scrollback is capped at 10,000 lines.

### The document for full-screen programs

A program on the alternate screen gives the terminal no scrollback. Claude Code goes further: it
never takes the alternate screen and repaints from the top of the ordinary screen, so its old
transcript is overwritten rather than scrolled. Either way a plain terminal holds only what is on
screen right now, and selecting anything taller than the window does not work.

`record_repaint` fixes this. After every `feed`, it compares the screen's text with the previous
screen. If rows moved up, the rows that left the top are added to history (with their colors). If
the program scrolls back down over text already in history, those rows are handed back instead of
recorded twice. A repaint with no overlap (an overlay, a cleared screen) records nothing. History
recorded on the alternate screen is dropped when the program leaves it. This is why selecting and
scrolling inside Claude Code works like ordinary text.

## pty, the shell host

**Windows** (`conpty.rs`) uses `CreatePseudoConsole` with flags `0`. A comment in the code records
that ConPTY does not pass through what a program wrote; it sends what it last drew, on its own
clock, so very fast repaints can be dropped. The passthrough flag (`0x8`) was measured on
2026-09-21 and did not help: 84 of 300 lines were still never delivered.

**macOS** (`unixpty.rs`) uses `openpty`, starts `$SHELL -l` (with `-c <command>` when a command was
given) in a new session with the pty as its controlling terminal, sets `TERM`, `COLORTERM` and
`TERM_PROGRAM`, and removes every `CLAUDE*` variable. On quit the window sends `SIGHUP` to the
shell's whole process group.

## The windows

Both windows are borderless and draw every pixel themselves: the glass body, the neon rim and
blurred glow in a 26 px transparent margin (which doubles as the resize border), the red, yellow
and green buttons, the sidebar with the palette button, and the resize grip. `layout.rs` holds the
shared geometry as plain math with tests, so both systems put things in the same place.

- **Windows** (`win.rs`, `gfx.rs`): a Direct3D 11 device, a see-through swap chain handed to
  DirectComposition, and Direct2D with DirectWrite for text. The window is per-monitor DPI aware
  and asks Windows not to round its corners or draw its own border. If the GPU device is lost
  (driver reset, remote session change) it is rebuilt on the next frame.
- **macOS** (`mac/`): an `NSWindow` subclass that can take keyboard focus while borderless, a view
  that implements `NSTextInputClient` so dictation and input methods work, and CoreGraphics for the
  chrome (`paint.rs`).

### Mouse: click or drag

When a program has asked for the mouse (mode 1000 or higher), a press in the text area is held
back. If the pointer leaves the pressed cell, it becomes a text selection in Trinidad Head. If the
button comes up on the same cell, the click is sent to the program as a press and release. Holding
Shift always selects. While a drag is held past the top or bottom edge, a timer keeps scrolling,
faster the further out the pointer is.

### Editing Claude Code's prompt

`prompt_edit.rs` handles a highlight that sits wholly inside Claude Code's input box, which it
recognizes as the rows between two horizontal rules (`─`) around the caret, with text starting two
columns in (after `❯ ` on the first row). The terminal cannot edit Claude's text directly, so it:

1. sends a mouse click just after the highlight and waits (up to 500 ms) for Claude's caret to
   move there;
2. checks that the caret is in the box and past the start of the highlight, otherwise gives up and
   sends the key as if nothing were highlighted;
3. sends one Backspace per painted character, then a few more one at a time if line wrapping hid
   characters, watching the caret each time;
4. sends whatever was typed or pasted meanwhile, which was queued so it cannot overtake the
   deletion.

### Glow colors per window

Each window writes its glow index into a small registry file named after its process id
(`%LOCALAPPDATA%\TrinidadHead\windows\` or `~/.trinidad-head/windows/`). A new window reads the
others, removes entries of processes that are gone, and picks the first color not in use (see
[Configuration](configuration.md#default-glow-color)).

### Typing-delay meter

`latency.rs` times each key from the moment it is sent to the shell to the first frame presented
after the shell printed something. Only the first unanswered key counts, and keys with no echo
are dropped after 2 seconds. It keeps the last 200 samples and reports the median and 95th
percentile. On Windows these show in the title (`typing delay N ms (p95 M ms)`). Every sample is
appended to `latency.log`. The code notes this does not include the monitor's own delay.

## Control socket (Mac)

Each Mac window listens on `~/.trinidad-head/<token>.sock` (mode 600) and hands its token to the
shell as `TRINIDAD_HEAD_TOKEN`. One command per connection, reply `ok` or `error ...`:

| Command | Effect |
|---|---|
| `ping` | Replies `ok`. |
| `type <text>` | Sends the text as if pasted (bracketed when the program asked for it; newlines become CR). |
| `key enter` / `key return` | Sends CR. |
| `key escape` / `key esc` | Sends ESC. |
| `key tab` | Sends Tab. |
| `key ctrl-c` | Sends Ctrl+C. |

The helpers in `mac-tools/` (`th-type`, `th-alive`, `th-self-id`, `th-open`) are thin clients of
this socket. Sockets left by windows that were killed are cleaned up when another window starts.
Windows has no control socket.

## One Dock icon (Mac)

Every Mac window is its own process (`open -n`). The app's `Info.plist` sets `LSUIElement`, so each
starts with no Dock tile. Whichever process holds an exclusive `flock` on
`~/.trinidad-head/dock.lock` becomes a regular app with the one icon; the rest wait on the lock in
the background. The kernel releases the lock when the process ends, however it ends, so the next
window takes the icon. Clicking the icon brings every Trinidad Head window forward.

## Self-updater

`update.rs` runs once at startup on a background thread, unless `TRINIDAD_HEAD_SELFTEST` or
`TRINIDAD_HEAD_NO_UPDATE` is set. `build.rs` bakes the commit the binary was built from into it.

1. Find a clone: `TRINIDAD_HEAD_REPO`, else `~/Desktop/PROJECTS/trinidad-head`,
   `~/dev/trinidad-head`, `~/trinidad-head`. A folder counts only if it has `.git` and
   `crates/app/Cargo.toml`.
2. Skip if `git status --porcelain` shows local changes.
3. `git fetch --quiet origin main`. Stop if `origin/main` is the commit this build came from.
4. Skip unless the clone is on branch `main`. Then `git merge --ff-only origin/main`.
5. Build and install: `scripts/build-mac-app.sh` on the Mac; `cargo build --release` then
   `pc-tools\install_th.ps1` on Windows (with no console windows).

The window already open keeps running the old build; the next window opened is the new one. A
failed build leaves the installed app as it was. Results go to `update.log`.
`scripts/update-check.sh` tests these rules against throwaway clones.

## Tests

Unit tests sit next to the code (`#[cfg(test)]` modules); the source has 62 `#[test]` functions
across the three crates. The larger checks drive the real app: `scripts/mac-selftest.sh` runs
the Mac self-test modes one throwaway window at a time, and `pc-tools/th_selftest.ps1` and
`pc-tools/th_prompt_test.ps1` do the same on Windows.
