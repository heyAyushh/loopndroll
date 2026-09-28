// Page tools: frame a drawing with a virtual camera, and draw by hand on top of it.
import { prng } from "./story.js";

export const PAGE = { width: 1920, height: 1080 };
export const GRAPHITE = "rgba(38, 35, 42, 0.9)";
export const CHALK = "rgba(236, 242, 255, 0.92)";

export function makeCanvas(width = PAGE.width, height = PAGE.height) {
  const element = document.createElement("canvas");
  element.width = width; element.height = height;
  return element;
}

// Draw a plate so it covers the page, then through a camera: zoom around a focus point (0..1 plate coords).
export function drawPlate(g, image, { zoom = 1, x = 0.5, y = 0.5, alpha = 1, rotate = 0 } = {}) {
  if (!image) { g.fillStyle = "#ece7dc"; g.fillRect(0, 0, PAGE.width, PAGE.height); return (u, v) => [u * PAGE.width, v * PAGE.height]; }
  const cover = Math.max(PAGE.width / image.width, PAGE.height / image.height) * zoom;
  const w = image.width * cover, h = image.height * cover;
  const left = Math.min(0, Math.max(PAGE.width - w, PAGE.width / 2 - x * w));
  const top = Math.min(0, Math.max(PAGE.height - h, PAGE.height / 2 - y * h));
  g.save();
  g.globalAlpha = alpha;
  if (rotate) { g.translate(PAGE.width / 2, PAGE.height / 2); g.rotate(rotate); g.translate(-PAGE.width / 2, -PAGE.height / 2); }
  g.drawImage(image, left, top, w, h);
  g.restore();
  // Map plate-normalized coordinates to page pixels, for drawing on the plate.
  return (u, v) => [left + u * w, top + v * h];
}

// Graphite handwriting / type: several faint passes with jitter so it reads as pencil, not a font.
export function pencilText(g, text, x, y, { font, color = GRAPHITE, passes = 3, jitter = 0.7, seed = 1, align = "left", reveal = 1, baseline = "alphabetic" } = {}) {
  const random = prng(seed);
  const shown = text.slice(0, Math.ceil(text.length * Math.min(1, Math.max(0, reveal))));
  if (!shown) return;
  g.save();
  const base = g.globalAlpha; // respect any fade the caller set
  g.font = font; g.textAlign = align; g.textBaseline = baseline; g.fillStyle = color;
  for (let pass = 0; pass < passes; pass += 1) {
    g.globalAlpha = base * (pass === 0 ? 0.9 : 0.35);
    g.fillText(shown, x + (random() - 0.5) * jitter * 2, y + (random() - 0.5) * jitter * 2);
  }
  g.restore();
}

// Graphite catches only on the paper's tooth: punch tiny gaps into whatever was just drawn, so type and
// lines read as pencil on paper instead of a clean digital fill.
export function graphiteGrain(g, width, height, { seed = 1, density = 0.9, size = 2.2 } = {}) {
  const random = prng(seed);
  g.save();
  g.globalCompositeOperation = "destination-out";
  g.fillStyle = "rgba(0,0,0,0.55)";
  const count = Math.floor(width * height * density / 900);
  for (let i = 0; i < count; i += 1) {
    const x = random() * width, y = random() * height;
    g.fillRect(x, y, size * (0.5 + random()), size * 0.35 * (0.5 + random()));
  }
  g.restore();
}

// Draw into a scratch layer, give it graphite grain, then lay it on the page.
const scratch = makeCanvas();
export function inGraphite(g, draw, options = {}) {
  const s = scratch.getContext("2d");
  s.setTransform(1, 0, 0, 1, 0, 0);
  s.clearRect(0, 0, scratch.width, scratch.height);
  draw(s);
  graphiteGrain(s, scratch.width, scratch.height, options);
  g.drawImage(scratch, 0, 0);
}

// A wobbly hand-drawn line.
export function handLine(g, x0, y0, x1, y1, { color = GRAPHITE, width = 2.2, wobble = 1.4, seed = 3, reveal = 1 } = {}) {
  const random = prng(seed);
  const steps = 18;
  g.save();
  const base = g.globalAlpha;
  g.strokeStyle = color; g.lineWidth = width; g.lineCap = "round";
  for (let pass = 0; pass < 2; pass += 1) {
    g.globalAlpha = base * (pass === 0 ? 0.85 : 0.35);
    g.beginPath();
    for (let i = 0; i <= steps * Math.min(1, reveal); i += 1) {
      const k = i / steps;
      const x = x0 + (x1 - x0) * k + (random() - 0.5) * wobble;
      const y = y0 + (y1 - y0) * k + (random() - 0.5) * wobble;
      if (i === 0) g.moveTo(x, y); else g.lineTo(x, y);
    }
    g.stroke();
  }
  g.restore();
}

// A square drawn by hand, stroke by stroke (reveal 0..1 draws its four sides in order).
export function handSquare(g, cx, cy, size, options = {}) {
  const h = size / 2;
  const corners = [[cx - h, cy - h], [cx + h, cy - h], [cx + h, cy + h], [cx - h, cy + h], [cx - h, cy - h]];
  const reveal = options.reveal ?? 1;
  for (let side = 0; side < 4; side += 1) {
    const local = Math.min(1, Math.max(0, reveal * 4 - side));
    if (local <= 0) break;
    const [x0, y0] = corners[side];
    const [x1, y1] = corners[side + 1];
    handLine(g, x0, y0, x0 + (x1 - x0) * local * 1.04, y0 + (y1 - y0) * local * 1.04, { ...options, seed: (options.seed ?? 1) + side * 7 });
  }
}

// Map a source canvas onto a quad (four page-space corners: tl, tr, br, bl) with two affine triangles.
export function drawQuad(g, source, quad, alpha = 1) {
  const [tl, tr, br, bl] = quad;
  const w = source.width, h = source.height;
  const triangle = (p0, p1, p2, s0, s1, s2) => {
    g.save();
    g.beginPath(); g.moveTo(...p0); g.lineTo(...p1); g.lineTo(...p2); g.closePath(); g.clip();
    const [x0, y0] = s0, [x1, y1] = s1, [x2, y2] = s2;
    const d = x0 * (y1 - y2) + x1 * (y2 - y0) + x2 * (y0 - y1);
    const a = (p0[0] * (y1 - y2) + p1[0] * (y2 - y0) + p2[0] * (y0 - y1)) / d;
    const b = (p0[1] * (y1 - y2) + p1[1] * (y2 - y0) + p2[1] * (y0 - y1)) / d;
    const c = (p0[0] * (x2 - x1) + p1[0] * (x0 - x2) + p2[0] * (x1 - x0)) / d;
    const dd = (p0[1] * (x2 - x1) + p1[1] * (x0 - x2) + p2[1] * (x1 - x0)) / d;
    const e = (p0[0] * (x1 * y2 - x2 * y1) + p1[0] * (x2 * y0 - x0 * y2) + p2[0] * (x0 * y1 - x1 * y0)) / d;
    const f = (p0[1] * (x1 * y2 - x2 * y1) + p1[1] * (x2 * y0 - x0 * y2) + p2[1] * (x0 * y1 - x1 * y0)) / d;
    g.setTransform(a, b, c, dd, e, f);
    g.globalAlpha = alpha;
    g.drawImage(source, 0, 0);
    g.restore();
  };
  triangle(tl, tr, br, [0, 0], [w, 0], [w, h]);
  triangle(tl, br, bl, [0, 0], [w, h], [0, h]);
}
