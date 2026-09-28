# Looper: "The Unbroken Line" (90s sketchbook film)

- **Treatment:** `DIRECTION.md`. The same McKee story as `../looper-film`, told as a sketchbook. The pencil lifting
  is the Stop; Looper is one continuous lavender line that never lifts.
- **Drawings:** 24 plates in `plates/`: graphite noir, cyanotype blueprint and watercolour. They were generated with
  Codex image generation from `plates/prompts.py` plus the shared style in `plates/prompts/_style.txt`, with the model
  sheet and the establishing room attached as references so the person and the loft stay consistent.
  PNG masters are local only; the JPEG copies are committed.
- **Music:** `score.py`. An original track built from the "looper" playlist analysis (`assets/playlist-analysis.json`),
  with paper foley: pencil scratch, eraser, page flips, the snapping lead, and the press slam at the drop.

## Build

```bash
python3 score.py                        # assets/score.wav + assets/envelope.json
python3 tools/analyze_plates.py         # blank screen quads + continuous-line paths from the drawings
npm install
node render.mjs                         # renders/looper-sketchbook.mp4
node render.mjs --midpoints             # one review still per shot
plates/generate.sh <name> model-sheet.png room-night.png   # regenerate one drawing
```

- `film/shots.js`: the edit, page by page
- `film/pencil.js`: the pencil shader (draw-on, boil, fatigue, riso, paper)
- `film/page.js`: hand-drawn primitives and perspective text
- `film/ink.js`: Looper's line
