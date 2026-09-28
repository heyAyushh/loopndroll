// The edit, page by page. Each shot draws its page (graphite / blueprint / watercolour) and its ink
// (Looper's lavender line), and returns how the pencil should treat it. See DIRECTION.md.
import { BAR, BEAT, BEATS, at, clamp, clockMinutes, clockText, easeInOut, easeOut, lerp, smooth } from "./story.js";
import { CHALK, GRAPHITE, PAGE, drawPlate, drawQuad, graphiteGrain, handLine, handSquare, inGraphite, makeCanvas, pencilText } from "./page.js";
import { LAVENDER, compassCircle, continuousLine, inkDot, risoType, roundingSquare } from "./ink.js";

const PAPER = 0xece7dc;
const BLUEPRINT = 0x21416e;
const MONO = '"SF Mono", Menlo, monospace';
const HAND = '"Bradley Hand", "Noteworthy", "Marker Felt", cursive';
const MARKER = '"Marker Felt", "Noteworthy", cursive';
const SANS = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';

// Blueprint geometry read off the drawings (plate-normalized).
const TRACK_VANISHING = [0.5, 0.527];
const PLAN_TRACKS = [0.188, 0.444, 0.7];
const PLAN_CENTER = [0.5, 0.465];
const PLAN_RADIUS_X = 0.105;
const PLAN_GATES = [[0.124, 0.3, 0.475, 0.697, 0.872], [0.124, 0.368, 0.628, 0.872], [0.124, 0.3, 0.475, 0.697, 0.872]];
// The four trapped agents: [track, x] just before a gate.
const TRAPPED = [[0, 0.275], [1, 0.343], [1, 0.603], [2, 0.45]];
// Where things sit in the room drawing.
const ROOM = { orb: [0.305, 0.475], window: [0.52, 0.06, 0.99, 0.58], monitor: [0.13, 0.4] };
const LENS_SQUARE = [0.59, 0.68];

const halfBeatOn = (t) => Math.floor(t / (BEAT / 2)) % 2 === 0;
const progress = (t, startBar, endBar) => clamp((t - at(startBar)) / (at(endBar) - at(startBar)));

export function createShots({ plates, orb, quads, lines }) {
  const plate = (name) => plates[name];
  const screen = makeCanvas(1400, 620);
  const phoneScreen = makeCanvas(600, 1100);
  const noteCanvas = makeCanvas(400, 400);
  const clockCanvas = makeCanvas(600, 200);
  const panel = makeCanvas(960, 540);
  const grayOrb = makeCanvas(256, 256);
  const og = grayOrb.getContext("2d");
  og.filter = "grayscale(1) contrast(1.2)"; og.drawImage(orb, 0, 0, 256, 256);

  const quadOn = (name, index, toPage) => (quads[name]?.[index] ?? []).map(([u, v]) => toPage(u, v));
  const paper = (g, color = "#ece7dc") => { g.fillStyle = color; g.fillRect(0, 0, PAGE.width, PAGE.height); };

  // ---------- screen content, written in pencil on the monitor's blank glow ----------
  function terminal(t, { resumed = false, panes = 1 } = {}) {
    const g = screen.getContext("2d");
    g.clearRect(0, 0, screen.width, screen.height); // the drawing's own blank glow is the screen
    const sessions = ["codex · looper", "claude-code · api", "cursor · web", "zed · docs"];
    const cols = panes === 1 ? 1 : 2, rows = panes === 4 ? 2 : 1;
    const w = screen.width / cols, h = screen.height / rows;
    for (let p = 0; p < panes; p += 1) {
      const x = (p % cols) * w, y = Math.floor(p / cols) * h;
      if (panes > 1) [[x + 6, y + 6, x + w - 6, y + 6], [x + w - 6, y + 6, x + w - 6, y + h - 6], [x + w - 6, y + h - 6, x + 6, y + h - 6], [x + 6, y + h - 6, x + 6, y + 6]]
        .forEach(([a, b, c, d], i) => handLine(g, a, b, c, d, { width: 3, seed: p * 4 + i, color: resumed ? LAVENDER : GRAPHITE }));
      const size = panes === 1 ? 38 : 30;
      const color = resumed ? LAVENDER : GRAPHITE;
      pencilText(g, sessions[p], x + 30, y + size + 18, { font: `600 ${size * 0.8}px ${MONO}`, color: "rgba(60,56,66,0.85)", seed: p });
      const bodyTop = y + size * 2.6;
      if (!resumed) {
        const lines = panes === 1 ? ["› make reconnect resume by seq", "  editing state_mini_recovery.rs +84 −31", "  cargo test  ok", "  next: h3 e2e, then lint"] : [`  stopped · ${idle(p, t)}`];
        lines.forEach((line, i) => pencilText(g, line, x + 30, bodyTop + i * size * 1.35, { font: `${size}px ${MONO}`, seed: p * 9 + i }));
        const promptY = bodyTop + lines.length * size * 1.35 + size * 0.6;
        pencilText(g, "■ continue? [y/N]", x + 30, promptY, { font: `700 ${size}px ${MONO}`, seed: p + 30, reveal: panes === 1 ? progress(t, 8.5, 9) : 1 });
        if (halfBeatOn(t)) { g.fillStyle = GRAPHITE; g.fillRect(x + 30, promptY + size * 0.45, size * 0.55, size * 0.9); }
      } else {
        const done = ["↻ looper: keep going", "  test ✓", "  lint ✓", "  typecheck ✓", "  e2e ✓"];
        const shown = (t - at(34) - p * BEAT * 0.5) / (BEAT * 0.8);
        done.slice(0, Math.max(0, Math.floor(shown))).forEach((line, i) => pencilText(g, line, x + 30, bodyTop + i * size * 1.3, { font: `${i ? 400 : 600} ${size}px ${MONO}`, color, seed: p * 5 + i }));
      }
    }
    graphiteGrain(g, screen.width, screen.height, { seed: Math.floor(t * 12) });
    return screen;
  }
  const idle = (pane, t) => { const m = Math.max(1, clockMinutes(t) - (167 - pane * 25)); return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`; };

  function phone(t) {
    const g = phoneScreen.getContext("2d");
    g.clearRect(0, 0, phoneScreen.width, phoneScreen.height);
    pencilText(g, clockText(clockMinutes(t)), 300, 220, { font: `200 150px ${SANS}`, align: "center", seed: 2 });
    handSquare(g, 300, 470, 470, { width: 3, seed: 8 });
    if (t < at(20)) {
      pencilText(g, "claude-code · api", 90, 330, { font: `600 32px ${SANS}`, seed: 4 });
      pencilText(g, "stopped: continue?", 90, 390, { font: `36px ${SANS}`, seed: 5 });
    } else {
      pencilText(g, "LOOPER", 90, 330, { font: `700 30px ${SANS}`, color: LAVENDER, seed: 4 });
      pencilText(g, "claude-code asks:", 90, 390, { font: `600 34px ${SANS}`, seed: 5 });
      pencilText(g, "Also run the migration?", 90, 440, { font: `34px ${SANS}`, seed: 6 });
      pencilText(g, "yes, then ship", 90, 640, { font: `44px ${HAND}`, seed: 7, reveal: progress(t, 37.3, 37.8) });
      if (t > at(37.85)) pencilText(g, "● same session · running", 90, 720, { font: `600 30px ${SANS}`, color: LAVENDER, seed: 9 });
    }
    graphiteGrain(g, phoneScreen.width, phoneScreen.height, { seed: Math.floor(t * 12) + 3 });
    return phoneScreen;
  }

  const shots = [];
  const shot = (startBar, endBar, draw) => shots.push({ start: at(startBar), end: at(endBar), startBar, endBar, draw });

  // ================= ACT I — the work =================
  shot(0, 1, (g, ink, t) => {
    // The antagonist, innocent: one pencil square, drawn and erased on the half beat. We push into it.
    paper(g);
    const push = 1 + Math.pow(progress(t, 0.5, 1), 2) * 5;
    g.save(); g.translate(PAGE.width / 2, PAGE.height / 2); g.scale(push, push); g.translate(-PAGE.width / 2, -PAGE.height / 2);
    // Drawn once, then it blinks like a cursor: filled on the half beat, an outline between.
    handSquare(g, PAGE.width / 2, PAGE.height / 2, 92, { width: 4.5, wobble: 2, reveal: Math.min(1, t / (BEAT * 1.2)), seed: 1 });
    if (halfBeatOn(t) && t > BEAT * 1.2) { g.fillStyle = "rgba(38,35,42,0.85)"; g.fillRect(PAGE.width / 2 - 40, PAGE.height / 2 - 40, 80, 80); }
    g.restore();
    return { paper: PAPER, reveal: 1 };
  });
  shot(1, 3, (g, ink, t) => {
    drawPlate(g, plate("eye-night"), { zoom: lerp(1.02, 1.14, progress(t, 1, 3)), x: 0.45, y: 0.55 });
    return { paper: PAPER, reveal: easeOut(progress(t, 1, 2.2)) };
  });
  const machineRun = (g, t, { zoomFrom, zoomTo, startBar, endBar, speed = 1, gate = false, frozen = null }) => {
    const k = progress(t, startBar, endBar);
    const toPage = drawPlate(g, plate("machine-track"), { zoom: lerp(zoomFrom, zoomTo, easeInOut(k)), x: TRACK_VANISHING[0], y: TRACK_VANISHING[1] });
    // The agent: a white-ink point racing the rail toward the vanishing point.
    const depth = frozen ?? ((t * speed * 0.45) % 1);
    const [vx, vy] = toPage(...TRACK_VANISHING);
    const [bx, by] = toPage(0.5, 1.05);
    const p = 1 - Math.pow(1 - depth, 2.2);
    const x = lerp(bx, vx, p), y = lerp(by, vy, p);
    const size = lerp(34, 7, p);
    g.save();
    g.fillStyle = CHALK; g.shadowColor = "white"; g.shadowBlur = 18;
    g.beginPath(); g.arc(x, y, size, 0, Math.PI * 2); g.fill();
    g.restore();
    if (frozen === null) for (let s = 1; s < 8; s += 1) handLine(g, x, y + size * s * 1.8, x, y + size * s * 1.8 + size * 1.5, { color: CHALK, width: Math.max(1, size / 5), seed: s });
    if (gate) {
      const [gx, gy] = toPage(0.5, 0.47);
      const gateSize = (toPage(0.56, 0.47)[0] - gx) * 2;
      handSquare(g, gx, gy, gateSize, { color: CHALK, width: 3, reveal: gate === true ? 1 : gate, seed: 21 });
    }
    return { paper: BLUEPRINT, mode: 1 };
  };
  shot(3, 4, (g, ink, t) => machineRun(g, t, { zoomFrom: 1.0, zoomTo: 1.25, startBar: 3, endBar: 4, speed: 1.6 }));
  shot(4, 6, (g, ink, t) => {
    const k = progress(t, 4, 6);
    const toPage = drawPlate(g, plate("room-night"), { zoom: lerp(1.0, 1.12, easeInOut(k)), x: 0.3, y: 0.5 });
    rain(g, t, toPage, 1);
    inkDot(ink, ...toPage(...ROOM.orb), 3.5, 0.55 + 0.25 * Math.sin(t * 3)); // one lavender glint, planted
    return { paper: PAPER, reveal: easeOut(progress(t, 4, 4.7)) };
  });
  shot(6, 7, (g, ink, t) => {
    const k = progress(t, 6, 7);
    const toPage = drawPlate(g, plate("note-clock"), { zoom: 1.15, x: 0.5, y: lerp(0.72, 0.3, easeInOut(smooth(0.35, 0.85, k))) });
    const n = noteCanvas.getContext("2d");
    n.clearRect(0, 0, 400, 400);
    pencilText(n, "LAUNCH", 200, 170, { font: `60px ${MARKER}`, align: "center", reveal: progress(t, 6, 6.4), seed: 3, passes: 4 });
    pencilText(n, "9:00", 200, 270, { font: `700 96px ${MARKER}`, align: "center", reveal: progress(t, 6.25, 6.55), seed: 4, passes: 4 });
    if (t > at(6.5)) handLine(n, 110, 300, 290, 292, { width: 5, seed: 6, reveal: progress(t, 6.5, 6.7) });
    const noteQuad = (quads["note-clock"] ?? []).slice().sort((a, b) => b[0][1] - a[0][1])[0];
    const clockQuad = (quads["note-clock"] ?? []).slice().sort((a, b) => a[0][1] - b[0][1])[0];
    if (noteQuad) drawQuad(g, noteCanvas, noteQuad.map(([u, v]) => toPage(u, v)));
    if (clockQuad) {
      const c = clockCanvas.getContext("2d");
      c.clearRect(0, 0, 600, 200);
      pencilText(c, clockText(clockMinutes(t)), 300, 150, { font: `500 130px ${MONO}`, align: "center", seed: 11, color: "rgba(38,35,42,0.75)" });
      graphiteGrain(c, 600, 200, { seed: 5, density: 1.4 });
      drawQuad(g, clockCanvas, clockQuad.map(([u, v]) => toPage(u, v)));
    }
    return { paper: PAPER };
  });
  shot(7, 8, (g, ink, t) => machineRun(g, t, { zoomFrom: 1.3, zoomTo: 1.9, startBar: 7, endBar: 8, speed: 0.9, gate: progress(t, 7, 7.6) }));
  // ================= The Stop: the pencil lifts =================
  shot(8, 8.5, (g, ink, t) => {
    const look = machineRun(g, t, { zoomFrom: 1.9, zoomTo: 1.9, startBar: 8, endBar: 8.5, gate: true, frozen: 0.93 });
    return { ...look, reveal: lerp(1, 0.4, easeOut(progress(t, 8, 8.5))), boil: 0.2 };
  });
  shot(8.5, 9.5, (g, ink, t) => {
    const toPage = drawPlate(g, plate("monitor-cu"), { zoom: lerp(1.05, 1.12, progress(t, 8.5, 9.5)), x: 0.5, y: 0.45 });
    drawQuad(g, terminal(t), quadOn("monitor-cu", 0, toPage));
    return { paper: PAPER };
  });
  shot(9.5, 10, (g, ink, t) => {
    const toPage = drawPlate(g, plate("eye-night"), { zoom: 1.9, x: LENS_SQUARE[0] - 0.06, y: LENS_SQUARE[1] - 0.04 });
    if (halfBeatOn(t)) handSquare(g, ...toPage(...LENS_SQUARE), 38, { color: "rgba(255,255,255,0.9)", width: 3, seed: 5 });
    return { paper: PAPER, fatigue: 0.1 };
  });

  // ================= ACT II — they become the loop =================
  shot(10, 10.6, (g, ink, t) => { drawPlate(g, plate("rub-eyes"), { zoom: lerp(1.0, 1.06, progress(t, 10, 10.6)), x: 0.45, y: 0.4 }); return { paper: PAPER, fatigue: 0.15 }; });
  shot(10.6, 11, (g, ink, t) => {
    drawPlate(g, plate("hands-keyboard"), { zoom: 1.12, x: 0.6, y: 0.55 });
    if (t > BEATS.firstYes) pencilText(g, "y", PAGE.width * 0.62, PAGE.height * 0.36, { font: `700 120px ${HAND}`, seed: 2, passes: 5 });
    return { paper: PAPER, fatigue: 0.18, shake: t > BEATS.firstYes && t < BEATS.firstYes + 0.12 ? 0.3 : 0 };
  });
  // The comic page: every stop adds a panel. Panels are squares; the page becomes a cage.
  const PANEL_SEQUENCE = [
    { at: 11, plate: "machine-track", caption: "it runs", x: 0.5, y: 0.6 },
    { at: 12, plate: "monitor-cu", caption: "03:10 — continue?", x: 0.5, y: 0.4 },
    { at: 12.5, plate: "hands-keyboard", caption: "y", x: 0.6, y: 0.5 },
    { at: 13, plate: "phone-desk", caption: "03:40 — another one", x: 0.5, y: 0.5 },
    { at: 13.5, plate: "rub-eyes", caption: "y", x: 0.45, y: 0.4 },
    { at: 14, plate: "neighbour-window", caption: "04:05 — not only me", x: 0.42, y: 0.35 },
    { at: 14.5, plate: "eye-night", caption: "y", x: 0.45, y: 0.55 },
    { at: 14.75, plate: "monitor-cu", caption: "continue?", x: 0.5, y: 0.4 },
    { at: 15, plate: "hands-keyboard", caption: "y  y  y", x: 0.6, y: 0.5 },
  ];
  shot(11, 15.5, (g, ink, t) => {
    paper(g);
    const shown = PANEL_SEQUENCE.filter((p) => t >= at(p.at));
    const count = shown.length;
    const cols = count <= 1 ? 1 : count <= 4 ? 2 : 3;
    const rows = Math.ceil(count / cols);
    const gutter = 26, edge = 60;
    const w = (PAGE.width - edge * 2 - gutter * (cols - 1)) / cols;
    const h = (PAGE.height - edge * 2 - gutter * (rows - 1)) / rows;
    shown.forEach((p, i) => {
      const x = edge + (i % cols) * (w + gutter), y = edge + Math.floor(i / cols) * (h + gutter);
      const fatigue = i / PANEL_SEQUENCE.length;
      const pg = panel.getContext("2d");
      pg.save(); pg.clearRect(0, 0, panel.width, panel.height);
      pg.filter = `contrast(${1 + fatigue * 0.5}) brightness(${1 - fatigue * 0.25}) blur(${fatigue * 1.4}px)`;
      const image = plate(p.plate);
      const scale = Math.max(panel.width / image.width, panel.height / image.height) * 1.15;
      const dw = image.width * scale, dh = image.height * scale;
      const dx = Math.min(0, Math.max(panel.width - dw, panel.width / 2 - p.x * dw));
      const dy = Math.min(0, Math.max(panel.height - dh, panel.height / 2 - p.y * dh));
      pg.drawImage(image, dx, dy, dw, dh);
      pg.restore();
      const appear = clamp((t - at(p.at)) / 0.18);
      g.save();
      g.translate(x + w / 2, y + h / 2); g.rotate((i % 2 ? 1 : -1) * fatigue * 0.02); g.translate(-(x + w / 2), -(y + h / 2));
      g.globalAlpha = appear;
      g.drawImage(panel, x, y, w, h);
      g.globalAlpha = 1;
      handLine(g, x, y, x + w, y, { width: 3.5, seed: i * 4 + 1, wobble: 1 + fatigue * 4 });
      handLine(g, x + w, y, x + w, y + h, { width: 3.5, seed: i * 4 + 2, wobble: 1 + fatigue * 4 });
      handLine(g, x + w, y + h, x, y + h, { width: 3.5, seed: i * 4 + 3, wobble: 1 + fatigue * 4 });
      handLine(g, x, y + h, x, y, { width: 3.5, seed: i * 4 + 4, wobble: 1 + fatigue * 4 });
      // Caption box, comic style.
      const captionSize = Math.max(22, 44 - count * 2);
      g.fillStyle = "#f2eee4"; g.fillRect(x + 12, y + 12, captionSize * p.caption.length * 0.55 + 30, captionSize * 1.5);
      pencilText(g, p.caption, x + 26, y + 12 + captionSize * 1.1, { font: `${captionSize}px ${HAND}`, seed: i + 50 });
      g.restore();
    });
    return { paper: PAPER, fatigue: lerp(0.15, 0.85, progress(t, 11, 15.5)) };
  });
  shot(15.5, 16, (g, ink, t) => { drawPlate(g, plate("asleep-desk"), { zoom: 1.05, x: 0.5, y: 0.5 }); return { paper: PAPER, fatigue: 0.8, reveal: lerp(1, 0.55, progress(t, 15.5, 16)) }; });

  const plan = (g, ink, t, { zoom, x, y, blink = true, ring = 0, round = 0, orbit = false }) => {
    const toPage = drawPlate(g, plate("machine-plan"), { zoom, x, y });
    const [cx, cy] = toPage(...PLAN_CENTER);
    const radius = toPage(PLAN_CENTER[0] + PLAN_RADIUS_X, 0)[0] - cx;
    // Gates round into circles when Looper keeps the loop.
    if (round > 0) PLAN_GATES.forEach((row, track) => row.forEach((gx, i) => {
      const [px, py] = toPage(gx, PLAN_TRACKS[track]);
      const size = (toPage(gx + 0.03, 0)[0] - px) * 2;
      roundingSquare(ink, px, py, size, clamp(round * 1.4 - (i + track) * 0.08), { width: 7 });
    }));
    if (ring > 0) compassCircle(ink, cx, cy, radius * 0.86, ring, { width: 6 });
    TRAPPED.forEach(([track, gx], i) => {
      let px, py;
      if (orbit) {
        const angle = (t - at(32)) * Math.PI * 2 / BAR + i * Math.PI / 2;
        px = cx + Math.cos(angle) * radius * 0.86; py = cy + Math.sin(angle) * radius * 0.86;
        inkDot(ink, px, py, 11, 1);
        for (let tail = 1; tail < 10; tail += 1) { const a = angle - tail * 0.06; inkDot(ink, cx + Math.cos(a) * radius * 0.86, cy + Math.sin(a) * radius * 0.86, 11 - tail, 0.5); }
      } else {
        [px, py] = toPage(gx, PLAN_TRACKS[track]);
        // A trapped agent: a chalk light pulsing at its gate, the square's shadow of the cursor.
        const on = blink && halfBeatOn(t);
        const r = (toPage(gx + 0.009, 0)[0] - px) * (on ? 1.25 : 1);
        g.save(); g.fillStyle = CHALK; g.shadowColor = "white"; g.shadowBlur = on ? 40 : 14;
        g.beginPath(); g.arc(px, py, Math.max(9, r), 0, Math.PI * 2); g.fill(); g.restore();
      }
    });
    return toPage;
  };
  shot(16, 19, (g, ink, t) => {
    const k = easeInOut(progress(t, 16, 19));
    plan(g, ink, t, { zoom: lerp(3.2, 1.0, k), x: lerp(0.275, 0.5, k), y: lerp(0.188, 0.46, k) });
    return { paper: BLUEPRINT, mode: 1, fatigue: 0.2 };
  });
  shot(19, 21, (g, ink, t) => {
    const toPage = drawPlate(g, plate("room-night"), { zoom: 1.0, x: 0.5, y: 0.5 });
    rain(g, t, toPage, 1 - smooth(at(19), at(19.8), t));
    // The window's square shadows slide across the floor as the hours go (eraser-lifted light).
    const slide = progress(t, 19, 21);
    g.save(); g.globalAlpha = 0.16; g.fillStyle = "#f5f1e6";
    for (let i = 0; i < 5; i += 1) for (let j = 0; j < 2; j += 1) {
      const [px, py] = toPage(0.55 - slide * 0.35 + i * 0.075 - j * 0.04, 0.72 + j * 0.1);
      g.beginPath(); g.moveTo(px, py); g.lineTo(px + 110, py); g.lineTo(px + 60, py + 70); g.lineTo(px - 50, py + 70); g.closePath(); g.fill();
    }
    g.restore();
    // The storyboard margin counts the night: each time is written, then struck through by the next.
    const times = [280, 296, 311, 327, 342, 358];
    const written = Math.min(times.length, 1 + Math.floor(slide * times.length));
    g.save(); g.fillStyle = "rgba(236,231,220,0.85)"; g.fillRect(40, 40, 280, 70 + written * 62); g.restore();
    times.slice(0, written).forEach((minutes, i) => {
      pencilText(g, clockText(minutes), 70, 100 + i * 62, { font: `700 50px ${HAND}`, seed: 70 + i });
      if (i < written - 1) handLine(g, 62, 84 + i * 62, 230, 88 + i * 62, { width: 3.5, seed: 90 + i });
    });
    return { paper: PAPER, fatigue: lerp(0.35, 0.75, slide) };
  });
  shot(21, 23, (g, ink, t) => {
    const k = progress(t, 21, 23);
    const toPage = drawPlate(g, plate("orb"), { zoom: lerp(1.0, 1.3, k), x: 0.5, y: 0.5 });
    inkDot(ink, ...toPage(0.5, 0.47), 7, 0.25 + 0.15 * Math.sin(t * 2.2)); // the unspoken alternative
    return { paper: PAPER, fatigue: 0.4, inkGlow: 0.4 };
  });
  shot(23, 24, (g, ink, t) => { plan(g, ink, t, { zoom: 1.8, x: 0.45, y: 0.44 }); return { paper: BLUEPRINT, mode: 1, fatigue: 0.35 }; });

  // ================= ACT III — crisis =================
  shot(24, 25, (g, ink, t) => {
    drawPlate(g, plate("wake-phone"), { zoom: lerp(1.08, 1.0, easeOut(progress(t, 24, 25))), x: 0.5, y: 0.5 });
    return { paper: PAPER, shake: Math.max(0, 1 - (t - at(24)) * 2.5), reveal: easeOut(clamp((t - at(24)) / 0.25)), fatigue: 0.3 };
  });
  shot(25, 26, (g, ink, t) => {
    const toPage = drawPlate(g, plate("ots-monitor"), { zoom: 1.04, x: 0.55, y: 0.45 });
    for (let pane = 0; pane < 4; pane += 1) {
      const quad = quadOn("ots-monitor", pane, toPage);
      if (quad.length === 4) {
        const c = makeCanvas(700, 420); const cg = c.getContext("2d");
        pencilText(cg, ["codex · looper", "claude-code · api", "cursor · web", "zed · docs"][pane], 36, 80, { font: `600 44px ${MONO}`, seed: pane });
        pencilText(cg, `stopped · ${idle(pane, t)}`, 36, 180, { font: `44px ${MONO}`, seed: pane + 5 });
        pencilText(cg, "■ continue? [y/N]", 36, 290, { font: `700 44px ${MONO}`, seed: pane + 9 });
        graphiteGrain(cg, 700, 420, { seed: pane + Math.floor(t * 12) });
        drawQuad(g, c, quad);
      }
    }
    margin(g, clockText(clockMinutes(t)), "launch 9:00");
    return { paper: PAPER, fatigue: 0.35 };
  });
  shot(26, 27, (g, ink, t) => {
    drawPlate(g, plate("hands-keyboard"), { zoom: lerp(1.15, 1.25, progress(t, 26, 27)), x: 0.6, y: 0.55 });
    if (t > BEATS.habitYes) pencilText(g, "y", PAGE.width * 0.62, PAGE.height * 0.34, { font: `700 110px ${HAND}`, seed: 12, passes: 5, color: "rgba(38,35,42,0.7)" });
    return { paper: PAPER, fatigue: 0.45, shake: t > BEATS.habitYes && t < BEATS.habitYes + 0.1 ? 0.25 : 0 };
  });
  shot(27, 28, (g, ink, t) => {
    const lurch = smooth(at(27), at(27.25), t);
    const look = machineRun(g, t, { zoomFrom: 1.9, zoomTo: 1.9, startBar: 27, endBar: 28, gate: true, frozen: lerp(0.86, 0.93, lurch) });
    // The habit snaps the pencil: a broken lead scar across the page.
    if (t > at(27.25)) {
      const k = clamp((t - at(27.25)) / 0.2);
      g.save(); g.strokeStyle = "rgba(20,18,22,0.95)"; g.lineWidth = 7; g.lineCap = "round";
      g.beginPath(); g.moveTo(PAGE.width * 0.36, PAGE.height * 0.62); g.lineTo(PAGE.width * (0.36 + 0.2 * k), PAGE.height * (0.62 - 0.05 * k)); g.stroke();
      g.fillStyle = "rgba(20,18,22,0.9)";
      for (let i = 0; i < 9; i += 1) g.fillRect(PAGE.width * (0.56 + i * 0.006) + Math.sin(i * 7) * 12, PAGE.height * (0.57 + Math.cos(i * 3) * 0.02), 5, 5);
      g.restore();
    }
    return { ...look, shake: t > at(27.25) && t < at(27.25) + 0.15 ? 0.6 : 0, reveal: t > at(27.25) ? lerp(1, 0.75, clamp((t - at(27.25)) / 0.6)) : 1 };
  });
  shot(28, 28.6, (g, ink, t) => { drawPlate(g, plate("look-orb"), { zoom: 1.05, x: 0.5, y: 0.45 }); return { paper: PAPER, fatigue: 0.3 }; });
  shot(28.6, 29.3, (g, ink, t) => {
    const toPage = drawPlate(g, plate("neighbour-window"), { zoom: 1.6, x: 0.42, y: 0.33 });
    return { paper: PAPER, fatigue: 0.3 };
  });
  shot(29.3, 30, (g, ink, t) => {
    const toPage = drawPlate(g, plate("orb"), { zoom: lerp(1.3, 1.6, progress(t, 29.3, 30)), x: 0.5, y: 0.5 });
    inkDot(ink, ...toPage(0.5, 0.47), 10, 0.5);
    return { paper: PAPER, inkGlow: 0.7 };
  });
  // The choice: a pencil-drawn menu bar. A pencil arrow finds the orb; one option gets circled by hand.
  // Big and central: this is the crisis, the one choice in the film.
  const MENU = { top: 110, orbX: 1560, orbY: 150, box: [420, 250, 1080, 620], row: 92 };
  const MENU_ROWS = ["○  Stop when the agent stops", "●  Keep going until checks pass", "○  Ask me on iPhone"];
  shot(30, 32, (g, ink, t) => {
    paper(g);
    const { top, orbX, orbY, box: [mx, my, mw, mh], row } = MENU;
    const open = t >= BEATS.menuOpen;
    // Camera: start tight on the grey orb in the menu bar, pull back as the menu opens.
    const zoom = lerp(2.4, 1.0, easeInOut(smooth(BEATS.menuOpen - BEAT, BEATS.menuOpen + BEAT, t)));
    const focusX = lerp(orbX - 300, PAGE.width / 2, easeInOut(smooth(BEATS.menuOpen - BEAT, BEATS.menuOpen + BEAT, t)));
    const focusY = lerp(orbY + 140, PAGE.height / 2, easeInOut(smooth(BEATS.menuOpen - BEAT, BEATS.menuOpen + BEAT, t)));
    const camera = (s) => s.setTransform(zoom, 0, 0, zoom, PAGE.width / 2 - focusX * zoom, PAGE.height / 2 - focusY * zoom);
    inGraphite(g, (s) => {
      camera(s);
      // The desktop behind the menu: the four stopped sessions, drawn faintly.
      s.save(); s.globalAlpha = 0.42;
      [[90, 250], [990, 250], [90, 660], [990, 660]].forEach(([x, y], i) => {
        [[x, y, x + 840, y], [x + 840, y, x + 840, y + 370], [x + 840, y + 370, x, y + 370], [x, y + 370, x, y]].forEach(([a, b, c, d], j) => handLine(s, a, b, c, d, { width: 3, seed: 90 + i * 4 + j }));
        pencilText(s, ["codex · looper", "claude-code · api", "cursor · web", "zed · docs"][i], x + 30, y + 60, { font: `600 38px ${MONO}`, seed: 80 + i });
        pencilText(s, "■ continue? [y/N]", x + 30, y + 140, { font: `38px ${MONO}`, seed: 84 + i });
      });
      s.restore();
      handLine(s, 70, top, PAGE.width - 70, top, { width: 3.5, seed: 1, wobble: 2.4 });
      handLine(s, 70, top + 84, PAGE.width - 70, top + 84, { width: 3.5, seed: 2, wobble: 2.4 });
      ["Terminal", "Shell", "Edit", "View", "Window"].forEach((label, i) => pencilText(s, label, 110 + i * 200, top + 58, { font: `${i ? 400 : 700} 44px ${SANS}`, seed: i, jitter: 1.2 }));
      pencilText(s, clockText(clockMinutes(t)), PAGE.width - 110, top + 58, { font: `44px ${SANS}`, align: "right", seed: 9, jitter: 1.2 });
      if (open) {
        s.fillStyle = "#ece7dc"; s.fillRect(mx, my, mw, mh); // the menu sits on top of the stopped desktop
        [[mx, my, mx + mw, my], [mx + mw, my, mx + mw, my + mh], [mx + mw, my + mh, mx, my + mh], [mx, my + mh, mx, my]].forEach(([a, b, c, d], i) => handLine(s, a, b, c, d, { width: 4, seed: 21 + i, wobble: 2.6, reveal: clamp((t - BEATS.menuOpen) / 0.18 - i * 0.3) }));
        pencilText(s, "Looper", mx + 60, my + 100, { font: `800 64px ${SANS}`, seed: 30, jitter: 1.2 });
        pencilText(s, "4 sessions stopped · 3h 20m", mx + 60, my + 170, { font: `44px ${SANS}`, seed: 31, jitter: 1.2 });
        handLine(s, mx + 40, my + 215, mx + mw - 40, my + 215, { width: 2, seed: 33 });
        MENU_ROWS.forEach((label, i) => pencilText(s, label, mx + 60, my + 300 + i * row, { font: `${i === 1 ? 800 : 400} 50px ${SANS}`, seed: 40 + i, jitter: 1.2 }));
        // The hand circles its choice: the first circle drawn in the film.
        const circle = progress(t, 31.35, 32);
        if (circle > 0) {
          s.save(); s.strokeStyle = GRAPHITE; s.lineWidth = 5; s.lineCap = "round"; s.beginPath();
          s.ellipse(mx + mw / 2, my + 300 + row - 16, mw / 2 - 20, 58, -0.025, -Math.PI * 0.95, -Math.PI * 0.95 + Math.PI * 2.1 * easeInOut(circle)); s.stroke(); s.restore();
        }
      }
      // The pencil arrow pointer travels: to the orb, then down to the choice.
      const route = [[BEATS.pointer, 960, 820], [BEATS.menuOpen - 0.05, orbX - 10, orbY + 8], [BEATS.menuOpen + BEAT * 1.4, mx + mw * 0.72, my + 300 + row - 30], [at(32), mx + mw * 0.72, my + 300 + row - 30]];
      let [px, py] = [route[0][1], route[0][2]];
      for (let i = 1; i < route.length; i += 1) if (t < route[i][0] || i === route.length - 1) { const k = smooth(route[i - 1][0], route[i][0], t); px = lerp(route[i - 1][1], route[i][1], k); py = lerp(route[i - 1][2], route[i][2], k); break; }
      handLine(s, px, py, px + 10, py + 70, { width: 5, seed: 60 }); handLine(s, px, py, px + 50, py + 48, { width: 5, seed: 61 }); handLine(s, px + 22, py + 56, px + 44, py + 98, { width: 5, seed: 62 });
    }, { seed: Math.floor(t * 12) });
    g.save(); camera(g); g.globalAlpha = 0.9; g.drawImage(grayOrb, orbX - 34, orbY - 34, 68, 68); g.restore();
    return { paper: PAPER, fatigue: 0.2 };
  });

  // ================= The loop closes (the drop) =================
  shot(32, 33, (g, ink, t) => {
    const k = progress(t, 32, 33);
    plan(g, ink, t, { zoom: 1.0, x: 0.5, y: 0.46, blink: false, ring: easeOut(k * 1.4), round: easeOut(k), orbit: k > 0.55 });
    if (t < at(32, 2)) risoType(ink, "LOOPER", { size: 380, alpha: 1 - smooth(at(32, 1), at(32, 2), t) });
    return { paper: BLUEPRINT, mode: 1, riso: 1, inkGlow: 0.8, flash: t < at(32) + 0.12 ? 0.7 * (1 - (t - at(32)) / 0.12) : 0, flashColor: 0xd9ccff };
  });
  const tracedPlate = (name, startBar, endBar, { zoom = 1.04, x = 0.5, y = 0.5 } = {}) => (g, ink, t, sub) => {
    const toPage = drawPlate(g, plate(name), { zoom, x, y });
    if (lines[name]) continuousLine(ink, lines[name], progress(t, startBar, endBar) * 1.05, toPage, { width: 2.6 + sub * 1.5 });
    return { paper: PAPER, riso: 0.55, inkGlow: 0.5 + sub * 0.5 };
  };
  shot(33, 34, tracedPlate("stand-up", 33, 34));
  shot(34, 35, (g, ink, t) => {
    const toPage = drawPlate(g, plate("monitor-cu"), { zoom: 1.08, x: 0.5, y: 0.45 });
    drawQuad(g, terminal(t, { resumed: true, panes: 4 }), quadOn("monitor-cu", 0, toPage));
    return { paper: PAPER, riso: 0.4 };
  });
  shot(35, 36, tracedPlate("walk-window", 35, 36));
  shot(36, 37, (g, ink, t) => {
    plan(g, ink, t, { zoom: 1.5, x: 0.5, y: 0.46, blink: false, ring: 1, round: 1, orbit: true });
    return { paper: BLUEPRINT, mode: 1, riso: 0.8, inkGlow: 0.9 };
  });
  shot(37, 38, (g, ink, t) => {
    const toPage = drawPlate(g, plate("couch-phone"), { zoom: 1.05, x: 0.5, y: 0.45 });
    drawQuad(g, phone(t), quadOn("couch-phone", 0, toPage));
    return { paper: PAPER };
  });
  shot(38, 39, (g, ink, t, sub) => {
    const look = tracedPlate("couch-sleep-wide", 38, 39.2)(g, ink, t, sub);
    margin(g, clockText(clockMinutes(t)), "it keeps going");
    return look;
  });
  shot(39, 40, (g, ink, t) => {
    const k = progress(t, 39, 40);
    const toPage = plan(g, ink, t, { zoom: lerp(1.5, 2.6, easeInOut(k)), x: 0.5, y: 0.46, blink: false, ring: 1, round: 1, orbit: true });
    const [cx, cy] = toPage(...PLAN_CENTER);
    for (let r = 0; r < 6; r += 1) compassCircle(ink, cx, cy, 60 + r * 90 * (1 + k * 3), 1, { width: 4 + k * 20, alpha: k });
    return { paper: BLUEPRINT, mode: 1, riso: 1, inkGlow: 1, flash: Math.pow(smooth(0.6, 1, k), 2), flashColor: 0xfff3e0 };
  });

  // ================= Morning: colour arrives =================
  shot(40, 41.5, (g, ink, t) => {
    drawPlate(g, plate("dawn-wide"), { zoom: lerp(1.0, 1.06, progress(t, 40, 41.5)), x: 0.5, y: 0.5 });
    margin(g, "08:52", "shipped ✓");
    return { paper: 0xf3ecdf, mode: 2, reveal: easeOut(progress(t, 40, 41)), flash: Math.max(0, 1 - (t - at(40)) / 0.5) * 0.9, flashColor: 0xfff3e0 };
  });
  shot(41.5, 42.5, (g, ink, t) => { drawPlate(g, plate("stretch"), { zoom: 1.04, x: 0.5, y: 0.45 }); return { paper: 0xf3ecdf, mode: 2 }; });
  shot(42.5, 43.2, (g, ink, t) => { drawPlate(g, plate("window-silhouette"), { zoom: lerp(1.0, 1.05, progress(t, 42.5, 43.2)), x: 0.5, y: 0.45 }); return { paper: 0xf3ecdf, mode: 2 }; });
  shot(43.2, 44, (g, ink, t) => { drawPlate(g, plate("eye-dawn"), { zoom: lerp(1.02, 1.12, progress(t, 43.2, 44)), x: 0.45, y: 0.55 }); return { paper: 0xf3ecdf, mode: 2 }; });

  // ================= End card: the square rounds into the orb =================
  shot(44, 48, (g, ink, t, sub) => {
    paper(g);
    const k = t - at(44);
    const cx = PAGE.width / 2, cy = PAGE.height * 0.36;
    const round = easeInOut(smooth(BAR * 0.25, BAR * 0.8, k));
    const grow = easeOut(smooth(BAR * 0.25, BAR * 1.0, k));
    const size = lerp(46, 300, grow);
    if (round < 0.02) { if (halfBeatOn(t)) handSquare(g, cx, cy, size, { width: 3, seed: 7 }); }
    else roundingSquare(ink, cx, cy, size, round, { width: lerp(3, 7, grow) });
    const orbAlpha = smooth(BAR * 0.9, BAR * 1.4, k);
    if (orbAlpha > 0) { g.save(); g.globalAlpha = orbAlpha; g.drawImage(orb, cx - 120, cy - 120, 240, 240); g.restore(); }
    const reveal = (startBars) => smooth(BAR * startBars, BAR * (startBars + 0.4), k);
    g.save(); g.globalAlpha = reveal(1.5);
    pencilText(g, "LOOPER", cx, PAGE.height * 0.68, { font: `800 150px ${SANS}`, align: "center", seed: 3, passes: 4 });
    g.restore();
    g.save(); g.globalAlpha = reveal(2.0);
    pencilText(g, "Leave the desk. Keep the loop.", cx, PAGE.height * 0.78, { font: `60px ${HAND}`, align: "center", color: LAVENDER, seed: 5 });
    g.restore();
    g.save(); g.globalAlpha = reveal(2.5);
    pencilText(g, "Keeps your coding agents moving until the work is actually done.", cx, PAGE.height * 0.85, { font: `32px ${SANS}`, align: "center", seed: 6 });
    pencilText(g, "codex · claude code · cursor · zed · devin · grok build      looper.fyi", cx, PAGE.height * 0.91, { font: `26px ${MONO}`, align: "center", color: "rgba(60,56,66,0.8)", seed: 8 });
    g.restore();
    return { paper: PAPER, inkGlow: 0.5 + sub * 0.4, riso: 0.25, fade: smooth(at(47.4), at(48), t) };
  });

  // ---------- helpers that need page context ----------
  function rain(g, t, toPage, amount) {
    if (amount <= 0) return;
    const [x0, y0] = toPage(ROOM.window[0], ROOM.window[1]);
    const [x1, y1] = toPage(ROOM.window[2], ROOM.window[3]);
    g.save(); g.strokeStyle = `rgba(230,228,235,${0.35 * amount})`; g.lineWidth = 1.2;
    for (let i = 0; i < 160; i += 1) {
      const seedX = (i * 0.618) % 1, speed = 0.6 + ((i * 0.37) % 1) * 0.8;
      const y = y0 + (((i * 0.123 + t * speed * 0.45) % 1) * (y1 - y0));
      const x = x0 + seedX * (x1 - x0);
      g.beginPath(); g.moveTo(x, y); g.lineTo(x - 2, y + 26); g.stroke();
    }
    g.restore();
  }
  // Storyboard margin notes: the director's pencil in the corner, keeping time.
  function margin(g, time, note) {
    g.save();
    g.fillStyle = "rgba(236,231,220,0.82)"; g.fillRect(40, 40, 420, 120);
    pencilText(g, time, 64, 104, { font: `700 56px ${HAND}`, seed: 71 });
    pencilText(g, note, 64, 144, { font: `30px ${HAND}`, seed: 72 });
    g.restore();
  }

  return {
    list: shots,
    draw(g, ink, t, sub) {
      const current = shots.find((s) => t >= s.start && t < s.end) ?? shots[shots.length - 1];
      g.globalAlpha = 1; g.filter = "none";
      const look = current.draw(g, ink, t, sub) ?? {};
      return look;
    },
  };
}
