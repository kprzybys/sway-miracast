#!/usr/bin/env python3
"""audioglitch: count audio stutters on the TV, using the laptop microphone against the sent signal.

Records the default sink monitor (what we send) and the microphone (what the TV plays) for SECS while
something plays (music works best), aligns them on band-energy spectrograms and tracks the local lag in
0.5 s windows (hop 0.1 s). A stutter (TV dropped/inserted audio, buffer resync) shows as a lag jump;
a dropout as a correlation dip while the reference is loud.

Usage: audioglitch.py SECS [--label X] [--mic SOURCE] [--keep DIR]    prints one summary line (+JSON)
"""

import argparse
import json
import os
import subprocess
import tempfile
import time

import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument("secs", type=float)
ap.add_argument("--label", default="")
ap.add_argument("--mic", default="alsa_input.pci-0000_00_1f.3.analog-stereo")
ap.add_argument("--keep", default=None)
a = ap.parse_args()
R, HOP = 48000, 480  # 10 ms
d = a.keep or tempfile.mkdtemp(prefix="audioglitch-")
os.makedirs(d, exist_ok=True)
rec = lambda dev, out: subprocess.Popen(
    [
        "timeout",
        f"{a.secs:.1f}",
        "parec",
        "-d",
        dev,
        "--format=s16le",
        f"--rate={R}",
        "--channels=1",
        "--latency-msec=5",
    ],
    stdout=open(out, "wb"),
)
t0 = time.time()
pm, pr = rec(a.mic, f"{d}/mic.raw"), rec("@DEFAULT_MONITOR@", f"{d}/mon.raw")
pm.wait()
pr.wait()
mic = np.fromfile(f"{d}/mic.raw", "<i2").astype(float)
mon = np.fromfile(f"{d}/mon.raw", "<i2").astype(float)


def bands(x):
    """log energy in 12 bands 150-5000 Hz per 10 ms frame (window 20 ms)."""
    n = (len(x) - 960) // HOP
    idx = np.arange(960)[None, :] + HOP * np.arange(n)[:, None]
    S = np.abs(np.fft.rfft(x[idx] * np.hanning(960), axis=1)) ** 2
    f = np.fft.rfftfreq(960, 1 / R)
    edges = np.geomspace(150, 5000, 13)
    B = np.stack([S[:, (f >= lo) & (f < hi)].sum(1) for lo, hi in zip(edges[:-1], edges[1:], strict=False)], 1)
    B = np.log(B + 1e3)
    B -= B.mean(0)
    return B / (B.std(0) + 1e-9), np.log(S.sum(1) + 1)


Bm, _ = bands(mic)
Br, er = bands(mon)
n = min(len(Bm), len(Br))


def corr(i0, i1, lag):
    """mean correlation of ref frames [i0,i1) with mic frames shifted by lag."""
    j0, j1 = i0 + lag, i1 + lag
    if j0 < 0 or j1 > len(Bm):
        return -1
    x, y = Br[i0:i1].ravel(), Bm[j0:j1].ravel()
    x = x - x.mean()
    y = y - y.mean()
    return float(np.dot(x, y) / (np.linalg.norm(x) * np.linalg.norm(y) + 1e-9))


# global lag: 200-1500 ms, on the middle part
mid0, mid1 = n // 5, n // 5 + min(1000, n // 2)
lags = range(20, 150)
g = max(lags, key=lambda L: corr(mid0, mid1, L))
gc = corr(mid0, mid1, g)
W, STEP = 50, 10  # 0.5 s windows, 0.1 s hop
track = []
for i0 in range(0, n - W - 160, STEP):
    cands = range(max(0, g - 8), g + 9)
    c, L = max((corr(i0, i0 + W, L), L) for L in cands)
    loud = er[i0 : i0 + W].mean()
    track.append((i0 * 10, L * 10, c, loud))
tr = np.array(track)
loudthr = np.percentile(tr[:, 3], 30)
ok = tr[:, 3] > loudthr
jumps = int(np.sum(np.abs(np.diff(tr[ok, 1])) >= 20))  # >= 20 ms lag change between windows
dips = int(np.sum(tr[ok, 2] < gc * 0.5))  # windows with correlation < half of global
minutes = a.secs / 60
res = {
    "label": a.label,
    "t": round(t0),
    "secs": a.secs,
    "lag_ms": g * 10,
    "global_corr": round(gc, 2),
    "lag_first_ms": int(tr[ok][0, 1]) if ok.any() else None,
    "lag_last_ms": int(tr[ok][-1, 1]) if ok.any() else None,
    "lag_jumps_per_min": round(jumps / minutes, 1),
    "corr_dips_per_min": round(dips / minutes, 1),
    "dir": d,
}
print(
    f"[{a.label}] lag {g * 10} ms (corr {gc:.2f}), lag {res['lag_first_ms']}->{res['lag_last_ms']} ms, "
    f"jumps/min {res['lag_jumps_per_min']}, dips/min {res['corr_dips_per_min']}"
)
print(json.dumps(res))
