# Samsung HG50BU800 (Tizen 6.5, 2022; wariant B2B konsumenckiego UE50BU8000)

Rozpoznanie: RTSP `User-Agent: SVDTZ6_5/SEC-UIBC3`; P2P: name `<TV name>` (ustawiana w TV), ModelNumber `QAQ80`,
pri_dev_type `7-0050F204-1`; WFD dev-info `0x0111` (primary sink, dostępny, HDCP2).
Możliwości: `data/probe/capabilities.md` (H.264 CBP do poziomu 4.2, 1080p60 natywnie, LPCM/AAC,
UIBC, rozszerzenia `microsoft_*`, `wfd_sec_*`).
Ustawienia źródła: 1920x1080p60, CBR 8 Mb/s, `wfd_content_protection: none` + `wfd_audio_codecs` w M4,
stała liczba klatek. Wynik: od ekranu do ekranu ~90 ms (pomiar kamerą, 2026-11-08).

**Źródła wiedzy o tym TV (czytaj przed pracą):**
- `data/probe/capabilities.md` — P2P/WFD IE, pełne M3 (kodeki, `wfd_sec_*`, `microsoft_*`), EDID, §6 ruch w sesji
  (RTCP RR co 1 s na nasz `server_port`+1, SDES TOOL „GStreamer”), model i platforma z `/api/v2/`
  (`HG50BU800EUXEN`, `22_NIKEL_UB1_B2B`). TV udostępnia też zwykłe usługi Samsunga (DLNA, DIAL, API pilota);
  tu nieużywane.
- `docs/tizen-sink-research.md` — wzorcowy odbiornik Tizen 6.5 (libmm-wfd): potok, `pulsesink` slave-method NONE,
  `ts-offset` 250 ms, resync przy rozjeździe ≥40 ms przez ≥1 s; mechanizm zapadki Q11.
- `docs/samsung-oss-research.md` — OSS Samsunga: paczka `22_DTV_NikeL` (§5: dekoder obrazu FREERUN = ignoruje PTS,
  audio DSP sync do STC).
- `docs/tizen-research.md` — widoczność P2P (wifi-direct-manager).

Ustawienie po stronie TV: Ustawienia → Ogólne i prywatność → Opcje ekranu startowego →
wyłączyć „Autorun Multi View Mirroring” (Samsung 2022; w firmware B2B może być niedostępne).

## Zachowania

### Q1: „krótkie okno widoczności” — OBALONE przy braku zakłóceń
- Obserwacja: ekran „czekam na połączenie” zostaje, ale po pewnym czasie skan P2P nie znajduje TV; trzeba wyjść i wejść.
  Ekran widoczny w Smart View to „Przewodnik połączeń” — statyczna instrukcja dla smartfona („Nazwa urządzenia: <TV name>”,
  przycisk „Zamknij”) nałożona na źródło Screen Mirroring; nie jest wskaźnikiem stanu radia.
- Dowód: 2026-11-08, skan 5 s bez urządzeń przy widocznym ekranie parowania przez kilka minut (surowe zdarzenia P2P
  z wpa_supplicant).
- Odtworzenie: wejść w Smart View, nie łączyć, skanować co 10 s — do zmierzenia długość okna.
- Specyfikacja: brak (harmonogram Listen ustala firmware).
- Pomiar 2026-11-08 (`tools/tvwindow.py`, ok. 19 min):
  ciągłe zapytania co 6 s → widoczny 6 min bez przerwy; potem cisza 3 min → widoczny; cisza 6 min → widoczny
  (łącznie ≥ 19 min w Smart View, 9 min bez kontaktu). TV sam nie wysyła Probe Request (0 zdarzeń w ciszy).
- Wniosek: brak krótkiego limitu i brak licznika bezczynności; wcześniejsze „zniknięcia” zbiegały się z naszymi
  nieudanymi próbami (Q2). Kod Tizena 6.5: limit nasłuchu ustala zamknięta aplikacja (wpa_supplicant: 0 → 3600 s),
  `docs/tizen-research.md`.
- Obejście: zbędne; skan 4 s co 4 s zostaje (szybka reakcja), ale do rozważenia rzadszy ze względu na STA.
  Status: obalone; górna granica (≥ 19 min, ≤ 60 min?) niezmierzona.

### Q2: nieudana formacja grupy lub nasz TEARDOWN → TV wychodzi ze Smart View
- Dowód (2026-11-08, log wpa_supplicant i zdarzenia źródła): nieudana formacja grupy, TEARDOWN po pomiarze oraz kilka
  nieudanych połączeń tuż po końcu sesji — za każdym razem TV znikał z odkrywania.
- Specyfikacja: brak reguły (§4.12).
- Obejście: łączenie tylko ze świeżo widzianym TV z bitem dostępności (`nm.rs` `lookup`); po TEARDOWN od TV
  ignorujemy wcześniejsze zobaczenia (`ignore_sightings_for`). Status: obejście w kodzie.

### Q3: ~25 s bez wyświetlalnego wideo → TEARDOWN od TV
- Dowód: przerwa w obrazie przy analizie oraz restart wfd-tx (nowy SSRC/PCR) — w obu przypadkach TEARDOWN po ~25 s.
- Odtworzenie: zatrzymać wysyłkę lub zrestartować strumień RTP/TS w trakcie sesji.
- Specyfikacja: brak (spec. ma tylko keepalive §6.5.1).
- Obejście: powtarzanie ostatniej klatki (`wfd-media`), `wfd-tx` żyje całą sesję (`media.rs`). Status: obejście w kodzie.

### Q4: bit dostępności 0x0111 → 0x0101 przy monicie zgody i tuż po sesji
- Specyfikacja: §4.5 — nie łączyć przy 0b00. Obejście: sprawdzane w `nm.rs`. Status: zgodne ze spec.

### Q5: Multi View (podzielony ekran) na starcie
- **`wfd_sec_low_latency_mode: on`** (SET_PARAMETER w sesji, 2026-11-09): 200 OK, A/V zgrane na oko (wcześniej dźwięk
  spóźniony). Wysyłane automatycznie 3 s po PLAY (session.rs). Pomiar liczbowy jeszcze nie zrobiony.
- Obserwacja: TV zgłasza `wfd_sec_view_mode: multi`; nasze `full` dostaje 200 OK; raz TV przeszedł na full ~1,4 s
  po trzech prośbach.
- Specyfikacja: brak (rozszerzenie Samsunga). Dokumentacja: ustawienie „Autorun Multi View Mirroring”.
- Obejście: prośba o `full` po PLAY i po zgłoszeniu `multi` (maks. 2). Status: hipoteza — nie wiadomo, czy zadziałała prośba, czy ustawienie.

### Q6: TV ignoruje PTS, buforuje stałą liczbę klatek (~1,5–3)
- Dowód: pomiar kamerą 2026-11-08 (PTS +100…400 ms bez efektu; 2,9 kl./s → 592 ms, 60 kl./s → ~90 ms).
- Obejście: stała liczba klatek, 60 kl./s. Status: potwierdzone pomiarem.

### Q7: prośba o IDR po starcie (`wfd_idr_request`)
- Obejście: SIGUSR1 do `wfd-media` → klatka kluczowa. Status: obejście w kodzie.

### Q8: monit „Zezwól” przy pierwszym połączeniu nowego źródła
- Dowód: pierwsza sonda 2026-11-08. Później bez monitu — zapamiętany. Status: potwierdzone.

### Q9: dźwięk LPCM — trzaski przy zamieraniu radia
- Obserwacja (2026-11-08): LPCM 48 kHz działa, synchronizacja OK; trzaski przy muzyce.
- Dowód: (1) taktowanie dźwięku zegarem źródła (`wfd-tx`) zamiast własnego 10 ms → mniej trzasków;
  (2) ping do TV w trakcie sesji: p99 78 ms, maks. 103 ms; po wyłączeniu oszczędzania energii STA
  (`802-11-wireless.powersave 2`, interfejs GO nie pozwala zmienić) p99 59 ms, maks. 74 ms →
  odsłuch: brak trzasków przy zapasie PTS 0 ms.
- Specyfikacja: LPCM wg WFD Tab. 106 (nagłówek A0 06 00 11).
- Obejście: rytm ze źródła, ciągłe PTS; powersave STA wyłączony; zapas PTS dźwięku regulowany w locie
  (`$XDG_RUNTIME_DIR/sway-miracast/audio-delay-ms` + `pkill -HUP -x wfd-tx`), domyślnie −1000 ms
  (`AUDIO_DELAY_MS`, zob. Q10). Status: potwierdzone jedną sesją; TV respektuje PTS dźwięku (Q10).

### Q10: dźwięk nierówny przy „cienkim” strumieniu TS (nieruchomy ekran)
- Obserwacja (2026-11-09): zacięcia dźwięku, gdy odtwarzacz nie jest widoczny (mało danych obrazu).
- Dowód (mikrofon, `tools/audiolat.py --period 0.25 --coded 5`, 120 piknięć): nieruchomy ekran std 13,7–24,5 ms,
  16–44 skoków ≥10 ms, dryf ±40 ms/30 s; duży ruch na ekranie std 1,6 ms, 0 skoków; mały element w ruchu
  gorzej (23 ms); 8 Mb/s UDP **poza** strumieniem bez zmian (16,8 ms) → nie radio, tylko przepływność TS.
- Odtworzenie: nieruchomy ekran + ciąg piknięć, `mux-kbps` 0.
- Obejście: stała przepływność TS z pakietami null (`wfd-tx --mux-kbps-file`): 4 Mb/s 11,5 ms; **8 Mb/s 3,2 ms, 0 skoków**;
  12 Mb/s 3,3 ms. Domyślnie 8000 (`$XDG_RUNTIME_DIR/sway-miracast/mux-kbps`, SIGHUP). Status: potwierdzone.
- Opóźnienie dźwięku: ~770 ms przy PTS +0 po 1 h (rosło), ~440 ms minimum; TV respektuje PTS dźwięku
  (−200 → 580, −400 → 462, −600 → 437 ms). Przy −400 ms obraz ~90 ms → dźwięk ~380 ms za obrazem.
- Po przejściu na null sink (zegar systemowy) i audio-delay −1000 ms offset A/V zmalał (<50 ms, dźwięk lekko za
  obrazem). Domyślne `AUDIO_DELAY_MS` to teraz −1000 ms (`crates/wfd-source/src/media.rs`), `VIDEO_HOLD_MS` 0.
- Stan obecny (2026-11-09): z `wfd_sec_low_latency_mode: on` (Q5, wysyłane automatycznie po PLAY) dźwięk i obraz są
  praktycznie zgrane (ocena na oko i ucho, bez pomiaru liczbowego).
- Tryb „film” (`tools/miracast-film.sh`): obraz wstrzymany w źródle o (dźwięk − obraz). Status: eksperymentalny;
  sprawdzony offline, na TV niesprawdzony; zwykle zbędny od trybu niskiego opóźnienia Samsunga. Stałe opóźnienie
  obrazu odrzucone jako domyślne (opóźnia też pracę przy biurku).

### Q11: bufor dźwięku tylko rośnie (zapadka)
- Dowód (2026-11-09, ok. 8 min, `audiolat.py`): −400 → −300: +112 ms; −300 → −400: tylko −49 ms; −700: 197 ms,
  −1000: 204 ms (dno), z powrotem −400: 518 ms. Przy −1000 dalej pełznie: 204 → 274 ms w 7 min.
- Wniosek: TV wyrównuje do PTS przy skoku do przodu (czeka), skoku wstecz nie nadrabia w pełni; nie stroi się do PCR.
- Obejście: brak pewnego. Unikać skoków PTS dźwięku (wfd-tx przestawia PTS tylko przy zmianie opóźnienia). Status: otwarte.

## Otwarte
- Zaproszenie do zapamiętanej grupy (P2P invitation, §4.13 opcjonalne) — czy TV je przyjmuje poza Smart View (E4).
- `wfd_sec_view_mode: full` w M4 — niesprawdzone.
