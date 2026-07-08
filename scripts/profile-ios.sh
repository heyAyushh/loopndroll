#!/bin/bash
# Profile the Looper iOS app running in a SIMULATOR using macOS `sample`
# (simulator apps are host processes). Chosen over `xctrace record`, which
# wedges on attach with the current Xcode beta and can't be stopped cleanly.
#
# Usage:
#   scripts/profile-ios.sh [seconds] [out-file]
#
# Start the interaction you want profiled right after launching this script
# (e.g. tap the search button). Output is a text call tree; the
# "Sort by top of stack" section at the bottom is the hotspot list, and the
# per-thread trees show exactly what the main thread is doing.
#
# Physical device: `sample` cannot reach iOS processes - use Instruments GUI
# (Time Profiler template) against the device instead.
set -euo pipefail

SECONDS_TO_SAMPLE="${1:-10}"
OUT_FILE="${2:-/tmp/looper-sample-$(date +%H%M%S).txt}"

PID=$(pgrep -x Looper | head -1)
if [[ -z "$PID" ]]; then
    echo "error: no running Looper simulator process found" >&2
    exit 1
fi

echo "==> sampling Looper (pid $PID) for ${SECONDS_TO_SAMPLE}s"
sample "$PID" "$SECONDS_TO_SAMPLE" -f "$OUT_FILE" >/dev/null

echo "==> main thread tree (first 40 lines):"
awk '/^    Thread .*com\.apple\.main-thread/{f=1} f{print; n++} n>=40{exit}' "$OUT_FILE"
echo ""
echo "==> hotspots:"
sed -n '/Sort by top of stack/,/^$/p' "$OUT_FILE" | head -20
echo ""
echo "Full call tree: $OUT_FILE"
