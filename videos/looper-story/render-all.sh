#!/usr/bin/env bash
# Render every frame with parallel Chrome workers, then encode 1080p60.
set -euo pipefail
cd "$(dirname "$0")"
FPS=60; TOTAL=3000; WORKERS=4; CHUNK=$((TOTAL / WORKERS))
rm -rf build/frames; mkdir -p build/frames
for w in $(seq 0 $((WORKERS - 1))); do
  PORT=$((8800 + w)) node capture.mjs frames $((w * CHUNK)) $(((w + 1) * CHUNK)) &
done
wait
ffmpeg -loglevel error -y -framerate $FPS -i build/frames/f%05d.jpg -c:v libx264 -preset slow -crf 16 -pix_fmt yuv420p -movflags +faststart build/picture.mp4
echo "picture: build/picture.mp4"
