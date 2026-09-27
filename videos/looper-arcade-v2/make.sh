#!/usr/bin/env bash
# Build both cuts: pinball sim log -> soundtrack -> frames -> muxed MP4s in build/.
set -euo pipefail
cd "$(dirname "$0")"

node render.mjs --events
python3 synth.py
for format in landscape portrait; do
  node render.mjs --format "$format"
  ffmpeg -y -v error -i "build/$format-video.mp4" -i build/soundtrack.wav \
    -c:v copy -c:a aac -b:a 256k -shortest -movflags +faststart "build/looper-insert-coin-$format.mp4"
  echo "wrote build/looper-insert-coin-$format.mp4"
done
