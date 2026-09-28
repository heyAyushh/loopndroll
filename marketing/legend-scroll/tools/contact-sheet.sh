#!/bin/bash
# Tiles review stills (sorted by time) into 3x3 contact sheets: tools/contact-sheet.sh out/stills out/sheet
set -euo pipefail
stills="$1"
prefix="$2"
ffmpeg -y -loglevel error -pattern_type glob -i "$stills/still-*.png" -vf "scale=640:360,tile=3x3" -frames:v 20 "$prefix-%d.png"
ls "$stills" | tr '\n' ' '
echo
