"""Child program for the Trinidad Head self-test: turns on mouse reporting, logs raw input."""
import ctypes, os, sys, threading, time
k32 = ctypes.windll.kernel32
h = k32.GetStdHandle(-10)
k32.SetConsoleMode(h, 0x200)             # VT input only: raw bytes, no echo, no line editing
out = k32.GetStdHandle(-11)
k32.SetConsoleMode(out, 0x0001 | 0x0004)  # processed output + VT processing
log = open(r"C:\Users\matt\dev\th_test_input.log", "wb", buffering=0)
def reader():
    while True:
        try:
            data = os.read(0, 4096)
        except OSError:
            return
        if not data:
            return
        log.write(repr(data).encode() + b"\n")
threading.Thread(target=reader, daemon=True).start()
w = sys.stdout.write
w("PHASE-A mouse on\r\n\x1b[?1002h\x1b[?1006h\x1b[?2004h"); sys.stdout.flush()
time.sleep(22)
w("\x1b[?1002l\x1b[?1006l\r\nPHASE-B mouse off\r\n"); sys.stdout.flush()
w("\x1b]52;c;b3NjNTItb2s=\x07")           # "osc52-ok"
w("SELECT-ME alpha beta\r\nLINK https://ineedhemp.com/shop ok\r\n"); sys.stdout.flush()
time.sleep(14)
# Phase C: several screens of numbered lines, so most of them sit in the scrollback and a
# drag held past an edge has somewhere to scroll to.
w("PHASE-C scrollback\r\n")
for i in range(1, 121):
    w(f"AUTOSCROLL-LINE-{i:03}\r\n")
sys.stdout.flush()
time.sleep(60)
