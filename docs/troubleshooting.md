---
title: Trinidad Head troubleshooting, errors and fixes
description: Fixes for common Trinidad Head terminal problems, built from the real error messages in its source code. Shell will not start, SmartScreen, Mac signing errors, copy and paste, updater messages.
---

# Troubleshooting

The project has no GitHub issues filed yet, so this page is built from the error messages and
comments in the source code. If your problem is not here, please
[open an issue](https://github.com/nicedreamzapp/trinidad-head/issues).

## The app does not start

### `trinidad-head: there is no window for this OS yet`

You ran it on Linux (or another system that is not Windows or macOS). There is no Linux window.

### Windows: "Could not start the shell"

A message box titled **Trinidad Head** shows `Could not start the shell:` followed by the command
and the system error. The command is whatever followed the program name on the command line, or
the default shell. Check that:

- the program exists and is spelled right; the command is started directly, not through a shell,
  so a `.bat` or `.cmd` file should be run as `cmd /c "path\to\file.bat"`;
- paths with spaces are quoted.

### Mac: `trinidad-head: could not start the shell`

Printed to standard error when the pty or `$SHELL -l` cannot be started. Check that `SHELL` points
at a real shell. If it is empty, `/bin/zsh` is used.

### Windows: "Windows protected your PC"

The Windows download is not code-signed. Click **More info**, then **Run anyway**.

## Copy and paste

**Selecting did not copy anything.** That is on purpose: selecting never copies. Use ++cmd+c++ on
the Mac, ++ctrl+c++ (with a selection) or ++ctrl+shift+c++ on Windows, or the right-click menu.

**Ctrl+V does not paste on Windows.** Trinidad Head pastes on ++ctrl+shift+v++ and from the
right-click menu. A plain ++ctrl+v++ is passed to the program in the window.
TODO-VERIFY: whether PowerShell's own Ctrl+V handling pastes anyway has not been checked.

**Ctrl+C interrupted my program instead of copying.** On Windows ++ctrl+c++ copies only when
something is selected. With nothing selected it goes to the program, as in any terminal. Use
++ctrl+shift+c++ to always copy.

**A click in Claude Code selects nothing.** When a program has taken the mouse, a click goes to the
program and only a drag selects. Drag, or hold ++shift++ while you drag.

## Editing Claude Code's prompt

**Backspace deleted one character, not my highlight.** Prompt editing only applies when:

- the highlight is wholly inside Claude Code's input box (between its two horizontal rules);
- the window is not scrolled back;
- the program has mouse reporting on, which is how Claude Code's caret is moved.

If Claude Code does not move its caret after the click within half a second, nothing is deleted
and the key is sent as if there were no highlight. This is deliberate, so the wrong text is never
deleted.

## Output and display

**Lines go missing from a program that redraws very fast (Windows).** A comment in
`crates/pty/src/conpty.rs` records that Windows' pseudo console sends what it last drew on its own
schedule, so a program repainting faster than that has repaints dropped. In a test on 2026-09-21,
84 of 300 lines never arrived. The code notes there is no fix on Trinidad Head's side.

**Accented letters made of combining marks look wrong.** Combining marks are not drawn yet
(`core-vt`, `print`).

**My prompts are not in a bubble.** The bubble follows Claude Code's prompt bar color. See
[Configuration](configuration.md#claude-code-prompt-bar-color) for the colors it looks for.

## Mac Dock

**Several Trinidad Head icons in the Dock.** Current builds keep one icon for all windows. Tiles
left by windows that were force-quit stay until the Dock restarts; the self-test script clears
them with `killall Dock`, which closes no windows. If the lock file
`~/.trinidad-head/dock.lock` cannot be opened, each window falls back to its own icon.

## Building on a Mac

### `no Apple Development certificate on this Mac.`

`scripts/build-mac-app.sh` will not ad hoc sign unless asked, because an ad hoc signature changes
every build and macOS then forgets the app's permissions. Either install an Apple Development
certificate, set `TH_SIGN_IDENTITY`, or run with `TH_ALLOW_ADHOC=1`.

### `codesign with ... failed, and no window on the Mac could do it either.`

Usually seen when building over SSH: the login keychain is locked in that session, so `codesign`
fails with `errSecInternalComponent`. The script then tries to run the signing step inside an open
Trinidad Head window on the Mac (via `th-open`), where the keychain is unlocked. If that also
fails, run the build from a window on the Mac itself, or use `TH_ALLOW_ADHOC=1`.

### `no Developer ID Application certificate on this Mac`

From `scripts/package-release.sh`, which is only for making the signed, notarized release zip. To
build for yourself, use `scripts/build-mac-app.sh` instead.

### The app forgets Accessibility or Full Disk Access after a rebuild

It was signed ad hoc. Sign with a real certificate (see above) so the signature stays the same.

## Building on Windows

### `x86_64-w64-mingw32-windres not found; building without the app icon`

A warning, not an error. Add llvm-mingw's `bin` folder to `PATH` to get the icon.

### `no build at ... - run cargo build --release first`

From `pc-tools\install_th.ps1`. Build first. The source path in that script is the author's own
clone location; change `$src` if yours is elsewhere.

### `copy did not take`

Also from `install_th.ps1`: the installed exe's size does not match the build. A running copy is
renamed out of the way first; try again with all Trinidad Head windows closed.

## Self-updater messages

These appear in `update.log` (`~/Library/Application Support/TrinidadHead/` or
`%LOCALAPPDATA%\TrinidadHead\`).

| Message | Meaning |
|---|---|
| `skipped: no repo on this machine` | No clone found. Normal for downloaded copies. Set `TRINIDAD_HEAD_REPO` if your clone is elsewhere. |
| `skipped: the repo has local changes` | It never builds over uncommitted work. |
| `skipped: cannot reach origin` | `git fetch` failed (offline, or no `origin` remote). |
| `skipped: no origin/main` | The remote has no `main` branch. |
| `skipped: on branch X, not main` | It only updates a clone that is on `main`. |
| `skipped: cannot fast-forward to origin/main` | Local `main` has commits that are not on `origin/main`. |
| `skipped: build failed: ...` | The build failed; the last three lines of its error output follow. The installed app is unchanged. |
| `installed <sha>; the next window you open is the new one` | Success. |

To turn the updater off, set `TRINIDAD_HEAD_NO_UPDATE`.

## Mac helper commands

| Output | Meaning |
|---|---|
| `th-type` prints `missing`, exits 2 | No window with that token (its socket is gone). |
| `th-type` prints `error unknown key ...` or `error unknown command ...` | The control socket did not understand the request. |
| `th-alive` prints `no` | The window is closed or not answering. |
| `th-self-id` exits 1 | Not running inside a Trinidad Head window, or its socket is missing. |
| `trinidad-head: control socket ...` on stderr | The window could not create its socket in `~/.trinidad-head/`; helpers cannot reach it. |
