// Stage 0: CRT power-on into an XP-flavoured pixel desktop; double-click
// "Looper Cadet", the window zooms open, PRESS START, insert coin.
import { createCanvas } from "@napi-rs/canvas";
import { T, clamp, lerp, prog, easeOut, easeIn, easeInOut, blinkOn, seeded, mixColor, orb } from "./core.js";

const B = T.boot;
const TASKBAR_H = 18;
const HILL_GREENS = ["#6cc43a", "#5bb32f", "#4ba226", "#3c8f1f", "#2f7b19", "#256815"];
const CURSOR = [
  "k..........", "kk.........", "kwk........", "kwwk.......", "kwwwk......", "kwwwwk.....",
  "kwwwwwk....", "kwwwwwwk...", "kwwwwwwwk..", "kwwwwwwwwk.", "kwwwwwkkkkk", "kwwkwwk....",
  "kwk.kwwk...", "kk..kwwk...", "k....kwwk..", ".....kwwk..", "......kk...",
];
const ICON_X = 44;
const ICONS = [
  { y: 16, label: ["My Agents"] },
  { y: 66, label: ["Recycle Bin"] },
  { y: 116, label: ["Looper", "Cadet"] },
];
const WINDOW_OPEN_DONE = B.window + 0.25;
const ZOOM_SCALE = 3.2;

export function createBoot(format) {
  let scratch = null;
  const rand = seeded(4);
  const clouds = Array.from({ length: 6 }, () => ({ x: rand(), y: rand(), w: 30 + rand() * 50 }));

  function layout(p) {
    const w = Math.min(p.W - 40, 380);
    const h = format === "portrait" ? 270 : 236;
    return { x: Math.round((p.W - w) / 2), y: Math.round((p.H - TASKBAR_H - h) / 2) - 4, w, h };
  }
  const hillTop = (p, x) =>
    p.H * 0.66 - p.H * 0.2 * Math.exp(-(((x - p.W * 0.62) / (p.W * 0.46)) ** 2)) - p.H * 0.05 * Math.exp(-(((x - p.W * 0.12) / (p.W * 0.23)) ** 2));

  function drawDesktop(p, t) {
    const skyBottom = p.H * 0.66;
    p.bands(0, skyBottom, "#1f5fd8", "#a7d0f7", 12);
    for (const cloud of clouds) {
      const x = ((cloud.x * (p.W + 120) + t * 5) % (p.W + 120)) - 60;
      const y = 14 + cloud.y * skyBottom * 0.6;
      p.rect("#ffffff", x, y, cloud.w, 7);
      p.rect("#ffffff", x + 7, y - 5, cloud.w - 16, 5);
      p.rect("#dfeefc", x + 2, y + 7, cloud.w - 4, 2);
    }
    for (let x = 0; x < p.W; x += 2) {
      const top = hillTop(p, x);
      HILL_GREENS.forEach((green, i) => {
        const y0 = top + i * 16;
        p.rect(green, x, y0, 2, p.H - y0);
      });
    }
    // icons
    p.rect("#d7dbe6", ICON_X - 11, ICONS[0].y, 22, 16);
    p.rect("#2458c8", ICON_X - 9, ICONS[0].y + 2, 18, 11);
    p.rect("#9aa1b3", ICON_X - 4, ICONS[0].y + 16, 8, 3);
    p.rect("#cfd6e0", ICON_X - 8, ICONS[1].y + 2, 16, 3);
    p.rect("#a9b3c3", ICON_X - 7, ICONS[1].y + 5, 14, 14);
    p.rect("#7f8a9c", ICON_X - 4, ICONS[1].y + 7, 2, 10);
    p.rect("#7f8a9c", ICON_X + 2, ICONS[1].y + 7, 2, 10);
    p.image(orb(22), ICON_X - 11, ICONS[2].y - 1);
    const selected = t >= B.click1;
    ICONS.forEach((icon, index) => {
      icon.label.forEach((line, row) => {
        const y = icon.y + 25 + row * 10;
        if (selected && index === 2) p.rect("#316ac5", ICON_X - line.length * 4 - 1, y - 1, line.length * 8 + 2, 10);
        p.text(line, ICON_X, y, "#ffffff", { align: "center", shadow: "#000000" });
      });
    });
    // taskbar
    const y = p.H - TASKBAR_H;
    p.rect("#245edb", 0, y, p.W, TASKBAR_H);
    p.rect("#3f8cf3", 0, y, p.W, 2);
    p.rect("#3c9e3c", 0, y, 60, TASKBAR_H);
    p.rect("#5cbf4f", 0, y, 60, 2);
    p.text("start", 10, y + 6, "#ffffff", { shadow: "#1d5e1d" });
    p.rect("#1591ea", p.W - 76, y, 76, TASKBAR_H);
    p.text("4:20 PM", p.W - 68, y + 6, "#ffffff");
    if (t >= WINDOW_OPEN_DONE) {
      p.rect("#1e4fb8", 66, y + 2, 118, TASKBAR_H - 3);
      p.text("Looper Cadet", 72, y + 6, "#ffffff");
    }
  }

  function drawWindow(p, t, win) {
    const { x, y, w, h } = win;
    p.rect("#0831d9", x, y, w, h);
    for (let i = 0; i < 20; i++) p.rect(mixColor("#3a8af5", "#0846d8", i / 19), x, y + i, w, 1);
    p.text("Looper Cadet", x + 8, y + 6, "#ffffff", { shadow: "#0a246a" });
    p.rect("#e0463a", x + w - 19, y + 3, 15, 14);
    p.text("x", x + w - 15, y + 6, "#ffffff");
    p.rect("#2f6fe8", x + w - 36, y + 3, 15, 14);
    p.rect("#2f6fe8", x + w - 53, y + 3, 15, 14);
    const cx = x + 3;
    const cy = y + 20;
    const cw = w - 6;
    const ch = h - 23;
    p.rect("#05081c", cx, cy, cw, ch);
    const starRand = seeded(9);
    for (let i = 0; i < 70; i++) {
      const sx = cx + starRand() * cw;
      const sy = cy + starRand() * ch;
      p.rect(blinkOn(t + i * 0.13, 1.5) ? "#ffffff" : "#6b77a8", sx, sy, 1, 1);
    }
    const mid = x + w / 2;
    const bob = Math.round(Math.sin(t * 3) * 3);
    p.image(orb(64), mid - 32, cy + 14 + bob);
    p.text("3D PINBALL", mid, cy + 88, "#7fe3ff", { align: "center" });
    p.text("LOOPER CADET", mid, cy + 100, "#ffd23f", { size: 16, align: "center", shadow: "#b3261e", shadowOffset: 2 });
    const barY = cy + 132;
    if (t < B.loadEnd) {
      const loading = prog(t, B.window + 0.2, B.loadEnd);
      p.rect("#8a93a8", mid - 100, barY, 200, 12);
      p.rect("#ffffff", mid - 98, barY + 2, 196, 8);
      const blocks = Math.floor(loading * 19);
      for (let i = 0; i < blocks; i++) p.rect("#2fb62f", mid - 96 + i * 10, barY + 3, 8, 6);
      p.text("LOADING TASKS...", mid, barY + 22, "#c8ccda", { align: "center" });
    } else {
      if (blinkOn(t, 3)) p.text("PRESS START", mid, barY + 4, "#ffd23f", { align: "center", shadow: "#b3261e" });
      if (t >= B.coin) p.text("CREDIT 1", cx + cw - 6, cy + ch - 14, "#7dff6a", { align: "right" });
    }
  }

  function drawScene(p, t) {
    p.clear("#000000");
    const lineEnd = B.crtOn + 0.25;
    const openEnd = B.crtOn + 0.6;
    if (t < B.crtOn) return;
    if (t < lineEnd) {
      const u = easeOut(prog(t, B.crtOn, lineEnd));
      p.rect("#ffffff", p.W / 2 - (p.W / 2) * u, p.H / 2 - 1, p.W * u, 2);
      return;
    }
    drawDesktop(p, t);
    if (t < openEnd) {
      const h = p.H * easeOut(prog(t, lineEnd, openEnd));
      p.rect("#000000", 0, 0, p.W, p.H / 2 - h / 2);
      p.rect("#000000", 0, p.H / 2 + h / 2, p.W, p.H);
    }
    p.alpha(0.8 * (1 - prog(t, openEnd, B.desktop)), () => p.clear("#ffffff"));
    const win = layout(p);
    if (t >= B.window) {
      const u = prog(t, B.window, WINDOW_OPEN_DONE);
      if (u < 1) {
        // Win9x-style zoom rectangles from the icon to the window
        for (let k = 0; k < 3; k++) {
          const v = easeOut(clamp(u - k * 0.15, 0, 1));
          const rx = lerp(ICON_X - 12, win.x, v);
          const ry = lerp(ICONS[2].y, win.y, v);
          const rw = lerp(24, win.w, v);
          const rh = lerp(24, win.h, v);
          p.rect("#ffffff", rx, ry, rw, 1);
          p.rect("#ffffff", rx, ry + rh, rw, 1);
          p.rect("#ffffff", rx, ry, 1, rh);
          p.rect("#ffffff", rx + rw, ry, 1, rh + 1);
        }
      } else drawWindow(p, t, win);
    }
    const move = easeInOut(prog(t, 1.6, 2.5));
    const cursorX = lerp(p.W * 0.72, ICON_X + 2, move);
    const cursorY = lerp(p.H * 0.72, ICONS[2].y + 8, move);
    if (t < B.window + 0.2) {
      CURSOR.forEach((row, r) =>
        [...row].forEach((ch, c) => {
          if (ch !== ".") p.rect(ch === "k" ? "#000000" : "#ffffff", cursorX + c, cursorY + r, 1, 1);
        }),
      );
    }
  }

  return {
    draw(p, t) {
      drawScene(p, t);
      if (t >= B.zoom) {
        // dive into the game window
        if (!scratch) scratch = createCanvas(p.W, p.H);
        const sc = scratch.getContext("2d");
        sc.drawImage(p.canvas, 0, 0);
        const s = lerp(1, ZOOM_SCALE, easeIn(prog(t, B.zoom, T.scenes.boot[1])));
        const win = layout(p);
        const fx = win.x + win.w / 2;
        const fy = win.y + win.h / 2;
        p.clear("#000000");
        p.ctx.drawImage(scratch, Math.round(fx - fx * s), Math.round(fy - fy * s), Math.round(p.W * s), Math.round(p.H * s));
      }
    },
  };
}
