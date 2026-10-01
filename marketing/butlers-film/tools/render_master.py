"""Resumable master render: 15 s chunks (skipped if already rendered), then a lossless concat.

Usage: python3 tools/render_master.py <width> <height> <out.mp4> [fps=60]
A stall on a busy machine only costs one chunk; re-run the same command to resume.
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DURATION = int(round(json.loads((ROOT / "build" / "audio" / "timing.json").read_text())["duration"]))   # real film length
CHUNK_SECONDS = 15


def main():
    width, height, out = sys.argv[1], sys.argv[2], Path(sys.argv[3])
    fps = sys.argv[4] if len(sys.argv) > 4 else "60"
    chunk_dir = ROOT / "build" / "chunks" / f"{width}x{height}"
    chunk_dir.mkdir(parents=True, exist_ok=True)
    chunks = []
    for start in range(0, DURATION, CHUNK_SECONDS):
        end = min(start + CHUNK_SECONDS, DURATION)
        chunk = chunk_dir / f"{start:03d}-{end:03d}.mp4"
        chunks.append(chunk)
        if chunk.exists() and chunk.stat().st_size > 0:
            continue
        partial = chunk.with_suffix(".partial.mp4")
        subprocess.run(["node", str(ROOT / "render.js"), "video", width, height, fps, str(partial), str(start), str(end)], check=True)
        partial.rename(chunk)
        print(f"chunk {chunk.name} done", flush=True)
    listing = chunk_dir / "concat.txt"
    listing.write_text("".join(f"file '{c}'\n" for c in chunks))
    subprocess.run(["ffmpeg", "-v", "error", "-y", "-f", "concat", "-safe", "0", "-i", str(listing), "-c", "copy", "-movflags", "+faststart", str(out)], check=True)
    print(out)


if __name__ == "__main__":
    main()
