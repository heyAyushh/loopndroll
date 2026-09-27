"""Synthesize the original chiptune score + SFX for the Looper arcade launch video.

Everything is generated from oscillators (no samples), so the soundtrack is
original and license-free. Moment timings come from assets/timing.js, the same
file the composition reads, so every hit lands on its visual frame.

Usage: python3 synth.py  ->  assets/soundtrack.wav
"""

import json
import pathlib
import wave

import numpy as np
from scipy.signal import butter, lfilter

HERE = pathlib.Path(__file__).parent
TIMING = json.loads(
    (HERE / "assets/timing.js").read_text().split("window.T = ", 1)[1].rstrip().rstrip(";")
)
SAMPLE_RATE = 44100
BEAT = 60 / TIMING["bpm"]
BAR = 4 * BEAT
EIGHTH = BEAT / 2
SIXTEENTH = BEAT / 4
TOTAL_SECONDS = TIMING["total"]
MASTER_PEAK = 0.89  # about -1 dBFS
rng = np.random.default_rng(1998)  # seeded so the render is reproducible

left = np.zeros(int(TOTAL_SECONDS * SAMPLE_RATE))
right = np.zeros_like(left)


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
    freq = np.broadcast_to(freq, timeline(duration).shape)
    return np.cumsum(freq) / SAMPLE_RATE


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
    b, a = butter(2, cutoff / (SAMPLE_RATE / 2))
    return lfilter(b, a, signal)


def highpass(signal, cutoff):
    b, a = butter(2, cutoff / (SAMPLE_RATE / 2), btype="high")
    return lfilter(b, a, signal)


def place(start, signal, gain=1.0, pan=0.0):
    begin = int(start * SAMPLE_RATE)
    if begin >= len(left):
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


def tom(start, gain=0.3):
    duration = 0.3
    t = timeline(duration)
    place(start, sine(70 + 60 * np.exp(-t * 20), duration) * np.exp(-t * 9), gain)


def crash(start, gain=0.14):
    duration = 1.6
    t = timeline(duration)
    place(start, highpass(noise(duration), 4000) * np.exp(-t * 2.2), gain)


def sweep(start, duration, low, high, gain=0.12):
    t = timeline(duration)
    cutoff_path = np.linspace(low, high, len(t))
    body = noise(duration)
    # Stepwise band sweep: cheap and plenty convincing for a whoosh.
    steps = 12
    out = np.zeros_like(body)
    for i in range(steps):
        segment = slice(i * len(t) // steps, (i + 1) * len(t) // steps)
        out[segment] = lowpass(body, max(200, cutoff_path[segment].mean()))[segment]
    place(start, out * np.sin(np.pi * t / duration), gain)


def slide(start, duration, f0, f1, gain=0.12, duty=0.5):
    freq = np.geomspace(f0, f1, len(timeline(duration)))
    place(start, square(freq, duration, duty) * envelope(duration, decay=duration, sustain=0.3, release=0.03), gain)


def blip(start, midi, duration=0.08, gain=0.1, duty=0.5):
    place(start, square(hz(midi), duration, duty) * envelope(duration, decay=0.05, sustain=0.4, release=0.01), gain)


def bumper(start, index=0):
    base = [84, 88, 91, 86][index % 4]
    blip(start, base, 0.06, 0.12)
    bell(start + 0.02, base + 12, 0.25, 0.09)


def arpeggio(start, midis, step, gain=0.1, duty=0.25):
    for i, m in enumerate(midis):
        blip(start + i * step, m, step * 0.95, gain, duty)


# ---------- groove helper ----------


def groove(bar_start, bars, kick_beats=(0, 2), snare_beats=(1, 3), hat_step=EIGHTH, hat_gain=0.05):
    for b in range(bars):
        s = bar_start + b * BAR
        for beat in kick_beats:
            kick(s + beat * BEAT)
        for beat in snare_beats:
            snare(s + beat * BEAT)
        n = int(BAR / hat_step)
        for i in range(n):
            hat(s + i * hat_step, hat_gain)


def bassline(bar_start, roots, pattern):
    for b, root in enumerate(roots):
        for i, offset in enumerate(pattern):
            if offset is not None:
                bass(bar_start + b * BAR + i * (BAR / len(pattern)), root + offset, BAR / len(pattern) * 0.9)


def melody(bar_start, bars, slot, **kwargs):
    for b, notes in enumerate(bars):
        for i, m in enumerate(notes):
            if m is None:
                continue
            length = slot
            j = i + 1
            while j < len(notes) and notes[j] == "-":
                length += slot
                j += 1
            if m != "-":
                lead(bar_start + b * BAR + i * slot, m, length * 0.95, **kwargs)


# ---------- scene 1: boot ----------
boot = TIMING["boot"]
static_len = 0.35
place(boot["crtOn"], highpass(noise(static_len), 2000) * np.exp(-timeline(static_len) * 8), 0.12)
kick(boot["crtOn"], 0.35)
pad(boot["desktop"], [51, 58, 62, 65, 67], 2.4, 0.05)  # Eb maj9 swell
for i, m in enumerate([75, 82, 79, 86]):
    bell(boot["desktop"] + i * 0.25, m, 1.2, 0.1, -0.3 + i * 0.2)
bell(boot["desktop"] + 1.2, 87, 1.8, 0.12)
for click in (boot["click1"], boot["click2"]):
    place(click, highpass(noise(0.02), 3000), 0.15)
for i, m in enumerate([72, 76, 79, 83, 84, 83, 79, 76] * 2):  # music-box loading loop
    pluck(boot["window"] + i * SIXTEENTH * 2, m, 0.06)
blip(boot["coin"], 83, 0.07, 0.12)
blip(boot["coin"] + 0.07, 88, 0.4, 0.12)
sweep(boot["zoom"], 0.3, 400, 6000, 0.1)

# ---------- scene 2: pinball ----------
pin = TIMING["pinball"]
p0 = TIMING["scenes"]["pinball"][0]
roots = [45, 41, 36, 43]  # A F C G
bassline(p0, roots, [0, 12, 7, 12, 0, 12, 10, 12])
groove(p0, 4, kick_beats=(0, 1.5, 2), snare_beats=(1, 3))
pad(p0, [57, 60, 64, 67], BAR, 0.03)
pad(p0 + BAR, [53, 57, 60, 64], BAR, 0.03)
pad(p0 + 2 * BAR, [48, 52, 55, 60], BAR, 0.03)
pad(p0 + 3 * BAR, [55, 59, 62, 65], BAR, 0.03)
melody(p0, [
    [69, None, 72, 76, 74, 72, None, 69],
    [65, None, 69, 72, 77, 76, 72, None],
    [72, 76, 79, "-", 77, 76, 74, 72],
    [71, None, 74, 79, "-", 77, 76, 74],
], EIGHTH, gain=0.11)
# bar 14: the break — ball drains, the agent "stops"
break_bar = p0 + 4 * BAR
kick(break_bar)
for i in range(2):
    hat(break_bar + i * EIGHTH)
bass(break_bar, 45, BEAT)
# bars 16-20: multiball, busier
bassline(p0 + 5 * BAR, [41, 43], [0, 12, 7, 12, 0, 12, 10, 12])
groove(p0 + 5 * BAR, 2, kick_beats=(0, 1, 2, 3), snare_beats=(1, 3), hat_step=SIXTEENTH, hat_gain=0.035)
melody(p0 + 5 * BAR, [
    [77, None, 81, 84, 89, 88, 84, None],
    [83, None, 86, 91, None, None, None, None],
], EIGHTH, gain=0.1)

blip(pin["launch"] - 0.4, 60, 0.35, 0.05, 0.125)  # plunger pull creak
slide(pin["launch"], 0.25, 180, 900, 0.12)
for t, index in pin["bumperHits"] + pin["multiHits"]:
    bumper(t, index)
for t in pin["flipLeft"] + pin["flipRight"]:
    place(t, lowpass(noise(0.06), 900) * np.exp(-timeline(0.06) * 50), 0.3)
for i, t in enumerate(pin["targets"]):
    blip(t, 84 + [0, 2, 4, 5, 7][i], 0.09, 0.11)
sweep(pin["loopRamp"][0], pin["loopRamp"][1] - pin["loopRamp"][0], 300, 5000, 0.12)
arpeggio(pin["loopRamp"][0], [72, 76, 79, 84, 88, 91, 96, 100], 0.125, 0.06)
bumper(pin["deployHit"], 3)
bell(pin["deployHit"], 96, 0.8, 0.1)
slide(pin["drain"], 0.5, 600, 140, 0.13, 0.25)  # wah-wah
for i in range(4):
    blip(pin["save"] + i * 0.1, [88, 84][i % 2], 0.09, 0.1)
kick(pin["kickback"], 0.6)
slide(pin["kickback"], 0.3, 200, 1200, 0.1)
crash(pin["kickback"], 0.1)
crash(pin["jackpot"], 0.16)
arpeggio(pin["jackpot"], [72, 76, 79, 84, 88], SIXTEENTH, 0.12)
brass(pin["jackpot"] + 5 * SIXTEENTH, 84, 0.7, 0.1)
brass(pin["jackpot"] + 5 * SIXTEENTH, 79, 0.7, 0.08)
sweep(pin["wipe"], 0.5, 6000, 300, 0.1)

# ---------- scene 3: palace (D phrygian dominant) ----------
pal = TIMING["palace"]
q0 = TIMING["scenes"]["palace"][0]
drone = saw(hz(38), 10) + saw(hz(45), 10) * 0.7
drone_env = np.ones_like(drone)
dip = timeline(10)
drone_env[(dip > pal["stop"] - q0) & (dip < pal["leap"] - q0)] = 0.5
place(q0, lowpass(drone, 500) * drone_env * envelope(10, attack=0.5, decay=1, sustain=1, release=0.6), 0.07)
harp = [62, 66, 69, 74, 69, 66, 63, 66]


def palace_bar(start, with_melody=None, beats=4):
    for i in range(beats * 4):
        pluck(start + i * SIXTEENTH, harp[i % len(harp)] + (12 if i >= 8 else 0), 0.05)
    for beat in (0, 0.75, 1.5, 2, 3, 3.5):
        if beat < beats:
            tom(start + beat * BEAT, 0.28)
    for beat in (1, 3):
        if beat < beats:
            hat(start + beat * BEAT, 0.05, True)
    if with_melody:
        melody(start, [with_melody], EIGHTH, gain=0.1, duty=0.5, vibrato=0.012, pan=-0.1)


palace_bar(q0, [None, None, 69, 70, 69, 67, 66, "-"])
palace_bar(q0 + BAR, [74, "-", 72, 70], beats=2)  # music drops out when the agent stops
palace_bar(pal["leap"], None, beats=2)  # and comes back on the leap
palace_bar(q0 + 3 * BAR, [74, 75, 74, 72, 70, 69, 70, 72])
palace_bar(q0 + 4 * BAR, None)
slide(pal["jump1"], 0.15, 400, 800, 0.1)
tom(pal["land1"], 0.2)
for i in range(2):
    blip(pal["stopped"] + i * 0.18, 45, 0.14, 0.14)
for i, m in enumerate([74, 78, 81, 86]):
    bell(pal["orb"] + i * 0.1, m, 1.0, 0.1)
arpeggio(pal["cont"], [62, 66, 69, 74, 78, 81, 86], 0.06, 0.08)
slide(pal["leap"], 0.4, 300, 1000, 0.1)
tom(pal["land2"], 0.3)
for t in pal["chomps"]:
    clang = square(hz(47), 0.25) * 0.5 + highpass(noise(0.25), 2500)
    place(t, clang * np.exp(-timeline(0.25) * 14), 0.12)
place(pal["gate"], lowpass(noise(0.8), 180) * np.sin(np.pi * timeline(0.8) / 0.8), 0.5)
arpeggio(pal["clear"], [74, 78, 81, 86], EIGHTH / 2, 0.12)
brass(pal["clear"] + 0.25, 86, 0.8, 0.09)
brass(pal["clear"] + 0.25, 81, 0.8, 0.07)
sweep(pal["wipe"], 0.5, 6000, 300, 0.1)

# ---------- scene 4: race (E minor) ----------
race = TIMING["race"]
r0 = TIMING["scenes"]["race"][0]
engine_len = race["finish"] - r0
et = timeline(engine_len)
go_offset = race["go"] - r0
nitro_offset = race["nitro"] - r0
rpm = np.where(et < go_offset, 55 + 4 * np.sin(2 * np.pi * 7 * et),
               np.where(et < nitro_offset, 60 + (et - go_offset) * 14, 130 + (et - nitro_offset) * 8))
engine = saw(rpm, engine_len) + 0.5 * square(rpm * 0.5, engine_len, 0.3)
place(r0, lowpass(engine, 900) * envelope(engine_len, attack=0.3, decay=1, sustain=1, release=0.3), 0.06)
for t in race["beeps"]:
    blip(t, 81, 0.14, 0.12)
blip(race["go"], 93, 0.45, 0.12)
for i in range(8):  # half bar of pumping E after GO
    bass(race["go"] + i * SIXTEENTH, 40 + 12 * (i % 2), SIXTEENTH * 0.9)
bassline(r0 + BAR, [36, 38, 40], [0, 12] * 8)
groove(r0 + BAR, 3, kick_beats=(0, 1, 2, 3), snare_beats=(1, 3), hat_step=SIXTEENTH, hat_gain=0.035)
melody(r0 + BAR, [
    [76, None, 79, None, 83, 81, 79, 76],
    [78, None, 81, None, 86, "-", 83, 81],
    [88, None, 86, 83, None, 81, 83, "-"],
], EIGHTH, gain=0.09, duty=0.5)
melody(r0 + BAR, [
    [64, None, 67, None, 71, 69, 67, 64],
    [66, None, 69, None, 74, "-", 71, 69],
    [76, None, 74, 71, None, 69, 71, "-"],
], EIGHTH, gain=0.06, duty=0.125, pan=-0.3)
for t in race["passes"] + race["fastPasses"]:
    sweep(t - 0.2, 0.45, 800, 3500, 0.12)
sweep(race["nitro"], 0.9, 500, 9000, 0.16)
slide(race["nitro"], 0.6, 200, 1600, 0.07, 0.25)
crash(race["nitro"], 0.1)
for i in range(4):
    bass(r0 + 4 * BAR + i * SIXTEENTH, 36 + 12 * (i % 2), SIXTEENTH * 0.9)
bass(race["finish"], 40, 1.2)
crash(race["finish"], 0.16)
arpeggio(race["finish"], [76, 80, 83, 88], SIXTEENTH, 0.12)
brass(race["finish"] + 0.5, 88, 0.8, 0.09)
brass(race["finish"] + 0.5, 83, 0.8, 0.07)
brass(race["finish"] + 0.5, 80, 0.8, 0.06)
sweep(race["wipe"], 0.5, 6000, 300, 0.1)

# ---------- scene 5: finale (C major) ----------
fin = TIMING["finale"]
f0 = TIMING["scenes"]["finale"][0]
bassline(f0, [36], [0, None, 12, None, 7, None, 12, None])
groove(f0, 1, kick_beats=(0, 2), snare_beats=(1, 3), hat_gain=0.04)
for i, t in enumerate(fin["rows"]):
    blip(t, 79 + i * 2, 0.06, 0.08)
arpeggio(fin["newHigh"], [72, 76, 79, 84, 79, 84, 88, 91], 0.07, 0.09)
bass(f0 + BAR, 41, BAR * 0.45)
kick(f0 + BAR)
crash(fin["logo"], 0.16)
kick(fin["logo"], 0.6)
for m in (60, 64, 67, 72):
    brass(fin["logo"], m, 1.4, 0.07)
arpeggio(fin["wordmark"], [84, 88, 91, 96], SIXTEENTH, 0.08)
bassline(f0 + 2 * BAR, [41, 43], [0, None, 12, None, 7, None, 12, None])
groove(f0 + 2 * BAR, 1, kick_beats=(0, 2), snare_beats=(1, 3), hat_gain=0.04)
groove(f0 + 3 * BAR, 1, kick_beats=(0,), snare_beats=(), hat_gain=0.03)
melody(f0 + 2 * BAR, [
    [72, "-", 77, "-", 81, "-", 79, 77],
    [79, "-", "-", 76, 74, "-", 71, "-"],
], EIGHTH, gain=0.09, duty=0.5)
pad(fin["tagline"], [53, 57, 60, 65], BAR, 0.035)
blip(fin["press"], 83, 0.07, 0.12)
blip(fin["press"] + 0.07, 88, 0.4, 0.12)
end_chord = TOTAL_SECONDS - (f0 + 3.5 * BAR)
for m in (48, 60, 64, 67, 72, 76):
    brass(f0 + 3.5 * BAR, m, end_chord, 0.05)
bell(f0 + 3.5 * BAR, 96, 1.0, 0.08)

# ---------- master ----------
stereo = np.stack([left, right], axis=1)
stereo = np.tanh(stereo * 1.2)
stereo *= MASTER_PEAK / np.max(np.abs(stereo))
fade = int(0.6 * SAMPLE_RATE)
stereo[-fade:] *= np.linspace(1, 0, fade)[:, None]
pcm = (stereo * 32767).astype(np.int16)
with wave.open(str(HERE / "assets/soundtrack.wav"), "wb") as out:
    out.setnchannels(2)
    out.setsampwidth(2)
    out.setframerate(SAMPLE_RATE)
    out.writeframes(pcm.tobytes())
print(f"wrote assets/soundtrack.wav ({TOTAL_SECONDS}s)")
