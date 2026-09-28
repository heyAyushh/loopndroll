# ΛΟΟΠΕΡ · The Odyssey — 60-second trailer

A black-figure-pottery trailer for Looper: Odysseus leaves the laptop at home
with Penelope, runs the whole voyage from his phone, and every stall the sea
throws at him gets a `continue`.

Everything is code — no footage, no stock audio, no animation framework:

| File | Role |
| --- | --- |
| `trailer.js` | Canvas renderer. Every frame is a pure function of time. Scenes are painted on a panel, warped onto the curved amphora surface, and cut by turning the vase. |
| `audio.js` | Sample-by-sample score: shuttle clacks, Shepard tone, watch ticks, taiko hits, storm, heartbeat, chimes, wood knock. Seeded and deterministic. |
| `player.js` + `index.html` | Live preview, audio-clocked (space to play, scrub bar). |
| `render.mjs` | Headless Chrome → ordered JPEG frames → ffmpeg (x264 + AAC). Mixes in Penelope's line with macOS `say -v Whisper`. |

## Rules the picture obeys

- Only clay, glaze-black and bone-white — plus **thread-red** (the phone↔desk
  connection) and **running-green** (work in progress).
- Figures move on a 12 fps shadow-puppet clock; only the thread and the green
  running state move smoothly.
- Sound cues are derived from the picture timeline (`LooperTrailer.CUES`), so
  the score cannot drift from the edit.

## Run

```bash
cd marketing/odyssey-trailer
PUPPETEER_SKIP_DOWNLOAD=1 npm install
open index.html                 # live preview
npm run stills                  # review PNGs → out/stills/
npm run render                  # 1080p60 MP4 → out/looper-odyssey-trailer.mp4
npm run render:draft            # faster 30 fps draft
```

Requires Google Chrome (override with `CHROME_PATH`) and `ffmpeg` with libx264.
