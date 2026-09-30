"""Offline mix for The Butlers Who Would Not Proceed (follows motion-video-kit references/audio.md).

- Music: "Secret Garden" (Eugenio Mininni, Mixkit), edited to picture: plays from its first downbeat, then an
  equal-power crossfade on a beat into the track's own ending so it resolves on the end card.
- Voice: macOS `say` lines at their cue times; music ducks under speech and drops for the deadpan triptych silence.
- Effects: per-event gain solved in each effect's own band against the bed (music + voice), 2-8 kHz lift capped.
- Master: integrated loudness to TARGET_LUFS, true peak limited.

Usage: python3 tools/mix.py            -> build/audio/mix.wav, build/audio/music_only.wav, build/audio/mix-report.txt
"""
import json
import re
import subprocess
from pathlib import Path

import librosa
import numpy as np
from scipy.signal import butter, sosfilt

ROOT = Path(__file__).resolve().parent.parent
BUILD = ROOT / "build"
OUT = BUILD / "audio"
MUSIC_SOURCE = BUILD / "music" / "secret-garden.mp3"   # fetched by tools/fetch_sfx.py; library audio is not committed
SR = 48000
FILM_SECONDS = 75.0
MUSIC_A_END_SOURCE = 57.1      # end of the first music section (source seconds), snapped to a bar line
MUSIC_SOURCE_END = 149.3       # the track's own ring-out (silence from ~148.7 s)
CROSSFADE_SECONDS = 0.3
MUSIC_BED_DB = -19.5           # music level before ducking, relative to full scale RMS target below
DUCK_UNDER_VOICE_DB = -6.0
DUCK_SILENCE_DB = -10.0        # the clock-tick silence over the triptych
SILENCE_WINDOW = (28.0, 31.7)
TARGET_LUFS = -16.0
TRUE_PEAK_DB = -2.0
SFX_TARGET_DB = 3.0            # in-band lift over the bed
SFX_HF_CAP_DB = 4.0            # 2-8 kHz lift cap

# film voice cues (must match VOICE_CUES in film.js)
VOICE_CUES = [
    ("n_chair", 5.9), ("n_butlers", 12.2), ("n_codex", 15.9), ("n_claude", 17.3), ("n_cursor", 18.7),
    ("n_stopped", 20.1), ("n_question", 26.5), ("n_beach", 32.3), ("f_still", 36.4),
    ("f_looper", 40.4), ("f_done", 43.2), ("f_back", 45.9), ("f_phone", 48.8),
    ("f_spaces", 56.2), ("f_no", 57.5), ("n_free", 60.9), ("n_day", 64.6),
]

# name -> ffmpeg soften chain (high-pass, low-pass, optional trim) per audio.md rule 1
SFX_SOURCES = {
    "whoosh": {"hp": 180, "lp": 10000},
    "sweep": {"hp": 260, "lp": 9000},
    "swoosh": {"hp": 200, "lp": 9000},
    "door": {"hp": 200, "lp": 9000, "trim": (0.0, 1.3)},
    "stamp": {"hp": 150, "lp": 7000},
    "pop": {"hp": 200, "lp": 9000},
    "click": {"hp": 300, "lp": 7000},
    "typing": {"hp": 250, "lp": 7000, "trim": (8.0, 1.3)},
    "tick": {"hp": 250, "lp": 7000, "trim": (4.0, 3.7)},
    "creak": {"hp": 200, "lp": 8000},
    "correct": {"hp": 200, "lp": 9000},
}
# (name, film time, in-band target dB over the bed, optional fixed gain dB for repeated sounds, audio.md rule 3)
EVENTS = [
    ("whoosh", 4.02, 3.0, 4.0),                                             # title card lifts away
    ("swoosh", 15.84, 2.0), ("swoosh", 17.24, 2.0), ("swoosh", 18.64, 2.0), ("swoosh", 20.0, 2.0),  # snap zooms
    ("tick", 28.0, 6.0),                                               # deadpan silence
    ("door", 31.8, 3.0),
    ("whoosh", 39.62, 3.0, 4.0),                                            # whip to the diagram
    ("stamp", 44.6, 4.0, 6.0), ("stamp", 45.9, 4.0, 6.0),
    ("typing", 47.2, 3.0), ("correct", 48.2, 3.0, -2.0),
    ("whoosh", 51.12, 3.0, 4.0),                                            # whip back to the room
    ("swoosh", 51.95, 2.5, 2.0), ("swoosh", 52.45, 2.5, 2.0),                    # cards take off
    ("pop", 54.05, 3.0, 5.0), ("pop", 54.55, 3.0, 5.0),                          # cards land in the phone
    ("pop", 55.45, 3.5, 5.0), ("pop", 55.85, 3.5, 5.0),                          # notifications on screen
    ("click", 56.05, 3.0, -7.0), ("click", 57.35, 3.0, -7.0),
    ("creak", 60.85, 4.0),
    ("swoosh", 62.2, 3.0, 2.0), ("whoosh", 63.42, 3.0, 4.0),                     # the dash, whip outside
]


def load(path, filters=None, trim=None):
    cmd = ["ffmpeg", "-v", "error"]
    if trim:
        cmd += ["-ss", str(trim[0]), "-t", str(trim[1])]
    cmd += ["-i", str(path)]
    if filters:
        cmd += ["-af", filters]
    cmd += ["-ac", "2", "-ar", str(SR), "-f", "f32le", "-"]
    raw = subprocess.run(cmd, capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.float32).reshape(-1, 2).copy()


def write(path, audio):
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "f32le", "-ar", str(SR), "-ac", "2", "-i", "-", str(path)],
                   input=np.clip(audio, -1, 1).astype(np.float32).tobytes(), check=True)


def loudness(path):
    err = subprocess.run(["ffmpeg", "-hide_banner", "-i", str(path), "-af", "ebur128=peak=true", "-f", "null", "-"],
                         capture_output=True, text=True).stderr
    integrated = float(re.findall(r"I:\s+(-?[\d.]+) LUFS", err)[-1])
    lra = float(re.findall(r"LRA:\s+([\d.]+) LU", err)[-1])
    peak = float((re.findall(r"Peak:\s+(-?[\d.]+) dBFS", err) or ["nan"])[-1])
    return integrated, lra, peak


def db(x):
    return 20 * np.log10(x)


def undb(x):
    return 10 ** (x / 20)


def rms(x):
    return float(np.sqrt(np.mean(x ** 2) + 1e-12))


def place(bus, clip, time, gain=1.0):
    start = int(round(time * SR))
    end = min(len(bus), start + len(clip))
    if end > start:
        bus[start:end] += clip[:end - start] * gain


def edit_music():
    """Film-length music: source from the first downbeat, crossfade on a beat into the ending."""
    mono, _ = librosa.load(str(MUSIC_SOURCE), sr=22050)
    _, beat_frames = librosa.beat.beat_track(y=mono, sr=22050)
    beats = librosa.frames_to_time(beat_frames, sr=22050)
    first = beats[0]
    first_index = 0
    a_index = int(np.argmin(np.abs(beats - MUSIC_A_END_SOURCE)))
    a_index -= (a_index - first_index) % 4                     # land on a bar line
    a_end = beats[a_index]
    film_join = a_end - first
    wanted_b = MUSIC_SOURCE_END - (FILM_SECONDS - film_join)
    candidates = [i for i in range(len(beats)) if (i - first_index) % 4 == 0]
    b_index = min(candidates, key=lambda i: abs(beats[i] - wanted_b))
    b_start = beats[b_index]
    source = load(MUSIC_SOURCE)
    half = int(CROSSFADE_SECONDS * SR / 2)
    part_a = source[int(first * SR):int(a_end * SR) + half]
    part_b = source[int(b_start * SR) - half:]
    ramp = np.linspace(0, np.pi / 2, 2 * half)[:, None]
    joined = np.concatenate([part_a[:-2 * half], part_a[-2 * half:] * np.cos(ramp) + part_b[:2 * half] * np.sin(ramp), part_b[2 * half:]])
    film = np.zeros((int(FILM_SECONDS * SR), 2), np.float32)
    film[:min(len(film), len(joined))] = joined[:len(film)]
    fade_in = int(0.005 * SR)                                   # a full-level first sample clicks
    film[:fade_in] *= np.linspace(0, 1, fade_in)[:, None]
    tail = int(1.0 * SR)
    film[-tail:] *= np.linspace(1, 0, tail)[:, None] ** 2
    return film, {"first_beat": round(float(first), 3), "join_film": round(float(film_join), 3),
                  "a_end_source": round(float(a_end), 3), "b_start_source": round(float(b_start), 3)}


def voice_bus():
    bus = np.zeros((int(FILM_SECONDS * SR), 2), np.float32)
    active = np.zeros(len(bus), bool)
    for line_id, time in VOICE_CUES:
        clip = load(BUILD / "voice" / f"{line_id}.wav", "highpass=f=90,acompressor=threshold=-20dB:ratio=2.5:attack=5:release=80")
        clip *= undb(-17.0) / rms(clip)                         # every line at the same speech level
        place(bus, clip, time)
        start = int(time * SR)
        active[start:start + len(clip)] = True
    return bus, active


def duck_envelope(active):
    target = np.where(active, DUCK_UNDER_VOICE_DB, 0.0)
    silence = (np.arange(len(active)) >= SILENCE_WINDOW[0] * SR) & (np.arange(len(active)) < SILENCE_WINDOW[1] * SR)
    target = np.where(silence, DUCK_SILENCE_DB, target)
    # smooth: 120 ms attack / 350 ms release one-pole in dB
    env = np.empty_like(target)
    level = 0.0
    attack, release = np.exp(-1 / (0.12 * SR)), np.exp(-1 / (0.35 * SR))
    for i, goal in enumerate(target):
        coeff = attack if goal < level else release
        level = goal + (level - goal) * coeff
        env[i] = level
    return undb(env)[:, None]


def band_peak(signal, sos, window):
    y = sosfilt(sos, signal.mean(axis=1))
    hop = int(0.025 * SR)
    size = int(0.05 * SR)
    if len(y) < size:
        return -120.0
    return max(10 * np.log10(np.mean(y[i:i + size] ** 2) + 1e-12) for i in range(0, len(y) - size, hop))


def solve_gain(bed, clip, time, target):
    """audio.md rule 2: lift `target` dB inside the effect's band; never more than the HF cap at 2-8 kHz."""
    spectrum = np.abs(np.fft.rfft(clip.mean(axis=1) * np.hanning(len(clip)))) ** 2
    freqs = np.fft.rfftfreq(len(clip), 1 / SR)
    cumulative = np.cumsum(spectrum) / spectrum.sum()
    low = max(freqs[np.searchsorted(cumulative, 0.2)], 60)
    high = min(max(freqs[np.searchsorted(cumulative, 0.8)], low * 2), SR / 2 - 500)
    band = butter(4, [low, high], btype="band", fs=SR, output="sos")
    hf = butter(4, [2000, 8000], btype="band", fs=SR, output="sos")
    start = int(time * SR)
    window = bed[start:start + max(len(clip), int(0.3 * SR))]
    base_band = max(band_peak(window, band, None), -60.0)       # level floor so a quiet bed can't hide the effect
    base_hf = max(band_peak(window, hf, None), -60.0)
    chosen = 0.0
    for gain in np.geomspace(0.003, 2.0, 160):
        mixed = window.copy()
        mixed[:len(clip)] += clip[:len(mixed)] * gain
        if band_peak(mixed, hf, None) - base_hf > SFX_HF_CAP_DB:
            break
        chosen = gain
        if band_peak(mixed, band, None) - base_band >= target:
            break
    return chosen, (low, high)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    music, edit_info = edit_music()
    music *= undb(MUSIC_BED_DB) / rms(music[int(3 * SR):int(50 * SR)])
    voices, active = voice_bus()
    music_ducked = music * duck_envelope(active)
    bed = music_ducked + voices
    sfx_bus = np.zeros_like(bed)
    report = [f"music edit: {json.dumps(edit_info)}"]
    clips = {name: load(ROOT / "build/sfx" / f"{name}.mp3",
                        f"highpass=f={cfg['hp']},lowpass=f={cfg['lp']},afade=t=in:d=0.005",
                        cfg.get("trim")) for name, cfg in SFX_SOURCES.items()}
    for name in ("typing", "tick", "door"):
        fade = int(0.15 * SR)
        clips[name][-fade:] *= np.linspace(1, 0, fade)[:, None]
    previous_time = -1.0
    for name, time, target, *fixed in EVENTS:
        gain, band = solve_gain(bed + sfx_bus, clips[name], time, target)
        if fixed:
            gain = undb(fixed[0])
        if time - previous_time < 0.15:
            gain *= 0.6                                          # rule 3: soften clustered sounds
        previous_time = time
        place(sfx_bus, clips[name], time, gain)
        report.append(f"{time:6.2f}s {name:8s} gain {db(gain + 1e-9):6.1f} dB  band {band[0]:.0f}-{band[1]:.0f} Hz")
    mix = bed + sfx_bus
    raw_path = OUT / "_raw.wav"
    write(raw_path, mix)
    integrated, _, _ = loudness(raw_path)
    gain = undb(TARGET_LUFS - integrated)
    for label, audio in (("mix", mix), ("music_only", music)):
        pre = OUT / f"_{label}_pre.wav"
        write(pre, audio * gain)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(pre), "-af",
                        f"alimiter=limit={undb(TRUE_PEAK_DB - 0.5):.4f}:attack=2:release=60:level=disabled",
                        "-ar", str(SR), str(OUT / f"{label}.wav")], check=True)
        pre.unlink()
    raw_path.unlink()
    for label in ("mix", "music_only"):
        integrated, lra, peak = loudness(OUT / f"{label}.wav")
        report.append(f"{label}: I {integrated:.1f} LUFS  LRA {lra:.1f} LU  peak {peak:.1f} dBFS")
    (OUT / "mix-report.txt").write_text("\n".join(report) + "\n")
    print("\n".join(report))


if __name__ == "__main__":
    main()
