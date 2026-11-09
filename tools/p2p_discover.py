#!/usr/bin/env python3
"""Wi-Fi Direct discovery via NetworkManager D-Bus; dumps peer properties and decodes WFD IE subelements.

Usage: p2p_discover.py [seconds] [--json out.json]
Requires: dbus-python, GLib main loop (python3-gi). No root needed (NM polkit).
"""

import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.realpath(__file__)), "lib"))

import dbus  # noqa: E402
import p2p  # noqa: E402


def main():
    secs = int(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].isdigit() else 25
    jout = sys.argv[sys.argv.index("--json") + 1] if "--json" in sys.argv else None
    bus = dbus.SystemBus()
    dev = p2p.find_p2p_device(bus)
    if dev is None:
        sys.exit("no wifi-p2p device")
    p2p.start_find(dev, min(secs, 120))
    print(f"P2P find for {secs}s ...", file=sys.stderr)
    time.sleep(secs)
    peers = []
    for _path, props in p2p.iter_peers(bus, dev):
        p = {}
        for k, v in props.items():
            if isinstance(v, dbus.Array) and k == "WfdIEs":
                raw = bytes(v)
                p["WfdIEs_hex"] = raw.hex()
                p["WfdIEs"] = p2p.decode_wfd_ies(raw)
            else:
                p[k] = v if not isinstance(v, (dbus.Array, dbus.Dictionary)) else list(v)
        peers.append(json.loads(json.dumps(p, default=str)))
    p2p.stop_find(dev)
    js = json.dumps(peers, indent=2, ensure_ascii=False)
    print(js)
    if jout:
        with open(jout, "w") as f:
            f.write(js + "\n")


if __name__ == "__main__":
    main()
