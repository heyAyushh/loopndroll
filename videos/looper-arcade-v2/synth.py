"""Original chiptune score + SFX for the Looper arcade launch video (v2).

Everything is generated from oscillators (no samples), so the soundtrack is
original and license-free. Scripted moments come from assets/timing.json;
pinball sounds come from build/events.json, the physics sim's collision log,
so every bumper, flipper and drain is heard on the exact frame it happens.

Usage: node render.mjs --events && python3 synth.py  ->  build/soundtrack.wav
"""

import json
import pathlib
import wave

import numpy as np
from scipy.signal import butter, lfilter

HERE = pathlib.Path(__file__).parent
TIMING = json.loads((HERE / "assets/timing.json").read_text())
EVENTS = json.loads((HERE / "build/events.json").read_text())
SAMPLE_RATE = 44100
BEAT = 60 / TIMING["bpm"]
BAR = 4 * BEAT
EIGHTH = BEAT / 2
SIXTEENTH = BEAT / 4
TOTAL_SECONDS = TIMING["total"]
MASTER_PEAK = 0.89  # about -1 dBFS
rng = np.random.default_rng(1998)  # seeded so every build sounds identical

left = np.zeros(int(TOTAL_SECONDS * SAMPLE_RATE))
right = np.zeros_like(left)


def first(kind, default=None):
    return next((e["t"] for e in EVENTS if e["type"] == kind), default)


# ---------- oscillators ----------


def hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def timeline(duration):
    return np.arange(int(duration * SAMPLE_RATE)) / SAMPLE_RATE


def envelope(duration, attack=0.005, decay=0.1, sustain=0.6, release=0.05):
    t = timeline(duration)
    env = np.where(t < attack, t / attack, sustain + (1 - sustain) * np.exp(-(t - attack) / max(decay, 1e-4)))
    tail = t > duration - release
    env[tail] *= np.clip((duration - t[tail]) / release, 0, 1)
    return env


def phase_of(freq, duration):
    """Integrated phase so frequency sweeps stay click-free."""
    return np.cumsum(np.broadcast_to(freq, timeline(duration).shape)) / SAMPLE_RATE


def square(freq, duration, duty=0.5):
    return np.where((phase_of(freq, duration) % 1) < duty, 1.0, -1.0)


def triangle(freq, duration):
    return 4 * np.abs((phase_of(freq, duration) % 1) - 0.5) - 1


def saw(freq, duration):
    return 2 * (phase_of(freq, duration) % 1) - 1


def sine(freq, duration):
    return np.sin(2 * np.pi * phase_of(freq, duration))


def noise(duration):
    return rng.uniform(-1, 1, len(timeline(duration)))


def lowpass(signal, cutoff):
    b, a = butter(2, min(cutoff, SAMPLE_RATE / 2 - 100) / (SAMPLE_RATE / 2))
    return lfilter(b, a, signal)


def highpass(signal, cutoff):
    b, a = butter(2, cutoff / (SAMPLE_RATE / 2), btype="high")
    return lfilter(b, a, signal)


def place(start, signal, gain=1.0, pan=0.0):
    begin = int(start * SAMPLE_RATE)
    if begin >= len(left) or begin < 0:
        return
    end = min(len(left), begin + len(signal))
    chunk = signal[: end - begin] * gain
    left[begin:end] += chunk * (1 - max(pan, 0))
    right[begin:end] += chunk * (1 + min(pan, 0))


# ---------- instruments ----------


def lead(start, midi, duration, gain=0.16, duty=0.25, vibrato=0.0, pan=0.1):
    t = timeline(duration)
    freq = hz(midi) * (1 + vibrato * np.sin(2 * np.pi * 5.5 * t) * np.clip(t / 0.15, 0, 1))
    place(start, square(freq, duration, duty) * envelope(duration, decay=0.18, sustain=0.55), gain, pan)


def brass(start, midi, duration, gain=0.12):
    tone = square(hz(midi), duration, 0.5) + 0.6 * saw(hz(midi) * 1.004, duration)
    place(start, lowpass(tone, 3500) * envelope(duration, attack=0.02, decay=0.3, sustain=0.8, release=0.15), gain)


def bass(start, midi, duration, gain=0.22):
    tone = triangle(hz(midi), duration) + 0.35 * square(hz(midi), duration, 0.5)
    place(start, lowpass(tone, 1400) * envelope(duration, decay=0.08, sustain=0.5, release=0.02), gain)


def pluck(start, midi, gain=0.08, pan=-0.2):
    duration = 0.35
    place(start, triangle(hz(midi), duration) * envelope(duration, decay=0.09, sustain=0.0), gain, pan)


def bell(start, midi, duration=0.9, gain=0.12, pan=0.0):
    f = hz(midi)
    tone = sine(f, duration) + 0.4 * sine(f * 2.76, duration) + 0.2 * sine(f * 5.4, duration)
    place(start, tone * envelope(duration, attack=0.002, decay=0.35, sustain=0.0), gain, pan)


def pad(start, midis, duration, gain=0.05, cutoff=1800):
    tone = sum(saw(hz(m), duration) + saw(hz(m) * 1.006, duration) for m in midis)
    place(start, lowpass(tone, cutoff) * envelope(duration, attack=0.4, decay=1.0, sustain=0.8, release=0.5), gain)


def kick(start, gain=0.5):
    duration = 0.25
    t = timeline(duration)
    place(start, sine(50 + 120 * np.exp(-t * 30), duration) * np.exp(-t * 12), gain)


def snare(start, gain=0.22):
    duration = 0.18
    t = timeline(duration)
    body = 0.5 * sine(190, duration) * np.exp(-t * 30) + highpass(noise(duration), 1200) * np.exp(-t * 18)
    place(start, body, gain)


def hat(start, gain=0.06, open_hat=False):
    duration = 0.25 if open_hat else 0.05
    t = timeline(duration)
    place(start, highpass(noise(duration), 7000) * np.exp(-t * (10 if open_hat else 60)), gain, 0.3)


def tom(start, gain=0.3, pitch=70):
    duration = 0.3
    t = timeline(duration)
    place(start, sine(pitch + 60 * np.exp(-t * 20), duration) * np.exp(-t * 9), gain)


def crash(start, gain=0.14):
    duration = 1.6
    t = timeline(duration)
    place(start, highpass(noise(duration), 4000) * np.exp(-t * 2.2), gain)


def sweep(start, duration, low, high, gain=0.12):
    t = timeline(duration)
    cutoffs = np.linspace(low, high, len(t))
    body = noise(duration)
    out = np.zeros_like(body)
    steps = 12
    for i in range(steps):
        seg = slice(i * len(t) // steps, (i + 1) * len(t) // steps)
        out[seg] = lowpass(body, max(200, cutoffs[seg].mean()))[seg]
    place(start, out * np.sin(np.pi * t / duration), gain)


def slide(start, duration, f0, f1, gain=0.12, duty=0.5):
    freq = np.geomspace(f0, f1, len(timeline(duration)))
    place(start, square(freq, duration, duty) * envelope(duration, decay=duration, sustain=0.3, release=0.03), gain)


def blip(start, midi, duration=0.08, gain=0.1, duty=0.5, pan=0.0):
    place(start, square(hz(midi), duration, duty) * envelope(duration, decay=0.05, sustain=0.4, release=0.01), gain, pan)


def arpeggio(start, midis, step, gain=0.1, duty=0.25):
    for i, m in enumerate(midis):
        blip(start + i * step, m, step * 0.95, gain, duty)


def clang(start, gain=0.12, pitch=47):
    duration = 0.3
    tone = square(hz(pitch), duration) * 0.4 + square(hz(pitch) * 2.71, duration) * 0.3 + highpass(noise(duration), 3000)
    place(start, tone * np.exp(-timeline(duration) * 12), gain)


def thud(start, gain=0.3):
    place(start, lowpass(noise(0.12), 400) * np.exp(-timeline(0.12) * 30), gain)
    kick(start, gain * 0.6)


# ---------- sequencing helpers ----------


def audible(at, since, until):
    return (since is None or at >= since - 1e-6) and (until is None or at < until)


def groove(bar_start, bars, kick_beats=(0, 2), snare_beats=(1, 3), hat_step=EIGHTH, hat_gain=0.05, until=None, since=None):
    for b in range(bars):
        s = bar_start + b * BAR
        hits = [(beat * BEAT, kick) for beat in kick_beats] + [(beat * BEAT, snare) for beat in snare_beats]
        hits += [(i * hat_step, lambda at: hat(at, hat_gain)) for i in range(int(BAR / hat_step))]
        for offset, fn in hits:
            if audible(s + offset, since, until):
                fn(s + offset)


def bassline(bar_start, roots, pattern, until=None, since=None):
    step = BAR / len(pattern)
    for b, root in enumerate(roots):
        for i, offset in enumerate(pattern):
            at = bar_start + b * BAR + i * step
            if offset is not None and audible(at, since, until):
                bass(at, root + offset, step * 0.9)


def melody(bar_start, bars, slot, until=None, since=None, **kwargs):
    for b, notes in enumerate(bars):
        for i, m in enumerate(notes):
            if m is None or m == "-":
                continue
            length = slot
            j = i + 1
            while j < len(notes) and notes[j] == "-":
                length += slot
                j += 1
            at = bar_start + b * BAR + i * slot
            if audible(at, since, until):
                lead(at, m, length * 0.95, **kwargs)


def bars_between(start, end):
    """Bar-grid starts inside [start, end)."""
    first_bar = np.ceil(start / BAR - 1e-6) * BAR
    return [b for b in np.arange(first_bar, end - 1e-6, BAR)]


# ======================================================
# 1. boot
# ======================================================
boot = TIMING["boot"]
place(boot["crtOn"], highpass(noise(0.35), 2000) * np.exp(-timeline(0.35) * 8), 0.12)
kick(boot["crtOn"], 0.35)
pad(boot["desktop"], [51, 58, 62, 65, 67], 2.4, 0.05)  # Eb maj9 swell
for i, m in enumerate([75, 82, 79, 86]):
    bell(boot["desktop"] + i * 0.25, m, 1.2, 0.1, -0.3 + i * 0.2)
bell(boot["desktop"] + 1.2, 87, 1.8, 0.12)
for click in (boot["click1"], boot["click2"]):
    place(click, highpass(noise(0.02), 3000), 0.15)
for i, m in enumerate([72, 76, 79, 83, 84, 83, 79, 76] * 2):  # music-box loading loop
    pluck(boot["window"] + i * EIGHTH, m, 0.06)
blip(boot["coin"], 83, 0.07, 0.12)
blip(boot["coin"] + 0.07, 88, 0.4, 0.12)
sweep(boot["zoom"], 0.3, 400, 6000, 0.1)

# ======================================================
# 2. pinball (A minor funk, sim-driven SFX)
# ======================================================
PIN = TIMING["pinball"]
p0, p1 = TIMING["scenes"]["pinball"]
stop_at = first("agentStop", PIN["agentStop"])
kickback_at = first("kickback", stop_at + 1.5)
jackpot_at = PIN["jackpot"]
PROGRESSION = [45, 41, 36, 43]  # A F C G
MELODY = [
    [69, None, 72, 76, 74, 72, None, 69],
    [65, None, 69, 72, 77, 76, 72, None],
    [72, 76, 79, "-", 77, 76, 74, 72],
    [71, None, 74, 79, "-", 77, 76, 74],
]
PAD_CHORDS = [[57, 60, 64, 67], [53, 57, 60, 64], [48, 52, 55, 60], [55, 59, 62, 65]]
for n, bar in enumerate(bars_between(p0, stop_at)):
    k = n % 4
    bassline(bar, [PROGRESSION[k]], [0, 12, 7, 12, 0, 12, 10, 12], until=stop_at)
    groove(bar, 1, kick_beats=(0, 1.5, 2), until=stop_at)
    pad(bar, PAD_CHORDS[k], min(BAR, stop_at - bar), 0.03)
    if n >= 1:
        melody(bar, [MELODY[k]], EIGHTH, until=stop_at, gain=0.1)
# the agent stops: everything powers down
slide(stop_at, 0.9, 440, 55, 0.12, 0.5)
pad(stop_at, [45, 52], kickback_at - stop_at + 0.3, 0.035, 700)
# kickback: back in exactly on the save (mid-bar if need be), busier, melody
# up an octave; the melody yields to the jackpot fanfare.
kick_bar = np.floor(kickback_at / BAR) * BAR
for n, bar in enumerate(np.arange(kick_bar, p1 - 1e-6, BAR)):
    k = (n + 1) % 4
    bassline(bar, [PROGRESSION[k]], [0, 12, 7, 12, 0, 12, 10, 12], since=kickback_at, until=p1 - 0.5)
    groove(bar, 1, kick_beats=(0, 1, 2, 3), hat_step=SIXTEENTH, hat_gain=0.035, since=kickback_at, until=p1 - 0.5)
    melody(bar, [[m + 12 if isinstance(m, int) else m for m in MELODY[k]]], EIGHTH, since=kickback_at, until=jackpot_at, gain=0.08)

SFX_PAN = lambda x: float(np.clip((x - 101) / 110, -0.6, 0.6)) if x is not None else 0.0
last_rail = -1
for e in EVENTS:
    t, kind, x = e["t"], e["type"], e.get("x")
    pan = SFX_PAN(x)
    if kind == "bumper":
        base = [84, 88, 91][e.get("i", 0) % 3]
        blip(t, base, 0.06, 0.12, pan=pan)
        bell(t + 0.02, base + 12, 0.25, 0.08, pan)
    elif kind == "sling":
        place(t, highpass(noise(0.05), 1500) * np.exp(-timeline(0.05) * 60), 0.2, pan)
        blip(t, 60, 0.05, 0.08, pan=pan)
    elif kind == "flip":
        place(t, lowpass(noise(0.06), 900) * np.exp(-timeline(0.06) * 50), 0.28, -0.3 if e.get("side", -1) < 0 else 0.3)
    elif kind == "target":
        blip(t, 84 + [0, 2, 4, 5][e.get("i", 0) % 4], 0.09, 0.11, pan=pan)
    elif kind == "bank":
        arpeggio(t + 0.05, [84, 88, 91, 96], 0.06, 0.09)
    elif kind == "lane":
        blip(t, [79, 81, 83, 84][e.get("i", 0) % 4], 0.12, 0.1, 0.25, pan)
    elif kind == "task":
        bell(t + 0.03, 96, 0.4, 0.05)
    elif kind == "saucerIn":
        thud(t, 0.25)
        bell(t + 0.05, 91, 0.6, 0.08)
    elif kind == "saucerOut":
        kick(t, 0.3)
        slide(t, 0.12, 300, 700, 0.06)
    elif kind == "launch":
        blip(t - 0.4, 60, 0.35, 0.04, 0.125)
        slide(t, 0.25, 180, 900, 0.12)
    elif kind == "rail" and t - last_rail > 0.15:
        place(t, lowpass(noise(0.05), 700) * np.exp(-timeline(0.05) * 40), 0.12, pan)
        last_rail = t
    elif kind == "clack":
        place(t, highpass(noise(0.015), 4000), 0.1, pan)
    elif kind == "drain" and stop_at <= t < kickback_at:
        slide(t, 0.5, 600, 140, 0.13, 0.25)  # wah-wah
    elif kind == "save":
        for i in range(4):
            blip(t + i * 0.1, [88, 84][i % 2], 0.09, 0.1)
    elif kind == "kickback":
        kick(t, 0.6)
        slide(t, 0.3, 200, 1200, 0.1)
        crash(t, 0.1)
crash(jackpot_at, 0.18)
arpeggio(jackpot_at, [72, 76, 79, 84, 88], SIXTEENTH, 0.12)
for m in (84, 79, 76):
    brass(jackpot_at + 5 * SIXTEENTH, m, 0.8, 0.08)
sweep(PIN["wipe"], 0.5, 6000, 300, 0.1)

# ======================================================
# 3. palace (D phrygian dominant)
# ======================================================
Q = TIMING["palace"]
q0, q1 = TIMING["scenes"]["palace"]
duration = q1 - q0
drone = saw(hz(38), duration) + saw(hz(45), duration) * 0.7
drone_env = np.ones_like(drone)
tt = timeline(duration) + q0
drone_env[(tt > Q["grab"]) & (tt < Q["climb"])] = 0.55
place(q0, lowpass(drone, 500) * drone_env * envelope(duration, attack=0.5, decay=1, sustain=1, release=0.6), 0.07)
HARP = [62, 66, 69, 74, 69, 66, 63, 66]
music_gap = (Q["grab"], Q["climb"])
fight = (Q["guardAlert"], Q["guardDown"])


def in_gap(at):
    return music_gap[0] <= at < music_gap[1]


for n, bar in enumerate(bars_between(q0, q1)):
    for i in range(16):
        at = bar + i * SIXTEENTH
        if not in_gap(at) and at < Q["clear"]:
            pluck(at, HARP[i % 8] + (12 if i >= 8 else 0), 0.05)
    for beat in (0, 0.75, 1.5, 2, 3, 3.5):
        at = bar + beat * BEAT
        if not in_gap(at) and at < Q["clear"]:
            tom(at, 0.34 if fight[0] <= at < fight[1] else 0.26)
    for beat in (1, 3):
        at = bar + beat * BEAT
        if not in_gap(at) and at < Q["clear"]:
            hat(at, 0.05, True)
PALACE_MELODY = {
    q0: [None, None, 69, 70, 69, 67, 66, "-"],
    q0 + BAR: [74, "-", 72, 70, 69, "-", 66, 63],
    q0 + 2 * BAR: [69, 70, 72, 70, 69, None, None, None],
    q0 + 4 * BAR: [74, 75, 74, 72, 70, 69, 70, 72],
}
for bar, notes in PALACE_MELODY.items():
    melody(bar, [notes], EIGHTH, until=None if bar > Q["climb"] else Q["grab"], gain=0.1, duty=0.5, vibrato=0.012, pan=-0.1)
# fight: stabbing low brass on the beat
for at in np.arange(fight[0], fight[1], BEAT):
    brass(at, 50, 0.15, 0.05)
# scripted SFX
for i in range(6):
    place(Q["crumbleShake"] + i * 0.06, highpass(noise(0.03), 2500), 0.08)
slide(Q["crumbleFall"], 0.5, 500, 120, 0.05, 0.25)
thud(Q["crumbleCrash"], 0.4)
sweep(Q["crumbleCrash"], 0.4, 3000, 400, 0.06)
place(Q["spikesUp"], highpass(noise(0.12), 5000) * np.exp(-timeline(0.12) * 25), 0.16)
blip(Q["spikesUp"], 96, 0.1, 0.06)
for step in Q["carefulSteps"]:
    place(step + 0.2, lowpass(noise(0.03), 900), 0.1)
slide(Q["jump"], 0.2, 350, 800, 0.08)
thud(Q["grab"], 0.3)
for i in range(2):
    blip(Q["stopped"] + i * 0.18, 45, 0.14, 0.14)
place(Q["slip"], highpass(noise(0.2), 2000) * np.exp(-timeline(0.2) * 12), 0.1)
slide(Q["slip"], 0.15, 300, 200, 0.06)
for i, m in enumerate([74, 78, 81, 86]):
    bell(Q["orb"] + i * 0.1, m, 1.0, 0.1)
arpeggio(Q["cont"], [62, 66, 69, 74, 78, 81, 86], 0.06, 0.08)
slide(Q["climb"], 0.8, 150, 500, 0.04, 0.25)
place(Q["guardAlert"], highpass(noise(0.3), 4000) * np.exp(-timeline(0.3) * 8), 0.12)  # sword draw
for at in Q["clashes"]:
    clang(at, 0.14, 59)
clang(Q["hit"], 0.16, 52)
thud(Q["hit"], 0.35)
crash(Q["hit"], 0.08)
thud(Q["guardDown"], 0.35)
place(Q["door"], lowpass(noise(0.7), 180) * np.sin(np.pi * timeline(0.7) / 0.7), 0.5)
arpeggio(Q["clear"], [74, 78, 81, 86], SIXTEENTH, 0.12)
for m in (86, 81):
    brass(Q["clear"] + 0.25, m, 0.8, 0.08)
sweep(Q["wipe"], 0.5, 6000, 300, 0.1)

# ======================================================
# 4. race (E minor)
# ======================================================
RACE = TIMING["race"]
r0, r1 = TIMING["scenes"]["race"]
engine_len = RACE["finish"] - r0
et = timeline(engine_len) + r0
rpm = np.where(et < RACE["go"], 55 + 4 * np.sin(2 * np.pi * 7 * et),
               np.where(et < RACE["nitro"], 60 + (et - RACE["go"]) * 14, 130 + (et - RACE["nitro"]) * 8))
engine = saw(rpm, engine_len) + 0.5 * square(rpm * 0.5, engine_len, 0.3)
place(r0, lowpass(engine, 900) * envelope(engine_len, attack=0.3, decay=1, sustain=1, release=0.3), 0.06)
for t in RACE["beeps"]:
    blip(t, 81, 0.14, 0.12)
blip(RACE["go"], 93, 0.45, 0.12)
for i in range(8):
    bass(RACE["go"] + i * SIXTEENTH, 40 + 12 * (i % 2), SIXTEENTH * 0.9)
bassline(RACE["go"] + BEAT, [36, 38, 40], [0, 12] * 8)
groove(RACE["go"] + BEAT, 3, kick_beats=(0, 1, 2, 3), hat_step=SIXTEENTH, hat_gain=0.035)
melody(RACE["go"] + BEAT, [
    [76, None, 79, None, 83, 81, 79, 76],
    [78, None, 81, None, 86, "-", 83, 81],
    [88, None, 86, 83, None, 81, 83, "-"],
], EIGHTH, gain=0.09, duty=0.5)
melody(RACE["go"] + BEAT, [
    [64, None, 67, None, 71, 69, 67, 64],
    [66, None, 69, None, 74, "-", 71, 69],
    [76, None, 74, 71, None, 69, 71, "-"],
], EIGHTH, gain=0.06, duty=0.125, pan=-0.3)
for t in RACE["passes"] + RACE["fastPasses"]:
    sweep(t - 0.2, 0.45, 800, 3500, 0.12)
sweep(RACE["nitro"], 0.9, 500, 9000, 0.16)
slide(RACE["nitro"], 0.6, 200, 1600, 0.07, 0.25)
crash(RACE["nitro"], 0.1)
for i in range(4):
    bass(RACE["finish"] - 0.5 + i * SIXTEENTH, 36 + 12 * (i % 2), SIXTEENTH * 0.9)
bass(RACE["finish"], 40, 1.2)
crash(RACE["finish"], 0.16)
arpeggio(RACE["finish"], [76, 80, 83, 88], SIXTEENTH, 0.12)
for m in (88, 83, 80):
    brass(RACE["finish"] + 0.5, m, 0.8, 0.07)
sweep(RACE["wipe"], 0.5, 6000, 300, 0.1)

# ======================================================
# 5. finale (C major)
# ======================================================
FIN = TIMING["finale"]
f0, f1 = TIMING["scenes"]["finale"]
logo_at = FIN["logo"]
# leaderboard: easy groove under the rows, a tick per row, a build into the logo
for n, bar in enumerate(np.arange(f0, logo_at - 1e-6, BAR)):
    root = [36, 41, 43][min(n, 2)] if bar < FIN["newHigh"] - 0.5 else 43
    bassline(bar, [root], [0, None, 12, None, 7, None, 12, None], until=logo_at - BEAT)
    groove(bar, 1, hat_gain=0.035, until=logo_at - BEAT)
    pad(bar, [[48, 52, 55, 60], [53, 57, 60, 65], [55, 59, 62, 67]][n % 3], min(BAR, logo_at - bar), 0.03)
for i, t in enumerate(FIN["rows"]):
    blip(t, 79 + i * 2, 0.08, 0.09)
arpeggio(FIN["newHigh"], [72, 76, 79, 84, 79, 84, 88, 91], 0.07, 0.09)
for i in range(8):  # snare roll into the logo
    snare(logo_at - BEAT * 2 + i * SIXTEENTH, 0.08 + i * 0.02)
# logo hit
crash(logo_at, 0.16)
kick(logo_at, 0.6)
for m in (60, 64, 67, 72):
    brass(logo_at, m, 1.4, 0.07)
arpeggio(FIN["wordmark"], [84, 88, 91, 96], SIXTEENTH, 0.08)
# tagline: two stabs, then the theme resolves and rings out
for i, t in enumerate(FIN["tagline"]):
    for m in ((65, 69, 72), (67, 71, 74))[i]:
        brass(t, m, 0.6, 0.05)
    kick(t, 0.4)
theme_start = FIN["tagline"][1] + BEAT
bassline(theme_start, [41], [0, None, 12, None, 7, None, 12, None])
groove(theme_start, 1, hat_gain=0.035)
melody(theme_start, [[72, "-", 77, "-", 81, "-", 79, 77]], EIGHTH, gain=0.09, duty=0.5)
end_start = theme_start + BAR
assert f1 - end_start >= 1.5, "final chord needs room to ring out"
for m in (48, 60, 64, 67, 72, 76):
    brass(end_start, m, f1 - end_start, 0.05)
bell(end_start, 96, 1.2, 0.08)

# ---------- master ----------
stereo = np.tanh(np.stack([left, right], axis=1) * 1.2)
stereo *= MASTER_PEAK / np.max(np.abs(stereo))
fade = int(0.6 * SAMPLE_RATE)
stereo[-fade:] *= np.linspace(1, 0, fade)[:, None]
with wave.open(str(HERE / "build/soundtrack.wav"), "wb") as out:
    out.setnchannels(2)
    out.setsampwidth(2)
    out.setframerate(SAMPLE_RATE)
    out.writeframes((stereo * 32767).astype(np.int16).tobytes())
print(f"wrote build/soundtrack.wav ({TOTAL_SECONDS}s, {len(EVENTS)} pinball events)")
