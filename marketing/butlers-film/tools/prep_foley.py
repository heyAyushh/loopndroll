"""Download Mixkit Foley and cut single-hit variants (footsteps) so repeated steps don't sound machine-gunned.

Writes build/foley/<name>.wav and build/foley/<kind>_<n>.wav step variants. Mixkit Sound Effects Free License.
"""
import subprocess
import urllib.request
from pathlib import Path

import librosa
import numpy as np
import soundfile as sf

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "build" / "foley"
SR = 48000
SOURCES = {
    "heels": 542,        # Footsteps on heels on the pavement -> butler shoes
    "soft": 534,         # Footsteps on mattress loop -> the friend's sandals
    "run": 1274,         # Crunchy road fast walking loop -> the developer's dash
    "creak": 1163,       # Door creak opened by the wind -> guest door
    "cloth": 1898,       # Cloth slide out -> bows, phone raise
    "squeak": 1016,      # Chair seat wood squeak -> chair spin, standing up
    "slurp": 3207,       # Soft slurp -> coconut
    "clink": 1855,       # Cutlery placement -> tray clink as a card lifts off
    "projector": 1441,   # Vintage film projector working -> bed under the whole film
    "projector_ends": 3013,  # Bell & Howell start/stop -> first and last frame
    "whistle_up": 611,   # Swoosh whistle -> whips, cards taking off (the trap drummer's slide whistle)
    "whistle_fall": 406, # Short whistle fall -> cards diving into the phone
    "toy_whistle": 616,  # Cartoon toy whistle -> the heel-click
    "rimshot": 578,      # Joke drums -> "Absolutely not."
}
STEP_VARIANTS = 6
STEP_SECONDS = 0.22


def fetch(name, sfx_id):
    mp3 = OUT / f"{name}.mp3"
    if not mp3.exists():
        request = urllib.request.Request(f"https://assets.mixkit.co/active_storage/sfx/{sfx_id}/{sfx_id}-preview.mp3", headers={"User-Agent": "Mozilla/5.0"})
        mp3.write_bytes(urllib.request.urlopen(request).read())
    wav = OUT / f"{name}.wav"
    # trim leading silence so the sound lands on its cue, then peak-normalise like the step variants
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", str(mp3), "-ac", "1", "-ar", str(SR), "-af",
                    "highpass=f=90,lowpass=f=10000,silenceremove=start_periods=1:start_threshold=-40dB,dynaudnorm=f=500:g=3,alimiter=limit=0.8:level=disabled",
                    str(wav)], check=True)
    audio, _ = sf.read(wav)
    sf.write(wav, audio / (np.abs(audio).max() + 1e-9) * 0.8, SR)
    return wav


def cut_steps(name, wav):
    audio, _ = librosa.load(str(wav), sr=SR, mono=True)
    onsets = librosa.onset.onset_detect(y=audio, sr=SR, units="samples", backtrack=True, delta=0.12)
    peaks = sorted(onsets, key=lambda i: -np.abs(audio[i:i + int(0.05 * SR)]).max())[:STEP_VARIANTS * 2]
    chosen = sorted(peaks)[:STEP_VARIANTS]
    length = int(STEP_SECONDS * SR)
    fade = np.linspace(1, 0, int(0.06 * SR)) ** 2
    for n, start in enumerate(chosen):
        clip = audio[start:start + length].copy()
        clip[:int(0.003 * SR)] *= np.linspace(0, 1, int(0.003 * SR))
        clip[-len(fade):] *= fade
        sf.write(OUT / f"{name}_{n}.wav", clip / (np.abs(clip).max() + 1e-9) * 0.8, SR)
    print(name, "steps", len(chosen))


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for name, sfx_id in SOURCES.items():
        wav = fetch(name, sfx_id)
        if name in ("heels", "soft", "run"):
            cut_steps(name, wav)
    print("foley ready")


if __name__ == "__main__":
    main()
