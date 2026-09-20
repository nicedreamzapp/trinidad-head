#!/bin/bash
# Build Trinidad Head for macOS and install it as ~/Applications/Trinidad Head.app.
# Each launch opens its own window:  open -n -a "Trinidad Head" --args <command>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$HOME/Applications/Trinidad Head.app"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

cd "$ROOT"
"$CARGO" build --release -p trinidad-head

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Icon: every size macOS asks for, from the 1024 px master.
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s "$ROOT/assets/icon-1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$ROOT/assets/icon-1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$WORK/AppIcon.icns"

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)"
STAGE="$WORK/Trinidad Head.app"
mkdir -p "$STAGE/Contents/MacOS" "$STAGE/Contents/Resources"
cp "$ROOT/target/release/trinidad-head" "$STAGE/Contents/MacOS/trinidad-head"
cp "$WORK/AppIcon.icns" "$STAGE/Contents/Resources/AppIcon.icns"
cat > "$STAGE/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Trinidad Head</string>
  <key>CFBundleDisplayName</key><string>Trinidad Head</string>
  <key>CFBundleIdentifier</key><string>com.nicedreamz.trinidadhead</string>
  <key>CFBundleExecutable</key><string>trinidad-head</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST
# Sign with the Apple Development certificate when this Mac has one. An ad-hoc signature
# changes with every build, and macOS then forgets Full Disk Access / Accessibility, so a
# failed signing stops the build instead of quietly installing an app that lost them.
IDENTITY="${TH_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*) \([0-9A-F]\{40\}\) "Apple Development:.*/\1/p' | head -1)}"
sign_adhoc() {
  codesign --force --deep --sign - "$STAGE" >/dev/null 2>&1 || true
  echo "ad-hoc signed (permissions reset on each build)"
}
if [ -z "$IDENTITY" ]; then
  if [ "${TH_ALLOW_ADHOC:-}" = 1 ]; then
    sign_adhoc
  else
    echo "no Apple Development certificate on this Mac." >&2
    echo "Set TH_SIGN_IDENTITY, or TH_ALLOW_ADHOC=1 to accept losing the app's permissions." >&2
    exit 1
  fi
elif codesign --force --deep --sign "$IDENTITY" "$STAGE" >/dev/null 2>&1; then
  echo "signed with $IDENTITY"
else
  # Over SSH the login keychain is locked in this session and codesign fails with
  # errSecInternalComponent. Finder is in the logged-in session, so ask it to run the
  # same command there.
  script="/usr/bin/codesign --force --deep --sign $IDENTITY '"'"'$STAGE'"'"'"
  if osascript -e "tell application \"Finder\" to do shell script \"$script\"" >/dev/null 2>&1 \
     && codesign --verify --strict "$STAGE" >/dev/null 2>&1; then
    echo "signed with $IDENTITY (through the logged-in session)"
  elif [ "${TH_ALLOW_ADHOC:-}" = 1 ]; then
    sign_adhoc
  else
    echo "codesign with $IDENTITY failed and the logged-in session could not do it either." >&2
    echo "Refusing to ad-hoc sign: that would drop Full Disk Access, Accessibility and" >&2
    echo "Screen Recording for Trinidad Head. Run this from a window on the Mac itself," >&2
    echo "or set TH_ALLOW_ADHOC=1 if you really want to re-grant them by hand." >&2
    exit 1
  fi
fi

mkdir -p "$HOME/Applications"
rm -rf "$APP"
mv "$STAGE" "$APP"
touch "$APP"
echo "installed $APP"
