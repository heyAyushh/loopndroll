// Stage 3: "Need for Ship" — pseudo-3D sunset racer; overtake the task cars,
// hit LOOPER NITRO, take first place.
import { T, clamp, lerp, prog, backOut, blinkOn, seeded, hash, scalarAt, mixColor, stageBanner } from "./core.js";

const R = T.race;
const [SCENE_START] = T.scenes.race;
const NEAR = 200; // projection depth: p = NEAR / (distance + NEAR)
const DRAW_DISTANCE = 3200;
const SEGMENT = 60;
const RIVAL_SPEED = 150;
const STEP = 1 / 240;
const LAYOUT = {
  landscape: { horizon: 150, roadHalf: 250, bend: 190, car: 2.8, countY: 130, bannerY: 70, carLift: 8 },
  portrait: { horizon: 250, roadHalf: 205, bend: 140, car: 2.5, countY: 250, bannerY: 120, carLift: 76 },
};
const RIVAL_LABELS = ["BUG #412", "PR #88", "TODO", "FLAKY", "LINT", "TYPES", "DEPLOY"];
const RIVAL_COLORS = ["#e8402f", "#3f7ce8", "#f5c400", "#d63fb8", "#2fb62f", "#f28c1a", "#c8ccda"];
const BOARD_TEXT = ["LOOPER", "KEEP GOING", "NO IDLE", "LOOPER"];

function speedAt(t) {
  if (t < R.go) return 0;
  return scalarAt([[R.go, 0], [R.go + 1, 500], [R.nitro, 650], [R.nitro + 0.5, 1100], [R.finish, 1100], [R.finish + 1, 500]], t);
}
const distanceTable = [0];
for (let i = 1; i <= Math.ceil(11 / STEP); i++) distanceTable.push(distanceTable[i - 1] + speedAt(SCENE_START + i * STEP) * STEP);
function distanceAt(t) {
  const f = clamp((t - SCENE_START) / STEP, 0, distanceTable.length - 1);
  const i = Math.floor(f);
  return lerp(distanceTable[i], distanceTable[Math.min(i + 1, distanceTable.length - 1)], f - i);
}
const curveAt = (d) => Math.sin(d * 0.0021) * 1.1 + Math.sin(d * 0.00083 + 1) * 0.7;
const PASSES = [...R.passes, ...R.fastPasses];
const rivals = PASSES.map((tp, i) => ({
  start: distanceAt(tp) - RIVAL_SPEED * (tp - R.go),
  lane: i % 2 ? 0.5 : -0.5,
  label: RIVAL_LABELS[i],
  color: RIVAL_COLORS[i],
}));
const rivalDistance = (rival, t) => rival.start + RIVAL_SPEED * Math.max(0, t - R.go);
const finishLine = distanceAt(R.finish);

export function createRace(format) {
  const L = LAYOUT[format];
  const stars = Array.from({ length: 60 }, (_, i) => [hash(i), hash(i + 99)]);
  const skyline = Array.from({ length: 50 }, (_, i) => ({ x: i * 26, w: 16 + hash(i + 7) * 14, h: 14 + hash(i + 31) * 44 }));

  function project(p, ahead, lateral, position) {
    const k = NEAR / (ahead + NEAR);
    const y = L.horizon + k * (p.H - L.horizon);
    const halfWidth = 4 + k * L.roadHalf;
    const bend = curveAt(position + ahead) * (1 - k) * (1 - k) * L.bend;
    return { k, y, x: p.W / 2 + bend + lateral * halfWidth, halfWidth };
  }
  function drawCar(p, cx, bottom, s, body, cabin) {
    const r = (color, x, y, w, h) => p.rect(color, cx + (x - 20) * s, bottom - (20 - y) * s, w * s, h * s);
    p.alpha(0.45, () => r("#000000", -1, 17, 42, 3));
    r(body, 2, 8, 36, 10);
    r(cabin, 8, 1, 24, 8);
    r("#1b2440", 10, 2, 20, 6);
    r(body, 3, 5, 34, 2);
    r("#ff3b3b", 3, 10, 7, 3);
    r("#ff3b3b", 30, 10, 7, 3);
    r("#f0f0f0", 16, 12, 8, 3);
    r("#111111", 1, 15, 6, 5);
    r("#111111", 33, 15, 6, 5);
  }
  function drawPalm(p, x, bottom, s) {
    p.rect("#6b4a2a", x - 2 * s, bottom - 40 * s, 4 * s, 40 * s);
    for (const [dx, dy, w] of [[-14, -44, 14], [0, -46, 14], [-8, -50, 16], [-18, -40, 8], [10, -40, 8]]) p.rect("#1f8a3a", x + dx * s, bottom + dy * s, w * s, 4 * s);
  }
  function drawBillboard(p, x, bottom, s, label) {
    p.rect("#3a3f55", x - s, bottom - 30 * s, 2 * s, 30 * s);
    p.rect("#101433", x - 26 * s, bottom - 52 * s, 52 * s, 22 * s);
    p.rect("#ffd23f", x - 24 * s, bottom - 50 * s, 48 * s, 18 * s);
    if (p.textWidth(label) <= 46 * s) p.text(label, x, bottom - 45 * s, "#101433", { align: "center" });
  }

  return {
    draw(p, t) {
      const position = distanceAt(t);
      const turn = curveAt(position);
      const H = L.horizon;
      // sky, sun, mountains, skyline
      p.bands(0, H, "#1b0f3a", "#ff9e5a", 10);
      stars.forEach(([x, y]) => p.rect("#ffffff", x * p.W, y * H * 0.55, 1, 1));
      const drift = (position * 0.02 + turn * 30) % 1300;
      const sunX = p.W * 0.62 - turn * 20;
      p.disc("#ffd36a", sunX, H - 38, 34);
      for (let i = 0; i < 5; i++) p.rect(mixColor("#ff9e5a", "#e0576b", i / 4), sunX - 36, H - 30 + i * 6, 72, 1 + i * 0.5);
      const ridge = [];
      for (let x = 0; x <= p.W; x += 8) ridge.push([x, H - 20 - 14 * Math.abs(Math.sin((x + drift * 0.5) * 0.021)) - 9 * Math.abs(Math.sin((x + drift * 0.5) * 0.057))]);
      p.poly("#3a1f5c", [[0, H], ...ridge, [p.W, H]]);
      skyline.forEach((b) => {
        const x = ((b.x - drift + 1300) % 1300) - 60;
        if (x > p.W) return;
        p.rect("#241440", x, H - b.h, b.w, b.h);
        for (let wy = H - b.h + 3; wy < H - 3; wy += 5) {
          for (let wx = x + 2; wx < x + b.w - 2; wx += 4) if (hash(Math.round(wx * 3 + wy + b.x)) > 0.62) p.rect("#ffd23f", wx, wy, 1, 2);
        }
      });
      // road, one scanline at a time
      const finishAhead = finishLine - position;
      for (let y = H; y < p.H; y++) {
        const k = (y - H + 0.5) / (p.H - H);
        const ahead = NEAR / k - NEAR;
        const stripe = Math.floor((ahead + position) / SEGMENT) % 2 === 0;
        const center = p.W / 2 + curveAt(position + ahead) * (1 - k) * (1 - k) * L.bend;
        const halfWidth = 4 + k * L.roadHalf;
        const rumble = halfWidth * 0.1;
        p.rect(stripe ? "#2f7d32" : "#276b2a", 0, y, p.W, 1);
        p.rect(stripe ? "#e8402f" : "#f0f0f0", center - halfWidth - rumble, y, (halfWidth + rumble) * 2, 1);
        p.rect(stripe ? "#5b5b68" : "#55555f", center - halfWidth, y, halfWidth * 2, 1);
        if (stripe) {
          p.rect("#f0f0f0", center - halfWidth / 3 - k * 2, y, Math.max(1, k * 5), 1);
          p.rect("#f0f0f0", center + halfWidth / 3 - k * 2, y, Math.max(1, k * 5), 1);
        }
        if (Math.abs(ahead - finishAhead) < 18) {
          const cells = 16;
          for (let c = 0; c < cells; c++) {
            const odd = (c + Math.floor((ahead - finishAhead + 18) / 12)) % 2;
            p.rect(odd ? "#ffffff" : "#000000", center - halfWidth + (c * halfWidth * 2) / cells, y, (halfWidth * 2) / cells + 1, 1);
          }
        }
      }
      // roadside props + rivals, far to near
      const objects = [];
      for (let d = Math.ceil(position / 240) * 240; d < position + DRAW_DISTANCE; d += 240) {
        const index = Math.round(d / 240);
        objects.push({ ahead: d - position, kind: index % 3 === 0 ? "board" : "palm", side: index % 2 ? 1.5 : -1.5, index });
      }
      rivals.forEach((rival) => {
        const ahead = rivalDistance(rival, t) - position;
        if (ahead > -40 && ahead < DRAW_DISTANCE) objects.push({ ahead, kind: "rival", rival });
      });
      if (finishAhead > 0 && finishAhead < DRAW_DISTANCE) objects.push({ ahead: finishAhead, kind: "gantry" });
      objects.sort((a, b) => b.ahead - a.ahead);
      for (const o of objects) {
        const pr = project(p, Math.max(o.ahead, 1), o.kind === "rival" ? o.rival.lane : o.side || 0, position);
        const s = pr.k * 2.6;
        if (o.kind === "palm") drawPalm(p, pr.x, pr.y, s);
        else if (o.kind === "board") drawBillboard(p, pr.x, pr.y, s, BOARD_TEXT[o.index % BOARD_TEXT.length]);
        else if (o.kind === "gantry") {
          p.rect("#c8ccda", pr.x - pr.halfWidth - 4 * s, pr.y - 60 * s, 3 * s, 60 * s);
          p.rect("#c8ccda", pr.x + pr.halfWidth + s, pr.y - 60 * s, 3 * s, 60 * s);
          p.rect("#101433", pr.x - pr.halfWidth - 4 * s, pr.y - 72 * s, pr.halfWidth * 2 + 8 * s, 13 * s);
          if (s > 0.5) p.text("FINISH", pr.x, pr.y - 69 * s, "#ffd23f", { align: "center" });
        } else {
          const carScale = pr.k * L.car * 0.75;
          drawCar(p, pr.x, pr.y, carScale, o.rival.color, "#2a2f45");
          if (pr.k > 0.2) p.text(o.rival.label, pr.x, pr.y - 30 * carScale - 10, "#ffffff", { align: "center", shadow: "#000000" });
        }
      }
      // player car
      const frame = Math.floor(t * 30);
      const jitter = speedAt(t) > 300 && hash(frame) > 0.5 ? 1 : 0;
      const lean = clamp(-turn * 10, -14, 14);
      const nitroOn = t >= R.nitro && t < R.finish;
      const carBottom = p.H - L.carLift - jitter;
      if (nitroOn) {
        const flame = frame % 2 ? "#ffd23f" : "#36d7ff";
        p.rect(flame, p.W / 2 + lean - 16 * L.car, carBottom - 2, 4 * L.car, (2 + (frame % 3)) * L.car);
        p.rect(flame, p.W / 2 + lean + 12 * L.car, carBottom - 2, 4 * L.car, (2 + ((frame + 1) % 3)) * L.car);
      }
      drawCar(p, p.W / 2 + lean, carBottom, L.car, "#d8202a", "#a8141c");
      p.text("LOOPER", p.W / 2 + lean, carBottom - 7 * L.car, "#ffffff", { align: "center", shadow: "#000000" });
      if (nitroOn) {
        const r = seeded(frame);
        for (let i = 0; i < 26; i++) {
          const a = r() * Math.PI * 2;
          const r0 = p.W * 0.3 + r() * p.W * 0.15;
          const cx = p.W / 2;
          const cy = L.horizon + 20;
          p.line("#ffffff", cx + Math.cos(a) * r0, cy + Math.sin(a) * r0 * 0.7, cx + Math.cos(a) * (r0 + 70), cy + Math.sin(a) * (r0 + 70) * 0.7);
        }
      }
      if (t >= R.nitro && t < R.nitro + 0.2) p.alpha(0.7 * (1 - (t - R.nitro) / 0.2), () => p.clear("#bff4ff"));
      // HUD
      if (t >= R.beeps[0]) {
        const raceTime = Math.max(0, t - R.go);
        p.text("LAP 3/3", 12, 12, "#ffffff", { shadow: "#000000" });
        p.text(`TIME 0:${raceTime.toFixed(2).padStart(5, "0")}`, 12, 24, "#ffffff", { shadow: "#000000" });
        const passed = PASSES.filter((tp) => t >= tp).length;
        p.text("POS", p.W - 12, 12, "#ffffff", { align: "right", shadow: "#000000" });
        p.text(`${8 - passed}/8`, p.W - 12, 24, "#ffd23f", { size: 24, align: "right", shadow: "#b3261e", shadowOffset: 2 });
        p.text(String(Math.round(speedAt(t) * 0.2)), p.W - 12, p.H - 64, "#ffd23f", { font: "VT", size: 48, align: "right", shadow: "#000000", shadowOffset: 2 });
        p.text("MPH", p.W - 12, p.H - 20, "#ffffff", { align: "right", shadow: "#000000" });
        const fill = t < R.nitro ? prog(t, R.go, R.nitro) : 1 - prog(t, R.nitro, R.finish);
        const barW = Math.min(180, p.W * 0.45);
        p.text("LOOPER NITRO", 12, p.H - 34, "#7fe3ff", { shadow: "#000000" });
        p.panel(12, p.H - 22, barW, 12, "#111111", "#ffffff", 2);
        for (let x = 0; x < (barW - 4) * fill; x += 6) p.rect("#36d7ff", 14 + x, p.H - 20, 5, 8);
      }
      // countdown + callouts
      const counts = [...R.beeps, R.go];
      let label = "";
      counts.forEach((tc, i) => {
        if (t >= tc) label = i < R.beeps.length ? String(3 - i) : "GO!";
      });
      if (label && t < R.go + 0.7) {
        const last = counts.filter((tc) => t >= tc).pop();
        const size = Math.round(lerp(96, 48, clamp(backOut(prog(t, last, last + 0.2)), 0, 1)) / 8) * 8;
        p.text(label, p.W / 2, L.countY - size / 2, "#ffd23f", { size, align: "center", shadow: "#b3261e", shadowOffset: 4 });
      }
      if (t >= R.nitro && t < R.nitro + 1.3) {
        const lines = format === "portrait" ? ["NITRO:", "AUTO-CONTINUE!"] : ["NITRO: AUTO-CONTINUE!"];
        lines.forEach((line, i) => p.text(line, p.W / 2, L.countY - 10 + i * 22, "#7fe3ff", { size: 16, align: "center", shadow: "#0a1a4a", shadowOffset: 2 }));
      }
      if (t >= R.finish) {
        p.text("FINISH!", p.W / 2, L.countY - 24, "#ffffff", { size: 32, align: "center", shadow: "#b3261e", shadowOffset: 3 });
        p.text("1ST PLACE", p.W / 2, L.countY + 16, "#ffd23f", { size: 16, align: "center", shadow: "#000000", shadowOffset: 2 });
      }
      stageBanner(p, t, SCENE_START + 0.05, R.beeps[0] - 0.1, "STAGE 3", "NEED FOR SHIP", L.bannerY, format === "portrait" ? 16 : 24);
    },
  };
}
