#!/usr/bin/env python3
"""sink_sim: WFD sink simulator with the Samsung TV's answers recorded in the 2026-11-08 probe (data/probe/capabilities.md). Test-only.

Connects to a source on HOST:7236, completes M1-M7, receives RTP/TS on UDP 19002 for SECONDS,
writes the TS payload to OUT.ts and prints a JSON summary. Usage: sink_sim.py [HOST] [SECONDS] [OUT.ts]
"""

import json
import os
import re
import socket
import sys
import time

HOST = sys.argv[1] if len(sys.argv) > 1 else "127.0.0.1"
SECS = float(sys.argv[2]) if len(sys.argv) > 2 else 5
OUT = sys.argv[3] if len(sys.argv) > 3 else "sink_sim.ts"
PORT = int(os.environ.get("RTSP_PORT", 7236))
CAPS = {
    "wfd_audio_codecs": "LPCM 00000003 00, AAC 00000001 00",
    "wfd_video_formats": "40 00 01 10 000001e3 0f3fffff 00000fff 00 0000 00c8 01 none none",
    "wfd_content_protection": "HDCP2.1 port=9999",
    "wfd_client_rtp_ports": "RTP/AVP/UDP;unicast 19002 0 mode=play",
    "microsoft_latency_management_capability": "supported",
    "wfd_sec_low_latency_mode": "off",
    "wfd_sec_view_mode": "multi",
}
UA = "User-Agent: SVDTZ6_5/SEC-UIBC3 (sink_sim)\r\n"
udp = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
udp.bind(("0.0.0.0", 19002))
udp.settimeout(0.2)
s = socket.create_connection((HOST, PORT), timeout=10)
buf = b""
cseq = 0
log = []


def read():
    global buf
    while b"\r\n\r\n" not in buf:
        buf += s.recv(65536)
    head, rest = buf.split(b"\r\n\r\n", 1)
    m = re.search(rb"(?im)^content-length:\s*(\d+)", head)
    n = int(m.group(1)) if m else 0
    while len(rest) < n:
        rest += s.recv(65536)
    buf = rest[n:]
    return head.decode(), rest[:n].decode()


def reply(head, body="", extra=""):
    c = re.search(r"(?im)^cseq:\s*(\d+)", head).group(1)
    msg = f"RTSP/1.0 200 OK\r\nCSeq: {c}\r\n{UA}{extra}"
    if body:
        msg += f"Content-Type: text/parameters\r\nContent-Length: {len(body)}\r\n"
    s.sendall((msg + "\r\n" + body).encode())


def request(text):
    global cseq
    cseq += 1
    s.sendall(text.replace("CSEQ", str(cseq)).encode())


url = None
session = None
while True:
    h, b = read()
    first = h.split("\r\n")[0]
    log.append(first)
    if first.startswith("OPTIONS"):
        reply(
            h, extra="Public: OPTIONS, GET_PARAMETER, PAUSE, PLAY, SETUP, SET_PARAMETER, TEARDOWN, org.wfa.wfd1.0\r\n"
        )
        request(f"OPTIONS * RTSP/1.0\r\nCSeq: CSEQ\r\nRequire: org.wfa.wfd1.0\r\n{UA}\r\n")
    elif first.startswith("GET_PARAMETER"):
        names = [l.strip() for l in b.splitlines() if l.strip()]
        reply(h, "".join(f"{n}: {CAPS[n]}\r\n" for n in names if n in CAPS))
    elif first.startswith("SET_PARAMETER"):
        reply(h)
        m = re.search(r"wfd_presentation_URL:\s*(\S+)", b)
        if m:
            url = m.group(1)
        if "wfd_trigger_method: SETUP" in b:
            request(
                f"SETUP {url} RTSP/1.0\r\nCSeq: CSEQ\r\nTransport: RTP/AVP/UDP;unicast;client_port=19002\r\n{UA}\r\n"
            )
            h2, _ = read()
            session = re.search(r"(?im)^session:\s*([^;\r\n]+)", h2).group(1)
            request(f"PLAY {url} RTSP/1.0\r\nCSeq: CSEQ\r\nSession: {session}\r\n{UA}\r\n")
            read()
            break
    elif first.startswith("RTSP/1.0"):
        pass
t_end = time.time() + SECS
pk = 0
seqs = []
first_t = None
with open(OUT, "wb") as f:
    while time.time() < t_end:
        try:
            d = udp.recv(2048)
        except TimeoutError:
            continue
        first_t = first_t or time.time()
        pk += 1
        seqs.append(int.from_bytes(d[2:4], "big"))
        f.write(d[12:])
lost = sum((b - a - 1) & 0xFFFF for a, b in zip(seqs, seqs[1:], strict=False))
s.close()
print(json.dumps({"rtsp": log, "rtp_packets": pk, "rtp_lost": lost, "pt": 33, "out": OUT}))
