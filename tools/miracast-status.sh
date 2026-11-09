#!/usr/bin/env bash
# miracast-status: Miracast state as Waybar JSON (custom/miracast, return-type json); a click opens miracast-menu.
# Classes: off (service stopped), waiting (waits for the TV / connecting), casting.
#   miracast-status                  print the Waybar JSON
#   miracast-status --notify-waiting send the "turn on Smart View on the TV" notification (wfd-source --notify-cmd)
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

case "${1:-}" in
	"") ;;
	--notify-waiting)
		mir_notify miracast "Miracast" "$(_ st.notify_waiting)"
		exit 0 ;;
	*)
		_ st.usage "${0##*/}" >&2; echo >&2
		exit 2 ;;
esac

# json_out TEXT TOOLTIP CLASS - one JSON object per line; jq, else python3, else manual quoting.
json_out() {
	if command -v jq >/dev/null 2>&1; then
		jq -cn --arg text "$1" --arg tooltip "$2" --arg class "$3" '{text: $text, tooltip: $tooltip, class: $class}'
	elif command -v python3 >/dev/null 2>&1; then
		python3 -c 'import json, sys; print(json.dumps(dict(zip(("text", "tooltip", "class"), sys.argv[1:])), ensure_ascii=False))' "$1" "$2" "$3"
	else
		local t="${2//\\/\\\\}"
		t="${t//\"/\\\"}"
		printf '{"text":"%s","tooltip":"%s","class":"%s"}\n' "$1" "$t" "$3"
	fi
}

mac="$(mir_current_mac)"
name="$(mir_known_name "$mac")"
name="${name:-${mac:-$(_ st.none)}}"

mir_unit_state
active="$MIR_ACTIVE" status="$MIR_STATUS"

if [[ "$active" != active && "$active" != activating ]]; then
	text="$BAR_OFF"; class=off
	tip="$(_ st.off)"
elif [[ "$MIR_CASTING" == yes ]]; then
	text="$BAR_CASTING"; class=casting
	mode="$(_ st.mode_dup)"
	mir_is_mirror || mode="$(_ st.mode_ext)"
	tip="$(_ st.casting "$name" "${status#casting }" "$mode")"
else
	text="$BAR_WAITING"; class=waiting
	if [[ -z "$mac" ]]; then
		tip="$(_ st.no_tv)"
	else
		tip="$(_ st.waiting "$name" "${status:-$(_ st.start)}")"
	fi
fi

json_out "$text" "$tip" "$class"
