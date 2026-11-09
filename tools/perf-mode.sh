#!/usr/bin/env bash
# perf-mode on|off|status — maximum responsiveness while casting (run with sudo).
#   on : CPU governor+EPP performance, idle states deeper than 10 us disabled, PCIe ASPM performance,
#        Wi-Fi power save off on all wlo1/p2p interfaces, iwlmvm power_scheme=1 (active) from next load.
#   off: restore exactly what was saved by "on" (undo files), remove the iwlmvm option. Only the idle states
#        that "on" disabled are re-enabled (listed in $UNDO.cstates); ones disabled by someone else stay off.
# Costs battery and heat; meant for casting sessions on mains power.
set -u
UNDO=/var/lib/sway-miracast/perf-undo
CSTATES=$UNDO.cstates   # one "cpuN/cpuidle/stateM" per idle state that "on" switched off
MODCONF=/etc/modprobe.d/sway-miracast-iwlmvm.conf
[ "$(id -u)" = 0 ] || { echo "run with sudo"; exit 1; }
cpus=(/sys/devices/system/cpu/cpu[0-9]*)
fails=0

# wr VALUE FILE: write and report failure (a missing optional knob is not an error: pass "opt" as 3rd arg)
wr() {
  [ "${3:-}" = opt ] && [ ! -e "$2" ] && return 0
  if ! printf '%s\n' "$1" > "$2" 2>/dev/null; then
    echo "WARN: cannot write '$1' to $2" >&2
    fails=$((fails+1))
  fi
}

status() {
  echo "governor: $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null)" \
       "epp: $(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null)"
  echo -n "idle states disabled (cpu0):"
  for s in /sys/devices/system/cpu/cpu0/cpuidle/state*; do [ "$(cat $s/disable)" = 1 ] && echo -n " $(cat $s/name)"; done; echo
  echo "aspm: $(cat /sys/module/pcie_aspm/parameters/policy)"
  for i in /sys/class/net/wlo* /sys/class/net/wlp* /sys/class/net/p2p-*; do [ -e "$i" ] || continue; i=${i##*/}; echo "$i: $(iw dev $i get power_save 2>&1)"; done
  echo "iwlmvm power_scheme: $(cat /sys/module/iwlmvm/parameters/power_scheme) (option file: $( [ -e $MODCONF ] && echo yes || echo no))"
}

case ${1:-status} in
on)
  mkdir -p "$(dirname $UNDO)"
  if [ ! -e $UNDO ]; then   # keep the first saved state (idempotent "on")
    {
      echo "gov=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)"
      echo "epp=$(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference 2>/dev/null)"
      echo "aspm=$(sed -n 's/.*\[\(.*\)\].*/\1/p' /sys/module/pcie_aspm/parameters/policy)"
    } > $UNDO
  fi
  for c in "${cpus[@]}"; do
    wr performance "$c/cpufreq/scaling_governor" opt
    wr performance "$c/cpufreq/energy_performance_preference" opt
    for s in "$c"/cpuidle/state*; do
      [ -e "$s/latency" ] || continue
      if [ "$(cat "$s/latency")" -gt 10 ] && [ "$(cat "$s/disable")" = 0 ]; then
        wr 1 "$s/disable"
        # remember only states that we really switched off
        [ "$(cat "$s/disable")" = 1 ] && echo "${s#/sys/devices/system/cpu/}" >> $CSTATES
      fi
    done
  done
  echo performance > /sys/module/pcie_aspm/parameters/policy 2>/dev/null || echo "aspm: not changeable at runtime"
  for i in /sys/class/net/wlo* /sys/class/net/wlp* /sys/class/net/p2p-*; do
    [ -e "$i" ] || continue; i=${i##*/}
    iw dev $i set power_save off 2>/dev/null || { echo "WARN: power_save off failed on $i" >&2; fails=$((fails+1)); }
  done
  echo "options iwlmvm power_scheme=1" > $MODCONF || { echo "WARN: cannot write $MODCONF" >&2; fails=$((fails+1)); }
  echo "perf-mode ON (iwlmvm power_scheme=1 applies after the Wi-Fi driver reloads / reboot)"
  status
  [ $fails -eq 0 ] || { echo "perf-mode: $fails write(s) failed (see WARN above)" >&2; exit 1; };;
off)
  # shellcheck source=/dev/null
  [ -e "$UNDO" ] && . "$UNDO"
  for c in "${cpus[@]}"; do
    wr "${gov:-powersave}" "$c/cpufreq/scaling_governor" opt
    [ -n "${epp:-}" ] && wr "$epp" "$c/cpufreq/energy_performance_preference" opt
  done
  if [ -e $CSTATES ]; then
    while IFS= read -r s; do wr 0 "/sys/devices/system/cpu/$s/disable"; done < $CSTATES
  else
    echo "note: no record of idle states disabled by 'on'; leaving them as they are"
  fi
  echo "${aspm:-default}" > /sys/module/pcie_aspm/parameters/policy 2>/dev/null || echo "aspm: not changeable at runtime"
  rm -f $MODCONF $UNDO $CSTATES
  echo "perf-mode OFF (restored)"
  status
  [ $fails -eq 0 ] || { echo "perf-mode: $fails write(s) failed (see WARN above)" >&2; exit 1; };;
status) status;;
*) echo "usage: $0 on|off|status"; exit 2;;
esac
