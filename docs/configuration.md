---
title: Trinidad Head configuration, settings file, environment variables and shortcuts
description: Every Trinidad Head option read from the source code. The settings file, glow colors, command line, environment variables, files it writes, build variables and keyboard shortcuts.
---

# Configuration

Trinidad Head has no settings screen and almost no options. Everything on this page was read from
the source code; file and line references point at where each option is handled.

## Settings file

| System | Path |
|---|---|
| macOS | `~/Library/Application Support/TrinidadHead/settings.txt` |
| Windows | `%LOCALAPPDATA%\TrinidadHead\settings.txt` |

The app **reads** this file at startup and never writes it. Create it yourself. The only key is
`glow`. Lines that do not start with `glow=` are ignored, and if `glow=` appears more than once the
last one wins (`crates/app/src/theme.rs`, `parse_settings`).

```text
# Trinidad Head settings
glow=tide
```

### Default glow color

`glow=` names the color new windows start from. Unknown names fall back to `aurora`.

| Name | Rim colors (hex) |
|---|---|
| `aurora` (default) | `3FD8FF` `5B6CFF` `B44DFF` `FF4FD8` |
| `ember` | `FFB45A` `FF7A1A` `FF4D1A` `FF9A3C` |
| `tide` | `7FFFD0` `14D6A0` `10A8C8` `2FE0FF` |
| `rose` | `FF9AC8` `FF4F9A` `D9368B` `FF7AB0` |
| `ocean` | `6FE6FF` `2F9BFF` `1B4DFF` `00C8FF` |
| `gold` | `FFE27A` `F5B82E` `E0901A` `FFD24D` |
| `violet` | `C9A2FF` `8A4DFF` `6A2FE0` `B07BFF` |
| `lime` | `D4FF7A` `8FE03A` `4CC23A` `B8F55A` |

The first color of each set is also the cursor and highlight color. A new window takes the saved
default if no other open window uses it, otherwise the next unused color in the list order above.
With all eight in use, colors repeat (`pick_glow` in `theme.rs`). The palette button changes only
the current window and does not touch the file.

## Claude Code prompt bar color

Trinidad Head finds your own prompts by their background color. It treats a cell as part of
Claude Code's "your message" bar when its background is one of:

- `rgb(55, 55, 55)` or `rgb(70, 70, 70)`: Claude Code's own dark theme bar and its hover shade
  (`CLAUDE_DEFAULT_BARS` in `theme.rs`);
- the `userMessageBackground` color in `~/.claude/themes/trinidad-head.json` (the code looks in your
  home folder, `HOME` on the Mac and `USERPROFILE` on Windows), or `rgb(196, 214, 230)` if that file
  is missing or has no such key.

The file is read once when the window opens. Only the first `rgb(r, g, b)` value after the
`"userMessageBackground"` key is used. Text on the bar is drawn bold, green (`4E9F6B`), 1.18 times
larger, on a see-through pill.

!!! warning "TODO-VERIFY"
    This repo does not ship `trinidad-head.json`, and the code only reads it. Whether Claude Code
    itself loads a theme from `~/.claude/themes/` (so the bar is painted in that color) has not been
    checked against Claude Code's documentation.

## Command line

| System | Behavior | Source |
|---|---|---|
| macOS | All arguments after the program name are joined with spaces and run as `$SHELL -l -c "<args>"`. Arguments that start with `-psn_` (added by older macOS launchers) are dropped. With no arguments, an interactive login shell starts. | `crates/app/src/mac/mod.rs`, `crates/pty/src/unixpty.rs` |
| Windows | The raw command line minus the program name is used as the command, exactly as typed, including quotes. With nothing after the name, the default shell starts. | `crates/app/src/cmdline.rs`, `crates/app/src/win.rs` |

The default shell on Windows is `"%ProgramFiles%\PowerShell\7\pwsh.exe" -NoLogo` if that file
exists, otherwise `powershell.exe -NoLogo` (`crates/pty/src/lib.rs`). On the Mac it is `$SHELL`, or
`/bin/zsh` if `SHELL` is unset or empty. The Mac shell starts in your home folder.

There are no command line flags of Trinidad Head's own.

## Environment variables

### Read by the app

| Variable | System | Effect |
|---|---|---|
| `TRINIDAD_HEAD_NO_UPDATE` | both | If set (any value), the self-updater does not run. |
| `TRINIDAD_HEAD_REPO` | both | Path of the git clone the self-updater uses. If set but not a clone of this repo, no other place is tried. |
| `TRINIDAD_HEAD_TOKEN` | macOS | The window's control token. Only letters, digits and `-` are kept. If unset or empty after filtering, a token `th-<pid>-<hex time>` is made up. |
| `TRINIDAD_HEAD_INPUT` | macOS | A line typed into the shell (followed by Return) right after it starts. Removed from the environment before the shell starts. Used by `th-open`. |
| `SHELL` | macOS | The shell to start. |
| `HOME` | both | Where `~/.trinidad-head`, the settings folder and the Claude theme file are found. Falls back to `/tmp` for `~/.trinidad-head` on the Mac. |
| `USERPROFILE` | Windows | Home folder for the Claude theme file, the folder button and the updater's search. |
| `LOCALAPPDATA` | Windows | Base of `TrinidadHead\` (settings, logs, window registry). |
| `ProgramFiles` | Windows | Where PowerShell 7 is looked for. Defaults to `C:\Program Files`. |

### Set for programs running inside

| Variable | Value | System |
|---|---|---|
| `TERM` | `xterm-256color` | macOS |
| `COLORTERM` | `truecolor` | both |
| `TERM_PROGRAM` | `TrinidadHead` | both |
| `TRINIDAD_HEAD_TOKEN` | this window's token | macOS |

On the Mac, every variable whose name starts with `CLAUDE` is **removed** before the shell starts,
so a Claude Code session started inside a new window does not think it is a child session
(`unixpty.rs`). Windows does not set `TERM` and does not remove these variables.

### Test hooks

These exist for the self-tests in `scripts/` and `pc-tools/`. They are not meant for daily use.

| Variable | System | Effect |
|---|---|---|
| `TRINIDAD_HEAD_SELFTEST` | both | Marks a test window. On the Mac its value picks a self-test mode (`full`, `claude`, `version`, `stress`, `program`, `program-chat`, `claude-select`, `prompt-edit`, `claude-edit`; any other value writes `FAIL unknown self-test mode`). Turns off the updater and the Dock icon, and stops links from really opening. |
| `TRINIDAD_HEAD_RESULTS` | macOS | File the self-test appends PASS/FAIL lines to. |
| `TRINIDAD_HEAD_CAPTURE` | macOS | File the `full` self-test reads raw typed bytes back from. |
| `TRINIDAD_HEAD_SHOTS` | macOS | Folder for self-test screenshots (default `/tmp`). |
| `TRINIDAD_HEAD_SHOT` | macOS | Path; about 3 seconds after start the window saves a picture of itself there. |
| `TRINIDAD_HEAD_DUMP` | both | Path; the visible screen text plus cursor, scroll and selection state is written there (every 250 ms on Windows, every frame on the Mac). |
| `TRINIDAD_HEAD_RAW` | Windows | Path; every byte read from the console is appended there with a `<<<READ>>>` marker per read. |
| `TRINIDAD_HEAD_FOLDER` | macOS | Folder the folder button opens instead of home. (The folder button is not in the current sidebar.) |
| `TRINIDAD_HEAD_UPDATE_NOW` | both | Run the update check once, print the result and exit, before any window opens. |
| `TRINIDAD_HEAD_UPDATE_DRYRUN` | both | With the check above, stop after the fast-forward and report `would install` instead of building. |
| `PROMPT_CHILD_OUT` | both | Used by the prompt-editing tests' stand-in program. |

On the Mac, while the file `~/.trinidad-head/dump-all` exists, every new window writes its screen to
`~/.trinidad-head/dumps/<pid>.txt` (a launcher test hook in `view.rs`).

## Files and folders it writes

| Path | System | What |
|---|---|---|
| `~/Library/Application Support/TrinidadHead/latency.log` | macOS | One typing-delay sample (ms) per line. |
| `%LOCALAPPDATA%\TrinidadHead\latency.log` | Windows | Same. |
| `.../TrinidadHead/startup.log` | both | `first shell output on screen after N ms`, overwritten each launch. |
| `.../TrinidadHead/update.log` | both | Self-updater notes (skips, installs), each with a Unix timestamp. |
| `%LOCALAPPDATA%\TrinidadHead\windows\<pid>` | Windows | Each open window's glow index. Entries of dead processes are removed. |
| `~/.trinidad-head/windows/<pid>` | macOS | Each open window's `glow=` index and `token=`. |
| `~/.trinidad-head/<token>.sock` | macOS | The window's control socket, mode 600. |
| `~/.trinidad-head/dock.lock` | macOS | Lock that decides which window owns the Dock icon. |

`*.log` files are ignored by git in this repo.

## Fixed values

These are constants in the code, not options:

| What | Value | Where |
|---|---|---|
| Scrollback | 10,000 lines | `max_scrollback` in `core-vt` |
| Font (Windows) | First found of Cascadia Mono, Cascadia Code, Consolas, Courier New, at 15 DIPs | `win.rs` |
| Font (Mac) | Menlo at 14 pt, else the system monospaced font | `mac/view.rs` |
| Starting window | 1080 x 700 | `win.rs`, `mac/mod.rs` |
| Smallest window (Mac) | 420 x 280 | `mac/mod.rs` |
| Wheel (Windows) | 3 lines per notch | `win.rs` |
| Glow margin | 26 px of transparent room around the body | `layout.rs` |

## Keyboard and mouse

| Action | macOS | Windows |
|---|---|---|
| Copy | ++cmd+c++ (selection stays) | ++ctrl+c++ with a selection, or ++ctrl+shift+c++ |
| Cut a highlight out of Claude Code's prompt | ++cmd+x++ | ++ctrl+x++ |
| Paste | ++cmd+v++ | ++ctrl+shift+v++ |
| Select all | ++cmd+a++ | right-click menu |
| Close window | ++cmd+w++ | ++alt+f4++ or the red button |
| Minimize | ++cmd+m++ | the yellow button |
| Quit | ++cmd+q++ | |
| Scroll a page of history | ++shift+page-up++ / ++shift+page-down++ | same |
| Open a link | ++cmd++-click | ++ctrl++-click |
| Context menu | right-click or ++ctrl++-click | right-click |
| Select inside a mouse-using program | drag, or ++shift++-drag | drag, or ++shift++-drag |
| Meta key | ++option++ sends ESC + key | ++alt++ sends ESC + key |
| NUL | | ++ctrl+space++ |
| Zoom | double-click the title area, or the green button | the green button |

Arrow keys, Home, End, Insert, Delete, Page Up/Down and F1 to F12 send the usual xterm sequences,
with modifiers encoded as `1 + Shift + 2*Alt + 4*Ctrl`.

## Build script variables

`scripts/build-mac-app.sh`:

| Variable | Effect |
|---|---|
| `CARGO` | cargo to use (default `~/.cargo/bin/cargo`). |
| `TH_APP_OUT` | Where to put the app (default `~/Applications/Trinidad Head.app`). |
| `TH_UNIVERSAL=1` | Build for Apple Silicon and Intel and join them with `lipo`. |
| `TH_SIGN_IDENTITY` | Signing identity. Default: the first "Apple Development" certificate. |
| `TH_HARDENED=1` | Sign with the hardened runtime, a timestamp and `assets/release.entitlements` (needed for notarization). |
| `TH_ALLOW_ADHOC=1` | Allow an ad hoc signature when no certificate is usable. |

`scripts/package-release.sh` also reads `ASC_KEY_ID` and `ASC_ISSUER` for the App Store Connect
API key used by `notarytool`. The self-test scripts read `TH_BIN` to test a build that is not
installed yet.
