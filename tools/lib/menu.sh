#!/usr/bin/env bash
# Menu launcher backend for sway-miracast: the first available of fuzzel, wofi, rofi, bemenu, dmenu.
# Source after lib/miracast.sh. Sets no shell options.
#
#   MIRACAST_MENU   a full dmenu-style command (words split on spaces), overrides detection; the prompt
#                   and the line count arrive in MIRACAST_PROMPT / MIRACAST_LINES. Also read from the env file.
#
#   mir_menu_detect            -> sets MIR_MENU_KIND and MIR_MENU_CMD (array); returns 1 when nothing is found
#   mir_menu_close_running     -> kills an open menu of this backend; returns 0 if one was open (toggle)
#   mir_menu PROMPT [--width N]  items on stdin, choice on stdout (empty on Esc, exit status 0)

MIR_MENU_KIND=""
MIR_MENU_CMD=()

mir_menu_detect() {
	local custom="${MIRACAST_MENU:-}" b
	if [[ -n "$custom" ]]; then
		read -ra MIR_MENU_CMD <<<"$custom"
		MIR_MENU_KIND=custom
		[[ ${#MIR_MENU_CMD[@]} -gt 0 ]]
		return
	fi
	for b in fuzzel wofi rofi bemenu dmenu; do
		if command -v "$b" >/dev/null 2>&1; then
			MIR_MENU_KIND="$b"
			MIR_MENU_CMD=("$b")
			return 0
		fi
	done
	return 1
}

mir_menu_close_running() {
	local name="${MIR_MENU_CMD[0]##*/}"
	[[ -n "$name" ]] && pkill -x "${name:0:15}" 2>/dev/null
}

mir_menu() {
	local prompt="$1" width="" items n
	shift
	while (($#)); do
		case "$1" in --width) width="${2:-}"; shift ;; esac
		shift
	done
	items="$(cat)"
	n="$(printf '%s\n' "$items" | grep -c '')"
	((n > 14)) && n=14
	case "$MIR_MENU_KIND" in
		fuzzel)
			local extra=()
			[[ -n "$width" ]] && extra=(--width "$width")
			printf '%s\n' "$items" | "${MIR_MENU_CMD[0]}" --log-level=warning --dmenu --prompt "$prompt  " --lines "$n" "${extra[@]}" || true ;;
		wofi) printf '%s\n' "$items" | "${MIR_MENU_CMD[0]}" --dmenu --prompt "$prompt" --lines "$n" || true ;;
		rofi) printf '%s\n' "$items" | "${MIR_MENU_CMD[0]}" -dmenu -i -p "$prompt" -l "$n" || true ;;
		bemenu) printf '%s\n' "$items" | "${MIR_MENU_CMD[0]}" -p "$prompt" -l "$n" || true ;;
		dmenu) printf '%s\n' "$items" | "${MIR_MENU_CMD[0]}" -p "$prompt" -l "$n" || true ;;
		*) printf '%s\n' "$items" | MIRACAST_PROMPT="$prompt" MIRACAST_LINES="$n" "${MIR_MENU_CMD[@]}" || true ;;
	esac
}
