#!/usr/bin/env python3
"""tvwindow: measure how long the sink stays discoverable (and "session available") after the user
enters Smart View. Keeps a P2P find running (restarted every 30 s) and samples the peer once a second
from NetworkManager: LastSeen age and WFD device-info. Never connects.

Usage: tvwindow.py MAC SECONDS OUT.jsonl     (prints state changes; full samples go to OUT)
"""

import json
import os
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.realpath(__file__)), "lib"))

import dbus  # noqa: E402
import p2p  # noqa: E402

mac, secs, out = sys.argv[1].upper(), float(sys.argv[2]), open(sys.argv[3], "a", buffering=1)
bus = dbus.SystemBus()
dev = p2p.find_p2p_device(bus)
if dev is None:
    sys.exit("no wifi-p2p device")


def boottime():
    return time.clock_gettime(time.CLOCK_BOOTTIME)


def sample():
    for _path, props in p2p.iter_peers(bus, dev):
        if str(props.get("HwAddress", "")).upper() != mac:
            continue
        return int(props.get("LastSeen", -1)), p2p.device_info(props.get("WfdIEs", []))
    return None


t0, last_find, prev = time.time(), 0, None
try:
    while time.time() - t0 < secs:
        if time.time() - last_find > 6:
            if not p2p.start_find(dev, 5):
                print("find already running", flush=True)
            last_find = time.time()
        s = sample()
        now = time.time()
        if s is None:
            rec = {"t": round(now, 2), "peer": False}
        else:
            age = boottime() - s[0] if s[0] >= 0 else None
            di = s[1]
            rec = {
                "t": round(now, 2),
                "peer": True,
                "age_s": round(age, 1) if age is not None else None,
                "devinfo": f"0x{di:04x}" if di is not None else None,
                "avail": ((di >> 4) & 3) == 1 if di is not None else None,
            }
        out.write(json.dumps(rec) + "\n")
        visible = rec.get("peer") and rec.get("age_s") is not None and rec["age_s"] <= 5
        state = (visible, rec.get("devinfo"))
        if state != prev:
            print(
                f"{time.strftime('%H:%M:%S')} visible={visible} devinfo={rec.get('devinfo')} age={rec.get('age_s')}",
                flush=True,
            )
            prev = state
        time.sleep(1)
finally:
    p2p.stop_find(dev)
