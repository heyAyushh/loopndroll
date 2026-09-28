// Reel type: big, bold, upper third (clear of Instagram's UI). Words pop in on the beat and blur out.
import { BAR, BEAT, CAPTIONS, at, clamp, easeOut, smooth } from "./story.js";

const DISPLAY = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';
const MONO = '"SF Mono", Menlo, monospace';
const STYLES = {
  hero: { size: 92, weight: 800, y: 0.25, color: "#f5f5f7" },
  sub: { size: 56, weight: 600, y: 0.31, color: "#c7c7cc" },
  clock: { size: 168, weight: 800, y: 0.24, color: "#f5f5f7", tracking: -4 },
  machine: { size: 50, weight: 400, y: 0.7, color: "#a1a1a6", font: MONO, tracking: 0 },
  brand: { size: 190, weight: 800, y: 0.22, color: "#c9baff", glow: true, tracking: -5 },
};
const POP = 0.14;
const OUT = 0.18;

function wrap(g, text, maxWidth) {
  const words = text.split(" ");
  const lines = [];
  let line = "";
  for (const word of words) {
    const test = line ? `${line} ${word}` : word;
    if (g.measureText(test).width > maxWidth && line) { lines.push(line); line = word; } else line = test;
  }
  lines.push(line);
  return lines;
}

function caption(g, text, style, t, start, end, width, height) {
  const inK = clamp((t - start) / POP);
  const outK = smooth(end - OUT, end, t);
  const alpha = Math.min(easeOut(inK), 1 - outK);
  if (alpha <= 0.001) return;
  const s = STYLES[style];
  const scale = 1 + (1 - easeOut(inK)) * 0.14 - outK * 0.04;
  g.save();
  g.font = `${s.weight} ${s.size}px ${s.font ?? DISPLAY}`;
  if ("letterSpacing" in g) g.letterSpacing = `${s.tracking ?? -1.5}px`;
  g.textAlign = "center"; g.textBaseline = "middle";
  const lines = wrap(g, text, width * 0.86);
  const cy = height * s.y;
  g.translate(width / 2, cy); g.scale(scale, scale); g.translate(-width / 2, -cy);
  g.globalAlpha = alpha;
  g.filter = outK > 0 ? `blur(${outK * 12}px)` : "none";
  g.shadowColor = s.glow ? "rgba(170,150,255,0.9)" : "rgba(0,0,0,0.65)";
  g.shadowBlur = s.glow ? 60 : 24;
  g.fillStyle = s.color;
  lines.forEach((line, i) => g.fillText(line, width / 2, cy + (i - (lines.length - 1) / 2) * s.size * 1.08));
  g.restore();
}

export function drawType(g, t, width, height) {
  // A soft shade behind the caption band keeps white type legible over any drawing.
  const shade = g.createLinearGradient(0, 0, 0, height * 0.5);
  shade.addColorStop(0, "rgba(0,0,0,0.55)"); shade.addColorStop(1, "rgba(0,0,0,0)");
  g.fillStyle = shade; g.fillRect(0, 0, width, height * 0.5);
  for (const [a, b, text, style] of CAPTIONS) caption(g, text, style, t, at(a), at(b), width, height);
  // End card.
  const end = t - at(17);
  if (end > 0) {
    const show = (delay) => easeOut(clamp((end - delay) / 0.5));
    g.save(); g.textAlign = "center"; g.textBaseline = "middle";
    g.globalAlpha = show(0.7); g.font = `800 150px ${DISPLAY}`; g.fillStyle = "#f5f5f7";
    if ("letterSpacing" in g) g.letterSpacing = "-4px";
    g.fillText("Looper", width / 2, height * 0.7);
    g.globalAlpha = show(1.1); g.font = `600 54px ${DISPLAY}`; g.fillStyle = "#c9baff";
    if ("letterSpacing" in g) g.letterSpacing = "-1px";
    g.fillText("Leave the desk. Keep the loop.", width / 2, height * 0.765);
    g.globalAlpha = show(1.6); g.font = `500 40px ${DISPLAY}`; g.fillStyle = "#a1a1a6";
    g.fillText("looper.fyi", width / 2, height * 0.815);
    g.restore();
  }
}

// What the drawings' blank screens show.
export function drawScreen(g, w, h, t, kind) {
  g.clearRect(0, 0, w, h);
  g.fillStyle = "rgba(214, 211, 204, 0.82)"; g.fillRect(0, 0, w, h); // a paper-grey screen so the pencil text survives the glow
  g.textBaseline = "middle";
  const blink = Math.floor(t / (BEAT / 2)) % 2 === 0;
  if (kind === "monitor") {
    g.font = `600 44px ${MONO}`; g.fillStyle = "rgba(40,38,48,0.85)";
    g.fillText("codex · looper", w * 0.3, h * 0.26);
    g.font = `40px ${MONO}`;
    g.fillText("› resume by seq", w * 0.3, h * 0.4);
    g.font = `700 46px ${MONO}`; g.fillStyle = "rgba(20,18,26,0.95)";
    g.fillText("■ continue? [y/N]", w * 0.3, h * 0.6);
    if (blink) g.fillRect(w * 0.3, h * 0.7, 26, 44);
  } else if (kind === "resumed") {
    g.font = `700 46px ${MONO}`; g.fillStyle = "rgba(110,80,230,0.95)";
    g.fillText("↻ looper: on it", w * 0.3, h * 0.3);
    const checks = ["test ✓", "lint ✓", "typecheck ✓"];
    const shown = Math.floor((t % BAR) / (BEAT * 0.6)) + 1;
    g.font = `44px ${MONO}`;
    checks.slice(0, shown).forEach((line, i) => g.fillText(line, w * 0.3, h * (0.48 + i * 0.14)));
  } else if (kind === "phone") {
    g.font = `700 60px ${DISPLAY}`; g.fillStyle = "rgba(110,80,230,0.95)";
    g.fillText("LOOPER", w * 0.1, h * 0.2);
    g.font = `600 56px ${DISPLAY}`; g.fillStyle = "rgba(30,28,36,0.95)";
    g.fillText("claude-code asks:", w * 0.1, h * 0.38);
    g.font = `52px ${DISPLAY}`;
    g.fillText("Also run the migration?", w * 0.1, h * 0.52);
    g.font = `700 56px ${DISPLAY}`; g.fillStyle = "rgba(110,80,230,0.95)";
    g.fillText("› yes, then ship", w * 0.1, h * 0.72);
  }
}
