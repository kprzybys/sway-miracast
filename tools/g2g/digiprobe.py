#!/usr/bin/env python3
"""digiprobe: the receiver's pipeline (as wfd_sink's gst-launch) plus a probe after the decoder that reads the
stripes.py time code from the decoded picture and logs "<wallclock> <counter>" (Pi clock) to $DIGIPROBE_LOG.
Also logs every Nth frame's arrival at the decoder input as "in <wallclock> <pts>" for queue analysis.
Usage (as WFD_SINK_PIPELINE_CMD): digiprobe.py RTP_PORT"""

import os
import sys
import time

import gi

gi.require_version("Gst", "1.0")
gi.require_version("GstVideo", "1.0")
from gi.repository import GLib, Gst, GstVideo  # noqa: E402

Gst.init(None)
port = int(sys.argv[1]) if len(sys.argv) > 1 else 19000
log = open(os.environ.get("DIGIPROBE_LOG", "/tmp/digiprobe.log"), "w", buffering=1)
NB = 12
p = Gst.parse_launch(
    f"udpsrc port={port} buffer-size=1048576 skip-first-bytes=12 caps=video/mpegts,systemstream=true,packetsize=188 "
    "! tsdemux name=d d. ! video/x-h264 ! h264parse name=parse ! v4l2h264dec name=dec ! kmssink sync=false "
    "d. ! audio/x-private-ts-lpcm ! fakesink sync=false async=false"
)
state = {"n": 0, "info": None}


def on_caps(pad, _pspec):
    caps = pad.get_current_caps()
    if caps:
        # DMA_DRM caps do not map to GstVideoInfo directly; width/height are enough, stride comes from the meta
        s = caps.get_structure(0)
        state["info"] = (s.get_value("width"), s.get_value("height"))


def dec_in(pad, info):
    b = info.get_buffer()
    log.write(f"in {time.time():.4f} {b.pts}\n")
    return Gst.PadProbeReturn.OK


def dec_out(pad, info):
    state["n"] += 1
    if state["n"] % 6 or state["info"] is None:
        return Gst.PadProbeReturn.OK
    t = time.time()
    buf = info.get_buffer()
    w, h = state["info"]
    meta = GstVideo.buffer_get_video_meta(buf)
    stride = meta.stride[0] if meta else w
    offset = meta.offset[0] if meta else 0
    ok, m = buf.map(Gst.MapFlags.READ)
    if not ok:
        return Gst.PadProbeReturn.OK
    try:
        row = offset + stride * (h // 2)
        v = [m.data[row + int((i + 0.5) * w / (NB + 2))] for i in range(NB + 2)]
    finally:
        buf.unmap(m)
    if not (v[0] > 160 and v[-1] < 90) or any(90 <= x <= 160 for x in v[1:-1]):
        log.write(f"bad {t:.4f} {buf.pts} {' '.join(map(str, v))}\n")
        return Gst.PadProbeReturn.OK
    g = 0
    for x in v[1:-1]:
        g = (g << 1) | (1 if x > 128 else 0)
    c, s = g, g >> 1
    while s:
        c ^= s
        s >>= 1
    log.write(f"out {t:.4f} {buf.pts} {c}\n")
    return Gst.PadProbeReturn.OK


src = p.get_by_name("dec").get_static_pad("src")
src.connect("notify::caps", on_caps)
src.add_probe(Gst.PadProbeType.BUFFER, dec_out)
p.get_by_name("dec").get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, dec_in)
p.set_state(Gst.State.PLAYING)
loop = GLib.MainLoop()
bus = p.get_bus()
bus.add_signal_watch()
bus.connect("message::error", lambda *_: loop.quit())
try:
    loop.run()
except KeyboardInterrupt:
    pass
p.set_state(Gst.State.NULL)
