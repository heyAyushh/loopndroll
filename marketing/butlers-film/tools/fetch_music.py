"""Download Mixkit candidate tracks and print duration + loudness summary."""
import subprocess
import sys
import urllib.request
from pathlib import Path

CANDIDATE_IDS = sys.argv[2:] or ["595", "613", "2", "506", "10", "9", "689"]
OUT_DIR = Path(sys.argv[1])
OUT_DIR.mkdir(parents=True, exist_ok=True)

for track_id in CANDIDATE_IDS:
    path = OUT_DIR / f"{track_id}.mp3"
    if not path.exists():
        urllib.request.urlretrieve(f"https://assets.mixkit.co/music/{track_id}/{track_id}.mp3", path)
    duration = subprocess.run(
        ["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)],
        capture_output=True, text=True).stdout.strip()
    ebur = subprocess.run(["ffmpeg", "-hide_banner", "-i", str(path), "-af", "ebur128", "-f", "null", "-"],
                          capture_output=True, text=True).stderr
    summary = [line.strip() for line in ebur.splitlines() if line.strip().startswith(("I:", "LRA:"))]
    print(track_id, f"dur={float(duration):.1f}s", " ".join(summary))
