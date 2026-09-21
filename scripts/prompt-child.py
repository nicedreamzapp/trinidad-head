"""A stand-in for Claude Code's prompt box, for the self-test of deleting a highlight.

It paints what Claude paints around the text you type: a rule, `❯ ` and the text word-wrapped
two columns in (a line breaks at a space, and that space is not drawn), another rule, and a
status line. It takes the alternate screen and the mouse, and a click inside the text moves
the caret there, which is the one thing about Claude the terminal leans on. Backspace and
Delete edit at the caret, bracketed paste inserts, Enter moves the text up into the
transcript. After every change it writes the exact text to PROMPT_CHILD_OUT, so the test
checks the program's own buffer rather than what happens to be on screen.
"""
import os, re, sys

OUT = os.environ.get("PROMPT_CHILD_OUT", "/tmp/prompt-child.txt")
TEXT_COL = 2
text = []        # the prompt, one entry per character
caret = 0
transcript = ["TRANSCRIPT-LINE-1 something said earlier", "TRANSCRIPT-LINE-2 more of it"]
edits = 0


def size():
    try:
        s = os.get_terminal_size()
        return s.columns, s.lines
    except OSError:
        return 80, 24


def layout(width):
    """Rows as (start offset, characters), broken at spaces the way Claude breaks them."""
    w = max(1, width - TEXT_COL)
    rows, i = [], 0
    while True:
        if len(text) - i <= w:
            rows.append((i, text[i:]))
            return rows
        cut = next((k for k in range(i + w, i, -1) if text[k] == " "), i + w)
        rows.append((i, text[i:cut]))
        i = cut + 1 if cut < len(text) and text[cut] == " " else cut


def caret_cell(rows):
    for r, (s, chars) in enumerate(rows):
        nxt = rows[r + 1][0] if r + 1 < len(rows) else None
        if caret <= s + len(chars) and (nxt is None or caret < nxt):
            return r, TEXT_COL + caret - s
    s, chars = rows[-1]
    return len(rows) - 1, TEXT_COL + len(chars)


def paint():
    cols, lines = size()
    rows = layout(cols)
    box_top = max(1, lines - 3 - len(rows))  # rule, rows, rule, status
    # Every row is written out whole, top to bottom. No clear-screen: Windows' console host
    # turns a full clear into scrolling, which left the PC's window blank.
    screen = [""] * lines
    body = transcript[-(box_top - 1):] if box_top > 1 else []
    for i, l in enumerate(body):
        screen[i] = l[:cols]
    screen[box_top - 1] = "─" * cols
    for r, (_, chars) in enumerate(rows):
        lead = "❯ " if r == 0 else "  "
        screen[box_top + r] = lead + "".join(chars)
    screen[box_top + len(rows)] = "─" * cols
    if box_top + len(rows) + 1 < lines:
        screen[box_top + len(rows) + 1] = f"  status {edits} edits"
    out = ["\x1b[?25l"]
    for i, l in enumerate(screen):
        out.append(f"\x1b[{i + 1};1H\x1b[2K{l}")
    cr, cc = caret_cell(rows)
    out.append(f"\x1b[{box_top + 1 + cr};{cc + 1}H\x1b[?25h")
    sys.stdout.write("".join(out))
    sys.stdout.flush()
    return box_top, rows


def save():
    global edits
    edits += 1
    with open(OUT, "w") as f:
        f.write("".join(text))


def click(row, col):
    """A click at a 0-based screen cell moves the caret, if it lands on the text."""
    global caret
    cols, _ = size()
    box_top, rows = state
    r = row - box_top  # box_top is 1-based, the first text row is box_top + 1
    if 0 <= r < len(rows):
        s, chars = rows[r]
        caret = s + min(max(col - TEXT_COL, 0), len(chars))


fd = sys.stdin.fileno()
if os.name == "nt":
    # The PC's self-test runs this too: raw VT bytes in, VT and UTF-8 out.
    import ctypes
    k32 = ctypes.windll.kernel32
    k32.SetConsoleMode(k32.GetStdHandle(-10), 0x200)
    k32.SetConsoleMode(k32.GetStdHandle(-11), 0x0001 | 0x0004)
    sys.stdout.reconfigure(encoding="utf-8")
else:
    import tty
    tty.setraw(fd)
sys.stdout.write("\x1b[?1049h\x1b[?1002h\x1b[?1006h\x1b[?2004h")
save()
state = paint()
buf = b""
MOUSE = re.compile(rb"\x1b\[<(\d+);(\d+);(\d+)([Mm])")
while True:
    try:
        data = os.read(fd, 4096)
    except OSError:
        break
    if not data:
        break
    buf += data
    while buf:
        m = MOUSE.match(buf)
        if m:
            buf = buf[m.end():]
            b, x, y, kind = int(m[1]), int(m[2]), int(m[3]), m[4]
            if b == 0 and kind == b"M":
                click(y - 1, x - 1)
            continue
        if buf.startswith(b"\x1b[200~"):
            end = buf.find(b"\x1b[201~")
            if end < 0:
                break
            pasted = buf[6:end].decode("utf-8", "replace").replace("\r", " ")
            buf = buf[end + 6:]
            text[caret:caret] = list(pasted)
            caret += len(pasted)
            save()
            continue
        if buf.startswith(b"\x1b[3~"):
            buf = buf[4:]
            if caret < len(text):
                del text[caret]
                save()
            continue
        if buf.startswith(b"\x1b") and len(buf) < 3:
            break
        if buf.startswith(b"\x1b"):
            buf = buf[1:]  # anything else escaped is ignored
            continue
        if buf[0] == 0x7F:
            buf = buf[1:]
            if caret > 0:
                caret -= 1
                del text[caret]
                save()
            continue
        if buf[0] == 0x0D:
            buf = buf[1:]
            transcript.append("".join(text))
            text.clear()
            caret = 0
            save()
            continue
        # One UTF-8 character of typing.
        n = 1 if buf[0] < 0x80 else 2 if buf[0] < 0xE0 else 3 if buf[0] < 0xF0 else 4
        if len(buf) < n:
            break
        ch, buf = buf[:n].decode("utf-8", "replace"), buf[n:]
        if ch.isprintable():
            text.insert(caret, ch)
            caret += 1
            save()
    state = paint()
