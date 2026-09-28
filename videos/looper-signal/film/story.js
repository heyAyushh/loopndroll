// Time. Everything is a pure function of t, on the score's grid (score.py uses the same bars).
export const FPS = 30;
export const BEAT = 60 / 128;
export const BAR = BEAT * 4;
export const DURATION = 32 * BAR; // 60s
export const at = (bar, beat = 0) => bar * BAR + beat * BEAT;

export const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
export const lerp = (a, b, k) => a + (b - a) * k;
export const smooth = (a, b, x) => { const k = clamp((x - a) / (b - a)); return k * k * (3 - 2 * k); };
export const easeOut = (k) => 1 - Math.pow(1 - clamp(k), 3);
export const easeInOut = (k) => { k = clamp(k); return k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2; };

// When the agents are working. Work-time only advances inside these windows, so a Stop is a true freeze.
const WORK = [[0.25, 7.75], [10, 11], [11.75, 12.5], [13, 13.5]];
const STOP_EASE = { 7.75: BEAT, 11: BEAT / 2, 12.5: BEAT / 2, 13.5: BEAT / 2 }; // tape-stop decelerations

// Integrated work-time: speed 1 while working, decelerating into each stop, spinning up after each "y".
export function workTime(t) {
  let total = 0;
  for (const [a, b] of WORK) {
    const start = at(a), end = at(b);
    const ease = STOP_EASE[b] ?? 0;
    const spinUp = a === 0.25 ? 0 : BEAT / 2;
    const upTo = Math.min(t, end);
    if (upTo <= start) continue;
    // speed(s): ramps 0->1 over spinUp, 1 in the middle, (1 - x)^1.5 over the final `ease` seconds.
    const steps = 24;
    const span = upTo - start;
    for (let i = 0; i < steps; i += 1) {
      const s = start + (i + 0.5) * span / steps;
      const up = spinUp ? clamp((s - start) / spinUp) : 1;
      const down = ease ? Math.pow(clamp((end - s) / ease), 1.5) : 1;
      total += Math.min(up, down) * span / steps;
    }
  }
  return total;
}
export const working = (t) => WORK.some(([a, b]) => t >= at(a) && t < at(b));

export const BEATS = {
  firstLight: at(0, 1),
  firstStop: at(7.75),
  typedYes: [at(9.75), at(11.5), at(12.75)],
  freezes: [at(11), at(12.5), at(13.5)],
  fall: at(14),
  crisis: at(16),
  reveal: at(17.5),
  drop: at(20),
  end: at(28),
};

// Deterministic randomness: the film's randomness is chosen, and the same every render.
export function prng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}
