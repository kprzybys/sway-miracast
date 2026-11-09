# Tizen / Samsung HG50BU800 — badanie: czas widoczności P2P sinka (2026-11-08)

Legenda: [DOC] udokumentowane, [INF] wniosek, [GAP] nie znaleziono / niezweryfikowane.

## Najważniejszy wniosek (do użycia w testach)

- [DOC] Tizenowy wifi-direct-manager (demon P2P na TV, jeśli TV go używa — [INF]) nie ma własnego
  timera „przestań być widoczny”. „Listen-only” to wywołanie StartDiscovery(mode=listen, timeout)
  z czasem podanym PRZEZ KLIENTA (aplikację TV), przekazanym wprost do wpa_supplicant `P2PDevice.Listen(timeout)`.
  Źródło: https://git.tizen.org/cgit/platform/core/connectivity/wifi-direct-manager/tree/plugin/wpasupplicant/ctrl_iface_dbus/wfd-plugin-wpasupplicant.c?h=tizen_6.5
  (`dbus_set_method_param(&params, "Listen", ...); value = g_variant_new("(i)", param->scan_time);`)
- [DOC] wpa_supplicant mapuje „Listen bez limitu” (timeout=0) na 3600 s:
  https://w1.fi/cgit/hostap/plain/wpa_supplicant/p2p_supplicant.c (wpas_p2p_listen: `timeout = 3600;`, ok. linii 9010)
- [INF] Czas widoczności TV zależy więc od timeoutu zakodowanego w zamkniętej aplikacji/menedżerze Screen Mirroring
  Samsunga, a nie od żadnego domyślnego timera Tizena. Żadne pole w kodzie open-source nie pokazuje „inactivity timeout
  resetowanego probe requestami”. Probe Request odbierany w Listen nie przedłuża `P2PDevice.Listen`
  (timer wpas_p2p_long_listen_timeout jest ustawiany raz w wpas_p2p_listen) [INF z p2p_supplicant.c].
- [INF] Pomiar (>6 min przy p2p_find co 6 s) może oznaczać, że to NIE nasz find/probe przedłuża Listen TV, tylko
  TV ma dłuższy timer (np. 5–10 min) albo listen jest odnawiany w pętli — rozróżnienie: test ciszy (bez probe) przez
  >10 min i log czasu ostatniego P2P-DEVICE-FOUND z TV.
- Ext Listen (cykliczny listen niezależny od find) w wpa_supplicant: `p2p_ext_listen <period> <interval>`
  https://w1.fi/cgit/hostap/plain/wpa_supplicant/README-P2P — jeśli TV go używa, będzie widoczny w beaconach/probe resp.
  w oknach co `interval` ms; [GAP] brak potwierdzenia, że Samsung to włącza (sprawdź IE P2P: „Extended Listen Timing”).

## 1. Firmware

- [DOC] Firmware HG50BU800EU jest do pobrania ze strony wsparcia Samsung (EU). Paczka USB: `T-NKLBDEUC_1720.7_USB.zip`
  (kwiecień 2026), starsza 1715.1:
  https://www.samsung.com/hu/support/model/HG50BU800EUXEN/ ; https://www.samsung.com/ch_fr/support/model/HG50BU800EUXEN
  Plik pobierany przez stronę wsparcia modelu. Warianty „USB” oraz „BEM” (BEM = aktualizacja przez menedżera
  urządzeń — [INF]). Bliźniak konsumencki UE50BU8000 — [GAP] nie zweryfikowano listingu.
- [DOC] Format: katalog z `IMAGE/upgrade.msd` na USB (nazwa katalogu = nazwa firmware, np. T-KTSUABC):
  https://www.samsung.com/in/support/tv-audio-video/how-to-update-your-smart-tv-to-the-latest-software/
- Obraz firmware jest szyfrowany; nie próbowaliśmy go odszyfrować i wybraliśmy obserwację sieciową.
- Prawo (UE) [INF, nie porada prawna]: dekompilacja dla interoperacyjności dozwolona w wąskim zakresie
  (art. 6 dyrektywy 2009/24/WE, https://eur-lex.europa.eu/eli/dir/2009/24/oj), ale tylko gdy informacje nie są
  dostępne inaczej (tu: specyfikacja WFD/P2P + obserwacja na radiu wystarcza), a pozyskanie klucza/obejście
  zabezpieczeń może naruszać inne przepisy; wniosek: preferuj obserwację sieciową.

## 2. opensource.samsung.com

- [GAP] Portal za Cloudflare (403 / „Just a moment” dla klientów automatycznych); listing BU8000 do sprawdzenia
  w przeglądarce. Adres wyszukiwania: https://opensource.samsung.com/uploadSearch?searchValue=UE50BU8000
  (też wzorzec `.../uploadList?menuItem=mobile|tv` — [INF], niezweryfikowane). Wynik: `docs/samsung-oss-research.md`.
- [INF] Samsung publikuje paczki GPL/LGPL dla TV per model/region (np. nazwy typu „UE50BU8000”, „HG50BU800”), zwykle:
  kernel Linux, toolchain/busybox, biblioteki GPL/LGPL (glib, dbus, gstreamer) i — jeśli jest GPL/BSD — wpa_supplicant/hostapd
  (BSD, więc Samsung nie musi go publikować). Zamknięte: menedżer Screen Mirroring, aplikacja sinka, Samsung WFD stack.
  Oczekuj: wpa_supplicant w paczce tylko jako fragment lub wcale [GAP]. Sprawdź wersję wpa_supplicant (p2p_find/listen
  zachowanie) przez `wpa_supplicant` w paczce lub po P2P IE.

## 3. Kod open-source Tizen (wersja: branch `tizen_6.5`, snapshot z 2021-10; Tizen 6.5 TV Samsunga jest jej pochodną [INF])

Repozytoria (git.tizen.org, cgit; GitHub-mirror pod github.com/Samsung/... nie istnieje — 404):
- https://git.tizen.org/cgit/platform/core/connectivity/wifi-direct-manager (branch tizen_6.5, accepted/tizen_6.5_unified)
- https://git.tizen.org/cgit/platform/core/api/screen-mirroring (capi-media-screen-mirroring)
- https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd (sink GStreamer/RTSP)
- `wifi-direct-plugin-wpasupplicant` jako osobne repo: 404; plugin jest w wifi-direct-manager/plugin/wpasupplicant/ctrl_iface_dbus.

Cytaty (wifi-direct-manager, ?h=tizen_6.5; baza URL
https://git.tizen.org/cgit/platform/core/connectivity/wifi-direct-manager/tree/):
- Listen/Find z timeoutem od klienta: `src/wifi-direct-manager.c` (wfd_manager_start_discovery):
  `if (mode) param.scan_mode = WFD_OEM_SCAN_MODE_PASSIVE; ... param.scan_time = timeout;`
- Plugin: `plugin/wpasupplicant/ctrl_iface_dbus/wfd-plugin-wpasupplicant.c` ws_start_scan: `"Listen"` -> `g_variant_new("(i)", param->scan_time)`;
  dla Find: `"Timeout", g_variant_new_int32(param->scan_time)` (tylko gdy scan_time != 0).
- Brak ponownego listen: `src/wifi-direct-method.c` (~l.178): `WDS_LOGW("Wi-Fi Direct device is already visible, do not start listen");` (gdy GO)
- Timer sesji łączenia 120 s: `src/wifi-direct-session.c` (~l.165): `session->timer = g_timeout_add(120000, (GSourceFunc) _session_timeout_cb, NULL);`
  po wygaśnięciu emituje sygnał „Connection” z `WIFI_DIRECT_ERROR_CONNECTION_TIME_OUT` (`session_timer = 120` w wfd_manager_init).
- Exit timer demona 120 s (tylko tryb on-demand, gdy 0 klientów i stan deactivated): `src/wifi-direct-manager.c` (~l.1820):
  `manager->exit_timer = g_timeout_add(120000, (GSourceFunc) _wfd_exit_timeout_cb, manager);`
- Auto-usuwanie grupy: `manager->auto_group_remove_enable = TRUE;` (wifi-direct-manager.c ~l.1797) i
  `wfd_util_is_remove_group_allowed()` (wifi-direct-util.c ~l.1957) — grupa jest zdejmowana po zerwaniu/porażce [INF].
- Persistent group: plugin ma `ws_get_persistent_groups`, `ws_remove_persistent_group`, `ws_set_persistent_reconnect`;
  [GAP] w tym kodzie brak logiki „po nieudanym GO negotiation zdeaktywuj” — decyzja o deaktywacji należy do klienta.
- Zdarzenia: plugin subskrybuje `FindStopped`/`PersistentGroupAdded|Removed` z wpa_supplicant (wfd-plugin-wpasupplicant.c ~l.3270, 3384).
- Wniosek [INF]: „po nieudanej formacji grupy lub TEARDOWN TV wychodzi z trybu mirroring” to zachowanie aplikacji
  Samsung (wywołanie deactivate/stop listen), nie wynika z kodu managera/wpa_supplicant.

Sink (screen-mirroring / libmm-wfd, tizen_6.5): to tylko warstwa RTSP/media; żadne wywołanie wifi-direct, brak timerów widoczności.
- libmm-wfd `mm_wfd_sink_ini.c`: `#define DEFAULT_STATE_CHANGE_TIMEOUT 5 /* sec */` (timeout zmiany stanu pipeline).
- capi `src/scmirroring_primary_sink.c`: `#define TIMEOUT_SEC 2` (SO_RCVTIMEO gniazda RTSP).
  Obie: https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd , .../api/screen-mirroring (?h=tizen_6.5).
- Źródło „TIZEN_ERROR_CONNECTION_TIME_OUT” w API: scmirroring_type.h `SCMIRRORING_ERROR_CONNECTION_TIME_OUT`.

## 4. Raporty społeczności

- [GAP] Nie znaleziono raportu o limicie czasu widoczności Samsung Tizen TV jako sinka Miracast ani opisu, jak TV
  decyduje o końcu listen. Szukane: XDA, Samsung Community i ogólne fora (wyszukiwarka, 3 zapytania).
- Słabe tropy: wpa_supplicant z TV Samsung widoczny jako „[TV] Samsung 5”:
  https://forum.manjaro.org/t/best-way-to-screen-share-android-phone-to-manjaro/117940 ;
  GNOME Network Displays „Authorization supplicant timed out” (inny sink):
  https://gitlab.gnome.org/GNOME/gnome-network-displays/-/issues/246 ;
  TV Android, timing P2P handshake, p2p_search_delay:
  https://discuss.cachyos.org/t/bug-wi-fi-direct-miracast-p2p-handshake-disconnects-with-redmi-android-tv-on-linux-cachyos-lts-works-on-arch-official-kernel/34219
- Wzmianka o ustawieniu „Auto Protection Time” jako lekarstwie (niski poziom wiarygodności, blog):
  https://pigeoncast.com/blogs/why-screen-mirroring-not-working-samsung-tv — [INF] to ustawienie dotyczy wygaszania
  ekranu, nie P2P.

## Propozycje testów

1. Cisza: 0 probe przez 10–15 min, potem p2p_find; zapisz czas zniknięcia (wpa_cli `p2p_peers`, `P2P-DEVICE-LOST`).
2. Sprawdź w ramce Probe Response / Beacon z TV P2P IE: Extended Listen Timing (attr 0x1B) i Listen Channel (attr 6).
3. Porównaj długość widoczności z wartościami 120 s / 300 s / 600 s / 3600 s (3600 = limit wpa_supplicant dla „unlimited Listen”).
