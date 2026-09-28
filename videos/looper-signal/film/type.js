// Type: few words, placed once, never moving after they land. Fade up with a de-blur, fade out softly.
import { BAR, BEAT, BEATS, at, clamp, easeOut, smooth } from "./story.js";

const DISPLAY = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';
const MONO = '"SF Mono", Menlo, monospace';
const WHITE = "#f5f5f7";
const GREY = "#a1a1a6";
const IN = 0.55;
const OUT = 0.4;

// [startBar, endBar, text, options]
const LINES = [
  [3, 6.6, "Your agents are working.", {}],
  [8.25, 9.6, "Then they stop.", {}],
  [14.35, 15.85, "You become the loop.", {}],
  [20.75, 22.45, "Looper keeps them going.", {}],
  [22.8, 24.5, "Same session. Until the checks pass.", {}],
  [24.85, 26.9, "Even while you sleep.", {}],
];
const SENTENCE = [["Every", 11], ["few", 12.5], ["minutes.", 13.5]]; // one word lands on each freeze
const SENTENCE_END = 14.15;

function envelope(t, start, end) {
  const fadeIn = easeOut((t - start) / IN);
  const fadeOut = 1 - smooth(end - OUT, end, t);
  return clamp(Math.min(fadeIn, fadeOut));
}

function write(g, text, x, y, { size = 84, weight = 600, color = WHITE, alpha = 1, font = DISPLAY, align = "center", tracking = -0.5 } = {}) {
  if (alpha <= 0.001) return;
  g.save();
  g.globalAlpha = alpha;
  g.filter = alpha < 1 ? `blur(${(1 - alpha) * 10}px)` : "none";
  g.font = `${weight} ${size}px ${font}`;
  if ("letterSpacing" in g) g.letterSpacing = `${tracking}px`;
  g.textAlign = align; g.textBaseline = "middle"; g.fillStyle = color;
  g.fillText(text, x, y + (1 - alpha) * 14);
  g.restore();
}

export function drawType(g, t, width, height) {
  const cx = width / 2, baseline = height * 0.8;
  for (const [a, b, text] of LINES) write(g, text, cx, baseline, { alpha: envelope(t, at(a), at(b)) });

  // "Every few minutes." — laid out once as a whole, each word revealed in place on its freeze.
  g.save();
  g.font = `600 84px ${DISPLAY}`;
  const words = SENTENCE.map(([word]) => word);
  const widths = words.map((word) => g.measureText(word).width);
  const space = g.measureText(" ").width;
  const total = widths.reduce((s, w) => s + w, 0) + space * (words.length - 1);
  g.restore();
  let x = cx - total / 2;
  SENTENCE.forEach(([word, bar], i) => {
    write(g, word, x + widths[i] / 2, baseline, { alpha: envelope(t, at(bar), at(SENTENCE_END)) });
    x += widths[i] + space;
  });

  // The cursor: the Stop made visible, blinking in the frozen field. "continue? y" is the only machine voice.
  const frozen = [[8.05, 10.0], [11.0, 11.75], [12.5, 13.0], [13.5, 14.2]];
  const inFreeze = frozen.find(([a, b]) => t >= at(a) && t < at(b));
  if (inFreeze) {
    const blink = Math.floor(t / (BEAT / 2)) % 2 === 0;
    const typed = BEATS.typedYes.find((yes) => t >= yes && t < yes + BEAT);
    const label = typed ? "continue? y" : "continue?";
    g.save();
    g.font = `400 44px ${MONO}`;
    const labelWidth = g.measureText(label).width;
    g.restore();
    const lx = cx - (labelWidth + 26) / 2;
    write(g, label, lx, height * 0.5, { size: 44, weight: 400, color: GREY, font: MONO, align: "left", tracking: 0, alpha: clamp((t - at(inFreeze[0])) / 0.2) });
    if (blink) { g.save(); g.fillStyle = WHITE; g.fillRect(lx + labelWidth + 12, height * 0.5 - 23, 23, 46); g.restore(); }
  }

  // End: the name, the promise, the address.
  const end = t - BEATS.end;
  if (end > 0) {
    write(g, "Looper", cx, height * 0.72, { size: 112, weight: 700, alpha: easeOut((end - BAR * 1.35) / 0.8), tracking: -1.5 });
    write(g, "Leave the desk. Keep the loop.", cx, height * 0.81, { size: 44, weight: 500, color: GREY, alpha: easeOut((end - BAR * 1.8) / 0.8) });
    write(g, "looper.fyi", cx, height * 0.88, { size: 30, weight: 500, color: GREY, alpha: easeOut((end - BAR * 2.3) / 0.8), tracking: 0.5 });
  }
}
