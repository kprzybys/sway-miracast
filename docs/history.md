# History: how sway-miracast came about

Notes from November 2026, written as lessons for anyone building or debugging a Miracast source on Linux.
Test setup: a Samsung HG50BU800 (Tizen 6.5, 2022; the B2B variant of the consumer UE50BU8000), an Intel
UHD 620 laptop with Sway 1.9, NetworkManager and PipeWire.

## 1. Existing tools first

- **GNOME Network Displays** does not work on Sway. Version 0.92 asks the portal for RemoteDesktop, which
  xdg-desktop-portal-wlr does not provide; 0.99 asks for a non-monitor (virtual) screencast source, which the wlroots
  portal refuses. The X11 fallback in its code was never reached. It is effectively a GNOME/mutter tool.
- **fluxcast** (Python, captures with `wf-recorder`/wlr-screencopy, P2P through NetworkManager) worked. Its software
  x264 path could not keep up with 1080p30 on a 4-core U-series CPU, and a 1024-frame input queue hid that: latency
  grew from seconds to over 40 s while ffmpeg grew to gigabytes of RAM. Lesson: *latency that grows over time is a
  component that cannot keep up plus a queue that hides it; drop frames instead of queueing them.*
- With VA-API encoding (Constrained Baseline, the only profile the TV accepts; do not force `-level` on iHD) the
  delay became a stable ~0.45 s. A camera measurement later showed the TV itself adds only ~75-120 ms, so most of the
  rest was on the laptop side: capture, ffmpeg queues and muxing.

That was the reason for an **own source**: no queues anywhere, own MPEG-TS/RTP mux with control over PCR and
pacing, direct wlr-screencopy capture, and a supervisor that keeps the session alive for hours.

## 2. What the TV actually does

- **It ignores video PTS and buffers a fixed number of frames** (~2-3), not a fixed time. Shifting PTS by 100-400 ms
  changed nothing; at 3 fps the latency was ~600 ms, at 60 fps ~90 ms. So the source sends a constant 60 fps,
  repeating the last frame when the screen is static.
- **~25 s without displayable video ends the session** (TEARDOWN from the TV). A restarted RTP/TS stream (new SSRC,
  sequence numbers, continuity counters, PCR jump) counts as "no video". The sender therefore lives for the whole
  session and only the encoder behind it is restarted.
- **Consent prompt:** the first connection from a new source shows an "Allow" prompt; until it is answered the
  TV advertises "session not available" in its WFD IE. The source must wait, not retry.
- **Discovery after an abrupt end:** after a failed group formation or our TEARDOWN the TV leaves its listening
  state and must be put into Smart View again. Connect only to a TV seen *after* the last session ended, and read
  availability from fresh wpa_supplicant events; NetworkManager's cached WFD IE stayed "not available" after the
  consent prompt.
- A thin TS stream (static screen, few bytes) made audio uneven; a constant TS rate padded with null packets fixed it.
- One radio, two networks: when the laptop's normal Wi-Fi roamed to another channel than the P2P group, the radio
  had to time-share and audio broke up. Keep the P2P group on the station's channel and avoid long P2P scans
  while casting, especially on a busy shared Wi-Fi network.

## 3. Audio clock drift

Audio delay on the TV followed a sawtooth: it shrank by 10-14 ms per minute and jumped back every 2-3 minutes,
each jump being our own audio resync. RTCP from the TV was clean, so it was not the network. The cause was the clock:
audio was captured from the sound card's monitor, whose clock ran ~250-300 ppm slower than the system clock that
drives PCR. **Fix: capture from a PipeWire null sink driven by the system clock** (the same base as PCR) and move
application audio to it for the session. Result: zero resyncs. Lesson: *in a source you control, every stream
must be timed by the same clock as the PCR.*

## 4. A/V sync and the Samsung low-latency mode

With clocks fixed, sound was still behind the picture. The reason is on the TV: video is shown freerun with a short
buffer, audio follows its PTS behind a much longer one, and no clock alignment from the source can change a
difference of buffer sizes. Samsung's open-source package for this platform (kernel drivers only; the Miracast
player itself is closed) showed a FREERUN mode in the video decoder driver and audio synchronised to the STC, which
matched the measurements. The package contains no code that reads the low-latency parameter. That parameter,
`wfd_sec_low_latency_mode`, is reportedly sent by Samsung phones (Smart View) during calls, and the TV listed it
(`off`) in its M3 answer. Sending `SET_PARAMETER wfd_sec_low_latency_mode: on` after PLAY (2026-11-09) made sound and
picture practically in sync and typing felt immediate (judged by eye and ear; a camera measurement of the video path
alone, done earlier, showed no clear change, so the effect is probably mostly on audio buffering). The source sends
it only to sinks that advertised `wfd_sec_*` parameters themselves: an unknown parameter in M3 may make another
sink reject the whole request.

## 5. Zero-copy capture pitfall

dmabuf capture (compositor copies into a GPU buffer, VA imports it) cut CPU use by about 40% compared with shm,
but showed two bugs. Red and blue were swapped because the format was taken from the shm path (XB24) instead of the
dmabuf one (XR24). Then the picture on the TV came out shredded: the buffer was Y-tiled, while appsrc announced plain
`video/x-raw`, so VA read it as linear memory. Average-colour checks did not catch it; comparing decoded frames did.
**Fix: allocate LINEAR buffers.** Tiled buffers with `DMA_DRM` caps worked only under a misnamed format in
GStreamer-VA 1.24 and were rejected as fragile. Lesson: *verify images (PSNR against the shm path), not statistics.*

## 6. Method

Spec as the minimum, the TV's real behaviour recorded per model in `sinks/` with evidence and a way to reproduce
it; reproduce a quirk before working around it; measure before and after each change (camera-based glass-to-glass,
microphone for audio gaps and delay), in one long-held session, because every unnecessary TEARDOWN costs a trip to
the TV remote.
