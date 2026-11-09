# Telewizor jako sink Miracast: możliwości (sonda 2026-11-08)

Źródła: odkrywanie P2P przez NM, pełna wymiana RTSP, EDID z `wfd_display_edid`, journal wpa_supplicant
(surowe logi sondy nie są publikowane). Narzędzia: `tools/`.

## 1. Warstwa Wi-Fi Direct (P2P)

| Pole | Wartość | Znaczenie |
|---|---|---|
| Nazwa / producent / model | `<TV name>` / SAMSUNG_ELECTRONICS / ModelNumber `QAQ80` | QAQ80 to oznaczenie platformy, nie model handlowy |
| pri_dev_type | `7-0050F204-1` | kategoria 7 (Display), podkategoria 1 (telewizor) |
| config_methods | `0x188` w trybie oczekiwania, `0x80` po zgodzie użytkownika | 0x100 Keypad, 0x80 Push Button, 0x08 Display; po „Zezwól” zostaje tylko PBC |
| dev_capab / group_capab | `0x24` / `0x0` (po zgodzie `0x8`) | 0x04 concurrent op., 0x20 invitation procedure |
| WFD Device Info | `0x0111`, port 7236, 54 Mb/s | primary sink, sesja dostępna, HDCP 2.x |
| | `0x0101` | ta sama flaga bez „session available”: TV zajęty (czeka na zgodę) |
| Kanał grupy | 2412 MHz (kanał 1), laptop jako GO, WPS PBC | |

**Zachowanie przy łączeniu (ważne dla własnej implementacji):**
- Przy pierwszym połączeniu od nowego źródła TV pokazuje **monit „zezwól”**. Do kliknięcia
  negocjacja GO nie rusza, a TV ogłasza „sesja niedostępna”.
- **WPS przy aktywnym STA na innym kanale (5 GHz) nie kończy się** (EAP startuje, brak WPS-REG-SUCCESS).
- **Powrót STA zaraz po GROUP-STARTED zrywa grupę** (FORMATION_FAILURE po ~5 s, TV nie zdąży
  się skojarzyć). STA trzeba trzymać wyłączone do otwarcia RTSP przez TV (~15 s od rozłączenia),
  potem współbieżność STA + P2P działa.
- **Wariant jednokanałowy działa bez rozłączania STA** (test 2026-11-08, STA pozostawione włączone): STA na współdzielonym
  2,4 GHz (profil z `band bg`), GO-NEG sam wybrał kanał STA (2462 MHz, kanał 11), WPS-REG-SUCCESS
  w 0,5 s, TV skojarzony po 0,6 s, RTSP po 15 s od startu, M3 jak wyżej. Jedna czkawka STA: AP
  rozłączył laptopa (reason 34, za dużo niepotwierdzonych ramek) w trakcie odkrywania P2P,
  ~6 s bez Wi-Fi, potem roaming ch6 → ch11. Do sprawdzenia: krótsze odkrywanie i roaming
  zmieniający kanał STA w trakcie sesji (grupa zostaje na starym kanale → znów 2 kanały).
- Po nieudanej grupie TV wychodzi ze Smart View i znika z odkrywania — trzeba włączyć ponownie.
- TV dostaje z DHCP `10.42.0.103`, łączy się do `10.42.0.1:7236` ~10 s po zestawieniu grupy.

## 2. Warstwa RTSP

Identyfikacja: **`User-Agent: SVDTZ6_5/SEC-UIBC3`** — Samsung Tizen 6.5, UIBC w wersji Samsunga.
Public: OPTIONS, GET_PARAMETER, PAUSE, PLAY, SETUP, SET_PARAMETER, TEARDOWN, org.wfa.wfd1.0.

### wfd_video_formats: `40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none`

| Pole | Wartość | Odczyt |
|---|---|---|
| native | `40` | tabela CEA, indeks 8 = **1920x1080p60** |
| preferred display mode | `00` | nie obsługuje |
| profil | `01` | **tylko Constrained Baseline** (brak CHP) |
| poziom | `10` | H.264 **4.2** |
| CEA | `000001e3` | 640x480p60, 720x480p60, 1280x720p30, 1280x720p60, **1920x1080p30, 1920x1080p60** (brak 50/25/24 Hz) |
| VESA | `0f3fffff` | 800x600 … 1600x900 (p30/p60), 1680x1024, 1680x1050 (p30/p60); brak 1600x1200, 1920x1200 |
| HH | `00000fff` | wszystkie 12 trybów handheld (800x480 … 960x540, p30/p60) |
| latency | `00` | nie podano |
| min slice / slice enc | `0000` / `00c8` | wiele slice'ów na klatkę dopuszczalne (max 200) |
| frame rate control | `01` | obsługuje zmianę klatkażu w trakcie |

### Pozostałe parametry

| Parametr | Odpowiedź | Uwagi |
|---|---|---|
| wfd_audio_codecs | `LPCM 00000003 00, AAC 00000001 00` | LPCM 44,1/48 kHz 2ch; AAC 48 kHz 2ch; latency 0 |
| wfd_content_protection | `HDCP2.1 port=9999` | |
| wfd_client_rtp_ports | `RTP/AVP/UDP;unicast 19002 0 mode=play` | |
| wfd_3d_video_formats, wfd_I2C, wfd_standby_resume_capability, wfd_coupled_sink | `none` | |
| wfd_uibc_capability | GENERIC (SingleTouch, MultiTouch), HIDC (Keyboard/USB, Keyboard/BT, MultiTouch/USB), port=none | kanał zwrotny: klawiatura i dotyk z TV do laptopa |
| wfd_connector_type | `05` | HDMI |
| wfd_display_edid | 2 bloki (niżej) | |
| wfd2_video_formats, wfd2_audio_codecs, wfd2_aux_stream_formats | `none` | zna nazwy R2, ale **nie obsługuje R2** (brak H.265) |
| **microsoft_latency_management_capability** | **supported** | rozszerzenie MS-WFDPE; do sprawdzenia w M4 |
| microsoft_cursor | `none` | |
| **wfd_sec_low_latency_mode** | **off** | rozszerzenie Samsunga; **kandydat na skrócenie bufora 0,39 s** |
| wfd_sec_hw_rotation | `none` | |
| wfd_sec_view_mode | `multi` | Multi View; TV sam zgłasza zmianę na `full` |
| wfd_sec_dmr_support | `disable` | |
| microsoft_rtcp_capability, microsoft_format_change_capability, microsoft_diagnostics_capability, inne `microsoft_*`, `intel_*`, pozostałe `wfd_sec_*`/`wfd_vnd_sec_*` | brak w odpowiedzi | nieobsługiwane — TV pomija nieznane parametry bez błędu |

Keepalive (pusty GET_PARAMETER co 5 s): odpowiedź natychmiastowa.

## 3. EDID przekazany przez Miracast

Generyczny EDID Samsunga (SAM 0x0a7d, rok 2012, EDID 1.3, „SAMSUNG”) — **nie** EDID panelu 4K:
- preferowany 1920x1080@60, drugi DTD 1366x768; zakres V 24–75 Hz, pclk do 230 MHz;
- CTA-861: VIC 1080p60 (natywny), 1080p50, 720p60/50, 1080i, 1080p24/25/30, SD;
- audio tylko LPCM 2ch 32/44,1/48 kHz; brak HDR, brak 4K.
Wniosek: ścieżka Miracast w tym TV ma sufit **1080p60**, H.264 CBP 4.2.

## 4. Specyfikacja ze źródeł (rozpoznanie internetowe)

- Wersja NA HG50BU800NF (datasheet KniTec): panel 3840x2160, Quantum Processor Lite, HDR10/HDR10+/HLG,
  Wi-Fi 5, BT 5.2, Wi-Fi Direct, mirroring, Multi View, Auto Game Mode (ALLM), HGiG, Samsung LYNK Cloud.
- Konsumencki odpowiednik UE50BU8000: Tizen 6.5 (zgodne z `SVDTZ6_5`), input lag HDMI w Game Mode ~10 ms.
- Nie znaleziono: datasheetu EU, SoC, pasm P2P, ustawień firmware B2B ograniczających Smart View.
- Doniesienia (modele 2019): Game Mode niedostępny podczas mirroringu — dla tego modelu niesprawdzone.

## 5. Wnioski i kolejne eksperymenty

1. Sufit: H.264 CBP, poziom 4.2, do 1080p60; audio LPCM/AAC stereo. H.265 i R2 odpadają.
2. **Największy potencjał na opóźnienie**: `wfd_sec_low_latency_mode: on` oraz
   `microsoft_latency_management_capability` w M4 (SET_PARAMETER) — zmierzyć kalibracją błyskiem.
3. Wiele slice'ów na klatkę (slice enc `00c8`) — możliwe kodowanie/wysyłanie częściami klatki.
4. UIBC (HIDC, Samsung UIBC3) — możliwy kanał zwrotny klawiatury/dotyku z TV.
5. Własny łącznik P2P musi obsłużyć: monit zgody, STA wyłączone do otwarcia RTSP, ponowne wejście TV w Smart View.

## 6. Ruch w trakcie sesji i informacje o TV (sonda 2026-11-09, tcpdump 60 s, interfejs P2P)

- **TV wysyła RTCP Receiver Report co ~1 s** z portu 19001 (RTP 19000 + 1) na nasz `server_port` 19101
  (z odpowiedzi SETUP, `SERVER_PORTS = 19100-19101`). Nikt tam nie słucha — odpowiadamy ICMP port unreachable.
  Treść: RR (PT 201) z blokiem dla naszego SSRC: fraction lost 0, cumulative lost 0xffffff (−1: duplikaty),
  extended highest seq, **jitter ~30–45 jednostek 90 kHz (0,3–0,5 ms)**, LSR/DLSR 0 (nie wysyłamy Sender Reportów).
- **SDES CNAME `user…@host-…`, TOOL `GStreamer`** → odbiornik Miracast w TV to **GStreamer (rtpbin/rtpsession)**.
  Wniosek: buforowanie dźwięku najpewniej wg GStreamera (rtpjitterbuffer, tsdemux, audiobasesink z drift-tolerance,
  alignment-threshold, slave-method) — do potwierdzenia w kodzie Tizena (`docs/tizen-sink-research.md`).
- Z TV do nas poza RTCP: nic (żadnych prób HDCP, UIBC, innych usług) w 60 s.
- `http://<TV>:8001/api/v2/`: `modelName` **HG50BU800EUXEN**, `model` **22_NIKEL_UB1_B2B** (platforma 2022, wersja B2B),
  OS Tizen, `firmwareVersion` Unknown, rozdzielczość panelu 3840x2160, API remote 2.0.25.

## 7. Inne usługi TV

TV udostępnia też zwykłe usługi Samsunga (DLNA, DIAL, API pilota); tu nieużywane.
