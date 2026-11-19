#!/usr/bin/env bash
# miracast-status: Miracast state as Waybar JSON (custom/miracast, return-type json); a click opens miracast-menu.
# Classes: off (service stopped), waiting (waits for the TV / connecting), casting; plus film (film mode on,
# then "class" is a list, e.g. ["casting", "film"]).
#   miracast-status                print the Waybar JSON
#   miracast-status --notify-event wfd-source --notify-cmd: redraw the bar and show the notification for WFD_EVENT
#                                  (waiting | connecting | casting | failed | ended; WFD_QUIET=1: bar only)
#   --notify-waiting               the old name of --notify-event (without WFD_EVENT: waiting)
#
# Deliberately NO `set -e`: Waybar calls this every few seconds and must always get valid JSON, even when
# systemctl, the state file or awk fail (the fields are then empty; the script stays alive).
set -uo pipefail

SELF="$(readlink -f -- "${BASH_SOURCE[0]}")"
MIR_BIN="$(dirname -- "$SELF")"
MIR_LIB="$MIR_BIN/lib"
[[ -d "$MIR_LIB" ]] || MIR_LIB="$MIR_BIN/../lib"
# shellcheck source=lib/miracast.sh
. "$MIR_LIB/miracast.sh"
# shellcheck source=lib/i18n.sh
. "$MIR_LIB/i18n.sh"

# notify_event - one notification per wfd-source state change; the tag "miracast" is shared with miracast-menu,
# so "Connecting..." gives way to "Casting to ..." instead of piling up.
notify_event() {
	local ev="${WFD_EVENT:-waiting}" mac name
	mir_bar_refresh
	[[ "${WFD_QUIET:-0}" == 1 ]] && return 0
	mac="${WFD_SINK:-$(mir_current_mac)}"
	name="$(mir_known_name "$mac")"
	name="${name:-$(_ st.tv)}"
	case "$ev" in
		waiting) mir_notify miracast "Miracast" "$(_ st.notify_waiting "$name")" ;;
		connecting) mir_notify miracast "Miracast" "$(_ st.notify_connecting "$name")" -e ;;
		casting) mir_notify miracast "Miracast" "$(_ st.notify_casting "$name")" -t 4000 ;;
		failed) mir_notify miracast "Miracast" "$(_ st.notify_failed "$name")" -u critical ;;
		ended) mir_notify miracast "Miracast" "$(_ st.notify_ended "$name")" ;;
	esac
}

case "${1:-}" in
	"") ;;
	--notify-event | --notify-waiting)
		notify_event
		exit 0 ;;
	*)
		_ st.usage "${0##*/}" >&2; echo >&2
		exit 2 ;;
esac

# json_out TEXT TOOLTIP CLASS... - one JSON object per line; one class as a string, several as a list (Waybar
# takes both). jq, else python3, else manual quoting.
json_out() {
	local text="$1" tip="$2"
	shift 2
	if command -v jq >/dev/null 2>&1; then
		jq -cn --arg text "$text" --arg tooltip "$tip" '{text: $text, tooltip: $tooltip,
			class: ($ARGS.positional | if length == 1 then .[0] else . end)}' --args "$@"
	elif command -v python3 >/dev/null 2>&1; then
		python3 -c 'import json, sys
c = sys.argv[3:]
print(json.dumps({"text": sys.argv[1], "tooltip": sys.argv[2], "class": c[0] if len(c) == 1 else c}, ensure_ascii=False))' \
			"$text" "$tip" "$@"
	else
		local t="${tip//\\/\\\\}" c
		t="${t//\"/\\\"}"
		if (($# == 1)); then
			c="\"$1\""
		else
			c="$(printf '"%s",' "$@")"
			c="[${c%,}]"
		fi
		printf '{"text":"%s","tooltip":"%s","class":%s}\n' "$text" "$t" "$c"
	fi
}

# status_words STATUS_TEXT - what wfd-source is doing (its English systemd StatusText) in the user's language;
# an unknown text gets a general phrase, never the raw English.
status_words() {
	case "$1" in
		"") _ st.s_start ;;
		"waiting for the TV"*) _ st.s_waiting ;;
		connecting*) _ st.s_connecting ;;
		"waiting for RTSP"*) _ st.s_rtsp ;;
		"no TV selected"*) _ st.s_no_tv ;;
		*) _ st.s_other ;;
	esac
}

mac="$(mir_current_mac)"
name="$(mir_known_name "$mac")"
name="${name:-${mac:-$(_ st.none)}}"

mir_unit_state
active="$MIR_ACTIVE" status="$MIR_STATUS"

if [[ "$active" != active && "$active" != activating ]]; then
	text="$BAR_OFF"; classes=(off)
	tip="$(_ st.off)"
elif [[ "$MIR_CASTING" == yes ]]; then
	text="$BAR_CASTING"; classes=(casting)
	mode="$(_ st.mode_dup)"
	mir_is_mirror || mode="$(_ st.mode_ext)"
	film="" film_ms="$(mir_film_ms)"
	if ((film_ms > 0)); then
		film="$(_ st.film "$film_ms")"
		classes+=(film)
	fi
	tip="$(_ st.casting "$name" "${status#casting }" "$mode" "$film")"
else
	text="$BAR_WAITING"; classes=(waiting)
	if [[ -z "$mac" ]]; then
		tip="$(_ st.no_tv)"
	else
		tip="$(_ st.waiting "$name" "$(status_words "$status")")"
	fi
fi

json_out "$text" "$tip" "${classes[@]}"
