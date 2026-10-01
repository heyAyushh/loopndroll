# The Butlers Who Would Not Proceed

An 82-second silent comedy for Looper in the manner of Charlie Chaplin, in two aspect ratios (1920×1080 and 1080×1920).
It was built with the [motion-video-kit](https://github.com/echris6/motion-video-kit) `business-motion-film` workflow:
[BRIEF.md](BRIEF.md) → [STORYBOARD.md](STORYBOARD.md) → deterministic code-built motion → measured checks → review
rounds ([LEDGER.md](LEDGER.md)).

Every frame is code-drawn SVG/HTML. `window.seek(t)` sets the whole frame from `t` alone, so any frame renders
identically in any order.

| File | Role |
| --- | --- |
| `rig.js` | Shared character rig: outlined, shaded limbs on hip/knee/shoulder/elbow joints; front/side/back views; walk/run cycles |
| `characters.js` | The cast (three butlers, the developer, the friend) and the question cards |
| `room.js` | The drawing room as a 3D set through one camera: dais, doors, desk, swivel chair, foreground, doorway clips |
| `film.js` | Scenes, camera, transitions, in story time |
| `silent.js` | Chaplin treatment: full-screen title cards that freeze story time, undercranked playback, a black-and-white print (grain, flicker, scratches, dust, gate weave) at 24 fps, colour arriving on the end card |

## Rebuild

```sh
cd marketing/butlers-film
npm install                                   # playwright-core (drives the installed Google Chrome)
python3 tools/fetch_sfx.py                    # Mixkit effects + the 1904 ragtime recording into build/
python3 tools/prep_foley.py                   # Mixkit Foley, projector, whistles, rimshot; single-step variants
node tools/dump_timing.js                     # real<->story time map + footsteps -> build/audio/timing.json
python3 tools/mix.py                          # build/audio/mix-chaplin.wav (+ music_only.wav, mix-report.txt)
python3 tools/render_master.py 1920 1080 build/picture-landscape.mp4 24   # resumable, 15 s chunks
python3 tools/render_master.py 1080 1920 build/picture-portrait.mp4 24
python3 tools/mux.py build/picture-landscape.mp4 build/audio/mix-chaplin.wav build/looper-butlers-landscape.mp4
python3 tools/mux.py build/picture-portrait.mp4 build/audio/mix-chaplin.wav build/looper-butlers-portrait.mp4
```

Preview stills: `node render.js stills 1920 1080 build/stills 0,5.5,30,60,80`.

## Credits and licences

- Score: Scott Joplin, "The Cascades" (1904), player-piano roll recording; Internet Archive item `1904Soundtrack`,
  Public Domain Mark 1.0.
- Sound effects and Foley: Mixkit Sound Effects Free License (IDs in `tools/fetch_sfx.py` and `tools/prep_foley.py`).
- No voices: it is a silent film.
- Library audio is downloaded into `build/` and not committed.
