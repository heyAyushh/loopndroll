"""Original score for the Looper launch film, derived from the "looper" playlist analysis.

assets/playlist-analysis.json (38 tracks, measured from Apple preview clips):
  tempo cluster 124.5-129.2 BPM (median 127.6)  -> 128 BPM
  tonal centres D minor / F major, A minor / C  -> D minor, resolving to F major at dawn
  81% of energy below 100 Hz                    -> kick + rumble + sub carry the track
  spectral centroid ~1.6 kHz, <0.5% above 5 kHz -> dark master lowpass, sparse hats
  bar-to-bar similarity 0.90                    -> one 8-bar phrase, varied only by texture

Nothing is sampled. Outputs:
  assets/score.wav      stereo 44.1 kHz
  assets/envelope.json  per-video-frame kick/sub, mid, and air energy for the picture to follow

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
TOTAL_BARS = 40
TOTAL_SECONDS = TOTAL_BARS * BAR_SECONDS  # 75.0
SEED = 47
PEAK_CEILING = 0.72

# Story, in bars. Five loops of 8 bars; film.html reads the same numbers.
SETUP = (0, 6)            # 02:40, the agent writes
INCITING = (6, 8)         # 02:47, it stops and asks
COMPLICATIONS = (8, 16)   # waiting, giving up, lying down
ESCALATION = (16, 23)     # the stops pile up; pressure returns
CRISIS = (23, 24)         # 05:10, the phone in the dark: the choice
CLIMAX = (24, 32)         # one line from bed; the loop resumes
RESOLUTION = (32, 36)     # dawn
END_CARD = (36, 40)

rng = np.random.default_rng(SEED)
sample_count = int(TOTAL_SECONDS * SAMPLE_RATE)


def seconds(bar, beat=0.0):
    return bar * BAR_SECONDS + beat * BEAT_SECONDS


def span(act):
    return slice(int(seconds(act[0]) * SAMPLE_RATE), int(seconds(act[1]) * SAMPLE_RATE))


def in_act(bar, *acts):
    return any(start <= bar < end for start, end in acts)


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
    start = int(at_seconds * SAMPLE_RATE)
    end = min(len(track), start + len(sound))
    if start < end:
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


def bell(midi, decay=3.0):
    length = int(1.8 * SAMPLE_RATE)
    t = np.arange(length) / SAMPLE_RATE
    return (np.sin(2 * np.pi * note_hz(midi) * t) + 0.25 * np.sin(2 * np.pi * note_hz(midi + 12) * t)) * envelope(length, 0.002, decay)


def thumb_tap():
    length = int(0.03 * SAMPLE_RATE)
    return filtered(rng.standard_normal(length), "band", [1800, 5000]) * envelope(length, 0.0005, 160)


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


# ---------- arrangement ----------

D_MINOR_STAB = [50, 53, 57, 60]         # D F A C  (Dm7)
D_MINOR_PAD = [38, 45, 50, 53, 57, 64]
F_MAJOR_STAB = [53, 57, 60, 64]         # F A C E  (Fmaj7)
F_MAJOR_PAD = [41, 48, 53, 57, 60, 67]
BASS_LINE = [None, 26, 26, 38, None, 26, 29, 26]  # rolling sixteenths around D1

drums = np.zeros(sample_count)
low = np.zeros(sample_count)
stabs = np.zeros(sample_count)
pads = np.zeros(sample_count)
fx = np.zeros(sample_count)
KICK, CLOSED, OPEN, CLAP = kick(), hat(), hat(True), clap()

GROOVE = (SETUP, (16, 23), CLIMAX, (32, 34))
for bar in range(TOTAL_BARS):
    groove = in_act(bar, *GROOVE)
    full = in_act(bar, CLIMAX)
    for beat in range(4):
        at = seconds(bar, beat)
        if groove:
            place(drums, KICK, at, 0.95)
        elif in_act(bar, COMPLICATIONS) and beat in (0, 2):
            place(drums, KICK, at, 0.5)  # heartbeat
        if (groove and bar >= 2) or (in_act(bar, COMPLICATIONS) and bar >= 12):
            place(drums, OPEN, at + BEAT_SECONDS / 2, 0.16 if full else 0.08)
        if full or bar >= 20 and in_act(bar, ESCALATION):
            for sixteenth in range(4):
                place(drums, CLOSED, at + sixteenth * BEAT_SECONDS / 4, 0.07 + 0.04 * (sixteenth == 2))
        if full and beat in (1, 3):
            place(drums, CLAP, at, 0.3)
        if (groove and bar >= 3) and not in_act(bar, (32, 34)):
            for step in range(2):
                midi = BASS_LINE[(beat * 2 + step) % len(BASS_LINE)]
                if midi is not None:
                    place(low, bass(midi), at + (step * 2 + 1) * BEAT_SECONDS / 4, 0.6)

    # The loop itself: three offbeat stabs per bar, the same every bar, as the playlist does.
    if (bar >= 4 and in_act(bar, SETUP)) or full or (bar >= 18 and in_act(bar, ESCALATION)):
        for offset in (0.5, 1.75, 3.0):
            place(stabs, stab(D_MINOR_STAB, brightness=1100 if bar < 24 else 2000), seconds(bar, offset), 0.38)
    elif in_act(bar, COMPLICATIONS) and bar % 2 == 1:
        place(stabs, stab(D_MINOR_STAB, 0.3, 900), seconds(bar, 0.5), 0.42)
    elif in_act(bar, RESOLUTION, END_CARD) and bar % 2 == 0:
        place(stabs, stab(F_MAJOR_STAB, 0.35, 1900), seconds(bar, 0.5), 0.32)

    if bar in (21, 22):
        subdivision = 2 if bar == 21 else 4
        for step in range(4 * subdivision):
            place(drums, CLAP, seconds(bar, step / subdivision), 0.06 + 0.18 * step / (4 * subdivision))

place(pads, pad(D_MINOR_PAD, seconds(6), 500), 0, 0.26)
place(pads, pad(D_MINOR_PAD, seconds(16), 450), seconds(6), 0.34)
place(pads, pad(D_MINOR_PAD, seconds(8), 1300), seconds(24), 0.2)
place(pads, pad(F_MAJOR_PAD, seconds(8) + 2.5, 1200), seconds(32), 0.4)

# Story punctuation.
place(fx, bell(74), seconds(6, 0.1), 0.28)          # 02:47 the agent asks (D5)
place(fx, bell(74), seconds(10, 0.1), 0.12)         # it keeps asking, quieter
place(fx, bell(77, 4), seconds(14, 0.1), 0.1)
place(fx, riser(seconds(3)), seconds(20), 0.2)
place(fx, bell(81, 2.2), seconds(23, 0.0), 0.26)    # 05:10 the phone lights (A5)
for index, beat in enumerate((0.9, 1.15, 1.35, 1.6, 1.75, 2.0, 2.2, 2.35, 2.6, 2.8, 3.0)):
    place(fx, thumb_tap(), seconds(23, beat), 0.35)  # typing one line
place(fx, cymbal(reverse=True, length_seconds=BEAT_SECONDS * 0.9), seconds(23, 3.1), 0.3)
place(fx, sub_drop(), seconds(24), 0.9)
place(fx, cymbal(), seconds(24), 0.25)
place(drums, KICK, seconds(36), 0.9)
place(fx, sub_drop(), seconds(36), 0.5)
place(fx, cymbal(), seconds(36), 0.2)

# Sidechain against the kick while the groove plays.
kick_active = np.zeros(sample_count, dtype=bool)
for act in GROOVE:
    kick_active[span(act)] = True
beat_phase = (np.arange(sample_count) / SAMPLE_RATE) % BEAT_SECONDS
sidechain = np.where(kick_active, 1 - 0.6 * np.exp(-beat_phase * 14), 1.0)

rumble = filtered(room(np.where(kick_active, drums, 0), 1.0), "low", 100) * 0.7
stab_left, stab_right = dub_delay(stabs * sidechain)
left = drums + low * sidechain + rumble + stab_left + pads * sidechain + fx
right = drums + low * sidechain + rumble + stab_right + pads * sidechain * 0.95 + fx


def master_cutoff(t):
    """Dark by default (the playlist lives under ~2 kHz); only the climax opens up."""
    if t < seconds(6):
        return 600 + 900 * t / seconds(6)
    if t < seconds(16):
        return 900
    if t < seconds(23):
        return 900 + 5000 * ((t - seconds(16)) / seconds(7)) ** 2
    if t < seconds(32):
        return 9000
    return 5000


stereo = np.stack([sweep_lowpass(room(channel, 0.18), master_cutoff) for channel in (left, right)], axis=1)

# 02:47: the agent stops, and so does the music (tape-stop into near silence).
stop_start, stop_end = int((seconds(6) - BEAT_SECONDS) * SAMPLE_RATE), int(seconds(6) * SAMPLE_RATE)
speed = np.linspace(1, 0, stop_end - stop_start) ** 1.5
reads = stop_start + np.cumsum(speed)
for channel in range(2):
    stereo[stop_start:stop_end, channel] = np.interp(reads, np.arange(sample_count), stereo[:, channel]) * np.linspace(1, 0.2, stop_end - stop_start)
stereo[span(INCITING)] = np.stack([room(fx, 0.5)[span(INCITING)] + pads[span(INCITING)]] * 2, axis=1)

# 05:10: the crisis bar holds its breath: the bell, the thumb, nothing else.
stereo[span(CRISIS)] = np.stack([room(fx, 0.35)[span(CRISIS)]] * 2, axis=1)

for act, gain in ((SETUP, 0.6), (ESCALATION, 0.8), (RESOLUTION, 1.25)):
    stereo[span(act)] *= gain
fade = int(3.0 * SAMPLE_RATE)
stereo[-fade:] *= np.linspace(1, 0, fade)[:, None] ** 2
stereo = np.tanh(stereo * 1.2)
stereo /= np.max(np.abs(stereo)) / PEAK_CEILING
wavfile.write("assets/score.wav", SAMPLE_RATE, (stereo * 32767).astype(np.int16))

# Per-frame energy so the room's light can breathe with the low end.
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
