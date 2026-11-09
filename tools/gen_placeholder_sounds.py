#!/usr/bin/env python3
"""Placeholder sound effects, synthesised from noise and tones.

Writes assets/sounds/*.ogg (needs numpy and ffmpeg). Overwrites existing files, so
replace sounds you've recorded or bought by name instead of rerunning this.
"""
import os
import subprocess
import tempfile
import wave

import numpy as np

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")
rng = np.random.default_rng(7)


def t(seconds):
    return np.arange(int(RATE * seconds)) / RATE


def env(n, attack=0.005, decay=0.1):
    x = np.arange(n) / RATE
    a = np.clip(x / max(attack, 1e-4), 0, 1)
    return a * np.exp(-x / decay)


def lowpass(x, k):
    """Cheap one-pole low-pass; k in 0..1, lower is darker."""
    y = np.empty_like(x)
    acc = 0.0
    for i, v in enumerate(x):
        acc += k * (v - acc)
        y[i] = acc
    return y


def highpass(x, k):
    return x - lowpass(x, k)


def noise(seconds):
    return rng.uniform(-1, 1, int(RATE * seconds))


def norm(x, peak=0.8):
    m = np.max(np.abs(x)) or 1.0
    return x / m * peak


def save(name, x, loop=False):
    x = norm(x, 0.7 if loop else 0.85)
    if loop:
        # Crossfade the ends so the loop has no click.
        n = int(RATE * 0.3)
        head = x[:n].copy()
        x = x[n:]
        x[-n:] = x[-n:] * np.linspace(1, 0, n) + head * np.linspace(0, 1, n)
    pcm = (x * 32767).astype(np.int16)
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
        path = f.name
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(pcm.tobytes())
    out = os.path.join(OUT, name + ".ogg")
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", path, "-c:a", "libvorbis", "-q:a", "4", out], check=True)
    os.remove(path)
    print("wrote", out)


def thud(seconds, k, decay, pitch=0.0):
    n = noise(seconds)
    x = lowpass(n, k) * env(len(n), 0.002, decay)
    if pitch:
        tt = t(seconds)
        x += np.sin(2 * np.pi * pitch * tt * (1 - tt)) * env(len(n), 0.001, decay * 0.6) * 0.6
    return x


def main():
    os.makedirs(OUT, exist_ok=True)
    # Footsteps: three variants each, picked at random in game.
    for i in range(3):
        save(f"step_soft_{i}", thud(0.18, 0.05 + 0.01 * i, 0.04))
        save(f"step_hard_{i}", highpass(thud(0.14, 0.35 + 0.05 * i, 0.025), 0.05))
        save(f"step_wood_{i}", thud(0.16, 0.12, 0.035, pitch=180 + 25 * i))
        save(f"step_plant_{i}", highpass(noise(0.2), 0.2) * env(int(RATE * 0.2), 0.02, 0.05))
    # Tool hits while mining, and the block giving way.
    save("dig_soft", thud(0.14, 0.08, 0.05))
    save("dig_hard", highpass(thud(0.12, 0.6, 0.02), 0.1) + np.pad(np.sin(2 * np.pi * 1900 * t(0.08)) * env(int(RATE * 0.08), 0.001, 0.015) * 0.5, (0, int(RATE * 0.04))))
    save("dig_wood", thud(0.15, 0.2, 0.04, pitch=240))
    save("dig_plant", highpass(noise(0.15), 0.15) * env(int(RATE * 0.15), 0.005, 0.04))
    save("break", lowpass(noise(0.4), 0.15) * env(int(RATE * 0.4), 0.002, 0.12) + thud(0.4, 0.04, 0.1))
    save("place", thud(0.15, 0.1, 0.04, pitch=140))
    # Fighting.
    save("swing", highpass(noise(0.22), 0.1) * np.sin(np.pi * t(0.22) / 0.22) ** 2 * 0.6)
    save("hit", thud(0.2, 0.3, 0.05, pitch=110))
    save("hurt", thud(0.3, 0.06, 0.09, pitch=70))
    save("creature_hurt", np.sin(2 * np.pi * (300 - 500 * t(0.25)) * t(0.25)) * env(int(RATE * 0.25), 0.01, 0.1) + 0.3 * thud(0.25, 0.2, 0.05))
    # Spirits: a breathy rising tone.
    tt = t(1.6)
    tone = np.sin(2 * np.pi * (330 + 40 * np.sin(2 * np.pi * 0.7 * tt)) * tt) * 0.4 + lowpass(noise(1.6), 0.03) * 0.8
    save("spirit", tone * np.sin(np.pi * tt / 1.6) ** 2)
    # Spells and progress.
    tt = t(0.9)
    save("spell_on", sum(np.sin(2 * np.pi * f * tt) / (k + 1) for k, f in enumerate([523, 659, 784])) * env(len(tt), 0.05, 0.35))
    tt = t(0.5)
    save("spell_off", np.sin(2 * np.pi * (600 - 300 * tt) * tt) * env(len(tt), 0.01, 0.15))
    tt = t(1.6)
    notes = np.zeros_like(tt)
    for k, f in enumerate([523, 659, 784, 1047]):
        start = int(RATE * 0.12 * k)
        seg = tt[: len(tt) - start]
        notes[start:] += np.sin(2 * np.pi * f * seg) * env(len(seg), 0.005, 0.5)
    save("level_up", notes)
    # Loops: wind, rain, levitation hum.
    save("wind_loop", lowpass(noise(8.3), 0.01) * (0.6 + 0.4 * np.sin(2 * np.pi * 0.15 * t(8.3))), loop=True)
    save("rain_loop", highpass(noise(6.3), 0.3) * 0.5 + lowpass(noise(6.3), 0.08) * 0.5, loop=True)
    tt = t(4.3)
    hum = np.sin(2 * np.pi * 110 * tt) * 0.4 + np.sin(2 * np.pi * 165 * tt) * 0.25 + lowpass(noise(4.3), 0.02) * 0.6
    save("levitate_loop", hum * (0.8 + 0.2 * np.sin(2 * np.pi * 0.5 * tt)), loop=True)
    # Thunder: a crack and a long rumble.
    n = noise(4.0)
    save("thunder", lowpass(n, 0.004) * env(len(n), 0.02, 1.3) * 3 + highpass(n, 0.2) * env(len(n), 0.001, 0.08))


if __name__ == "__main__":
    main()
