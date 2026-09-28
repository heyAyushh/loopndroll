// Time for the wild cut. Same bars as score.py (19 bars at 128 BPM = 35.6s). Nothing here is ever still.
export const FPS = 30;
export const BEAT = 60 / 128;
export const BAR = BEAT * 4;
export const DURATION = 19 * BAR;
export const W = 1080, H = 1920;
export const at = (bar, beat = 0) => bar * BAR + beat * BEAT;
export const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
export const lerp = (a, b, k) => a + (b - a) * k;
export const smooth = (a, b, x) => { const k = clamp((x - a) / (b - a)); return k * k * (3 - 2 * k); };
export const easeOut = (k) => 1 - Math.pow(1 - clamp(k), 3);
export const easeInOut = (k) => { k = clamp(k); return k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2; };
export const between = (t, a, b) => t >= at(a) && t < at(b);

export const STUTTERS = Array.from({ length: 8 }, (_, k) => 3 + k * 0.5);
export const STUTTER_TIMES = ["3:10", "3:31", "3:52", "4:14", "4:36", "4:58", "5:17", "5:33"];

// The agents' work runs only in these windows (and the tape-stop decelerates into each end).
const WORK = [[0, 2, 1], ...STUTTERS.map((b) => [b, b + 0.25, 0.25]), [10, 19, 0]];
export function workTime(t) {
  let total = 0;
  for (const [a, b, stopBeats] of WORK) {
    const start = at(a), end = at(b), upTo = Math.min(t, end);
    if (upTo <= start) continue;
    const ease = stopBeats * BEAT;
    const steps = 16;
    for (let i = 0; i < steps; i += 1) {
      const s = start + (i + 0.5) * (upTo - start) / steps;
      const speed = ease ? Math.pow(clamp((end - s) / ease), 1.4) : 1;
      total += speed * (upTo - start) / steps;
    }
  }
  return total;
}
export const working = (t) => WORK.some(([a, b]) => t >= at(a) && t < at(b));
export const stopsSoFar = (t) => [2, ...STUTTERS.map((b) => b + 0.25)].filter((b) => t >= at(b)).length;

export function prng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}
