---
id: samsung-tizen65-hg50bu800
vendor: Samsung
model: HG50BU800 (B2B variant of the consumer UE50BU8000)
platform: Tizen 6.5 (2022)
firmware: 22_NIKEL_UB1_B2B
identify:
  user_agent: SVDTZ6_5/SEC-UIBC3
  p2p_model_number: QAQ80
  pri_dev_type: 7-0050F204-1
  wfd_dev_info: "0x0111"
source_settings:
  mode: 1920x1080p60
  video: H.264 CBP, CBR 8000 kbit/s, constant frame rate
  m4: wfd_content_protection none, wfd_audio_codecs LPCM 48 kHz
announced:
  wfd_video_formats: 40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none
  wfd_audio_codecs: LPCM 00000003 00, AAC 00000001 00
  wfd_content_protection: HDCP2.1 port=9999
  wfd_client_rtp_ports: RTP/AVP/UDP;unicast 19002 0 mode=play
  wfd_3d_video_formats: none
  wfd_display_edid: 2 blocks, SAM 0x0a7d, 2012, raw not logged
  wfd_coupled_sink: none
  wfd_I2C: none
  wfd_uibc_capability: input_category_list=GENERIC, HIDC; generic_cap_list=SingleTouch, MultiTouch; hidc_cap_list=Keyboard/USB, Keyboard/BT, MultiTouch/USB;port=none
  wfd_connector_type: "05"
  wfd_standby_resume_capability: none
  wfd2_video_formats: none
  wfd2_audio_codecs: none
  wfd2_aux_stream_formats: none
  microsoft_cursor: none
  microsoft_latency_management_capability: supported
  wfd_sec_low_latency_mode: "off"
  wfd_sec_hw_rotation: none
  wfd_sec_view_mode: multi
  wfd_sec_dmr_support: disable
checklist:
  wake_from_standby:
    value: yes, P2P frame not identified
    status: observed
    date: 2026-11-20
    ref: E4
    note: firmware feature, probably "Power on with mobile device"; the source sends no CEC, WoL or remote API call
  p2p_visible_in_standby:
    value: "yes"
    status: observed
    date: 2026-11-20
    ref: E4
    note: implied, the source found the TV in standby before connecting
  discoverable_after_smart_view:
    value: no limit seen (>= 19 min, 9 min of it without contact)
    status: confirmed
    date: 2026-11-08
    ref: Q1
  availability_bit_changes:
    value: 0x0111 -> 0x0101 during the consent prompt and right after a session
    status: confirmed
    ref: Q4
  first_connect_consent_prompt:
    value: "yes"
    status: confirmed
    date: 2026-11-08
    ref: Q8
  consent_remembered:
    value: "yes"
    status: observed
    ref: Q8
  persistent_group_invitation:
    value: unknown
    status: open
    ref: E4
  failed_formation_exits_mirroring:
    value: "yes"
    status: confirmed
    date: 2026-11-08
    ref: Q2
  single_channel_needed:
    value: yes, WPS does not finish while the source's Wi-Fi is on another channel (5 GHz)
    status: observed
    date: 2026-11-08
    note: works when the source's Wi-Fi shares a 2.4 GHz channel with the group
  group_owner:
    value: source is GO, the TV joins as client (WPS PBC)
    status: observed
    date: 2026-11-08
  teardown_without_video_s:
    value: 25
    status: confirmed
    ref: Q3
  stream_restart_teardown:
    value: "yes"
    status: confirmed
    ref: Q3
  idr_request_after_start:
    value: "yes"
    status: confirmed
    ref: Q7
  multi_view_on_start:
    value: yes, reports wfd_sec_view_mode multi
    status: observed
    ref: Q5
  m4_requires:
    value: [wfd_content_protection, wfd_audio_codecs]
    status: confirmed
    note: without them the TV sends TEARDOWN after SETUP
  honours_video_pts:
    value: "no"
    status: confirmed
    date: 2026-11-08
    ref: Q6
  buffer_frames:
    value: 1.5-3
    status: confirmed
    date: 2026-11-08
    ref: Q6
  audio_codecs_working:
    value: LPCM 48 kHz stereo (AAC announced, not tried)
    status: observed
    date: 2026-11-08
    ref: Q9
  honours_audio_pts:
    value: yes, waits on a forward jump
    status: confirmed
    date: 2026-11-09
    ref: Q10
  audio_buffer_ratchet:
    value: "yes"
    status: confirmed
    date: 2026-11-09
    ref: Q11
  needs_constant_ts_rate:
    value: yes, padded to 8000 kbit/s with null packets
    status: confirmed
    date: 2026-11-09
    ref: Q10
  vendor_extensions:
    value: [wfd_sec_low_latency_mode, wfd_sec_view_mode, wfd_sec_hw_rotation, wfd_sec_dmr_support, microsoft_latency_management_capability, microsoft_cursor]
    status: observed
    date: 2026-11-08
  low_latency_mode:
    value: wfd_sec_low_latency_mode on accepted (200 OK); audio and video in sync by eye and ear
    status: observed
    date: 2026-11-09
    ref: Q5
  glass_to_glass_ms:
    value: 90 (1920x1080p60, camera)
    status: confirmed
    date: 2026-11-08
---
# Samsung HG50BU800 (Tizen 6.5, 2022; B2B variant of the consumer UE50BU8000)

Identification: RTSP `User-Agent: SVDTZ6_5/SEC-UIBC3`; P2P: name `<TV name>` (set on the TV), ModelNumber `QAQ80`,
pri_dev_type `7-0050F204-1`; WFD device info `0x0111` (primary sink, available, HDCP 2).
Capabilities: `data/probe/capabilities.md` (H.264 CBP up to level 4.2, native 1080p60, LPCM/AAC, UIBC, `microsoft_*`
and `wfd_sec_*` extensions).
Source settings: 1920x1080p60, CBR 8 Mbit/s, `wfd_content_protection: none` and `wfd_audio_codecs` in M4, constant
frame rate. Result: about 90 ms screen to screen (camera measurement, 2026-11-08).

**Sources of knowledge about this TV (read before working on it; the `docs/` notes are in Polish):**
- `data/probe/capabilities.md`: P2P/WFD IE, the full M3 (codecs, `wfd_sec_*`, `microsoft_*`), EDID, §6 traffic in a
  session (RTCP RR every 1 s to our `server_port`+1, SDES TOOL "GStreamer"), model and platform from `/api/v2/`
  (`HG50BU800EUXEN`, `22_NIKEL_UB1_B2B`). The TV also offers the usual Samsung services (DLNA, DIAL, remote API);
  not used here.
- `docs/tizen-sink-research.md`: the reference Tizen 6.5 sink (libmm-wfd): pipeline, `pulsesink` slave-method NONE,
  `ts-offset` 250 ms, resync after a drift of at least 40 ms for at least 1 s; the mechanism behind the ratchet (Q11).
- `docs/samsung-oss-research.md`: Samsung's open source: package `22_DTV_NikeL` (§5: video decoder in FREERUN, so PTS
  is ignored; the audio DSP syncs to the STC).
- `docs/tizen-research.md`: P2P visibility (wifi-direct-manager).

Setting on the TV: Settings → General & Privacy → Start Screen Options → turn off "Autorun Multi View Mirroring"
(Samsung 2022; may be missing from the B2B firmware).

## Behaviours

### Q1: "short visibility window": disproved without interference
- Observation: the "waiting for connection" screen stays, but after a while a P2P scan does not find the TV; leaving
  and reopening screen mirroring helps. The screen shown in Smart View is the "Connection Guide": a static
  instruction for a phone ("Device name: <TV name>", a "Close" button) over the Screen Mirroring source. It does not
  show the state of the radio.
- Evidence: 2026-11-08, a 5 s scan found no device for several minutes while the pairing screen was showing (raw
  P2P events from wpa_supplicant).
- Reproduce: open Smart View, do not connect, scan every 10 s and measure how long the TV stays visible.
- Specification: none (the Listen schedule is up to the firmware).
- Measurement 2026-11-08 (`tools/tvwindow.py`, about 19 min): queries every 6 s → visible for 6 min without a break;
  then 3 min of silence → visible; 6 min of silence → visible (at least 19 min in Smart View in total, 9 min without
  contact). The TV sends no Probe Request of its own (0 events during the silence).
- Conclusion: no short limit and no idle timer; the earlier "disappearances" coincided with our failed attempts (Q2).
  Tizen 6.5 code: the listen limit is set by a closed application (wpa_supplicant: 0 → 3600 s),
  `docs/tizen-research.md`.
- Workaround: not needed; the 4 s scan every 4 s stays (fast reaction), a slower one may be kinder to the STA.
  Status: disproved; the upper bound (at least 19 min, at most 60 min?) not measured.

### Q2: a failed group formation or our TEARDOWN → the TV leaves Smart View
- Evidence (2026-11-08, wpa_supplicant log and source events): a failed group formation, a TEARDOWN after a
  measurement and several failed connections right after a session ended; each time the TV disappeared from
  discovery.
- Specification: no rule (§4.12).
- Workaround: connect only to a freshly seen TV with the availability bit set (`nm.rs` `lookup`); after a TEARDOWN
  from the TV, earlier sightings are ignored (`ignore_sightings_for`). Status: workaround in code.

### Q3: about 25 s without displayable video → TEARDOWN from the TV
- Evidence: a gap in the picture during an analysis, and a wfd-tx restart (new SSRC/PCR); a TEARDOWN after about
  25 s in both cases.
- Reproduce: stop sending, or restart the RTP/TS stream, during a session.
- Specification: none (the spec only has the keepalive, §6.5.1).
- Workaround: repeat the last frame (`wfd-media`); `wfd-tx` lives for the whole session (`media.rs`). Status:
  workaround in code.

### Q4: availability bit 0x0111 → 0x0101 during the consent prompt and right after a session
- Specification: §4.5, do not connect while it is 0b00. Workaround: checked in `nm.rs`. Status: as the spec says.

### Q5: Multi View (split screen) at the start
- **`wfd_sec_low_latency_mode: on`** (SET_PARAMETER in the session, 2026-11-09): 200 OK, audio and video in sync by
  eye (before, the sound was late). Sent automatically 3 s after PLAY (session.rs). Not measured in numbers yet.
- Observation: the TV reports `wfd_sec_view_mode: multi`; our `full` gets 200 OK; once the TV switched to full about
  1.4 s after three requests.
- Specification: none (a Samsung extension). Documentation: the "Autorun Multi View Mirroring" setting.
- Workaround: ask for `full` after PLAY and after a `multi` report (at most 2). Status: hypothesis; not known whether
  the request or the setting did it.

### Q6: the TV ignores PTS and buffers a constant number of frames (about 1.5-3)
- Evidence: camera measurement 2026-11-08 (PTS +100…400 ms had no effect; 2.9 frames/s → 592 ms, 60 frames/s →
  about 90 ms).
- Workaround: constant frame rate, 60 frames/s. Status: confirmed by measurement.

### Q7: IDR request after the start (`wfd_idr_request`)
- Workaround: SIGUSR1 to `wfd-media` → a key frame. Status: workaround in code.

### Q8: "Allow" prompt on the first connection of a new source
- Evidence: the first probe, 2026-11-08. No prompt afterwards: the source is remembered. Status: confirmed.

### Q9: LPCM audio: crackles when the radio stalls
- Observation (2026-11-08): LPCM 48 kHz plays, sync is fine; crackles with music.
- Evidence: (1) clocking audio from the source (`wfd-tx`) instead of its own 10 ms timer → fewer crackles;
  (2) ping to the TV during a session: p99 78 ms, max 103 ms; with STA power saving off
  (`802-11-wireless.powersave 2`; the GO interface does not allow the change) p99 59 ms, max 74 ms →
  listening: no crackles with an audio PTS margin of 0 ms.
- Specification: LPCM as in WFD table 106 (header A0 06 00 11).
- Workaround: rhythm from the source, continuous PTS; STA power saving off; the audio PTS margin can be changed live
  (`$XDG_RUNTIME_DIR/sway-miracast/audio-delay-ms` + `pkill -HUP -x wfd-tx`), default -1000 ms (`AUDIO_DELAY_MS`,
  see Q10). Status: confirmed in one session; the TV honours audio PTS (Q10).

### Q10: uneven audio with a "thin" TS stream (still screen)
- Observation (2026-11-09): audio stutters while the player is not visible (little video data).
- Evidence (microphone, `tools/audiolat.py --period 0.25 --coded 5`, 120 beeps): still screen std 13.7-24.5 ms,
  16-44 jumps of 10 ms or more, drift ±40 ms in 30 s; lots of motion on screen std 1.6 ms, 0 jumps; a small moving
  element worse (23 ms); 8 Mbit/s of UDP **outside** the stream changed nothing (16.8 ms) → not the radio but the TS
  rate.
- Reproduce: still screen plus a beep train, `mux-kbps` 0.
- Workaround: constant TS rate with null packets (`wfd-tx --mux-kbps-file`): 4 Mbit/s 11.5 ms; **8 Mbit/s 3.2 ms,
  0 jumps**; 12 Mbit/s 3.3 ms. Default 8000 (`$XDG_RUNTIME_DIR/sway-miracast/mux-kbps`, SIGHUP). Status: confirmed.
- Audio delay: about 770 ms at PTS +0 after 1 h (it grew), about 440 ms at least; the TV honours audio PTS
  (-200 → 580, -400 → 462, -600 → 437 ms). At -400 ms with video at about 90 ms → audio about 380 ms behind.
- After moving to the null sink (system clock) and an audio delay of -1000 ms the A/V offset shrank (under 50 ms,
  audio slightly behind). The default `AUDIO_DELAY_MS` is now -1000 ms (`crates/wfd-source/src/media.rs`),
  `VIDEO_HOLD_MS` 0.
- Current state (2026-11-09): with `wfd_sec_low_latency_mode: on` (Q5, sent automatically after PLAY) audio and video
  are practically in sync (by eye and ear, no numeric measurement).
- "Film" mode (`tools/miracast-film.sh`): the source holds the picture back by (audio - video). Status: experimental;
  checked offline, not on the TV; mostly unnecessary since Samsung's low-latency mode. A constant video delay was
  rejected as the default (it also delays desk work).

### Q11: the audio buffer only grows (ratchet)
- Evidence (2026-11-09, about 8 min, `audiolat.py`): -400 → -300: +112 ms; -300 → -400: only -49 ms; -700: 197 ms,
  -1000: 204 ms (the floor), back to -400: 518 ms. At -1000 it still creeps: 204 → 274 ms in 7 min.
- Conclusion: the TV aligns to PTS on a forward jump (it waits) but does not fully catch up after a backward jump;
  it does not tune itself to the PCR.
- Workaround: none reliable. Avoid audio PTS jumps (wfd-tx moves the audio PTS only when the delay changes).
  Status: open.

## Open
- E4: invitation to a remembered group (P2P invitation, §4.13, optional): does the TV accept it outside Smart View?
  Observation 2026-11-20: a connection from the menu switches the TV on from standby by itself and opens the picture,
  without Smart View and without a consent prompt. The source wakes nothing (no CEC, WoL or remote API): it is the
  firmware (network in standby, probably "Power on with mobile device"). Not measured: which frame does it
  (`P2P-PROV`, `GO-NEG`, `INVITATION`); check with `tools/linkmon.sh` while the TV is off, then close E4.
  The source has a per-TV "connect only when picked" setting for this (miracast-menu, TV settings).
- `wfd_sec_view_mode: full` in M4: not tried.
