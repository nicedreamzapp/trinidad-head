#!/bin/bash
# Build the Mac download: one app for Apple Silicon and Intel, zipped for a GitHub release.
#   scripts/package-release.sh            -> dist/Trinidad-Head-mac.zip
# The Windows download is built on the PC (cargo build --release) and zipped there by
# pc-tools/package-release.ps1.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$ROOT/dist"
rm -rf "$DIST/mac"
mkdir -p "$DIST/mac"
TH_UNIVERSAL=1 TH_APP_OUT="$DIST/mac/Trinidad Head.app" bash "$ROOT/scripts/build-mac-app.sh"
lipo -archs "$DIST/mac/Trinidad Head.app/Contents/MacOS/trinidad-head"
rm -f "$DIST/Trinidad-Head-mac.zip"
ditto -c -k --keepParent "$DIST/mac/Trinidad Head.app" "$DIST/Trinidad-Head-mac.zip"
ls -la "$DIST/Trinidad-Head-mac.zip"
