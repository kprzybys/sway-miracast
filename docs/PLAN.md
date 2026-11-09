# Plan: własne źródło Miracast (WFD R1) dla Sway, z minimalnym opóźnieniem

> Plan historyczny (2026-11-08); część wymienionych narzędzi nie została opublikowana. Stan obecny:
> README.md, docs/history.md, sinks/.

Wersja 2 (2026-11-08), po review (§8). Kontekst: `data/probe/capabilities.md`. Język: Rust. Styl: małe narzędzia, każde do jednego zadania.

## 1. Cele i mierniki

| Cel | Miernik | Stan wyjściowy (fluxcast + VA-API) |
|---|---|---|
| Opóźnienie glass-to-glass | kalibracja błyskiem (`flash-calib.sh` + `calib_analyze.py`), mediana z ≥5 błysków | 0,45 s (w tym ~0,39 s bufor TV) |
| Stabilność opóźnienia | rozrzut (max−min) w serii, brak narastania w 30 min | 0,43–0,47 s, nie narasta |
| Odporność łącza | sesja żyje ≥8 h; automatyczny powrót po zerwaniu bez udziału człowieka, gdy TV czeka w Smart View | ręcznie / nadzorca w bashu |
| **Opóźnienie sinka (TV)** | czas od wysłania ostatniego pakietu RTP klatki N do jej pojawienia się na TV (kamera, po kalibracji opóźnienia kamery) | nieznane — „0,39 s” to reszta z odejmowania, nie pomiar |
| Internet bez przerw | `linkmon`: ≤1 P2P find ≤10 s na sesję, 0 s utraty STA poza nim | ~6 s przerwy (AP rozłączył w trakcie P2P find) |
| Koszt | CPU < 50% jednego rdzenia przy 1080p30 | ffmpeg ~200% (kopie surowych klatek przez CPU) |

Cele nie-funkcjonalne: zero kolejek bez limitu (drop zamiast kolejkowania), każdy stan z deadline'em,
każda decyzja w logu (JSON lines + journald), da się testować bez TV (atrapa sinka).

## 2. Ograniczenia znane z sondowania

- Sink: H.264 **Constrained Baseline**, poziom 4.2, CEA do 1080p60 (native 1080p60), brak 50/25/24 Hz;
  audio LPCM/AAC stereo; HDCP 2.1 (pomijamy); UIBC (HIDC). R2/H.265: brak.
- Rozszerzenia z potencjałem na opóźnienie: `wfd_sec_low_latency_mode: off` (Samsung, brak publicznej
  dokumentacji), `microsoft_latency_management_capability: supported` (MS-WFDPE: wartości `low|normal|high`,
  `low` = sink SHOULD < 50 ms od ostatniego pakietu klatki do renderu).
- Sink pyta o zgodę przy pierwszym połączeniu (flaga „session available” = 0 w tym czasie), wychodzi ze
  Smart View po nieudanej próbie, łączy RTSP ~10–15 s po starcie grupy, IP 10.42.0.103 z DHCP NM.
- Radio: `managed + P2P-GO`, ≤2 kanały. **WPS nie kończy się, gdy STA jest na innym kanale.** Działa:
  STA na 2,4 GHz, GO na tym samym kanale (wpa_supplicant sam wybiera kanał STA).
- Uprawnienia: NM D-Bus (polkit) dostępny dla użytkownika; **wpa_supplicant D-Bus zabroniony** (wymagałby
  polityki D-Bus instalowanej przez sudo). Persistent groups/`p2p_invite`/`freq=` tylko przez wpa_supplicant.
- Środowisko: Sway 1.9 (wlr-screencopy v1 z dmabuf; ext-image-copy-capture dopiero od 1.10),
  xdg-desktop-portal-wlr (ScreenCast monitor), PipeWire, GStreamer 1.24 z `vah264lpenc`, Rust 1.99.

## 3. Architektura (po review: 2 procesy + biblioteka)

```
                         Unix socket (JSON lines): status, komendy, zdarzenia
 wfd-ctl (podkomenda) ─────────────┐
                                   ▼
 ┌──────────────────────────────────────────────┐   stdin/argumenty, sygnały    ┌──────────────────────┐
 │ wfd-source (tokio)                           │ ─────────────────────────────▶│ wfd-media (dziecko)  │──UDP RTP/TS──▶ TV
 │  p2p (NM D-Bus, zbus) + rtsp (M1–M16)        │ ◀── statystyki (JSON lines) ──│ capture→encode→mux   │
 │  + maszyny stanów + socket sterujący         │                               │ →RTP, wątek std      │
 └──────────────────────────────────────────────┘                               └──────────────────────┘
        ▲ TCP 7236 (sink łączy się do nas)                         ▲ PipeWire/dmabuf → VA-API (GStreamer)
 wfd-proto (crate bez I/O): RTSP, parametry WFD, maski, wybór trybu, maszyny stanów jako czyste fn step()
 linkmon (bash, obserwacja) · latency-bench (Python + kamera) · wfd-sink-sim (test: odtwarza odpowiedzi z logów)
```

- **`wfd-source`**: jeden proces sterujący. Osobne procesy dla P2P i RTSP to za dużo dla jednej osoby i
  wymagałyby własnego kanału zdarzeń; podział zostaje na poziomie modułów i maszyn stanów.
- **`wfd-media`**: osobny proces, bo awaria VA-API/GStreamera nie może zrywać RTSP (restart media + IDR
  bez renegocjacji). GStreamer tylko do capture→encode (appsink); **mux TS + RTP + pacing we własnym
  kodzie Rust** na dedykowanym wątku (blokujący `UdpSocket`, DSCP, bez alokacji w pętli). Powód:
  `mpegtsmux` nie pozwala sterować relacją PTS–PCR, a to główna hipoteza o buforze TV.
  Zalążek: **`wfd-tx`** (stdin: H.264 Annex B → TS/RTP/UDP), składany potokiem z `gst-launch`.
- **`wfd-proto`**: tylko to, czego potrzebuje sesja; testy na prawdziwych logach z sondy.
- Narzędzia pomiarowe zostają w Pythonie (kamera/OpenCV); `linkmon` w bashu.
- Kontrakty: zdarzenia i statystyki jako JSON lines z polami `ts`, `event`, `state`, `frame`, `elapsed_ms`.

## 4. Strategia minimalizacji opóźnienia

Najpierw **izolacja bufora sinka** (E0b), potem decyzje. Pomiar: plansza z wyraźnym błyskiem co N klatek;
`wfd-tx` loguje czas wysłania ostatniego pakietu RTP każdej klatki; kamera (znaczniki wallclock) widzi TV;
opóźnienie kamery kalibrowane osobno błyskiem ekranu laptopa na ścianie. Sink = t_kamera(N) − d_kamera − t_send(N).

Kolejność eksperymentów (największa informacja najpierw), każdy mierzony tak samo, ≥5 błysków:
1. Baseline własnego potoku: tylko wideo, 1080p30, PTS ≈ PCR. Czy 0,45 s się utrzymuje?
   (Sprawdzić, czy fluxcast nadawał audio — wpływ A/V sync.)
2. `wfd_sec_low_latency_mode` (`on`/`1`/`enable`) i `microsoft_latency_management_capability: low`
   — w M4 oraz po PLAY, osobno.
3. **Sweep PTS−PCR** (0/40/100/200/400 ms): liniowa zależność = TV honoruje znaczniki i bufor da się zbić;
   płasko = bufor stały, zostają flagi vendorowe.
4. 60 vs 30 fps na własnym potoku (bufor liczony w klatkach?).
5. Audio: brak / LPCM / AAC.  6. `wfd_sec_view_mode` full.  7. slices 1/4.
8. CBR z wypełnieniem null-packetami vs VBR (stały strumień bywa warunkiem „trybu live”).
(VBV wykluczony wcześniej: 16 Mbit ≈ 1 Mbit.)

Po stronie nadawcy (porządek, mniejszy zysk): `vah264enc` CBR (na UHD 620 `vah264lpenc` ma tylko CQP),
`b-frames=0`, `ref-frames=1`, `cabac=false`, długi GOP + IDR na żądanie, `target-usage=7`, kolejki
`max-size-buffers=1 leaky=downstream`, dmabuf bez kopii, wysyłka natychmiast, DSCP 34, `power_save off`.
Instrumentacja: znacznik przy capture, po encode, przy wysyłce (p50/p99 co sekundę).

## 5. Odporność: zasady

- **Maszyna stanów w każdym demonie**, stan i powód w logu i w `STATUS=` (sd_notify).
  `wfd-p2p`: `Idle → Discover → ChannelGuard → Forming → AwaitConsent → Up → Recovering`.
  `wfd-rtsp`: `Listen → Capabilities(M1–M3) → Negotiate(M4) → Setup(M5–M7) → Playing → Teardown`.
- **Deadline per stan** (np. Discover 10 s, Forming 25 s, AwaitConsent 120 s, M1–M7 15 s,
  keepalive: z `Session: timeout=` lub 60 s, ping co 0,5×).
- **Błędy przejściowe vs trwałe:** przejściowe → ponowienie z backoffem wykładniczym + jitter (5 s → 60 s);
  trwałe (brak zgody, brak peera po N próbach) → kod wyjścia `RestartPreventExitStatus=`, powiadomienie
  dla człowieka, bez pętli. Circuit breaker: `StartLimitBurst`.
- **Idempotentny start:** każda próba zaczyna od sprzątania (usunięcie starej grupy/połączenia NM,
  zabicie grupy procesów media).
- **Strażnik kanału:** formowanie grupy tylko, gdy STA jest na 2,4 GHz (kanał 1/6/11) lub odłączone
  świadomie; w trakcie sesji blokada kanału STA w profilu (tymczasowa, zawsze cofana).
- **Find minimalny:** peer z cache NM bez find; inaczej find ≤10 s i stop zaraz po znalezieniu.
- **Liveness:** M16 + statystyki wysyłki; brak odpowiedzi 2× → Recovering. Watchdog systemd.
- **Drop zamiast kolejki** w danych; po dropie IDR.
- **Żądania od sinka**: TEARDOWN (M8), SET_PARAMETER (`wfd_sec_view_mode`, `wfd_idr_request`),
  GET_PARAMETER — odpowiedź 200 w < 5 s, nieznane parametry potwierdzać.
- **Ekran zablokowany (swaylock)**: czarna plansza albo PAUSE — prywatność (monit hasła widoczny na TV dla innych).
- **Zmiany wyjść Sway / rozdzielczości**: skalowanie na GPU do trybu z M4, bez renegocjacji.
- **Suspend/resume**: TEARDOWN przed uśpieniem (`sleep.target`), ponowne zestawienie po.
- **Adres P2P stały** (zgoda TV jest per urządzenie; randomizacja MAC = monit przy każdym połączeniu).
- **Roaming STA**: profil z `band bg` (bez przypinania kanału — zostawał po crashu); wykrywanie zmiany kanału.
- **Firewall jako część instalacji**: reguły ufw dla 10.42.0.0/24 (7236/TCP i port RTP/UDP sinka).
- **Stan „do cofnięcia” w pliku** + sprzątanie przy starcie (każde połączenie NM `wifi-p2p`).
- **Logi:** JSON lines na stdout (journald), pola `state`, `event`, `peer`, `attempt`, `elapsed_ms`.

## 6. Etapy

| Etap | Zakres | Kryterium „gotowe” |
|---|---|---|
| E0 (zrobione) | sondy, `linkmon`, podtrzymywacz (fluxcast pod systemd), `wfd-proto` (parser M1–M4) | TV w sesji, monitoring w journald, testy zielone |
| E0b (zrobione) | sesja do PLAY (skrypt w Pythonie, uprząż eksperymentów) + `wfd-tx` (Rust, mux TS/RTP z kontrolą PTS−PCR) + plansza z błyskami z `gst-launch` + pomiar izolujący sink; eksperymenty §4.1–8 | tabela wyników; decyzja go/no-go i ustawienia domyślne |
| E1 (prototyp) | `wfd-source`: RTSP w Rust (negocjacja, keepalive, żądania sinka), uruchamia `wfd-media` z planszą; `wfd-sink-sim` | plansza przez Rust 10 min; testy na logach |
| E2 (prototyp, shm) | `wfd-media`: PipeWire/dmabuf → vah264enc → appsink → `wfd-tx` w procesie; instrumentacja; czarna plansza przy swaylock | mirror; CPU < 50%; opóźnienie ≤ E0b + 0,05 s |
| E3 (kod, do testu na TV) | P2P przez zbus w `wfd-source`; unity systemd (notify/watchdog, dev.lock, sleep hook, ufw); wycofanie podtrzymywacza | 8 h bez interwencji; powrót bez człowieka |
| E4 (opcja) | wpa_supplicant D-Bus (polityka): persistent group, `freq=`, invitation | tylko gdy E3 pokaże, że NM nie wystarcza |

Pomysły poza planem: UIBC (klawiatura/dotyk z TV), własny capture wlr-screencopy + VA bez GStreamera.

Praca przy działającej usłudze: skrypty testowe tworzą `$XDG_RUNTIME_DIR/sway-miracast/dev.lock`
(`tools/devlock.sh`) i zatrzymują usługę (unit ma `ConditionPathExists=!…dev.lock`). Każda nieudana próba wyrzuca TV ze Smart View
→ sesje testowe seriami, nie pojedynczo.

## 7. Ryzyka

| Ryzyko | Skutek | Ograniczenie |
|---|---|---|
| Bufor TV niezależny od nas | cel < 0,2 s nieosiągalny | wcześnie (E0b) sprawdzić parametry sinka; raportować uczciwie |
| `pipewiresrc` przez portal wymaga wyboru ekranu (dialog) | brak automatyzacji | konfiguracja `chooser_type=none`/`output_name` w xdg-desktop-portal-wlr albo własny wlr-screencopy |
| Brak nagłówków dev (gstreamer, libva) | nie zbuduje się gstreamer-rs | lista apt dla użytkownika; E1 nie wymaga |
| NM wifi-p2p bez kontroli kanału/persistent | gorszy powrót | E4 z polityką D-Bus |
| Konflikt z podtrzymywaczem | testy kolidują | stop/start unitu w skryptach testowych |
| AP sieci współdzielonej rozłącza STA przy find | przerwy w internecie | find tylko gdy peer nieznany, ≤10 s |

## 8. Review (2026-11-08) — co zmieniono
- „Bufor TV 0,39 s” to reszta z odejmowania (pomija kolejki fluxcasta i ffmpeg) → dodano pomiar izolujący sink (E0b) i miernik w §1.
- Eksperymenty sinka przeniesione na początek (decydują o sensie projektu).
- 3 demony → 2 procesy (`wfd-source`, `wfd-media`) + biblioteka; `latency-bench` zostaje w Pythonie; UIBC poza planem.
- Własny mux TS/RTP (kontrola PTS−PCR, pacing, instrumentacja); GStreamer tylko capture→encode.
- Dodane przypadki: żądania sinka, swaylock, zmiany wyjść, suspend, stały adres P2P, roaming, ufw, stan do cofnięcia.
- E1 (parser) nie jest już etapem samym w sobie — parser rośnie razem z sesją.
- Podtrzymywacz: dev.lock, sprzątanie wszystkich połączeń wifi-p2p, bez blokady kanału w profilu.
- Odrzucone: brak. Zmodyfikowane: eksperyment E0b napędza Python (uprząż), ale mux/wysyłka od razu w Rust (`wfd-tx`), zgodnie z wyborem języka.

## 9. Zmiany po pomiarach (2026-11-08)

- Wyniki (pomiar kamerą, 2026-11-08): TV dodaje ~75–120 ms, **ignoruje PTS** i **buforuje liczbę klatek**
  (nie czas) → źródło wysyła ze stałą częstotliwością, domyślnie **1080p60**; od ekranu do ekranu ~90 ms.
- §4 eksperymenty 2–3 (PTS−PCR) i tryby niskiego opóźnienia: bez efektu, nie są domyślne.
  Aktualizacja 2026-11-09: `wfd_sec_low_latency_mode: on` wysłane po PLAY działa — docs/history.md §4.
- `wfd-media`: zamiast PipeWire/portalu bezpośrednio **wlr-screencopy** (sway 1.9, bez okna wyboru),
  na razie shm (~36 % jednego rdzenia przy 60 kl./s); dmabuf jako optymalizacja.
- `wfd-source`: **wątki i blokujący zbus zamiast tokio** — kilka pętli z terminami nie potrzebuje
  runtime'u asynchronicznego. Dane: procesy potomne `wfd-media | wfd-tx` (jedno narzędzie, jedno zadanie;
  awaria mediów nie zabija sterowania).
- TV kończy sesję po ~25 s bez wideo i po TEARDOWN wychodzi ze Smart View → pomiary w jednej
  trzymanej sesji (skrypty eksperymentów), nigdy TEARDOWN między eksperymentami.
- Podtrzymywacz fluxcast wycofany; usługa docelowa `sway-miracast-source` (Type=notify, watchdog).
