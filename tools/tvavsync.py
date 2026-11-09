#!/usr/bin/env python3
"""tvavsync: A/V offset on the TV itself — camera sees the TV picture, the microphone hears the TV sound.

Plays a flash+beep clip (white frame and 1 kHz tone together, once a second) fullscreen in mpv on the
laptop (mirrored to the TV), records the camera (/dev/video0, kernel timestamps -> wallclock) and the
microphone (PulseAudio, wallclock) in one ffmpeg, then for every flash: offset = beep onset - TV light
half-rise. Positive = sound late. Also prints the sink monitor -> microphone audio latency.

Camera timing error: up to one frame (33 ms at 30 fps) plus exposure; compare runs (e.g. with a video hold
of +200 ms the offset must drop by 200) rather than trusting the absolute to better than ~30 ms.

Usage: tvavsync.py [--secs 30] [--keep DIR] [--json]
"""

import argparse
import json
import os
import subprocess
import tempfile
import time

import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument("--secs", type=int, default=30)
ap.add_argument("--mic", default="alsa_input.pci-0000_00_1f.3.analog-stereo")
ap.add_argument("--keep", default=None)
ap.add_argument("--json", action="store_true")
a = ap.parse_args()
d = a.keep or tempfile.mkdtemp(prefix="tvavsync-")
os.makedirs(d, exist_ok=True)
SR = 48000
W, H = 64, 36

clip = f"{d}/avsync.mp4"
if not os.path.exists(clip):
    subprocess.run(
        [
            "ffmpeg", "-loglevel", "error", "-y",
            "-f", "lavfi", "-i",
            "color=c=black:s=1920x1080:r=60:d=10,"
            "drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='lt(mod(t,1),0.1)'",
            "-f", "lavfi", "-i", "aevalsrc='if(lt(mod(t,1),0.1),0.6*sin(2*PI*1000*t),0)':s=48000:d=10",
            "-c:v", "libx264", "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac", clip,
        ],
        check=True,
    )  # fmt: skip

subprocess.run(["playerctl", "-a", "pause"], stderr=subprocess.DEVNULL)
dur = str(a.secs + 4)
rec = subprocess.Popen(
    [
        "ffmpeg", "-nostdin", "-loglevel", "error", "-y",
        "-f", "v4l2", "-input_format", "mjpeg", "-video_size", "640x360", "-framerate", "30",
        "-ts", "mono2abs", "-thread_queue_size", "512", "-t", dur, "-i", "/dev/video0",
        "-f", "pulse", "-sample_rate", str(SR), "-channels", "1", "-fragment_size", "960",
        "-thread_queue_size", "512", "-t", dur, "-i", a.mic,
        "-f", "pulse", "-sample_rate", str(SR), "-channels", "1", "-fragment_size", "960",
        "-thread_queue_size", "512", "-t", dur, "-i", "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
        "-map", "0:v", "-map", "1:a", "-map", "2:a", "-c:v", "copy", "-c:a", "pcm_s16le",
        "-copyts", f"{d}/rec.nut",
    ]
)  # fmt: skip
time.sleep(2)
mpv = subprocess.Popen(["mpv", "--fs", "--loop=inf", "--really-quiet", clip])
time.sleep(a.secs)
mpv.terminate()
rec.wait()

# camera: per-frame wallclock + 64x36 gray
pts = [
    float(x)
    for x in subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "frame=pts_time", "-of", "csv=p=0",
         f"{d}/rec.nut"],
        capture_output=True, text=True, check=True,
    ).stdout.split()
]  # fmt: skip
raw = subprocess.run(
    ["ffmpeg", "-loglevel", "error", "-copyts", "-i", f"{d}/rec.nut", "-map", "0:v", "-vf",
     f"scale={W}:{H},format=gray", "-f", "rawvideo", "-"],
    capture_output=True, check=True,
).stdout  # fmt: skip
fr = np.frombuffer(raw, np.uint8).reshape(-1, H * W).astype(float)
n = min(len(fr), len(pts))
fr, vt = fr[:n], np.array(pts[:n])
# TV region: cells that change most (flashes); the laptop screen faces away from the camera
var = fr.std(axis=0)
roi = var > 0.5 * var.max()
sig = fr[:, roi].mean(axis=1)
lo, hi = np.percentile(sig, 10), np.percentile(sig, 95)
half = (lo + hi) / 2
rises = []
for i in range(1, n):
    if sig[i - 1] < half <= sig[i] and (not rises or vt[i] - rises[-1] > 0.5):
        f = (half - sig[i - 1]) / (sig[i] - sig[i - 1])
        rises.append(vt[i - 1] + f * (vt[i] - vt[i - 1]))


def audio(idx):
    """(start wallclock, samples) of audio stream idx (1 = mic, 2 = monitor)."""
    t0 = float(
        subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", f"a:{idx - 1}", "-show_entries", "packet=pts_time",
             "-read_intervals", "%+#1", "-of", "csv=p=0", f"{d}/rec.nut"],
            capture_output=True, text=True, check=True,
        ).stdout.split()[0]
    )  # fmt: skip
    pcm = subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-i", f"{d}/rec.nut", "-map", f"0:{idx}", "-f", "s16le", "-"],
        capture_output=True, check=True,
    ).stdout  # fmt: skip
    return t0, np.frombuffer(pcm, "<i2").astype(float)


def onsets(t0, x):
    """1 kHz burst onsets (wallclock): quadrature 1 kHz envelope, 5 ms resolution, half of peak."""
    t = np.arange(len(x)) / SR
    hop = SR // 200
    k = len(x) // hop
    i = (x[: k * hop] * np.cos(2 * np.pi * 1000 * t[: k * hop])).reshape(k, hop).sum(1)
    q = (x[: k * hop] * np.sin(2 * np.pi * 1000 * t[: k * hop])).reshape(k, hop).sum(1)
    env = np.convolve(np.hypot(i, q), np.ones(2) / 2, "same")
    th = 0.5 * np.percentile(env, 99)
    out = []
    for j in range(1, k):
        if env[j - 1] < th <= env[j] and (not out or (j * hop / SR + t0) - out[-1] > 0.5):
            f = (th - env[j - 1]) / (env[j] - env[j - 1])
            out.append(t0 + (j - 1 + f) * hop / SR)
    return np.array(out)


mic_on = onsets(*audio(1))
mon_on = onsets(*audio(2))


def pair(a_ts, b_ts, lim=0.6):
    return [float(b_ts[np.argmin(abs(b_ts - x))] - x) for x in a_ts if len(b_ts) and min(abs(b_ts - x)) < lim]


av = np.array(pair(np.array(rises), mic_on)) * 1000
alat = np.array(pair(mon_on, mic_on)) * 1000
res = {
    "flashes": len(rises),
    "beeps_tv": len(mic_on),
    "av_ms_median": round(float(np.median(av)), 1) if len(av) else None,
    "av_ms_p10_p90": [round(float(np.percentile(av, p)), 1) for p in (10, 90)] if len(av) else None,
    "av_ms_std": round(float(av.std()), 1) if len(av) else None,
    "audio_lat_ms_median": round(float(np.median(alat)), 1) if len(alat) else None,
    "roi_cells": int(roi.sum()),
    "dir": d,
}
if a.json:
    print(json.dumps(res))
else:
    print(f"flashes {res['flashes']}, TV beeps {res['beeps_tv']}, roi {res['roi_cells']} cells")
    print(
        f"A/V on TV (sound - picture): median {res['av_ms_median']} ms, p10-p90 {res['av_ms_p10_p90']}, "
        f"std {res['av_ms_std']}  (+ = sound late)"
    )
    print(f"audio monitor -> TV speaker: median {res['audio_lat_ms_median']} ms")
    print("per flash:", [round(x) for x in av])
    print(f"files: {d}")
subprocess.run(["playerctl", "-a", "play"], stderr=subprocess.DEVNULL)
