#!/usr/bin/env python3
"""g2g_code: glass-to-glass latency from a webcam recording of a stripes.py time code on the sink.

1. calibration: mean camera frame during the white phase minus during the black phase -> screen mask -> bbox;
2. per camera frame: 14 equal columns of the bbox (middle 50% of each column, middle 60% of the height),
   threshold = midpoint of the white and black reference bars; reject frames with a bar near the threshold
   (mid-transition) or low contrast; Gray -> counter;
3. latency = camera wallclock - wallclock when stripes.py drew that counter (latest draw before the camera frame).
Usage: g2g_code.py cam.nut stripes.log [--skip SECONDS]
"""

import subprocess
import sys

import numpy as np

cam, logf = sys.argv[1], sys.argv[2]
skip = float(sys.argv[sys.argv.index("--skip") + 1]) if "--skip" in sys.argv else 0.0
W, H, NB = 320, 180, 12
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

cal, draws = {}, {}
for line in open(logf):
    f = line.split()
    if f[0] == "cal":
        cal.setdefault(f[1], float(f[2]))
    else:
        draws.setdefault(int(f[0]), []).append(float(f[1]))
codes_start = min(t for ts in draws.values() for t in ts)


# calibration: alternating bars (A) and inverse (B). Split the calibration frames into A and B by the first
# principal component of their column profiles, then bar boundaries = sign changes of the A-B profile.
sel = np.nonzero((pts > cal["A"]) & (pts < codes_start + 2.0))[0]
if len(sel) < 20:
    sys.exit("calibration phases not in the recording")
prof = fr[sel].mean(1)
X = prof - prof.mean(0)
v1 = np.linalg.svd(X, full_matrices=False)[2][0]
score = X @ v1
order = sel[np.argsort(score)]
k = max(5, len(order) // 5)
pa, pb = fr[order[-k:]].mean(0), fr[order[:k]].mean(0)
d = (pa - pb).mean(0)
d = np.convolve(d, np.ones(3) / 3, mode="same")
# fit the whole pattern at once: NB+2 equal bars of width w starting at x0, signs +,-,+,... (or the inverse).
# Score = sum of d over each bar * its sign, normalised by width; grid search (robust to weak bars under glare).
dn = d - np.median(d)
cs = np.concatenate([[0.0], np.cumsum(dn)])
best_fit = (-1e18, 0, 0.0)
for w in np.arange(W / 30, W / (NB + 2) + 0.01, 0.25):
    for x0f in np.arange(0, W - w * (NB + 2), 0.5):
        e = x0f + w * np.arange(NB + 3)
        ei = np.clip(np.round(e).astype(int), 0, W)
        seg = cs[ei[1:]] - cs[ei[:-1]]
        sc = abs(np.sum(seg * np.where(np.arange(NB + 2) % 2 == 0, 1, -1)))
        if sc > best_fit[0]:
            best_fit = (sc, x0f, w)
_, fx0, fw = best_fit
e = fx0 + fw * np.arange(NB + 3)
signs = np.where(np.arange(NB + 2) % 2 == 0, 1, -1)
seg = np.array([dn[int(round(e[i])) : int(round(e[i + 1]))].sum() for i in range(NB + 2)])
first_sign = 1 if np.sum(seg * signs) > 0 else -1
best = [(int(round(e[i])), int(round(e[i + 1])), first_sign * signs[i]) for i in range(NB + 2)]
agree = np.mean(np.sign(seg) == first_sign * signs)
print(f"pattern fit: x0 {fx0:.1f} bar width {fw:.2f} px, bars agreeing in sign {agree * 100:.0f}%")
if agree < 0.8:
    sys.exit("bar pattern not found: camera framing or focus")
if best[0][2] < 0:  # make phase A the one with bar 0 white
    pa, pb = pb, pa
white, black = np.maximum(pa, pb), np.minimum(pa, pb)
cols = [(int(a0 + 0.25 * (a1 - a0)), int(a0 + 0.75 * (a1 - a0)) + 1) for a0, a1, _ in best]
ad = np.abs(pa - pb)[:, best[0][0] : best[-1][1]].mean(1)
ys = np.nonzero(ad > 0.3 * ad.max())[0]
y0, y1 = ys.min(), ys.max()
ya, yb = int(y0 + 0.2 * (y1 - y0)), int(y0 + 0.8 * (y1 - y0)) + 1
x0, x1 = best[0][0], best[-1][1]
print(f"code area x {x0}-{x1} y {y0}-{y1} of {W}x{H}")

# per-column white/black levels from the calibration (glare differs across the screen)
cw = np.array([white[ya:yb, a:b].mean() for a, b in cols])
cb = np.array([black[ya:yb, a:b].mean() for a, b in cols])
cthr = (cw + cb) / 2
cmargin = 0.2 * (cw - cb)
print("column contrast:", " ".join(f"{x:.0f}" for x in cw - cb))
edge = x0 < 3 or y0 < 3 or x1 > W - 4 or y1 > H - 4
if edge:
    print("WARNING: screen touches the camera frame edge, the code may be cut off")
lat, bad = [], 0
for i in range(n):
    if pts[i] < codes_start + skip:
        continue
    v = np.array([fr[i, ya:yb, a:b].mean() for a, b in cols])
    # references must read white / black, every bit clearly on one side of its column threshold
    if not (v[0] > cthr[0] + cmargin[0] and v[-1] < cthr[-1] - cmargin[-1]):
        bad += 1
        continue
    if np.any(np.abs(v[1:-1] - cthr[1:-1]) < cmargin[1:-1]):
        bad += 1
        continue
    g = 0
    for x, t in zip(v[1:-1], cthr[1:-1], strict=True):
        g = (g << 1) | (1 if x > t else 0)
    c, s = g, g >> 1
    while s:
        c ^= s
        s >>= 1
    prev = [t for t in draws.get(c, []) if t < pts[i]]
    if not prev:
        bad += 1
        continue
    lat.append((pts[i], (pts[i] - prev[-1]) * 1000))

print(f"camera frames {n}, decoded {len(lat)}, rejected {bad}")
if lat:
    a = np.array([x for _, x in lat])
    q = np.quantile(a, [0.1, 0.5, 0.9])
    print(
        f"latency ms (incl. camera delay): p10 {q[0]:.0f}  median {q[1]:.0f}  p90 {q[2]:.0f}  min {a.min():.0f}  max {a.max():.0f}"
    )
    t0 = lat[0][0]
    per = {}
    for t, x in lat:
        per.setdefault(int((t - t0) // 5), []).append(x)
    print("median per 5 s:", " ".join(f"{np.median(v):.0f}" for _, v in sorted(per.items())))
