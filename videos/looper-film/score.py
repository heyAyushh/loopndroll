"""Original score for "Keep the Loop" (90s), derived from the "looper" playlist analysis.

assets/playlist-analysis.json (38 tracks, measured from Apple preview clips):
  tempo cluster 124.5-129.2 BPM (median 127.6)  -> 128 BPM
  tonal centres D minor / F major               -> D minor, resolving to F major at dawn
  81% of energy below 100 Hz                    -> kick + rumble + sub carry the track
  spectral centroid ~1.6 kHz, <0.5% above 5 kHz -> dark master; the "air" only arrives at dawn
  bar-to-bar similarity 0.90                    -> one riff, varied only by texture

The groove is the agent's momentum: when an agent stops, the music stops (tape-stop). Each "y"
restarts it weaker, and every stop comes sooner. See DIRECTION.md.

Outputs: assets/score.wav, assets/envelope.json (per-video-frame sub / mid / air energy).
Run: python3 score.py
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
TOTAL_BARS = 48
TOTAL_SECONDS = TOTAL_BARS * BAR_SECONDS  # 90.0
SEED = 90
PEAK_CEILING = 0.72

# Groove windows in bars: (start, end, gain, brightness). Mirrors STORY in film/story.js.
GROOVE_WINDOWS = [
    (0.0, 8.0, 0.62, 1.0),     # setup: the agent writes
    (11.0, 12.0, 0.8, 0.8),    # "y" #1
    (12.5, 13.0, 0.7, 0.65),   # "y" #2, weaker
    (13.5, 14.0, 0.6, 0.5),    # "y" #3
    (14.5, 15.0, 0.5, 0.4),    # "y" #4, then they give up
    (24.0, 31.0, 0.8, 0.0),    # build: they wake to the problem (muffled, filter opens)
    (32.0, 40.0, 1.0, 1.0),    # the drop: Looper keeps it going
    (40.0, 42.0, 0.85, 0.6),   # dawn, still running
]
TAPE_STOP_BEATS = {8.0: 1.0, 12.0: 0.5, 13.0: 0.5, 14.0: 0.5, 15.0: 0.5, 31.0: 0.75}
TAPE_START_BEATS = 0.5
HEARTBEAT = (15.5, 24.0)
RAIN = (0.0, 19.0)
DROP = (32.0, 40.0)

rng = np.random.default_rng(SEED)
sample_count = int(TOTAL_SECONDS * SAMPLE_RATE)
time_axis = np.arange(sample_count) / SAMPLE_RATE


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

D_MINOR_STAB = [50, 53, 57, 60]
D_MINOR_PAD = [38, 45, 50, 53, 57, 64]
F_MAJOR_STAB = [53, 57, 60, 64]
F_MAJOR_PAD = [41, 48, 53, 57, 60, 67]
BASS_LINE = [None, 26, 26, 38, None, 26, 29, 26]

KICK, CLOSED, OPEN, CLAP, RIDE = kick(), hat(), hat(True), clap(), ride()
groove = np.zeros(sample_count)
groove_stabs = np.zeros(sample_count)

for bar in range(TOTAL_BARS):
    drop = DROP[0] <= bar < DROP[1]
    dawn = 40 <= bar < 42
    for beat in range(4):
        at = seconds(bar, beat)
        place(groove, KICK, at, 0.95)
        if bar >= 2 or drop:
            place(groove, OPEN, at + BEAT_SECONDS / 2, 0.16 if drop else 0.09)
        if drop or bar >= 27:
            for sixteenth in range(4):
                place(groove, CLOSED, at + sixteenth * BEAT_SECONDS / 4, 0.07 + 0.04 * (sixteenth == 2))
        if drop and beat in (1, 3):
            place(groove, CLAP, at, 0.3)
        if drop and bar >= 36:
            place(groove, RIDE, at + BEAT_SECONDS / 2, 0.08)
        if bar >= 3 and not dawn:
            for step in range(2):
                midi = BASS_LINE[(beat * 2 + step) % len(BASS_LINE)]
                if midi is not None:
                    place(groove, bass(midi), at + (step * 2 + 1) * BEAT_SECONDS / 4, 0.6)
    chord = F_MAJOR_STAB if bar >= 40 else D_MINOR_STAB
    if bar >= 4:
        for offset in (0.5, 1.75, 3.0):
            place(groove_stabs, stab(chord, brightness=2100 if drop else 1100), seconds(bar, offset), 0.38)
    if bar in (29, 30):
        subdivision = 2 if bar == 29 else 4
        for step in range(4 * subdivision):
            place(groove, CLAP, seconds(bar, step / subdivision), 0.06 + 0.2 * step / (4 * subdivision))

# Gate the groove by the story, with tape-stops into each Stop and tape-starts out of each "y".
gate = np.zeros(sample_count)
brightness = np.zeros(sample_count)
for start, end, gain, bright in GROOVE_WINDOWS:
    gate[index(seconds(start)):index(seconds(end))] = gain
    brightness[index(seconds(start)):index(seconds(end))] = bright
groove_bus = groove + dub_delay(groove_stabs)[0] * 0.5
groove_right = groove + dub_delay(groove_stabs)[1] * 0.5
rumble = filtered(room(groove, 1.0), "low", 100) * 0.7


def warp(bus):
    out = bus * gate
    for stop_bar, beats in TAPE_STOP_BEATS.items():
        stop = index(seconds(stop_bar))
        length = index(beats * BEAT_SECONDS)
        speed = np.linspace(1, 0, length) ** 1.5
        reads = stop - length + np.cumsum(speed)
        gain = gate[stop - 1]
        out[stop - length:stop] = np.interp(reads, np.arange(sample_count), bus) * gain * np.linspace(1, 0.15, length)
    for start, _, gain, _ in GROOVE_WINDOWS[1:5]:
        begin = index(seconds(start))
        length = index(TAPE_START_BEATS * BEAT_SECONDS)
        speed = np.linspace(0, 1, length)
        # Spin up from standstill and land back on the beat grid exactly at the end of the ramp.
        reads = begin + length - (speed.sum() - np.cumsum(speed))
        out[begin:begin + length] = np.interp(reads, np.arange(sample_count), bus) * gain * np.linspace(0.2, 1, length)
    return out


def groove_cutoff(t):
    bar = t / BAR_SECONDS
    if bar < 8:
        return 600 + 3000 * (bar / 8) ** 1.5
    if bar < 15:
        return 700 + 2600 * brightness[min(sample_count - 1, index(t))]
    if 24 <= bar < 31:
        return 700 + 8000 * ((bar - 24) / 7) ** 2
    if 32 <= bar < 40:
        return 12000
    return 6000


left = sweep_lowpass(warp(groove_bus) + warp(rumble), groove_cutoff)
right = sweep_lowpass(warp(groove_right) + warp(rumble), groove_cutoff)

# ---------- beds and story sounds (never gated) ----------
beds = np.zeros(sample_count)
fx = np.zeros(sample_count)

for bar in np.arange(HEARTBEAT[0], HEARTBEAT[1], 0.5):
    place(beds, KICK, seconds(bar), 0.45)  # the heartbeat while they sleep
for bar in range(16, 24, 2):
    place(beds, stab(D_MINOR_STAB, 0.3, 900), seconds(bar, 0.5), 0.3)
place(beds, pad(D_MINOR_PAD, seconds(24) - seconds(8), 480), seconds(8), 0.32)
place(beds, pad(D_MINOR_PAD, seconds(8), 1400), seconds(32), 0.16)
place(beds, pad(F_MAJOR_PAD, seconds(8) + 2.5, 1300), seconds(40), 0.4)

rain_length = index(seconds(RAIN[1] + 1))
rain = filtered(rng.standard_normal(rain_length), "band", [600, 5000]) * 0.05
drops = np.zeros(rain_length)
drops[rng.integers(0, rain_length, 2600)] = rng.uniform(0.2, 1.0, 2600)
rain += filtered(drops, "band", [1500, 6000]) * 0.25
rain *= np.clip((seconds(RAIN[1] + 1) - np.arange(rain_length) / SAMPLE_RATE) / BAR_SECONDS, 0, 1)
beds[:rain_length] += rain

for step in range(24):  # they type while the agent works
    place(fx, key_click(), seconds(4) + step * BEAT_SECONDS * 0.63 + (step % 3) * 0.04, 0.12)
place(fx, gate_clack(), seconds(8), 0.5)
place(fx, bell(74), seconds(8, 2), 0.24)         # "continue? [y/N]"
for bar, soft in ((10.6, False), (12.25, False), (13.25, True), (14.25, False)):
    place(fx, key_click(soft), seconds(bar), 0.4)   # "y"
    place(fx, key_click(soft), seconds(bar) + 0.16, 0.45)  # enter
for bar in (12.0, 13.0, 14.0, 15.0):
    place(fx, gate_clack(), seconds(bar), 0.32)
    place(fx, bell(74 if bar < 15 else 72, 4), seconds(bar, 0.2), 0.12)
place(fx, buzz(), seconds(24), 0.35)
place(fx, buzz(), seconds(24, 0.6), 0.35)
place(fx, riser(seconds(5)), seconds(26), 0.2)
for tap in (0.0, 0.3):
    place(fx, key_click(), seconds(26, 2 + tap), 0.3)  # "y"... and they stop
place(fx, gate_clack(), seconds(27.25), 0.4)          # the habit buys a quarter bar
place(fx, mouse_click(), seconds(31, 0.5), 0.5)       # open the Looper menu
place(fx, cymbal(reverse=True, length_seconds=BEAT_SECONDS * 1.2), seconds(31, 2.8), 0.28)
place(fx, mouse_click(), seconds(32), 0.6)            # the choice lands on the downbeat
place(fx, sub_drop(), seconds(32), 0.9)
place(fx, cymbal(), seconds(32), 0.25)
place(fx, buzz(0.2), seconds(37, 0.5), 0.2)           # the question on the couch
for tap in range(10):
    place(fx, key_click(True), seconds(37, 1.5) + tap * 0.09, 0.18)
place(fx, cymbal(reverse=True, length_seconds=BAR_SECONDS), seconds(39), 0.25)
chirps = rng.uniform(0, seconds(4), 14)
for offset in chirps:
    start_hz = rng.uniform(3200, 4800)
    place(fx, chirp(start_hz, start_hz * rng.uniform(1.15, 1.4)), seconds(40) + offset, 0.05)
place(fx, KICK, seconds(44), 0.9)
place(fx, sub_drop(), seconds(44), 0.5)
place(fx, cymbal(), seconds(44), 0.2)
for step, midi in enumerate((77, 81, 84, 88)):   # the square rounds into the orb: F A C E
    place(fx, bell(midi, 2.5), seconds(44.5 + step * 0.5), 0.12)

left = left + beds + fx
right = right + beds * 0.96 + fx
stereo = np.stack([room(left, 0.16), room(right, 0.16)], axis=1)

# The crisis bar holds its breath: only the clicks and the swell survive.
crisis = slice(index(seconds(31)), index(seconds(32)))
stereo[crisis] = np.stack([room(fx, 0.3)[crisis]] * 2, axis=1)

fade = int(3.5 * SAMPLE_RATE)
stereo[-fade:] *= np.linspace(1, 0, fade)[:, None] ** 2
stereo = np.tanh(stereo * 1.2)
stereo /= np.max(np.abs(stereo)) / PEAK_CEILING
wavfile.write("assets/score.wav", SAMPLE_RATE, (stereo * 32767).astype(np.int16))

mono = stereo.mean(axis=1)
bands = {"sub": filtered(mono, "low", 120), "mid": filtered(mono, "band", [300, 2500]), "air": filtered(mono, "high", 5000)}
samples_per_frame = SAMPLE_RATE // VIDEO_FPS
frame_count = int(TOTAL_SECONDS * VIDEO_FPS)
energy = {}
for name, signal in bands.items():
    rms = np.array([np.sqrt(np.mean(signal[i * samples_per_frame:(i + 1) * samples_per_frame] ** 2)) for i in range(frame_count)])
    energy[name] = np.round(rms / (rms.max() + 1e-9), 4).tolist()
json.dump({"fps": VIDEO_FPS, "bpm": BEATS_PER_MINUTE, **energy}, open("assets/envelope.json", "w"))
print(f"wrote assets/score.wav ({TOTAL_SECONDS:.2f}s) and assets/envelope.json ({frame_count} frames)")
