# Looper launch film — "One Night, One Loop" (75s)

## The music came first

`assets/playlist-analysis.json` measures the 38 tracks of the "looper" Apple Music playlist from their
30-second previews:

| Measure | Playlist | Score (`score.py`) |
| --- | --- | --- |
| Tempo | cluster 124.5–129.2 BPM, median 127.6 | 128 BPM (measures 127.6) |
| Tonal centre | D minor / F major, A minor / C | D minor → F major at dawn |
| Energy below 100 Hz | 81% median (57–93%) | 93% |
| Spectral centroid | 1.6 kHz median (0.3–3.1 kHz) | 0.95 kHz |
| Bar-to-bar similarity | 0.90 | 0.85 |

The music is loud, dark, and made of the low end, and it barely changes from bar to bar. So the film is a
loop too: one room and one fixed camera across five 8-bar phrases. The frame hardly changes, but each
loop turns the story's value. The room's light follows the score's sub-bass envelope
(`assets/envelope.json`).

## Story (McKee)

- **Protagonist and desire:** a developer who wants tonight's work to ship and wants to sleep.
- **Antagonism:** every agent stops and waits for a human.
- **Controlling idea:** the work survives the night when you stop being the loop.

| Loop | Bars | Beat | Value |
| --- | --- | --- | --- |
| 1 · Setup | 0–5 | 02:40. At the desk, the agent writes code. The orb sits dark on the desk. | + |
| Inciting incident | 6–7 | 02:47. The score tape-stops. Insert: "agent stopped. continue? [y/N]" | − |
| 2 · Complications | 8–15 | They wait, then give up and lie down. The sessions pile up in the sidebar; the idle timers grow. | − − |
| 3 · Escalation | 16–22 | The pressure comes back and the phone keeps buzzing. The night runs out. | − − − |
| Crisis | 23 | 05:10. Insert: the phone. "4 sessions stopped." Get up and babysit, or lose the night? | dilemma |
| 4 · Climax | 24–31 | They choose: "keep going until the checks pass." Send. The drop hits, the orb wakes, the loop is thrown onto the wall, and the sessions run. They sleep. | + + |
| 5 · Resolution | 32–35 | Dawn, 07:02. "✓ all checks passed." They sit up. | + |
| End card | 36–39 | LOOPER — "Keeps your agents moving until the work is actually done." | |

## Build

- `python3 score.py` writes `assets/score.wav` and `assets/envelope.json`.
- `npm install && node render.mjs` renders `renders/looper-launch.mp4`: `film.html` draws each frame on a
  canvas in headless Chrome, and the frames are piped into ffmpeg.
- `node render.mjs --stills 12.5,44.6` writes review frames.
