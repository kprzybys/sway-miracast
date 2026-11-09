#!/usr/bin/env bash
# linkmon: one job - report link health as one event line per change (stdout -> journald).
#   - STA: SSID/BSSID/channel/signal changes
#   - P2P: group interface up/down, sink associated, RTSP (7236) ESTAB up/down
#   - reachability: 1 ping/s to $PING_HOST and to the sink; logs transitions + per-minute summary
#   - wpa_supplicant/NM key events (P2P-*, AP-STA-*, CTRL-EVENT-(DIS)CONNECTED)
# Usage: linkmon.sh   (env: STA_IF=wlo1 PING_HOST=1.1.1.1 SINK_IP=10.42.0.103)
# Run it by hand while casting, e.g. `tools/linkmon.sh | tee linkmon.log`, or as a transient user unit:
#   systemd-run --user --unit=miracast-linkmon tools/linkmon.sh   (then: journalctl --user -u miracast-linkmon)
set -u
STA_IF=${STA_IF:-wlo1}; PING_HOST=${PING_HOST:-1.1.1.1}; SINK_IP=${SINK_IP:-10.42.0.103}
ts(){ date +%T.%3N; }
ev(){ echo "$(ts) $*"; }

journalctl -f -n0 -o cat -u wpa_supplicant -u NetworkManager 2>/dev/null \
  | grep --line-buffered -E "P2P-(GO-NEG|GROUP|INVITATION|PROV|DEVICE-LOST)|AP-STA-(DIS)?CONNECTED|$STA_IF: CTRL-EVENT-(DIS)?CONNECTED|FORMATION" \
  | while IFS= read -r l; do ev "SYS ${l:0:200}"; done &

sta(){ iw dev "$STA_IF" link 2>/dev/null | awk '/^Connected/{b=$3}/SSID/{s=$2}/freq/{f=$2}/signal/{g=$2}END{if(b)printf "%s %s %dMHz %sdBm",s,b,f,g; else print "DOWN"}'; }
p2p(){ local i; i=$(iw dev | awk '/Interface p2p-/{print $2; exit}'); [ -z "$i" ] && { echo none; return; }
  echo "$i@$(iw dev "$i" info | awk '/channel/{print $2}') sta=$(iw dev "$i" station dump | grep -c ^Station) rtsp=$(ss -Htn state established '( sport = :7236 or dport = :7236 )' | wc -l)"; }
probe(){ ping -c1 -W1 "$1" 2>/dev/null | awk -F'time=' '/time=/{split($2,a," ");print a[1]; f=1}END{if(!f)print "LOST"}'; }

last_sta=""; last_p2p=""; last_net=""; last_sink=""
n=0; lost=0; sum=0; max=0; sl=0; sn=0
while :; do
  s=$(sta); [ "${s% *}" != "${last_sta% *}" ] && ev "STA $s"; last_sta=$s
  p=$(p2p); [ "$p" != "$last_p2p" ] && ev "P2P $p"; last_p2p=$p
  r=$(probe "$PING_HOST"); n=$((n+1))
  if [ "$r" = LOST ]; then lost=$((lost+1)); st=LOST; else st=OK; sum=$(awk -v a=$sum -v b=$r 'BEGIN{print a+b}'); max=$(awk -v a=$max -v b=$r 'BEGIN{print (b>a)?b:a}'); fi
  [ "$st" != "$last_net" ] && ev "NET $PING_HOST $st"; last_net=$st
  if [ "$p" != none ]; then k=$(probe "$SINK_IP"); sn=$((sn+1)); [ "$k" = LOST ] && { sl=$((sl+1)); ks=LOST; } || ks=OK
    [ "$ks" != "$last_sink" ] && ev "SINK $SINK_IP $ks"; last_sink=$ks; fi
  if [ $n -ge 60 ]; then
    ok=$((n-lost)); avg=$([ $ok -gt 0 ] && awk -v a=$sum -v b=$ok 'BEGIN{printf "%.1f",a/b}' || echo -)
    ev "SUMMARY 60s net loss=$lost/$n avg=${avg}ms max=${max}ms sink loss=$sl/$sn | STA ${last_sta}"
    n=0; lost=0; sum=0; max=0; sl=0; sn=0
  fi
  sleep 1
done
