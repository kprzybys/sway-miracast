# sway-miracast

A Miracast (Wi-Fi Display) source for Sway and other wlroots compositors: mirror or extend your laptop screen to a
TV over Wi-Fi Direct, with low latency, without GNOME and without extra hardware.

## Why

The usual Linux route, GNOME Network Displays, depends on xdg-desktop-portal features (RemoteDesktop, virtual
sources) that the wlroots portal does not offer, so it does not work on Sway. Generic tools built on ffmpeg pipelines
worked (see Related projects below), but in our setup with deep queues and about half a second of latency. sway-miracast is its own WFD source written in Rust:
it captures with `wlr-screencopy`, encodes on the GPU, muxes MPEG-TS/RTP itself and keeps the TV session alive for
hours without attention.

## Status

- Developed and tested with **one TV**: Samsung HG50BU800 (Tizen 6.5, 2022), from a laptop with an Intel iGPU (VA-API),
  Sway 1.9, PipeWire, NetworkManager. Screen to screen about 90 ms at 1080p60, audio LPCM 48 kHz.
- **Other TVs are untested.** The code follows the WFD specification as the baseline and keeps TV-specific
  workarounds narrow, but every sink has quirks. Reports are welcome, ideally as a profile in [`sinks/`](sinks/)
  (template in `sinks/README.md`).
- Wi-Fi Display R1 only (H.264 Constrained Baseline, LPCM), no HDCP, no UIBC.

## Features

- TV picker menu (`miracast-menu`): Wi-Fi Direct scan plus remembered TVs, in fuzzel/wofi/rofi/bemenu/dmenu.
- Waybar module (`miracast-status`): off / waiting for the TV / casting.
- Duplicate the laptop screen or extend to a separate virtual output (`miracast-screen`), switched without
  dropping the session.
- Samsung low-latency mode (`wfd_sec_low_latency_mode`) requested automatically from Samsung sinks that
  send `wfd_sec_*` parameters themselves.
- VA-API encoding with zero-copy dmabuf capture and the low-power encoder (`vah264lpenc`, then `vah264enc`);
  software `x264enc` fallback when VA-API is missing.
- Unattended operation: systemd user service with watchdog, reconnects when the TV comes back, encoder restarts
  without ending the session. `miracast-doctor` checks the environment.
- English and Polish messages.

## Architecture

```
                 Wi-Fi Direct (NetworkManager D-Bus) + RTSP :7236 (M1-M16, keepalive)
  wfd-source  <───────────────────────────────────────────────────────────────>  TV (sink)
  (supervisor)                                                                      ▲
     │ spawns and restarts                                                          │ RTP/UDP
     ├── wfd-media: wlr-screencopy ─▶ VA-API H.264 ──▶ FIFO ─┐                      │ (MPEG-TS)
     ├── parec: PipeWire null sink (system clock) ─▶ FIFO ───┼──▶ wfd-tx ───────────┘
     │                                                       │    TS mux, PCR, constant rate, RTCP
  wfd-proto: RTSP/WFD parsing, no I/O        wfd-util: shared JSON event log
```

- `crates/wfd-source` - P2P group, RTSP negotiation, session supervision, sink quirks.
- `crates/wfd-media` - capture and encode one output, H.264 access units on a FIFO.
- `crates/wfd-tx` - MPEG-TS/RTP sender; lives for the whole session because the TV does not accept a restarted stream.
- `crates/wfd-proto`, `crates/wfd-util` - libraries.
- `tools/` - the `miracast-*` commands and measurement scripts (latency, A/V sync, audio gaps).

Process boundaries and FIFOs instead of shared state: a crashing encoder is restarted while the stream continues.

## Quick start

See [INSTALL.md](INSTALL.md) for requirements (wlroots compositor, Wi-Fi card with P2P, NetworkManager with
wpa_supplicant, GStreamer 1.24+ with VA plugins, Rust). In short:

```bash
make build && make install
systemctl --user daemon-reload && systemctl --user enable sway-miracast-source.service
miracast-doctor     # environment check
miracast-menu       # start Smart View / screen mirroring on the TV, then pick it here
```

## Latency and A/V sync in two sentences

The TV ignores video timestamps and shows frames after a fixed number of buffered frames, so the source sends a
constant 60 fps and gets about 90 ms screen to screen, while audio follows its PTS behind a much longer buffer.
Samsung's undocumented `wfd_sec_low_latency_mode: on` (advertised by the TV, reportedly sent by Samsung phones
(Smart View) during calls), sent as a SET_PARAMETER after PLAY, shortens the TV's buffering and brings sound and
picture together (judged by eye and ear; not yet measured).

The longer story, with the dead ends, is in [docs/history.md](docs/history.md).

## Related projects

sway-miracast is not the only Miracast source that works on wlroots. If it does not fit your setup, try these:

- [fluxcast](https://github.com/IlyaP358/fluxcast) (Python, GPL-3.0) - the most mature option: Miracast/WFD plus
  DLNA and Chromecast fallbacks, works on GNOME, KDE, Sway and Hyprland, tested with many LG, Samsung and other
  sinks. sway-miracast started from it (see [docs/history.md](docs/history.md)).
- [swaybeam](https://github.com/forkline/swaybeam) (Rust, MIT) and its fork [waycast](https://github.com/alchemy/waycast) -
  Miracast sources for Sway, River, Labwc and Hyprland with their own RTSP, portal capture, VA-API or x264 and an
  extend mode; tested with LG and Samsung TVs. We found them only after building ours; the architecture is close.
- [GNOME Network Displays](https://gitlab.gnome.org/GNOME/gnome-network-displays) and
  [MiracleCast](https://github.com/albfan/miraclecast) - the older Linux tools; on wlroots they work poorly or not at all.

What sway-miracast focuses on is latency and A/V sync with a Samsung TV (frame-count buffering, the Samsung
low-latency mode, an audio clock that does not drift), zero-copy `wlr-screencopy` capture without a portal dialog,
and running unattended as a systemd service. It has been tested with one TV; the projects above cover far more
hardware. Findings that may help them are written up in [docs/history.md](docs/history.md) and [sinks/](sinks/).

## Documentation

Some documents are in Polish (the project started as a personal one); the code, the README, INSTALL.md and the
history are in English.

- [INSTALL.md](INSTALL.md) - requirements, installation, commands, settings, troubleshooting.
- [docs/history.md](docs/history.md) - how we got here and lessons for other Miracast sources.
- [sinks/](sinks/) - per-TV behaviour profiles (Polish); [docs/METODA.md](docs/METODA.md) - working method (Polish).
- [docs/spec-notes.md](docs/spec-notes.md), [docs/tizen-sink-research.md](docs/tizen-sink-research.md),
  [docs/samsung-oss-research.md](docs/samsung-oss-research.md), [docs/tizen-research.md](docs/tizen-research.md) -
  specification notes and research on the Samsung/Tizen sink (Polish).
- [data/probe/capabilities.md](data/probe/capabilities.md) - full capability probe of the tested TV (Polish).
- [docs/PLAN.md](docs/PLAN.md) - original plan (Polish, historical, partly outdated).
- [CONTRIBUTING.md](CONTRIBUTING.md) - how to build, test and contribute (short English section, details in Polish).

## License

[Mozilla Public License 2.0](LICENSE).
