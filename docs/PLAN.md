> **2026-09-20, Matt went through the roadmap and dropped most of it.** Do not build or advertise
> these: sessions that outlive the window (he wants programs to END when a window closes), the
> Linux/WSL bridge, the agent layer (command records, agent tabs, approvals, audit log: Claude Code
> already does this), screen-reader support (Speak Anywhere and the Mac's own speech cover it) and
> plugins. The one next step he kept is a download people can install. The rest of this file is
> the original 9/16 plan, kept for history.

# Trinidad Head — plan (2026-09-16)

Written from scratch in Rust, with no Ghostty engine inside, and built as a **platform** other
things grow from, not a single app. This replaces the "Draft plan" in RESEARCH.md, but the pain
points and Windows traps in that file still apply.

## The idea in one line
Terminal sessions live in a background service that people, AI agents, phones and web pages can all
see and drive. The window is just one of the ways to look at them.

## What makes it ours
1. **Linux and Windows as one system.** A WSL Linux shell and a Windows shell sit side by side and
   share a clipboard, a command history and file paths. Paths convert automatically between
   `C:\Users\you` and `/mnt/c/Users/you`. You can run a command for either OS from either shell.
2. **Sessions outlive the window.** A daemon owns every shell. A crashed window, a closed laptop lid
   or a reboot (restored from a saved snapshot) doesn't lose your work. A second window, the phone,
   or FIA can attach to the same live session.
3. **Output is recorded as structured blocks, not only pixels.** Every command becomes a record:
   the command, its output, its exit code, its time, the folder it ran in and the machine it ran on.
   AI can search, summarize and act on those blocks without scraping the screen.
4. **AI is a first-class user, with rules.** Agents open their own tabs through the control API.
   Like browser-broker, they see only their own tabs unless they're granted more. Risky commands can
   require approval from the window, the phone or iMessage. Every agent keystroke goes into an audit
   log. The AI side isn't tied to one provider: it works with Claude and with the local models on
   the M5 and the mini.
5. **Accessible from day one.** It exposes a real Windows UI Automation tree (and the Mac
   accessibility tree later), so screen readers can read it line by line. This ties into the
   AppleVis / accessibility app work.
6. **Fast, and we measure it.** A built-in latency meter runs from the first build. The target is to
   beat the old Windows console's 33–58 ms, not just look fast.

## Architecture: layers other things can build on
Each layer is its own Rust crate with a stable interface, so anything can reuse one layer without the others.

| Layer | What it does | What else could use it |
|---|---|---|
| `core-vt` | Parses terminal output and keeps screen state, scrollback and reflow. Has no UI. Scrollback is capped and compressed. | web viewer, phone app, AI screen reader, test tools |
| `pty` | Runs processes through Windows ConPTY, WSL, Unix ptys and SSH (falling back automatically to a TERM value the server knows) | remote runners, remote command tools |
| `sessiond` | Background daemon that owns every session, block and history entry. Snapshots to disk. | everything below |
| `control-api` | Local socket + HTTP/WebSocket with a JSON protocol: open, type, read, subscribe, lease | agents, dictation, phone mode, iMessage, HQ dashboard, FIA |
| `bridge` | Linux↔Windows path translation, shared clipboard, unified history | file tools, scripts |
| `render` | GPU renderer on wgpu (DX12 / Metal / Vulkan), with DirectWrite text on Windows and CoreText on Mac | any app that needs fast text |
| `app` | The window: tabs, splits, search, themes, and a real settings screen that also writes a text file | — |
| `plugins` | Sandboxed WebAssembly plugins: commands, panels, block actions, AI tools | other people's add-ons |
| `ai` | Provider-neutral agent runtime: block search, "explain this error", command suggestions, approvals, audit | Claude Code tabs, local models |

## Things that can be built on it later
- Remote-control dashboards running on this instead of raw commands (live, streaming, multiple tabs)
- A live agent view on the HQ dashboard showing what each agent is typing right now
- A phone/iMessage "approve this command" flow
- A screen-reader-first terminal for blind developers (the public accessibility angle)
- Demo and recording mode for Studio Record / YouTube (replay a session from its blocks)
- Mac and Linux versions from the same code

## Build order (honest estimates)
1. **PC toolchain.** Rust plus Microsoft's C++ build tools. This needs one admin "Yes" click on the PC. (~30 min)
2. **Bare window.** ConPTY, `core-vt` and a DirectWrite/DX12 renderer, with keyboard input and the latency meter. It types, runs PowerShell and scrolls. (several days)
3. **Daemon + control API.** Sessions survive the window closing, and a second viewer can attach. (~1 week)
4. **WSL + bridge.** Linux and Windows side by side with shared paths, clipboard and history. (~1 week)
5. **Blocks + AI layer + approvals + audit log.** (~1–2 weeks)
6. **Daily-driver polish.** Tabs, splits, search, themes, settings screen, IME, accessibility tree. (weeks)
7. **Plugins, Mac port, signing** (paid code-signing certificate), installer, auto-update, public release.

A daily-driver version is a matter of weeks, not days. Each step ends with something that runs and
can be demoed on the PC through FIA.

## Decisions already made
- Written in Rust, from scratch, with no Ghostty code.
- Windows first, with the Mac from the same codebase later.
- Developed and tested on a Windows 11 laptop (i7-1185G7, Iris Xe GPU) driven remotely from a Mac.
