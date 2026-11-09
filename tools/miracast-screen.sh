#!/usr/bin/env bash
# miracast-screen: what the TV shows - a copy of the laptop screen (mirror) or a separate Sway screen (extend),
# like a projector.
#
# extend: a virtual Sway output (HEADLESS-N, in the mode negotiated with the TV) to the right of the laptop,
#         with its own workspace; move windows as to a second monitor (e.g. `move container to output right`).
# mirror: the TV shows the laptop again; the virtual output goes away, its windows return to the laptop.
# Switching restarts only the encoder (SIGUSR2 to wfd-source), the session with the TV stays.
# After the session ends wfd-source removes the virtual output itself.
#
# Mirror or extend is decided by the capture-output file alone: HEADLESS-* = extend, anything else = mirror.
#
# Usage: miracast-screen [toggle|mirror|extend|status]
set -euo pipefail

SELF="$(readlink -f -- "${BASH_SOURCE[0]}")"
MIR_BIN="$(dirname -- "$SELF")"
MIR_LIB="$MIR_BIN/lib"
[[ -d "$MIR_LIB" ]] || MIR_LIB="$MIR_BIN/../lib"
# shellcheck source=lib/miracast.sh
. "$MIR_LIB/miracast.sh"
# shellcheck source=lib/i18n.sh
. "$MIR_LIB/i18n.sh"

mkdir -p "$MIR_RUN"
file="$MIR_RUN/capture-output"

note() {
    echo "$1"
    mir_notify miracast-screen "$(_ sc.title)" "$1" -t 4000
}

# the laptop's output (MONITOR, else the internal panel, else the first output)
laptop="$(mir_laptop_output)"
if [ -z "$laptop" ] && [ "${1:-toggle}" != status ]; then
    note "$(_ sc.no_sway)"
    exit 1
fi
mir_virt_mode
virt_w="$MIR_VW" virt_h="$MIR_VH" virt_fps="$MIR_VFPS"

# headless: name of the virtual Sway output (empty when there is none)
headless() {
    swaymsg -t get_outputs -r | python3 -c 'import json,sys; print(next((o["name"] for o in json.load(sys.stdin) if o["name"].startswith("HEADLESS-")), ""))'
}

# laptop_size: "width height" of the laptop output from Sway (rect); 1920 1080 when that fails
laptop_size() {
    local out
    out="$(swaymsg -t get_outputs -r 2>/dev/null | python3 -c '
import json, sys
name = sys.argv[1]
for o in json.load(sys.stdin):
    if o["name"] == name:
        print(o["rect"]["width"], o["rect"]["height"])
        break
' "$laptop" 2>/dev/null || true)"
    echo "${out:-1920 1080}"
}

# what is written in the file now; mirror mode is "auto" (or the MONITOR name) when the file is missing
current() { cat "$file" 2>/dev/null || echo "${MONITOR:-auto}"; }

need_session() {
    if [ ! -f "$file" ] || ! pgrep -x wfd-tx >/dev/null; then
        note "$(_ sc.no_session)"
        exit 1
    fi
}

pointer_region() {
    swaymsg "input type:pointer map_to_region 0 0 $1 $2" >/dev/null || true
    swaymsg "input type:touchpad map_to_region 0 0 $1 $2" >/dev/null || true
}

# switch_capture OUTPUT: writes the output to capture and asks wfd-source to restart the encoder.
# The file changes only after checking that wfd-source runs; if the signal fails the old value comes back.
switch_capture() {
    local old
    old="$(current)"
    if ! pgrep -x wfd-source >/dev/null; then
        note "$(_ sc.no_source)"
        return 1
    fi
    mir_atomic_write "$file" "$1" || { note "$(_ sc.write_failed "$file")"; return 1; }
    if ! pkill -USR2 -x wfd-source; then
        mir_atomic_write "$file" "$old" || true
        note "$(_ sc.signal_failed "$old")"
        return 1
    fi
}

extend() {
    need_session
    local h created=0 lw lh ws
    h="$(headless)"
    if [ -z "$h" ]; then
        swaymsg create_output >/dev/null
        created=1
        h="$(headless)"
    fi
    [ -n "$h" ] || { note "$(_ sc.create_failed)"; exit 1; }
    read -r lw lh <<<"$(laptop_size)"
    swaymsg "output $laptop pos 0 0" >/dev/null
    swaymsg "output $h mode ${virt_w}x${virt_h}@${virt_fps}Hz pos $lw 0" >/dev/null
    if ! switch_capture "$h"; then
        # do not leave a virtual output we added ourselves that nobody captures
        [ "$created" = 1 ] && swaymsg "output $h unplug" >/dev/null || true
        exit 1
    fi
    pointer_region $((lw + virt_w)) $((lh > virt_h ? lh : virt_h))
    ws="$(swaymsg -t get_workspaces -r | python3 -c 'import json,sys; print(next((w["name"] for w in json.load(sys.stdin) if w["output"]==sys.argv[1]), "?"))' "$h")"
    note "$(_ sc.extended "$h" "$ws")"
}

mirror() {
    need_session
    local h lw lh
    h="$(headless)"
    switch_capture "${MONITOR:-auto}" || exit 1
    if [ -n "$h" ]; then
        # after the encoder switched: windows from the virtual screen return to the laptop
        sleep 0.3
        swaymsg "output $h unplug" >/dev/null || true
    fi
    read -r lw lh <<<"$(laptop_size)"
    pointer_region "$lw" "$lh"
    note "$(_ sc.mirrored)"
}

case "${1:-toggle}" in
extend) extend ;;
mirror) mirror ;;
toggle) if [[ "$(current)" == HEADLESS-* ]]; then mirror; else extend; fi ;;
status) _ sc.shows "$(current)"; echo ;;
*)
    _ sc.usage "${0##*/}" >&2; echo >&2
    exit 2
    ;;
esac
