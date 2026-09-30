"""Mux a silent picture render with the mix: video stream copied untouched, AAC 256k (audio.md, Mix & master).

Usage: python3 tools/mux.py <picture.mp4> <audio.wav> <out.mp4>
"""
import subprocess
import sys

picture, audio, out = sys.argv[1:4]
subprocess.run(["ffmpeg", "-v", "error", "-y", "-i", picture, "-i", audio, "-map", "0:v:0", "-map", "1:a:0",
                "-c:v", "copy", "-c:a", "aac", "-b:a", "256k", "-shortest", "-movflags", "+faststart", out], check=True)
print(out)
