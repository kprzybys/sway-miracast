# Sink profiles

A Miracast TV does more, and less, than the Wi-Fi Display specification says, and none of it is in its data sheet:
whether it wakes from standby, how long it stays discoverable, when it drops a session, whether it honours PTS.
Wi-Fi Alliance certification does not cover these, and no public database collects them. This directory does,
one file per TV model, and every profile answers the same list of questions, so TVs can be compared.

- One file per model: `<vendor>-<platform>-<model>.md`. How we work with sinks: `docs/METODA.md` (Polish), in short:
  the specification is the minimum, the profile is the truth about the device, reproduce before working around.
- The top of each profile is YAML (identification, what the TV announced in M3, the checklist); below it the
  behaviours in prose (Q1, Q2, …) with evidence. The questions are in [checklist.yaml](checklist.yaml).
- `make lint` checks every profile against the checklist (`tools/sink-profile.py check`) and that the table below
  is current (`tools/sink-profile.py table` rewrites it).

<!-- profiles:begin (tools/sink-profile.py table) -->
| Profile | Platform | RTSP User-Agent | Wakes from standby | TEARDOWN without video (s) | Glass to glass (ms) | Open items |
|---|---|---|---|---|---|---|
| [samsung-tizen65-hg50bu800](samsung-tizen65-hg50bu800.md) | Tizen 6.5 (2022) | `SVDTZ6_5/SEC-UIBC3` | yes, P2P frame not identified (observed) | 25 | 90 (1920x1080p60, camera) | 1 of 24 |
<!-- profiles:end -->

## Add your TV

1. Cast to the TV once with the full capability probe: add `WFD_PROBE_CAPS=1` to `~/.config/sway-miracast/env`,
   restart the service (`systemctl --user restart sway-miracast-source`) and connect from `miracast-menu`.
   The probe only asks questions; an untested sink might still dislike an unknown vendor name, so remove the line
   after the session.
2. `tools/sink-profile.py new --id <vendor>-<platform>-<model> > sinks/<vendor>-<platform>-<model>.md`
   writes a skeleton: identification and announced capabilities filled in, every checklist item `open`.
   wfd-source redacts MAC addresses and TV names, and logs only a summary of the EDID (the raw one has a serial
   number); check the file anyway before you publish it.
3. Answer what you can: set `value` and `status` (`confirmed` = reproduced on purpose, `observed` = seen once), and
   describe each behaviour as a Q section with its evidence. Leave the rest `open`.
4. `tools/sink-profile.py table`, `make lint`, and send a pull request.

## Template for a behaviour

```
### Q<n>: <short name>
- Observation:
- Evidence: (log or measurement, path, date)
- Reproduce: (how to trigger it on purpose)
- Specification: (what it says, or "none")
- Workaround: (where in the code). Status: confirmed | hypothesis | workaround in code | open
```
