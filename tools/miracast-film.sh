#!/usr/bin/env bash
# miracast-film: switches between "desk" mode (picture without delay) and "film" mode (picture held back,
# in sync with the sound).
#
# The TV ignores the picture PTS and keeps ~100 ms, while it buffers audio for at least ~0.45 s (sinks/ Q6, Q10).
# In film mode wfd-tx sends every frame delayed by (audio delay - video delay).
#
# Usage: miracast-film [toggle|on|off|status|MS]
#   on      measure with the microphone (audiolat.py, ~6 s of beeps on the TV), then switch on
#   MS      switch on with a given video delay in ms, no measurement (0 = desk)
#   toggle  on or off depending on the state (for a Sway key binding)
# Variables: VIDEO_MS (video delay, default 100, measured on 2026-11-08),
#            FALLBACK_MS (when the measurement fails: the last good value, else 350).
# The measurement needs audiolat.py (numpy) next to the script or in the lib directory; without it the
# fallback value is used.
set -euo pipefail

SELF="$(readlink -f -- "${BASH_SOURCE[0]}")"
MIR_BIN="$(dirname -- "$SELF")"
MIR_LIB="$MIR_BIN/lib"
[[ -d "$MIR_LIB" ]] || MIR_LIB="$MIR_BIN/../lib"
# shellcheck source=lib/miracast.sh
. "$MIR_LIB/miracast.sh"
# shellcheck source=lib/i18n.sh
. "$MIR_LIB/i18n.sh"

dir="$MIR_RUN"
mkdir -p "$dir"
hold="$dir/video-hold-ms"
last="$dir/film-hold-last"
video_ms="${VIDEO_MS:-100}"

note() {
    echo "$1"
    mir_notify miracast-film "Miracast" "$1" -t 4000
}

# current: video delay in ms from the file; missing file or junk = 0
current() {
    local v
    v="$(cat "$hold" 2>/dev/null || true)"
    [[ "$v" =~ ^[0-9]+$ ]] || v=0
    echo "$v"
}

apply() {
    local ms="$1" before
    before="$(current)"
    before="${before:-0}"
    mir_atomic_write "$hold" "$ms" || { note "$(_ fm.write_failed "$hold")"; exit 1; }
    if ! pkill -HUP -x wfd-tx; then
        # no session: nobody will read the change; keep the file as it was (except "desk", always true)
        [ "$ms" = 0 ] || mir_atomic_write "$hold" "$before" || true
        note "$(_ fm.no_session)"
        exit 1
    fi
    # shorter hold: wfd-tx drops the backlog and waits for a keyframe
    if [ "$ms" -lt "$before" ]; then
        pkill -USR1 -x wfd-media || note "$(_ fm.no_sigusr1)"
    fi
}

measure() {
    # median audio delay (ms) from audiolat.py; empty when it failed or the tool is absent
    local tool=""
    [ -x "$MIR_BIN/audiolat.py" ] && tool="$MIR_BIN/audiolat.py"
    [ -z "$tool" ] && [ -x "$MIR_LIB/audiolat.py" ] && tool="$MIR_LIB/audiolat.py"
    [ -n "$tool" ] || return 0
    "$tool" --bursts 12 --period 0.5 --coded 3 2>/dev/null |
        sed -n 's/^jitter: //p' | python3 -c 'import json,sys; print(round(json.load(sys.stdin)["median"]))' 2>/dev/null || true
}

off() {
    apply 0
    note "$(_ fm.off)"
}

on() {
    # the measurement takes ~6 s and occupies the microphone and the TV: a second one would spoil both
    exec 9>"$dir/film-mode.lock"
    flock -n 9 || { note "$(_ fm.running)"; exit 0; }
    note "$(_ fm.measuring)"
    local audio ms
    audio="$(measure)"
    if [ -n "$audio" ]; then
        ms=$((audio - video_ms))
        [ "$ms" -lt 0 ] && ms=0
        [ "$ms" -gt 2000 ] && ms=2000
        mir_atomic_write "$last" "$ms" || true
        apply "$ms"
        note "$(_ fm.on "$ms" "$audio" "$video_ms")"
    else
        ms="$(cat "$last" 2>/dev/null || echo "${FALLBACK_MS:-350}")"
        apply "$ms"
        note "$(_ fm.failed "$ms")"
    fi
}

case "${1:-toggle}" in
on) on ;;
off) off ;;
toggle) if [ "$(current)" = 0 ]; then on; else off; fi ;;
status) _ fm.status "$(current)"; echo ;;
'' | *[!0-9]*)
    _ fm.usage "${0##*/}" >&2; echo >&2
    exit 2
    ;;
*)
    apply "$1"
    note "$(_ fm.video "$1")"
    ;;
esac
