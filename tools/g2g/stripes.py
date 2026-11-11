#!/usr/bin/env python3
"""stripes: full-terminal time code for glass-to-glass latency. Run in a fullscreen kitty.

Phase 1 (calibration): 1.5 s white, 1.5 s black (the analyser finds the screen in the camera image).
Phase 2: 14 vertical bars at ~30 Hz: bar 0 white, bar 13 black (references), bars 1..12 = Gray code of a frame
counter (MSB left). LOG gets "<counter> <wallclock after flush>" per drawn frame and "cal white|black <t>".
Usage: stripes.py LOG SECONDS
"""

import os
import sys
import time

log = open(sys.argv[1], "w", buffering=1)
secs = float(sys.argv[2])
NB = 12
out = sys.stdout


def size():
    try:
        c, r = os.get_terminal_size()
    except OSError:
        c, r = 160, 45
    return c, r


def fill(rgb):
    c, r = size()
    out.write(f"\033[H\033[48;2;{rgb};{rgb};{rgb}m" + (" " * c) * r)
    out.flush()


def bars(bits):
    c, r = size()
    n = len(bits)
    edges = [round(i * c / n) for i in range(n + 1)]  # equal bars (integer cells, remainder spread)
    line = ""
    for i, b in enumerate(bits):
        v = 255 if b else 0
        line += f"\033[48;2;{v};{v};{v}m" + " " * (edges[i + 1] - edges[i])
    out.write("\033[H" + line * r)
    out.flush()


def frame(n):
    g = n ^ (n >> 1)
    bars([1] + [(g >> (NB - 1 - i)) & 1 for i in range(NB)] + [0])


# calibration: 14 alternating bars and their inverse (constant mean brightness for the webcam's auto exposure;
# the analyser finds the bar boundaries where A-B changes sign)
out.write("\033[?25l\033[2J")
fill(0)
time.sleep(3.0)  # the window goes fullscreen ~1.5 s after start: calibrate only after that
ALT = [(i + 1) % 2 for i in range(NB + 2)]
for _ in range(3):
    bars(ALT)
    log.write(f"cal A {time.time():.4f}\n")
    time.sleep(1.2)
    bars([1 - x for x in ALT])
    log.write(f"cal B {time.time():.4f}\n")
    time.sleep(1.2)
t0 = time.monotonic()
n = 0
while time.monotonic() - t0 < secs:
    frame(n % (1 << NB))
    log.write(f"{n % (1 << NB)} {time.time():.4f}\n")
    n += 1
    nxt = t0 + n / float(os.environ.get("RATE", "30"))
    d = nxt - time.monotonic()
    if d > 0:
        time.sleep(d)
fill(0)
