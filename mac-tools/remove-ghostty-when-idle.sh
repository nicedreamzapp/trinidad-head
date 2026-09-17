#!/bin/bash
# remove-ghostty-when-idle.sh — one-shot: once Ghostty is no longer running, remove it.
#
# Installed 2026-09-17 when Trinidad Head replaced Ghostty. Runs every minute from the
# LaunchAgent com.nicedreamz.remove-ghostty. While any Ghostty is open (Matt's Claude sessions
# live in Ghostty windows) it does nothing. After Ghostty quits it:
#   - uninstalls the Ghostty cask (or moves Ghostty.app to the Trash if brew didn't install it)
#   - removes a Ghostty tile from the Dock, if there is one
#   - drops any LaunchServices file handlers that point at Ghostty itself (Ghostty Run, which
#     now opens Trinidad Head windows, stays the .command default)
#   - leaves ~/.config/ghostty in place as a backup
# then logs what it did and removes its own LaunchAgent and itself.
#
#   install:  remove-ghostty-when-idle.sh --install
LOG="$HOME/Library/Logs/remove-ghostty.log"
LABEL="com.nicedreamz.remove-ghostty"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
SELF="$HOME/Scripts/trinidad-head/remove-ghostty-when-idle.sh"
APP="/Applications/Ghostty.app"
BREW=/opt/homebrew/bin/brew

log() { echo "$(date '+%Y-%m-%d %H:%M:%S') $*" >> "$LOG"; }

if [ "${1:-}" = "--install" ]; then
    mkdir -p "$HOME/Scripts/trinidad-head" "$HOME/Library/LaunchAgents"
    [ "$0" -ef "$SELF" ] || install -m 755 "$0" "$SELF"
    cat > "$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$LABEL</string>
  <key>ProgramArguments</key><array><string>/bin/bash</string><string>$SELF</string></array>
  <key>StartInterval</key><integer>60</integer>
  <key>RunAtLoad</key><true/>
  <key>StandardErrorPath</key><string>$LOG</string>
</dict>
</plist>
EOF
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null
    launchctl bootstrap "gui/$(id -u)" "$PLIST"
    log "installed; waiting for Ghostty to quit"
    echo "installed $LABEL"
    exit 0
fi

# Still in use: wait.
if /usr/bin/pgrep -f "Ghostty.app/Contents/MacOS/ghostty" >/dev/null; then
    exit 0
fi

finish() {
    log "done; removing the watcher"
    rm -f "$PLIST"
    rm -f "$SELF"
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null
    exit 0
}

if [ ! -d "$APP" ]; then
    log "Ghostty.app is already gone"
else
    log "Ghostty is not running; removing it"
    if [ -x "$BREW" ] && "$BREW" list --cask ghostty >/dev/null 2>&1; then
        if "$BREW" uninstall --cask ghostty >> "$LOG" 2>&1; then
            log "brew uninstall --cask ghostty: ok"
        else
            log "brew uninstall failed; moving the app to the Trash instead"
        fi
    fi
    if [ -d "$APP" ]; then
        mv "$APP" "$HOME/.Trash/Ghostty.app.$(date +%Y%m%d-%H%M%S)" && log "moved Ghostty.app to the Trash" \
            || { log "could not remove $APP; will retry next minute"; exit 0; }
    fi
fi

# Dock tile, if any.
/usr/bin/python3 - <<'PY' >> "$LOG" 2>&1
import plistlib, subprocess
raw = subprocess.run(["defaults", "export", "com.apple.dock", "-"], capture_output=True).stdout
d = plistlib.loads(raw)
apps = d.get("persistent-apps", [])
keep = [t for t in apps if t.get("tile-data", {}).get("bundle-identifier") != "com.mitchellh.ghostty"]
if len(keep) != len(apps):
    d["persistent-apps"] = keep
    subprocess.run(["defaults", "import", "com.apple.dock", "-"], input=plistlib.dumps(d))
    subprocess.run(["killall", "Dock"])
    print("removed Ghostty from the Dock")
else:
    print("Ghostty was not pinned in the Dock")
PY

# File handlers that name Ghostty itself (not Ghostty Run).
/usr/bin/python3 - <<'PY' >> "$LOG" 2>&1
import os, plistlib, subprocess
p = os.path.expanduser("~/Library/Preferences/com.apple.LaunchServices/com.apple.launchservices.secure.plist")
try:
    d = plistlib.load(open(p, "rb"))
except OSError:
    d = {}
h = d.get("LSHandlers", [])
keep = [x for x in h if "com.mitchellh.ghostty" not in str(x).lower()]
if len(keep) != len(h):
    d["LSHandlers"] = keep
    plistlib.dump(d, open(p, "wb"))
    print(f"removed {len(h) - len(keep)} Ghostty file handler(s)")
else:
    print("no file handlers pointed at Ghostty")
PY
LSREG=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$LSREG" -u "$APP" >/dev/null 2>&1
"$LSREG" -f "$HOME/Applications/Ghostty Run.app" >/dev/null 2>&1
log "Ghostty Run re-registered; ~/.config/ghostty kept as a backup"
finish
