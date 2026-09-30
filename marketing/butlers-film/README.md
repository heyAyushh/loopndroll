# The Butlers Who Would Not Proceed

A 75-second deadpan Looper film in two aspect ratios (1920×1080 and 1080×1920). It was built with the
[motion-video-kit](https://github.com/echris6/motion-video-kit) `business-motion-film` workflow:
[BRIEF.md](BRIEF.md) → [STORYBOARD.md](STORYBOARD.md) → deterministic code-built motion → measured checks → fresh-critic review.

Every frame is code-drawn SVG/HTML. `window.seek(t)` in `film.js` sets the whole frame from `t` alone, so any frame
renders identically in any order.

## Rebuild

```sh
cd marketing/butlers-film
npm install                                   # playwright-core (drives the installed Google Chrome)
python3 tools/voices.py                       # narrator + friend lines via macOS `say`
python3 tools/fetch_sfx.py                    # Mixkit music + sound effects into build/
python3 tools/mix.py                          # build/audio/mix.wav and music_only.wav (+ mix-report.txt)
node render.js video 1920 1080 60 build/picture-landscape.mp4
node render.js video 1080 1920 60 build/picture-portrait.mp4
python3 tools/mux.py build/picture-landscape.mp4 build/audio/mix.wav build/looper-butlers-landscape.mp4
python3 tools/mux.py build/picture-portrait.mp4 build/audio/mix.wav build/looper-butlers-portrait.mp4
```

Preview stills: `node render.js stills 1920 1080 build/stills 0,16.3,27,53,72`.

## Credits and licences

- Music: "Secret Garden" by Eugenio Mininni, Mixkit Stock Music Free License.
- Sound effects: Mixkit Sound Effects Free License (IDs listed in `tools/fetch_sfx.py`).
- Voices: macOS `say` (Daniel, Samantha).
- Library audio is downloaded into `build/` and not committed.
