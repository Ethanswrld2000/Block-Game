#!/usr/bin/env python3
"""Placeholder sound effects, synthesised from noise and tones.

Writes assets/sounds/*.ogg (needs numpy and ffmpeg). Overwrites existing files, so
replace sounds you've recorded or bought by name instead of rerunning this.
`--only bird_ frog_` writes just the sounds whose names start with those prefixes.
"""
import os
import subprocess
import sys
import tempfile
import wave

import numpy as np

RATE = 44100
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "sounds")
rng = np.random.default_rng(7)
ONLY = sys.argv[sys.argv.index("--only") + 1:] if "--only" in sys.argv else []


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
    if ONLY and not any(name.startswith(p) for p in ONLY):
        return
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
        # Wading through shallow water: a splash with a watery wobble.
        sp = lowpass(noise(0.32), 0.25 + 0.05 * i) * env(int(RATE * 0.32), 0.01, 0.09)
        sp += np.sin(2 * np.pi * (420 + 60 * i) * t(0.32) * (1 + 0.6 * t(0.32))) * env(int(RATE * 0.32), 0.005, 0.05) * 0.25
        save(f"step_splash_{i}", sp)
        # Thin snow: a short icy crunch (bright grains).
        cr = highpass(noise(0.16), 0.35) * env(int(RATE * 0.16), 0.003, 0.03)
        cr *= (np.random.rand(len(cr)) < 0.35 + 0.05 * i)
        save(f"step_snow_{i}", cr)
        # Deep snow: a soft, dull slosh.
        save(f"step_slosh_{i}", lowpass(noise(0.34), 0.06 + 0.01 * i) * env(int(RATE * 0.34), 0.04, 0.1))
    # Tool hits while mining, and the block giving way.
    save("dig_soft", thud(0.14, 0.08, 0.05))
    save("dig_hard", highpass(thud(0.12, 0.6, 0.02), 0.1) + np.pad(np.sin(2 * np.pi * 1900 * t(0.08)) * env(int(RATE * 0.08), 0.001, 0.015) * 0.5, (0, int(RATE * 0.04))))
    save("dig_wood", thud(0.15, 0.2, 0.04, pitch=240))
    save("dig_plant", highpass(noise(0.15), 0.15) * env(int(RATE * 0.15), 0.005, 0.04))
    save("break", lowpass(noise(0.4), 0.15) * env(int(RATE * 0.4), 0.002, 0.12) + thud(0.4, 0.04, 0.1))
    save("place", thud(0.15, 0.1, 0.04, pitch=140))
    # Cooking: a slosh of the ladle with a few bubbles, and a knife through food onto wood.
    tt = t(0.45)
    slosh = lowpass(noise(0.45), 0.06) * np.sin(np.pi * tt / 0.45) ** 2
    bubbles = np.zeros_like(tt)
    for k, (start, f) in enumerate([(0.08, 420), (0.17, 560), (0.26, 380), (0.33, 640)]):
        seg = t(0.06)
        i = int(RATE * start)
        bubbles[i:i + len(seg)] += np.sin(2 * np.pi * (f + 900 * seg) * seg) * env(len(seg), 0.002, 0.015) * 0.5
    save("stir", slosh + bubbles)
    # A bench making something: a rising shimmer while the parts draw together, then a bright pop.
    tt = t(1.1)
    shimmer = sum(np.sin(2 * np.pi * (f + 500 * tt) * tt) for f in (880, 1320, 1760)) * (tt / 1.1) ** 2 * 0.25
    shimmer *= 0.6 + 0.4 * np.sin(2 * np.pi * 18 * tt)
    save("craft_shimmer", shimmer * np.minimum(1, (1.1 - tt) * 20))
    tt = t(0.35)
    pop = np.sin(2 * np.pi * (300 + 1400 * np.exp(-tt * 30)) * tt) * env(len(tt), 0.001, 0.05)
    chime = sum(np.sin(2 * np.pi * f * tt) * env(len(tt), 0.002, 0.12) for f in (1046, 1568, 2093)) * 0.35
    save("craft_pop", pop + chime + highpass(noise(0.35), 0.4) * env(len(tt), 0.001, 0.02) * 0.3)
    save("knife", np.concatenate([highpass(noise(0.05), 0.3) * env(int(RATE * 0.05), 0.001, 0.015) * 0.5, thud(0.12, 0.18, 0.03, pitch=260)]))
    # Fighting.
    save("swing", highpass(noise(0.22), 0.1) * np.sin(np.pi * t(0.22) / 0.22) ** 2 * 0.6)
    save("hit", thud(0.2, 0.3, 0.05, pitch=110))
    # Blades meeting: a bright, inharmonic ring with a sharp click.
    tt = t(0.6)
    ring = sum(np.sin(2 * np.pi * f * tt) * a for f, a in [(1180, 1.0), (1730, 0.7), (2610, 0.5), (3470, 0.3)]) * env(len(tt), 0.001, 0.12)
    save("clash", ring * 0.5 + highpass(noise(0.6), 0.4) * env(len(tt), 0.001, 0.01) * 0.6)
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
    # A gunshot: a sharp crack, a boom under it and a short tail.
    n = noise(0.9)
    save("gunshot", highpass(n, 0.5) * env(len(n), 0.0005, 0.03) * 1.5 + lowpass(n, 0.02) * env(len(n), 0.001, 0.25) * 4)
    save("thunder", lowpass(n, 0.004) * env(len(n), 0.02, 1.3) * 3 + highpass(n, 0.2) * env(len(n), 0.001, 0.08))

    critters()
    voices()
    ui()
    music()


def sweep(seconds, f0, f1, decay=0.05, attack=0.004):
    """A quick pitch slide with a soft start: one chirp note."""
    tt = t(seconds)
    f = f0 + (f1 - f0) * tt / seconds
    return np.sin(2 * np.pi * np.cumsum(f) / RATE) * env(len(tt), attack, decay)


def sequence(parts, gap):
    out = []
    for p in parts:
        out += [p, np.zeros(int(RATE * gap))]
    return np.concatenate(out)


def critters():
    # Birds: little runs of high slides, three different songs.
    save("bird_chirp_0", sequence([sweep(0.07, 3800, 5200), sweep(0.07, 3900, 5400), sweep(0.12, 5000, 3600, 0.08)], 0.04))
    save("bird_chirp_1", sequence([sweep(0.05, 4400, 3600, 0.03)] * 5, 0.03))
    save("bird_chirp_2", sequence([sweep(0.15, 3000, 4600, 0.1), sweep(0.2, 4600, 3800, 0.12)], 0.06))
    # Wings beating on take-off.
    n = noise(0.45)
    beats = (np.sin(2 * np.pi * 14 * t(0.45)) > 0.2) * np.exp(-t(0.45) / 0.25)
    save("bird_flap", lowpass(n, 0.12) * beats)
    # The kite: a long falling whistle.
    tt = t(1.1)
    save("kite_call", sweep(1.1, 2600, 1700, 0.6, 0.05) * (1 + 0.3 * np.sin(2 * np.pi * 7 * tt)))
    # Frogs: a low buzzy croak (a tone pulsed ~30 times a second), one and two-part.
    def croak(seconds, f):
        tt = t(seconds)
        tone = sum(np.sin(2 * np.pi * f * h * tt) / h for h in (1, 2, 3, 5))
        pulses = np.clip(np.sin(2 * np.pi * 32 * tt), 0, 1) ** 2
        return tone * pulses * np.sin(np.pi * tt / seconds) ** 0.5
    save("frog_croak_0", croak(0.45, 180))
    save("frog_croak_1", sequence([croak(0.18, 210), croak(0.3, 170)], 0.08))
    # Crickets: a high tone in quick bursts.
    def trill(bursts, f):
        tt = t(0.06)
        burst = np.sin(2 * np.pi * f * tt) * np.clip(np.sin(2 * np.pi * 50 * tt), 0, 1)
        return sequence([burst] * bursts, 0.045)
    save("cricket_chirp_0", trill(3, 4400))
    save("cricket_chirp_1", trill(4, 4700))


def voices():
    # NPC speech: one short voiced syllable per file (a buzzy tone shaped by two
    # vowel formants). The game plays one per syllable at the speaker's pitch.
    def syllable(f1, f2, seconds=0.13, f0=150):
        tt = t(seconds)
        glide = f0 * (1 + 0.08 * np.sin(np.pi * tt / seconds))
        phase = 2 * np.pi * np.cumsum(glide) / RATE
        x = 0
        for h in range(1, 30):
            f = glide * h
            w = np.exp(-((f - f1) / 120) ** 2) + 0.6 * np.exp(-((f - f2) / 200) ** 2) + 0.15 / h
            x = x + np.sin(h * phase) * w
        return x * np.sin(np.pi * tt / seconds) ** 0.6
    save("voice_0", syllable(730, 1090))  # ah
    save("voice_1", syllable(530, 1840))  # eh
    save("voice_2", syllable(570, 840))   # oh
    save("voice_3", syllable(300, 870, 0.1))  # oo


def ui():
    """Soft inventory clicks: wooden taps and a cloth rustle, kept short and quiet."""
    def tap(f, decay=0.03, seconds=0.09):
        tt = t(seconds)
        body = np.sin(2 * np.pi * f * tt) * env(len(tt), 0.001, decay)
        return body + lowpass(noise(seconds), 0.25) * env(len(tt), 0.001, 0.008) * 0.6
    def rustle(seconds, k=0.3):
        return highpass(lowpass(noise(seconds), k), 0.05) * np.sin(np.pi * t(seconds) / seconds) ** 2
    save("ui_open", np.concatenate([rustle(0.12), np.zeros(int(RATE * 0.01))]) + np.pad(tap(420, 0.05, 0.12), (0, int(RATE * 0.01))) * 0.6)
    save("ui_close", np.concatenate([tap(330, 0.04, 0.1) * 0.7, rustle(0.08, 0.2) * 0.5]))
    save("ui_tab", tap(760, 0.02, 0.07))
    save("ui_pick", tap(620, 0.025) + np.pad(rustle(0.05, 0.4) * 0.4, (0, int(RATE * 0.04))))
    save("ui_drop", tap(380, 0.035, 0.1))
    tt = t(0.22)
    clasp = tap(520, 0.03, 0.22) * 0.7 + np.pad(tap(880, 0.04, 0.14), (int(RATE * 0.08), 0)) * 0.6
    save("ui_equip", clasp + rustle(0.22, 0.3) * 0.3)
    # Drawing a blade: a rising scrape and a ring; putting it away: a falling scrape and a knock.
    tt = t(0.32)
    scrape = highpass(noise(0.32), 0.3) * np.sin(np.pi * tt / 0.32) ** 2 * 0.5
    ring = sum(np.sin(2 * np.pi * f * tt) for f in (1500, 2240)) * env(len(tt), 0.2, 0.15) * 0.25
    save("ui_draw", scrape + ring)
    save("ui_sheathe", scrape[::-1] * 0.8 + np.pad(tap(300, 0.04, 0.1), (int(RATE * 0.22), 0)))


def music():
    """Quiet placeholder music: slow pad chords and a plucked melody in a modal scale.
    `music_day_*` by day, `music_night_*` at night (game::music). Replace by name."""
    def note(midi):
        return 440.0 * 2 ** ((midi - 69) / 12)

    def pad(freqs, seconds):
        tt = t(seconds)
        x = sum(np.sin(2 * np.pi * f * tt) + 0.3 * np.sin(2 * np.pi * f * 2.003 * tt) for f in freqs)
        swell = np.sin(np.pi * tt / seconds) ** 2
        return x * swell / len(freqs)

    def pluck(f, seconds, decay):
        tt = t(seconds)
        x = np.sin(2 * np.pi * f * tt) + 0.4 * np.sin(2 * np.pi * f * 2 * tt) + 0.15 * np.sin(2 * np.pi * f * 3 * tt)
        return x * env(len(tt), 0.004, decay)

    def piece(seed, root, scale, chords, bar, bars, decay, density):
        r = np.random.default_rng(seed)
        total = int(RATE * bar * bars) + RATE * 4
        out = np.zeros(total)
        for b in range(bars):
            degs = chords[b % len(chords)]
            fs = [note(root - 12 + scale[d % len(scale)] + 12 * (d // len(scale))) for d in degs]
            x = pad(fs, bar * 1.6)
            i = int(RATE * bar * b)
            out[i:i + len(x)] += x * 0.35
            # A few melody notes per bar, staying near the last one.
            step = 0
            for k in range(8):
                if r.random() > density:
                    continue
                step = int(np.clip(step + r.integers(-2, 3), -2, 9))
                d = degs[0] + step
                f = note(root + 12 + scale[d % len(scale)] + 12 * (d // len(scale)))
                y = pluck(f, decay * 4, decay) * (0.5 + 0.3 * r.random())
                j = i + int(RATE * bar * k / 8)
                out[j:j + len(y)] += y[: max(0, total - j)]
        # A little room: two soft echoes.
        for delay, g in ((0.31, 0.3), (0.67, 0.15)):
            d = int(RATE * delay)
            out[d:] += out[:-d] * g
        fade = np.ones(total)
        n = RATE * 3
        fade[-n:] = np.linspace(1, 0, n)
        fade[:RATE] = np.linspace(0, 1, RATE)
        return lowpass(out * fade, 0.25)

    dorian = [0, 2, 3, 5, 7, 9, 10]
    lydian = [0, 2, 4, 6, 7, 9, 11]
    aeolian = [0, 2, 3, 5, 7, 8, 10]
    save("music_day_0", piece(11, 62, dorian, [[0, 2, 4], [3, 5, 7], [6, 8, 10], [4, 6, 8]], 4.0, 18, 0.9, 0.45))
    save("music_day_1", piece(23, 65, lydian, [[0, 2, 4], [1, 3, 5], [4, 6, 8], [0, 2, 4]], 3.6, 20, 0.7, 0.5))
    save("music_night_0", piece(37, 57, aeolian, [[0, 2, 4], [5, 7, 9], [3, 5, 7], [4, 6, 8]], 5.0, 14, 1.4, 0.3))


if __name__ == "__main__":
    main()
