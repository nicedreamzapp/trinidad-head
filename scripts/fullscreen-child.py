"""A stand-in for Claude Code for the self-test: a full-screen program that owns the grid.

It takes the alternate screen, turns on mouse tracking, and paints one window onto a long
buffer it keeps to itself — exactly the shape that makes selecting past the edge hard, because
none of the text above or below is in the terminal's scrollback. A wheel report moves the
window and it repaints.

`CHILD_SHAPE=chat` paints the shape Claude Code really has: a prompt box pinned to the bottom
that never scrolls, a status line inside it that changes on every repaint, and three lines of
travel per wheel notch. A plain screen-wide scroll is the easy case; this is Matt's case.

It also streams: it starts at the top of its buffer and walks to the end, the way a chat does
when output arrives, so the terminal sees the same thing it sees in real life — a screen that
scrolls forward while nobody is touching the mouse. That is what the window records.
"""
import os, select, sys, termios, tty

LINES = [f"PROGRAM-LINE-{i:03}" for i in range(1, 301)]
rows = int(os.environ.get("LINES") or 24)
CHAT = os.environ.get("CHILD_SHAPE") == "chat"
FOOTER = 4 if CHAT else 0          # the prompt box Claude Code keeps at the bottom
STEP = 3 if CHAT else 1            # lines of travel per wheel notch
paints = 0


def size():
    try:
        return os.get_terminal_size().lines
    except OSError:
        return rows


def text_rows():
    return max(1, size() - FOOTER)


def paint(top):
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
    sys.stdout.write("".join(out))
    sys.stdout.flush()


fd = sys.stdin.fileno()
tty.setraw(fd)
sys.stdout.write("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?25l")
# Stream from the top to the end, a chunk at a time, the way output arrives in a chat.
top = 0
paint(top)
end = len(LINES) - text_rows()
buf = b""
while top < end:
    if select.select([fd], [], [], 0.02)[0]:
        break
    top = min(end, top + 3)
    paint(top)

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
            top = max(0, top - STEP)
            paint(top)
        elif button == 65:
            top = min(len(LINES) - text_rows(), top + STEP)
            paint(top)
