#!/usr/bin/env bash
# miracast-menu: pick a TV (Miracast) like a Wi-Fi list: a dmenu-style launcher + a Wi-Fi Direct scan
# (miracast-list). The choice is saved as the selected TV in $XDG_STATE_HOME/sway-miracast/sink (TV_MAC=...)
# and the service sway-miracast-source is restarted. While casting there is no scan: it would take the radio
# off the channel and interrupt the picture on the TV. A second invocation closes an open menu.
# Launcher: fuzzel, wofi, rofi, bemenu or dmenu (first found), or MIRACAST_MENU="<command>".
set -euo pipefail

SELF="$(readlink -f -- "${BASH_SOURCE[0]}")"
MIR_BIN="$(dirname -- "$SELF")"
MIR_LIB="$MIR_BIN/lib"
[[ -d "$MIR_LIB" ]] || MIR_LIB="$MIR_BIN/../lib"
# shellcheck source=lib/miracast.sh
. "$MIR_LIB/miracast.sh"
# shellcheck source=lib/i18n.sh
. "$MIR_LIB/i18n.sh"
# shellcheck source=lib/menu.sh
. "$MIR_LIB/menu.sh"

tv_notify() { mir_notify miracast "Miracast" "$@"; }

if ! mir_menu_detect; then
	tv_notify "$(_ menu.no_launcher)" -u critical
	_ menu.no_launcher >&2; echo >&2
	exit 1
fi
# A second click / key press closes the open menu (toggle). Before the lock below, on purpose.
mir_menu_close_running && exit 0

UNIT="$MIR_UNIT"
STATE="$MIR_STATE"
SINK_FILE="$STATE/sink"   # TV_MAC=AA:BB:...
KNOWN="$STATE/known"      # MAC<TAB>name, one line per TV
SCAN_SECS=5
mkdir -p "$STATE"
touch "$KNOWN"

# One instance at a time (key + bar click): two scans in parallel would take the radio off the channel twice.
exec 9>"$STATE/menu.lock"
flock -n 9 || exit 0

ITEM_RESCAN="$ICON_REFRESH  $(_ menu.rescan)"
ITEM_STOP="$ICON_CAST_OFF  $(_ menu.stop)"

# refresh the Waybar module (custom/miracast, signal 6) after a state change
bar_refresh() { pkill -RTMIN+6 -x waybar 2>/dev/null || true; }

signal_icon() {
	local s="$1" level
	if ((s >= 75)); then level=4
	elif ((s >= 50)); then level=3
	elif ((s >= 25)); then level=2
	else level=1
	fi
	local var="ICON_WIFI_${level}"
	echo "${!var}"
}

# Current target: the state file, else TV_MAC from the optional env file.
current="$(mir_current_mac)"

declare -A known=()
while IFS=$'\t' read -r mac name; do
	[[ -n "$mac" ]] && known["${mac^^}"]="$name"
done <"$KNOWN"
# First run with a TV from the env file: take its name from the last P2P-DEVICE-FOUND in the journal.
if [[ -n "$current" && -z "${known[$current]:-}" ]]; then
	name="$(journalctl -t wpa_supplicant -o cat --no-pager 2>/dev/null \
		| sed -n "s/.*P2P-DEVICE-FOUND ${current,,} .*name='\([^']*\)'.*/\1/p" | tail -n1 || true)"
	known["$current"]="${name:-$(_ menu.tv_default "$current")}"
fi

mir_unit_state
active="$MIR_ACTIVE"
casting="$MIR_CASTING"

declare -A item_mac=() visible=()
items=()
add_item() { items+=("$1"); item_mac["$1"]="$2"; }

if [[ "$casting" == no ]]; then
	tv_notify "$(_ menu.scanning)" -e -t $((SCAN_SECS * 1000))
	scan="$(python3 -I "$(mir_tool miracast-list)" "$SCAN_SECS" 2>/dev/null | sort -t$'\t' -k2,2nr || true)"
	while IFS=$'\t' read -r mac strength avail name; do
		[[ -z "$mac" || -n "${visible[$mac]:-}" ]] && continue
		visible["$mac"]=1
		known["$mac"]="$name"   # refresh the name
		mark=""
		[[ "$mac" == "$current" ]] && mark="  $ICON_STAR $(_ menu.selected)"
		[[ "$avail" == 0 ]] && mark+="  $(_ menu.busy)"
		add_item "$(signal_icon "$strength")  $(printf '%3s' "$strength")%  $name$mark" "$mac"
	done <<<"$scan"
fi

# Known but not visible now (TV off or no Smart View): picking one sets the target, the service waits for it.
mapfile -t known_sorted < <(printf '%s\n' "${!known[@]}" | sort)
for mac in "${known_sorted[@]}"; do
	[[ -z "$mac" || -n "${visible[$mac]:-}" ]] && continue
	name="${known[$mac]}"
	if [[ "$mac" == "$current" && "$casting" == yes ]]; then
		label="$ICON_CAST_ON  $name  $ICON_CHECK $(_ menu.mark_casting)"
		items=("$label" "${items[@]}"); item_mac["$label"]="$mac"
		continue
	fi
	mark=""
	[[ "$mac" == "$current" && "$active" == active ]] && mark="  $ICON_STAR $(_ menu.mark_waiting)"
	[[ "$mac" == "$current" && "$active" != active ]] && mark="  $ICON_STAR $(_ menu.selected)"
	add_item "$ICON_TV  $name  $(_ menu.invisible)$mark" "$mac"
done

# While casting: what the TV shows (miracast-screen; the session stays, only the encoder restarts).
ITEM_EXTEND="$ICON_MONITOR  $(_ menu.extend)"
ITEM_MIRROR="$ICON_MONITORS  $(_ menu.mirror)"
if [[ "$casting" == yes ]]; then
	if mir_is_mirror; then items+=("$ITEM_EXTEND"); else items+=("$ITEM_MIRROR"); fi
fi
[[ "$casting" == no ]] && items+=("$ITEM_RESCAN")
[[ "$active" == active || "$active" == activating ]] && items+=("$ITEM_STOP")

# Remember the names (also those refreshed by the scan).
known_tmp="$(mktemp "$KNOWN.XXXXXX")"
for mac in "${!known[@]}"; do printf '%s\t%s\n' "$mac" "${known[$mac]}"; done | sort >"$known_tmp"
mv -f "$known_tmp" "$KNOWN"

prompt="$ICON_CAST $(_ menu.prompt)"
[[ "$casting" == no && ${#visible[@]} -eq 0 ]] && prompt="$ICON_CAST $(_ menu.prompt_none)"
sel="$(printf '%s\n' "${items[@]}" | mir_menu "$prompt" --width 56)"
[[ -z "$sel" ]] && exit 0

case "$sel" in
	"$ITEM_RESCAN") flock -u 9; exec "$SELF" ;;
	"$ITEM_EXTEND") "$(mir_tool miracast-screen)" extend >/dev/null; bar_refresh; exit 0 ;;
	"$ITEM_MIRROR") "$(mir_tool miracast-screen)" mirror >/dev/null; bar_refresh; exit 0 ;;
	"$ITEM_STOP")
		systemctl --user stop "$UNIT" || tv_notify "$(_ menu.stop_failed "$UNIT")" -u critical
		bar_refresh
		tv_notify "$(_ menu.stopped)"
		exit 0 ;;
esac

mac="${item_mac[$sel]:-}"
[[ -z "$mac" ]] && exit 0
name="${known[$mac]:-$mac}"
if [[ "$mac" == "$current" && "$active" == active ]]; then
	# already selected and the service runs: interrupt nothing
	if [[ "$casting" == yes ]]; then
		tv_notify "$(_ menu.now_casting "$name")"
	else
		tv_notify "$(_ menu.waiting_for "$name")"
	fi
	exit 0
fi
# Developer mode (devlock on): the unit has ConditionPathExists=!dev.lock, a restart would start nothing.
if [[ -e "$MIR_RUN/dev.lock" ]]; then
	tv_notify "$(_ menu.devlock)"
	exit 0
fi
# Casting to another TV: do not interrupt without asking.
if [[ "$casting" == yes && "$mac" != "$current" ]]; then
	cur_name="${known[$current]:-$current}"
	ask="$(_ menu.confirm "$cur_name" "$name")"
	yes_item="$ICON_CHECK  $(_ menu.yes)"
	no_item="$ICON_CAST_OFF  $(_ menu.no)"
	answer="$(printf '%s\n%s\n' "$yes_item" "$no_item" | mir_menu "$ask" --width 56)"
	[[ "$answer" == "$yes_item" ]] || exit 0
fi
tmp_sink="$(mktemp "$SINK_FILE.XXXXXX")"
printf 'TV_MAC=%s\n' "$mac" >"$tmp_sink" && mv -f "$tmp_sink" "$SINK_FILE"
systemctl --user restart "$UNIT" || true
bar_refresh
mir_unit_state
if [[ "$MIR_ACTIVE" != active && "$MIR_ACTIVE" != activating ]]; then
	tv_notify "$(_ menu.start_failed "$name" "${MIR_ACTIVE:-$(_ menu.unknown)}" "$UNIT")" -u critical
	exit 1
fi
tv_notify "$(_ menu.chosen "$name")"
