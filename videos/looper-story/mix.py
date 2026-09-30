#!/usr/bin/env python3
"""Offline mix for "Done means done", following motion-video-kit's audio rules.

- Music: Mixkit "Rising Sun" (id 892), edited to picture on its own beat grid (~76 BPM).
  Section A (build) under the night, low-passed and ducked through All Is Lost / Dark Night,
  then section B enters on the bar at Break into Three and rides to the track's natural stop
  under the end card.
- Effects: real library recordings only on real actions. Each gain is solved so the effect's
  50 ms in-band peak sits TARGET dB over the music in the same band and window, with the
  2-8 kHz lift capped and the peak capped over the local music peak.

Usage: python3 mix.py build/picture.mp4 build/looper-done-means-done.mp4 [--no-sfx]
Writes a per-event report to build/mix-report.txt.
"""
import re
import subprocess
import sys
from pathlib import Path

import numpy as np
from scipy.signal import butter, sosfilt

ROOT = Path(__file__).resolve().parent
SR = 48000
FILM_SECONDS = 50.0
MUSIC_BEAT = 0.78948
TRACK_FIRST_BEAT = 0.1045            # measured onset phase of the track's beat grid
MUSIC_FILE = ROOT / 'assets/music/rising-sun-mixkit-892.mp3'
SFX_DIR = ROOT / 'assets/sfx'

on_beat = lambda n: n * MUSIC_BEAT
track_beat = lambda n: TRACK_FIRST_BEAT + n * MUSIC_BEAT

# Film beats (must match film.html BEAT).
ALL_LOST, DARK_NIGHT, BREAK_THREE = on_beat(36), on_beat(38), on_beat(40)
SECTION_A_TRACK_BEAT = 12            # film beat n plays track beat n + 12 (bar-aligned with B)
SECTION_B_TRACK_BEAT = 96            # film beat 40 plays track beat 96; film beat 56 lands on 112
DARK_LOWPASS_HZ = 520
DARK_DUCK_DB = -9.0
# The track's groove stops on the logo (film ~45.4 s); lift its ring-out so the CTA never sits in silence.
TRACK_GROOVE_STOP = 89.75           # measured: where the track's groove drops into its ring-out
RING_OUT_START = BREAK_THREE + (TRACK_GROOVE_STOP - track_beat(SECTION_B_TRACK_BEAT))
RING_OUT_LIFT_DB = 8.0
CROSSFADE_S = 0.08
END_FADE_S = 0.7
MIX_TARGET_LUFS = -16.0              # calm, cinematic piece (quality-bar: about -16 LUFS)
TRUE_PEAK_CEILING_DB = -2.6          # AAC encoding overshoots the limiter by ~0.7 dB

TARGET_DB = 3.5                      # in-band lift of an effect over the music
WHOOSH_IDS = {'1492'}
WHOOSH_TARGET_DB = 2.5               # whooshes stay a soft air pass (audio.md)
HF_CAP_DB = 4.0                      # cap on the ear-sensitive 2-8 kHz lift
PEAK_CAP_DB = 6.0                    # effect peak vs local music peak
CLUSTER_SOFTEN = 0.6                 # sounds within 0.15 s of another get x0.6

B = lambda n: on_beat(n)
LAP_START, LAP = B(21) + B(1), B(2)
# [file id, film time, source start, source duration, note]
FINALE = B(45)
PLAN = [
    ('1485', B(7) - 0.02, 0, None, 'first bubble: "keep going, please"'),
    ('1492', B(12) - 0.35, 0, 0.6, 'the wall collapses', 4.0),
    ('2356', B(12) + 0.6, 0, None, 'Session stopped pill'),
    ('1492', B(14) - 0.35, 0, 0.6, 'the fail line grows into "14 failed"'),
    ('1392', B(16) + 0.15, 1.0, 0.5, 'typing "keep go"'),
    ('1492', B(18) + 0.15, 0, 0.6, 'the orb rolls in'),
    ('2568', B(18) + 1.95, 0, None, 'picker lands on Completion Checks'),
    *[('2356', LAP_START + LAP / 4 + k * LAP, 0, None, f'checks run, lap {k + 1}') for k in range(4)],
    ('1110', B(30), 0, None, 'tiles land'),
    ('2356', B(30) + 0.55, 0, None, 'lint fails'),
    ('2356', B(30) + 0.85, 0, None, 'typecheck fails'),
    ('2358', B(30) + 1.9, 0, None, 'lint fixed'),
    ('2358', B(30) + 2.2, 0, None, 'typecheck fixed'),
    ('2356', B(36) + 0.2, 0, None, 'All Is Lost: session stopped'),
    ('1107', B(40) + 0.45, 0, None, 'phone notification'),
    ('1376', B(40) + 1.6, 0.3, 1.7, 'typing the reply'),
    ('2848', FINALE - 0.05, 0, None, 'send'),
    ('1485', FINALE + 1.0, 0, None, 'reply lands in the same session'),
    ('2867', FINALE + 3.2, 0, None, 'all checks pass (hero)'),
    ('1492', B(52) - 0.3, 0, 0.6, 'night turns to dawn'),
]


def decode(path, start=0.0, duration=None, extra_filter=None):
    cmd = ['ffmpeg', '-v', 'error', '-ss', str(start)]
    if duration:
        cmd += ['-t', str(duration)]
    cmd += ['-i', str(path)]
    if extra_filter:
        cmd += ['-af', extra_filter]
    cmd += ['-ac', '2', '-ar', str(SR), '-f', 'f32le', '-']
    raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.float32).reshape(-1, 2).copy()


def write_wav(path, x):
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 'f32le', '-ar', str(SR), '-ac', '2', '-i', '-', str(path)],
                   input=np.clip(x, -1, 1).astype(np.float32).tobytes(), check=True)


def loudness(x):
    tmp = ROOT / 'build/_lufs.wav'
    write_wav(tmp, x)
    err = subprocess.run(['ffmpeg', '-hide_banner', '-i', str(tmp), '-af', 'ebur128=peak=true', '-f', 'null', '-'],
                         capture_output=True, text=True).stderr
    integrated = float(re.findall(r'I:\s+(-?[\d.]+) LUFS', err)[-1])
    peak = float((re.findall(r'Peak:\s+(-?[\d.]+) dBFS', err) or ['nan'])[-1])
    return integrated, peak


def ramp_gain(n, points):
    """Piecewise-linear gain curve from [(time_s, linear_gain), ...]."""
    times = np.arange(n) / SR
    return np.interp(times, [p[0] for p in points], [p[1] for p in points]).astype(np.float32)


def music_stem():
    total = int(FILM_SECONDS * SR)
    a_len = BREAK_THREE + CROSSFADE_S
    section_a = decode(MUSIC_FILE, track_beat(SECTION_A_TRACK_BEAT), a_len)
    section_b = decode(MUSIC_FILE, track_beat(SECTION_B_TRACK_BEAT) - CROSSFADE_S, FILM_SECONDS - BREAK_THREE + CROSSFADE_S)

    # All Is Lost -> Dark Night: the world muffles and recedes, but the pulse never fully dies.
    lowpass = butter(2, DARK_LOWPASS_HZ, btype='low', fs=SR, output='sos')
    muffled = sosfilt(lowpass, section_a, axis=0).astype(np.float32)
    wet = ramp_gain(len(section_a), [(0, 0), (ALL_LOST, 0), (DARK_NIGHT - 0.4, 1), (a_len, 1)])[:, None]
    duck = ramp_gain(len(section_a), [(0, 1), (ALL_LOST + 0.3, 1), (DARK_NIGHT, 10 ** (DARK_DUCK_DB / 20)), (a_len, 10 ** (DARK_DUCK_DB / 20))])[:, None]
    section_a = (section_a * (1 - wet) + muffled * wet) * duck

    out = np.zeros((total, 2), np.float32)
    start_b = int((BREAK_THREE - CROSSFADE_S) * SR)
    fade_len = int(2 * CROSSFADE_S * SR)
    k = np.linspace(0, np.pi / 2, fade_len, dtype=np.float32)[:, None]
    a_part = section_a[:start_b + fade_len]
    out[:len(a_part)] += a_part * np.concatenate([np.ones((start_b, 1), np.float32), np.cos(k)])[:len(a_part)]
    b_part = section_b[:total - start_b]
    b_env = np.concatenate([np.sin(k), np.ones((len(b_part) - fade_len, 1), np.float32)])
    out[start_b:start_b + len(b_part)] += b_part * b_env
    lift = 10 ** (RING_OUT_LIFT_DB / 20)
    out *= ramp_gain(total, [(0, 1), (RING_OUT_START, 1), (RING_OUT_START + 0.1, lift), (FILM_SECONDS, lift)])[:, None]
    out[:int(0.005 * SR)] *= np.linspace(0, 1, int(0.005 * SR))[:, None]      # a full-level first sample clicks
    tail = int(END_FADE_S * SR)
    out[-tail:] *= (np.linspace(1, 0, tail) ** 1.5)[:, None]
    return out


def band_for(effect):
    spectrum = np.abs(np.fft.rfft(effect.mean(1))) ** 2
    freqs = np.fft.rfftfreq(len(effect), 1 / SR)
    centroid = (spectrum * freqs).sum() / spectrum.sum()
    low, high = max(150, centroid / 2), min(12000, centroid * 2)
    return butter(4, [low, high], btype='band', fs=SR, output='sos'), centroid


def peak_db(y, window=0.05):
    hop = int(window * SR)
    if len(y) <= hop:
        return 10 * np.log10((y ** 2).mean() + 1e-12)
    return max(10 * np.log10((y[i:i + hop] ** 2).mean() + 1e-12) for i in range(0, len(y) - hop, hop // 2))


def effects_stem(music):
    total = len(music)
    fx = np.zeros_like(music)
    hf_band = butter(4, [2000, 8000], btype='band', fs=SR, output='sos')
    mono_music = music.mean(1)
    times = sorted(p[1] for p in PLAN)
    report = []
    for sfx_id, at, src_start, src_dur, note, *override in PLAN:
        effect = decode(SFX_DIR / f'mixkit-{sfx_id}.mp3', src_start, src_dur,
                        'highpass=f=170,lowpass=f=10000,afade=t=in:d=0.008')
        fade = min(len(effect), int(0.08 * SR))
        effect[-fade:] *= np.linspace(1, 0, fade)[:, None]
        i0 = int(at * SR)
        length = min(len(effect), total - i0)
        effect = effect[:length]
        band, centroid = band_for(effect)
        window = mono_music[i0:i0 + max(length, int(0.2 * SR))]
        music_in_band = peak_db(sosfilt(band, window))
        effect_in_band = peak_db(sosfilt(band, effect.mean(1)))
        target = override[0] if override else (WHOOSH_TARGET_DB if sfx_id in WHOOSH_IDS else TARGET_DB)
        gain_db = music_in_band + target - effect_in_band
        # Cap the 2-8 kHz lift so ticks never spike.
        music_hf = peak_db(sosfilt(hf_band, window))
        effect_hf = peak_db(sosfilt(hf_band, effect.mean(1))) + gain_db
        hf_lift = 10 * np.log10(10 ** (music_hf / 10) + 10 ** (effect_hf / 10)) - music_hf
        if hf_lift > HF_CAP_DB:
            gain_db -= hf_lift - HF_CAP_DB
        # Cap the full-band peak over the local music peak.
        over = peak_db(effect.mean(1)) + gain_db - peak_db(window)
        if over > PEAK_CAP_DB:
            gain_db -= over - PEAK_CAP_DB
        gain = 10 ** (gain_db / 20)
        if any(0 < abs(other - at) < 0.15 for other in times):
            gain *= CLUSTER_SOFTEN
        fx[i0:i0 + length] += effect * gain
        report.append(f'{at:6.2f}s  {sfx_id:>5}  centroid {centroid:5.0f} Hz  gain {20 * np.log10(gain):6.1f} dB  hf-lift<= {min(hf_lift, HF_CAP_DB):4.1f}  {note}')
    return fx, report


def main():
    picture, output = sys.argv[1], sys.argv[2]
    no_sfx = '--no-sfx' in sys.argv
    music = music_stem()
    fx, report = (np.zeros_like(music), []) if no_sfx else effects_stem(music)
    mix = music + fx
    integrated, _ = loudness(mix)
    mix *= 10 ** ((MIX_TARGET_LUFS - integrated) / 20)
    master = ROOT / 'build/_master.wav'
    write_wav(master, mix)
    subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', picture, '-i', str(master),
                    '-af', f'alimiter=limit={10 ** (TRUE_PEAK_CEILING_DB / 20)}:attack=5:release=60:level=disabled',
                    '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '256k', '-shortest', output], check=True)
    if report:
        (ROOT / 'build/mix-report.txt').write_text('\n'.join(report) + '\n')
    print('\n'.join(report))
    print('wrote', output)


if __name__ == '__main__':
    main()
