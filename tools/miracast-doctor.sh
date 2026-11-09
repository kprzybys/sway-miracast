#!/usr/bin/env bash
# miracast-doctor: read-only environment check for sway-miracast. Prints OK / WARN / FAIL lines with hints.
# Changes nothing, needs no root. Exit status 1 when anything FAILed, else 0.
set -uo pipefail

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

n_ok=0 n_warn=0 n_fail=0
ok() { printf '[ OK ] %s\n' "$1"; n_ok=$((n_ok + 1)); }
warn() { printf '[WARN] %s\n' "$1"; n_warn=$((n_warn + 1)); }
fail() { printf '[FAIL] %s\n' "$1"; n_fail=$((n_fail + 1)); }
hint() { printf '       %s\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }
# gst_has ELEMENT...: every element is known to GStreamer
gst_has() { local e; for e in "$@"; do gst-inspect-1.0 "$e" >/dev/null 2>&1 || return 1; done; }

echo "$(_ dr.title)"
echo

# --- Wi-Fi P2P ---------------------------------------------------------------------------------------
iw_bin="$(command -v iw || true)"
for c in /usr/sbin/iw /sbin/iw; do [[ -z "$iw_bin" && -x "$c" ]] && iw_bin="$c"; done
if [[ -z "$iw_bin" ]]; then
	warn "$(_ dr.iw_missing)"; hint "$(_ dr.iw_hint)"
else
	iw_out="$("$iw_bin" list 2>/dev/null || true)"
	if grep -q 'P2P-GO' <<<"$iw_out" && grep -q 'P2P-client' <<<"$iw_out"; then
		ok "$(_ dr.p2p_ok)"
	else
		fail "$(_ dr.p2p_fail)"; hint "$(_ dr.p2p_hint)"
	fi
fi

# --- NetworkManager ----------------------------------------------------------------------------------
if have nmcli && [[ "$(nmcli -t -f RUNNING general 2>/dev/null)" == running ]]; then
	ok "$(_ dr.nm_ok)"
	if nmcli -t -f TYPE device 2>/dev/null | grep -qx 'wifi-p2p'; then
		ok "$(_ dr.nmp2p_ok)"
	else
		fail "$(_ dr.nmp2p_fail)"; hint "$(_ dr.nmp2p_hint)"
	fi
	perms="$(nmcli -t -f PERMISSION,VALUE general permissions 2>/dev/null || true)"
	perm=org.freedesktop.NetworkManager.wifi.share.protected
	val="$(sed -n "s/^${perm//./\\.}:\\(.*\\)$/\\1/p" <<<"$perms")"
	if [[ -z "$val" ]]; then
		perm=org.freedesktop.NetworkManager.enable-disable-wifi
		val="$(sed -n "s/^${perm//./\\.}:\\(.*\\)$/\\1/p" <<<"$perms")"
	fi
	if [[ "$val" == yes ]]; then
		ok "$(_ dr.polkit_ok "$perm")"
	elif [[ -n "$val" ]]; then
		warn "$(_ dr.polkit_warn "$val" "$perm")"; hint "$(_ dr.polkit_hint)"
	else
		warn "$(_ dr.polkit_unknown)"
	fi
else
	fail "$(_ dr.nm_fail)"; hint "$(_ dr.nm_hint)"
fi

# --- journal (wpa_supplicant lines speed up TV detection) ----------------------------------------------
if have journalctl && jout="$(journalctl -t wpa_supplicant -n1 --no-pager -q 2>&1)" \
	&& ! grep -qiE 'insufficient permissions|not seeing messages' <<<"$jout"; then
	ok "$(_ dr.journal_ok)"
else
	warn "$(_ dr.journal_warn)"; hint "$(_ dr.journal_hint)"
fi

# --- python dbus (TV scan) ---------------------------------------------------------------------------
if python3 -I -c 'import dbus' 2>/dev/null; then
	ok "$(_ dr.dbus_ok)"
else
	warn "$(_ dr.dbus_warn)"; hint "$(_ dr.dbus_hint)"
fi

# --- GStreamer ---------------------------------------------------------------------------------------
if ! have gst-inspect-1.0; then
	fail "$(_ dr.gst_missing)"; hint "$(_ dr.gst_hint)"
else
	gv="$(gst-inspect-1.0 --version 2>/dev/null | sed -n 's/^GStreamer //p' | head -n1)"
	if [[ "$gv" =~ ^([0-9]+)\.([0-9]+) ]] && ((BASH_REMATCH[1] > 1 || BASH_REMATCH[2] >= 24)); then
		ok "$(_ dr.gst_ver_ok "$gv")"
	else
		warn "$(_ dr.gst_ver_warn "${gv:-?}")"
	fi
	if gst_has vah264lpenc vapostproc; then
		ok "$(_ dr.enc_hw vah264lpenc)"
	elif gst_has vah264enc vapostproc; then
		ok "$(_ dr.enc_hw vah264enc)"
	elif gst_has x264enc videoconvert; then
		warn "$(_ dr.enc_sw)"; hint "$(_ dr.enc_sw_hint)"
	else
		fail "$(_ dr.enc_fail)"; hint "$(_ dr.enc_sw_hint)"
	fi
fi

# --- audio -------------------------------------------------------------------------------------------
if have pactl && pactl info >/dev/null 2>&1; then
	ok "$(_ dr.audio_ok)"
else
	fail "$(_ dr.audio_fail)"; hint "$(_ dr.audio_hint)"
fi

# --- compositor --------------------------------------------------------------------------------------
if [[ -n "${SWAYSOCK:-}" ]] && have swaymsg && swaymsg -t get_version >/dev/null 2>&1; then
	ok "$(_ dr.wl_sway)"
elif [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
	if have wayland-info && wayland-info 2>/dev/null | grep -q zwlr_screencopy_manager_v1; then
		ok "$(_ dr.wl_ok)"
	else
		warn "$(_ dr.wl_other)"
	fi
else
	fail "$(_ dr.wl_fail)"; hint "$(_ dr.wl_hint)"
fi

# --- firewall ----------------------------------------------------------------------------------------
fw_found=0
if have systemctl && systemctl is-active --quiet ufw 2>/dev/null; then
	fw_found=1
	cmd='sudo ufw allow from 10.42.0.0/24 to any port 7236 proto tcp'
	rules="$(ufw status 2>/dev/null || true)"
	if [[ -z "$rules" ]]; then
		warn "$(_ dr.fw_unknown ufw)"; hint "$(_ dr.fw_hint "$cmd")"
	elif grep -E '7236' <<<"$rules" | grep -qE '10\.42\.0\.0/24|Anywhere.*ALLOW'; then
		ok "$(_ dr.fw_ok ufw)"
	else
		warn "$(_ dr.fw_warn ufw)"; hint "$(_ dr.fw_hint "$cmd")"
	fi
fi
if have systemctl && systemctl is-active --quiet firewalld 2>/dev/null; then
	fw_found=1
	cmd="sudo firewall-cmd --permanent --add-rich-rule='rule family=\"ipv4\" source address=\"10.42.0.0/24\" port port=\"7236\" protocol=\"tcp\" accept' && sudo firewall-cmd --reload"
	rules="$(firewall-cmd --list-rich-rules 2>/dev/null || true)"
	if grep -q '10\.42\.0\.0/24' <<<"$rules" && grep -q '7236' <<<"$rules"; then
		ok "$(_ dr.fw_ok firewalld)"
	else
		warn "$(_ dr.fw_warn firewalld)"; hint "$(_ dr.fw_hint "$cmd")"
	fi
fi
((fw_found)) || ok "$(_ dr.fw_none)"

# --- binaries ----------------------------------------------------------------------------------------
bindir=""
if src="$(command -v wfd-source)"; then
	bindir="$(dirname -- "$(readlink -f -- "$src")")"
elif [[ -x "$MIR_LIB/../../target/release/wfd-source" ]]; then
	bindir="$(cd -P -- "$MIR_LIB/../../target/release" && pwd)"
fi
missing=""
for b in wfd-source wfd-media wfd-tx; do
	[[ -n "$bindir" && -x "$bindir/$b" ]] || have "$b" || missing+="$b "
done
if [[ -z "$bindir" && -n "$missing" ]]; then
	fail "$(_ dr.bin_fail "${missing% }")"; hint "$(_ dr.bin_hint)"
elif [[ -n "$missing" ]]; then
	fail "$(_ dr.bin_split "$bindir" "${missing% }")"; hint "$(_ dr.bin_hint)"
else
	ok "$(_ dr.bin_ok "$bindir")"
fi

# --- systemd unit ------------------------------------------------------------------------------------
if systemctl --user cat "$MIR_UNIT" >/dev/null 2>&1; then
	st="$(systemctl --user is-enabled "$MIR_UNIT" 2>/dev/null || true)"
	if [[ "$st" == enabled || "$st" == static || "$st" == linked ]]; then
		ok "$(_ dr.unit_enabled "$MIR_UNIT" "$st")"
	else
		warn "$(_ dr.unit_disabled "$MIR_UNIT")"; hint "$(_ dr.unit_enable_hint "$MIR_UNIT")"
	fi
else
	fail "$(_ dr.unit_missing "$MIR_UNIT")"; hint "$(_ dr.unit_hint)"
fi

# --- selected TV -------------------------------------------------------------------------------------
mac="$(mir_current_mac)"
if [[ -n "$mac" ]]; then
	tvname="$(mir_known_name "$mac")"
	ok "$(_ dr.tv_ok "${tvname:-$mac}")"
else
	warn "$(_ dr.tv_warn)"; hint "$(_ dr.tv_hint)"
fi

# --- menu launcher and notifications -------------------------------------------------------------------
if mir_menu_detect; then
	ok "$(_ dr.menu_ok "${MIR_MENU_CMD[*]}")"
else
	warn "$(_ dr.menu_warn)"; hint "$(_ dr.menu_hint)"
fi
if have notify-send; then
	ok "$(_ dr.notify_ok)"
else
	warn "$(_ dr.notify_warn)"; hint "$(_ dr.notify_hint)"
fi

echo
_ dr.summary "$n_ok" "$n_warn" "$n_fail"; echo
((n_fail == 0))
