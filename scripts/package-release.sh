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
# Signed with the Developer ID certificate and notarized by Apple, so it opens on any Mac with
# no warning. The notary login is an App Store Connect API key (ASC_KEY_ID / ASC_ISSUER, key
# file in ~/.appstoreconnect/private_keys).
DEVID="$(security find-identity -v -p codesigning | sed -n 's/.*) \([0-9A-F]\{40\}\) "Developer ID Application:.*/\1/p' | head -1)"
[ -n "$DEVID" ] || { echo "no Developer ID Application certificate on this Mac" >&2; exit 1; }
TH_SIGN_IDENTITY="$DEVID" TH_HARDENED=1 TH_UNIVERSAL=1 TH_APP_OUT="$DIST/mac/Trinidad Head.app" bash "$ROOT/scripts/build-mac-app.sh"
lipo -archs "$DIST/mac/Trinidad Head.app/Contents/MacOS/trinidad-head"
rm -f "$DIST/Trinidad-Head-mac.zip"
ditto -c -k --keepParent "$DIST/mac/Trinidad Head.app" "$DIST/Trinidad-Head-mac.zip"
KEY_ID="${ASC_KEY_ID:-VSXKZZ79TK}"
ISSUER="${ASC_ISSUER:-1ab8acba-26d0-4a22-b2e4-96398ed7ade5}"
xcrun notarytool submit "$DIST/Trinidad-Head-mac.zip" --key "$HOME/.appstoreconnect/private_keys/AuthKey_$KEY_ID.p8" \
  --key-id "$KEY_ID" --issuer "$ISSUER" --wait
xcrun stapler staple "$DIST/mac/Trinidad Head.app"
spctl --assess --type execute -vv "$DIST/mac/Trinidad Head.app"
# Zip again so the download carries the stapled ticket and opens even offline.
rm -f "$DIST/Trinidad-Head-mac.zip"
ditto -c -k --keepParent "$DIST/mac/Trinidad Head.app" "$DIST/Trinidad-Head-mac.zip"
ls -la "$DIST/Trinidad-Head-mac.zip"
