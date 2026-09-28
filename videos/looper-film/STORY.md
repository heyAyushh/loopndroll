# Looper launch film: "Keep the Loop" (90s)

- **Story, shot list and visual grammar:** `DIRECTION.md`
- **Music analysis:** `assets/playlist-analysis.json`. The 38 tracks of the "looper" Apple Music playlist, measured from their 30s previews:

| Measure | Playlist | Score (`score.py`) |
| --- | --- | --- |
| Tempo | cluster 124.5–129.2 BPM, median 127.6 | 128 BPM |
| Tonal centre | D minor / F major | D minor → F major at dawn |
| Energy below 100 Hz | 81% median | kick, rumble and sub carry the track |
| Spectral centroid | 1.6 kHz; <0.5% of energy above 5 kHz | dark; the high end (birdsong) arrives only at dawn |
| Bar-to-bar similarity | 0.90 | one riff, varied only by texture |

## Build

```bash
python3 score.py                      # assets/score.wav + assets/envelope.json (numpy/scipy)
npm install
node render.mjs                       # renders/looper-launch.mp4 (three.js in headless Chrome → ffmpeg)
node render.mjs --midpoints           # one review still per shot → renders/stills/
node render.mjs --from 58 --to 66     # a review segment → renders/segment.mp4
```

- `film/story.js`: beat grid, story beats, clock
- `film/shots.js`: the edit
- `film/room.js`, `film/character.js`, `film/machine.js`, `film/endcard.js`: the worlds
- `film/ui.js`: live screens
- `film/post.js`: the lens and the print
