#!/usr/bin/env bash
# srcprobe.sh OUTDIR [KBPS] [SECONDS]: source-only latency. Headless output with stripes.py, wfd-source --no-p2p to a local
# sink_sim (isolated port + runtime dir), TS + per-packet arrival times; analysis decodes the counter per frame.
set -u
HERE=$(cd -P "$(dirname "$0")" && pwd); OUT=${1:?outdir}; KBPS=${2:-2000}; SECS=${3:-40}; mkdir -p "$OUT"; rm -f "$OUT"/*
FOCUS=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(next(o["name"] for o in json.load(sys.stdin) if o.get("focused")))')
before=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(" ".join(o["name"] for o in json.load(sys.stdin)))')
swaymsg create_output >/dev/null
HL=$(swaymsg -t get_outputs | python3 -c 'import json,sys; b=set(sys.argv[1].split()); print(next(o["name"] for o in json.load(sys.stdin) if o["name"] not in b))' "$before")
swaymsg output "$HL" mode 1280x720@30Hz pos 10000 0 >/dev/null
swaymsg "for_window [app_id=\"g2gsrc\"] move container to output $HL, focus output $FOCUS" >/dev/null
kitty --class g2gsrc -o background_opacity=1.0 -o font_size=20 "$HERE/stripes.py" "$OUT/stripes.log" $((SECS + 30)) &
KP=$!
sleep 1.5; swaymsg "[app_id=\"g2gsrc\"] fullscreen enable" >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
RT=$(mktemp -d); mkdir -p "$RT/sway-miracast"
XDG_RUNTIME_DIR_ORIG=$XDG_RUNTIME_DIR
env XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY="$XDG_RUNTIME_DIR_ORIG/$WAYLAND_DISPLAY" WFD_RTSP_PORT=17236 PULSE_SERVER="unix:$XDG_RUNTIME_DIR_ORIG/pulse/native" \
  ~/.local/bin/wfd-source --no-p2p --output "$HL" --mode 1280x720p30 --kbps "$KBPS" --no-audio > "$OUT/src.log" 2>&1 &
SRC=$!
sleep 2
RTSP_PORT=17236 python3 "$HERE/sink_sim_ts.py" 127.0.0.1 "$SECS" "$OUT/rx.ts" > "$OUT/sim.json" 2>&1
kill -INT "$SRC"; wait "$SRC" 2>/dev/null; kill "$KP" 2>/dev/null
swaymsg output "$HL" unplug >/dev/null; swaymsg focus output "$FOCUS" >/dev/null; rm -rf "$RT"
python3 - "$OUT" <<'PY'
import bisect, subprocess, sys
import numpy as np
d = sys.argv[1]
draws = {}
for line in open(f"{d}/stripes.log"):
    f = line.split()
    if f[0] != "cal":
        draws.setdefault(int(f[0]), []).append(float(f[1]))
arr = np.loadtxt(f"{d}/rx.ts.arr")
pos_a, t_a = arr[:, 0], arr[:, 1]
pk = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "packet=pts,pos", "-of", "csv=p=0", f"{d}/rx.ts"], capture_output=True, text=True).stdout.split()
pts2pos = {}
for x in pk:
    a = x.split(",")
    if len(a) >= 2 and a[0] not in ("N/A", "") and a[1] not in ("N/A", ""):
        pts2pos[int(a[0])] = int(a[1])
fr = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v", "-show_entries", "frame=pts", "-of", "csv=p=0", f"{d}/rx.ts"], capture_output=True, text=True).stdout.split()
fpts = [int(x) for x in fr if x not in ("N/A", "")]
raw = subprocess.run(["ffmpeg", "-v", "error", "-i", f"{d}/rx.ts", "-vf", "crop=1280:2:0:359,format=gray", "-f", "rawvideo", "-"], capture_output=True).stdout
rows = np.frombuffer(raw, np.uint8).reshape(-1, 2, 1280)[:, 0, :]
n = min(len(rows), len(fpts))
res = []
for i in range(n):
    v = [int(rows[i][int((k + 0.5) * 1280 / 14)]) for k in range(14)]
    if not (v[0] > 160 and v[-1] < 90) or any(90 <= x <= 160 for x in v[1:-1]):
        continue
    g = 0
    for x in v[1:-1]:
        g = (g << 1) | (1 if x > 128 else 0)
    c, s = g, g >> 1
    while s:
        c ^= s; s >>= 1
    p = pts2pos.get(fpts[i])
    if p is None:
        continue
    j = bisect.bisect_right(pos_a, p) - 1
    if j < 0:
        continue
    t_arr = t_a[j]
    prev = [x for x in draws.get(c, []) if x < t_arr]
    if prev:
        res.append((t_arr, (t_arr - prev[-1]) * 1000, prev[-1]))
a = np.array(res)
t = a[:, 0] - a[0, 0]
k = np.polyfit(t, a[:, 2] - a[0, 2], 1)[0]
q = np.quantile(a[:, 1], [0.1, 0.5, 0.9])
print(f"source only ({len(a)} frames over {t[-1]:.0f} s): draw -> packet received ms p10 {q[0]:.0f} median {q[1]:.0f} p90 {q[2]:.0f} max {a[:,1].max():.0f}; content pace {k:.3f}")
per = {}
for tt, l in zip(t, a[:, 1]):
    per.setdefault(int(tt // 5), []).append(l)
print("median per 5 s:", " ".join(f"{np.median(v):.0f}" for _, v in sorted(per.items())))
PY
