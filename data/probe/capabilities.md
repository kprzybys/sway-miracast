# The TV as a Miracast sink: capabilities (probe 2026-11-08)

Sources: P2P discovery through NetworkManager, a full RTSP exchange, the EDID from `wfd_display_edid`, the
wpa_supplicant journal (the raw probe logs are not published). Tools: `tools/`. Profile: `sinks/samsung-tizen65-hg50bu800.md`.

## 1. Wi-Fi Direct (P2P) layer

| Field | Value | Meaning |
|---|---|---|
| Name / manufacturer / model | `<TV name>` / SAMSUNG_ELECTRONICS / ModelNumber `QAQ80` | QAQ80 is the platform code, not the model as sold |
| pri_dev_type | `7-0050F204-1` | category 7 (Display), subcategory 1 (television) |
| config_methods | `0x188` while waiting, `0x80` after the user allowed the source | 0x100 Keypad, 0x80 Push Button, 0x08 Display; after "Allow" only PBC is left |
| dev_capab / group_capab | `0x24` / `0x0` (`0x8` after consent) | 0x04 concurrent operation, 0x20 invitation procedure |
| WFD Device Info | `0x0111`, port 7236, 54 Mbit/s | primary sink, session available, HDCP 2.x |
| | `0x0101` | the same without "session available": the TV is busy (waiting for consent) |
| Group channel | 2412 MHz (channel 1), laptop as GO, WPS PBC | |

**Behaviour while connecting (important for our own implementation):**
- On the first connection from a new source the TV shows an **"allow" prompt**. Until it is clicked, GO negotiation
  does not start and the TV announces "session not available".
- **WPS does not finish while the STA is active on another channel (5 GHz)** (EAP starts, no WPS-REG-SUCCESS).
- **Bringing the STA back right after GROUP-STARTED breaks the group** (FORMATION_FAILURE after about 5 s, the TV has
  no time to associate). The STA has to stay off until the TV opens RTSP (about 15 s after the disconnect);
  after that STA + P2P concurrency works.
- **The single-channel variant works without disconnecting the STA** (test 2026-11-08, STA left on): STA on a shared
  2.4 GHz channel (a profile with `band bg`), GO negotiation picked the STA's channel by itself (2462 MHz, channel 11),
  WPS-REG-SUCCESS in 0.5 s, TV associated after 0.6 s, RTSP 15 s after the start, M3 as below. One STA hiccup: the AP
  disconnected the laptop (reason 34, too many unacknowledged frames) during P2P discovery, about 6 s without Wi-Fi,
  then roaming ch6 → ch11. To check: shorter discovery, and roaming that changes the STA channel during a session
  (the group stays on the old channel → two channels again).
- After a failed group the TV leaves Smart View and disappears from discovery; it has to be opened again.
- The TV gets `10.42.0.103` from DHCP and connects to `10.42.0.1:7236` about 10 s after the group is up.

## 2. RTSP layer

Identification: **`User-Agent: SVDTZ6_5/SEC-UIBC3`**: Samsung Tizen 6.5, Samsung's version of UIBC.
Public: OPTIONS, GET_PARAMETER, PAUSE, PLAY, SETUP, SET_PARAMETER, TEARDOWN, org.wfa.wfd1.0.

### wfd_video_formats: `40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none`

| Field | Value | Reading |
|---|---|---|
| native | `40` | CEA table, index 8 = **1920x1080p60** |
| preferred display mode | `00` | not supported |
| profile | `01` | **Constrained Baseline only** (no CHP) |
| level | `10` | H.264 **4.2** |
| CEA | `000001e3` | 640x480p60, 720x480p60, 1280x720p30, 1280x720p60, **1920x1080p30, 1920x1080p60** (no 50/25/24 Hz) |
| VESA | `0f3fffff` | 800x600 … 1600x900 (p30/p60), 1680x1024, 1680x1050 (p30/p60); no 1600x1200, 1920x1200 |
| HH | `00000fff` | all 12 handheld modes (800x480 … 960x540, p30/p60) |
| latency | `00` | not given |
| min slice / slice enc | `0000` / `00c8` | several slices per frame allowed (at most 200) |
| frame rate control | `01` | supports changing the frame rate during a session |

### Other parameters

| Parameter | Answer | Notes |
|---|---|---|
| wfd_audio_codecs | `LPCM 00000003 00, AAC 00000001 00` | LPCM 44.1/48 kHz 2ch; AAC 48 kHz 2ch; latency 0 |
| wfd_content_protection | `HDCP2.1 port=9999` | |
| wfd_client_rtp_ports | `RTP/AVP/UDP;unicast 19002 0 mode=play` | |
| wfd_3d_video_formats, wfd_I2C, wfd_standby_resume_capability, wfd_coupled_sink | `none` | |
| wfd_uibc_capability | GENERIC (SingleTouch, MultiTouch), HIDC (Keyboard/USB, Keyboard/BT, MultiTouch/USB), port=none | back channel: keyboard and touch from the TV to the laptop |
| wfd_connector_type | `05` | HDMI |
| wfd_display_edid | 2 blocks (below) | |
| wfd2_video_formats, wfd2_audio_codecs, wfd2_aux_stream_formats | `none` | knows the R2 names but **does not support R2** (no H.265) |
| **microsoft_latency_management_capability** | **supported** | MS-WFDPE extension; to be tried in M4 |
| microsoft_cursor | `none` | |
| **wfd_sec_low_latency_mode** | **off** | Samsung extension; **candidate for shortening the 0.39 s buffer** |
| wfd_sec_hw_rotation | `none` | |
| wfd_sec_view_mode | `multi` | Multi View; the TV reports a change to `full` by itself |
| wfd_sec_dmr_support | `disable` | |
| microsoft_rtcp_capability, microsoft_format_change_capability, microsoft_diagnostics_capability, other `microsoft_*`, `intel_*`, other `wfd_sec_*`/`wfd_vnd_sec_*` | not in the answer | not supported; the TV skips unknown parameters without an error |

Keepalive (empty GET_PARAMETER every 5 s): answered at once.

## 3. EDID passed over Miracast

A generic Samsung EDID (SAM 0x0a7d, year 2012, EDID 1.3, "SAMSUNG"), **not** the EDID of the 4K panel:
- preferred 1920x1080@60, second DTD 1366x768; V range 24-75 Hz, pixel clock up to 230 MHz;
- CTA-861: VIC 1080p60 (native), 1080p50, 720p60/50, 1080i, 1080p24/25/30, SD;
- audio LPCM 2ch 32/44.1/48 kHz only; no HDR, no 4K.
Conclusion: the Miracast path of this TV tops out at **1080p60**, H.264 CBP 4.2.

## 4. Specification from other sources (web research)

- North American HG50BU800NF (KniTec data sheet): 3840x2160 panel, Quantum Processor Lite, HDR10/HDR10+/HLG,
  Wi-Fi 5, BT 5.2, Wi-Fi Direct, mirroring, Multi View, Auto Game Mode (ALLM), HGiG, Samsung LYNK Cloud.
- Consumer counterpart UE50BU8000: Tizen 6.5 (matches `SVDTZ6_5`), HDMI input lag in Game Mode about 10 ms.
- Not found: an EU data sheet, the SoC, the P2P bands, B2B firmware settings that restrict Smart View.
- Reports (2019 models): Game Mode unavailable during mirroring; not checked for this model.

## 5. Conclusions and next experiments

1. Ceiling: H.264 CBP, level 4.2, up to 1080p60; audio LPCM/AAC stereo. H.265 and R2 are out.
2. **Most promising for latency**: `wfd_sec_low_latency_mode: on` and `microsoft_latency_management_capability` in M4
   (SET_PARAMETER); measure with the flash calibration.
3. Several slices per frame (slice enc `00c8`): frames could be encoded and sent in parts.
4. UIBC (HIDC, Samsung UIBC3): a possible keyboard/touch back channel from the TV.
5. Our own P2P connector must handle: the consent prompt, the STA off until RTSP opens, the TV re-entering Smart View.

## 6. Traffic during a session and information about the TV (probe 2026-11-09, tcpdump 60 s, P2P interface)

- **The TV sends an RTCP Receiver Report about every 1 s** from port 19001 (RTP 19000 + 1) to our `server_port` 19101
  (from the SETUP answer, `SERVER_PORTS = 19100-19101`). At the time nobody listened there, so we answered with ICMP
  port unreachable. Content: RR (PT 201) with a block for our SSRC: fraction lost 0, cumulative lost 0xffffff (-1:
  duplicates), extended highest seq, **jitter about 30-45 units of 90 kHz (0.3-0.5 ms)**, LSR/DLSR 0 (we sent no
  Sender Reports then).
- **SDES CNAME `user…@host-…`, TOOL `GStreamer`** → the Miracast sink in the TV is **GStreamer (rtpbin/rtpsession)**.
  Conclusion: audio buffering most likely follows GStreamer (rtpjitterbuffer, tsdemux, audiobasesink with
  drift-tolerance, alignment-threshold, slave-method); to be confirmed in the Tizen code (`docs/tizen-sink-research.md`).
- From the TV to us, besides RTCP: nothing (no HDCP, UIBC or other services) in 60 s.
- `http://<TV>:8001/api/v2/`: `modelName` **HG50BU800EUXEN**, `model` **22_NIKEL_UB1_B2B** (2022 platform, B2B
  version), OS Tizen, `firmwareVersion` Unknown, panel resolution 3840x2160, remote API 2.0.25.

## 7. Other TV services

The TV also offers the usual Samsung services (DLNA, DIAL, remote API); not used here.
