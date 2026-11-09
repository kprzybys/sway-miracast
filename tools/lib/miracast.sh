#!/usr/bin/env bash
# Shared functions of the sway-miracast tools (miracast-menu, -status, -screen, -film, -doctor). Source only:
# sets no shell options (set -e etc. stays with the calling script).
#
# The caller finds its directories through the real path (works from the repo and when installed):
#   SELF="$(readlink -f -- "${BASH_SOURCE[0]}")"; MIR_BIN="$(dirname -- "$SELF")"
#   MIR_LIB="$MIR_BIN/lib"; [[ -d "$MIR_LIB" ]] || MIR_LIB="$MIR_BIN/../lib"
#   . "$MIR_LIB/miracast.sh"
#
# shellcheck disable=SC2034  # MIR_* variables are read by the scripts that source this file
#
# Variables: MIR_UNIT, MIR_ENV, MIR_STATE (persistent: sink, known), MIR_RUN (in $XDG_RUNTIME_DIR:
# capture-output, mode, video-hold-ms, dev.lock), MONITOR (laptop output ONLY if set by the user or the
# env file; empty = automatic, see mir_laptop_output), MIRACAST_ICONS (nerd | plain), MIRACAST_MENU.

MIR_UNIT=sway-miracast-source.service
MIR_ENV="${XDG_CONFIG_HOME:-$HOME/.config}/sway-miracast/env"
MIR_STATE="${XDG_STATE_HOME:-$HOME/.local/state}/sway-miracast"
MIR_RUN="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/sway-miracast"

# mir_env_get FILE KEY - the value of KEY=... from an EnvironmentFile-style file (never executed).
mir_env_get() { [[ -f "$1" ]] && sed -n "s/^$2=//p" "$1" | tail -n1; return 0; }

# Settings: environment first, then the optional env file.
MONITOR="${MONITOR:-$(mir_env_get "$MIR_ENV" MONITOR)}"
MIRACAST_ICONS="${MIRACAST_ICONS:-$(mir_env_get "$MIR_ENV" MIRACAST_ICONS)}"
MIRACAST_MENU="${MIRACAST_MENU:-$(mir_env_get "$MIR_ENV" MIRACAST_MENU)}"

# Icons: Nerd Font glyphs only with MIRACAST_ICONS=nerd; otherwise plain Unicode markers every font has.
if [[ "$MIRACAST_ICONS" == nerd ]]; then
	ICON_CAST=$'\U000f0118'; ICON_CAST_ON=$'\U000f0119'; ICON_CAST_OFF=$'\U000f078a'; ICON_TV=$'\U000f0502'
	ICON_REFRESH=$'\U000f0450'; ICON_CHECK=$'\U000f012c'; ICON_STAR=$'\U000f04ce'
	ICON_MONITOR=$'\U000f0379'; ICON_MONITORS=$'\U000f037a'
	ICON_WIFI_1=$'\U000f091f'; ICON_WIFI_2=$'\U000f0922'; ICON_WIFI_3=$'\U000f0925'; ICON_WIFI_4=$'\U000f0928'
	BAR_OFF=$'\U000f078a'; BAR_WAITING=$'\U000f0118'; BAR_CASTING=$'\U000f0119'
else
	ICON_CAST="▶"; ICON_CAST_ON="●"; ICON_CAST_OFF="■"; ICON_TV="▭"
	ICON_REFRESH="↻"; ICON_CHECK="✓"; ICON_STAR="★"
	ICON_MONITOR="▭"; ICON_MONITORS="▣"
	ICON_WIFI_1="▂"; ICON_WIFI_2="▄"; ICON_WIFI_3="▆"; ICON_WIFI_4="█"
	BAR_OFF="TV ○"; BAR_WAITING="TV …"; BAR_CASTING="TV ●"
fi

# mir_tool NAME - path of a sibling command (installed name, or repo name with .sh/.py); needs MIR_BIN.
mir_tool() {
	local n
	for n in "$MIR_BIN/$1" "$MIR_BIN/$1.sh" "$MIR_BIN/$1.py"; do
		[[ -x "$n" ]] && { printf '%s\n' "$n"; return 0; }
	done
	return 1
}

# mir_current_mac - MAC of the current TV (upper case): the state file written by the menu, else TV_MAC
# from the env file (optional), else empty.
mir_current_mac() {
	local mac
	mac="$(mir_env_get "$MIR_STATE/sink" TV_MAC)"
	[[ -z "$mac" ]] && mac="$(mir_env_get "$MIR_ENV" TV_MAC)"
	printf '%s\n' "${mac^^}"
}

# mir_known_name MAC - the TV's name from $MIR_STATE/known (MAC<TAB>name); empty when unknown.
mir_known_name() {
	awk -F'\t' -v m="${1^^}" 'toupper($1) == m { print $2; exit }' "$MIR_STATE/known" 2>/dev/null || true
}

# mir_unit_state - sets MIR_ACTIVE (active/activating/inactive/failed...), MIR_STATUS (StatusText) and
# MIR_CASTING (yes/no) with one systemctl call.
mir_unit_state() {
	local line
	MIR_ACTIVE="" MIR_STATUS="" MIR_CASTING=no
	while IFS= read -r line; do
		case "$line" in
			ActiveState=*) MIR_ACTIVE="${line#ActiveState=}" ;;
			StatusText=*) MIR_STATUS="${line#StatusText=}" ;;
		esac
	done < <(systemctl --user show -p ActiveState,StatusText "$MIR_UNIT" 2>/dev/null || true)
	[[ "$MIR_ACTIVE" == active && "$MIR_STATUS" == casting* ]] && MIR_CASTING=yes
	return 0
}

# mir_laptop_output - the laptop's Sway output: $MONITOR if set; else the internal panel (eDP/LVDS/DSI),
# else the first non-virtual output. Same rule as wfd-source. Empty when Sway cannot be asked.
mir_laptop_output() {
	if [[ -n "${MONITOR:-}" ]]; then
		printf '%s\n' "$MONITOR"
		return 0
	fi
	swaymsg -t get_outputs -r 2>/dev/null | python3 -c '
import json, sys
names = [o["name"] for o in json.load(sys.stdin)]
internal = [n for n in names if n.upper().startswith(("EDP", "LVDS", "DSI"))]
real = [n for n in names if not n.startswith("HEADLESS-")]
print((internal or real or [""])[0])
' 2>/dev/null || true
}

# mir_shown_output - what the TV shows (capture-output file): "auto", an output name (mirror) or HEADLESS-N
# (extend); empty without a file.
mir_shown_output() { cat "$MIR_RUN/capture-output" 2>/dev/null || true; }

# mir_is_mirror - true unless the TV shows a virtual HEADLESS-N output (extend mode).
mir_is_mirror() {
	[[ "$(mir_shown_output)" != HEADLESS-* ]]
}

# mir_virt_mode - sets MIR_VW, MIR_VH, MIR_VFPS from $MIR_RUN/mode (WxH@FPS negotiated with the TV);
# falls back to 1920x1080@60.
mir_virt_mode() {
	local m
	m="$(cat "$MIR_RUN/mode" 2>/dev/null || true)"
	if [[ "$m" =~ ^([0-9]+)x([0-9]+)@([0-9]+) ]]; then
		MIR_VW="${BASH_REMATCH[1]}" MIR_VH="${BASH_REMATCH[2]}" MIR_VFPS="${BASH_REMATCH[3]}"
	else
		MIR_VW=1920 MIR_VH=1080 MIR_VFPS=60
	fi
}

# mir_notify TAG TITLE BODY [notify-send options...] - notifications with the same TAG replace each other.
# Without notify-send (a session with no daemon) it is not an error.
mir_notify() {
	local tag="$1" title="$2" body="$3"
	shift 3
	command -v notify-send >/dev/null 2>&1 || return 0
	notify-send -a "$title" -i video-display -h "string:x-canonical-private-synchronous:$tag" "$@" "$title" "$body" || true
}

# mir_atomic_write FILE CONTENT - write through a temp file and mv: the Rust programs never read half a file.
mir_atomic_write() {
	local file="$1" tmp
	mkdir -p "$(dirname -- "$file")"
	tmp="$(mktemp "$file.XXXXXX")" || return 1
	if printf '%s\n' "$2" >"$tmp" && mv -f "$tmp" "$file"; then
		return 0
	fi
	rm -f "$tmp"
	return 1
}
