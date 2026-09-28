// Lavender ink: Looper's line. One stroke that never lifts, a compass for circles, riso type for the drop.
import { PAGE } from "./page.js";

export const LAVENDER = "rgba(157, 134, 246, 1)";

// Draw the first `progress` (0..1) of a continuous path, mapped through `toPage` (plate coords -> page px).
// The nib glows where the line is being drawn right now.
export function continuousLine(g, points, progress, toPage, { width = 2.4, alpha = 1, nib = true } = {}) {
  const count = Math.floor(points.length * Math.min(1, Math.max(0, progress)));
  if (count < 2) return;
  g.save();
  g.globalAlpha = alpha;
  g.strokeStyle = LAVENDER; g.lineWidth = width; g.lineJoin = "round"; g.lineCap = "round";
  g.beginPath();
  const [x0, y0] = toPage(...points[0]);
  g.moveTo(x0, y0);
  for (let i = 1; i < count; i += 1) { const [x, y] = toPage(...points[i]); g.lineTo(x, y); }
  g.stroke();
  if (nib) {
    const [x, y] = toPage(...points[count - 1]);
    const glow = g.createRadialGradient(x, y, 0, x, y, 26);
    glow.addColorStop(0, "rgba(200,185,255,0.95)"); glow.addColorStop(1, "rgba(157,134,246,0)");
    g.fillStyle = glow; g.fillRect(x - 26, y - 26, 52, 52);
  }
  g.restore();
}

// A compass circle, drawn from angle 0 around to `progress`.
export function compassCircle(g, cx, cy, radius, progress, { width = 3, alpha = 1, start = -Math.PI / 2 } = {}) {
  if (progress <= 0) return;
  g.save();
  g.globalAlpha = alpha;
  g.strokeStyle = LAVENDER; g.lineWidth = width; g.lineCap = "round";
  g.beginPath(); g.arc(cx, cy, radius, start, start + Math.PI * 2 * Math.min(1, progress)); g.stroke();
  g.restore();
}

// A square whose corners round into a circle as `round` goes 0 -> 1 (the Stop becoming the loop).
export function roundingSquare(g, cx, cy, size, round, { width = 3, alpha = 1, color = LAVENDER } = {}) {
  const half = size / 2;
  g.save();
  g.globalAlpha = alpha; g.strokeStyle = color; g.lineWidth = width;
  g.beginPath(); g.roundRect(cx - half, cy - half, size, size, half * Math.min(1, Math.max(0, round))); g.stroke();
  g.restore();
}

// Techno-flyer type, printed in lavender for the riso pass.
export function risoType(g, text, { size = 420, alpha = 1, y = PAGE.height / 2, letterSpacing = 20 } = {}) {
  g.save();
  g.globalAlpha = alpha;
  g.fillStyle = LAVENDER;
  g.font = `900 ${size}px -apple-system, "SF Pro Display", "Helvetica Neue", sans-serif`;
  if ("letterSpacing" in g) g.letterSpacing = `${letterSpacing}px`;
  g.textAlign = "center"; g.textBaseline = "middle";
  g.fillText(text, PAGE.width / 2, y);
  g.restore();
}

export function inkDot(g, x, y, radius, alpha = 1) {
  g.save();
  g.globalAlpha = alpha;
  g.fillStyle = LAVENDER;
  g.beginPath(); g.arc(x, y, radius, 0, Math.PI * 2); g.fill();
  g.restore();
}
