---
title: Trinidad Head quickstart, windows, colors, copy and paste
description: Get started with the Trinidad Head terminal. Open windows, run a command in a new window, change the glow color, select, copy, paste and edit Claude Code's prompt.
---

# Quickstart

This page assumes Trinidad Head is [installed](install.md).

## 1. Open a window

=== "Mac"

    Open **Trinidad Head** from Applications. Each window is its own process, so to open another
    one from a terminal use `-n`:

    ```bash
    open -n -a "Trinidad Head"
    ```

    The window starts your login shell (`$SHELL -l`, or `/bin/zsh` if `SHELL` is empty) in your
    home folder.

=== "Windows"

    Double-click **Trinidad Head.exe**. Run it again for another window.

    The window starts PowerShell 7 (`%ProgramFiles%\PowerShell\7\pwsh.exe -NoLogo`) if it is
    installed, otherwise Windows PowerShell (`powershell.exe -NoLogo`).

Every new window takes a glow color that no other open window is using.

## 2. Run a command in its own window

Anything after the program name is the command to run instead of a plain shell.

=== "Mac"

    ```bash
    open -n -a "Trinidad Head" --args claude
    ```

    The command runs through your login shell as `$SHELL -l -c "<command>"`, so your `PATH` and
    aliases are there.

=== "Windows"

    ```powershell
    trinidad-head.exe cmd /c "C:\some folder\run.bat"
    ```

    The rest of the command line is passed through exactly as typed, quotes included, so shortcuts
    whose target has a quoted path with spaces work.

When the shell or command ends, the window closes.

## 3. Change the color

Click the **palette button** on the left side of the window. Each click moves this window to the
next of the eight colors. It changes only this window. To change which color new windows start
from, see [Configuration](configuration.md#default-glow-color).

## 4. Select, copy and paste

| Action | Mac | Windows |
|---|---|---|
| Select | Drag | Drag |
| Copy | ++cmd+c++ | ++ctrl+c++ while something is selected, or ++ctrl+shift+c++ |
| Paste | ++cmd+v++ | ++ctrl+shift+v++ |
| Select all | ++cmd+a++ | Right-click, **Select All** |
| Menu | Right-click or ++ctrl++-click | Right-click |

Selecting never copies on its own. On Windows, ++ctrl+c++ with nothing selected goes to the
shell as usual (it interrupts the running program).

Inside a program that takes the mouse, such as Claude Code's full-screen view, a click still goes
to the program and a drag selects text. Hold ++shift++ to always select. Drag past the top or
bottom edge and the view keeps scrolling so you can select more than fits in the window.

## 5. Edit Claude Code's prompt like a text box

Inside Claude Code's input box (between its two horizontal rules), highlight some words, then:

- press ++backspace++ or ++delete++ to delete them,
- type to replace them,
- paste to replace them,
- or cut them with ++cmd+x++ (Mac) or ++ctrl+x++ (Windows).

This works across wrapped lines. Trinidad Head does it the way a person would: it clicks just after
the highlight so Claude Code moves its caret there, then presses Backspace once per character,
watching the caret. A highlight anywhere else is left alone. Details are in
[How it works](how-it-works.md#editing-claude-codes-prompt).

## 6. Links and files

- ++cmd++-click (Mac) or ++ctrl++-click (Windows) on a web link opens it. The right-click menu over
  a link also has **Open Link** and **Copy Link**.
- Drop files on the window and their paths are typed in. On the Mac paths are backslash-escaped;
  on Windows a path with spaces is quoted. Claude Code turns dropped image paths into attachments.

## 7. Scroll back

- Mouse wheel or trackpad scrolls the history. When a full-screen program has the mouse, the wheel
  goes to the program unless you are already scrolled back or hold ++shift++.
- ++shift+page-up++ and ++shift+page-down++ scroll a page of history on both systems.
- Up to 10,000 lines are kept.

More shortcuts are listed in [Configuration](configuration.md#keyboard-and-mouse).
