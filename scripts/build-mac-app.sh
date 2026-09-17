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
# changes with every build, and macOS then forgets Full Disk Access / Accessibility.
IDENTITY="${TH_SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*) \([0-9A-F]\{40\}\) "Apple Development:.*/\1/p' | head -1)}"
if [ -n "$IDENTITY" ] && codesign --force --deep --sign "$IDENTITY" "$STAGE" >/dev/null 2>&1; then
  echo "signed with $IDENTITY"
else
  codesign --force --deep --sign - "$STAGE" >/dev/null 2>&1 || true
  echo "ad-hoc signed (permissions reset on each build)"
fi

mkdir -p "$HOME/Applications"
rm -rf "$APP"
mv "$STAGE" "$APP"
touch "$APP"
echo "installed $APP"
