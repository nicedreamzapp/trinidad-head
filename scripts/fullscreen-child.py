"""A stand-in for Claude Code for the self-test: a full-screen program that owns the grid.

It takes the alternate screen, turns on mouse tracking, and paints one window onto a long
buffer it keeps to itself — exactly the shape that makes selecting past the edge hard, because
none of the text above or below is in the terminal's scrollback. A wheel report moves the
window and it repaints.
"""
import os, sys, termios, tty

LINES = [f"PROGRAM-LINE-{i:03}" for i in range(1, 301)]
rows = int(os.environ.get("LINES") or 24)


def size():
    try:
        return os.get_terminal_size().lines
    except OSError:
        return rows


def paint(top):
    h = size()
    out = ["\x1b[H"]
    for i in range(h):
        out.append("\x1b[2K")
        if top + i < len(LINES):
            out.append(LINES[top + i])
        out.append("\r\n" if i < h - 1 else "")
    sys.stdout.write("".join(out))
    sys.stdout.flush()


fd = sys.stdin.fileno()
tty.setraw(fd)
sys.stdout.write("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?25l")
top = len(LINES) - size()          # start at the end, the way a chat does
paint(top)

buf = b""
while True:
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    buf += data
    while b"M" in buf or b"m" in buf:
        end = min((buf.index(c) for c in (b"M", b"m") if c in buf), default=-1)
        if end < 0:
            break
        report, buf = buf[: end + 1], buf[end + 1 :]
        if not report.startswith(b"\x1b[<"):
            continue
        try:
            button = int(report[3:].split(b";")[0])
        except ValueError:
            continue
        if button == 64:
            top = max(0, top - 1)
            paint(top)
        elif button == 65:
            top = min(len(LINES) - size(), top + 1)
            paint(top)
