// The Reel's story, on the score's grid (score.py uses the same bars). 19 bars at 128 BPM = 35.6s.
export const FPS = 30;
export const BEAT = 60 / 128;
export const BAR = BEAT * 4;
export const DURATION = 19 * BAR;
export const at = (bar, beat = 0) => bar * BAR + beat * BEAT;
export const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
export const lerp = (a, b, k) => a + (b - a) * k;
export const smooth = (a, b, x) => { const k = clamp((x - a) / (b - a)); return k * k * (3 - 2 * k); };
export const easeOut = (k) => 1 - Math.pow(1 - clamp(k), 3);
export const easeInOut = (k) => { k = clamp(k); return k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2; };

// Babysitting: every half bar they type "y", the agent runs one beat, and stops again.
export const STUTTERS = Array.from({ length: 8 }, (_, k) => 3 + k * 0.5);
const STUTTER_SCENES = ["hands", "phone", "rub", "eye", "neighbour", "hands", "ots", "rub"];
const STUTTER_TIMES = ["03:10", "03:31", "03:52", "04:14", "04:36", "04:58", "05:17", "05:33"];

// [scene, startBar, endBar, options]. `morph` = seconds the particles take to become this scene.
// zoom: [from, to]; focus: [x, y] in the crop; fatigue 0..1; ink: particle colour for this scene.
export const SCENES = [
  ["eye", 0, 1, { morph: 0.45, from: "burst", zoom: [1.35, 1.05], focus: [0.5, 0.55] }],
  ["room", 1, 1.75, { morph: 0.3, zoom: [1.0, 1.12], focus: [0.45, 0.45] }],
  ["monitor", 1.75, 3, { morph: BEAT * 1.2, zoom: [1.05, 1.18], focus: [0.5, 0.4], stopMid: true }],
  ...STUTTERS.map((bar, k) => [STUTTER_SCENES[k], bar, bar + 0.5, { morph: BEAT * 0.7, zoom: [1.05 + k * 0.03, 1.15 + k * 0.03], focus: [0.5, 0.45], fatigue: 0.1 + k * 0.1, stutter: STUTTER_TIMES[k] }]),
  ["asleep", 7, 8, { morph: 0.35, zoom: [1.0, 1.1], focus: [0.5, 0.5], fatigue: 0.9, dust: true }],
  ["wake", 8, 8.75, { morph: 0.12, zoom: [1.12, 1.02], focus: [0.5, 0.5], fatigue: 0.4, shake: true }],
  ["look", 8.75, 9.5, { morph: 0.3, zoom: [1.0, 1.12], focus: [0.42, 0.62], fatigue: 0.2 }],
  ["dark", 9.5, 10, { morph: 0.25 }],
  ["ring", 10, 11, { morph: 0.45, ink: "lavender" }],
  ["stand", 11, 12, { morph: 0.5, zoom: [1.1, 1.0], focus: [0.5, 0.5], ink: "lavender" }],
  ["walk", 12, 13, { morph: 0.4, zoom: [1.0, 1.1], focus: [0.5, 0.5], ink: "lavender" }],
  ["monitor", 13, 13.5, { morph: 0.3, zoom: [1.1, 1.18], focus: [0.5, 0.4], ink: "lavender", resumed: true }],
  ["couch", 13.5, 14.5, { morph: 0.35, zoom: [1.0, 1.12], focus: [0.55, 0.3], ink: "lavender" }],
  ["sleep", 14.5, 15, { morph: 0.35, zoom: [1.0, 1.06], focus: [0.5, 0.5], ink: "lavender" }],
  ["dawn", 15, 16, { morph: 0.5, zoom: [1.0, 1.1], focus: [0.5, 0.4], ink: "warm" }],
  ["stretch", 16, 16.5, { morph: 0.3, zoom: [1.0, 1.06], focus: [0.5, 0.4], ink: "warm" }],
  ["window", 16.5, 17, { morph: 0.3, zoom: [1.0, 1.05], focus: [0.5, 0.45], ink: "warm" }],
  ["ring", 17, 19, { morph: 0.6, ink: "lavender", end: true }],
];

// Freezes: the world stops mid-flight. The first Stop freezes a transition in the air.
export const FREEZES = [[at(2), at(2.25)], ...STUTTERS.map((bar) => [at(bar + 0.25), at(bar + 0.5)])];

// Captions: [startBar, endBar, text, style]
export const CAPTIONS = [
  [0.05, 1.0, "2:47 AM.", "hero"],
  [0.45, 1.0, "Launch is at 9.", "sub"],
  [1.05, 1.95, "Your agents are shipping it.", "hero"],
  [2.0, 3.0, "Then they stop.", "hero"],
  ...STUTTERS.map((bar, k) => [bar, bar + 0.5, STUTTER_TIMES[k], "clock"]),
  [3.0, 7.0, "continue? y", "machine"],
  [7.05, 8.0, "You became the loop.", "hero"],
  [8.0, 8.75, "5:58 AM.", "hero"],
  [8.8, 9.5, "Keep babysitting?", "hero"],
  [9.55, 10.0, "Or…", "hero"],
  [10.0, 11.0, "Looper.", "brand"],
  [11.05, 12.0, "Leave the desk.", "hero"],
  [12.05, 13.0, "Your agents keep going.", "hero"],
  [13.0, 13.5, "Until the checks pass ✓", "hero"],
  [13.55, 14.5, "It asks only what matters.", "hero"],
  [14.55, 15.0, "You sleep.", "hero"],
  [15.05, 16.2, "8:52 AM. Shipped.", "hero"],
];

export function prng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}
