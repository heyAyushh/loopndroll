// Shared math, determinism and pixel-painting helpers.
// Every scene draws at true low resolution; nothing here antialiases, so the
// nearest-neighbour upscale in the encoder keeps hard pixel edges.
import { createCanvas, GlobalFonts, loadImage } from "@napi-rs/canvas";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const ROOT = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
export const asset = (name) => path.join(ROOT, "assets", name);
export const T = JSON.parse(readFileSync(asset("timing.json"), "utf8"));

GlobalFonts.registerFromPath(asset("PressStart2P.ttf"), "PS2P");
GlobalFonts.registerFromPath(asset("VT323.ttf"), "VT");

export const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
export const lerp = (a, b, u) => a + (b - a) * u;
export const prog = (t, a, b) => clamp((t - a) / (b - a), 0, 1);
export const easeOut = (u) => 1 - Math.pow(1 - u, 3);
export const easeIn = (u) => u * u * u;
export const easeInOut = (u) => (u < 0.5 ? 4 * u * u * u : 1 - Math.pow(-2 * u + 2, 3) / 2);
export const backOut = (u) => {
  const c = 2.2;
  return 1 + (c + 1) * Math.pow(u - 1, 3) + c * Math.pow(u - 1, 2);
};
export const blinkOn = (t, hz) => Math.floor(t * hz * 2) % 2 === 0;
export const DEG = Math.PI / 180;

export function seeded(seed) {
  return function () {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let r = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}
export const hash = (n) => seeded(Math.floor(n) * 7919 + 13)();

export function scalarAt(points, t) {
  if (t <= points[0][0]) return points[0][1];
  for (let i = 1; i < points.length; i++) {
    if (t <= points[i][0]) {
      const [t0, v0] = points[i - 1];
      const [t1, v1] = points[i];
      return lerp(v0, v1, (t - t0) / (t1 - t0));
    }
  }
  return points[points.length - 1][1];
}

export function mixColor(a, b, u) {
  const pa = parseInt(a.slice(1), 16);
  const pb = parseInt(b.slice(1), 16);
  const ch = (s) => Math.round(lerp((pa >> s) & 255, (pb >> s) & 255, clamp(u, 0, 1)));
  const hex = (v) => v.toString(16).padStart(2, "0");
  return `#${hex(ch(16))}${hex(ch(8))}${hex(ch(0))}`;
}

// ---------- images ----------
let logoImage = null;
export async function loadAssets() {
  logoImage = await loadImage(readFileSync(asset("looper-logo.png")));
}
// The logo PNG is a glass orb on a grey field; crop just the orb.
const ORB_CENTER = [512, 498];
const ORB_RADIUS = 312;
const orbCache = new Map();
export function orb(size) {
  size = Math.max(2, Math.round(size));
  if (!orbCache.has(size)) {
    const canvas = createCanvas(size, size);
    const c = canvas.getContext("2d");
    c.drawImage(logoImage, ORB_CENTER[0] - ORB_RADIUS, ORB_CENTER[1] - ORB_RADIUS, ORB_RADIUS * 2, ORB_RADIUS * 2, 0, 0, size, size);
    // hard circular mask, no soft edge
    const img = c.getImageData(0, 0, size, size);
    const r = size / 2;
    for (let y = 0; y < size; y++) {
      for (let x = 0; x < size; x++) {
        const dx = x + 0.5 - r;
        const dy = y + 0.5 - r;
        if (dx * dx + dy * dy > r * r) img.data[(y * size + x) * 4 + 3] = 0;
        else img.data[(y * size + x) * 4 + 3] = 255;
      }
    }
    c.putImageData(img, 0, 0);
    orbCache.set(size, canvas);
  }
  return orbCache.get(size);
}

// ---------- painter ----------
const textCache = new Map();

export class Painter {
  constructor(width, height) {
    this.W = width;
    this.H = height;
    this.canvas = createCanvas(width, height);
    this.ctx = this.canvas.getContext("2d");
    this.ctx.imageSmoothingEnabled = false;
  }

  alpha(a, fn) {
    if (a <= 0) return;
    const prev = this.ctx.globalAlpha;
    this.ctx.globalAlpha = prev * clamp(a, 0, 1);
    fn();
    this.ctx.globalAlpha = prev;
  }

  rect(color, x, y, w, h) {
    const x0 = Math.round(x);
    const y0 = Math.round(y);
    const x1 = Math.round(x + w);
    const y1 = Math.round(y + h);
    if (x1 <= x0 || y1 <= y0) return;
    this.ctx.fillStyle = color;
    this.ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
  }

  clear(color) {
    this.ctx.fillStyle = color;
    this.ctx.fillRect(0, 0, this.W, this.H);
  }

  // Vertical banded gradient: the 16-colour-era way to shade a sky.
  bands(top, bottom, from, to, count, x = 0, width = this.W) {
    const h = (bottom - top) / count;
    for (let i = 0; i < count; i++) this.rect(mixColor(from, to, count === 1 ? 0 : i / (count - 1)), x, top + i * h, width, h + 1);
  }

  // Checkerboard dither for shading without new colours.
  dither(color, x, y, w, h, phase = 0) {
    this.ctx.fillStyle = color;
    const x0 = Math.round(x);
    const y0 = Math.round(y);
    for (let j = 0; j < Math.round(h); j++) {
      for (let i = (j + phase) % 2; i < Math.round(w); i += 2) this.ctx.fillRect(x0 + i, y0 + j, 1, 1);
    }
  }

  line(color, x0, y0, x1, y1, thickness = 1) {
    const steps = Math.max(1, Math.ceil(Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0))));
    this.ctx.fillStyle = color;
    const half = (thickness - 1) / 2;
    for (let i = 0; i <= steps; i++) {
      const u = i / steps;
      const x = Math.round(lerp(x0, x1, u) - half);
      const y = Math.round(lerp(y0, y1, u) - half);
      this.ctx.fillRect(x, y, thickness, thickness);
    }
  }

  ellipse(color, cx, cy, rx, ry) {
    if (rx < 0.5 || ry < 0.5) return;
    this.ctx.fillStyle = color;
    const top = Math.floor(cy - ry);
    const bottom = Math.ceil(cy + ry);
    for (let y = top; y < bottom; y++) {
      const v = (y + 0.5 - cy) / ry;
      if (Math.abs(v) > 1) continue;
      const half = rx * Math.sqrt(1 - v * v);
      const x0 = Math.round(cx - half);
      const x1 = Math.round(cx + half);
      if (x1 > x0) this.ctx.fillRect(x0, y, x1 - x0, 1);
    }
  }

  disc(color, cx, cy, r) {
    this.ellipse(color, cx, cy, r, r);
  }

  // Scanline polygon fill (even-odd), crisp at any angle.
  poly(color, points) {
    if (points.length < 3) return;
    this.ctx.fillStyle = color;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const [, y] of points) {
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    }
    for (let y = Math.floor(minY); y <= Math.ceil(maxY); y++) {
      const sy = y + 0.5;
      const xs = [];
      for (let i = 0; i < points.length; i++) {
        const [ax, ay] = points[i];
        const [bx, by] = points[(i + 1) % points.length];
        if ((ay <= sy && by > sy) || (by <= sy && ay > sy)) xs.push(ax + ((sy - ay) / (by - ay)) * (bx - ax));
      }
      xs.sort((a, b) => a - b);
      for (let k = 0; k + 1 < xs.length; k += 2) {
        const x0 = Math.round(xs[k]);
        const x1 = Math.round(xs[k + 1]);
        if (x1 > x0) this.ctx.fillRect(x0, y, x1 - x0, 1);
      }
    }
  }

  image(img, x, y, w = img.width, h = img.height) {
    this.ctx.drawImage(img, Math.round(x), Math.round(y), Math.round(w), Math.round(h));
  }

  // Crisp bitmap text: rasterise once, threshold alpha, cache.
  glyphs(str, size, color, font) {
    const key = `${font}|${size}|${color}|${str}`;
    if (textCache.has(key)) return textCache.get(key);
    const probe = createCanvas(4, 4).getContext("2d");
    probe.font = `${size}px ${font}`;
    const width = Math.max(1, Math.ceil(probe.measureText(str).width));
    const height = Math.ceil(size * 1.25);
    const canvas = createCanvas(width, height);
    const c = canvas.getContext("2d");
    c.font = `${size}px ${font}`;
    c.fillStyle = color;
    // Press Start 2P is drawn on an 8-unit grid: an integer alphabetic baseline
    // lands every glyph pixel exactly on a canvas pixel ("top" is half a pixel off).
    if (font === "PS2P") {
      c.textBaseline = "alphabetic";
      c.fillText(str, 0, size);
    } else {
      c.textBaseline = "top";
      c.fillText(str, 0, 0);
    }
    const img = c.getImageData(0, 0, width, height);
    for (let i = 3; i < img.data.length; i += 4) img.data[i] = img.data[i] >= 110 ? 255 : 0;
    c.putImageData(img, 0, 0);
    textCache.set(key, canvas);
    return canvas;
  }

  textWidth(str, size = 8, font = "PS2P") {
    return this.glyphs(str, size, "#ffffff", font).width;
  }

  text(str, x, y, color, { size = 8, align = "left", font = "PS2P", shadow = null, shadowOffset = 1 } = {}) {
    const g = this.glyphs(str, size, color, font);
    let left = x;
    if (align === "center") left = x - g.width / 2;
    else if (align === "right") left = x - g.width;
    if (shadow) this.ctx.drawImage(this.glyphs(str, size, shadow, font), Math.round(left) + shadowOffset, Math.round(y) + shadowOffset);
    this.ctx.drawImage(g, Math.round(left), Math.round(y));
    return g.width;
  }

  // Word-wrapped text block; returns the height used.
  paragraph(str, x, y, maxWidth, color, { size = 8, font = "PS2P", lineHeight = size * 1.5, align = "left", shadow = null } = {}) {
    const words = str.split(" ");
    const lines = [];
    let line = "";
    for (const word of words) {
      const next = line ? `${line} ${word}` : word;
      if (this.textWidth(next, size, font) > maxWidth && line) {
        lines.push(line);
        line = word;
      } else line = next;
    }
    if (line) lines.push(line);
    lines.forEach((l, i) => this.text(l, x, y + i * lineHeight, color, { size, font, align, shadow }));
    return lines.length * lineHeight;
  }

  // Chunky box with a 2px border, the arcade dialog look.
  panel(x, y, w, h, fill, border, thickness = 2) {
    this.rect(border, x, y, w, h);
    this.rect(fill, x + thickness, y + thickness, w - thickness * 2, h - thickness * 2);
  }
}

// Pixel-grid wipe used between stages: cells snap to full then vanish in a seeded order.
const wipeOrder = [];
{
  const r = seeded(77);
  for (let i = 0; i < 4096; i++) wipeOrder.push(r());
}
export function pixelWipe(p, t, coverStart, revealStart, cell = 20) {
  const COVER = 0.3;
  const REVEAL = 0.4;
  if (t < coverStart || t > revealStart + REVEAL) return;
  const cols = Math.ceil(p.W / cell);
  const rows = Math.ceil(p.H / cell);
  for (let j = 0; j < rows; j++) {
    for (let i = 0; i < cols; i++) {
      const d = wipeOrder[(j * cols + i) % wipeOrder.length];
      let on;
      if (t < revealStart) on = t >= coverStart + d * COVER;
      else on = t < revealStart + d * REVEAL;
      if (on) p.rect("#000000", i * cell, j * cell, cell, cell);
    }
  }
}

// Big arcade title: stage label + slammed title, used at each stage open.
export function stageBanner(p, t, tIn, tOut, stage, title, y, size = 16, cx = p.W / 2) {
  if (t < tIn || t > tOut) return;
  const fade = 1 - prog(t, tOut - 0.2, tOut);
  if (Math.floor(fade * 3) <= 0) return;
  const slam = backOut(prog(t, tIn + 0.1, tIn + 0.35));
  p.text(stage, cx, y, "#7fe3ff", { align: "center", shadow: "#0a1a4a" });
  if (t >= tIn + 0.1) {
    const s = Math.max(1, Math.round(lerp(size * 2, size, clamp(slam, 0, 1)) / 8) * 8);
    p.text(title, cx, y + 14 + (size - s) / 2, "#ffd23f", { size: s, align: "center", shadow: "#b3261e", shadowOffset: 2 });
  }
}
