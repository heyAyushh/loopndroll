"""Score for "Signal" (60s, 32 bars at 128 BPM), from the "looper" playlist analysis.

assets/playlist-analysis.json: 128 BPM, D minor / F major, sub-heavy, dark, hypnotic (bar similarity 0.90).
Restraint first: the film opens on one tick and silence. The groove is the agents' work; when they stop,
the music stops. Each "y" restarts it for less time. One bar of nothing. Then the loop.

Run: python3 score.py -> assets/score.wav, assets/envelope.json
"""

import json

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, fftconvolve, sosfilt

SAMPLE_RATE = 44100
VIDEO_FPS = 30
BEATS_PER_MINUTE = 128
BEAT_SECONDS = 60 / BEATS_PER_MINUTE
BAR_SECONDS = BEAT_SECONDS * 4
TOTAL_BARS = 32
TOTAL_SECONDS = TOTAL_BARS * BAR_SECONDS  # 60.0
SEED = 60
PEAK_CEILING = 0.72

rng = np.random.default_rng(SEED)
sample_count = int(TOTAL_SECONDS * SAMPLE_RATE)

def seconds(bar, beat=0.0):
    return bar * BAR_SECONDS + beat * BEAT_SECONDS


def index(at_seconds):
    return int(round(at_seconds * SAMPLE_RATE))


def note_hz(midi):
    return 440.0 * 2 ** ((midi - 69) / 12)


def envelope(length, attack, decay_rate):
    t = np.arange(length) / SAMPLE_RATE
    return np.clip(t / max(attack, 1e-4), 0, 1) * np.exp(-t * decay_rate)


def filtered(signal, kind, cutoff, order=2):
    return sosfilt(butter(order, cutoff, kind, fs=SAMPLE_RATE, output="sos"), signal)


def sweep_lowpass(signal, cutoff_at, block=1024):
    output = np.zeros_like(signal)
    state = np.zeros((1, 2))
    for start in range(0, len(signal), block):
        cutoff = float(np.clip(cutoff_at(start / SAMPLE_RATE), 40, 18000))
        sos = butter(1, cutoff, "low", fs=SAMPLE_RATE, output="sos")
        output[start:start + block], state = sosfilt(sos, signal[start:start + block], zi=state)
    return output


def place(track, sound, at_seconds, gain=1.0):
    start = index(at_seconds)
    end = min(len(track), start + len(sound))
    if 0 <= start < end:
        track[start:end] += sound[: end - start] * gain


def saw(frequency, length, detune_cents=(0,)):
    t = np.arange(length) / SAMPLE_RATE
    wave = sum(2 * ((t * frequency * 2 ** (c / 1200) + rng.random()) % 1) - 1 for c in detune_cents)
    return wave / len(detune_cents)


# ---------- instruments ----------

def kick():
    length = int(0.5 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    pitch = 42 + 105 * np.exp(-t * 34)
    body = np.sin(2 * np.pi * np.cumsum(pitch) / SAMPLE_RATE) * np.exp(-t * 6.5)
    click = filtered(rng.standard_normal(length), "high", 2200) * np.exp(-t * 420) * 0.18
    return np.tanh((body + click) * 1.8)


def hat(open_hat=False):
    length = int((0.26 if open_hat else 0.05) * SAMPLE_RATE)
    return filtered(rng.standard_normal(length), "high", 7000) * envelope(length, 0.001, 14 if open_hat else 80)


def clap():
    length = int(0.32 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    hits = sum(np.exp(-np.clip(t - o, 0, None) * 90) * (t >= o) for o in (0, 0.012, 0.024))
    return filtered(rng.standard_normal(length), "band", [900, 2800]) * (hits * 0.6 + np.exp(-t * 18) * 0.4)


def ride():
    length = int(0.5 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    metal = sum(np.sin(2 * np.pi * f * t) for f in (3120, 4470, 5210, 6890)) / 4
    return (metal * 0.5 + filtered(rng.standard_normal(length), "high", 6000) * 0.5) * envelope(length, 0.001, 9)


def bass(midi, length_seconds=BEAT_SECONDS / 4 * 0.85):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    tone = saw(note_hz(midi), length, (-5, 5)) * 0.6 + np.sin(2 * np.pi * note_hz(midi) * t)
    return filtered(tone, "low", 380) * envelope(length, 0.003, 16)


def stab(midis, length_seconds=0.24, brightness=1500):
    length = int(length_seconds * SAMPLE_RATE)
    tone = sum(saw(note_hz(m), length, (-8, 0, 8)) for m in midis) / len(midis)
    return filtered(tone, "low", brightness) * envelope(length, 0.002, 15)


def pad(midis, length_seconds, brightness=800):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    tone = sum(saw(note_hz(m), length, (-13, -4, 4, 13)) for m in midis) / len(midis)
    swell = np.clip(t / 3.0, 0, 1) * np.clip((length_seconds - t) / 2.5, 0, 1)
    return filtered(tone, "low", brightness) * swell


def riser(length_seconds):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    rise = sweep_lowpass(rng.standard_normal(length), lambda s: 250 + 7000 * (s / length_seconds) ** 2)
    return filtered(rise, "high", 200) * (t / length_seconds) ** 2


def bell(midi, decay=3.0, length_seconds=1.8):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    return (np.sin(2 * np.pi * note_hz(midi) * t) + 0.25 * np.sin(2 * np.pi * note_hz(midi + 12) * t)) * envelope(length, 0.002, decay)


def gate_clack():
    """The agent hitting the square gate: a dry metal latch."""
    length = int(0.25 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    ring = sum(np.sin(2 * np.pi * f * t) for f in (310, 740, 1230)) / 3
    return (ring * envelope(length, 0.0005, 28) + filtered(rng.standard_normal(length), "band", [1500, 6000]) * envelope(length, 0.0005, 120) * 0.6)


def key_click(soft=False):
    length = int(0.035 * SAMPLE_RATE)
    return filtered(rng.standard_normal(length), "band", [1200, 5000] if soft else [2000, 7000]) * envelope(length, 0.0004, 150)


def mouse_click():
    length = int(0.02 * SAMPLE_RATE)
    return filtered(rng.standard_normal(length), "band", [2500, 8000]) * envelope(length, 0.0003, 260)


def buzz(length_seconds=0.32):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    return np.sin(2 * np.pi * 172 * t) * (0.6 + 0.4 * np.sin(2 * np.pi * 31 * t)) * np.clip(np.minimum(t, length_seconds - t) / 0.02, 0, 1)


def chirp(start_hz, end_hz, length_seconds=0.09):
    length = int(length_seconds * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    frequency = np.linspace(start_hz, end_hz, length)
    return np.sin(2 * np.pi * np.cumsum(frequency) / SAMPLE_RATE) * np.sin(np.pi * t / length_seconds) ** 2


def sub_drop():
    length = int(2.0 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    return np.sin(2 * np.pi * np.cumsum(28 + 72 * np.exp(-t * 2.8)) / SAMPLE_RATE) * np.exp(-t * 1.4)


def cymbal(reverse=False, length_seconds=2.4):
    length = int(length_seconds * SAMPLE_RATE)
    sound = filtered(rng.standard_normal(length), "high", 3500) * envelope(length, 0.001, 2.4)
    return sound[::-1] if reverse else sound


def dub_delay(signal, delay_seconds=BEAT_SECONDS * 0.75, feedback=0.55, repeats=8):
    delay = int(delay_seconds * SAMPLE_RATE)
    left, right = signal.copy(), signal * 0.35
    echo = signal.copy()
    for repeat in range(1, repeats + 1):
        echo = filtered(echo, "low", 2200) * feedback
        shifted = np.zeros_like(signal)
        shifted[delay * repeat:] = echo[: len(signal) - delay * repeat]
        (left if repeat % 2 == 0 else right)[:] += shifted
    return left, right


def room(signal, wet):
    length = int(2.6 * SAMPLE_RATE)
    impulse = filtered(rng.standard_normal(length), "low", 4500) * envelope(length, 0.01, 2.6)
    impulse /= np.sqrt(np.sum(impulse ** 2))
    return signal * (1 - wet) + fftconvolve(signal, impulse)[: len(signal)] * wet


# ---------- the groove bus (generated everywhere, then gated by the story) ----------



# ---------- arrangement: film/story.js uses the same bars ----------

WORK = (1.0, 7.75)                                   # the river of work builds
RESTARTS = [(10.0, 11.0), (11.75, 12.5), (13.0, 13.5)]  # each "y" buys less
DROP = (20.0, 28.0)                                  # the loop
BUILD = (17.5, 20.0)
END = 28.0
STOPS = {7.75: 1.0, 11.0: 0.5, 12.5: 0.5, 13.5: 0.5}  # tape-stops, in beats

D_MINOR_STAB = [50, 53, 57, 60]
F_MAJOR_STAB = [53, 57, 60, 64]
D_MINOR_PAD = [38, 45, 50, 53, 57, 64]
F_MAJOR_PAD = [41, 48, 53, 57, 60, 67]
BASS_LINE = [None, 26, 26, 38, None, 26, 29, 26]

KICK, CLOSED, OPEN, CLAP = kick(), hat(), hat(True), clap()
groove = np.zeros(sample_count)
stabs = np.zeros(sample_count)
for bar in range(TOTAL_BARS):
    drop = DROP[0] <= bar < DROP[1]
    for beat in range(4):
        at = seconds(bar, beat)
        place(groove, KICK, at, 0.95)
        if bar >= 3 or drop:
            place(groove, OPEN, at + BEAT_SECONDS / 2, 0.15 if drop else 0.08)
        if drop or bar >= 19:
            for sixteenth in range(4):
                place(groove, CLOSED, at + sixteenth * BEAT_SECONDS / 4, 0.06 + 0.04 * (sixteenth == 2))
        if drop and beat in (1, 3):
            place(groove, CLAP, at, 0.28)
        if bar >= 4:
            for step in range(2):
                midi = BASS_LINE[(beat * 2 + step) % 8]
                if midi is not None:
                    place(groove, bass(midi), at + (step * 2 + 1) * BEAT_SECONDS / 4, 0.6)
    if bar >= 5:
        for offset in (0.5, 1.75, 3.0):
            place(stabs, stab(D_MINOR_STAB, brightness=2000 if drop else 1100), seconds(bar, offset), 0.36)
    if bar == 19:
        for step in range(16):
            place(groove, CLAP, seconds(bar, step / 4), 0.05 + 0.2 * step / 16)

gate = np.zeros(sample_count)
for start, end in [WORK, *RESTARTS, BUILD, DROP]:
    gain = 0.55 if (start, end) == WORK else 0.7 if (start, end) in RESTARTS else 0.42 if (start, end) == BUILD else 1.0
    gate[index(seconds(start)):index(seconds(end))] = gain
left_delay, right_delay = dub_delay(stabs)
bus_left = groove + left_delay * 0.55
bus_right = groove + right_delay * 0.55
rumble = filtered(room(groove, 1.0), "low", 100) * 0.7


def warp(bus):
    out = bus * gate
    for stop_bar, beats in STOPS.items():
        stop = index(seconds(stop_bar))
        length = index(beats * BEAT_SECONDS)
        speed = np.linspace(1, 0, length) ** 1.5
        out[stop - length:stop] = np.interp(stop - length + np.cumsum(speed), np.arange(sample_count), bus) * gate[stop - 1] * np.linspace(1, 0.1, length)
    for start, _ in RESTARTS:
        begin = index(seconds(start)); length = index(0.5 * BEAT_SECONDS)
        speed = np.linspace(0, 1, length)
        out[begin:begin + length] = np.interp(begin + length - (speed.sum() - np.cumsum(speed)), np.arange(sample_count), bus) * gate[begin] * np.linspace(0.2, 1, length)
    return out


def cutoff(t):
    bar = t / BAR_SECONDS
    if bar < WORK[1]:
        return 500 + 3500 * clip01((bar - 1) / 6.75) ** 1.4
    if bar < 14:
        return 2400
    if bar < DROP[0]:
        return 600 + 9000 * clip01((bar - BUILD[0]) / 2.5) ** 2
    if bar < END:
        return 13000
    return 5000


def clip01(x):
    return min(1.0, max(0.0, x))


left = sweep_lowpass(warp(bus_left) + warp(rumble), cutoff)
right = sweep_lowpass(warp(bus_right) + warp(rumble), cutoff)

beds = np.zeros(sample_count)
fx = np.zeros(sample_count)
place(fx, bell(86, 5.0), seconds(0, 1), 0.12)                 # the first point of light
for half in np.arange(8.0, 10.0, 0.5):
    place(fx, key_click(True), seconds(half), 0.05)           # the cursor, blinking in the silence
for yes in (9.75, 11.5, 12.75):
    place(fx, key_click(), seconds(yes), 0.35)                 # y
    place(fx, key_click(), seconds(yes) + 0.14, 0.4)           # enter
for bar in np.arange(14.0, 16.0, 0.5):
    place(beds, KICK, seconds(bar), 0.4)                      # heartbeat: you become the loop
place(beds, pad(D_MINOR_PAD, seconds(10), 460), seconds(8), 0.3)
place(fx, riser(seconds(2.5)), seconds(17.5), 0.18)
place(fx, cymbal(reverse=True, length_seconds=BAR_SECONDS), seconds(19), 0.25)
place(fx, sub_drop(), seconds(20), 0.9)
place(fx, cymbal(), seconds(20), 0.22)
place(beds, pad(D_MINOR_PAD, seconds(8), 1400), seconds(20), 0.16)
place(beds, pad(F_MAJOR_PAD, seconds(4) + 2.5, 1300), seconds(28), 0.42)  # the exhale
place(fx, KICK, seconds(28), 0.85)
place(fx, sub_drop(), seconds(28), 0.5)
for step, midi in enumerate((77, 81, 84, 88)):
    place(fx, bell(midi, 2.6), seconds(28.5 + step * 0.5), 0.1)

left = left + beds + fx
right = right + beds * 0.96 + fx
stereo = np.stack([room(left, 0.16), room(right, 0.16)], axis=1)
silence = slice(index(seconds(16)), index(seconds(17.5)))        # the crisis: one bar of nothing
stereo[silence] = np.stack([room(fx, 0.3)[silence]] * 2, axis=1)
fade = int(3.0 * SAMPLE_RATE)
stereo[-fade:] *= np.linspace(1, 0, fade)[:, None] ** 2
stereo = np.tanh(stereo * 1.2)
stereo /= np.max(np.abs(stereo)) / PEAK_CEILING
wavfile.write("assets/score.wav", SAMPLE_RATE, (stereo * 32767).astype(np.int16))

mono = stereo.mean(axis=1)
sub = filtered(mono, "low", 120)
samples_per_frame = SAMPLE_RATE // VIDEO_FPS
frames = int(TOTAL_SECONDS * VIDEO_FPS)
rms = np.array([np.sqrt(np.mean(sub[i * samples_per_frame:(i + 1) * samples_per_frame] ** 2)) for i in range(frames)])
json.dump({"fps": VIDEO_FPS, "sub": np.round(rms / (rms.max() + 1e-9), 4).tolist()}, open("assets/envelope.json", "w"))
print(f"wrote assets/score.wav ({TOTAL_SECONDS:.1f}s)")
