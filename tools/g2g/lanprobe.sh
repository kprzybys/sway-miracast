#!/usr/bin/env bash
# lanprobe.sh OUTDIR [KBPS] [WARM] [MEAS]: same source and sink pipeline as a P2P session, but over the existing LAN.
# env: SINK_SSH (ssh host of the sink), LAPTOP_IP (this machine's LAN address), SINK_DIR (where sink_ctl.py is on the sink).
# The laptop firewall must accept TCP 17236 from the sink for the test.
set -u
OUT=${1:?}; KBPS=${2:-2000}; WARM=${3:-30}; MEAS=${4:-40}
HERE=$(cd -P "$(dirname "$0")" && pwd); mkdir -p "$OUT"; rm -f "$OUT"/*
SINK_SSH=${SINK_SSH:?ssh host of the sink}; LAPTOP_IP=${LAPTOP_IP:?LAN address of this machine}; SINK_DIR=${SINK_DIR:-.}
FOCUS=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(next(o["name"] for o in json.load(sys.stdin) if o.get("focused")))')
before=$(swaymsg -t get_outputs | python3 -c 'import json,sys; print(" ".join(o["name"] for o in json.load(sys.stdin)))')
swaymsg create_output >/dev/null
HL=$(swaymsg -t get_outputs | python3 -c 'import json,sys; b=set(sys.argv[1].split()); print(next(o["name"] for o in json.load(sys.stdin) if o["name"] not in b))' "$before")
swaymsg output "$HL" mode 1280x720@30Hz pos 10000 0 >/dev/null
swaymsg "for_window [app_id=\"g2glan\"] move container to output $HL, focus output $FOCUS" >/dev/null
RT=$(mktemp -d); mkdir -p "$RT/sway-miracast"
cleanup(){ kill -INT "${SRC:-}" 2>/dev/null; kill "${KP:-}" "${CAMP:-}" 2>/dev/null; ssh -o BatchMode=yes "$SINK_SSH" 'pkill -f "^gst-launch-1.0 -q udpsrc port=19002"; pkill -f sink_ctl.py' 2>/dev/null; swaymsg output "$HL" unplug >/dev/null 2>&1; swaymsg focus output "$FOCUS" >/dev/null 2>&1; rm -rf "$RT"; }
trap cleanup EXIT
env XDG_RUNTIME_DIR="$RT" WAYLAND_DISPLAY="$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" WFD_RTSP_PORT=17236 \
  ~/.local/bin/wfd-source --no-p2p --output "$HL" --mode 1280x720p30 --kbps "$KBPS" --no-audio > "$OUT/src.log" 2>&1 &
SRC=$!
ssh -o BatchMode=yes "$SINK_SSH" 'setsid nohup gst-launch-1.0 -q udpsrc port=19002 buffer-size=1048576 skip-first-bytes=12 caps=video/mpegts,systemstream=true,packetsize=188 '\''!'\'' tsdemux name=d d. '\''!'\'' video/x-h264 '\''!'\'' h264parse '\''!'\'' v4l2h264dec '\''!'\'' kmssink sync=false d. '\''!'\'' audio/x-private-ts-lpcm '\''!'\'' fakesink sync=false async=false > /tmp/lanpipe.log 2>&1 < /dev/null &'
sleep 8
ssh -o BatchMode=yes "$SINK_SSH" "RTSP_PORT=17236 timeout $((WARM + MEAS + 25)) python3 $SINK_DIR/sink_ctl.py $LAPTOP_IP $((WARM + MEAS + 20)) > /tmp/sinkctl.json 2>&1" &
for _ in $(seq 1 30); do grep -q '"sink_connected"' "$OUT/src.log" && break; sleep 1; done
grep -q '"sink_connected"' "$OUT/src.log" || { echo "no session"; exit 4; }
ffmpeg -nostdin -hide_banner -loglevel error -f v4l2 -input_format mjpeg -video_size 1280x720 -framerate 30 \
  -use_wallclock_as_timestamps 1 -i "${CAM:-/dev/video2}" -c:v copy -copyts -y -f nut "$OUT/cam.nut" &
CAMP=$!
sleep 1.5
kitty --class g2glan -o background_opacity=1.0 -o font_size=20 "$HERE/stripes.py" "$OUT/stripes.log" $((WARM + MEAS)) &
KP=$!
sleep 1.5; swaymsg "[app_id=\"g2glan\"] fullscreen enable" >/dev/null; swaymsg focus output "$FOCUS" >/dev/null
sleep $((WARM + MEAS + 4))
kill -INT "$CAMP"; wait "$CAMP" 2>/dev/null; CAMP=""
python3 "$HERE/g2g_code.py" "$OUT/cam.nut" "$OUT/stripes.log" --skip "$WARM"
