# Installing sway-miracast

A Miracast (Wi-Fi Display) source for Sway/wlroots: mirror or extend your laptop screen to a TV over Wi-Fi Direct,
with low latency and no extra hardware.

Tested on: an Intel iGPU laptop (VA-API H.264 encoder) with a Samsung Tizen (2022) TV, Sway, PipeWire.
Other hardware and TVs are untested; reports are welcome.

## Requirements

- Sway (or another wlroots compositor with `wlr-screencopy`), running on Wayland.
- A Wi-Fi card and driver with Wi-Fi Direct (`iw list` shows `P2P-GO` and `P2P-client`).
- NetworkManager with wpa_supplicant (not iwd); it exposes a `wifi-p2p` device.
- GStreamer 1.24+ with the VA plugins (`vah264lpenc` or `vah264enc`, `vapostproc`) and your GPU's VA driver.
  Without them `x264enc` is used (software encoding, more CPU and latency).
- PipeWire with `pactl` (PulseAudio compatibility) for the sound.
- Rust via [rustup](https://rustup.rs) (distribution packages are usually too old for gstreamer-rs).
- A dmenu-style launcher for the TV picker: fuzzel, wofi, rofi, bemenu or dmenu.
- `python3-dbus` (TV scan), `notify-send` and `jq` (optional but recommended).

On Debian/Ubuntu the packages are roughly: `iw network-manager gstreamer1.0-tools gstreamer1.0-plugins-base
gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly libgstreamer1.0-dev
libgstreamer-plugins-base1.0-dev pipewire-pulse pulseaudio-utils python3-dbus libnotify-bin jq intel-media-va-driver`.

## Build and install

```bash
make build
make install            # PREFIX=$HOME/.local by default; DESTDIR is supported
```

This puts `wfd-source`, `wfd-media`, `wfd-tx` and the `miracast-*` commands in `$PREFIX/bin`, helper files in
`$PREFIX/share/sway-miracast`, and the user unit in `~/.config/systemd/user/`. Nothing is enabled or started.
Make sure `$PREFIX/bin` is on the `PATH` of your Sway session.

Working from a checkout? `make install-dev` makes the same layout out of symlinks into the repo, so `make build`
and edits to `tools/` take effect at once. `make uninstall` removes both kinds.

## Enable

```bash
systemctl --user daemon-reload
systemctl --user enable sway-miracast-source.service
miracast-doctor          # checks the environment: OK / WARN / FAIL with hints
miracast-menu            # with the TV showing Smart View / screen mirroring: pick it from the list
```

Without a selected TV the service exits quietly and stays stopped. After you pick a TV in `miracast-menu` the
service starts (and restarts after a lost session). Then turn on Smart View / screen mirroring on the TV when asked.

## Commands

| Command | What it does |
| --- | --- |
| `miracast-menu` | Pick a TV (scan + known TVs), duplicate/extend, disconnect. A second call closes the menu. |
| `miracast-status` | Waybar JSON (`off`, `waiting`, `casting`). `--notify-waiting` sends the "turn on Smart View" notification. |
| `miracast-screen [toggle\|mirror\|extend\|status]` | TV shows a copy of the laptop screen or a separate screen to its right. |
| `miracast-film [toggle\|on\|off\|status\|MS]` | Film mode: picture delayed to match the TV's audio delay (experimental; mostly unnecessary since the Samsung low-latency mode). |
| `miracast-doctor` | Read-only environment check. |

## Settings

All optional, in `~/.config/sway-miracast/env` (`KEY=value` lines, read by the unit and by the commands) or in the
environment:

- `MONITOR` - output to capture (default: the internal panel eDP/LVDS/DSI, else the first output).
- `TV_MAC` - TV to use when none was picked in the menu.
- `MIRACAST_MENU` - a full dmenu-style command to use instead of the autodetected launcher.
- `MIRACAST_ICONS=nerd` - Nerd Font glyphs in the menu and the bar (default: plain Unicode markers).
- Language: English, or Polish when `LC_ALL`/`LC_MESSAGES`/`LANG` starts with `pl`.
- `SEC_LOW_LATENCY_MODE` (`on`|`off`, default `on`) - send `wfd_sec_low_latency_mode` 3 s after PLAY; only to
  sinks that send `wfd_sec_*` parameters themselves.
- `AUDIO_DELAY_MS` (default `-1000`) - audio PTS offset in ms; negative values keep the TV's audio buffer at its floor.
- `VIDEO_HOLD_MS` (default `0`) - delay the picture in the source by this many ms (`miracast-film` changes it live via `video-hold-ms`).

Read once when the service starts (restart it after a change). Live tuning during a session, in
`$XDG_RUNTIME_DIR/sway-miracast/`:

- `audio-delay-ms`, `mux-kbps` (constant TS rate, default 8000) - write a value, then `pkill -HUP -x wfd-tx`.
- `rtsp-set` - write `name: value` lines (e.g. `wfd_sec_low_latency_mode: off`); the running session sends them to
  the TV as one SET_PARAMETER and deletes the file.

## Sway and Waybar

Ready-made snippets are in `contrib/`: `sway.conf` (key bindings, and the line that hands the service a fresh
`WAYLAND_DISPLAY`/`SWAYSOCK` when Sway starts), `waybar-module.json` and `waybar-style.css`
(`custom/miracast`, signal 6, interval 5 s).

## Firewall

If a host firewall (ufw, firewalld) is active, the TV's connection to the laptop may be blocked. `miracast-doctor`
prints the exact command; for ufw it is

```bash
sudo ufw allow from 10.42.0.0/24 to any port 7236 proto tcp
```

The P2P link uses NetworkManager's shared address range 10.42.0.0/24.

## Troubleshooting

- `journalctl --user -u sway-miracast-source.service` shows the one-line JSON events of `wfd-source`.
- Reading the wpa_supplicant log makes TV detection faster: join the `systemd-journal` group.
- The TV only answers Wi-Fi Direct scans for a short time after it enters Smart View: an empty list is normal
  until you start it on the TV.
