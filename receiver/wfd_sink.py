#!/usr/bin/env python3
"""wfd-sink: minimal Miracast (Wi-Fi Display R1) sink for a small Linux board (tested target: Raspberry Pi Zero W).

One job: accept one WFD source at a time and show its stream on HDMI. The station Wi-Fi stays up: P2P goes
through the wpa_supplicant that NetworkManager already runs (control socket of `p2p-dev-<if>`), so SSH over
the same radio keeps working. MiracleCast cannot do that (it spawns its own wpa_supplicant on the interface).

Flow (each step has a deadline; any failure ends the session and returns to LISTEN):
  LISTEN   WFD IE "primary sink, session available", P2P listen, PBC from any source (or --source MAC only)
  GROUP    we are GO (go_intent 15): static IP on the group interface, dnsmasq hands one lease to the source
  RTSP     we connect to <source>:7236, answer M1-M4, send M2/M6/M7 (OPTIONS, SETUP, PLAY), keepalive replies
  PLAY     gst-launch: udpsrc (RTP header skipped) -> tsdemux -> v4l2h264dec -> kmssink (+ LPCM -> alsasink)
The decoder pipeline starts as soon as the group is up: gst-launch needs ~7 s on a Zero W, which overlaps
DHCP and RTSP instead of adding to them.

Log: one JSON object per line on stderr ({"event": ...}). Needs root (IP address, dnsmasq, wpa control socket).
Usage: wfd_sink.py [--iface wlan0] [--name NAME] [--source MAC] [--no-audio] [--rtp-port 19000]
"""

from __future__ import annotations

import argparse
import contextlib
import json
import os
import re
import select
import shlex
import signal
import socket
import subprocess
import sys
import time
from dataclasses import dataclass, field

RUN_DIR = "/run/wfd-sink"
WPA_DIR = "/run/wpa_supplicant"
RTSP_PORT = 7236
GO_IP = "192.168.173.1"
DHCP_RANGE = "192.168.173.10,192.168.173.20,255.255.255.0,1h"

# Deadlines (seconds)
LEASE_WAIT = 30.0
RTSP_CONNECT_WAIT = 15.0
RTSP_IDLE = 35.0  # the source sends a keepalive GET_PARAMETER every <= 30 s (WFD 6.4.12)
PLAY_WAIT = 20.0

# WFD device information subelement (WFD 5.1.2): 2-byte length, bitmap, RTSP port, max throughput (Mb/s).
DEV_PRIMARY_SINK = 0x0001
SESSION_AVAILABLE = 0x0010
MAX_TPUT_MBPS = 50

# M3 answers. Video: CBP, level 4.0, CEA 640x480p60 + 1280x720p30/p60 + 1920x1080p30 (bits 0, 5, 6, 7);
# native 1920x1080p30 (CEA index 7 << 3). No 1080p60: the Zero W decodes 1080p30 at ~50% CPU.
VIDEO_FORMATS = "38 00 01 04 000000e1 00000000 00000000 00 0000 0000 00 none none"
AUDIO_LPCM = "LPCM 00000002 00"  # 48 kHz 16 bit stereo, the only audio our pipeline decodes (dvdlpcmdec)


def log(event: str, **kw: object) -> None:
    print(json.dumps({"event": event, **kw}, ensure_ascii=False), file=sys.stderr, flush=True)


def device_info_subelement(available: bool) -> str:
    """WFD_SUBELEM_SET 0 argument: hex of length + body (device info bitmap, RTSP port, throughput)."""
    bitmap = DEV_PRIMARY_SINK | (SESSION_AVAILABLE if available else 0)
    return f"0006{bitmap:04x}{RTSP_PORT:04x}{MAX_TPUT_MBPS:04x}"


def m3_reply(names: list[str], rtp_port: int) -> str:
    """Body of the reply to the source's GET_PARAMETER (M3). Unknown parameters are answered with 'none'."""
    caps = {
        "wfd_video_formats": VIDEO_FORMATS,
        "wfd_audio_codecs": AUDIO_LPCM,
        "wfd_client_rtp_ports": f"RTP/AVP/UDP;unicast {rtp_port} 0 mode=play",
        "wfd_content_protection": "none",
        "wfd_display_edid": "none",
        "wfd_coupled_sink": "none",
        "wfd_uibc_capability": "none",
        "wfd_standby_resume_capability": "none",
        "wfd_connector_type": "05",  # HDMI
    }
    return "".join(f"{n}: {caps.get(n, 'none')}\r\n" for n in names)


def parse_params(body: str) -> dict[str, str]:
    out = {}
    for line in body.splitlines():
        k, sep, v = line.partition(":")
        if sep:
            out[k.strip()] = v.strip()
    return out


def dhcp_server(nmcli_options: str) -> str | None:
    """dhcp_server_identifier from `nmcli -g DHCP4.OPTION device show` ('a = b | c = d' or one per line)."""
    m = re.search(r"dhcp_server_identifier\s*=\s*(\d+\.\d+\.\d+\.\d+)", nmcli_options)
    return m.group(1) if m else None


def lease_ip(text: str) -> str | None:
    """First IPv4 address in a dnsmasq lease file ('<expiry> <mac> <ip> <name> <client-id>')."""
    for line in text.splitlines():
        f = line.split()
        if len(f) >= 3 and re.fullmatch(r"\d+\.\d+\.\d+\.\d+", f[2]):
            return f[2]
    return None


# --- wpa_supplicant control interface -------------------------------------------------------------------


class Wpa:
    """One unix datagram socket to wpa_supplicant's control interface (commands, or events after ATTACH)."""

    def __init__(self, ifname: str, tag: str):
        self.local = f"{RUN_DIR}/wpa-{os.getpid()}-{tag}"
        with contextlib.suppress(FileNotFoundError):
            os.unlink(self.local)
        self.s = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
        self.s.bind(self.local)
        self.s.connect(f"{WPA_DIR}/{ifname}")

    def cmd(self, c: str, timeout: float = 5.0) -> str:
        self.s.send(c.encode())
        end = time.monotonic() + timeout
        while (left := end - time.monotonic()) > 0:
            r, _, _ = select.select([self.s], [], [], left)
            if r:
                msg = self.s.recv(8192).decode(errors="replace")
                if not msg.startswith("<"):  # unsolicited event on an attached socket
                    return msg.strip()
        raise TimeoutError(c.split()[0])

    def ok(self, c: str) -> None:
        r = self.cmd(c)
        if r != "OK":
            raise RuntimeError(f"{c.split()[0]}: {r}")

    def fileno(self) -> int:
        return self.s.fileno()

    def close(self) -> None:
        self.s.close()
        with contextlib.suppress(FileNotFoundError):
            os.unlink(self.local)


class Events:
    """wpa_supplicant events (P2P-*), with lines stripped of the '<level>' prefix."""

    def __init__(self, ifname: str):
        self.w = Wpa(ifname, "ev")
        self.w.ok("ATTACH")

    def fileno(self) -> int:
        return self.w.fileno()

    def read(self) -> list[str]:
        out = []
        while select.select([self.w.s], [], [], 0)[0]:
            msg = self.w.s.recv(8192).decode(errors="replace").strip()
            out.append(re.sub(r"^<\d+>", "", msg))
        return out

    def close(self) -> None:
        with contextlib.suppress(Exception):
            self.w.cmd("DETACH", 1)
        self.w.close()


# --- RTSP ------------------------------------------------------------------------------------------------


@dataclass
class Msg:
    first: str
    headers: dict[str, str]
    body: str

    @property
    def is_response(self) -> bool:
        return self.first.startswith("RTSP/")

    @property
    def cseq(self) -> int:
        return int(self.headers.get("cseq", "0"))


@dataclass
class Rtsp:
    sock: socket.socket
    buf: bytes = b""
    cseq: int = 0
    pending: dict[int, str] = field(default_factory=dict)

    def read(self) -> Msg | None:
        """Next complete message from the buffer, or None (call feed() when the socket is readable)."""
        if b"\r\n\r\n" not in self.buf:
            return None
        head, rest = self.buf.split(b"\r\n\r\n", 1)
        lines = head.decode(errors="replace").split("\r\n")
        headers = {}
        for line in lines[1:]:
            k, _, v = line.partition(":")
            headers[k.strip().lower()] = v.strip()
        n = int(headers.get("content-length", "0"))
        if len(rest) < n:
            return None
        self.buf = rest[n:]
        return Msg(lines[0], headers, rest[:n].decode(errors="replace"))

    def feed(self) -> bool:
        data = self.sock.recv(65536)
        self.buf += data
        return bool(data)

    def reply(self, req: Msg, body: str = "", extra: str = "") -> None:
        msg = f"RTSP/1.0 200 OK\r\nCSeq: {req.cseq}\r\n{extra}"
        if body:
            msg += f"Content-Type: text/parameters\r\nContent-Length: {len(body)}\r\n"
        self.sock.sendall((msg + "\r\n" + body).encode())

    def request(self, method: str, url: str, extra: str = "") -> None:
        self.cseq += 1
        self.pending[self.cseq] = method
        self.sock.sendall(f"{method} {url} RTSP/1.0\r\nCSeq: {self.cseq}\r\n{extra}\r\n".encode())


# --- media pipeline ---------------------------------------------------------------------------------------


def pipeline_cmd(rtp_port: int, audio: bool) -> list[str]:
    """gst-launch argv. Cheapest path measured on a Zero W (2026-11-11, 6 Mb/s, gst CPU 47% -> 42%, idle 37% -> 54%):
    no rtpjitterbuffer (direct link, in order) and no rtpmp2tdepay: udpsrc drops the fixed 12-byte RTP header
    (WFD sources send no CSRC/extension) and tsdemux gets 7 TS packets per datagram."""
    ts = "video/mpegts,systemstream=true,packetsize=188"
    dump = os.environ.get("WFD_SINK_DUMP")  # debug: also write the received TS to this file
    video_sink = "kmssink sync=false"
    if os.environ.get("WFD_SINK_STATS"):  # debug: rendered/dropped counters in the log (gst-launch -v)
        video_sink = "fpsdisplaysink text-overlay=false sync=false fps-update-interval=2000 video-sink=kmssink"
    p = (
        f'udpsrc port={rtp_port} buffer-size=1048576 skip-first-bytes=12 caps="{ts}" ! '
        + (f"tee name=t t. ! queue ! filesink location={dump} async=false t. ! queue ! " if dump else "")
        + "tsdemux name=d "
        f"d. ! video/x-h264 ! h264parse ! v4l2h264dec ! {video_sink} "
    )
    # async=false: tsdemux exposes no pad for WFD LPCM (stream type 0x83 without an HDMV registration descriptor),
    # and an async sink that never gets data keeps the whole pipeline in PAUSED on the first video frame.
    if audio:
        p += "d. ! audio/x-private-ts-lpcm ! queue ! dvdlpcmdec ! audioconvert ! audioresample ! alsasink sync=false async=false"
    else:
        p += "d. ! audio/x-private-ts-lpcm ! fakesink sync=false async=false"
    verbose = ["-v"] if os.environ.get("WFD_SINK_STATS") else ["-q"]
    return ["gst-launch-1.0", *verbose, *shlex.split(p)]


# --- one session ------------------------------------------------------------------------------------------


class SessionEnd(Exception):
    pass


STOP = False  # set by SIGTERM/SIGINT; checked by the main loop and inside a session


@dataclass
class Session:
    args: argparse.Namespace
    ctl: Wpa
    events: Events
    ifname: str
    role: str = "GO"
    procs: list[subprocess.Popen] = field(default_factory=list)

    def check_events(self) -> None:
        if STOP:
            raise SessionEnd("stopping")
        for e in self.events.read():
            if e.startswith(("P2P-GROUP-REMOVED", "P2P-GROUP-FORMATION-FAILURE")):
                raise SessionEnd(e.split()[0].lower())
        # brcmfmac: the group interface can vanish without P2P-GROUP-REMOVED (only "group interface not found")
        if not os.path.exists(f"/sys/class/net/{self.ifname}"):
            raise SessionEnd("group interface gone")

    def spawn(self, name: str, cmd: list[str]) -> subprocess.Popen:
        p = subprocess.Popen(cmd, stdin=subprocess.DEVNULL)
        self.procs.append(p)
        log("spawn", name=name, pid=p.pid)
        return p

    def wait_lease(self, leases: str) -> str:
        end = time.monotonic() + LEASE_WAIT
        while time.monotonic() < end:
            self.check_events()
            with contextlib.suppress(FileNotFoundError):
                with open(leases) as f:
                    if ip := lease_ip(f.read()):
                        return ip
            select.select([self.events], [], [], 0.2)
        raise SessionEnd("no DHCP lease")

    def wait_dhcp_server(self) -> str:
        end = time.monotonic() + LEASE_WAIT
        while time.monotonic() < end:
            self.check_events()
            r = subprocess.run(
                ["nmcli", "-g", "DHCP4.OPTION", "device", "show", f"p2p-dev-{self.args.iface}"],
                capture_output=True,
                text=True,
            )
            if ip := dhcp_server(r.stdout):
                return ip
            select.select([self.events], [], [], 0.3)
        raise SessionEnd("no DHCP from the source")

    def connect(self, ip: str) -> socket.socket:
        end = time.monotonic() + RTSP_CONNECT_WAIT
        while True:
            self.check_events()
            try:
                return socket.create_connection((ip, RTSP_PORT), timeout=2)
            except OSError as e:
                if time.monotonic() > end:
                    raise SessionEnd(f"rtsp connect: {e}") from e
                time.sleep(0.5)

    def run(self) -> None:
        a = self.args
        if a.p2p == "nm":
            # NM may make either side the GO (it passes no intent of its own and ignores p2p_go_intent).
            # GO: NM turns ipv4.method=auto into shared (dnsmasq), the lease file names the source.
            # client: the source is the GO, i.e. our DHCP server (NM keeps DHCP4 on p2p-dev-<if>, not the group if).
            gst = self.spawn("gst", pipeline_cmd(a.rtp_port, not a.no_audio))
            if self.role == "GO":
                ip = self.wait_lease(f"/var/lib/NetworkManager/dnsmasq-{self.ifname}.leases")
            else:
                ip = self.wait_dhcp_server()
            log("source_ip", ip=ip)
            sock = self.connect(ip)
            log("rtsp_connected", ip=ip)
            self.rtsp(Rtsp(sock), gst)
            return
        # --p2p wpa (a system without NetworkManager): we own the group, so we configure it ourselves.
        subprocess.run(["ip", "addr", "replace", f"{GO_IP}/24", "dev", self.ifname], check=True)
        subprocess.run(["ip", "link", "set", self.ifname, "up"], check=True)
        leases = f"{RUN_DIR}/leases-{self.ifname}"
        with contextlib.suppress(FileNotFoundError):
            os.unlink(leases)
        dnsmasq = ["dnsmasq", "--keep-in-foreground", "--conf-file=/dev/null", "--port=0", "--bind-interfaces"]
        dnsmasq += [f"--interface={self.ifname}", "--except-interface=lo", f"--dhcp-range={DHCP_RANGE}"]
        dnsmasq += ["--dhcp-option=3", "--dhcp-option=6", "--dhcp-authoritative", f"--dhcp-leasefile={leases}"]
        self.spawn("dnsmasq", dnsmasq)
        gst = self.spawn("gst", pipeline_cmd(a.rtp_port, not a.no_audio))
        ip = self.wait_lease(leases)
        log("source_ip", ip=ip)
        sock = self.connect(ip)
        log("rtsp_connected", ip=ip)
        self.rtsp(Rtsp(sock), gst)

    def rtsp(self, r: Rtsp, gst: subprocess.Popen) -> None:
        a = self.args
        url = None
        session = None
        playing_by = time.monotonic() + PLAY_WAIT
        last_rx = time.monotonic()
        while True:
            self.check_events()
            if gst.poll() is not None:
                raise SessionEnd(f"pipeline exited {gst.returncode}")
            if session is None and time.monotonic() > playing_by:
                raise SessionEnd("no PLAY within deadline")
            if time.monotonic() - last_rx > RTSP_IDLE:
                raise SessionEnd("rtsp idle")
            ready, _, _ = select.select([r.sock, self.events], [], [], 1.0)
            if r.sock in ready:
                if not r.feed():
                    raise SessionEnd("rtsp closed by source")
                last_rx = time.monotonic()
            while (m := r.read()) is not None:
                if m.is_response:
                    method = r.pending.pop(m.cseq, "?")
                    if " 200 " not in m.first + " ":
                        raise SessionEnd(f"{method}: {m.first}")
                    if method == "SETUP":
                        session = m.headers.get("session", "").split(";")[0]
                        r.request("PLAY", url, f"Session: {session}\r\n")
                    elif method == "PLAY":
                        log("playing", session=session)
                    continue
                method = m.first.split()[0]
                if method == "OPTIONS":  # M1
                    r.reply(m, extra="Public: org.wfa.wfd1.0, GET_PARAMETER, SET_PARAMETER\r\n")
                    r.request("OPTIONS", "*", "Require: org.wfa.wfd1.0\r\n")  # M2
                elif method == "GET_PARAMETER":  # M3, or keepalive with an empty body
                    names = [n.strip() for n in m.body.splitlines() if n.strip()]
                    r.reply(m, m3_reply(names, a.rtp_port))
                elif method == "SET_PARAMETER":
                    p = parse_params(m.body)
                    r.reply(m)
                    if "wfd_presentation_URL" in p:  # M4
                        url = p["wfd_presentation_URL"].split()[0]
                        log("m4", video=p.get("wfd_video_formats"), audio=p.get("wfd_audio_codecs"))
                    trig = p.get("wfd_trigger_method")
                    if trig == "SETUP":  # M5
                        transport = f"RTP/AVP/UDP;unicast;client_port={a.rtp_port}"
                        r.request("SETUP", url, f"Transport: {transport}\r\n")
                    elif trig == "TEARDOWN":
                        r.request("TEARDOWN", url, f"Session: {session}\r\n")
                        raise SessionEnd("source asked for teardown")
                elif method == "TEARDOWN":
                    r.reply(m)
                    raise SessionEnd("teardown")
                else:
                    log("rtsp_unhandled", first=m.first)

    def cleanup(self) -> None:
        for p in reversed(self.procs):
            if p.poll() is None:
                p.terminate()
                try:
                    p.wait(3)
                except subprocess.TimeoutExpired:
                    p.kill()
                    p.wait()
        if self.args.p2p == "nm":
            nm_down()
        with contextlib.suppress(Exception):
            self.ctl.cmd(f"P2P_GROUP_REMOVE {self.ifname}")
        remove_stale_groups(self.ifname.rsplit("-", 1)[0])


# --- NetworkManager mode ----------------------------------------------------------------------------------
# With NM running, NM calls wpa_supplicant's RemoveInterface on every P2P group it did not start (seen on D-Bus,
# 2026-11-11: the GO dies ~80 ms after P2P-GROUP-STARTED, also with p2p-dev unmanaged). So NM has to start the
# group: on the source's GO negotiation request we activate an NM wifi-p2p connection to that peer, with
# ipv4.method=auto (NM serves DHCP itself when it ends up GO). p2p_go_intent=15 is set but NM did not honour
# it in the 2026-11-11 test (we became client), so both roles are handled.

NM_CON = "wfd-sink"


def nm_connect(p2p_dev: str, mac: str) -> subprocess.Popen:
    nm_down()
    ies = ":".join(f"{b:02x}" for b in bytes.fromhex("00" + device_info_subelement(False)))
    add = ["nmcli", "connection", "add", "type", "wifi-p2p", "con-name", NM_CON, "ifname", p2p_dev]
    add += ["wifi-p2p.peer", mac, "wifi-p2p.wfd-ies", ies, "connection.autoconnect", "no"]
    add += ["ipv4.method", "auto", "ipv4.never-default", "yes", "ipv6.method", "disabled"]
    r = subprocess.run(add, capture_output=True, text=True)
    log("nm_connection_add", ok=r.returncode == 0, err=r.stderr.strip()[:200])
    return subprocess.Popen(["nmcli", "--wait", "45", "connection", "up", NM_CON], stdout=subprocess.DEVNULL)


def nm_down() -> None:
    subprocess.run(["nmcli", "connection", "delete", NM_CON], capture_output=True)


def remove_stale_groups(prefix: str) -> None:
    """Delete leftover P2P group netdevs (`p2p-wlan0-N`). brcmfmac allows one: a stale one makes the next
    group fail with 'Failed to create interface p2p-wlan0-1: -16 (Device or resource busy)'."""
    for name in os.listdir("/sys/class/net"):
        if name.startswith(prefix + "-"):
            r = subprocess.run(["iw", "dev", name, "del"], capture_output=True, text=True)
            log("stale_group_removed", ifname=name, ok=r.returncode == 0)


# --- main loop --------------------------------------------------------------------------------------------


def setup(ctl: Wpa, name: str) -> None:
    ctl.ok(f"SET device_name {name}")
    ctl.ok("SET device_type 7-0050F204-1")  # Displays / Television
    ctl.ok("SET config_methods pbc")
    ctl.ok("SET wifi_display 1")
    set_available(ctl, True)


def set_available(ctl: Wpa, available: bool) -> None:
    ctl.ok(f"WFD_SUBELEM_SET 0 {device_info_subelement(available)}")


def listen(ctl: Wpa) -> None:
    ctl.cmd("P2P_STOP_FIND")
    ctl.ok("P2P_LISTEN")
    log("listening")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--iface", default="wlan0", help="station interface (P2P device is p2p-dev-<iface>)")
    ap.add_argument("--name", default=socket.gethostname(), help="name shown to sources")
    ap.add_argument("--source", help="accept only this source MAC (P2P device address)")
    ap.add_argument("--no-audio", action="store_true", help="drop audio (still offered: some sources need it)")
    ap.add_argument("--rtp-port", type=int, default=19000)
    ap.add_argument(
        "--p2p",
        choices=["nm", "wpa"],
        default="nm",
        help="who forms the P2P group: NetworkManager (default; NM removes groups it did not start) or wpa_supplicant directly",
    )
    a = ap.parse_args()
    only = a.source.lower() if a.source else None

    os.makedirs(RUN_DIR, exist_ok=True)
    p2p_dev = f"p2p-dev-{a.iface}"
    ctl = Wpa(p2p_dev, "ctl")
    events = Events(p2p_dev)

    def on_signal(*_: object) -> None:
        global STOP
        STOP = True

    signal.signal(signal.SIGTERM, on_signal)
    signal.signal(signal.SIGINT, on_signal)

    remove_stale_groups(f"p2p-{a.iface}")
    if a.p2p == "nm":
        nm_down()
        ctl.ok("SET p2p_go_intent 15")
    setup(ctl, a.name)
    listen(ctl)
    try:
        while not STOP:
            ready, _, _ = select.select([events], [], [], 1.0)
            if not ready:
                continue
            for e in events.read():
                f = e.split()
                kind = f[0]
                if kind in ("P2P-GO-NEG-REQUEST", "P2P-PROV-DISC-PBC-REQ") and len(f) > 1:
                    mac = f[1].lower()
                    if only and mac != only:
                        log("ignored_source", kind=kind)
                        continue
                    if a.p2p == "nm":
                        if kind == "P2P-GO-NEG-REQUEST":
                            nm_up = nm_connect(p2p_dev, mac)
                            log("p2p_connect", trigger=kind, via="nm", pid=nm_up.pid)
                        continue
                    auth = " auth" if kind == "P2P-PROV-DISC-PBC-REQ" else ""
                    r = ctl.cmd(f"P2P_CONNECT {mac} pbc{auth} go_intent=15")
                    log("p2p_connect", trigger=kind, result=r)
                elif kind == "P2P-GROUP-STARTED" and len(f) > 2:
                    ifname, role = f[1], f[2]
                    log("group_started", ifname=ifname, role=role)
                    s = Session(a, ctl, events, ifname, role)
                    try:
                        if role != "GO" and a.p2p == "wpa":
                            raise SessionEnd(f"we are {role}, need GO (source uses DHCP)")
                        set_available(ctl, False)
                        s.run()
                    except SessionEnd as end:
                        log("session_end", reason=str(end))
                    except Exception as ex:  # one broken session must not kill the receiver
                        log("session_error", error=repr(ex))
                    finally:
                        s.cleanup()
                        setup(ctl, a.name)  # NM rewrites the WFD IEs during its connection
                        listen(ctl)
                elif kind in ("P2P-GO-NEG-FAILURE", "P2P-GROUP-FORMATION-FAILURE", "P2P-FIND-STOPPED"):
                    log("p2p", kind=kind, detail=" ".join(f[1:2]))
                    if kind != "P2P-FIND-STOPPED":
                        listen(ctl)
    finally:
        with contextlib.suppress(Exception):
            set_available(ctl, False)
            ctl.cmd("P2P_STOP_FIND")
            ctl.cmd("SET wifi_display 0")
            ctl.cmd("SET p2p_go_intent 7")
            nm_down()
        events.close()
        ctl.close()
        log("stopped")
    return 0


if __name__ == "__main__":
    sys.exit(main())
