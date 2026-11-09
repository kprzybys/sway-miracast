#!/usr/bin/env python3
"""audiolat: audio latency source -> TV speaker, measured with the laptop microphone.

Pauses the media player (playerctl), plays N short tone bursts into the default sink (laptop speakers
should be muted / at 0 %: the sink monitor is taken before volume, the TV still gets the signal),
records the sink monitor (= what is sent) and the microphone (= what the TV plays) at the same time,
finds burst onsets in both and reports the delay. Resumes the player afterwards.

Usage: audiolat.py [--bursts 8] [--freq 2000] [--mic SOURCE] [--keep DIR]
"""

import argparse
import json
import os
import subprocess
import tempfile
import time
import wave

import numpy as np

ap = argparse.ArgumentParser()
ap.add_argument("--bursts", type=int, default=8)
ap.add_argument("--freq", type=float, default=2000)
ap.add_argument("--mic", default="alsa_input.pci-0000_00_1f.3.analog-stereo")
ap.add_argument("--keep", default=None)
ap.add_argument("--period", type=float, default=1.25, help="seconds between bursts")
ap.add_argument("--blen", type=float, default=0.08, help="burst length s")
ap.add_argument("--json", action="store_true")
ap.add_argument("--coded", type=int, default=0, help="cycle N frequencies (1500,2000,...) so pairing is unambiguous")
a = ap.parse_args()
R = 48000
d = a.keep or tempfile.mkdtemp(prefix="audiolat-")
os.makedirs(d, exist_ok=True)

# test signal: 80 ms bursts (5 ms ramps) every 1.25 s, after 1 s of silence
period, blen = int(a.period * R), int(a.blen * R)
sig = np.zeros(R + a.bursts * period, dtype=np.float32)
t = np.arange(blen) / R
burst = np.sin(2 * np.pi * a.freq * t) * np.minimum(1, np.minimum(t, t[::-1]) / 0.005)
FREQS = [a.freq] if not a.coded else [1500 + 500 * i for i in range(a.coded)]
for k in range(a.bursts):
    s = R + k * period
    f = FREQS[k % len(FREQS)]
    sig[s : s + blen] = 0.5 * np.sin(2 * np.pi * f * t) * np.minimum(1, np.minimum(t, t[::-1]) / 0.005)
with wave.open(f"{d}/bursts.wav", "wb") as w:
    w.setnchannels(2)
    w.setsampwidth(2)
    w.setframerate(R)
    pcm = (np.repeat(sig[:, None], 2, axis=1) * 32767).astype("<i2")
    w.writeframes(pcm.tobytes())

# the microphone must hear only the TV: mute the laptop speakers (the sink monitor, i.e. what the TV
# gets, is taken before volume/mute) and restore the previous state afterwards
# the sway-miracast null sink has no speakers to mute, and muting it would also silence its monitor
# (what we send to the TV); only mute a real default sink
default_sink = subprocess.run(["pactl", "get-default-sink"], capture_output=True, text=True).stdout.strip()
mute_default = default_sink != "sway_miracast"
was_muted = mute_default and (
    "yes"
    in subprocess.run(
        ["pactl", "get-sink-mute", "@DEFAULT_SINK@"], capture_output=True, text=True, env={**os.environ, "LC_ALL": "C"}
    ).stdout
)
if mute_default:
    subprocess.run(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "1"])
was_playing = subprocess.run(["playerctl", "status"], capture_output=True, text=True).stdout.strip() == "Playing"
if was_playing:
    subprocess.run(["playerctl", "pause"])
    time.sleep(0.5)
dur = len(sig) / R + 2.0
rec = lambda dev, out: subprocess.Popen(
    ["timeout", f"{dur:.1f}", "parec", "-d", dev, "--format=s16le", f"--rate={R}", "--channels=1", "--latency-msec=5"],
    stdout=open(out, "wb"),
)
try:
    pm = rec(a.mic, f"{d}/mic.raw")
    pr = rec("@DEFAULT_MONITOR@", f"{d}/mon.raw")
    time.sleep(0.3)
    subprocess.run(["paplay", f"{d}/bursts.wav"])
    pm.wait()
    pr.wait()
finally:
    if was_playing:
        subprocess.run(["playerctl", "play"])
    if mute_default and not was_muted:
        subprocess.run(["pactl", "set-sink-mute", "@DEFAULT_SINK@", "0"])


def onsets(x, freq):
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / R)
    X[np.abs(f - freq) > 120] = 0
    y = np.abs(np.fft.irfft(X, len(x)))
    hop = 48  # 1 ms
    n = len(y) // hop
    e = y[: n * hop].reshape(n, hop).max(1)
    thr = (np.percentile(e, 50) + e.max()) / 4
    out, i = [], 0
    while i < n:
        if e[i] > thr:
            out.append(i)  # ms
            i += int(a.period * len(FREQS) * 600)  # skip the rest of this burst (ms)
        else:
            i += 1
    return np.array(out)


mic = np.fromfile(f"{d}/mic.raw", "<i2").astype(float)
mon = np.fromfile(f"{d}/mon.raw", "<i2").astype(float)
# per frequency: aliases are len(FREQS)*period apart, larger than the latency spread
lat_t = []  # (send time, latency)
nheard = nsent = 0
for f in FREQS:
    om, oo = onsets(mic, f), onsets(mon, f)
    nheard += len(om)
    nsent += len(oo)
    diffs = [int(m - s) for s in oo for m in om if 150 <= m - s <= 1500]
    if not diffs:
        continue
    h = np.bincount(np.array(diffs) // 10)
    L0 = int(np.argmax(np.convolve(h, np.ones(3), "same"))) * 10 + 5
    for s in oo:
        j = np.argmin(np.abs(om - (s + L0)))
        if abs(om[j] - (s + L0)) <= 100:
            lat_t.append((int(s), int(om[j] - s)))
lat = [l for _, l in sorted(lat_t)]
oo, om = [0] * nsent, [0] * nheard
print(
    f"sent {len(oo)} bursts, heard {len(om)}; latency ms: "
    + (
        f"median {np.median(lat):.0f}, min {min(lat):.0f}, max {max(lat):.0f}" + ("" if len(lat) > 12 else f" ({lat})")
        if lat
        else "none matched"
    )
)
print(f"files: {d}")
if lat:
    L = np.array(lat, float)
    dl = np.diff(L)
    st = {
        "n": len(L),
        "median": float(np.median(L)),
        "p5": float(np.percentile(L, 5)),
        "p95": float(np.percentile(L, 95)),
        "std": round(float(L.std()), 1),
        "steps_ge10ms": int(np.sum(np.abs(dl) >= 10)),
        "drift_ms": float(L[-5:].mean() - L[:5].mean()),
    }
    print("jitter:", json.dumps(st))
