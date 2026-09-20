#!/bin/bash
# Checks the self-updater against throwaway clones: it must notice a newer main, refuse a repo
# with local changes, refuse a branch that is not main, and do nothing when already current.
# Nothing here touches the installed app — TRINIDAD_HEAD_UPDATE_DRYRUN stops before the build.
set -u
BIN="${TH_BIN:-$HOME/Applications/Trinidad Head.app/Contents/MacOS/trinidad-head}"
WORK="$(mktemp -d /tmp/th_update.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
pass=0; fail=0
check() { # check <name> <expected substring> <actual>
  if [[ "$3" == *"$2"* ]]; then echo "PASS $1"; pass=$((pass+1))
  else echo "FAIL $1: wanted ~'$2', got '$3'"; fail=$((fail+1)); fi
}
ask() { env -u TRINIDAD_HEAD_SELFTEST TRINIDAD_HEAD_UPDATE_NOW=1 TRINIDAD_HEAD_UPDATE_DRYRUN=1 \
         TRINIDAD_HEAD_REPO="$1" "$BIN" 2>/dev/null | tail -1; }

git init -q --bare "$WORK/origin.git"
git clone -q "$WORK/origin.git" "$WORK/seed" 2>/dev/null
cp -R crates "$WORK/seed/" 2>/dev/null
echo "seed" > "$WORK/seed/README.md"
git -C "$WORK/seed" add -A >/dev/null
git -C "$WORK/seed" -c user.name=t -c user.email=t@t commit -qm "seed"
git -C "$WORK/seed" branch -M main
git -C "$WORK/seed" push -q origin main

git clone -q "$WORK/origin.git" "$WORK/clone"

# The real repo, which this binary WAS built from, must read as already current — but only
# once it is committed and pushed, so skip it while there is work in progress.
if [ -n "$(git -C "$PWD" status --porcelain)" ]; then
  echo "NOTE working repo has local changes; skipping the already-current check"
elif [ "$(git -C "$PWD" rev-parse HEAD)" != "$(git -C "$PWD" rev-parse origin/main 2>/dev/null)" ]; then
  echo "NOTE working repo is not at origin/main; skipping the already-current check"
else
  check "the repo this build came from is left alone" "up to date" "$(ask "$PWD")"
fi

# Move origin forward; the clone is now behind.
echo "newer" >> "$WORK/seed/README.md"
git -C "$WORK/seed" add -A >/dev/null
git -C "$WORK/seed" -c user.name=t -c user.email=t@t commit -qm "newer"
git -C "$WORK/seed" push -q origin main
check "a newer main is picked up and fast-forwarded" "would install" "$(ask "$WORK/clone")"
here="$(git -C "$WORK/clone" rev-parse HEAD)"
there="$(git -C "$WORK/origin.git" rev-parse main)"
check "the clone really moved to origin/main" "$there" "$here"

# Local changes: never build over half-finished work.
echo "mine" >> "$WORK/clone/README.md"
check "a repo with local changes is refused" "local changes" "$(ask "$WORK/clone")"
git -C "$WORK/clone" checkout -q -- README.md

# A side branch is not ours to fast-forward.
echo "side" >> "$WORK/seed/README.md"
git -C "$WORK/seed" add -A >/dev/null
git -C "$WORK/seed" -c user.name=t -c user.email=t@t commit -qm "side"
git -C "$WORK/seed" push -q origin main
git -C "$WORK/clone" checkout -q -b experiment
check "a side branch is refused" "not main" "$(ask "$WORK/clone")"

check "a path that is not a repo is refused" "no repo" "$(ask "$WORK/nowhere")"

echo "---"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
