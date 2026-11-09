#!/usr/bin/env bash
# Message table for the sway-miracast tools: English by default, Polish when the locale
# (LC_ALL, then LC_MESSAGES, then LANG; the first non-empty one wins) starts with "pl".
# Source only; sets no shell options. Usage:  _ key [printf arguments...]
#   notify "$(_ menu.now_casting "$name")"
# An unknown key prints the key itself; a missing Polish text falls back to English.
#
# shellcheck disable=SC2034  # tables are read through a nameref in _()

mir_lang() {
	local v
	for v in "${LC_ALL:-}" "${LC_MESSAGES:-}" "${LANG:-}"; do
		if [[ -n "$v" ]]; then
			case "$v" in pl*) echo pl ;; *) echo en ;; esac
			return 0
		fi
	done
	echo en
}
MIR_LANG="$(mir_lang)"

declare -A MIR_MSG_EN=() MIR_MSG_PL=()
# m KEY ENGLISH POLISH
m() { MIR_MSG_EN["$1"]="$2"; MIR_MSG_PL["$1"]="$3"; }

# shellcheck disable=SC2059  # the message is the printf format on purpose
_() {
	local key="$1" fmt
	shift
	local -n tbl="MIR_MSG_${MIR_LANG^^}"
	fmt="${tbl[$key]-}"
	[[ -n "$fmt" ]] || fmt="${MIR_MSG_EN[$key]-$key}"
	printf -- "$fmt" "$@"
}

# --- miracast-menu -----------------------------------------------------------------------------------
m menu.no_launcher "No menu launcher found (fuzzel, wofi, rofi, bemenu, dmenu) - TV picker unavailable. Set MIRACAST_MENU." \
	"Brak programu menu (fuzzel, wofi, rofi, bemenu, dmenu) - wybór TV niedostępny. Ustaw MIRACAST_MENU."
m menu.rescan "Scan again" "Skanuj ponownie"
m menu.stop "Disconnect (turn Miracast off)" "Rozłącz (wyłącz Miracast)"
m menu.scanning "Looking for TVs... (the TV must have Smart View / screen mirroring on)" \
	"Szukam telewizorów... (TV musi mieć włączone Smart View)"
m menu.selected "selected" "wybrany"
m menu.busy "(busy)" "(zajęty)"
m menu.mark_casting "casting" "nadaje"
m menu.mark_waiting "waiting for Smart View" "czeka na Smart View"
m menu.invisible "(not visible)" "(niewidoczny)"
m menu.extend "Extend: TV as a separate screen on the right" "Rozszerz: TV jako osobny ekran po prawej"
m menu.mirror "Duplicate: TV shows the laptop screen" "Duplikuj: TV pokazuje ekran laptopa"
m menu.prompt "TV" "Telewizor"
m menu.prompt_none "No TV nearby - turn on Smart View" "Brak TV w pobliżu - włącz Smart View"
m menu.stop_failed "Could not stop %s." "Nie udało się zatrzymać usługi %s."
m menu.stopped "Disconnected. Miracast stays off until you pick a TV." "Rozłączono. Miracast wyłączony do następnego wyboru TV."
m menu.now_casting "Casting to: %s" "Nadaje na: %s"
m menu.waiting_for "Waiting for %s - turn on Smart View on the TV." "Czekam na %s - włącz Smart View na TV."
m menu.devlock "Developer mode (dev.lock) - Miracast paused" "Tryb deweloperski (dev.lock) - Miracast wstrzymany"
m menu.confirm "Stop casting to %s and switch to %s?" "Przerwać nadawanie na %s i przełączyć na %s?"
m menu.yes "Yes, switch" "Tak, przełącz"
m menu.no "No, stay" "Nie, zostań"
m menu.unknown "unknown" "nieznany"
m menu.start_failed "Could not start Miracast for: %s (service state: %s). See: journalctl --user -u %s" \
	"Nie udało się uruchomić Miracasta dla: %s (stan usługi: %s). Zobacz: journalctl --user -u %s"
m menu.chosen "Selected: %s. Turn on Smart View on the TV." "Wybrano: %s. Włącz Smart View na telewizorze."
m menu.tv_default "TV %s" "TV %s"

# --- miracast-status ---------------------------------------------------------------------------------
m st.none "none" "brak"
m st.start "starting" "start"
m st.off "Miracast is off. Click to pick a TV." "Miracast wyłączony. Klik - wybierz telewizor."
m st.mode_dup "duplicate" "duplikacja"
m st.mode_ext "separate screen" "osobny ekran"
m st.casting "Miracast: casting to %s (%s, %s). Click for the menu (duplicate/extend, disconnect, another TV)." \
	"Miracast: nadaje na %s (%s, %s). Klik - menu (duplikuj/rozszerz, rozłącz, inny TV)."
m st.waiting "Miracast: waiting for %s (%s). Turn on Smart View on the TV. Click to pick a TV." \
	"Miracast: czekam na %s (%s). Włącz Smart View na TV. Klik - wybierz telewizor."
m st.no_tv "Miracast: no TV selected. Click to pick a TV." "Miracast: nie wybrano telewizora. Klik - wybierz telewizor."
m st.notify_waiting "Turn on Smart View / screen mirroring on the TV" "Włącz Smart View na telewizorze"
m st.usage "usage: %s [--notify-waiting]" "użycie: %s [--notify-waiting]"

# --- miracast-screen ---------------------------------------------------------------------------------
m sc.title "TV screen" "Ekran TV"
m sc.no_session "No session with the TV - turn on Smart View first" "Brak sesji z TV - najpierw włącz Smart View"
m sc.no_source "wfd-source is not running - TV screen change skipped" "wfd-source nie działa - zmiana ekranu TV pominięta"
m sc.write_failed "Could not write %s" "Nie udało się zapisać %s"
m sc.signal_failed "Could not notify wfd-source (SIGUSR2) - staying on: %s" \
	"Nie udało się powiadomić wfd-source (SIGUSR2) - zostaje: %s"
m sc.create_failed "Could not create the virtual screen" "Nie udało się utworzyć wirtualnego ekranu"
m sc.no_sway "Sway is not running (swaymsg failed)" "Sway nie działa (swaymsg zawiódł)"
m sc.extended "TV: separate screen (%s, workspace %s) to the right of the laptop" \
	"TV: osobny ekran (%s, pulpit %s) po prawej od laptopa"
m sc.mirrored "TV: mirroring the laptop screen" "TV: kopia ekranu laptopa"
m sc.shows "TV shows: %s" "TV pokazuje: %s"
m sc.usage "usage: %s [toggle|mirror|extend|status]" "użycie: %s [toggle|mirror|extend|status]"

# --- miracast-film -----------------------------------------------------------------------------------
m fm.write_failed "Could not write %s" "Nie udało się zapisać %s"
m fm.no_session "No session (wfd-tx is not running); a new session starts in desk mode" \
	"Brak sesji (wfd-tx nie działa); nowa sesja startuje w trybie biurko"
m fm.no_sigusr1 "Note: wfd-media does not answer SIGUSR1 (keyframe after the change may be late)" \
	"Uwaga: wfd-media nie odpowiada na SIGUSR1 (klatka kluczowa po zmianie może się spóźnić)"
m fm.off "Desk mode: video without delay" "Tryb biurko: obraz bez opóźnienia"
m fm.running "Film mode measurement already running" "Pomiar trybu film już trwa"
m fm.measuring "Film mode: measuring audio latency (~6 s of beeps on the TV)..." \
	"Tryb film: pomiar opóźnienia dźwięku (~6 s piknięć na TV)..."
m fm.on "Film mode: video +%s ms (audio %s ms, video ~%s ms)" "Tryb film: obraz +%s ms (dźwięk %s ms, obraz ~%s ms)"
m fm.failed "Film mode: measurement failed, video +%s ms (last value)" \
	"Tryb film: pomiar nieudany, obraz +%s ms (ostatnia wartość)"
m fm.video "Video +%s ms" "Obraz +%s ms"
m fm.status "video-hold-ms: %s" "video-hold-ms: %s"
m fm.usage "usage: %s [toggle|on|off|status|MS]" "użycie: %s [toggle|on|off|status|MS]"

# --- miracast-doctor ---------------------------------------------------------------------------------
m dr.title "sway-miracast: environment check (read-only)" "sway-miracast: kontrola środowiska (tylko odczyt)"
m dr.summary "Result: %s OK, %s warnings, %s failures" "Wynik: %s OK, %s ostrzeżeń, %s błędów"
m dr.p2p_ok "Wi-Fi supports P2P (P2P-GO and P2P-client)" "Wi-Fi obsługuje P2P (P2P-GO i P2P-client)"
m dr.p2p_fail "No Wi-Fi interface advertises P2P-GO and P2P-client" "Żadna karta Wi-Fi nie ogłasza P2P-GO i P2P-client"
m dr.p2p_hint "Miracast needs a Wi-Fi chip and driver with Wi-Fi Direct (see 'iw list')." \
	"Miracast wymaga układu i sterownika Wi-Fi z Wi-Fi Direct (zob. 'iw list')."
m dr.iw_missing "'iw' not found - cannot check P2P support" "Brak 'iw' - nie można sprawdzić obsługi P2P"
m dr.iw_hint "Install the 'iw' package." "Zainstaluj pakiet 'iw'."
m dr.nm_ok "NetworkManager is running" "NetworkManager działa"
m dr.nm_fail "NetworkManager is not running (or nmcli is missing)" "NetworkManager nie działa (albo brak nmcli)"
m dr.nm_hint "sway-miracast drives Wi-Fi Direct through NetworkManager." "sway-miracast steruje Wi-Fi Direct przez NetworkManager."
m dr.nmp2p_ok "NetworkManager has a wifi-p2p device" "NetworkManager ma urządzenie wifi-p2p"
m dr.nmp2p_fail "NetworkManager has no wifi-p2p device" "NetworkManager nie ma urządzenia wifi-p2p"
m dr.nmp2p_hint "Needs NetworkManager >= 1.16 with wpa_supplicant (not iwd) and a P2P-capable card." \
	"Wymaga NetworkManager >= 1.16 z wpa_supplicant (nie iwd) i karty z P2P."
m dr.polkit_ok "polkit lets this user control NetworkManager (%s: yes)" "polkit pozwala temu użytkownikowi sterować NetworkManagerem (%s: yes)"
m dr.polkit_warn "polkit answers '%s' for %s" "polkit odpowiada '%s' dla %s"
m dr.polkit_hint "Use an active local session, or add a polkit rule for this user." \
	"Użyj aktywnej sesji lokalnej albo dodaj regułę polkit dla tego użytkownika."
m dr.polkit_unknown "Could not read NetworkManager permissions" "Nie udało się odczytać uprawnień NetworkManagera"
m dr.journal_ok "Journal readable for wpa_supplicant (fast TV detection)" "Dziennik czytelny dla wpa_supplicant (szybkie wykrywanie TV)"
m dr.journal_warn "Cannot read the journal for wpa_supplicant - TV detection will be slower" \
	"Brak dostępu do dziennika wpa_supplicant - wykrywanie TV będzie wolniejsze"
m dr.journal_hint "Add yourself to the 'systemd-journal' group: sudo usermod -aG systemd-journal \$USER (re-login)." \
	"Dodaj siebie do grupy 'systemd-journal': sudo usermod -aG systemd-journal \$USER (ponowne logowanie)."
m dr.gst_missing "GStreamer tools (gst-inspect-1.0) not found" "Brak narzędzi GStreamera (gst-inspect-1.0)"
m dr.gst_hint "Install gstreamer1.0-tools and the plugin packages (base, good, bad, vaapi/va)." \
	"Zainstaluj gstreamer1.0-tools i wtyczki (base, good, bad, vaapi/va)."
m dr.gst_ver_ok "GStreamer %s" "GStreamer %s"
m dr.gst_ver_warn "GStreamer %s is older than 1.24" "GStreamer %s jest starszy niż 1.24"
m dr.enc_hw "Hardware H.264 encoding available (%s + vapostproc)" "Sprzętowe kodowanie H.264 dostępne (%s + vapostproc)"
m dr.enc_sw "No VA encoder - software encoding with x264enc (higher CPU load, more latency)" \
	"Brak kodera VA - kodowanie programowe x264enc (większe obciążenie CPU i opóźnienie)"
m dr.enc_sw_hint "Install the VA plugins (gstreamer1.0-plugins-bad) and your GPU's VA driver (e.g. intel-media-va-driver)." \
	"Zainstaluj wtyczki VA (gstreamer1.0-plugins-bad) i sterownik VA karty (np. intel-media-va-driver)."
m dr.enc_fail "No usable H.264 encoder (vah264lpenc/vah264enc or x264enc + videoconvert)" \
	"Brak użytecznego kodera H.264 (vah264lpenc/vah264enc albo x264enc + videoconvert)"
m dr.audio_ok "PipeWire/PulseAudio is running (pactl)" "PipeWire/PulseAudio działa (pactl)"
m dr.audio_fail "pactl missing or no audio server" "Brak pactl albo serwera dźwięku"
m dr.audio_hint "Install pipewire, pipewire-pulse and pulseaudio-utils; start them in your session." \
	"Zainstaluj pipewire, pipewire-pulse i pulseaudio-utils; uruchom w sesji."
m dr.wl_sway "Running under Sway (wlr-screencopy available)" "Działa pod Swayem (wlr-screencopy dostępne)"
m dr.wl_other "Wayland session, but not Sway; wlr-screencopy not verified" "Sesja Wayland, ale nie Sway; wlr-screencopy niezweryfikowane"
m dr.wl_ok "Compositor offers wlr-screencopy" "Kompozytor oferuje wlr-screencopy"
m dr.wl_fail "No Sway/Wayland session visible (SWAYSOCK and WAYLAND_DISPLAY empty)" \
	"Brak sesji Sway/Wayland (SWAYSOCK i WAYLAND_DISPLAY puste)"
m dr.wl_hint "Run miracast-doctor from inside your Sway session." "Uruchom miracast-doctor z wnętrza sesji Sway."
m dr.fw_none "No active firewall (ufw/firewalld) detected" "Nie wykryto aktywnej zapory (ufw/firewalld)"
m dr.fw_ok "%s allows 10.42.0.0/24 to TCP 7236" "%s przepuszcza 10.42.0.0/24 na TCP 7236"
m dr.fw_warn "%s is active and no rule for 10.42.0.0/24 TCP 7236 was found" \
	"%s jest aktywna i nie znaleziono reguły dla 10.42.0.0/24 TCP 7236"
m dr.fw_unknown "%s is active; rules cannot be read without root" "%s jest aktywna; reguł nie da się odczytać bez roota"
m dr.fw_hint "If the TV cannot connect, add:  %s" "Gdy TV nie może się połączyć, dodaj:  %s"
m dr.bin_ok "wfd-source, wfd-media and wfd-tx found in %s" "wfd-source, wfd-media i wfd-tx znalezione w %s"
m dr.bin_fail "Missing binaries: %s" "Brak programów: %s"
m dr.bin_hint "Build with 'make build' and install with 'make install' (see INSTALL.md)." \
	"Zbuduj 'make build' i zainstaluj 'make install' (zob. INSTALL.md)."
m dr.bin_split "wfd-source is in %s but %s is not next to it or on PATH" "wfd-source jest w %s, ale %s nie leży obok ani w PATH"
m dr.unit_missing "systemd user unit %s is not installed" "Jednostka użytkownika %s nie jest zainstalowana"
m dr.unit_hint "Run 'make install' (see INSTALL.md), then: systemctl --user daemon-reload" \
	"Uruchom 'make install' (zob. INSTALL.md), potem: systemctl --user daemon-reload"
m dr.unit_enabled "Unit %s is installed and enabled (state: %s)" "Jednostka %s zainstalowana i włączona (stan: %s)"
m dr.unit_disabled "Unit %s is installed but not enabled" "Jednostka %s zainstalowana, ale niewłączona"
m dr.unit_enable_hint "Enable it:  systemctl --user enable %s" "Włącz:  systemctl --user enable %s"
m dr.tv_ok "Selected TV: %s" "Wybrany TV: %s"
m dr.tv_warn "No TV selected yet" "Nie wybrano jeszcze telewizora"
m dr.tv_hint "Run miracast-menu while the TV shows Smart View / screen mirroring." \
	"Uruchom miracast-menu, gdy TV pokazuje Smart View / duplikowanie ekranu."
m dr.dbus_ok "python3 dbus module present (TV scan in the menu works)" "Moduł python3 dbus jest (skan TV w menu działa)"
m dr.dbus_warn "python3 module 'dbus' missing - the TV scan in miracast-menu will not work" \
	"Brak modułu python3 'dbus' - skan TV w miracast-menu nie zadziała"
m dr.dbus_hint "Install python3-dbus." "Zainstaluj python3-dbus."
m dr.menu_ok "Menu launcher: %s" "Program menu: %s"
m dr.menu_warn "No menu launcher found (fuzzel, wofi, rofi, bemenu, dmenu)" "Brak programu menu (fuzzel, wofi, rofi, bemenu, dmenu)"
m dr.menu_hint "Install one or set MIRACAST_MENU to a dmenu-style command." \
	"Zainstaluj jeden albo ustaw MIRACAST_MENU na polecenie w stylu dmenu."
m dr.notify_warn "notify-send missing - no desktop notifications" "Brak notify-send - bez powiadomień na pulpicie"
m dr.notify_hint "Install libnotify-bin and a notification daemon (mako, dunst...)." \
	"Zainstaluj libnotify-bin i demona powiadomień (mako, dunst...)."
m dr.notify_ok "notify-send present" "notify-send jest"
unset -f m
