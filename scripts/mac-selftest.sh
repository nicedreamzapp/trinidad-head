#!/bin/bash
# Automated checks for the Mac app. Each check runs in its own throwaway window that drives
# itself with synthetic events (nothing global), one window at a time. Results: $OUT.
# Only processes started here are ever stopped.
set -u
# TH_BIN points the run at a build that is not installed yet (test before you ship).
APP_BIN="${TH_BIN:-$HOME/Applications/Trinidad Head.app/Contents/MacOS/trinidad-head}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-/tmp/trinidad-head-selftest.txt}"
WORK="$(mktemp -d /tmp/th_selftest.XXXXXX)"
: > "$OUT"

CLIP="$WORK/clipboard.txt"
pbpaste > "$CLIP" 2>/dev/null

# How many Trinidad Head tiles the Dock shows right now. A tile with no process behind it is a
# dead one, and the Dock keeps it until it is restarted.
dock_tiles() {
  osascript -e 'tell application "System Events" to tell process "Dock" to get name of UI elements of list 1' 2>/dev/null \
    | tr ',' '\n' | grep -c 'Trinidad Head'
}
# pgrep -f does not match this binary on macOS 27 (it finds nothing while ps lists it), so the
# count comes from ps. Match the executable path, not the command line: any shell sitting in the
# trinidad-head folder has the name in its arguments and would be counted as a window.
th_running() { ps -axo comm | grep -c 'trinidad-head$'; }

# Every test window is the bare binary, and a window that has to be killed outright leaves its
# tile stuck in the Dock with nothing behind it: a few runs and the Dock is a row of dead
# Trinidad Head icons. Restarting the Dock rebuilds it from what is really running and closes
# nothing. This lives in the exit trap on purpose, because a run that fails early or is
# interrupted is exactly the run that killed a window, so it is the run that must clean up.
clear_dead_tiles() {
  local tiles running
  tiles="$(dock_tiles)"
  running="$(th_running)"
  [ "$tiles" -le "$running" ] && return 0
  pgrep -x Dock >/dev/null 2>&1 || return 0
  killall Dock 2>/dev/null || return 0
  echo "NOTE restarted the Dock: it had $tiles Trinidad Head tiles for $running open windows" >> "$OUT"
}

on_exit() { pbcopy < "$CLIP"; clear_dead_tiles; }
trap on_exit EXIT

# Records everything typed into the window, raw, for byte-level checks.
cat > "$WORK/capture.py" <<'PY'
import os, sys, termios, tty, time
fd = sys.stdin.fileno()
tty.setraw(fd)
out = open(os.environ["TRINIDAD_HEAD_CAPTURE"], "ab", buffering=0)
while True:
    b = os.read(fd, 4096)
    if not b:
        break
    out.write(b)
PY

# run_mode <mode> <timeout secs> <command...>
run_mode() {
  local mode="$1" limit="$2"; shift 2
  # ONLY=<mode> runs just one group.
  if [ -n "${ONLY:-}" ] && [ "$ONLY" != "$mode" ]; then return; fi
  local res="$WORK/$mode.txt"
  : > "$res"
  # Don't hand this session's Claude Code markers to the test windows.
  env -u CLAUDE_CODE_CHILD_SESSION -u CLAUDECODE -u CLAUDE_CODE_ENTRYPOINT -u CLAUDE_CODE_SSE_PORT \
  TRINIDAD_HEAD_SELFTEST="$mode" TRINIDAD_HEAD_RESULTS="$res" \
  TRINIDAD_HEAD_CAPTURE="$WORK/$mode.bin" TRINIDAD_HEAD_FOLDER="$WORK/finder-test" \
    "$APP_BIN" "$@" >"$WORK/$mode.log" 2>&1 &
  local pid=$!
  local waited=0
  while kill -0 "$pid" 2>/dev/null && ! grep -q '^DONE' "$res" && [ "$waited" -lt "$limit" ]; do
    sleep 1; waited=$((waited + 1))
  done
  sleep 1
  if kill -0 "$pid" 2>/dev/null; then
    grep -q '^DONE' "$res" || echo "FAIL $mode: test window did not finish in ${limit}s" >> "$res"
    # SIGTERM closes the window the normal way, which takes the Dock tile with it. Give it real
    # time to do that: SIGKILL skips the close and strands the tile, so it is the last resort.
    kill "$pid" 2>/dev/null
    local gone=0
    for _ in 1 2 3 4 5 6 7 8; do
      kill -0 "$pid" 2>/dev/null || { gone=1; break; }
      sleep 1
    done
    if [ "$gone" = 0 ]; then
      kill -9 "$pid" 2>/dev/null
      echo "NOTE $mode: window ignored SIGTERM and had to be killed outright" >> "$res"
    fi
  elif ! grep -q '^DONE' "$res"; then
    echo "FAIL $mode: test window quit early (see $WORK/$mode.log)" >> "$res"
  fi
  grep -v -e '^DONE' "$res" >> "$OUT"
}

mkdir -p "$WORK/finder-test"
: > "$WORK/full.bin"
run_mode full 120 "python3 '$WORK/capture.py'"
# The folder test opened a throwaway Finder window; close just that one.
if [ -z "${ONLY:-}" ] || [ "$ONLY" = full ]; then
  names="$(osascript -e 'tell application "Finder" to get name of every window' 2>&1)"
  case "$names" in
    *finder-test*)
      osascript -e 'tell application "Finder" to repeat while (exists window "finder-test")' -e 'close window "finder-test"' -e 'end repeat' >/dev/null 2>&1 \
        && echo "NOTE closed the test Finder window" >> "$OUT" \
        || echo "NOTE could not close the test Finder window: $names" >> "$OUT" ;;
    *) echo "NOTE no test Finder window left open ($names)" >> "$OUT" ;;
  esac
fi

# Selecting past the edge inside a program that owns the screen, like Claude Code does.
run_mode program 90 "python3 '$ROOT/scripts/fullscreen-child.py'"
# The same thing in the shape Claude Code really has: a prompt box pinned to the bottom that
# never scrolls, and three lines of travel per notch.
run_mode program-chat 90 "CHILD_SHAPE=chat python3 '$ROOT/scripts/fullscreen-child.py'"

# Deleting a highlight inside a program's own input box (Claude's prompt), against a stand-in
# that writes its exact text to a file.
PROMPT_CHILD_OUT="$WORK/prompt-child.txt" run_mode prompt-edit 90 "python3 '$ROOT/scripts/prompt-child.py'"

run_mode version 40 "claude --version; sleep 20"
# A fixed folder, so Claude's "trust this folder?" question is answered only once, ever.
CLAUDE_DIR="$HOME/Library/Application Support/TrinidadHead/claude-selftest"
mkdir -p "$CLAUDE_DIR"
run_mode claude 150 "cd '$CLAUDE_DIR' && claude"
# The same selection job against the real thing. /help is local, so this costs no tokens.
run_mode claude-select 150 "cd '$CLAUDE_DIR' && claude"
# The same deletes in the real prompt. Nothing is submitted, so no tokens.
run_mode claude-edit 150 "cd '$CLAUDE_DIR' && claude"
if [ -z "${ONLY:-}" ] || [ "$ONLY" = claude ]; then
  sleep 2
  left=""
  for p in $(pgrep -f claude); do
    cwd=$(lsof -a -p "$p" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p')
    case "$cwd" in *claude-selftest*) left="$left $p";; esac
  done
  if [ -z "$left" ]; then echo "PASS closing the window leaves no Claude processes behind" >> "$OUT"
  else echo "FAIL closing the window left Claude processes:$left" >> "$OUT"; kill $left 2>/dev/null; fi
fi
run_mode stress 150 "yes | head -2000000; CLICOLOR_FORCE=1 ls -laG /usr/bin /System/Library/Frameworks; echo STRESS-DONE; sleep 60"

# The run is over, so every tile beyond the windows still open is a dead one this run left in
# Matt's Dock. The exit trap clears them either way; this is the check that says so out loud,
# because a run that strands tiles is a run whose windows did not close the way they should.
tiles_left="$(dock_tiles)"; windows_left="$(th_running)"
if [ "$tiles_left" -le "$windows_left" ]; then
  echo "PASS every test window took its Dock icon with it" >> "$OUT"
else
  echo "FAIL $((tiles_left - windows_left)) dead Trinidad Head icons left in the Dock" >> "$OUT"
fi

echo "---" >> "$OUT"
echo "$(grep -c '^PASS' "$OUT") passed, $(grep -c '^FAIL' "$OUT") failed" >> "$OUT"
cat "$OUT"
