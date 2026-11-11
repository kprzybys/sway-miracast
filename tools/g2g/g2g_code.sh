#!/usr/bin/env bash
# g2g_code.sh OUTDIR SINK_MAC [KBPS] [WARMUP_S] [MEASURE_S]
# Casts a headless sway output (not the laptop screen) to the sink, shows stripes.py on it, the USB webcam
# ($CAM, default /dev/video2) records the sink's screen. Restores the user's source service at the end.
set -u
OUT=${1:?outdir}; MAC=${2:?sink mac}; KBPS=${3:-2000}; WARM=${4:-30}; MEAS=${5:-40}
HERE=$(cd -P "$(dirname "$0")" && pwd)
mkdir -p "$OUT"; rm -f "$OUT"/cam.nut "$OUT"/stripes.log
FOCUS=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(next(o["name"] for o in json.load(sys.stdin) if o.get("focused")))')
before=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(" ".join(o["name"] for o in json.load(sys.stdin)))')
swaymsg create_output >/dev/null
HL=$(swaymsg -t get_outputs | python3 -c 'import json,sys; b=set(sys.argv[1].split()); print(next(o["name"] for o in json.load(sys.stdin) if o["name"] not in b))' "$before")
swaymsg output "$HL" mode 1280x720@30Hz pos 10000 0 >/dev/null
swaymsg "for_window [app_id=\"g2gstripes\"] move container to output $HL, focus output $FOCUS" >/dev/null
cleanup() {
  [ -n "${SRC:-}" ] && kill -INT "$SRC" 2>/dev/null && wait "$SRC" 2>/dev/null
  [ -n "${KP:-}" ] && kill "$KP" 2>/dev/null
  [ -n "${CAMP:-}" ] && kill -INT "$CAMP" 2>/dev/null
  swaymsg output "$HL" unplug >/dev/null 2>&1
  swaymsg focus output "$FOCUS" >/dev/null 2>&1
  systemctl --user start sway-miracast-source
}
trap cleanup EXIT
kitty --class g2gstripes -o background_opacity=1.0 -o font_size=20 "$HERE/stripes.py" "$OUT/stripes.log" $((WARM + MEAS + 10)) &
KP=$!
sleep 2; swaymsg "[app_id=\"g2gstripes\"] fullscreen enable" >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
systemctl --user stop sway-miracast-source
~/.local/bin/wfd-source --sink "$MAC" --output "$HL" --mode 1280x720p30 --kbps "$KBPS" --no-audio > "$OUT/src.log" 2>&1 &
SRC=$!
for _ in $(seq 1 60); do grep -q '"sink_connected"' "$OUT/src.log" && break; sleep 2; done
grep -q '"sink_connected"' "$OUT/src.log" || { echo "no session"; exit 4; }
# restart the code (with its calibration phase) once the sink shows our output
kill "$KP"
ffmpeg -nostdin -hide_banner -loglevel error -f v4l2 -input_format mjpeg -video_size 1280x720 -framerate 30 \
  -use_wallclock_as_timestamps 1 -i "${CAM:-/dev/video2}" -c:v copy -copyts -y -f nut "$OUT/cam.nut" &
CAMP=$!
sleep 1.5
kitty --class g2gstripes -o background_opacity=1.0 -o font_size=20 "$HERE/stripes.py" "$OUT/stripes.log" $((WARM + MEAS)) &
KP=$!
sleep 1.5; swaymsg "[app_id=\"g2gstripes\"] fullscreen enable" >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
sleep $((WARM + MEAS + 4))
kill -INT "$CAMP"; wait "$CAMP" 2>/dev/null; CAMP=""
python3 "$HERE/g2g_code.py" "$OUT/cam.nut" "$OUT/stripes.log" --skip "$WARM"
