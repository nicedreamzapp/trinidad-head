# Tombolo — research (2026-09-16)

Early goal: a Windows-first terminal on Ghostty's engine (libghostty, MIT) that fixes what people complain about in Ghostty. We later chose to write everything from scratch (see PLAN.md), but these findings still drive the design.

## What people complain about in Ghostty (sourced)
1. Memory bloat in long sessions, which Claude Code triggers. A leak was fixed in 1.3.0, but the base footprint is still about 10x a light terminal (812MiB vs 84MiB). Sources: issues #254, #10289, 1.3.0 notes.
2. No official Windows build and no timeline. Maintainers say every Windows UI framework option is bad. Sources: discussions #2563, #12290.
3. Settings live only in a text file. A GUI is "planned" with no date. Source: ghostty.org/docs/config.
4. Session restore reopens windows but not shell state, and Linux has none. Sources: discussions #12055, #10584.
5. SSH fails with "missing or unsuitable terminal: xterm-ghostty". There are workarounds, and friction was still reported in July 2026. Sources: discussion #4064, vninja.net 2026-07-19.
6. No remote-control/scripting API (Kitty and iTerm2 have one). AppleScript arrived only as a preview in 1.3. Source: discussion #2353.
7. Accessibility is weak. The GPU renderer bypasses the UI accessibility tree, so VoiceOver gets whole-text blobs. Sources: issues #3520, #9932.
8. IME/CJK input bugs: preedit text disappears, and Ctrl+C is lost during composition. Sources: #4634, #10775, #10310.
9. Crashes: many were fixed in 1.3 via fuzzing, and a long tail remains (fonts, multi-monitor, images).
10. Ligature/font rendering inconsistencies. Sources: #2000, #4470, #4469.
11. Mitchell says input latency has "never once" been reliably measured or optimized, and many-unique-styles workloads are slow. Source: discussion #4837.
12. The tmux + scrollbar/search interplay is buggy. Source: #10227.

## Windows-specific bottlenecks (sourced)
- libghostty-vt covers VT parsing, state, scrollback, reflow, input encoding and selection. It is MIT licensed and builds for Windows. It does NOT include rendering or font shaping, and its C API is "still in flux". Sources: mitchellh.com/writing/libghostty-is-coming, PR #8840.
- The community ports (WolftacDigital, Thr45hx and others) use Win32 + OpenGL + FreeType/HarfBuzz + ConPTY. Thr45hx's BUILD-LOG lists these problems:
  - a Zig absolute-path assertion
  - CreateProcessW PATH resolution
  - DirectWrite unable to resolve bold/italic faces
  - silent paste rejection
  - no default shell detection
  None of the ports is blessed, and the ecosystem is fragmented.
- ConPTY has:
  - spawn latency under contention
  - conhost.exe leaks, reported in VS Code
  - passthrough mode with rough edges
  - no DCS forwarding (microsoft/terminal #17313)
- Fonts: Alacritty's Windows text is widely called worse than Windows Terminal's DirectWrite rendering. Use DirectWrite.
- SmartScreen: unsigned or newly signed exes get warnings, and EV certs lost instant reputation in Aug 2024. Sign the exe, the DLLs, the installer and the updater.
- Latency on Windows (chadaustin.me 2024, high-speed camera): conhost and MinTTY measured 33–58 ms, while GPU terminals (Windows Terminal, WezTerm) measured 66–87 ms. GPU rendering does not automatically mean low latency.
- Other terminals' complaints:
  - WezTerm: slow startup, keystroke lag, CRLF paste corruption.
  - Alacritty: fonts, lag.
  - Tabby: Electron weight.

## Draft plan
1. Confirm the PC can build: Zig + Windows SDK headers, and libghostty-vt compiles and passes its tests.
2. Minimal window: ConPTY + Direct3D/DirectWrite renderer (not OpenGL/FreeType) + keyboard/IME input. Measure input latency from day one; the target is to beat conhost's 33–58 ms.
3. Differentiators: settings screen, automatic SSH TERM fallback, local control API (window ids + type-into-window, built in), real session restore, capped/compressed scrollback for long AI sessions, and a Windows UI Automation accessibility tree from the start.
4. Tabs/splits, search, themes.
5. Signing (paid code-signing cert), installer, auto-update, public release.
