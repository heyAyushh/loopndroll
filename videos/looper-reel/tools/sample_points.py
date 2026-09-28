"""Turns each drawing's vertical crop into a cloud of points sampled from its light and its lines.

The points are how scenes turn into each other: a drawing disintegrates into its own light and
reassembles as the next drawing. Output: assets/points/<scene>.bin (float32 x, y, r, g, b per point,
x/y normalized to the 9:16 crop, top-left origin) and assets/crops.json (crop boxes, used by the film).

Run: python3 tools/sample_points.py
"""
import json
from pathlib import Path

import cv2
import numpy as np

ROOT = Path(__file__).resolve().parent.parent
PLATES = ROOT.parent / "looper-sketchbook" / "plates"
OUT = ROOT / "assets" / "points"
POINTS = 42000

# scene -> (plate, crop centre x in the plate, 0..1)
CROPS = {
    "eye": ("eye-night", 0.47), "room": ("room-night", 0.19), "monitor": ("monitor-cu", 0.5),
    "hands": ("hands-keyboard", 0.6), "phone": ("phone-desk", 0.58), "rub": ("rub-eyes", 0.42),
    "neighbour": ("neighbour-window", 0.4), "ots": ("ots-monitor", 0.4), "asleep": ("asleep-desk", 0.35),
    "wake": ("wake-phone", 0.45), "look": ("look-orb", 0.42), "stand": ("stand-up", 0.45),
    "walk": ("walk-window", 0.55), "couch": ("couch-phone", 0.55), "sleep": ("couch-sleep-wide", 0.78),
    "dawn": ("dawn-wide", 0.62), "stretch": ("stretch", 0.47), "window": ("window-silhouette", 0.55),
}


def plate_path(name):
    png = PLATES / f"{name}.png"
    return png if png.exists() else PLATES / f"{name}.jpg"


OUT.mkdir(parents=True, exist_ok=True)
crops = {}
rng = np.random.default_rng(7)
for scene, (plate, centre) in CROPS.items():
    image = cv2.imread(str(plate_path(plate)))[:, :, ::-1].astype(np.float32) / 255
    height, width, _ = image.shape
    crop_width = int(round(height * 9 / 16))
    x0 = int(np.clip(centre * width - crop_width / 2, 0, width - crop_width))
    crop = image[:, x0:x0 + crop_width]
    crops[scene] = {"plate": plate, "x": x0 / width, "w": crop_width / width}
    luminance = crop @ np.array([0.299, 0.587, 0.114], np.float32)
    gray = (luminance * 255).astype(np.uint8)
    edges = cv2.GaussianBlur(cv2.Canny(gray, 40, 120).astype(np.float32) / 255, (0, 0), 1.2)
    low, high = np.percentile(luminance, [5, 99.5])
    light = np.clip((luminance - low) / (high - low + 1e-6), 0, 1)
    saturation = crop.max(axis=2) - crop.min(axis=2)
    # Where the drawing's light is, plus its lines, plus any colour (lavender, dawn): that is its form.
    weight = light ** 2.2 + edges * 0.4 + saturation * 0.8
    weight /= weight.sum()
    index = rng.choice(weight.size, POINTS, p=weight.ravel())
    ys, xs = np.unravel_index(index, weight.shape)
    colour = np.clip(crop[ys, xs] * 0.55 + 0.45, 0, 1)
    points = np.stack([(xs + rng.random(POINTS)) / crop_width, (ys + rng.random(POINTS)) / height, *colour.T], axis=1).astype(np.float32)
    points.tofile(OUT / f"{scene}.bin")
    print(scene, plate, f"x0={x0}", points.shape)
(ROOT / "assets" / "crops.json").write_text(json.dumps(crops, indent=1))
