# Tizen Wi-Fi Display SINK: badanie otwartego kodu (a opóźnienie audio w HG50BU800)

Data: 2026-11-09. Gałąź: `tizen_6.5` (git.tizen.org). Opis tego, co jest w kodzie, odróżniony od wniosków.
Uwaga ogólna: TV HG50BU800 uruchamia zamkniętą aplikację Smart View. Publiczny kod to referencja, nie dowód.

## 0. Najważniejsze wnioski (skrót)

1. Publiczny jest tylko "klej" sinka (libmm-wfd). Elementy `wfdsrc` i `wfdtsdemux` (parser RTSP,
   wszystkie parametry `wfd_sec_*`, `wfd_vnd_sec_*`, `microsoft_*`) NIE są w publicznym Tizenie.
   W repo libmm-wfd nie ma ani jednego wystąpienia `wfd_sec`, `wfd_vnd`, `microsoft`, `intel_`.
2. Potok audio sinka (dokumentowane w kodzie): `audio_sink` = `pulsesink`, `provide-clock=FALSE`,
   `buffer-time=100000` us (100 ms), `slave-method=2` (NONE), `ts-offset=250 ms` (ini `sink_ts_offset`),
   `async=FALSE`. Jitterbuffer wfdsrc `latency=10 ms`. Domyślne `alignment-threshold` 40 ms,
   `drift-tolerance` 40 ms, `discont-wait` 1 s (nie ustawiane przez libmm-wfd, więc upstreamowe domyślne).
3. Mechanizm "zapadki" jest wyjaśniony przez `GstAudioBaseSink` (upstream 1.16.2, ten sam w Tizen 6.5):
   przy `slave-method=NONE` próbki są dokładane ciągle do poprzednich, a PTS jest brany pod uwagę dopiero
   gdy odchyłka >= `alignment-threshold` (40 ms) trwa >= `discont-wait` (1 s). Wtedy następuje resync do PTS.
   Resync w przód zostawia ciszę w ringbuforze (opóźnienie rośnie w całości), resync wstecz nie może
   pisać "za" wskaźnikiem odczytu, więc opóźnienie maleje tylko częściowo (jest to wniosek z kodu, patrz 4).
4. Dodatkowo libmm-wfd co 30 s (po pierwszych 60 s) sam koryguje `ts-offset` audio i wideo o średnią
   zmianę "spóźnienia" bufora (`_mm_wfd_sink_check_running_time`), krokami dowolnej wielkości >= 1 ms.
   To drugi kandydat na schodkowe pełzanie (patrz 4).
5. Źródło nie ma w publicznym kodzie ŻADNEGO parametru, który zmienia buforowanie audio sinka.
   `audio_latency`/`video_latency` w M3 to tylko deklaracja sinka (domyślnie 0). RTCP SR od źródła
   prawdopodobnie nic nie zmieni (patrz 6, to wniosek).

## 1. Komponenty sinka (repo i rola)

Sink "mm-wfd-sink" w Tizenie 6.5 składa się z (URL-e cgit, gałąź `tizen_6.5`):

| Komponent | Repo | Rola |
|---|---|---|
| libmm-wfd (mm_wfd_sink) | https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd/?h=tizen_6.5 (wersja 0.3.11 wg `packaging/libmm-wfd.spec`) | budowa potoku GStreamer, ini, kompensacja `ts-offset` |
| capi-media-screen-mirroring | https://git.tizen.org/cgit/platform/core/api/screen-mirroring/?h=tizen_6.5 | API C dla aplikacji (sink: `scmirroring_sink.h`, katalog `test_sink`; także `miracast_server` = źródło) |
| gst-rtsp-server-wfd | https://git.tizen.org/cgit/platform/core/multimedia/gst-rtsp-server-wfd/ | strona ŹRÓDŁA (nie sink) |
| GStreamer 1.16.2 | https://git.tizen.org/cgit/platform/upstream/gstreamer/ (oraz gst-plugins-base/good/bad, wszystkie Version 1.16.2 w spec na `tizen_6.5`) | rtpbin/rtpjitterbuffer (good), audiobasesink (base), mpegtsdemux (bad) |

Element `wfdsrc` (domyślnie `name_of_source = "wfdsrc"`) i `wfdtsdemux` (`name_of_tsdemux`) wołane są
po nazwie z ini (`mm_wfd_sink_ini.c` linie 58-59). Ich źródeł nie znaleziono:
- cgit `?q=wfd` zwraca tylko `gst-rtsp-server-wfd` i `libmm-wfd` (platform/core oraz framework/);
- `platform/upstream/gst-plugins-good/gst/rtsp` na `tizen_6.5` zawiera zwykły `gstrtspsrc.c` bez "wfd";
- `gst-plugins-bad/gst/mpegtsdemux` zawiera zwykły `tsdemux.c` (właściwości tylko `program-number`, `emit-stats`).
Wniosek: wfdsrc/wfdtsdemux to element zamknięty (najpewniej w pakietach Samsunga, nie w Tizen Public).
Samsung TV (SVDTZ6_5, platforma 22_NIKEL_UB1_B2B) ma też swoją warstwę Smart View. Nie sprawdzano GitHub Samsung
(wyszukiwarka webowa nie znalazła `gstwfdrtspsrc`).

## 2. Parametry RTSP rozumiane przez sink

W publicznym kodzie: praktycznie żadne nazwy parametrów RTSP nie występują jako tekst. libmm-wfd przekazuje
konfigurację do `wfdsrc` jako właściwości GObject (`mm_wfd_sink_priv.c`, `__mm_wfd_sink_prepare_source`, linie 2877-3062),
a sam parser M1-M16 jest w zamkniętym `wfdsrc`. Co jest widoczne:

Właściwości `wfdsrc` ustawiane przez libmm-wfd (priv.c):
- `udp-buffer-size = 2097152` (2899), `latency = ini.jitter_buffer_latency` (2900, domyślnie 10 ms),
  `user-agent` (2901, ini, domyślnie ""), `dump-rtsp-message`, `do-request = ini.enable_retransmission` (2910, domyślnie FALSE),
  `enable-pad-probe`, `dump-rtp-data`, `trace-*` (diagnostyka).
- Struktury negocjacji M3: `wfd-audio-codecs` (codec, `audio_latency`, channels, sampling_frequency),
  `wfd-video-formats` (w tym `video_latency`), `wfd-coupled-sink`, `wfd-content-protection`,
  `wfd2-audio-codecs`, `wfd2-video-format-h264`, `wfd2-video-format-h265` (linie 2913-3062).
  Domyślne: audio LPCM|AAC, `audio_latency = 0x0`, 44.1|48 kHz, 2 kanały (`mm_wfd_sink_ini.c` linie 84-87);
  `video_latency = 0x0` (linia 101).
- Sygnały: `update-media-info`, `change-av-format` (3065-3068), `request-pause/resume/close` (441, 464, 490).
- Wiadomości z elementu obsługiwane przez libmm: tylko `GstUDPSrcTimeout` i `GstWFDSessionTimeout` (1058-1075).

Parametry `wfd_sec_*`, `wfd_vnd_sec_*`, `microsoft_latency_management_capability`, `intel_*`:
nie występują w żadnym publicznym pliku. Z kodu nie da się więc stwierdzić, co robią `wfd_sec_low_latency_mode`,
`wfd_sec_view_mode`, `wfd_sec_dmr_support` ani czy zmieniają latency jitterbuffera lub sinka.
W libmm-wfd żaden ini/kod nie ma przełącznika "low latency". To, że TV nie odpowiada na `wfd_vnd_sec_max_buffer_length`,
`wfd_sec_rtp_latency` itp., zgadza się z tym, że to rozszerzenia zamkniętego wfdsrc/aplikacji, nie otwartego sinka.

## 3. Potok audio sinka (dokumentowane w kodzie)

Plik: https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd/tree/src/mm_wfd_sink_priv.c?h=tizen_6.5

Potok główny (3157-3200): `wfdsrc` (RTSP + RTP + jitterbuffer wewnątrz) -> `rtpmp2tdepay` -> `queue ts_queue`
(`max-size-buffers=200000`) -> `valve` -> `wfdtsdemux` -> (po pad-added) valve -> decodebin audio/wideo.
Audio decodebin (3639+): `queue audio_queue` (`max-size-time` 3 s, bytes/buffers bez limitu, linia 3116-3119)
-> (LPCM: `capsfilter` o nazwie `audio_lpcm_filter`, caps S16BE, rate/channels z M4, linie 3330-3340) -> `audioconvert`
(resampler) -> `pulsesink`.

Audio sink (`__mm_wfd_sink_prepare_audiosink`, linie 3482-3498):
```
provide-clock = FALSE
buffer-time   = 100000 us (100 ms)     (nie ini, wartość na sztywno)
slave-method  = 2                      (GST_AUDIO_BASE_SINK_SLAVE_NONE)
async         = ini.audio_sink_async   (domyślnie FALSE)
ts-offset     = ini.sink_ts_offset     (domyślnie 250000000 ns = 250 ms)
```
Nieustawione, więc domyślne audiobasesink (gst-plugins-base 1.16.2 `gstaudiobasesink.c` linie 88-106):
`latency-time` 10 ms (w pulsesink), `alignment-threshold` 40 ms, `drift-tolerance` 40 ms, `discont-wait` 1 s.
`sync` = TRUE (domyślne GstBaseSink; libmm tego nie zmienia).

Wideo (4239-4243): `qos=FALSE`, `async=FALSE`, `max-lateness = 20 ms`, `ts-offset = 250 ms` (to samo ini). Sink wideo `tizenwlsink`.

Domyślne ini (`mm_wfd_sink_ini.c` linie 37-81, plik `/etc/multimedia/mmfw_wfd_sink.ini`, sekcja "general"):
- `jitter buffer latency` = 10 ms, `enable_retransmission` = FALSE
- `enable_reset_basetime` = TRUE, `video_sink_max_lateness` = 20 ms
- `sink_ts_offset` = 250 ms, `audio_sink_async` = FALSE, `video_sink_async` = FALSE
- `name_of_audio_sink` = `pulsesink`, `name_of_audio_resampler` = `audioconvert`, `name_of_lpcm_filter` = `capsfilter`
Sam plik mmfw_wfd_sink.ini NIE jest w repo libmm-wfd (nie ma go w drzewie ani spec). Wartości na prawdziwym TV
mogą się różnić (Samsung potrafi go nadpisać). Publicznie nie jest dostępny.

Zegar i czas bazowy:
- Zegar potoku wybiera pipeline (`GST_MESSAGE_NEW_CLOCK`, priv.c 1025-1043). Audio nie dostarcza zegara (`provide-clock=FALSE`),
  więc sink audio jest "slaved" do zegara potoku, ale z `slave-method=NONE` (nie koryguje dryfu sprzętu).
- `enable_reset_basetime=TRUE`: przy pierwszym buforze `_mm_wfd_sink_reset_basetime` (1469-1525) ustawia `base_time` całego potoku
  na "teraz" zegara potoku. Czyli running time pierwszego bufora = ~0 i dalszy czas odtwarzania =
  (PTS względem pierwszego PTS) + `ts-offset` 250 ms.
- PCR/zegar: libmm-wfd nie ustawia nic dla PCR (wfdtsdemux nie ma tu właściwości). W upstreamowym tsdemux 1.16.2
  (https://git.tizen.org/cgit/platform/upstream/gst-plugins-bad/tree/gst/mpegtsdemux/mpegtsbase.c?h=tizen_6.5 linia ~1445)
  przy segmencie w TIME `calculate_skew = TRUE`: PCR jest porównywany z czasem przybycia (okno 2 s, 512 próbek,
  `mpegtspacketizer.c` 1312-1530) i PTS są korygowane o ten skew. Czy tak działa zamknięty `wfdtsdemux`: NIE WIEM (wniosek).
- Brak kodu, który wstawia ciszę, kasuje audio lub robi resync na poziomie libmm (poza drop valve przy stanach).
  Jedyna korekcja czasu: kompensacja `ts-offset` poniżej.

Jitterbuffer (rtpbin/rtpjitterbuffer, upstream 1.16.2, `rtpmanager/gstrtpbin.c` 297-319, `gstrtpjitterbuffer.c` 132-153):
`latency` 200 ms (libmm nadpisuje na 10 ms przez wfdsrc), `drop-on-latency=FALSE`, `do-lost=FALSE`, `mode/buffer-mode=slave`,
`ntp-sync=FALSE`, `rtcp-sync=always`, `do-retransmission=FALSE`. Jak wfdsrc mapuje swoje `latency` na te właściwości,
tego nie widać (zamknięty/niepubliczny element).

## 4. Co wyjaśnia: zapadka, wzrost schodkowy (wniosek z kodu + wiedza o GStreamer)

A. `GstAudioBaseSink` z `slave-method=NONE` (`gstaudiobasesink.c` render 1800-2110, `get_alignment` 1730-1790):
 - Pozycja zapisu w ringbuforze = `sample_offset` z PTS (po `ts-offset`, `base_time`), ale gdy bufor "pasuje" do poprzedniego
   (`|next_sample - sample_offset| < alignment_threshold` = 40 ms), próbki są dołączane ciągle do `next_sample`
   (PTS jest ignorowany, "align with prev sample"). Odchyłki < 40 ms są więc "połykane" i kumulują się.
 - Gdy odchyłka >= 40 ms utrzymuje się >= `discont-wait` (1 s), jest "Unexpected discontinuity ..., resyncing":
   `align = 0`, zapis w pozycji wyliczonej z PTS. Natychmiastowy resync także po buforze z flagą DISCONT/RESYNC (1928-1935).
 - Skok PTS w PRZÓD: nowa pozycja zapisu leży daleko przed wskaźnikiem odczytu, w ringbuforze powstaje luka
   (cisza), opóźnienie rośnie o całą wielkość skoku. Zgodne z pomiarem ("rośnie w całości").
 - Skok PTS WSTECZ: zapis wypadałby "za" wskaźnikiem odczytu. Kod ma `allow_align = FALSE` gdy
   `sample_diff > headroom && align < 0`, a próbki w przeszłości są odrzucane przy commit do ringbuffera (lub `too_late`).
   Opóźnienie więc maleje najwyżej do tego, co da się nadrobić (bufor ma `buffer-time` 100 ms, a sink dodaje
   ts-offset 250 ms), nie o cały skok. Zgodne z "zapadką" i z ~200 ms przy PTS daleko w przeszłości
   (ts-offset 250 ms minus ~50 ms; liczby zbieżne rzędem wielkości, nie dowód).
B. Pełzanie krokami 25-35 ms (204 -> 238 -> 262 -> 274): dwa kandydaci z kodu.
 1. Resync na `alignment-threshold` 40 ms przy `slave-method=NONE`: jeśli zegar audio sprzętu TV chodzi wolniej niż
    zegar, względem którego wystawiamy PTS (i/lub zegar potoku), to `sample_offset` ucieka do przodu względem `next_sample`;
    po przekroczeniu ~40 ms przez 1 s następuje resync w przód = jeden skok w górę o ~(40 ms minus nadwyżka),
    czyli rzędu 25-40 ms. Okresowość: zależy od dryfu (u nas 7 min dla ~70 ms to dryf ~0.17 ms/s, wiarygodny rząd wielkości
    różnicy zegarów + dostarczanie przez Wi-Fi). Wniosek, nie pomiar na TV.
 2. Kompensacja libmm (`_mm_wfd_sink_check_running_time`, 2326-2545): co 30 s (`COMPENSATION_CHECK_PERIOD`),
    po pierwszych 60 s, liczy średnie "spóźnienie" (running_time minus PTS-z-ts-offset) dla audio i wideo i jeśli zmiana
    >= 1 ms (`COMPENSATION_CRETERIA_VALUE`) przesuwa `ts-offset` OBU sinków o `avgrage_gap_diff`
    (wybiera taką, "która nie powoduje dropów"). Ponieważ wideo ma stałe ~100 ms i ignoruje PTS, a audio ma zegar dryfujący,
    ts-offset może rosnąć w krokach niepowiązanych z naszymi zdarzeniami. Czy Samsung ma ten kod: nieznane.
C. Zapadka na PTS: dla PTS w przód ts-offset odgrywa rolę (running time + 250 ms). Dla wideo
   `max-lateness=20 ms` i `qos=FALSE` (tizenwlsink) pasuje do "TV ignoruje PTS wideo", ale to tylko zgodność, nie dowód.

## 5. Parametry, które źródło może wysłać, a zmieniają buforowanie/sync audio

W publicznym kodzie: żadnego. Co istnieje:
- M3 GET_PARAMETER odpowiedź sinka: `wfd_audio_codecs` ma pole `audio_latency` (domyślnie 0x0) i `wfd_video_formats` pole
  `video_latency` (0x0). Te pola to deklaracje sinka dla źródła (WFD R1: "latency" w jednostkach 5 ms), a nie
  ustawienia sinka. libmm-wfd nigdy nie zmienia buforów na podstawie M4 (sygnały `update-media-info`, `change-av-format`
  tylko budują pipeline dla kodeka/rozdzielczości; `priv.c` 3065-3068).
- Nie ma w kodzie obsługi `wfd_sec_low_latency_mode`, `wfd_sec_rtp_latency`, `wfd_vnd_sec_max_buffer_length`,
  `microsoft_latency_management_capability`. Ich ewentualne działanie jest wyłącznie w zamkniętym kodzie TV.
  TV je ignoruje w M3 (brak odpowiedzi), więc M4 SET_PARAMETER z nimi najpewniej też nic nie da (wniosek).
  Aktualizacja 2026-11-09: `wfd_sec_low_latency_mode: on` wysłane po PLAY działa — docs/history.md §4.
- Wpływ pośredni, wynikający z kodu: (a) PTS/PCR w TS źródła (patrz 4); (b) tempo dostarczania (jitterbuffer 10 ms,
  `do-lost=FALSE`, `drop-on-latency=FALSE` - bufory nie są gubione, tylko wychodzą w kolejności); (c) flaga
  DISCONT na buforze audio wymusza natychmiastowy resync w audiobasesink (1928-1935). Na TV, który używa
  `rtpmp2tdepay` -> demux, DISCONT może powstać po luce numeracji RTP (zagubiony pakiet) albo restarcie TS.

## 6. RTCP: czy Sender Report od źródła coś zmieni (nowy fakt: TV wysyła RR, SDES TOOL "GStreamer")

Dokumentowane: TV używa `rtpbin`/`rtpsession` GStreamera (potwierdza to RR z SDES TOOL=GStreamer; w kodzie libmm to element `wfdsrc`).
Domyślne rtpbin 1.16.2: `ntp-sync=FALSE`, `rtcp-sync=always`, `buffer-mode=slave` (gstrtpbin.c 302-306).
Wnioski (bez weryfikacji na TV):
- SR daje parę NTP<->RTP i służy rtpbinowi do synchronizacji MIĘDZY strumieniami RTP (audio i wideo w osobnych sesjach).
  Tu jest jeden strumień RTP z MPEG-TS (audio i wideo są zmultipleksowane w TS i rozdzielane dopiero w `wfdtsdemux`),
  więc SR nie ma czego synchronizować. Synchronizację A/V robi PTS/PCR w TS, a nie RTCP.
- W trybie `slave` jitterbuffer liczy PTS bufora z czasu przybycia i RTP timestamp (skew względem zegara lokalnego), nie z SR;
  przy `ntp-sync=FALSE` SR i tak nie zmienia znacznika. Skutek: bez SR nic się nie psuje, z SR najpewniej też nic się nie zmieni.
- Wyjątek warty testu: `rtcp-sync=always` + LSR/DLSR. Odsyłanie RR z DLSR=0 jest normalne bez SR. Dodanie SR jest tanie
  (jeden pakiet/s na port+1), więc można spróbować i zmierzyć mikrofonem, ale oczekiwanie: brak efektu.
- Skoro tsdemux (jeśli podobny do upstreamowego) liczy skew PCR względem czasu PRZYBYCIA bufora (mpegtspacketizer calculate_skew),
  to jednostajne tempo wysyłania i PCR zgodny z czasem wysyłki są ważniejsze niż RTCP.

## 7. Czego nie znaleziono

- Źródeł `wfdsrc`, `wfdtsdemux`, listy parametrów RTSP z `wfd_sec_*`/`microsoft_*` (poza libmm-wfd): brak w publicznym Tizenie.
- Rzeczywistego `mmfw_wfd_sink.ini` z TV; podano tylko defaulty z kodu.
- Czy TV używa `pulsesink` (na telewizorach Samsung częściej własny sink audio z `audiotimestamp`/AMP); ini TV może go podmienić.
- GitHub Samsung/mirrory: nie sprawdzono systematycznie (jedna wyszukiwarka, bez trafień).

## 8. Praktyczne wskazówki dla źródła (wnioski, do testu)

1. Trzymać PTS audio ciągłe i monotoniczne względem PCR i wideo, bez skoków (każdy skok >= 40 ms, trwający > 1 s, lub flaga
   DISCONT powoduje resync i trwałą zmianę opóźnienia). Zapadka oznacza: nie "naprawiać" w tył, tylko nie dopuszczać skoków w przód.
2. Taktować PTS audio z liczby wysłanych próbek (48000 Hz), a nie z zegara systemowego w chwili wysyłki, żeby nie wprowadzać
   dryfu względem zegara DAC; kotwiczyć PCR i PTS do jednego zegara. Dryf źródło-TV rzędu 0.1-0.2 ms/s
   daje przy `alignment-threshold` 40 ms schodki ~co kilka minut (hipoteza B1).
3. Pomiary mikrofonem po 30 s i co 30 s (okno kompensacji libmm) rozróżnią kandydata B2 (zmiany co 30 s) od B1 (rzadkie schodki).
4. Test z dodaniem RTCP SR: tanio, spodziewany brak efektu.

## Źródła (git.tizen.org, gałąź tizen_6.5)

- libmm-wfd: https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd/tree/src/mm_wfd_sink_priv.c?h=tizen_6.5
  (linie: 2326-2545 kompensacja, 2877-3062 wfdsrc, 3157-3200 potok, 3482-3498 audio sink, 4239-4243 wideo sink, 1469-1525 base_time)
- libmm-wfd ini: https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd/tree/src/mm_wfd_sink_ini.c?h=tizen_6.5 (linie 37-134 defaulty)
- libmm-wfd struktura ini: https://git.tizen.org/cgit/platform/core/multimedia/libmm-wfd/tree/src/include/mm_wfd_sink_ini.h?h=tizen_6.5
- screen-mirroring: https://git.tizen.org/cgit/platform/core/api/screen-mirroring/?h=tizen_6.5
- audiobasesink: https://git.tizen.org/cgit/platform/upstream/gst-plugins-base/tree/gst-libs/gst/audio/gstaudiobasesink.c?h=tizen_6.5
- rtpbin/jitterbuffer: https://git.tizen.org/cgit/platform/upstream/gst-plugins-good/tree/gst/rtpmanager/?h=tizen_6.5
- mpegtsdemux: https://git.tizen.org/cgit/platform/upstream/gst-plugins-bad/tree/gst/mpegtsdemux/?h=tizen_6.5
