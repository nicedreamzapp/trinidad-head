#!/bin/bash
# Build Trinidad Head for macOS and install it as ~/Applications/Trinidad Head.app.
# Each launch opens its own window:  open -n -a "Trinidad Head" --args <command>
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# TH_APP_OUT puts the app somewhere else (the release packager uses it); TH_UNIVERSAL=1 builds
# one app that runs on both Apple Silicon and Intel Macs.
APP="${TH_APP_OUT:-$HOME/Applications/Trinidad Head.app}"
CARGO="${CARGO:-$HOME/.cargo/bin/cargo}"

cd "$ROOT"
if [ "${TH_UNIVERSAL:-}" = 1 ]; then
  "$CARGO" build --release -p trinidad-head --target aarch64-apple-darwin
  "$CARGO" build --release -p trinidad-head --target x86_64-apple-darwin
  BIN="$ROOT/target/universal/trinidad-head"
  mkdir -p "$(dirname "$BIN")"
  lipo -create -output "$BIN" "$ROOT/target/aarch64-apple-darwin/release/trinidad-head" "$ROOT/target/x86_64-apple-darwin/release/trinidad-head"
else
  "$CARGO" build --release -p trinidad-head
  BIN="$ROOT/target/release/trinidad-head"
fi

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
cp "$BIN" "$STAGE/Contents/MacOS/trinidad-head"
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
  # Over SSH the login keychain is locked in this session, so codesign fails with
  # errSecInternalComponent. A Trinidad Head window runs in the logged-in session, where the
  # keychain is open, so hand the same command to one of those and wait for it. (Finder's
  # "do shell script" cannot: it hits "User interaction is not allowed".)
  TH_OPEN=""
  for c in "$HOME/Scripts/trinidad-head/th-open" "$ROOT/mac-tools/th-open"; do
    [ -x "$c" ] && TH_OPEN="$c" && break
  done
  FLAG="$WORK/signed"
  if [ -n "$TH_OPEN" ] && [ -x "$APP/Contents/MacOS/trinidad-head" ]; then
    cat > "$WORK/sign.command" <<SIGN
#!/bin/bash
/usr/bin/codesign --force --deep --sign "$IDENTITY" "$STAGE" && echo ok > "$FLAG"
SIGN
    chmod +x "$WORK/sign.command"
    "$TH_OPEN" "$WORK/sign.command" >/dev/null 2>&1 || true
    for _ in $(seq 60); do
      [ -f "$FLAG" ] && break
      sleep 0.5
    done
  fi
  if [ -f "$FLAG" ] && codesign --verify --strict "$STAGE" >/dev/null 2>&1; then
    echo "signed with $IDENTITY (in a window on the Mac itself)"
  elif [ "${TH_ALLOW_ADHOC:-}" = 1 ]; then
    sign_adhoc
  else
    echo "codesign with $IDENTITY failed, and no window on the Mac could do it either." >&2
    echo "Refusing to ad-hoc sign: that would drop Full Disk Access, Accessibility and" >&2
    echo "Screen Recording for Trinidad Head. Run this from a window on the Mac itself," >&2
    echo "or set TH_ALLOW_ADHOC=1 if you really want to re-grant them by hand." >&2
    exit 1
  fi
fi

mkdir -p "$(dirname "$APP")"
rm -rf "$APP"
mv "$STAGE" "$APP"
touch "$APP"
echo "installed $APP"
