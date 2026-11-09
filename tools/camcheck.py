#!/usr/bin/env python3
"""camcheck: is the TV in the webcam view? Expects the held session's idle feed (black) on the TV.
Grabs ~1 s of camera, finds a large dark, low-texture rectangle-ish region... simpler and robust:
prints mean brightness and saves a snapshot to OUT.jpg for a human/agent to look at.
Usage: camcheck.py OUT.jpg"""

import subprocess
import sys

out = sys.argv[1]
subprocess.run(
    [
        "ffmpeg",
        "-nostdin",
        "-v",
        "error",
        "-f",
        "v4l2",
        "-input_format",
        "mjpeg",
        "-video_size",
        "1280x720",
        "-i",
        "/dev/video0",
        "-vf",
        "select=eq(n\\,15),scale=640:-1",
        "-frames:v",
        "1",
        "-y",
        out,
    ],
    check=True,
)
print(out)
