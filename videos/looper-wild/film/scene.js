// The wild cut, composed. Everything is drawn by code every frame; nothing is ever an image.
import { BAR, BEAT, H, STUTTERS, STUTTER_TIMES, W, at, between, clamp, easeInOut, easeOut, lerp, prng, smooth, stopsSoFar, workTime, working } from "./story.js";
import { createSwarm } from "./swarm.js";
import { drawFigure, poseAt, tremble } from "./figure.js";

const WHITE = [240, 240, 248];
const LAVENDER = [178, 156, 255];
const DAWN = [255, 190, 130];
const MONITOR = { x0: 180, y0: 1000, x1: 540, y1: 1280 };
const DESK_ORB = [640, 1262];

const CAPTIONS = [
  [0, 1.0, [["2:47 AM", 230], ["LAUNCH IS AT 9", 70]], 0.27],
  [1.05, 1.95, [["YOUR AGENTS", 100], ["ARE SHIPPING IT", 100]], 0.25],
  [2.0, 3.0, [["THEN THEY", 120], ["STOP.", 150]], 0.24],
  ...STUTTERS.map((b, k) => [b, b + 0.5, [[STUTTER_TIMES[k], 260]], 0.24]),
  [7.05, 8.0, [["YOU BECAME", 120], ["THE LOOP.", 140]], 0.24],
  [8.0, 8.75, [["5:58 AM", 230]], 0.24],
  [8.8, 9.5, [["KEEP", 120], ["BABYSITTING?", 120]], 0.24],
  [9.55, 10.0, [["OR…", 180]], 0.24],
  [10.0, 11.0, [["LOOPER", 230]], 0.2, LAVENDER],
  [11.05, 12.0, [["LEAVE", 150], ["THE DESK.", 150]], 0.2],
  [12.05, 13.0, [["YOUR AGENTS", 110], ["KEEP GOING.", 110]], 0.2],
  [13.0, 13.5, [["UNTIL THE", 110], ["CHECKS PASS.", 110]], 0.2],
  [13.55, 14.5, [["IT ASKS ONLY", 96], ["WHAT MATTERS.", 96]], 0.2],
  [14.55, 15.0, [["YOU SLEEP.", 130]], 0.24],
  [15.05, 16.4, [["8:52 AM", 220], ["SHIPPED.", 130]], 0.2],
  [17.2, 19.0, [["LOOPER", 210]], 0.63, LAVENDER],
];

// Where the developer is and what they're doing.
const TRACK = [
  [0, "typing"], [at(1.9), "typing"], [at(2.3), "slump"], [at(7), "slump"], [at(7.4), "asleep"], [at(8), "asleep"], [at(8.12), "jolt"],
  [at(8.75), "jolt"], [at(9.1), "look"], [at(10.9), "look"], [at(11.3), "stand"], [at(11.5), "walk", 40], [at(12.2), "walk", 700],
  [at(13.45), "walk", 700], [at(13.55), "lie"], [at(15.9), "lie"], [at(16.3), "stretch"], [at(19), "stretch"],
];

export function createScene(p) {
  const swarm = createSwarm(p, CAPTIONS.map(([a, b, lines, y, colour]) => ({ start: at(a), end: at(b), lines, y, colour })));
  const random = prng(21);
  const streams = Array.from({ length: 2400 }, () => ({ sx: MONITOR.x0 + random() * (MONITOR.x1 - MONITOR.x0), sy: MONITOR.y0 + random() * (MONITOR.y1 - MONITOR.y0), phase: random(), angle: -Math.PI / 2 + (random() - 0.5) * 1.6, length: 700 + random() * 1400, curl: (random() - 0.5) * 3 }));
  const stars = Array.from({ length: 260 }, () => ({ x: random() * W, y: random() * H * 0.7, twinkle: random() * 6.28, size: 1 + random() * 2.5 }));
  const cage = [];
  for (let row = 0; row < 18; row += 1) for (let col = 0; col < 10; col += 1) cage.push({ x: 54 + col * 108, y: 54 + row * 108, row, col, seed: random() * 100 });

  // ---------- pieces ----------
  const room = (t, alpha, colour = WHITE, fatigue = 0) => {
    const o = { colour, alpha, shake: 1 + fatigue * 5, weight: 3 };
    tremble(p, [[110, 1300], [730, 1300]], t, { ...o, seed: 10 });
    tremble(p, [[150, 1300], [150, 1700]], t, { ...o, seed: 11 });
    tremble(p, [[690, 1300], [700, 1700]], t, { ...o, seed: 12 });
    const { x0, y0, x1, y1 } = MONITOR;
    tremble(p, [[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]], t, { ...o, weight: 4, seed: 13 });
    tremble(p, [[360, y1], [360, 1300]], t, { ...o, seed: 14 });
    tremble(p, [[470, 1294], [630, 1294]], t, { ...o, weight: 5, seed: 15 });
    tremble(p, [[700, 1455], [870, 1455], [890, 1170]], t, { ...o, seed: 16 });
    tremble(p, [[790, 1455], [790, 1650]], t, { ...o, seed: 17 });
    tremble(p, [[715, 1668], [865, 1668]], t, { ...o, seed: 18 });
  };
  // The work: code pouring out of the monitor. It runs on work-time, so a Stop freezes it in the air.
  const code = (t, colour, intensity, energy = 0) => {
    const w = workTime(t);
    p.push(); p.blendMode(p.ADD);
    for (const s of streams) {
      const age = (s.phase + w * 0.32) % 1;
      const tail = Math.max(0, age - 0.035);
      const pos = (a) => {
        const d = a * s.length * (1 + energy * 0.7); // each burst of work blasts up the frame
        const bend = Math.sin(a * 5 + s.curl) * 90 * a + p.noise(s.sx * 0.01, a * 2) * 120 - 60;
        return [s.sx + Math.cos(s.angle) * d + Math.cos(s.angle + Math.PI / 2) * bend, s.sy + Math.sin(s.angle) * d + Math.sin(s.angle + Math.PI / 2) * bend];
      };
      const [x0, y0] = pos(tail), [x1, y1] = pos(age);
      p.stroke(colour[0], colour[1], colour[2], 150 * (1 - age) * intensity);
      p.strokeWeight(2);
      p.line(x0, y0, x1, y1);
    }
    p.pop();
    // The screen itself glows and scrolls.
    const { x0, y0, x1, y1 } = MONITOR;
    p.noStroke(); p.fill(colour[0], colour[1], colour[2], 26 * intensity + 10); p.rect(x0 + 6, y0 + 6, x1 - x0 - 12, y1 - y0 - 12);
    for (let line = 0; line < 12; line += 1) {
      const scroll = (line * 23 + w * 70) % 260;
      const width = 60 + ((line * 97) % 200);
      p.fill(colour[0], colour[1], colour[2], 110 * intensity);
      p.rect(x0 + 24, y0 + 14 + scroll, width, 6);
    }
  };
  // The cage: every Stop adds squares; at the drop they round into circles and spiral into the loop.
  const cageAndLoop = (t, centre, energy) => {
    const stops = stopsSoFar(t);
    const drop = t >= at(10);
    const since = t - at(10);
    for (const cell of cage) {
      const shown = (17 - cell.row) < stops * 2.2 ? 1 : 0;
      if (!shown && !drop) continue;
      const wobble = (p.noise(cell.seed, t * 0.8) - 0.5) * 0.18;
      let x = cell.x + (p.noise(cell.seed + 9, t * 0.5) - 0.5) * 6, y = cell.y;
      let size = 66;
      let round = 0;
      let colour = WHITE, alpha = 55 + stops * 6;
      if (drop) {
        const dist = Math.hypot(cell.x - centre[0], cell.y - centre[1]);
        round = easeOut(clamp((since - dist / 2400) / 0.35));
        const orbit = easeInOut(clamp((since - 0.3 - dist / 3000) / 1.4));
        const angle = Math.atan2(cell.y - centre[1], cell.x - centre[0]) + since * (0.9 + 380 / (dist + 200));
        const radius = lerp(dist, 300 + (dist % 260) * 0.9, orbit);
        x = lerp(x, centre[0] + Math.cos(angle) * radius, orbit); y = lerp(y, centre[1] + Math.sin(angle) * radius * 0.92, orbit);
        size = lerp(66, 26 + (cell.seed % 1) * 20, orbit);
        colour = LAVENDER; alpha = (90 + energy * 120) * (shown ? 1 : round);
      }
      p.push();
      p.translate(x, y); p.rotate(wobble * (1 - round));
      p.noFill(); p.stroke(colour[0], colour[1], colour[2], alpha); p.strokeWeight(3);
      p.rect(-size / 2, -size / 2, size, size, (size / 2) * round);
      p.pop();
    }
  };
  // Looper's mark: a circle with the S-fold inside, drawn in light.
  const orb = (t, [cx, cy], radius, glow, colour = LAVENDER) => {
    p.push(); p.blendMode(p.ADD); p.noFill();
    for (let k = 6; k > 0; k -= 1) { p.stroke(colour[0], colour[1], colour[2], glow * 16 / k); p.strokeWeight(k * 7); p.circle(cx, cy, radius * 2); }
    p.stroke(255, 255, 255, 200 * Math.min(1, glow)); p.strokeWeight(4); p.circle(cx, cy, radius * 2);
    p.strokeWeight(5);
    p.beginShape();
    for (let k = 0; k <= 40; k += 1) { const u = k / 40 * 2 - 1; p.curveVertex(cx + u * radius * 0.72, cy + Math.sin(u * Math.PI + t * 1.4) * radius * 0.34 - u * radius * 0.1); }
    p.endShape();
    p.pop();
  };
  const ring = (t, [cx, cy], radius, energy) => {
    p.push(); p.blendMode(p.ADD); p.noFill();
    for (let k = 0; k < 90; k += 1) {
      const a0 = k / 90 * Math.PI * 2 + t * 1.6 + Math.sin(k * 7.1) * 0.1;
      const r = radius + Math.sin(k * 3.3 + t * 2) * 14;
      p.stroke(LAVENDER[0], LAVENDER[1], LAVENDER[2], 120 + energy * 120); p.strokeWeight(2 + (k % 3));
      p.arc(cx, cy, r * 2, r * 2 * 0.92, a0, a0 + 0.28);
    }
    p.pop();
  };
  const check = (x, y, size, k) => {
    if (k <= 0) return;
    p.stroke(LAVENDER[0], LAVENDER[1], LAVENDER[2], 240); p.strokeWeight(14); p.noFill();
    const a = [x - size * 0.5, y], b = [x - size * 0.12, y + size * 0.4], c = [x + size * 0.6, y - size * 0.55];
    const first = clamp(k * 2), second = clamp(k * 2 - 1);
    p.line(a[0], a[1], lerp(a[0], b[0], first), lerp(a[1], b[1], first));
    if (second > 0) p.line(b[0], b[1], lerp(b[0], c[0], second), lerp(b[1], c[1], second));
  };
  const couch = (t, colour, alpha) => {
    const o = { colour, alpha, weight: 4 };
    tremble(p, [[180, 1640], [940, 1640], [960, 1760], [160, 1760], [180, 1640]], t, { ...o, seed: 30 });
    tremble(p, [[200, 1640], [210, 1500], [930, 1500], [920, 1640]], t, { ...o, seed: 31 });
  };
  const bubble = (x, y, text, colour, k, alignRight) => {
    if (k <= 0) return;
    p.push();
    p.textFont('-apple-system, "SF Pro Display", sans-serif'); p.textSize(48); p.textStyle(p.BOLD);
    const w = p.textWidth(text) + 70;
    const bx = alignRight ? x - w : x;
    p.translate(bx + w / 2, y); p.scale(lerp(0.6, 1, easeOut(k))); p.translate(-(bx + w / 2), -y);
    p.noFill(); p.stroke(colour[0], colour[1], colour[2], 230 * k); p.strokeWeight(4); p.rect(bx, y - 50, w, 100, 50);
    p.noStroke(); p.fill(colour[0], colour[1], colour[2], 255 * k); p.textAlign(p.LEFT, p.CENTER); p.text(text, bx + 35, y + 2);
    p.pop();
  };

  return {
    draw(t, energy) {
      const dawn = smooth(at(15), at(15.8), t);
      // Background: black; warmed by dawn.
      p.background(4, 4, 7);
      if (dawn > 0) {
        const g = p.drawingContext.createLinearGradient(0, 0, 0, H);
        g.addColorStop(0, `rgba(120,70,90,${0.55 * dawn})`); g.addColorStop(0.55, `rgba(255,150,90,${0.45 * dawn})`); g.addColorStop(1, `rgba(40,20,20,${0.4 * dawn})`);
        p.drawingContext.fillStyle = g; p.drawingContext.fillRect(0, 0, W, H);
      }
      const end = smooth(at(17), at(17.5), t);
      // After the drop the whole frame punches on every kick.
      p.push();
      if (t >= at(10)) { p.translate(W / 2, H / 2); p.scale(1 + energy * 0.02); p.translate(-W / 2, -H / 2); }
      const night = t >= at(13.5) && t < at(15);
      if (night || dawn > 0) for (const s of stars) { p.noStroke(); p.fill(255, 255, 255, (80 + 80 * Math.sin(t * 2 + s.twinkle)) * (1 - dawn)); p.circle(s.x, s.y, s.size); }

      const drop = t >= at(10);
      const orbCentre = drop ? [540, lerp(900, t < at(17) ? 560 : 700, easeInOut(smooth(at(10.7), at(11.4), t)))] : DESK_ORB;
      // The cage and, after the drop, the loop.
      if (t < at(17.5) || drop) cageAndLoop(t, orbCentre, energy);

      // The room and the person (until they leave for the couch).
      const onCouch = t >= at(13.5);
      const fatigue = clamp((stopsSoFar(t) - 1) / 8) * (t < at(10) ? 1 : 0.2);
      const roomAlpha = (onCouch ? 60 : 150) * (1 - end);
      if (!onCouch || t < at(15)) {
        room(t, roomAlpha, drop ? LAVENDER : WHITE, fatigue);
        code(t, drop ? LAVENDER : WHITE, (working(t) ? 1 : 0.35) * (1 - end) * (onCouch ? 0.5 : 1), working(t) ? energy : 0);
      }
      if (onCouch) couch(t, dawn > 0 ? DAWN : WHITE, 170 * (1 - end));
      const pose = poseAt(t, TRACK);
      const personAlpha = 240 * (1 - end) * (between(t, 12.2, 13.45) ? 0 : 1);
      if (personAlpha > 0) drawFigure(p, pose, t, { fatigue, colour: dawn > 0 ? [255, 225, 200] : WHITE, alpha: personAlpha });

      // The orb: dark on the desk, then the one lavender circle in a world of squares.
      const crisisGlow = smooth(at(8.75), at(9.3), t);
      if (!drop) orb(t, DESK_ORB, lerp(22, 30, crisisGlow), 0.25 + crisisGlow * 1.4 + (between(t, 9.5, 10) ? Math.sin(t * 8) * 0.3 : 0));
      else {
        const grow = easeOut(smooth(at(10), at(10.25), t));
        const radius = lerp(30, t < at(11) ? 150 : 90, grow) * (1 - end * 0.1);
        orb(t, orbCentre, radius, 1.2 + energy * 0.8);
        ring(t, orbCentre, radius * 2.3, energy);
      }
      // The work finishing without them.
      if (between(t, 12, 13.5)) [12.5, 12.75, 13.0].forEach((b, k) => check(250 + k * 150, 1150, 90, clamp((t - at(b)) / 0.18)));
      // The phone, from the couch.
      if (between(t, 13.6, 14.55)) {
        p.noStroke(); p.fill(220, 225, 255, 200); p.rect(400, 1380, 70, 120, 14);
        bubble(90, 1080, "Also run the migration?", WHITE, clamp((t - at(13.7)) / 0.2), false);
        bubble(990, 1220, "yes, then ship", LAVENDER, clamp((t - at(14.05)) / 0.2), true);
      }
      // Dawn: a sun of concentric strokes rising.
      if (dawn > 0) { p.noFill(); for (let k = 0; k < 7; k += 1) { p.stroke(255, 200, 140, 140 * dawn / (k + 1)); p.strokeWeight(6); p.circle(540, lerp(1100, 780, dawn), 140 + k * 60 + Math.sin(t * 2 + k) * 8); } }

      // The Stop made visible: a giant cursor and the question.
      if (between(t, 2, 3)) {
        const blink = Math.floor(t / (BEAT / 2)) % 2 === 0;
        p.noStroke(); p.fill(240, 240, 248, blink ? 235 : 40); p.rect(430, 610, 220, 220);
        p.fill(240, 240, 248, 230); p.textFont('"SF Mono", Menlo, monospace'); p.textSize(86); p.textAlign(p.CENTER, p.CENTER); p.text("continue? [y/N]", 540, 920);
      }
      if (between(t, 3, 7)) { p.noStroke(); p.fill(200, 200, 210, 180); p.textFont('"SF Mono", Menlo, monospace'); p.textSize(46); p.textAlign(p.CENTER, p.CENTER); p.text("continue? y", 540, 900); }

      swarm.draw(t, { energy });
      p.pop();

      // End: the promise.
      if (t > at(17.6)) {
        const k = easeOut(clamp((t - at(17.6)) / 0.5));
        p.noStroke(); p.textFont('-apple-system, "SF Pro Display", sans-serif'); p.textAlign(p.CENTER, p.CENTER);
        p.fill(240, 240, 248, 255 * k); p.textSize(60); p.textStyle(p.BOLD); p.text("Leave the desk. Keep the loop.", 540, 1390);
        p.fill(170, 170, 185, 255 * easeOut(clamp((t - at(18)) / 0.5))); p.textSize(40); p.textStyle(p.NORMAL); p.text("looper.fyi", 540, 1470);
      }
    },
  };
}
