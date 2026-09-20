#!/bin/bash
# Automated checks for the Mac app. Each check runs in its own throwaway window that drives
# itself with synthetic events (nothing global), one window at a time. Results: $OUT.
# Only processes started here are ever stopped.
set -u
# TH_BIN points the run at a build that is not installed yet (test before you ship).
APP_BIN="${TH_BIN:-$HOME/Applications/Trinidad Head.app/Contents/MacOS/trinidad-head}"
OUT="${1:-/tmp/trinidad-head-selftest.txt}"
WORK="$(mktemp -d /tmp/th_selftest.XXXXXX)"
: > "$OUT"

CLIP="$WORK/clipboard.txt"
pbpaste > "$CLIP" 2>/dev/null
restore_clipboard() { pbcopy < "$CLIP"; }
trap restore_clipboard EXIT

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
    kill "$pid" 2>/dev/null; sleep 1; kill -9 "$pid" 2>/dev/null
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

run_mode version 40 "claude --version; sleep 20"
# A fixed folder, so Claude's "trust this folder?" question is answered only once, ever.
CLAUDE_DIR="$HOME/Library/Application Support/TrinidadHead/claude-selftest"
mkdir -p "$CLAUDE_DIR"
run_mode claude 150 "cd '$CLAUDE_DIR' && claude"
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

echo "---" >> "$OUT"
echo "$(grep -c '^PASS' "$OUT") passed, $(grep -c '^FAIL' "$OUT") failed" >> "$OUT"
cat "$OUT"
