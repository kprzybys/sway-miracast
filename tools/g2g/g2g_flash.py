#!/usr/bin/env python3
"""g2g_flash: glass-to-glass latency from a webcam recording of the sink's screen.

Inputs: cam.nut (MJPEG, wallclock pts via -use_wallclock_as_timestamps), flashes.txt (screenflash wallclock times).
1. decode at 160x90 gray, keep per-frame pts;
2. the region that changes most (max-min per pixel over the run) is the sink's screen: top 15% pixels;
3. its mean brightness -> rises through the half level between dark baseline and white plateau;
4. each rise is matched to the latest flash before it (period must exceed the latency).
Usage: g2g_flash.py cam.nut flashes.txt
"""

import subprocess
import sys

import numpy as np

cam, flashes = sys.argv[1], sys.argv[2]
W, H = 160, 90
pts = subprocess.run(
    ["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "frame=pts_time", "-of", "csv=p=0", cam],
    capture_output=True,
    text=True,
    check=True,
).stdout.split()
pts = np.array([float(x) for x in pts if x])
raw = subprocess.run(
    [
        "ffmpeg",
        "-v",
        "error",
        "-i",
        cam,
        "-fps_mode",
        "passthrough",
        "-vf",
        f"scale={W}:{H},format=gray",
        "-f",
        "rawvideo",
        "-",
    ],
    capture_output=True,
    check=True,
).stdout
fr = np.frombuffer(raw, np.uint8).reshape(-1, H, W).astype(np.float32)
n = min(len(fr), len(pts))
fr, pts = fr[:n], pts[:n]
span = fr.max(0) - fr.min(0)
mask = span >= np.quantile(span, 0.85)
y = fr[:, mask].mean(1)
lo, hi = np.quantile(y, 0.1), np.quantile(y, 0.97)
half = (lo + hi) / 2
rises = [
    pts[i - 1] + (half - y[i - 1]) / (y[i] - y[i - 1]) * (pts[i] - pts[i - 1])
    for i in range(1, n)
    if y[i - 1] < half <= y[i]
]
fl = sorted(float(x) for x in open(flashes).read().split())
lat = []
for r in rises:
    prev = [f for f in fl if f < r]
    if prev:
        lat.append((r - prev[-1]) * 1000)
print(
    f"frames {n}  cam fps {n / (pts[-1] - pts[0]):.1f}  flashes {len(fl)}  rises {len(rises)}  contrast {hi - lo:.0f}"
)
if lat:
    a = np.array(lat)
    print("latency ms (incl. camera delay):", " ".join(f"{x:.0f}" for x in a))
    print(f"median {np.median(a):.0f}  min {a.min():.0f}  max {a.max():.0f}")
