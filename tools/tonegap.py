#!/usr/bin/env python3
"""tonegap: steady 1 kHz tone to the TV, finds dropouts (dips >12 dB, >=3 ms) in the sink monitor (sent)
and in the microphone (played by the TV). Works in the current directory. Usage: tonegap.py [seconds=30]"""

import os
import subprocess
import sys
import time
import wave

import numpy as np

R = 48000
DUR = float(sys.argv[1]) if len(sys.argv) > 1 else 30
F = 1000
t = np.arange(int(R * DUR)) / R
s = 0.3 * np.sin(2 * np.pi * F * t)
s[:2400] *= np.linspace(0, 1, 2400)
s[-2400:] *= np.linspace(1, 0, 2400)
with wave.open("tone.wav", "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(R)
    w.writeframes((np.repeat(s[:, None], 2, 1) * 32767).astype("<i2").tobytes())
env = {**os.environ, "LC_ALL": "C"}
muted = (
    "yes"
    in subprocess.run(["pactl", "get-sink-mute", "@DEFAULT_SINK@"], capture_output=True, text=True, env=env).stdout
)
playing = subprocess.run(["playerctl", "status"], capture_output=True, text=True).stdout.strip() == "Playing"
subprocess.run(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "1"])
if playing:
    subprocess.run(["playerctl", "pause"])
    time.sleep(0.5)
try:
    pm = subprocess.Popen(
        [
            "timeout",
            f"{DUR + 2.5}",
            "parec",
            "-d",
            "alsa_input.pci-0000_00_1f.3.analog-stereo",
            "--format=s16le",
            f"--rate={R}",
            "--channels=1",
            "--latency-msec=5",
        ],
        stdout=open("mic.raw", "wb"),
    )
    po = subprocess.Popen(
        [
            "timeout",
            f"{DUR + 2.5}",
            "parec",
            "-d",
            "@DEFAULT_MONITOR@",
            "--format=s16le",
            f"--rate={R}",
            "--channels=1",
            "--latency-msec=5",
        ],
        stdout=open("mon.raw", "wb"),
    )
    time.sleep(0.3)
    subprocess.run(["paplay", "tone.wav"])
    pm.wait()
    po.wait()
finally:
    if playing:
        subprocess.run(["playerctl", "play"])
    if not muted:
        subprocess.run(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "0"])


def env_db(x):
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / R)
    X[np.abs(f - F) > 60] = 0
    y = np.abs(np.fft.irfft(X, len(x)))
    h = 48
    n = len(y) // h
    e = y[: n * h].reshape(n, h).max(1)  # 1 ms
    return 20 * np.log10(e + 1e-3)


for name in ["mon", "mic"]:
    x = np.fromfile(f"{name}.raw", "<i2").astype(float)
    e = env_db(x)
    on = np.where(e > e.max() - 20)[0]
    e = e[on[0] + 300 : on[-1] - 300]  # steady part
    ref = np.median(e)
    dip = e < ref - 12
    d = np.diff(np.concatenate([[0], dip.astype(int), [0]]))
    s = np.where(d == 1)[0]
    en = np.where(d == -1)[0]
    ev = [(int(a), int(b - a)) for a, b in zip(s, en, strict=False) if b - a >= 3]
    print(f"{name}: {len(e) / 1000:.1f} s steady, level {ref:.1f} dB, dips >12 dB >=3 ms: {len(ev)}", ev[:20])
