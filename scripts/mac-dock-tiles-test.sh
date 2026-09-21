#!/bin/bash
# Checks the leftover-Dock-icon cleanup in mac-selftest.sh without opening a single window:
# it fires when a run dies early, it stays quiet when nothing is stranded, and its counters
# match the machine. Removing the exit trap from mac-selftest.sh makes the first check fail.
set -u
SRC="$(cd "$(dirname "$0")" && pwd)/mac-selftest.sh"
HEAD="$(sed -n '1,/^trap on_exit EXIT$/p' "$SRC")"   # everything through the exit trap
pass=0; fail=0
check() { if [ "$2" = "$3" ]; then echo "PASS $1"; pass=$((pass+1)); else echo "FAIL $1 (got '$2', wanted '$3')"; fail=$((fail+1)); fi; }

# Case 1: tiles stranded, run dies early (exit 1 from inside) -> trap still restarts the Dock.
OUT1=$(mktemp); RAN1=$(mktemp)
bash -c "$HEAD
dock_tiles() { echo 9; }
th_running() { echo 1; }
killall() { echo restarted >> '$RAN1'; }
exit 1" th-test "$OUT1" >/dev/null 2>&1
check "an interrupted run still clears the dead icons" "$(cat "$RAN1")" "restarted"
check "and says so in the results" "$(grep -c 'NOTE restarted the Dock' "$OUT1")" "1"

# Case 2: nothing stranded -> the Dock is left alone.
OUT2=$(mktemp); RAN2=$(mktemp)
bash -c "$HEAD
dock_tiles() { echo 2; }
th_running() { echo 2; }
killall() { echo restarted >> '$RAN2'; }
exit 0" th-test "$OUT2" >/dev/null 2>&1
check "a clean run does not touch the Dock" "$(cat "$RAN2")" ""

# Case 3: the real counters agree with the machine right now.
eval "$HEAD" 2>/dev/null
trap - EXIT
real_tiles="$(dock_tiles)"; real_run="$(th_running)"
check "the counters see the real window" "$real_run" "$(ps -axo comm | grep -c 'trinidad-head$')"
check "no dead icons left in the Dock" "$([ "$real_tiles" -le "$real_run" ] && echo ok)" "ok"

# Case 4: a shell sitting in the repo folder is not mistaken for a window.
cd "$(dirname "$SRC")/.." && check "a shell in the trinidad-head folder is not counted" "$(th_running)" "$real_run"
echo "--- $pass passed, $fail failed"
