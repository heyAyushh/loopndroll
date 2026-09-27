// Single source of truth for beat-synced moments (seconds). Read by index.html
// and by synth.py (which strips the assignment and parses the JSON body), so
// every sound effect lands on the frame its visual happens.
window.T = {
  "bpm": 120,
  "total": 48,
  "scenes": { "boot": [0, 6], "pinball": [6, 20], "palace": [20, 30], "race": [30, 40], "finale": [40, 48] },
  "boot": { "crtOn": 0.1, "desktop": 1.0, "click1": 2.6, "click2": 2.8, "window": 3.0, "loadEnd": 4.9, "coin": 5.4, "zoom": 5.7 },
  "pinball": {
    "launch": 7.0,
    "bumperHits": [[7.5, 0], [8.0, 1], [8.5, 2], [9.0, 0]],
    "flipLeft": [9.75, 16.0, 17.0],
    "flipRight": [13.25, 16.5, 17.5],
    "targets": [10.25, 10.375, 10.5, 10.625, 10.75],
    "loopRamp": [11.25, 12.25],
    "deployHit": 12.75,
    "drain": 14.25,
    "save": 14.75,
    "kickback": 15.25,
    "multiHits": [[15.75, 1], [16.25, 2], [16.75, 0], [17.25, 3], [17.75, 1], [18.0, 2]],
    "jackpot": 18.5,
    "wipe": 19.5
  },
  "palace": {
    "title": 20.1,
    "run": 20.75,
    "jump1": 22.0,
    "land1": 22.5,
    "stop": 23.0,
    "stopped": 23.25,
    "orb": 24.0,
    "cont": 24.5,
    "leap": 25.0,
    "land2": 25.75,
    "chomps": [26.0, 26.75],
    "pass": 26.4,
    "gate": 27.5,
    "clear": 28.25,
    "wipe": 29.5
  },
  "race": {
    "beeps": [30.5, 30.83, 31.17],
    "go": 31.5,
    "passes": [32.25, 33.25, 34.25],
    "nitro": 35.0,
    "fastPasses": [36.0, 36.5, 37.0, 37.5],
    "finish": 38.5,
    "wipe": 39.5
  },
  "finale": {
    "rows": [40.25, 40.5, 40.75, 41.0, 41.25],
    "newHigh": 41.75,
    "logo": 43.0,
    "wordmark": 43.5,
    "tagline": 44.5,
    "sub": 45.5,
    "press": 46.5
  }
};
