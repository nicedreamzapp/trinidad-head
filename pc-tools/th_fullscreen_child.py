"""A stand-in for Claude Code on Windows: a full-screen program that owns the grid.

It takes the alternate screen, turns on mouse tracking, and paints one window onto a long
buffer it keeps to itself, so none of the text above or below is in the terminal's scrollback.
A wheel report moves the window and it repaints. Same idea as scripts/fullscreen-child.py,
with the console-mode calls Windows needs instead of termios.
"""
import ctypes, os, sys

k32 = ctypes.windll.kernel32
k32.SetConsoleMode(k32.GetStdHandle(-10), 0x200)            # VT input: raw bytes, no echo
k32.SetConsoleMode(k32.GetStdHandle(-11), 0x0001 | 0x0004)  # processed output + VT

LINES = [f"PROGRAM-LINE-{i:03}" for i in range(1, 301)]


def size():
    try:
        return os.get_terminal_size().lines
    except OSError:
        return 24


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


sys.stdout.write("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?25l")
top = len(LINES) - size()          # start at the end, the way a chat does
paint(top)

buf = b""
while True:
    try:
        data = os.read(0, 4096)
    except OSError:
        break
    if not data:
        break
    buf += data
    while True:
        ends = [buf.index(c) for c in (b"M", b"m") if c in buf]
        if not ends:
            break
        end = min(ends)
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
