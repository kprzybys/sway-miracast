#!/usr/bin/env python3
"""latency_track: audio latency source -> TV speaker over time (tools/audiolat.py every N seconds), plus the
analysis that tells the two TV mechanisms apart (sinks/ Q11): steps every ~30 s (sink-side compensation) vs
rare steps with a slope between them (TV DAC clock drift against our PTS), and whether steps line up with
radio trouble in the sink's RTCP reports ($XDG_RUNTIME_DIR/sway-miracast/rtcp.jsonl, wfd-tx --rtcp-log).

Usage: latency_track.py OUT.jsonl [--minutes 15] [--every 10] [--analyze-only]
"""

import argparse
import json
import os
import re
import subprocess
import sys
import tempfile
import time

ap = argparse.ArgumentParser()
ap.add_argument("out")
ap.add_argument("--minutes", type=float, default=15)
ap.add_argument("--every", type=float, default=10)
ap.add_argument("--analyze-only", action="store_true")
ap.add_argument("--rtcp", default=os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "sway-miracast/rtcp.jsonl"))
a = ap.parse_args()
here = os.path.dirname(os.path.realpath(__file__))
LINE = re.compile(r"heard (\d+); latency ms: median (\d+), min (\d+), max (\d+)")


def measure(keep):
    r = subprocess.run(
        [sys.executable, f"{here}/audiolat.py", "--bursts", "4", "--period", "0.5", "--coded", "2", "--keep", keep],
        capture_output=True,
        text=True,
    )
    m = LINE.search(r.stdout)
    if not m:
        return None
    heard, med, lo, hi = map(int, m.groups())
    return {"heard": heard, "median": med, "min": lo, "max": hi}


if not a.analyze_only:
    keep = tempfile.mkdtemp(prefix="latrack-")
    end = time.time() + a.minutes * 60
    with open(a.out, "a", buffering=1) as f:
        while time.time() < end:
            t0 = time.time()
            m = measure(keep)
            rec = {"t": round(t0, 2), **(m or {"error": "no match"})}
            f.write(json.dumps(rec) + "\n")
            print(json.dumps(rec), flush=True)
            time.sleep(max(0.0, a.every - (time.time() - t0)))

# --- analysis ---
pts = [json.loads(x) for x in open(a.out)]
pts = [p for p in pts if "median" in p]
if len(pts) < 3:
    sys.exit("too few points")
t0 = pts[0]["t"]
steps = []
seg_start = 0
segments = []
for i in range(1, len(pts)):
    d = pts[i]["median"] - pts[i - 1]["median"]
    if abs(d) >= 10:
        steps.append((round(pts[i]["t"] - t0), d))
        segments.append(pts[seg_start:i])
        seg_start = i
segments.append(pts[seg_start:])
print(
    f"points {len(pts)} over {(pts[-1]['t'] - t0) / 60:.1f} min; latency {pts[0]['median']} -> {pts[-1]['median']} ms"
)
print(f"steps >=10 ms: {len(steps)} at s/ms: {steps}")
if len(steps) >= 2:
    gaps = [steps[i][0] - steps[i - 1][0] for i in range(1, len(steps))]
    print(f"gaps between steps (s): {gaps}")
slopes = []
for s in segments:
    if len(s) >= 6 and s[-1]["t"] - s[0]["t"] >= 60:
        n = len(s)
        xs = [p["t"] for p in s]
        ys = [p["median"] for p in s]
        mx, my = sum(xs) / n, sum(ys) / n
        k = sum((x - mx) * (y - my) for x, y in zip(xs, ys, strict=True)) / sum((x - mx) ** 2 for x in xs)
        slopes.append(k)
if slopes:
    print(
        "slopes between steps (ms/min):",
        [round(k * 60, 2) for k in slopes],
        " ~ppm:",
        [round(k * 1000) for k in slopes],
    )
total = (pts[-1]["median"] - pts[0]["median"]) / (pts[-1]["t"] - t0)
print(f"overall drift: {total * 60:.2f} ms/min (~{total * 1000:.0f} ppm)")
# radio: RTCP reports around each step
try:
    rr = [json.loads(x) for x in open(a.rtcp)]
except OSError:
    rr = []
if rr:
    span = [r for r in rr if t0 * 1e6 <= r["t"] <= pts[-1]["t"] * 1e6]
    if span:
        js = sorted(r["jitter_ms"] for r in span)
        print(
            f"RTCP reports {len(span)}: jitter median {js[len(js) // 2]:.2f} ms, max {js[-1]:.2f} ms; "
            f"loss reports {sum(r['lost_frac'] > 0 for r in span)}"
        )
        for ts, d in steps:
            w = [r for r in span if abs(r["t"] / 1e6 - (t0 + ts)) <= 15]
            if w:
                print(
                    f"  step at {ts}s ({d:+} ms): jitter max {max(r['jitter_ms'] for r in w):.2f} ms, "
                    f"loss {sum(r['lost_frac'] > 0 for r in w)} reports in +-15 s"
                )
