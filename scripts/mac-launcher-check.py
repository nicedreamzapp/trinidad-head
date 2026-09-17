#!/usr/bin/python3
"""Open each of Matt's Mac launchers through the real path (launcher app → Ghostty Run →
Trinidad Head), one at a time, and check it:

  - a Trinidad Head window opens, running a login shell with no CLAUDE* variables
  - Claude launchers: Claude Code starts (then the window is closed, no prompt sent)
  - local-model launchers: the model loads (its ready screen or server health)
  - everything the launcher started is stopped again afterwards

Heavy model launchers wait until Song Forge is idle and forge_guard has room.
Usage: mac-launcher-check.py [name-filter...]    Results: /tmp/trinidad-head-launchers.txt
"""
import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.request

HOME = os.path.expanduser("~")
REG = f"{HOME}/.trinidad-head/windows"
DUMPS = f"{HOME}/.trinidad-head/dumps"
FLAG = f"{HOME}/.trinidad-head/dump-all"
OUT = "/tmp/trinidad-head-launchers.txt"
LAUNCH = f"{HOME}/Desktop/Launchers"
LOCAL = f"{HOME}/Desktop/PROJECTS/Local AI Setup/launchers"
RUNNER = f"{HOME}/Applications/Ghostty Run.app"


def log(line):
    print(line, flush=True)
    with open(OUT, "a") as f:
        f.write(line + "\n")


def sh(cmd, timeout=30):
    try:
        return subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=timeout).stdout
    except subprocess.TimeoutExpired:
        return ""


def http_json(url, timeout=3):
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            return json.loads(r.read() or b"{}")
    except Exception:
        return None


def windows():
    try:
        return {int(n) for n in os.listdir(REG) if n.isdigit()}
    except FileNotFoundError:
        return set()


def procs():
    rows = {}
    for line in sh("ps -axo pid=,ppid=,command=").splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and parts[0].isdigit():
            rows[int(parts[0])] = (int(parts[1]), parts[2])
    return rows


def descendants(pid, rows=None):
    rows = rows or procs()
    out, todo = [], [pid]
    while todo:
        p = todo.pop()
        for c, (pp, cmd) in rows.items():
            if pp == p:
                out.append((c, cmd))
                todo.append(c)
    return out


def listening_ports():
    ports = {}
    for line in sh("lsof -nP -iTCP -sTCP:LISTEN -Fpn").splitlines():
        if line.startswith("p"):
            pid = int(line[1:])
        elif line.startswith("n"):
            m = re.search(r":(\d+)$", line)
            if m:
                ports[int(m.group(1))] = pid
    return ports


def screen(pid):
    try:
        return open(f"{DUMPS}/{pid}.txt", errors="replace").read()
    except OSError:
        return ""


def forge_idle():
    st = http_json("http://127.0.0.1:8767/api/status")
    return st is None or st.get("jobs_running", 0) == 0


def mem_room(gb):
    try:
        st = json.loads(sh(f"/usr/bin/python3 {HOME}/SongForgeM5/mem_client.py state"))
    except Exception:
        return True
    others = sum(l["gb"] for l in st.get("leases", []) if l["gb"] >= 5)
    return others == 0


def brave_count(fragment):
    r = sh(f"""osascript -e 'set n to 0' -e 'tell application "Brave Browser" to repeat with w in windows' -e 'set n to n + (count (tabs of w whose URL contains "{fragment}"))' -e 'end repeat' -e 'return n'""")
    try:
        return sum(int(x) for x in re.findall(r"\d+", r))
    except ValueError:
        return 0


def brave_close(fragment):
    sh(f"""osascript -e 'tell application "Brave Browser" to repeat with w in windows' -e 'close (tabs of w whose URL contains "{fragment}")' -e 'end repeat'""")


# name, how to launch, kind, ready test, timeout, extra
SPECS = [
    dict(name="Claude Code.app", open=f"{LAUNCH}/Claude Code.app", kind="claude"),
    dict(name="Divine Tribe HQ.app", open=f"{HOME}/Desktop/Divine Tribe HQ.app", kind="claude"),
    dict(name="Cannabis Device Safety Institute.app", open=f"{LAUNCH}/Cannabis Device Safety Institute.app", kind="claude"),
    dict(name="Job Track.app", open=f"{LAUNCH}/Job Track.app", kind="claude"),
    dict(name="Reddit Mastermind.app", open=f"{LAUNCH}/Reddit Mastermind.app", kind="claude"),
    dict(name="Free Claude.app (cloud free model)", open=f"{LAUNCH}/Free Claude.app", kind="claude"),
    dict(name="Divine Tribe HQ Free.app (cloud free model)", open=f"{LAUNCH}/Divine Tribe HQ Free.app", kind="agent", timeout=120),
    dict(name="Voice Board.command", open=f"{LAUNCH}/Voice Board.command", kind="quick", brave="VoiceBoard/index.html"),
    dict(name="Mac Mini.command", open=f"{LAUNCH}/Mac Mini.command", kind="quick", brave="127.0.0.1:9494", port=9494),
    dict(name="MattPaint Studio.app", open=f"{LAUNCH}/MattPaint Studio.app", kind="ready", ready="What should I paint", timeout=60),
    dict(name="Wan Avatar.command (ComfyUI)", open=f"{LAUNCH}/Wan Avatar.command", kind="quick", port=8188, brave="127.0.0.1:8188", timeout=240, heavy_gb=0),
    dict(name="Gemma 4 - Chat.command (local Gemma 4)", open=f"{LAUNCH}/Gemma 4 - Chat.command", kind="agent", timeout=600, heavy_gb=48),
    dict(name="Narrative Gemma.command (local Gemma 4)", open=f"{LAUNCH}/Narrative Gemma.command", kind="agent", timeout=600, heavy_gb=48),
    dict(name="Gemma 4 Browser.command (local Gemma 4)", open=f"{LAUNCH}/Gemma 4 Browser.command", kind="ready", ready="What should I do", timeout=600, heavy_gb=48, health=4000),
    dict(name="Qwen 3.8 Heretic.app (local Qwen 3.8)", open=f"{LAUNCH}/Qwen 3.8 Heretic.app", kind="agent", timeout=600, heavy_gb=48, forge=True),
    dict(name="Qwen 3.8 Browser.app (local Qwen 3.8)", open=f"{LAUNCH}/Qwen 3.8 Browser.app", kind="ready", ready="What should I do", timeout=600, heavy_gb=48, forge=True, health=4000),
]


def wait_until(test, timeout, step=0.5):
    end = time.time() + timeout
    while time.time() < end:
        v = test()
        if v:
            return v
        time.sleep(step)
    return None


def check(spec):
    name = spec["name"]
    heavy = spec.get("heavy_gb", 0)
    if heavy:
        if not wait_until(lambda: forge_idle() and mem_room(heavy), 1800, 10):
            log(f"SKIP {name}: Song Forge busy or no memory room for 30 min")
            return
    before_windows = windows()
    before_ports = listening_ports()
    before_procs = set(procs())
    brave_before = brave_count(spec["brave"]) if spec.get("brave") else 0
    t0 = time.time()
    subprocess.run(["/usr/bin/open", spec["open"]])

    seen = {}

    def new_window():
        for p in windows() - before_windows:
            seen.setdefault(p, time.time())
        return next(iter(seen), None)

    pid = wait_until(new_window, 25, 0.1)
    if not pid:
        log(f"FAIL {name}: no Trinidad Head window appeared")
        return
    # The shell the window started.
    shell = wait_until(lambda: next((c for c, cmd in descendants(pid) if "zsh" in cmd), None), 5, 0.1)
    ok_shell, detail = True, ""
    if shell:
        cmd = sh(f"ps -o command= -p {shell}").strip()
        env = sh(f"ps -Eww -o command= -p {shell}")
        claude_vars = sorted(set(re.findall(r"(?:^|\s)(CLAUDE[A-Z_]*)=", env)))
        login = " -l" in cmd
        ok_shell = login and not claude_vars
        detail = f"shell '{cmd}', CLAUDE vars {claude_vars or 'none'}"
    elif spec["kind"] != "quick":
        ok_shell, detail = False, "shell not found"

    kind = spec["kind"]
    ready_ok, ready_detail = True, ""
    timeout = spec.get("timeout", 90)
    if kind == "claude":
        r = wait_until(lambda: "Claude Code v" in screen(pid) and any("claude" in c for _, c in descendants(pid)), timeout)
        ready_ok = bool(r)
        m = re.search(r"Claude Code v[\d.]+", screen(pid))
        model = re.search(r"\n\S*\s*(.*(?:Opus|Sonnet|Haiku|/|:free).*?)\n", screen(pid))
        ready_detail = f"{m.group(0) if m else 'Claude did not start'}"
        if model:
            ready_detail += f" · {model.group(1).strip()[:70]}"
    elif kind == "agent":
        r = wait_until(lambda: "paste lands in the box" in screen(pid) or "press Return to close" in screen(pid), timeout, 2)
        text = screen(pid)
        ready_ok = bool(r) and "press Return to close" not in text and "Traceback" not in text
        title = next((l.strip() for l in text.splitlines() if l.strip()), "")
        ready_detail = f"ready screen shown ({title[:60]})" if ready_ok else "not ready: " + " | ".join(l.strip() for l in text.splitlines() if l.strip())[-300:]
    elif kind == "ready":
        r = wait_until(lambda: spec["ready"] in screen(pid), timeout, 2)
        ready_ok = bool(r)
        ready_detail = f"shows '{spec['ready']}'" if r else "not ready: " + " | ".join(l.strip() for l in screen(pid).splitlines() if l.strip())[-300:]
    elif kind == "quick":
        # The .command window runs, then closes itself.
        closed = wait_until(lambda: not os.path.exists(f"{REG}/{pid}"), 20, 0.2)
        ready_detail = "window ran and closed itself" if closed else "window still open"
        ready_ok = bool(closed)
        if spec.get("port"):
            up = wait_until(lambda: spec["port"] in listening_ports(), timeout, 2)
            ready_ok &= bool(up)
            ready_detail += f"; port {spec['port']} {'up' if up else 'never came up'}"
        if spec.get("brave"):
            opened = wait_until(lambda: brave_count(spec["brave"]) > brave_before, 20, 1)
            ready_detail += "; Brave page opened" if opened else "; Brave page NOT opened"
            ready_ok &= bool(opened)
    if spec.get("health") and ready_ok:
        h = http_json(f"http://127.0.0.1:{spec['health']}/health")
        ready_detail += f"; server {spec['health']} health {h and h.get('status')}"
        ready_ok &= bool(h and h.get("status") == "ok")
    secs = time.time() - t0

    # Close what was opened.
    tree = descendants(pid)
    if os.path.exists(f"{REG}/{pid}"):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    wait_until(lambda: not os.path.exists(f"{REG}/{pid}") and pid not in procs(), 10)
    time.sleep(3)
    rows = procs()
    leftovers = [(c, cmd) for c, cmd in tree if c in rows and "restore" not in cmd]
    for port, p in listening_ports().items():
        if port not in before_ports and p not in before_procs and p != os.getpid():
            leftovers.append((p, rows.get(p, (0, f"port {port}"))[1]))
    stopped = []
    for c, cmd in leftovers:
        # Song Forge coming back is intended; never touch it.
        if "SongForge" in cmd or "songforge" in cmd or re.search(r":(8001|8767|9420)\b", cmd):
            continue
        try:
            os.kill(c, signal.SIGTERM)
            stopped.append(cmd.split()[0].split("/")[-1])
        except ProcessLookupError:
            pass
    if spec.get("brave") and brave_count(spec["brave"]) > brave_before and brave_before == 0:
        brave_close(spec["brave"])
        stopped.append("Brave tab")
    forge_note = ""
    if spec.get("forge"):
        back = wait_until(lambda: (http_json("http://127.0.0.1:8767/api/status") or {}).get("ace_up"), 180, 3)
        forge_note = "; Song Forge back up" if back else "; SONG FORGE NOT BACK"
        if not back:
            ready_ok = False
    verdict = "PASS" if ok_shell and ready_ok else "FAIL"
    extra = f"; stopped {', '.join(sorted(set(stopped)))}" if stopped else ""
    log(f"{verdict} {name} ({secs:.0f}s): {detail}; {ready_detail}{forge_note}{extra}")


def main():
    filters = [a.lower() for a in sys.argv[1:]]
    os.makedirs(os.path.dirname(FLAG), exist_ok=True)
    open(FLAG, "w").close()
    try:
        for spec in SPECS:
            if filters and not any(f in spec["name"].lower() for f in filters):
                continue
            try:
                check(spec)
            except Exception as e:  # keep going
                log(f"FAIL {spec['name']}: checker error {e!r}")
    finally:
        os.remove(FLAG)


if __name__ == "__main__":
    main()
