#!/usr/bin/env bash
# devlock on|off: take the TV for development (stops the mirroring service) or give it back.
# SVC overrides the service (default sway-miracast-source).
D=${XDG_RUNTIME_DIR:-/run/user/$UID}/sway-miracast; mkdir -p "$D"
SVC=${SVC:-sway-miracast-source}
case ${1:-} in
  on)  touch "$D/dev.lock"; systemctl --user stop "$SVC" 2>/dev/null; echo "dev.lock on, $SVC stopped";;
  off) rm -f "$D/dev.lock"; systemctl --user start "$SVC"; echo "dev.lock off, $SVC started";;
  *)   [ -e "$D/dev.lock" ] && echo on || echo off;;
esac
