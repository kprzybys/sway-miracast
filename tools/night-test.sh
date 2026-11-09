#!/usr/bin/env bash
# night-test: long unattended test of the cast (session stability + audio to the TV).
# Every INTERVAL s switches between an empty workspace (9) and a workspace with a small terminal
# clock redrawing in place every 50 ms (8, like a TUI repainting), then measures audio latency and
# jitter with the microphone (tools/audiolat.py, coded bursts). Video health comes from the
# service's own "stats" events (captured/encoded per 10 s) in journald, dumped at the end.
# Microphone recordings are deleted after each measurement (only numbers are kept).
# The clock window inhibits idle (lock / DPMS off) while it exists; closing it restores swayidle.
# Usage: night-test.sh [hours=8] [interval_s=1200]     (run under systemd-run --user)
# Output: one directory per run under $NIGHT_TEST_DIR
#   (default: ${XDG_STATE_HOME:-~/.local/state}/sway-miracast/night-test).
set -u
T=$(cd -P "$(dirname "$0")/.." && pwd)
HOURS=${1:-8}; INTERVAL=${2:-1200}
OUT_BASE=${NIGHT_TEST_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/sway-miracast/night-test}
OUT="$OUT_BASE/$(date +%F-%H%M)"; mkdir -p "$OUT"
TMP=$(mktemp -d); SINCE=$(date '+%F %T')
log() { echo "$(date +%T) $*" | tee -a "$OUT/run.log"; }
cleanup() {
  swaymsg '[title="^night-clock$"] kill' >/dev/null 2>&1
  rm -rf "$TMP"
  journalctl --user -u sway-miracast-source --since "$SINCE" -o short-iso --no-pager > "$OUT/journal.log"
  log "end; journal saved"
}
trap cleanup EXIT
trap 'exit 0' TERM INT

log "start: ${HOURS} h, every ${INTERVAL} s, out $OUT"
systemctl --user is-active -q sway-miracast-source || { "$T/tools/devlock.sh" off >/dev/null; log "service started"; }

# terminal clock on workspace 8 (in-place redraw of one line), idle inhibited while it lives
swaymsg -q "workspace 8; exec foot -T night-clock sh -c 'while :; do printf \"\\r%s\" \"\$(date +%T.%N)\"; sleep 0.05; done'"
sleep 2
swaymsg -q '[title="^night-clock$"] inhibit_idle open' || log "WARN: idle inhibit failed"

end=$(( $(date +%s) + HOURS * 3600 )); i=0
while [ "$(date +%s)" -lt "$end" ]; do
  ws=$(( i % 2 ? 8 : 9 )); swaymsg -q "workspace $ws"; sleep 5
  st=$(systemctl --user show sway-miracast-source -p ActiveState -p NRestarts --value | paste -sd/)
  res=$(timeout 120 python3 "$T/tools/audiolat.py" --coded 4 --bursts 24 --keep "$TMP/m" 2>&1 | /usr/bin/grep -E '^(sent|jitter)' | paste -sd' ')
  rm -rf "$TMP/m"
  log "ws=$ws svc=$st $res"
  printf '{"ts":"%s","ws":%d,"svc":"%s","res":"%s"}\n' "$(date -Is)" "$ws" "$st" "${res//\"/\'}" >> "$OUT/audio.jsonl"
  i=$((i + 1)); sleep "$INTERVAL"
done
