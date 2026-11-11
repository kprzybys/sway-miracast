#!/usr/bin/env python3
"""Relative one-way delay of RTP packets to UDP port PORT (sniffed with AF_PACKET, needs root).

delay = arrival_time - rtp_timestamp/90000, reported relative to the minimum seen: growth = queueing between the
sender's clock and our NIC (sender queue + Wi-Fi). Usage: owd.py IFACE SECONDS [PORT]
"""

import socket
import struct
import sys
import time

iface, secs = sys.argv[1], float(sys.argv[2])
port = int(sys.argv[3]) if len(sys.argv) > 3 else 19000
s = socket.socket(socket.AF_PACKET, socket.SOCK_DGRAM, socket.htons(0x0800))
s.bind((iface, 0))
s.settimeout(0.5)
end = time.monotonic() + secs
d, wrap, last_ts, per_sec = [], 0, None, {}
t0 = time.monotonic()
while time.monotonic() < end:
    try:
        pkt = s.recv(2048)
    except TimeoutError:
        continue
    now = time.monotonic()
    if pkt[9] != 17:
        continue
    ihl = (pkt[0] & 0x0F) * 4
    if struct.unpack("!H", pkt[ihl + 2 : ihl + 4])[0] != port:
        continue
    rtp = pkt[ihl + 8 :]
    ts = struct.unpack("!I", rtp[4:8])[0]
    if last_ts is not None and ts < last_ts and last_ts - ts > 1 << 31:
        wrap += 1 << 32
    last_ts = ts
    v = now - (ts + wrap) / 90000.0
    d.append(v)
    per_sec.setdefault(int(now - t0), []).append(v)
if not d:
    print("no packets")
    sys.exit(1)
m = min(d)
rel = sorted(x - m for x in d)


def q(p):
    return rel[min(len(rel) - 1, int(p * len(rel)))] * 1000


print(
    f"packets {len(d)}  rel delay ms: p50 {q(0.5):.0f}  p90 {q(0.9):.0f}  p99 {q(0.99):.0f}  max {rel[-1] * 1000:.0f}"
)
print(
    "per second (median rel ms):",
    " ".join(f"{sorted(x - m for x in v)[len(v) // 2] * 1000:.0f}" for _, v in sorted(per_sec.items())),
)
