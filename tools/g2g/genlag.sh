#!/usr/bin/env bash
# genlag.sh OUTDIR [FONT] [SINK_MAC]: how far behind its log is the stripes.py picture? (headless output, grim screenshots, no camera)
set -u
HERE=$(cd -P "$(dirname "$0")" && pwd); OUT=${1:?outdir}; FONT=${2:-20}; MAC=${3:-}; mkdir -p "$OUT"; rm -f "$OUT"/*
FOCUS=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(next(o["name"] for o in json.load(sys.stdin) if o.get("focused")))')
before=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(" ".join(o["name"] for o in json.load(sys.stdin)))')
swaymsg create_output >/dev/null
HL=$(swaymsg -t get_outputs | python3 -c 'import json,sys; b=set(sys.argv[1].split()); print(next(o["name"] for o in json.load(sys.stdin) if o["name"] not in b))' "$before")
swaymsg output "$HL" mode 1280x720@30Hz pos 10000 0 >/dev/null
swaymsg "for_window [app_id=\"g2glag\"] move container to output $HL, focus output $FOCUS" >/dev/null
kitty --class g2glag -o background_opacity=1.0 -o font_size="$FONT" "$HERE/stripes.py" "$OUT/stripes.log" 120 &
KP=$!
sleep 1.5; swaymsg "[app_id=\"g2glag\"] fullscreen enable" >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
if [ -n "$MAC" ]; then
  systemctl --user stop sway-miracast-source
  ~/.local/bin/wfd-source --sink "$MAC" --output "$HL" --mode 1280x720p30 --kbps 2000 --no-audio > "$OUT/src.log" 2>&1 &
  SRC=$!
  for _ in $(seq 1 40); do grep -q '"sink_connected"' "$OUT/src.log" && break; sleep 1; done
fi
sleep 10
for _ in $(seq 1 40); do t=$(date +%s.%N); grim -o "$HL" "$OUT/s_$t.png"; sleep 0.7; done
kill "$KP" 2>/dev/null
[ -n "${SRC:-}" ] && { kill -INT "$SRC"; wait "$SRC" 2>/dev/null; systemctl --user start sway-miracast-source; }
swaymsg output "$HL" unplug >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
python3 - "$OUT" <<'EOF'
import glob, os, sys, subprocess
import numpy as np
out = sys.argv[1]
draws = {}
for line in open(f"{out}/stripes.log"):
    f = line.split()
    if f[0] != "cal":
        draws.setdefault(int(f[0]), []).append(float(f[1]))
lags = []
for p in sorted(glob.glob(f"{out}/s_*.png")):
    t = float(os.path.basename(p)[2:-4])
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", p, "-vf", "format=gray", "-f", "rawvideo", "-"], capture_output=True).stdout
    img = np.frombuffer(raw, np.uint8).reshape(720, 1280)
    row = img[360].astype(int)
    v = [row[int((i + 0.5) * 1280 / 14)] for i in range(14)]
    g = 0
    for x in v[1:-1]:
        g = (g << 1) | (1 if x > 128 else 0)
    c, s = g, g >> 1
    while s:
        c ^= s; s >>= 1
    prev = [d for d in draws.get(c, []) if d < t]
    lags.append((t - prev[-1]) * 1000 if prev else float("nan"))
print("generator lag ms:", " ".join(f"{x:.0f}" for x in lags))
src = f"{out}/src.log"
if os.path.exists(src):
    print("sink connected:", '"sink_connected"' in open(src).read())
EOF
