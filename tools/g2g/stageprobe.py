#!/usr/bin/env python3
"""stageprobe: the receiver's video path with pad probes; per frame (by PTS) the monotonic time it passes
demux-out, dec-in, dec-out, sink-in. Prints median/max delay between stages. Usage: stageprobe.py SECONDS [PORT]"""

import statistics
import sys
import time

import gi

gi.require_version("Gst", "1.0")
from gi.repository import GLib, Gst  # noqa: E402

Gst.init(None)
secs = float(sys.argv[1])
port = 19000
dec_props = sys.argv[2] if len(sys.argv) > 2 else ""
sink = sys.argv[3] if len(sys.argv) > 3 else "kmssink sync=false"
sink, sink_props = (sink.split(" ", 1) + [""])[:2]
p = Gst.parse_launch(
    f"udpsrc port={port} buffer-size=1048576 skip-first-bytes=12 caps=video/mpegts,systemstream=true,packetsize=188 "
    f"! tsdemux name=d d. ! video/x-h264 ! h264parse name=parse ! v4l2h264dec name=dec {dec_props} ! {sink} name=sink {sink_props}"
)
seen = {}  # pts -> {stage: t}
first_arrival = {}


def probe(stage):
    def cb(pad, info):
        b = info.get_buffer()
        if b and b.pts != Gst.CLOCK_TIME_NONE:
            seen.setdefault(b.pts, {})[stage] = time.monotonic()
        return Gst.PadProbeReturn.OK

    return cb


p.get_by_name("parse").get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, probe("demux"))
p.get_by_name("dec").get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, probe("dec_in"))
p.get_by_name("dec").get_static_pad("src").add_probe(Gst.PadProbeType.BUFFER, probe("dec_out"))
p.get_by_name("sink").get_static_pad("sink").add_probe(Gst.PadProbeType.BUFFER, probe("sink"))
p.set_state(Gst.State.PLAYING)
loop = GLib.MainLoop()
GLib.timeout_add(int(secs * 1000), loop.quit)
loop.run()
p.set_state(Gst.State.NULL)

stages = ["demux", "dec_in", "dec_out", "sink"]
full = [v for v in seen.values() if all(s in v for s in stages)]
print(f"frames seen {len(seen)}, with all stages {len(full)}")
for a, b in zip(stages, stages[1:], strict=False):
    d = [(v[b] - v[a]) * 1000 for v in full]
    if d:
        print(f"{a:>7} -> {b:<7} median {statistics.median(d):7.1f} ms  max {max(d):7.1f} ms")
# demux output vs expected pace: PTS-relative arrival drift (growing = upstream backlog)
ks = sorted(k for k in seen if "demux" in seen[k])
if len(ks) > 2:
    rel = [(seen[k]["demux"] - (k - ks[0]) / 1e9) for k in ks]
    m = min(rel)
    print(
        f"demux-out vs PTS pace: start {(rel[0] - m) * 1000:.0f} ms, end {(rel[-1] - m) * 1000:.0f} ms, max {(max(rel) - m) * 1000:.0f} ms"
    )
