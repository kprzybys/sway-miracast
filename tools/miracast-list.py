#!/usr/bin/env python3
"""Wi-Fi Display sinks (TVs) nearby, for the TV picker (tools/miracast-menu.sh).

Usage: miracast-list [--stream] [seconds]   (0 = no scan, only NetworkManager's peer cache)
Output: one TSV line per sink: MAC  STRENGTH(0-100)  AVAILABLE(1/0/?)  NAME
--stream prints each sink as soon as it shows up during the scan (polled every 0.5 s, flushed), so the
menu can open at once and grow, instead of a blank wait for the whole scan.
Only peers with a WFD IE whose device type is a sink (primary/secondary/dual-role) are listed;
phones and laptops (WFD source) are skipped. A TV answers P2P finds only for a short time
after entering Smart View, so an empty list is normal.
A scan takes the radio off the Wi-Fi channel for a moment: do not scan during a cast.
"""

import os
import sys
import time

# `python3 -I` (miracast-menu) drops the script directory from sys.path: add lib/ explicitly
# (tools/lib in the repo, ../lib when installed).
_here = os.path.dirname(os.path.realpath(__file__))
for _lib in (os.path.join(_here, "lib"), os.path.join(_here, "..", "lib")):
    if os.path.isfile(os.path.join(_lib, "p2p.py")):
        sys.path.insert(0, _lib)
        break

import dbus  # noqa: E402
import p2p  # noqa: E402

POLL_S = 0.5


def sinks(bus, dev):
    """Yield (MAC, TSV line) for each WFD sink among the known peers."""
    for _path, props in p2p.iter_peers(bus, dev):
        info = p2p.sink_info(props.get("WfdIEs", []))
        if info is None or info[0] == 0:  # no WFD or a WFD source
            continue
        mac = str(props["HwAddress"]).upper()
        name = str(props.get("Name", "")).replace("\t", " ").strip() or mac
        yield mac, f"{mac}\t{int(props.get('Strength', 0))}\t{1 if info[1] else 0}\t{name}"


def main():
    args = sys.argv[1:]
    stream = "--stream" in args
    args = [a for a in args if a != "--stream"]
    secs = int(args[0]) if args else 5
    bus = dbus.SystemBus()
    dev = p2p.find_p2p_device(bus)
    if dev is None:
        sys.exit("no Wi-Fi P2P device in NetworkManager")
    seen = set()

    def emit():
        for mac, line in sinks(bus, dev):
            if mac not in seen:
                seen.add(mac)
                print(line, flush=True)

    if secs > 0:
        p2p.start_find(dev, secs)  # a find already running (wfd-source waits for its TV) still fills Peers
        end = time.monotonic() + secs
        while (left := end - time.monotonic()) > 0:
            time.sleep(min(POLL_S, left))
            if stream:
                emit()
        p2p.stop_find(dev)
    emit()


if __name__ == "__main__":
    main()
