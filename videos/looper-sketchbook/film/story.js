// The story clock. Everything on screen is a pure function of time t (seconds), on the score's grid.
export const FPS = 30;
export const BPM = 128;
export const BEAT = 60 / BPM;
export const BAR = BEAT * 4;
export const TOTAL_BARS = 48;
export const DURATION = TOTAL_BARS * BAR; // 90s
export const at = (bar, beat = 0) => bar * BAR + beat * BEAT;

export const clamp = (value, low = 0, high = 1) => Math.min(high, Math.max(low, value));
export const lerp = (a, b, k) => a + (b - a) * k;
export const smooth = (edge0, edge1, x) => {
  const k = clamp((x - edge0) / (edge1 - edge0));
  return k * k * (3 - 2 * k);
};
export const easeInOut = (k) => (k < 0.5 ? 4 * k * k * k : 1 - Math.pow(-2 * k + 2, 3) / 2);
export const easeOut = (k) => 1 - Math.pow(1 - k, 3);
export const easeIn = (k) => k * k * k;
export const between = (t, startBar, endBar) => t >= at(startBar) && t < at(endBar);

// Story beats (bars). Mirrors GROOVE_WINDOWS / TAPE_STOP_BEATS in score.py.
export const BEATS = {
  firstStop: at(8),
  rubEyes: at(10),
  firstYes: at(10, 2.4),
  stops: [at(12), at(13), at(14), at(15)],
  yeses: [at(12, 1), at(13, 1), at(14, 1)],
  asleep: at(15, 2),
  rainStops: at(19),
  buzz: at(24),
  hesitate: at(26, 2),
  habitYes: at(26, 2.6),
  futileStop: at(27, 1),
  lookWindow: at(28, 2),
  lookOrb: at(29, 2),
  pointer: at(30),
  menuOpen: at(31, 0.5),
  click: at(32),
  standUp: at(33, 0.2),
  walkStart: at(35),
  onCouch: at(36),
  question: at(37, 0.5),
  reply: at(37, 1.5),
  asleepCouch: at(38, 2),
  timelapseEnd: at(39),
  dawn: at(40),
  wake: at(41, 2),
  toWindow: at(42, 1),
  endCard: at(44),
};

// When the agents are actually moving (the groove windows in score.py).
const RUNNING = [[0, 8], [11, 12], [12.5, 13], [13.5, 14], [14.5, 15], [27, 27.25], [32, 48]];
export const agentsRunning = (t) => RUNNING.some(([a, b]) => t >= at(a) && t < at(b));
export const looperOn = (t) => t >= BEATS.click;
export const stopCount = (t) => [BEATS.firstStop, ...BEATS.stops].filter((s) => t >= s).length;

// Wall-clock minutes since midnight. It crawls in real time, then races in the two time-lapses.
const CLOCK = [[0, 151], [8, 167], [12, 190], [13, 220], [14, 245], [15, 262], [19, 280], [21, 358], [24, 372], [32, 375], [38, 380], [39, 520], [40, 532], [42, 535], [48, 537]];
export function clockMinutes(t) {
  const bar = t / BAR;
  for (let i = 1; i < CLOCK.length; i += 1) {
    const [b0, m0] = CLOCK[i - 1];
    const [b1, m1] = CLOCK[i];
    if (bar < b1) return Math.floor(lerp(m0, m1, (bar - b0) / (b1 - b0)));
  }
  return CLOCK[CLOCK.length - 1][1];
}
export const clockText = (minutes) => `${String(Math.floor(minutes / 60) % 24).padStart(2, "0")}:${String(minutes % 60).padStart(2, "0")}`;

// Deterministic PRNG for set dressing.
export function prng(seed) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let x = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    x = (x + Math.imul(x ^ (x >>> 7), 61 | x)) ^ x;
    return ((x ^ (x >>> 14)) >>> 0) / 4294967296;
  };
}
