#!/usr/bin/env bash
# miracast-menu: pick a TV (Miracast) like a Wi-Fi list: a dmenu-style launcher + a Wi-Fi Direct scan
# (miracast-list). The choice is saved as the selected TV in $XDG_STATE_HOME/sway-miracast/sink (TV_MAC=...)
# and the service sway-miracast-source is restarted. While casting there is no scan: it would take the radio
# off the channel and interrupt the picture on the TV. A second invocation closes an open menu.
# Launcher: fuzzel, wofi, rofi, bemenu or dmenu (first found), or MIRACAST_MENU="<command>".
# With fuzzel >= 1.11 the menu opens at once with the remembered TVs and the scan results are appended while
# it is open (a feed process, `miracast-menu --feed RUN_DIR`); the other launchers wait for the whole scan.
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

UNIT="$MIR_UNIT"
STATE="$MIR_STATE"
SINK_FILE="$STATE/sink"   # TV_MAC=AA:BB:...
KNOWN="$STATE/known"      # MAC<TAB>name, one line per TV
SCAN_SECS=5
mkdir -p "$STATE"
touch "$KNOWN"

ITEM_RESCAN="$ICON_REFRESH  $(_ menu.rescan)"
ITEM_STOP="$ICON_CAST_OFF  $(_ menu.stop)"
ITEM_SETTINGS="$ICON_GEAR  $(_ menu.settings)"

bar_refresh() { mir_bar_refresh; }

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

# load_known -> known[MAC]=name and current (the target: the state file, else TV_MAC from the env file)
declare -A known=()
current=""
load_known() {
	local mac name
	current="$(mir_current_mac)"
	known=()
	while IFS=$'\t' read -r mac name; do
		[[ -n "$mac" ]] && known["${mac^^}"]="$name"
	done <"$KNOWN"
	# First run with a TV from the env file: take its name from the last P2P-DEVICE-FOUND in the journal.
	if [[ -n "$current" && -z "${known[$current]:-}" ]]; then
		name="$(journalctl -t wpa_supplicant -o cat --no-pager 2>/dev/null \
			| sed -n "s/.*P2P-DEVICE-FOUND ${current,,} .*name='\([^']*\)'.*/\1/p" | tail -n1 || true)"
		known["$current"]="${name:-$(_ menu.tv_default "$current")}"
	fi
}

# Remember the names (also those refreshed by the scan).
save_known() {
	local tmp mac
	tmp="$(mktemp "$KNOWN.XXXXXX")"
	for mac in "${!known[@]}"; do printf '%s\t%s\n' "$mac" "${known[$mac]}"; done | sort >"$tmp"
	mv -f "$tmp" "$KNOWN"
}

# scan_progress [RUN_DIR] - a determinate progress bar in the notification while the scan runs: the scan
# length is fixed (SCAN_SECS), so a percentage instead of a bare "looking...". Kill it when the scan ends.
# With RUN_DIR (the feed): the count from RUN_DIR/count and the lines of RUN_DIR/seen (remembered TVs the scan
# saw: they stay in the menu once, fuzzel cannot change a line it already shows).
scan_progress() {
	local run_dir="${1:-}" t0 el pct body
	t0=$(date +%s%3N)
	while :; do
		el=$(($(date +%s%3N) - t0))
		pct=$((el * 100 / (SCAN_SECS * 1000)))
		((pct > 95)) && pct=95
		if [[ -n "$run_dir" ]]; then
			body="$(_ menu.scan_progress "$SCAN_SECS" "$(cat "$run_dir/count" 2>/dev/null || echo 0)")"
			[[ -s "$run_dir/seen" ]] && body+=$'\n'"$(cat "$run_dir/seen")"
		else
			body="$(_ menu.scanning)"
		fi
		tv_notify "$body" -e -t 1500 -h "int:value:$pct"
		sleep 0.25
	done
}

# --feed RUN_DIR: menu lines on stdout, "label<TAB>kind<TAB>MAC" in RUN_DIR/items (kind: tv|stop|rescan).
# Remembered TVs at once, then the TVs found by the scan as they show up. No traps: miracast-menu ends the
# whole process group (scan, progress bar) with one kill.
if [[ "${1:-}" == --feed ]]; then
	run_dir="$2"
	item() { printf '%s\n' "$1"; printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$run_dir/items"; }
	load_known
	mir_unit_state
	declare -A remembered=() seen=()
	mapfile -t known_sorted < <(printf '%s\n' "${!known[@]}" | sort)
	for mac in "${known_sorted[@]}"; do
		[[ -z "$mac" ]] && continue
		remembered["$mac"]=1
		mark=""
		[[ "$mac" == "$current" && "$MIR_ACTIVE" == active ]] && mark="  $ICON_STAR $(_ menu.mark_waiting)"
		[[ "$mac" == "$current" && "$MIR_ACTIVE" != active ]] && mark="  $ICON_STAR $(_ menu.selected)"
		item "$ICON_TV  ${known[$mac]}  ($(_ menu.remembered))$mark" tv "$mac"
	done
	[[ "$MIR_ACTIVE" == active || "$MIR_ACTIVE" == activating ]] && item "$ITEM_STOP" stop ""
	((${#known_sorted[@]})) && item "$ITEM_SETTINGS" settings ""

	echo 0 >"$run_dir/count"
	: >"$run_dir/seen"
	scan_progress "$run_dir" &
	progress_pid=$!
	found=0
	while IFS=$'\t' read -r mac strength avail name; do
		[[ -z "$mac" || -n "${seen[$mac]:-}" ]] && continue
		seen["$mac"]=1
		known["$mac"]="$name"
		found=$((found + 1))
		echo "$found" >"$run_dir/count"
		if [[ -n "${remembered[$mac]:-}" ]]; then
			# already in the menu (remembered): its visibility goes to the progress notification
			if [[ "$avail" == 0 ]]; then
				_ menu.known_visible_busy "$name" >>"$run_dir/seen"
			else
				_ menu.known_visible "$name" >>"$run_dir/seen"
			fi
			echo >>"$run_dir/seen"
			continue
		fi
		mark=""
		[[ "$mac" == "$current" ]] && mark="  $ICON_STAR $(_ menu.selected)"
		[[ "$avail" == 0 ]] && mark+="  $(_ menu.busy)"
		item "$(signal_icon "$strength")  $(printf '%3s' "$strength")%  $name  $(_ menu.found_now)$mark" tv "$mac"
	done < <(python3 -I "$(mir_tool miracast-list)" --stream "$SCAN_SECS" 2>/dev/null || true)
	kill "$progress_pid" 2>/dev/null || true
	done_body="$(_ menu.scan_done "$found")"
	[[ -s "$run_dir/seen" ]] && done_body+=$'\n'"$(cat "$run_dir/seen")"
	tv_notify "$done_body" -e -t 3000 -h int:value:100
	item "$ITEM_RESCAN" rescan ""
	save_known
	exit 0
fi

if ! mir_menu_detect; then
	tv_notify "$(_ menu.no_launcher)" -u critical
	_ menu.no_launcher >&2; echo >&2
	exit 1
fi
# A second click / key press closes the open menu (toggle). Before the lock below, on purpose.
mir_menu_close_running && exit 0

# One instance at a time (key + bar click): two scans in parallel would take the radio off the channel twice.
exec 9>"$STATE/menu.lock"
flock -n 9 || exit 0

load_known
mir_unit_state
active="$MIR_ACTIVE"
casting="$MIR_CASTING"

ITEM_EXTEND="$ICON_MONITOR  $(_ menu.extend)"
ITEM_MIRROR="$ICON_MONITORS  $(_ menu.mirror)"
kind="" mac=""

if [[ "$casting" == no ]] && mir_menu_can_stream; then
	run_dir="$(mktemp -d)"
	feed_pid=""
	# The scan and the progress bar live in the feed's process group (setsid): one kill ends them all,
	# also when the menu closes mid-scan.
	stop_feed() {
		if [[ -n "$feed_pid" ]]; then
			kill -TERM -- "-$feed_pid" 2>/dev/null || true
			feed_pid=""
		fi
	}
	trap 'stop_feed; rm -rf "$run_dir"' EXIT
	exec {feed_out}< <(exec setsid "$SELF" --feed "$run_dir")
	feed_pid=$!
	sel="$(mir_menu_stream "$ICON_CAST $(_ menu.prompt)" --width 56 <&"$feed_out")"
	exec {feed_out}<&-
	[[ -z "$sel" ]] && exit 0
	IFS=$'\t' read -r _ kind mac < <(awk -F'\t' -v s="$sel" '$1 == s { print; exit }' "$run_dir/items") || true
	stop_feed
	rm -rf "$run_dir"
	load_known   # the names the feed refreshed
else
	declare -A item_mac=() item_kind=() visible=()
	items=()
	add_item() { items+=("$1"); item_kind["$1"]="$2"; item_mac["$1"]="$3"; }

	if [[ "$casting" == no ]]; then
		scan_progress &
		progress_pid=$!
		scan="$(python3 -I "$(mir_tool miracast-list)" "$SCAN_SECS" 2>/dev/null | sort -t$'\t' -k2,2nr || true)"
		kill "$progress_pid" 2>/dev/null || true
		while IFS=$'\t' read -r tv_mac strength avail name; do
			[[ -z "$tv_mac" || -n "${visible[$tv_mac]:-}" ]] && continue
			visible["$tv_mac"]=1
			known["$tv_mac"]="$name"   # refresh the name
			mark=""
			[[ "$tv_mac" == "$current" ]] && mark="  $ICON_STAR $(_ menu.selected)"
			[[ "$avail" == 0 ]] && mark+="  $(_ menu.busy)"
			add_item "$(signal_icon "$strength")  $(printf '%3s' "$strength")%  $name$mark" tv "$tv_mac"
		done <<<"$scan"
	fi

	# Known but not visible now (TV off or no Smart View): picking one sets the target, the service waits for it.
	mapfile -t known_sorted < <(printf '%s\n' "${!known[@]}" | sort)
	for tv_mac in "${known_sorted[@]}"; do
		[[ -z "$tv_mac" || -n "${visible[$tv_mac]:-}" ]] && continue
		name="${known[$tv_mac]}"
		if [[ "$tv_mac" == "$current" && "$casting" == yes ]]; then
			label="$ICON_CAST_ON  $name  $ICON_CHECK $(_ menu.mark_casting)"
			items=("$label" "${items[@]}"); item_kind["$label"]=tv; item_mac["$label"]="$tv_mac"
			continue
		fi
		mark=""
		[[ "$tv_mac" == "$current" && "$active" == active ]] && mark="  $ICON_STAR $(_ menu.mark_waiting)"
		[[ "$tv_mac" == "$current" && "$active" != active ]] && mark="  $ICON_STAR $(_ menu.selected)"
		add_item "$ICON_TV  $name  $(_ menu.invisible)$mark" tv "$tv_mac"
	done

	# While casting: what the TV shows (miracast-screen; the session stays, only the encoder restarts).
	if [[ "$casting" == yes ]]; then
		if mir_is_mirror; then add_item "$ITEM_EXTEND" extend ""; else add_item "$ITEM_MIRROR" mirror ""; fi
	fi
	[[ "$casting" == no ]] && add_item "$ITEM_RESCAN" rescan ""
	[[ "$active" == active || "$active" == activating ]] && add_item "$ITEM_STOP" stop ""
	((${#known[@]})) && add_item "$ITEM_SETTINGS" settings ""

	save_known

	prompt="$ICON_CAST $(_ menu.prompt)"
	[[ "$casting" == no && ${#visible[@]} -eq 0 ]] && prompt="$ICON_CAST $(_ menu.prompt_none)"
	sel="$(printf '%s\n' "${items[@]}" | mir_menu "$prompt" --width 56)"
	[[ -z "$sel" ]] && exit 0
	kind="${item_kind[$sel]:-}"
	mac="${item_mac[$sel]:-}"
fi

# TV settings: per remembered TV, whether wfd-source connects on its own (default) or only on a pick here
# (on demand). Picking a TV toggles it; the list comes back until Back / Esc.
tv_settings() {
	local mac label sel back
	back="$ICON_CAST_OFF  $(_ menu.back)"
	local -A by_label=()
	if ((${#known[@]} == 0)); then
		tv_notify "$(_ menu.settings_none)"
		return 0
	fi
	while :; do
		by_label=()
		local lines=()
		mapfile -t known_sorted < <(printf '%s\n' "${!known[@]}" | sort)
		for mac in "${known_sorted[@]}"; do
			[[ -z "$mac" ]] && continue
			if mir_on_demand "$mac"; then
				label="$ICON_TV  ${known[$mac]}  —  $(_ menu.on_demand)"
			else
				label="$ICON_TV  ${known[$mac]}  —  $(_ menu.auto)"
			fi
			by_label["$label"]="$mac"
			lines+=("$label")
		done
		sel="$(printf '%s\n' "${lines[@]}" "$back" | mir_menu "$ICON_GEAR $(_ menu.settings_prompt)" --width 64)"
		mac="${by_label[$sel]:-}"
		[[ -z "$mac" ]] && return 0
		if mir_on_demand "$mac"; then
			mir_set_on_demand "$mac" off
			tv_notify "$(_ menu.set_auto "${known[$mac]}")"
		else
			mir_set_on_demand "$mac" on
			tv_notify "$(_ menu.set_on_demand "${known[$mac]}")"
		fi
	done
}

case "$kind" in
	rescan) flock -u 9; exec "$SELF" ;;
	settings) tv_settings; exit 0 ;;
	extend) "$(mir_tool miracast-screen)" extend >/dev/null; bar_refresh; exit 0 ;;
	mirror) "$(mir_tool miracast-screen)" mirror >/dev/null; bar_refresh; exit 0 ;;
	stop)
		systemctl --user stop "$UNIT" || tv_notify "$(_ menu.stop_failed "$UNIT")" -u critical
		rm -f "$MIR_RUN/connect-now"
		bar_refresh
		tv_notify "$(_ menu.stopped)"
		exit 0 ;;
	tv) ;;
	*) exit 0 ;;
esac

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
# a pick here also lets wfd-source connect to an on-demand TV (until that session ends or "stop")
mir_atomic_write "$MIR_RUN/connect-now" "$mac"
systemctl --user restart "$UNIT" || true
bar_refresh
mir_unit_state
if [[ "$MIR_ACTIVE" != active && "$MIR_ACTIVE" != activating ]]; then
	tv_notify "$(_ menu.start_failed "$name" "${MIR_ACTIVE:-$(_ menu.unknown)}" "$UNIT")" -u critical
	exit 1
fi
tv_notify "$(_ menu.chosen "$name")"
