# Looper: "Done means done"

A 50 s story film for Looper, at 1920×1080 and 60 fps. It follows the `business-motion-film` workflow from
[echris6/motion-video-kit](https://github.com/echris6/motion-video-kit): a brief, a storyboard critic,
deterministic code motion, measured checks and independent critic rounds. The story is built on
Blake Snyder's *Save the Cat!* beat sheet and Robert McKee's *Story*. See `BRIEF.md` for the brief, the
controlling idea and the storyboard.

Every frame is HTML/CSS/SVG in `film.html`. Each element is a pure function of `window.render(t)`, so
there's no wall-clock animation, and the film contains no AI footage.

## Build

```sh
npm install                         # playwright-core; uses the system Google Chrome
node capture.mjs stills 0 12.5 44   # spot-check frames -> build/stills/
./render-all.sh                     # 3000 frames with 4 workers -> build/picture.mp4
python3 mix.py build/picture.mp4 build/looper-done-means-done.mp4
python3 mix.py build/picture.mp4 build/looper-done-means-done-music-only.mp4 --no-sfx
```

`mix.py` edits the music to picture on its beat grid. It then places each effect on a real on-screen action,
with its gain solved in-band against the music, the 2–8 kHz lift capped at 4 dB and the peak capped at 6 dB, and
masters to −16 LUFS with a true peak of at most −1 dBFS. The per-event report goes to `build/mix-report.txt`.

## Audio credits

- Music: "Rising Sun" by Mixkit (track 892), under the Mixkit Stock Music Free License
- Sound effects by Mixkit (ids in `assets/sfx/`), under the Mixkit Sound Effects Free License

## Honesty

The end card and a persistent corner label read "Dramatization · UI simplified". The product behaviour shown
is real:
- The mode names, including Completion Checks, match the app.
- Checks re-run at every stop.
- The session is allowed to stop when the checks pass.
- The push title is "Session stopped", with the computer name as the subtitle and the agent's last message as the body.
- A reply from the iPhone goes into the same session.

The file counts, test counts and times are illustrative.
