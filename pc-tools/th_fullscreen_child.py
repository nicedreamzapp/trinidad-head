"""A stand-in for Claude Code on Windows: a full-screen program that owns the grid.

It takes the alternate screen, turns on mouse tracking, and paints one window onto a long
buffer it keeps to itself, so none of the text above or below is in the terminal's scrollback.
Same idea as scripts/fullscreen-child.py, with the console-mode calls Windows needs instead
of termios.

`CHILD_SHAPE=chat` paints the shape Claude Code really has: a prompt box pinned to the bottom
that never scrolls, a status line inside it that changes on every repaint, and three lines of
travel per wheel notch. A plain screen-wide scroll is the easy case; this is Matt's case.

It also streams: it starts at the top of its buffer and walks to the end, the way a chat does
when output arrives, so the terminal sees the same thing it sees in real life — a screen that
scrolls forward while nobody is touching the mouse. It streams in bursts, because that is how
a chat really arrives: a dozen repaints land in one read, having travelled further than the
screen is tall. That is what the window records into alt_history, and that recording is what a
selection scrolls back through.
"""
import ctypes, msvcrt, os, sys, time

k32 = ctypes.windll.kernel32
k32.SetConsoleMode(k32.GetStdHandle(-10), 0x200)            # VT input: raw bytes, no echo
k32.SetConsoleMode(k32.GetStdHandle(-11), 0x0001 | 0x0004)  # processed output + VT

LINES = [f"PROGRAM-LINE-{i:03}" for i in range(1, 301)]
CHAT = os.environ.get("CHILD_SHAPE") == "chat"
FOOTER = 4 if CHAT else 0          # the prompt box Claude Code keeps at the bottom
STEP = 3 if CHAT else 1            # lines of travel per wheel notch
paints = 0


def size():
    try:
        return os.get_terminal_size().lines
    except OSError:
        return 24


def text_rows():
    return max(1, size() - FOOTER)


def frame(top):
    global paints
    paints += 1
    h = size()
    body = text_rows()
    out = ["\x1b[H"]
    for i in range(h):
        out.append("\x1b[2K")
        if i < body:
            if top + i < len(LINES):
                out.append(LINES[top + i])
        elif i == body:
            out.append("╭" + "─" * 20 + "╮")
        elif i == body + 1:
            out.append("│ > ask me anything  │")
        elif i == body + 2:
            out.append("╰" + "─" * 20 + "╯")
        else:
            # the status line ticks, the way a spinner or a token count does
            out.append(f"  ? for shortcuts        {paints} paints")
        out.append("\r\n" if i < h - 1 else "")
    return "".join(out)


def paint(top):
    sys.stdout.write(frame(top))
    sys.stdout.flush()


sys.stdout.write("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?25l")
# Stream from the top to the end, a chunk at a time, the way output arrives in a chat.
#
# Bursts are deliberately small. A program that writes repaints faster than the Windows console
# draws does not get them delivered: conhost keeps its own screen, paints it on its own clock,
# and sends only what it last drew. Measured 2026-09-21 with TRINIDAD_HEAD_RAW: twelve repaints
# written back to back arrived as one, and 84 of 300 lines were never handed to the terminal at
# all. Nothing this side of the pipe can recover those, so the test does not pretend to.
top = 0
paint(top)
end = len(LINES) - text_rows()
buf = b""
while top < end:
    if msvcrt.kbhit():
        break
    burst = []
    for _ in range(3):
        if top >= end:
            break
        top = min(end, top + 3)
        burst.append(frame(top))
    sys.stdout.write("".join(burst))
    sys.stdout.flush()
    time.sleep(0.02)

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
            top = max(0, top - STEP)
            paint(top)
        elif button == 65:
            top = min(len(LINES) - text_rows(), top + STEP)
            paint(top)
