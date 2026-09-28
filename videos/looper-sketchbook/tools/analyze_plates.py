"""Reads the drawings and writes what the renderer needs to draw ON them.

1. Blank quads: the prompts ask for blank glowing screens, a blank note and a blank clock, so the
   film can write into them. Find those bright quads and store their corners (normalized, tl tr br bl).
2. Continuous line: chain a drawing's long contours into ONE path that never lifts, for the lavender
   line that redraws the world at the climax (the product idea: the loop never breaks).

Run: python3 tools/analyze_plates.py  -> plates/quads.json, plates/lines.json
"""
import json
from pathlib import Path

import cv2
import numpy as np

PLATES = Path(__file__).resolve().parent.parent / "plates"
QUADS = {"monitor-cu": 1, "ots-monitor": 4, "note-clock": 2, "phone-desk": 1, "couch-phone": 1}
LINE_ART = ["stand-up", "walk-window", "couch-sleep-wide", "room-night", "dawn-wide"]
MAX_LINE_POINTS = 5000
MIN_CONTOUR_PIXELS = 120


def order_corners(points):
    points = np.array(points, dtype=float)
    s, d = points.sum(axis=1), np.diff(points, axis=1).ravel()
    return [points[np.argmin(s)], points[np.argmin(d)], points[np.argmax(s)], points[np.argmax(d)]]


def bright_quads(image, count):
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    level = max(200, np.percentile(gray, 97))
    mask = cv2.morphologyEx((gray >= level).astype(np.uint8) * 255, cv2.MORPH_CLOSE, np.ones((9, 9), np.uint8))
    contours, _ = cv2.findContours(mask, cv2.RETR_EXTERNAL, cv2.CHAIN_APPROX_SIMPLE)
    height, width = gray.shape
    found = []
    for contour in sorted(contours, key=cv2.contourArea, reverse=True):
        if cv2.contourArea(contour) < width * height * 0.002:
            break
        approx = cv2.approxPolyDP(contour, 0.03 * cv2.arcLength(contour, True), True)
        # Trust a four-corner fit only if it covers the blob; otherwise take the rotated bounding box.
        fits = len(approx) == 4 and cv2.contourArea(approx) >= 0.93 * cv2.contourArea(cv2.convexHull(contour))
        corners = approx.reshape(-1, 2) if fits else cv2.boxPoints(cv2.minAreaRect(cv2.convexHull(contour)))
        found.append([[round(x / width, 4), round(y / height, 4)] for x, y in order_corners(corners)])
        if len(found) == count:
            break
    return found


def continuous_line(image):
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    gray = cv2.bilateralFilter(gray, 9, 40, 9)
    edges = cv2.Canny(gray, 60, 150)
    contours, _ = cv2.findContours(edges, cv2.RETR_LIST, cv2.CHAIN_APPROX_NONE)
    strokes = [cv2.approxPolyDP(c, 1.6, False).reshape(-1, 2).astype(float) for c in contours if len(c) >= MIN_CONTOUR_PIXELS]
    strokes = [s for s in strokes if len(s) >= 3]
    strokes.sort(key=len, reverse=True)
    strokes = strokes[:400]
    # Chain: the pen always travels to the nearest unused stroke end (either end), so it never lifts.
    ends = np.array([[s[0], s[-1]] for s in strokes])  # (n, 2, 2)
    used = np.zeros(len(strokes), bool)
    path = [strokes[0]]; used[0] = True; pen = strokes[0][-1]
    for _ in range(len(strokes) - 1):
        distances = np.linalg.norm(ends - pen, axis=2)
        distances[used] = np.inf
        index, which_end = np.unravel_index(np.argmin(distances), distances.shape)
        stroke = strokes[index] if which_end == 0 else strokes[index][::-1]
        used[index] = True
        path.append(stroke)
        pen = stroke[-1]
    points = np.vstack(path)
    step = max(1, len(points) // MAX_LINE_POINTS)
    points = points[::step]
    # Soften the pixel stair-steps so the line reads as a confident nib, not an edge detector.
    kernel = np.ones(3) / 3
    points = np.stack([np.convolve(points[:, i], kernel, mode='same') for i in range(2)], axis=1)
    height, width = gray.shape
    return [[round(x / width, 4), round(y / height, 4)] for x, y in points]


def plate_path(name):
    """PNG master if present locally, otherwise the committed JPEG copy."""
    png = PLATES / f"{name}.png"
    return png if png.exists() else PLATES / f"{name}.jpg"


quads, lines = {}, {}
for name, count in QUADS.items():
    path = plate_path(name)
    if path.exists():
        quads[name] = bright_quads(cv2.imread(str(path)), count)
        print(name, len(quads[name]), "quads")
for name in LINE_ART:
    path = plate_path(name)
    if path.exists():
        lines[name] = continuous_line(cv2.imread(str(path)))
        print(name, len(lines[name]), "line points")
(PLATES / "quads.json").write_text(json.dumps(quads))
(PLATES / "lines.json").write_text(json.dumps(lines))
