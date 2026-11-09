# Metoda: specyfikacja jako minimum, profil odbiornika jako prawda o urządzeniu

Specyfikacja mówi, co jest dozwolone; odbiornik (producent, model, firmware, konfiguracja instalatora)
decyduje, co faktycznie działa. Dlatego:

1. **Specyfikacja = minimum wspólne.** Źródło spełnia terminy i reguły WFD/RTSP/P2P zawsze, dla każdego
   odbiornika (np. §4.5 bit „sesja dostępna”, §6.5 terminy RTSP, keepalive < timeout − 5 s).
2. **Profil odbiornika = zachowania konkretnego modelu.** Jeden plik na model w `sinks/`
   (szablon: `sinks/README.md`). Rozpoznanie po `User-Agent` z RTSP i danych P2P (model, WFD IE).
   Każde zachowanie (Q1, Q2, …) ma: obserwację, dowód (log, pomiar, ścieżka), sposób odtworzenia,
   obejście w kodzie i status. Kod nie zawiera obejść bez wpisu w profilu.
3. **Najpierw odtwórz, potem obchodź.** Zachowanie trafia do kodu dopiero, gdy umiemy je wywołać celowo
   (np. restart strumienia RTP → TEARDOWN po ~25 s). Inaczej „naprawiamy” zbieg okoliczności.
4. **Obejście domyślnie wąskie.** Działa dla profilu, który go wymaga; ogólne tylko, gdy jest zgodne ze
   specyfikacją i nie szkodzi innym odbiornikom (np. stała liczba klatek).
5. **Ciągły zapis.** Surowe zdarzenia P2P, journald `wfd-source` (JSON), pomiary z opisem metody
   (lokalnie, poza publicznym repo). Pomiar bez opisu metody nie jest wynikiem.
6. **Pomiar przed zmianą, pomiar po zmianie** — ta sama metoda, w tej samej trzymanej sesji, gdy się da.
7. **Nie rób rzeczy, które wyrzucają odbiornik ze stanu gotowości** (TEARDOWN, nieudana formacja grupy,
   restart strumienia) bez potrzeby — koszt to interwencja człowieka.

## Dokumenty źródłowe

Lokalnie w `ref/` (poza repo: prawa autorskie, nie publikujemy).

| Dokument | Wersja | Do czego | Status |
|---|---|---|---|
| Wi-Fi Display Technical Specification (Miracast) | v2.1 (2017); historia: 1.0 2012-08 (R1), 1.1 2014-04 (HDCP 2.2), 2.0 2017-04 (R2), 2.1 2017-07 | RTSP M1–M16, parametry, WFD IE, terminy | lokalna kopia (nie publikujemy) |
| Wi-Fi P2P (Wi-Fi Direct) Technical Specification | v1.5 (2014; WFD odwołuje się do v1.7, niepubliczna) | wykrywanie, Listen/Search, zaproszenia, grupy trwałe | lokalna kopia (nie publikujemy) |
| Wi-Fi Simple Configuration (WPS) | 2.0.8 | parowanie PBC | **brak** — wi-fi.org wymaga konta; zastępczo: P2P §3.1.4.3 i kod hostap |
| RFC 2326 RTSP 1.0 (+ RFC 7826 RTSP 2.0 dla kontekstu) | — | składnia RTSP | `ref/` |
| RFC 3550 RTP, RFC 2250 RTP dla MPEG-2 TS | — | pakietowanie | `ref/` |
| ITU-T H.222.0 (MPEG-2 TS), ITU-T H.264 | 10/2014, 03/2010 wg WFD | mux, profil CBP, poziomy | **brak** — itu.int wymaga konta; zastępczo FFmpeg/GStreamer |
| MS-WFDPE (rozszerzenia Microsoft) | — | `microsoft_*` parametry | `ref/` |
| wpa_supplicant README-P2P, NM D-Bus API, wlr-screencopy | — | implementacja po stronie Linuksa | online (źródła upstream) |
| HDCP 2.x | — | ochrona treści | nie potrzebujemy (wysyłamy bez HDCP) |

Streszczenie pod kątem naszych problemów (z numerami sekcji): `docs/spec-notes.md`.
