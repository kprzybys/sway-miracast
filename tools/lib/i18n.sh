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
	"Brak programu menu (fuzzel, wofi, rofi, bemenu, dmenu) — wybór telewizora niedostępny. Ustaw MIRACAST_MENU."
m menu.rescan "Scan again" "Skanuj ponownie"
m menu.stop "Laptop only (disconnect the TV)" "Tylko laptop (rozłącz telewizor)"
m menu.scanning "Looking for TVs... (the TV must have Smart View / screen mirroring on)" \
	"Szukam telewizorów… (TV musi mieć włączone Smart View)"
m menu.scan_progress "Looking for TVs (%s s, found: %s)... the TV must have Smart View on" \
	"Szukam telewizorów (%s s, znaleziono: %s)… TV musi mieć włączone Smart View"
m menu.scan_done "Scan finished, TVs found: %s." "Skan zakończony, znalezione telewizory: %s."
m menu.remembered "remembered" "zapamiętany"
m menu.found_now "visible now" "widoczny teraz"
m menu.known_visible "%s - visible" "%s — widoczny"
m menu.known_visible_busy "%s - visible (busy)" "%s — widoczny (zajęty)"
m menu.selected "selected" "wybrany"
m menu.busy "(busy)" "(zajęty)"
m menu.mark_casting "casting" "nadaje"
m menu.mark_waiting "waiting for Smart View" "czeka na Smart View"
m menu.invisible "(not visible)" "(niewidoczny)"
m menu.extend "Separate screen: the TV to the right of the laptop" "Osobny ekran: telewizor po prawej od laptopa"
m menu.mirror "Screen copy: the TV shows the laptop screen" "Kopia ekranu: telewizor pokazuje ekran laptopa"
m menu.prompt "TV" "Telewizor"
m menu.prompt_none "No TV nearby - turn on Smart View" "Brak telewizora w pobliżu — włącz Smart View"
m menu.stop_failed "Could not disconnect the TV (service %s). Try again or log out." \
	"Nie udało się rozłączyć telewizora (usługa %s). Spróbuj jeszcze raz albo wyloguj się."
m menu.stopped "Disconnected. Miracast stays off until you pick a TV." "Rozłączono. Miracast wyłączony do następnego wyboru telewizora."
m menu.now_casting "Casting to %s" "Nadaję na %s"
m menu.waiting_for "Waiting for %s - turn on Smart View on the TV." "Czekam na %s — włącz Smart View na telewizorze."
m menu.devlock "Miracast is paused for development; nothing will connect until it is resumed (devlock off)." \
	"Miracast jest wstrzymany na czas prac nad programem — nic się nie połączy, dopóki go nie wznowisz (devlock off)."
m menu.confirm "Stop casting to %s and switch to %s?" "Przerwać nadawanie na %s i przełączyć na %s?"
m menu.yes "Yes, switch" "Tak, przełącz"
m menu.no "No, stay" "Nie, zostań"
m menu.unknown "unknown" "nieznany"
m menu.start_failed "Could not start Miracast for %s (service state: %s). Details: journalctl --user -u %s" \
	"Nie udało się włączyć Miracasta dla %s (stan usługi: %s). Szczegóły: journalctl --user -u %s"
m menu.chosen "Selected: %s. Turn on Smart View on the TV." "Wybrano: %s. Włącz Smart View na telewizorze."
m menu.tv_default "TV %s" "TV %s"
m menu.settings "TV settings..." "Ustawienia telewizorów…"
m menu.settings_prompt "Connect on its own at login?" "Łączyć sam po zalogowaniu?"
m menu.settings_none "No remembered TVs yet - pick one first." "Brak zapamiętanych telewizorów — najpierw wybierz jeden."
m menu.auto "connects on its own" "łączy sam"
m menu.on_demand "only when picked here" "tylko po wybraniu tutaj"
m menu.back "Back" "Wstecz"
m menu.set_auto "%s: connects on its own (at login and after the TV turns off)." \
	"%s: łączy się sam (po zalogowaniu i po wyłączeniu telewizora)."
m menu.set_on_demand "%s: connects only when picked in this menu (a TV that wakes from standby stays off)." \
	"%s: łączy się tylko po wybraniu w tym menu (telewizor budzący się z czuwania zostaje wyłączony)."

# --- miracast-status ---------------------------------------------------------------------------------
m st.none "none" "brak"
m st.off "Miracast is off. Click to pick a TV." "Miracast wyłączony. Kliknij — wybierz telewizor."
m st.mode_dup "screen copy" "kopia ekranu"
m st.mode_ext "separate screen" "osobny ekran"
m st.film ", film mode: video +%s ms" ", tryb film: obraz +%s ms"
m st.casting "Miracast: casting to %s (%s, %s%s). Click for the menu (screen copy / separate screen, laptop only, another TV)." \
	"Miracast: nadaję na %s (%s, %s%s). Kliknij — menu (kopia ekranu / osobny ekran, tylko laptop, inny telewizor)."
m st.waiting "Miracast: waiting for %s (%s). Turn on Smart View on the TV. Click to pick a TV." \
	"Miracast: czekam na %s (%s). Włącz Smart View na telewizorze. Kliknij — wybierz telewizor."
m st.no_tv "Miracast: no TV selected. Click to pick a TV." "Miracast: nie wybrano telewizora. Kliknij — wybierz telewizor."
# what wfd-source is doing (its systemd StatusText, English, mapped in miracast-status)
m st.s_start "starting" "uruchamianie"
m st.s_waiting "the TV is not visible yet" "telewizora jeszcze nie widać"
m st.s_connecting "connecting..." "łączenie…"
m st.s_rtsp "connecting, waiting for the TV to answer..." "łączenie — czekam na odpowiedź telewizora…"
m st.s_no_tv "no TV selected" "nie wybrano telewizora"
m st.s_other "getting the connection ready" "przygotowanie połączenia"
# notifications on wfd-source state changes (wfd-source --notify-cmd, env WFD_EVENT)
m st.notify_waiting "Waiting for %s - turn on Smart View / screen mirroring on the TV" \
	"Czekam na %s — włącz Smart View na telewizorze"
m st.notify_connecting "Connecting to %s..." "Łączę z %s…"
m st.notify_casting "Casting to %s" "Nadaję na %s"
m st.notify_failed "Could not connect to %s - check that Smart View is on" \
	"Nie udało się połączyć z %s — sprawdź, czy Smart View jest włączone"
m st.notify_ended "Disconnected from %s" "Rozłączono z %s"
m st.tv "the TV" "TV"
m st.usage "usage: %s [--notify-event]" "użycie: %s [--notify-event]"

# --- miracast-screen ---------------------------------------------------------------------------------
# %.0s swallows an argument a message does not show (a technical name)
m sc.title "TV screen" "Ekran telewizora"
m sc.no_session "Not connected to a TV - connect first (Mod+Shift+T)" \
	"Najpierw połącz się z telewizorem (Mod+Shift+T)"
m sc.no_source "Miracast is off - connect to a TV first (Mod+Shift+T)" \
	"Miracast jest wyłączony — najpierw połącz się z telewizorem (Mod+Shift+T)"
m sc.write_failed "Could not change what the TV shows (cannot write %s)" \
	"Nie udało się zmienić obrazu na telewizorze (nie można zapisać %s)"
m sc.signal_failed "Could not change what the TV shows; it stays as it was.%.0s" \
	"Nie udało się zmienić obrazu na telewizorze; zostaje jak było.%.0s"
m sc.create_failed "Could not create the separate screen" "Nie udało się utworzyć osobnego ekranu"
m sc.no_sway "Sway does not answer - cannot change the screens" "Sway nie odpowiada — nie można zmienić ekranów"
m sc.extended "%.0sTV: separate screen to the right of the laptop (workspace %s)" \
	"%.0sTelewizor: osobny ekran po prawej od laptopa (pulpit %s)"
m sc.mirrored "TV: screen copy (shows the laptop screen)" "Telewizor: kopia ekranu laptopa"
m sc.shows "TV shows: %s" "TV pokazuje: %s"
m sc.usage "usage: %s [toggle|mirror|extend|status]" "użycie: %s [toggle|mirror|extend|status]"

# --- miracast-film -----------------------------------------------------------------------------------
m fm.write_failed "Could not change the film mode (cannot write %s)" \
	"Nie udało się zmienić trybu film (nie można zapisać %s)"
m fm.no_session "Connect to a TV first (Mod+Shift+T)" "Najpierw połącz się z telewizorem (Mod+Shift+T)"
m fm.no_sigusr1 "The picture on the TV may freeze for a moment after the change" \
	"Obraz na telewizorze może się na chwilę zatrzymać po zmianie"
m fm.off "Desk mode: video without delay" "Tryb biurko: obraz bez opóźnienia"
m fm.running "Film mode measurement already running" "Pomiar trybu film już trwa"
m fm.measuring "Film mode: measuring audio latency (~6 s of beeps on the TV)..." \
	"Tryb film: mierzę opóźnienie dźwięku (ok. 6 s piknięć z telewizora)…"
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
m dr.iw_missing "'iw' not found - cannot check P2P support" "Brak 'iw' — nie można sprawdzić obsługi P2P"
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
	"Brak dostępu do dziennika wpa_supplicant — wykrywanie TV będzie wolniejsze"
m dr.journal_hint "Add yourself to the 'systemd-journal' group: sudo usermod -aG systemd-journal \$USER (re-login)." \
	"Dodaj siebie do grupy 'systemd-journal': sudo usermod -aG systemd-journal \$USER (ponowne logowanie)."
m dr.gst_missing "GStreamer tools (gst-inspect-1.0) not found" "Brak narzędzi GStreamera (gst-inspect-1.0)"
m dr.gst_hint "Install gstreamer1.0-tools and the plugin packages (base, good, bad, vaapi/va)." \
	"Zainstaluj gstreamer1.0-tools i wtyczki (base, good, bad, vaapi/va)."
m dr.gst_ver_ok "GStreamer %s" "GStreamer %s"
m dr.gst_ver_warn "GStreamer %s is older than 1.24" "GStreamer %s jest starszy niż 1.24"
m dr.enc_hw "Hardware H.264 encoding available (%s + vapostproc)" "Sprzętowe kodowanie H.264 dostępne (%s + vapostproc)"
m dr.enc_sw "No VA encoder - software encoding with x264enc (higher CPU load, more latency)" \
	"Brak kodera VA — kodowanie programowe x264enc (większe obciążenie CPU i opóźnienie)"
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
	"Brak modułu python3 'dbus' — skan TV w miracast-menu nie zadziała"
m dr.dbus_hint "Install python3-dbus." "Zainstaluj python3-dbus."
m dr.menu_ok "Menu launcher: %s" "Program menu: %s"
m dr.menu_warn "No menu launcher found (fuzzel, wofi, rofi, bemenu, dmenu)" "Brak programu menu (fuzzel, wofi, rofi, bemenu, dmenu)"
m dr.menu_hint "Install one or set MIRACAST_MENU to a dmenu-style command." \
	"Zainstaluj jeden albo ustaw MIRACAST_MENU na polecenie w stylu dmenu."
m dr.notify_warn "notify-send missing - no desktop notifications" "Brak notify-send — bez powiadomień na pulpicie"
m dr.notify_hint "Install libnotify-bin and a notification daemon (mako, dunst...)." \
	"Zainstaluj libnotify-bin i demona powiadomień (mako, dunst…)."
m dr.notify_ok "notify-send present" "notify-send jest"
unset -f m
