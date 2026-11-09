#!/usr/bin/env python3
"""Wi-Fi Display sinks (TVs) nearby, for the TV picker (tools/miracast-menu.sh).

Usage: miracast-list [seconds]   (0 = no scan, only NetworkManager's peer cache)
Output: one TSV line per sink: MAC  STRENGTH(0-100)  AVAILABLE(1/0/?)  NAME
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


def main():
    secs = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    bus = dbus.SystemBus()
    dev = p2p.find_p2p_device(bus)
    if dev is None:
        sys.exit("no Wi-Fi P2P device in NetworkManager")
    if secs > 0:
        p2p.start_find(dev, secs)  # a find already running (wfd-source waits for its TV) still fills Peers
        time.sleep(secs)
        p2p.stop_find(dev)
    for _path, props in p2p.iter_peers(bus, dev):
        info = p2p.sink_info(props.get("WfdIEs", []))
        if info is None or info[0] == 0:  # no WFD or a WFD source
            continue
        name = str(props.get("Name", "")).replace("\t", " ").strip() or str(props["HwAddress"])
        print(f"{str(props['HwAddress']).upper()}\t{int(props.get('Strength', 0))}\t{1 if info[1] else 0}\t{name}")


if __name__ == "__main__":
    main()
