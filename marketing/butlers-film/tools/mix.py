"""Silent-film mix for The Butlers Who Would Not Proceed (Chaplin treatment; follows motion-video-kit references/audio.md).

- Score: Scott Joplin, "The Cascades" (1904), player-piano roll recording, Internet Archive "1904Soundtrack",
  Public Domain Mark. Edited to picture: its opening strains from the first downbeat, then an equal-power join on a
  bar line into the rag's own final cadence, which lands on the end card.
- Projector whirr under the whole film, with a start and stop on the first and last frames.
- The theatre's trap drummer: slide whistles, a toy whistle and a rimshot on the gags; Foley footsteps from the film's
  own gait math; door creak, cloth, squeaks, slurp, clinks.
- Every event is authored in story time and placed through the film's real<->story time map (build/audio/timing.json),
  so sounds stay on their frames even though title cards freeze the story and the rest is undercranked.
- Effects are levelled in their own band against the bed (rule 2); repeated footsteps share one level (rule 3).
- Master: integrated loudness to TARGET_LUFS, true peak limited.

Usage: python3 tools/mix.py   -> build/audio/mix-chaplin.wav, build/audio/music_only.wav, build/audio/mix-report.txt
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
MUSIC_SOURCE = BUILD / "music" / "ragtime-1904.mp3"   # downloaded by tools/fetch_sfx.py (Internet Archive, Public Domain Mark)
SR = 48000
TIMING = json.loads((OUT / "timing.json").read_text())
FILM_SECONDS = float(TIMING["duration"])
CASCADES_END = 173.35          # the rag's final chord rings out here (source seconds)
ENDING_SECONDS = 12.6          # the closing strain and cadence, landing on the end card
CROSSFADE_SECONDS = 0.25
MUSIC_BED_DB = -18.0
PROJECTOR_DB = -34.0
DUCK_SILENCE_DB = -9.0         # the deadpan clock-tick silence over the triptych (story time)
SILENCE_STORY = (28.0, 31.7)
TARGET_LUFS = -16.0
TRUE_PEAK_DB = -2.0
SFX_HF_CAP_DB = 4.0

# (clip name, story time, in-band target dB | None, fixed gain dB | None)
EVENTS = [
    ("tick", 28.0, 6.0, None),                      # deadpan silence
    ("creak", 31.82, 5.0, None),                    # the guest door
    ("slurp", 38.42, 5.0, None),                    # the coconut, instead of an answer
    ("cloth", 39.1, 5.0, None),                     # phone comes out
    ("whistle_up", 39.85, None, -4.0),              # whip to the diagram
    ("stamp", 44.6, None, 4.0), ("stamp", 45.9, None, 4.0),
    ("typing", 47.2, 3.0, None), ("correct", 48.2, None, -4.0),
    ("whistle_up", 51.3, None, -4.0),               # whip back to the room
    ("clink", 52.0, 5.0, None), ("clink", 52.5, 5.0, None),
    ("whistle_up", 52.05, None, -3.0),              # cards take off
    ("whistle_fall", 53.55, None, -3.0),            # ...and dive into the phone
    ("pop", 55.45, None, 2.0), ("pop", 55.85, None, 2.0),
    ("click", 56.05, None, -7.0), ("click", 57.35, None, -7.0),
    ("rimshot", 57.62, None, -3.0),                 # "Absolutely not."
    ("cloth", 59.05, 5.0, None), ("cloth", 59.15, 5.0, None),   # the bows
    ("squeak", 60.95, 5.0, None),                   # the developer gets up
    ("squeak", 61.85, 5.0, None),                   # the chair spins
    ("toy_whistle", 62.82, None, -8.0),             # heel-click
    ("whistle_up", 63.62, None, -4.0),              # whip outside
]
SOURCES = {   # clip name -> (path, ffmpeg filter, optional trim); everything else comes from build/foley/
    "tick": (BUILD / "sfx/tick.mp3", "highpass=f=250,lowpass=f=7000", (4.0, 3.7)),
    "stamp": (BUILD / "sfx/stamp.mp3", "highpass=f=150,lowpass=f=7000", None),
    "typing": (BUILD / "sfx/typing.mp3", "highpass=f=250,lowpass=f=7000", (8.0, 1.3)),
    "correct": (BUILD / "sfx/correct.mp3", "highpass=f=200,lowpass=f=9000", None),
    "pop": (BUILD / "sfx/pop.mp3", "highpass=f=200,lowpass=f=9000", None),
    "click": (BUILD / "sfx/click.mp3", "highpass=f=300,lowpass=f=7000", None),
}
FOOTSTEP_TARGET_DB = 4.0
FOOTSTEP_VARIANTS = 6


# ---------------------------------------------------------------- time map
def real_time(story):
    """Story time -> real time through the film's play segments (cards hold the story still)."""
    for seg in TIMING["segments"]:
        if seg["kind"] == "play" and seg["storyStart"] <= story < seg["storyEnd"]:
            return seg["realStart"] + (story - seg["storyStart"]) / seg["speed"]
    return FILM_SECONDS


# ---------------------------------------------------------------- audio helpers
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
    err = subprocess.run(["ffmpeg", "-hide_banner", "-i", str(path), "-af", "ebur128=peak=true", "-f", "null", "-"], capture_output=True, text=True).stderr
    return (float(re.findall(r"I:\s+(-?[\d.]+) LUFS", err)[-1]), float(re.findall(r"LRA:\s+([\d.]+) LU", err)[-1]),
            float((re.findall(r"Peak:\s+(-?[\d.]+) dBFS", err) or ["nan"])[-1]))


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


def band_peak(signal, sos):
    y = sosfilt(sos, signal.mean(axis=1))
    hop, size = int(0.025 * SR), int(0.05 * SR)
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
    base_band, base_hf = max(band_peak(window, band), -60.0), max(band_peak(window, hf), -60.0)
    chosen = 0.0
    for gain in np.geomspace(0.003, 2.8, 170):
        mixed = window.copy()
        mixed[:len(clip)] += clip[:len(mixed)] * gain
        if band_peak(mixed, hf) - base_hf > SFX_HF_CAP_DB:
            break
        chosen = gain
        if band_peak(mixed, band) - base_band >= target:
            break
    return chosen


# ---------------------------------------------------------------- score, silence, projector
def edit_music():
    """Opening strains from the first downbeat, joined on a bar line into the rag's own ending."""
    mono, _ = librosa.load(str(MUSIC_SOURCE), sr=22050, duration=CASCADES_END + 1)
    _, beat_frames = librosa.beat.beat_track(y=mono, sr=22050)
    beats = librosa.frames_to_time(beat_frames, sr=22050)
    first = beats[0]
    bar_lines = [i for i in range(len(beats)) if i % 4 == 0]
    b_index = min(bar_lines, key=lambda i: abs(beats[i] - (CASCADES_END - ENDING_SECONDS)))
    ending = CASCADES_END - beats[b_index]
    # the latest bar line that still lets the whole cadence ring out before the last frame
    a_index = max(i for i in bar_lines if (beats[i] - first) + ending <= FILM_SECONDS - 0.3)
    source = load(MUSIC_SOURCE, "highpass=f=70,lowpass=f=9000")
    half = int(CROSSFADE_SECONDS * SR / 2)
    part_a = source[int(first * SR):int(beats[a_index] * SR) + half]
    part_b = source[int(beats[b_index] * SR) - half:int(CASCADES_END * SR)]
    ramp = np.linspace(0, np.pi / 2, 2 * half)[:, None]
    joined = np.concatenate([part_a[:-2 * half], part_a[-2 * half:] * np.cos(ramp) + part_b[:2 * half] * np.sin(ramp), part_b[2 * half:]])
    film = np.zeros((int(FILM_SECONDS * SR), 2), np.float32)
    film[:min(len(film), len(joined))] = joined[:len(film)]
    film[:int(0.005 * SR)] *= np.linspace(0, 1, int(0.005 * SR))[:, None]
    return film, {"first_beat": round(float(first), 3), "join_source": round(float(beats[a_index]), 3),
                  "ending_from": round(float(beats[b_index]), 3), "music_length": round(len(joined) / SR, 2)}


def silence_duck(length):
    start, end = real_time(SILENCE_STORY[0]), real_time(SILENCE_STORY[1])
    target = np.zeros(length)
    target[int(start * SR):int(end * SR)] = DUCK_SILENCE_DB
    env, level = np.empty(length), 0.0
    attack, release = np.exp(-1 / (0.12 * SR)), np.exp(-1 / (0.35 * SR))
    for i, goal in enumerate(target):
        level = goal + (level - goal) * (attack if goal < level else release)
        env[i] = level
    return undb(env)[:, None]


def projector_bed(length):
    loop = load(BUILD / "foley/projector.wav")
    bed = np.concatenate([loop] * (int(np.ceil(length / len(loop))) + 1))[:length] * undb(PROJECTOR_DB)
    fade = int(1.0 * SR)
    bed[:fade] *= np.linspace(0, 1, fade)[:, None]
    bed[-fade:] *= np.linspace(1, 0, fade)[:, None]
    ends = load(BUILD / "foley/projector_ends.wav")[: int(1.2 * SR)]
    place(bed, ends, 0.0, undb(-22))
    place(bed, ends, FILM_SECONDS - 1.4, undb(-24))
    return bed


# ---------------------------------------------------------------- main
def main():
    OUT.mkdir(parents=True, exist_ok=True)
    report = []
    music, edit_info = edit_music()
    music *= undb(MUSIC_BED_DB) / rms(music[int(3 * SR):int(60 * SR)])
    report.append(f"music edit: {json.dumps(edit_info)}; undercrank {TIMING['undercrank']:.3f}")
    bed = music * silence_duck(len(music)) + projector_bed(len(music))
    fx = np.zeros_like(bed)
    cache = {}

    def clip(name):
        if name not in cache:
            if name in SOURCES:
                path, filters, trim = SOURCES[name]
                audio = load(path, f"{filters},afade=t=in:d=0.005", trim)
                fade = int(0.12 * SR)
                audio[-fade:] *= np.linspace(1, 0, fade)[:, None]
            else:
                audio = load(BUILD / "foley" / f"{name}.wav", "afade=t=in:d=0.004")[: int(1.6 * SR)]
            cache[name] = audio
        return cache[name]

    for name, story, target, fixed in EVENTS:
        time = real_time(story)
        gain = undb(fixed) if fixed is not None else solve_gain(bed + fx, clip(name), time, target)
        place(fx, clip(name), time, gain)
        report.append(f"{story:6.2f}s story -> {time:6.2f}s  {name:12s} {db(gain + 1e-9):+6.1f} dB")

    steps = TIMING["footsteps"]
    solved = {}
    for i, step in enumerate(steps):
        name = f"{step['kind']}_{i % FOOTSTEP_VARIANTS}"
        solved.setdefault(step["kind"], []).append(db(solve_gain(bed, clip(name), real_time(step["t"]), FOOTSTEP_TARGET_DB) + 1e-9))
    level = {kind: float(np.clip(np.median(values), -18, 9)) for kind, values in solved.items()}   # rule 3
    for i, step in enumerate(steps):
        place(fx, clip(f"{step['kind']}_{i % FOOTSTEP_VARIANTS}"), real_time(step["t"]), undb(level[step["kind"]]))
    report.append(f"footsteps: {len(steps)} at {json.dumps({k: round(v, 1) for k, v in level.items()})} dB")

    mix = bed + fx
    raw = OUT / "_raw.wav"
    write(raw, mix)
    gain = undb(TARGET_LUFS - loudness(raw)[0])
    raw.unlink()
    for label, audio in (("mix-chaplin", mix), ("music_only", music)):
        pre = OUT / f"_{label}.wav"
        write(pre, audio * gain)
        subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(pre), "-af", f"alimiter=limit={undb(TRUE_PEAK_DB - 0.5):.4f}:attack=2:release=60:level=disabled",
                        "-ar", str(SR), str(OUT / f"{label}.wav")], check=True)
        pre.unlink()
        integrated, lra, peak = loudness(OUT / f"{label}.wav")
        report.append(f"{label}: I {integrated:.1f} LUFS  LRA {lra:.1f} LU  peak {peak:.1f} dBFS")
    (OUT / "mix-report.txt").write_text("\n".join(report) + "\n")
    print("\n".join(report))


if __name__ == "__main__":
    main()
