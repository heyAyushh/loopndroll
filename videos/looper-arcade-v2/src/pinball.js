// Stage 1: "3D Pinball: Looper Cadet".
// A real (deterministic) physics sim drives the ball; the story beats are
// game-state changes layered on top. Every collision is logged so the
// soundtrack can put a sound exactly on it.
import { T, clamp, lerp, prog, blinkOn, backOut, seeded, hash, orb, stageBanner, DEG } from "./core.js";

const PIN = T.pinball;
const [SCENE_START, SCENE_END] = T.scenes.pinball;
const FPS = T.fps;
const SUBSTEPS = 10;
const DT = 1 / (FPS * SUBSTEPS);

// ---------- table geometry (world units, y grows toward the player) ----------
const TABLE_W = 220;
const TABLE_H = 400;
const CENTER_X = 101; // playfield centre (the plunger lane sits right of x=194)
const BALL_R = 5;
const GRAVITY = 560;
const MAX_SPEED = 1150;
const ARCH = { x: 110, y: 104, r: 102 };
const PLUNGER_REST = [203, 386];
const LAUNCH_SPEED = 980;

const walls = [];
const addWall = (a, b, opts = {}) => walls.push({ a, b, e: 0.45, kick: 0, ...opts });
{
  const steps = 26;
  let prev = null;
  for (let i = 0; i <= steps; i++) {
    const a = Math.PI + (Math.PI * i) / steps;
    const pt = [ARCH.x + Math.cos(a) * ARCH.r, ARCH.y + Math.sin(a) * ARCH.r];
    if (prev) addWall(prev, pt, { rail: true });
    prev = pt;
  }
}
addWall([8, 104], [8, 300], { rail: true });
addWall([8, 300], [57, 344], { rail: true });
addWall([212, 104], [212, 400], { rail: true });
addWall([194, 150], [194, 400], { rail: true });
addWall([194, 300], [145, 344], { rail: true });
addWall([194, 392], [212, 392], { plunger: true });
addWall([194, 150], [212, 134], { oneWayDown: true, gate: true });
const LANE_SEPARATORS = [50, 78, 106, 134, 162];
LANE_SEPARATORS.forEach((x) => addWall([x, 30], [x, 50], { post: true, e: 0.3 }));
const LANES = LANE_SEPARATORS.slice(0, -1).map((x, i) => ({ x0: x, x1: LANE_SEPARATORS[i + 1], letter: "LOOP"[i] }));
const LANE_SENSOR_Y = 40;

const SLINGS = [
  { pts: [[22, 256], [22, 292], [48, 306]] },
  { pts: [[180, 256], [180, 292], [154, 306]] },
];
SLINGS.forEach((sling, i) => {
  const [a, b, c] = sling.pts;
  addWall(a, b, { e: 0.3 });
  addWall(b, c, { e: 0.3 });
  addWall(c, a, { e: 0.4, kick: 280, sling: i });
});

const BUMPERS = [
  { x: 62, y: 126, r: 12, label: "BUG", color: "#e8402f", dark: "#8a1f16" },
  { x: 124, y: 112, r: 12, label: "QA", color: "#f28c1a", dark: "#8f4c06" },
  { x: 92, y: 170, r: 12, label: "PR", color: "#d63fb8", dark: "#7a1a66" },
];
const BUMPER_KICK = 330;

const TARGETS = [50, 72, 94, 116].map((x) => ({ x0: x, x1: x + 16, y: 226 }));
TARGETS.forEach((target, i) => addWall([target.x0, target.y], [target.x1, target.y], { target: i, e: 0.35 }));
const BANK_RESET_DELAY = 1.4;

const SAUCER = { x: 100, y: 80, captureSpeed: 520, hold: 0.55 };
const SAUCER_REARM = 0.8; // seconds before an ejected ball can be caught again

const FLIPPER_LENGTH = 30;
const FLIPPER_RADIUS = 4;
const FLIPPERS = [
  { x: 62, y: 348, rest: 30 * DEG, up: -28 * DEG, side: -1 },
  { x: 140, y: 348, rest: 150 * DEG, up: 208 * DEG, side: 1 },
];
const FLIP_UP_RATE = 26; // rad/s
const FLIP_DOWN_RATE = 14;
const FLIP_HOLD = 0.2;
const FLIP_COOLDOWN = 0.16;

export const TASKS = [
  "FIX BUG #412", "WRITE TESTS", "REVIEW PR", "LINT", "TYPECHECK",
  "BUILD", "E2E SUITE", "UPDATE DOCS", "REFACTOR", "SHIP IT",
];
const FINAL_SCORE = 9999990;
const MAJOR_EVENTS = new Set(["target", "lane", "saucerIn", "bank", "loop"]);
const TASK_FIRST_DEADLINE = 8.0;
const TASK_DEADLINE_STEP = 0.95;
const TASK_MIN_GAP = 0.3;
const POINTS = { bumper: 1000, sling: 250, target: 2500, bank: 15000, lane: 1000, loop: 25000, saucerIn: 10000 };

// ---------- simulation ----------
function closestOnSegment(px, py, a, b) {
  const abx = b[0] - a[0];
  const aby = b[1] - a[1];
  const u = clamp(((px - a[0]) * abx + (py - a[1]) * aby) / (abx * abx + aby * aby), 0, 1);
  return [a[0] + abx * u, a[1] + aby * u, u];
}

let cached = null;
export function simulatePinball() {
  if (cached) return cached;
  const events = [];
  const frames = [];
  const balls = [];
  const flippers = FLIPPERS.map((f) => ({ ...f, angle: f.rest, omega: 0, holdUntil: -1, readyAt: 0 }));
  const targetDown = TARGETS.map(() => false);
  let bankResetAt = Infinity;
  const laneLit = LANES.map(() => false);
  let laneResetAt = Infinity;
  const bumperCooldown = BUMPERS.map(() => -1);
  const slingCooldown = SLINGS.map(() => -1);
  let score = 0;
  let tasksDone = 0;
  let lastTaskAt = -1;
  let message = { text: "PULL THE PLUNGER", color: "#ffd23f", t: SCENE_START };
  let flippersOn = true;
  let magnetOn = false;
  let stopAt = Infinity;
  let storyDrainAt = null;
  let multiball = false;
  let jackpotDone = false;
  const pendingLaunches = [];

  const emit = (t, type, x, y, extra = {}) => {
    // The jackpot tops the score up to FINAL_SCORE; nothing scores after it,
    // so the pinball total matches the finale's high-score table.
    const pts = POINTS[type] && !jackpotDone ? POINTS[type] * (multiball ? 2 : 1) : 0;
    events.push({ t: +t.toFixed(4), type, x, y, pts, ...extra });
    score += pts;
    if (["bumper", "sling", ...MAJOR_EVENTS].includes(type) && tasksDone < TASKS.length - 1) {
      const deadline = TASK_FIRST_DEADLINE + tasksDone * TASK_DEADLINE_STEP;
      if ((MAJOR_EVENTS.has(type) || t >= deadline) && t - lastTaskAt >= TASK_MIN_GAP && t >= PIN.launch + 0.4) {
        events.push({ t: +t.toFixed(4), type: "task", x, y, task: tasksDone });
        message = { text: `SMASHED: ${TASKS[tasksDone]}`, color: "#7dff6a", t };
        tasksDone += 1;
        lastTaskAt = t;
      }
    }
  };
  // After the jackpot the panel keeps "JACKPOT! SHIPPED" up; later hits don't overwrite it.
  const say = (t, text, color) => {
    if (!jackpotDone || text.startsWith("JACKPOT")) message = { text, color, t };
  };

  const spawnBall = (t, launchAt, speed) => {
    const ball = { id: balls.length, x: PLUNGER_REST[0], y: PLUNGER_REST[1], vx: 0, vy: 0, held: true, launchAt, speed, active: true, capturedUntil: -1, born: t };
    balls.push(ball);
    return ball;
  };
  spawnBall(SCENE_START, PIN.launch, LAUNCH_SPEED);

  const totalSteps = Math.round((SCENE_END - SCENE_START) * FPS * SUBSTEPS);
  for (let step = 0; step <= totalSteps; step++) {
    const t = SCENE_START + step * DT;

    // ----- story director -----
    if (t >= PIN.agentStop && stopAt === Infinity) {
      stopAt = t;
      flippersOn = false;
      magnetOn = true;
      events.push({ t, type: "agentStop" });
      say(t, "AGENT STOPPED...", "#ff4a3d");
    }
    for (let i = pendingLaunches.length - 1; i >= 0; i--) {
      const pending = pendingLaunches[i];
      if (t >= pending.at && !balls.some((b) => b.active && b.held)) {
        spawnBall(t, t + 0.25, pending.speed);
        pendingLaunches.splice(i, 1);
      }
    }
    if (storyDrainAt != null && !multiball && t >= storyDrainAt + 0.35 && !events.some((e) => e.type === "save")) {
      events.push({ t, type: "save" });
      say(t, "LOOPER SAVE!", "#7dff6a");
    }
    if (storyDrainAt != null && !multiball && t >= storyDrainAt + 0.8) {
      multiball = true;
      flippersOn = true;
      magnetOn = false;
      events.push({ t, type: "kickback" });
      say(t, "MULTIBALL! KEEP GOING", "#7fe3ff");
      spawnBall(t, t + 0.05, LAUNCH_SPEED);
      pendingLaunches.push({ at: t + 0.35, speed: 940 }, { at: t + 0.7, speed: 1010 });
    }
    if (!jackpotDone && t >= PIN.jackpot) {
      jackpotDone = true;
      events.push({ t, type: "task", task: TASKS.length - 1, x: SAUCER.x, y: SAUCER.y });
      tasksDone = TASKS.length;
      const topUp = FINAL_SCORE - score;
      events.push({ t, type: "jackpot", x: SAUCER.x, y: SAUCER.y, pts: topUp });
      score = FINAL_SCORE;
      say(t, "JACKPOT! SHIPPED", "#ffd23f");
    }
    if (t >= bankResetAt && balls.every((b) => !b.active || Math.abs(b.y - TARGETS[0].y) > 12)) {
      targetDown.fill(false);
      bankResetAt = Infinity;
    }
    if (t >= laneResetAt) {
      laneLit.fill(false);
      laneResetAt = Infinity;
    }

    // ----- flippers: autoplay AI + kinematics -----
    flippers.forEach((flipper) => {
      if (flippersOn && t >= flipper.readyAt && t > flipper.holdUntil) {
        for (const ball of balls) {
          if (!ball.active || ball.held || t < ball.capturedUntil) continue;
          const dx = ball.x - flipper.x;
          const dy = ball.y - flipper.y;
          const onSide = flipper.side < 0 ? ball.x < CENTER_X + 4 : ball.x > CENTER_X - 4;
          const reach = Math.abs(dx) / FLIPPER_LENGTH;
          if (onSide && ball.y > 322 && ball.y < 362 && reach > 0.3 && reach < 1.05 && dy > -22 && ball.vy > -80) {
            flipper.holdUntil = t + FLIP_HOLD;
            flipper.readyAt = t + FLIP_HOLD + FLIP_COOLDOWN;
            events.push({ t, type: "flip", side: flipper.side });
            break;
          }
        }
      }
      const target = flippersOn && t <= flipper.holdUntil ? flipper.up : flipper.rest;
      const rate = target === flipper.up ? FLIP_UP_RATE : FLIP_DOWN_RATE;
      const delta = target - flipper.angle;
      const move = clamp(delta, -rate * DT, rate * DT);
      flipper.omega = move / DT;
      flipper.angle += move;
    });

    // ----- balls -----
    for (const ball of balls) {
      if (!ball.active) continue;
      if (ball.held) {
        ball.x = PLUNGER_REST[0];
        ball.y = PLUNGER_REST[1];
        if (t >= ball.launchAt) {
          ball.held = false;
          ball.vy = -ball.speed;
          events.push({ t, type: "launch", x: ball.x, y: ball.y });
          if (message.text === "PULL THE PLUNGER") say(t, "AGENT LAUNCHED", "#7fe3ff");
        }
        continue;
      }
      if (t < ball.capturedUntil) {
        ball.x = SAUCER.x;
        ball.y = SAUCER.y;
        continue;
      }
      if (ball.capturedUntil > 0 && ball.vx === 0 && ball.vy === 0) {
        ball.vx = -180;
        ball.vy = 420;
        events.push({ t, type: "saucerOut", x: ball.x, y: ball.y });
        ball.capturedUntil = -1;
        ball.saucerIgnoreUntil = t + SAUCER_REARM;
      }
      ball.vy += GRAVITY * DT;
      if (magnetOn) {
        const pull = 350 + 700 * Math.max(0, t - stopAt);
        const mx = CENTER_X - ball.x;
        const my = 430 - ball.y;
        const len = Math.hypot(mx, my) || 1;
        ball.vx += (mx / len) * pull * DT;
        ball.vy += (my / len) * pull * DT;
      }
      const speed = Math.hypot(ball.vx, ball.vy);
      if (speed > MAX_SPEED) {
        ball.vx *= MAX_SPEED / speed;
        ball.vy *= MAX_SPEED / speed;
      }
      ball.x += ball.vx * DT;
      ball.y += ball.vy * DT;

      // static + one-way walls
      for (const wall of walls) {
        if (wall.target != null && targetDown[wall.target]) continue;
        if (wall.oneWayDown && ball.vy <= 0) continue;
        const [qx, qy] = closestOnSegment(ball.x, ball.y, wall.a, wall.b);
        let nx = ball.x - qx;
        let ny = ball.y - qy;
        const dist = Math.hypot(nx, ny);
        if (dist >= BALL_R || dist === 0) continue;
        nx /= dist;
        ny /= dist;
        ball.x += nx * (BALL_R - dist);
        ball.y += ny * (BALL_R - dist);
        const vn = ball.vx * nx + ball.vy * ny;
        if (vn < 0) {
          ball.vx -= (1 + wall.e) * vn * nx;
          ball.vy -= (1 + wall.e) * vn * ny;
          if (wall.kick && -vn > 40 && t - slingCooldown[wall.sling] > 0.12) {
            ball.vx += nx * wall.kick;
            ball.vy += ny * wall.kick;
            slingCooldown[wall.sling] = t;
            emit(t, "sling", qx, qy, { i: wall.sling });
          }
          if (wall.target != null && !targetDown[wall.target]) {
            targetDown[wall.target] = true;
            emit(t, "target", qx, qy, { i: wall.target });
            if (targetDown.every(Boolean)) {
              emit(t, "bank", 88, 226);
              say(t, "TARGET BANK CLEARED!", "#ffd23f");
              bankResetAt = t + BANK_RESET_DELAY;
            }
          }
          if (wall.rail && -vn > 450) events.push({ t, type: "rail", x: qx, y: qy });
        }
      }
      // flippers (moving capsules)
      for (const flipper of flippers) {
        const tip = [flipper.x + Math.cos(flipper.angle) * FLIPPER_LENGTH, flipper.y + Math.sin(flipper.angle) * FLIPPER_LENGTH];
        const [qx, qy] = closestOnSegment(ball.x, ball.y, [flipper.x, flipper.y], tip);
        let nx = ball.x - qx;
        let ny = ball.y - qy;
        const dist = Math.hypot(nx, ny);
        const reachR = BALL_R + FLIPPER_RADIUS;
        if (dist >= reachR || dist === 0) continue;
        nx /= dist;
        ny /= dist;
        ball.x += nx * (reachR - dist);
        ball.y += ny * (reachR - dist);
        const rx = qx - flipper.x;
        const ry = qy - flipper.y;
        const cvx = -flipper.omega * ry;
        const cvy = flipper.omega * rx;
        const rvn = (ball.vx - cvx) * nx + (ball.vy - cvy) * ny;
        if (rvn < 0) {
          const e = Math.abs(flipper.omega) > 1 ? 0.55 : 0.25;
          ball.vx -= (1 + e) * rvn * nx;
          ball.vy -= (1 + e) * rvn * ny;
        }
      }
      // pop bumpers
      BUMPERS.forEach((bumper, i) => {
        let nx = ball.x - bumper.x;
        let ny = ball.y - bumper.y;
        const dist = Math.hypot(nx, ny);
        const reachR = BALL_R + bumper.r;
        if (dist >= reachR || dist === 0) return;
        nx /= dist;
        ny /= dist;
        ball.x = bumper.x + nx * reachR;
        ball.y = bumper.y + ny * reachR;
        const vn = ball.vx * nx + ball.vy * ny;
        if (vn < 0) {
          ball.vx -= 1.6 * vn * nx;
          ball.vy -= 1.6 * vn * ny;
        }
        if (t - bumperCooldown[i] > 0.07) {
          ball.vx += nx * BUMPER_KICK;
          ball.vy += ny * BUMPER_KICK;
          bumperCooldown[i] = t;
          emit(t, "bumper", bumper.x, bumper.y, { i });
        }
      });
      // rollover lanes
      LANES.forEach((lane, i) => {
        const prevY = ball.y - ball.vy * DT;
        if (ball.x > lane.x0 && ball.x < lane.x1 && (prevY - LANE_SENSOR_Y) * (ball.y - LANE_SENSOR_Y) <= 0 && !laneLit[i]) {
          laneLit[i] = true;
          emit(t, "lane", (lane.x0 + lane.x1) / 2, LANE_SENSOR_Y, { i });
          if (laneLit.every(Boolean)) {
            emit(t, "loop", 106, 50);
            say(t, "L-O-O-P LIT! BONUS", "#ffd23f");
            laneResetAt = t + 1.0;
          }
        }
      });
      // saucer
      const sd = Math.hypot(ball.x - SAUCER.x, ball.y - SAUCER.y);
      if (sd < 4 && Math.hypot(ball.vx, ball.vy) < SAUCER.captureSpeed && ball.capturedUntil < 0 && t > PIN.launch + 0.5 && t > (ball.saucerIgnoreUntil || 0)) {
        ball.capturedUntil = t + SAUCER.hold;
        ball.vx = 0;
        ball.vy = 0;
        emit(t, "saucerIn", SAUCER.x, SAUCER.y);
      }
      // stuck guard: a real player would nudge the cabinet
      if (Math.hypot(ball.vx, ball.vy) < 15 && ball.y < 320 && !ball.held) {
        ball.stillFor = (ball.stillFor || 0) + DT;
        if (ball.stillFor > 0.8) {
          ball.vx += (ball.x < CENTER_X ? 1 : -1) * 90;
          ball.vy -= 160;
          ball.stillFor = 0;
          events.push({ t, type: "nudge" });
        }
      } else ball.stillFor = 0;
      // drain
      if (ball.y > 412) {
        ball.active = false;
        events.push({ t, type: "drain", x: ball.x, y: 400 });
        if (t >= stopAt && storyDrainAt == null) storyDrainAt = t;
        else if (t < stopAt || multiball) {
          pendingLaunches.push({ at: t + 0.55, speed: 960 });
          if (!multiball) say(t, "BALL SAVED", "#7dff6a");
        }
      }
    }
    // ball-ball contacts during multiball
    for (let i = 0; i < balls.length; i++) {
      for (let j = i + 1; j < balls.length; j++) {
        const a = balls[i];
        const b = balls[j];
        if (!a.active || !b.active || a.held || b.held) continue;
        let nx = b.x - a.x;
        let ny = b.y - a.y;
        const dist = Math.hypot(nx, ny);
        if (dist >= BALL_R * 2 || dist === 0) continue;
        nx /= dist;
        ny /= dist;
        const overlap = (BALL_R * 2 - dist) / 2;
        a.x -= nx * overlap;
        a.y -= ny * overlap;
        b.x += nx * overlap;
        b.y += ny * overlap;
        const rel = (b.vx - a.vx) * nx + (b.vy - a.vy) * ny;
        if (rel < 0) {
          a.vx += rel * nx;
          a.vy += rel * ny;
          b.vx -= rel * nx;
          b.vy -= rel * ny;
          events.push({ t, type: "clack", x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 });
        }
      }
    }

    if (step % SUBSTEPS === 0) {
      frames.push({
        t,
        balls: balls.filter((b) => b.active).map((b) => ({ id: b.id, x: b.x, y: b.y, held: b.held })),
        flippers: flippers.map((f) => f.angle),
        targetDown: [...targetDown],
        laneLit: [...laneLit],
        score,
        tasksDone,
        message: { ...message },
        flippersOn,
        multiball,
      });
    }
  }
  cached = { frames, events, storyDrainAt };
  return cached;
}

// ---------- rendering ----------
const LAYOUTS = {
  landscape: { cx: 180, baseY: 354, S: 1.25, SY: 1.0, persp: 0.45, ballScale: 1 },
  portrait: { cx: 180, baseY: 634, S: 1.5, SY: 1.2, persp: 0.45, ballScale: 1 },
};
const RAINBOW = ["#ffd23f", "#ff4a3d", "#7dff6a", "#7fe3ff", "#d63fb8"];

export function createPinball(format) {
  const L = LAYOUTS[format];
  const sim = simulatePinball();
  const stars = Array.from({ length: 110 }, (_, i) => [hash(i + 1) * TABLE_W, hash(i + 301) * TABLE_H]);
  const kickback = sim.events.find((e) => e.type === "kickback")?.t ?? Infinity;
  const saveAt = sim.events.find((e) => e.type === "save")?.t ?? Infinity;

  let shake = [0, 0];
  const P = (x, y, z = 0) => {
    const depth = (TABLE_H - y) / TABLE_H;
    const k = 1 / (1 + depth * L.persp);
    return [L.cx + shake[0] + (x - TABLE_W / 2) * k * L.S, L.baseY + shake[1] - (TABLE_H - y) * k * L.S * L.SY - z * k * L.S, k];
  };
  // Cabinet shake: a tick on every pop bumper, a real jolt on kickback and jackpot.
  function shakeAt(t) {
    let amount = 0;
    for (const e of sim.events) {
      if (e.t > t) break;
      const age = t - e.t;
      if (e.type === "bumper" && age < 0.05) amount = Math.max(amount, 1);
      if (e.type === "kickback" && age < 0.3) amount = Math.max(amount, 2 * (1 - age / 0.3) + 0.5);
      if (e.type === "jackpot" && age < 0.6) amount = Math.max(amount, 4 * (1 - age / 0.6) + 0.5);
    }
    if (amount === 0) return [0, 0];
    const frame = Math.floor(t * FPS);
    return [Math.round((hash(frame) - 0.5) * 2 * amount), Math.round((hash(frame + 911) - 0.5) * 2 * amount)];
  }
  const scaleAt = (y) => P(0, y)[2] * L.S;

  function frameAt(t) {
    const i = clamp(Math.round((t - SCENE_START) * FPS), 0, sim.frames.length - 1);
    return sim.frames[i];
  }
  const lastEventBefore = (type, t, match = () => true) => {
    let last = null;
    for (const e of sim.events) {
      if (e.t > t) break;
      if (e.type === type && match(e)) last = e;
    }
    return last;
  };

  function rail(p, a, b, height = 6, face = "#4b5268", top = "#c9d0de") {
    const a0 = P(a[0], a[1]);
    const b0 = P(b[0], b[1]);
    const a1 = P(a[0], a[1], height);
    const b1 = P(b[0], b[1], height);
    p.poly(face, [[a0[0], a0[1]], [b0[0], b0[1]], [b1[0], b1[1]], [a1[0], a1[1]]]);
    p.line(top, a1[0], a1[1], b1[0], b1[1], format === "portrait" ? 2 : 1);
  }

  function drawPlayfield(p, t, f) {
    // cabinet rim + playfield
    const outline = [];
    const rimOutline = [];
    for (let i = 0; i <= 30; i++) {
      const a = Math.PI + (Math.PI * i) / 30;
      const [x, y] = P(ARCH.x + Math.cos(a) * (ARCH.r + 8), ARCH.y + Math.sin(a) * (ARCH.r + 8));
      outline.push([x, y]);
      const [rx, ry] = P(ARCH.x + Math.cos(a) * (ARCH.r + 18), ARCH.y + Math.sin(a) * (ARCH.r + 18));
      rimOutline.push([rx, ry]);
    }
    const bl = P(0, TABLE_H);
    const br = P(TABLE_W, TABLE_H);
    const rbl = P(-10, TABLE_H);
    const rbr = P(TABLE_W + 10, TABLE_H);
    p.poly("#2b3148", [[rbl[0], rbl[1] + 6], ...rimOutline, [rbr[0], rbr[1] + 6]]);
    p.poly("#8f98ad", [[bl[0] - 3, bl[1]], ...outline.map(([x, y]) => [x, y - 2]), [br[0] + 3, br[1]]]);
    p.poly("#0b1646", [[bl[0], bl[1]], ...outline, [br[0], br[1]]]);
    // painted art: nebula, grid, stars, planet
    for (let i = 0; i < 5; i++) {
      const [nx, ny, nk] = P(60 + i * 12, 250 - i * 18);
      p.alpha(0.5, () => p.ellipse(i % 2 ? "#23155c" : "#2b1f74", nx, ny, (46 - i * 5) * nk * L.S, (30 - i * 3) * nk * L.S * L.SY));
    }
    for (let gy = 110; gy <= TABLE_H; gy += 20) {
      const a = P(8, gy);
      const b = P(194, gy);
      p.line("#13235e", a[0], a[1], b[0], b[1]);
    }
    for (let gx = 20; gx <= 190; gx += 20) {
      const a = P(gx, 110);
      const b = P(gx, TABLE_H);
      p.line("#13235e", a[0], a[1], b[0], b[1]);
    }
    stars.forEach(([x, y], i) => {
      if (x > 192) return;
      const [sx, sy] = P(x, y);
      p.rect(blinkOn(t + i * 0.37, 0.9) ? "#ffffff" : "#56609a", sx, sy, 1, 1);
    });
    const [px, py, pk] = P(150, 262);
    for (let i = 0; i < 6; i++) p.ellipse(i % 2 ? "#8b5cd6" : "#6a3fb5", px, py, (24 - i * 3.5) * pk * L.S, (24 - i * 3.5) * pk * L.S * L.SY * 0.8);
    p.alpha(0.8, () => p.ellipse("#c9b3f2", px, py, 34 * pk * L.S, 3 * pk * L.S));

    // inserts (lamps)
    const insert = (x, y, r, lit, on, off) => {
      const [ix, iy, ik] = P(x, y);
      p.ellipse(lit ? on : off, ix, iy, r * ik * L.S, r * ik * L.S * L.SY * 0.7);
      if (lit) p.alpha(0.35, () => p.ellipse(on, ix, iy, (r + 3) * ik * L.S, (r + 3) * ik * L.S * L.SY * 0.7));
    };
    // chase lights: a ring under the arch and strips down both rails, like attract mode
    const celebrating = t < PIN.launch || t >= PIN.jackpot;
    const chaseStep = Math.floor(t * (celebrating ? 18 : 9));
    const ARCH_LAMPS = 24;
    for (let i = 0; i < ARCH_LAMPS; i++) {
      const a = Math.PI + (Math.PI * (i + 0.5)) / ARCH_LAMPS;
      const lit = celebrating ? (i + chaseStep) % 3 === 0 : (i - chaseStep) % 8 === 0 || (i - chaseStep) % 8 === -8;
      insert(ARCH.x + Math.cos(a) * (ARCH.r - 9), ARCH.y + Math.sin(a) * (ARCH.r - 9), 1.6, lit, i % 2 ? "#ff4a3d" : "#ffd23f", "#2a2140");
    }
    for (let i = 0; i < 9; i++) {
      const y = 132 + i * 18;
      const lit = (i + chaseStep) % 4 === 0;
      insert(15, y, 1.8, lit, "#7fe3ff", "#12305a");
      insert(187, y, 1.8, lit, "#7fe3ff", "#12305a");
    }
    // chevrons pointing up at the bumper cluster, rippling toward it
    for (let i = 0; i < 3; i++) {
      const y = 214 - i * 12;
      const lit = t >= PIN.launch && (chaseStep - i) % 3 === 0;
      const c = [P(CENTER_X - 9, y + 4), P(CENTER_X, y - 3), P(CENTER_X + 9, y + 4), P(CENTER_X, y)];
      p.poly(lit ? "#ff8a1f" : "#3a2410", c.map(([x, yy]) => [x, yy]));
    }
    // orbit ring decal around the bumpers
    for (let i = 0; i < 48; i += 2) {
      const a0 = (Math.PI * 2 * i) / 48;
      const a1 = (Math.PI * 2 * (i + 1)) / 48;
      const q0 = P(94 + Math.cos(a0) * 58, 140 + Math.sin(a0) * 46);
      const q1 = P(94 + Math.cos(a1) * 58, 140 + Math.sin(a1) * 46);
      p.line("#1e3a80", q0[0], q0[1], q1[0], q1[1]);
    }
    TARGETS.forEach((target, i) => insert((target.x0 + target.x1) / 2, target.y + 12, 3, f.targetDown[i], "#ffd23f", "#3a3410"));
    LANES.forEach((lane, i) => {
      const lit = f.laneLit[i] || (t >= PIN.jackpot && blinkOn(t + i * 0.1, 5));
      insert((lane.x0 + lane.x1) / 2, 64, 6, lit, "#ffd23f", "#2a3470");
      const [lx, ly] = P((lane.x0 + lane.x1) / 2, 64);
      p.text(lane.letter, lx, ly - 4, lit ? "#b3261e" : "#56609a", { align: "center" });
    });
    const jackpotLamp = t >= PIN.jackpot - 1.5 && blinkOn(t, t >= PIN.jackpot ? 6 : 3);
    const a1 = P(106, 104);
    const a2 = P(94, 118);
    const a3 = P(118, 118);
    p.poly(jackpotLamp ? "#ffd23f" : "#3a3410", [[a1[0], a1[1]], [a2[0], a2[1]], [a3[0], a3[1]]]);
    insert(CENTER_X, 300, 7, f.multiball && blinkOn(t, 2), "#7fe3ff", "#12305a");
    {
      const [xx, xy] = P(CENTER_X, 300);
      p.text("x2", xx, xy - 4, f.multiball ? "#0a1a4a" : "#2c4a7a", { align: "center" });
    }
    const saveLit = t >= saveAt && t < kickback + 0.6 && blinkOn(t, 5);
    insert(CENTER_X, 376, 9, saveLit, "#7dff6a", "#12301a");
    {
      const [sx, sy] = P(CENTER_X, 376);
      p.text("SAVE", sx, sy - 4, saveLit ? "#0b3b12" : "#2f5a37", { align: "center" });
    }
    // saucer
    const [ux, uy, uk] = P(SAUCER.x, SAUCER.y);
    p.ellipse("#8f98ad", ux, uy, 9 * uk * L.S, 9 * uk * L.S * L.SY * 0.6);
    p.ellipse("#02030d", ux, uy, 6 * uk * L.S, 6 * uk * L.S * L.SY * 0.6);
  }

  function drawBumper(p, t, bumper, i) {
    const hit = lastEventBefore("bumper", t, (e) => e.i === i);
    const since = hit ? t - hit.t : 9;
    const flash = since < 0.12;
    const [bx, by, k] = P(bumper.x, bumper.y);
    const rx = bumper.r * k * L.S;
    const ry = rx * L.SY * 0.6;
    const h = 11 * k * L.S;
    p.ellipse("#05081c", bx + 2, by + 2, rx + 1, ry + 1);
    p.rect(bumper.dark, bx - rx, by - h, rx * 2, h);
    p.ellipse(bumper.dark, bx, by, rx, ry);
    if (flash) p.alpha(0.6, () => p.ellipse("#ffd23f", bx, by - h, rx + 5, ry + 4));
    p.ellipse(flash ? "#ffffff" : bumper.color, bx, by - h, rx, ry);
    p.ellipse(flash ? "#ffd23f" : "#f4f1e8", bx, by - h, rx * 0.62, ry * 0.62);
    p.text(bumper.label, bx, by - h - 4, "#1a1f3a", { align: "center" });
  }

  function drawFlipper(p, angle, flipper, powered) {
    const tipX = flipper.x + Math.cos(angle) * FLIPPER_LENGTH;
    const tipY = flipper.y + Math.sin(angle) * FLIPPER_LENGTH;
    const nx = -Math.sin(angle);
    const ny = Math.cos(angle);
    const shape = (grow, z) => {
      const pts = [
        P(flipper.x + nx * (5 + grow), flipper.y + ny * (5 + grow), z),
        P(tipX + nx * (2.5 + grow), tipY + ny * (2.5 + grow), z),
        P(tipX - nx * (2.5 + grow), tipY - ny * (2.5 + grow), z),
        P(flipper.x - nx * (5 + grow), flipper.y - ny * (5 + grow), z),
      ];
      return pts.map(([x, y]) => [x, y]);
    };
    p.poly("#05081c", shape(1, 0).map(([x, y]) => [x + 2, y + 2]));
    p.poly(powered ? "#b3261e" : "#5a2a2a", shape(1.2, 4));
    p.poly(powered ? "#f4f1e8" : "#8a8f9e", shape(0, 5));
    const [pvx, pvy, pk] = P(flipper.x, flipper.y, 5);
    p.ellipse("#c9d0de", pvx, pvy, 2.5 * pk * L.S, 2 * pk * L.S);
  }

  function drawBall(p, x, y, t, trail) {
    const [sx, sy, k] = P(x, y, BALL_R);
    const size = BALL_R * 2 * k * L.S;
    const [shx, shy] = P(x + 2, y + 2);
    p.alpha(0.5, () => p.ellipse("#000000", shx, shy, size * 0.5, size * 0.3));
    trail.forEach(([tx, ty], j) => {
      const [txs, tys] = P(tx, ty, BALL_R);
      p.alpha(0.15 * (trail.length - j), () => p.image(orb(size), txs - size / 2, tys - size / 2));
    });
    p.image(orb(size), sx - size / 2, sy - size / 2);
  }

  function drawEffects(p, t) {
    for (const e of sim.events) {
      if (e.t > t) break;
      const age = t - e.t;
      if (age > 0.7) continue;
      if (["bumper", "sling", "target", "lane", "jackpot", "bank", "saucerIn", "loop"].includes(e.type)) {
        const [ex, ey, k] = P(e.x, e.y, 10);
        const r = seeded(Math.round(e.t * 1000));
        const count = e.type === "jackpot" ? 40 : 10;
        if (age < 0.4) {
          for (let s = 0; s < count; s++) {
            const a = r() * Math.PI * 2;
            const v = (40 + r() * 70) * k * L.S * (e.type === "jackpot" ? 2.2 : 1);
            const sx = ex + Math.cos(a) * v * age;
            const sy = ey + Math.sin(a) * v * age * 0.7 + 60 * age * age;
            p.rect(s % 3 ? "#ffd23f" : "#ffffff", sx, sy, age < 0.2 ? 2 : 1, age < 0.2 ? 2 : 1);
          }
        }
        // Popups show what the hit actually scored (x2 in multiball, nothing after the jackpot).
        const word = { bank: "BANK!", loop: "LOOP!" }[e.type];
        const label = word || (e.type !== "sling" && e.type !== "jackpot" && e.pts > 0 ? `+${e.pts.toLocaleString("en-US")}` : null);
        if (label && age < 0.6) p.text(label, ex, ey - 14 - age * 26, age < 0.3 || blinkOn(t, 8) ? "#ffd23f" : "#ffffff", { align: "center", shadow: "#000000" });
      }
    }
  }

  function drawPanelLandscape(p, t, f) {
    const x0 = 344;
    p.panel(x0, 10, 288, 340, "#04050f", "#b9c0cf", 3);
    p.rect("#3a4058", x0 + 3, 13, 282, 1);
    p.image(orb(32), x0 + 12, 22);
    p.text("3D PINBALL", x0 + 52, 24, "#7fe3ff");
    p.text("LOOPER CADET", x0 + 52, 36, "#ffd23f", { size: 16, shadow: "#b3261e" });
    p.panel(x0 + 12, 64, 264, 84, "#0b1a0b", "#2c4a2c", 2);
    p.text("BALL", x0 + 20, 68, "#7dff6a", { font: "VT", size: 16 });
    p.text(f.multiball ? "1+2" : "1", x0 + 268, 68, "#7dff6a", { font: "VT", size: 16, align: "right" });
    p.text("PLAYER", x0 + 20, 82, "#7dff6a", { font: "VT", size: 16 });
    p.text("1", x0 + 268, 82, "#7dff6a", { font: "VT", size: 16, align: "right" });
    p.text(displayScore(t, f), x0 + 268, 98, "#b8ff9e", { font: "VT", size: 48, align: "right" });
    const m = f.message;
    if (t - m.t > 0.25 || blinkOn(t, 8)) p.text(m.text, x0 + 12, 160, m.color);
    TASKS.forEach((task, i) => {
      const done = i < f.tasksDone;
      const y = 178 + i * 16;
      const color = done ? "#7dff6a" : "#8a91a8";
      const w = p.text(`${done ? "[x]" : "[ ]"} ${task}`, x0 + 14, y, color, { font: "VT", size: 16 });
      if (done) p.rect(color, x0 + 14, y + 9, w, 1);
      const justDone = sim.events.find((e) => e.type === "task" && e.task === i);
      if (justDone && t >= justDone.t && t - justDone.t < 0.35 && blinkOn(t, 10)) p.rect("#ffd23f", x0 + 8, y + 2, 4, 8);
    });
  }

  function drawHudPortrait(p, t, f) {
    p.rect("#04050f", 0, 0, 360, 132);
    p.rect("#b9c0cf", 0, 130, 360, 2);
    p.image(orb(24), 10, 8);
    p.text("3D PINBALL", 42, 9, "#7fe3ff");
    p.text("LOOPER CADET", 42, 20, "#ffd23f", { size: 16, shadow: "#b3261e" });
    p.panel(8, 42, 344, 54, "#0b1a0b", "#2c4a2c", 2);
    p.text(`BALL ${f.multiball ? "1+2" : "1"}`, 16, 48, "#7dff6a", { font: "VT", size: 16 });
    p.text("PLAYER 1", 16, 64, "#7dff6a", { font: "VT", size: 16 });
    p.text(displayScore(t, f), 344, 48, "#b8ff9e", { font: "VT", size: 48, align: "right" });
    const m = f.message;
    if (t - m.t > 0.25 || blinkOn(t, 8)) p.text(m.text, 180, 102, m.color, { align: "center" });
    p.text(`TASKS ${f.tasksDone}/${TASKS.length}`, 10, 116, "#c8ccda");
    for (let i = 0; i < TASKS.length; i++) p.rect(i < f.tasksDone ? "#7dff6a" : "#1f2a3a", 120 + i * 23, 115, 20, 9);
  }

  const SCORE_ROLL = 0.35; // seconds for the LCD digits to count up to a new total
  function displayScore(t, f) {
    let rolling = 0;
    for (const e of sim.events) {
      if (e.t > t) break;
      const age = t - e.t;
      if (e.pts && age < SCORE_ROLL) rolling += e.pts * (1 - age / SCORE_ROLL);
    }
    return (Math.round((f.score - rolling) / 10) * 10).toLocaleString("en-US");
  }

  return {
    draw(p, t) {
      const f = frameAt(t);
      shake = shakeAt(t);
      p.clear("#03051a");
      // backdrop
      p.bands(0, p.H, "#060a26", "#010108", 6);
      if (format === "landscape") {
        for (let i = 0; i < 40; i++) p.rect(blinkOn(t + i * 0.3, 0.8) ? "#ffffff" : "#34407a", hash(i + 7) * 340, hash(i + 70) * 360, 1, 1);
      }
      drawPlayfield(p, t, f);
      // rails, posts, gate
      for (const wall of walls) {
        if (wall.target != null || wall.plunger) continue;
        if (wall.post) rail(p, wall.a, wall.b, 8, "#6b7390", "#ffffff");
        else if (wall.gate) rail(p, wall.a, wall.b, 4, "#3a4058", "#c9d0de");
        else if (wall.rail) rail(p, wall.a, wall.b);
      }
      // slingshots
      SLINGS.forEach((sling, i) => {
        const hit = lastEventBefore("sling", t, (e) => e.i === i);
        const flash = hit && t - hit.t < 0.1;
        const base = sling.pts.map(([x, y]) => P(x, y, 0)).map(([x, y]) => [x, y]);
        const top = sling.pts.map(([x, y]) => P(x, y, 7)).map(([x, y]) => [x, y]);
        p.poly("#6b1a14", base);
        p.poly(flash ? "#ffffff" : "#d8352a", top);
        const [a, , c] = top;
        p.line(flash ? "#ffd23f" : "#f4f1e8", c[0], c[1], a[0], a[1], 1);
      });
      // plunger
      const pull = t < PIN.launch ? 9 * prog(t, PIN.launch - 0.6, PIN.launch) : 0;
      const pl0 = P(197, 392 + pull, 0);
      const pl1 = P(209, 400, 0);
      p.rect("#c0c6d4", pl0[0], pl0[1], pl1[0] - pl0[0], Math.max(2, pl1[1] - pl0[1]));
      // depth-sorted objects
      const objects = [];
      BUMPERS.forEach((b, i) => objects.push({ y: b.y, draw: () => drawBumper(p, t, b, i) }));
      TARGETS.forEach((target, i) => {
        objects.push({
          y: target.y,
          draw: () => {
            const a0 = P(target.x0, target.y, 0);
            const b0 = P(target.x1, target.y, 0);
            if (f.targetDown[i]) {
              p.line("#1a2150", a0[0], a0[1], b0[0], b0[1], 2);
              return;
            }
            const a1 = P(target.x0, target.y, 10);
            const b1 = P(target.x1, target.y, 10);
            p.poly("#f5c400", [[a0[0], a0[1]], [b0[0], b0[1]], [b1[0], b1[1]], [a1[0], a1[1]]]);
            p.line("#fff3a0", a1[0], a1[1], b1[0], b1[1]);
          },
        });
      });
      FLIPPERS.forEach((flipper, i) => objects.push({ y: flipper.y, draw: () => drawFlipper(p, f.flippers[i], flipper, f.flippersOn) }));
      f.balls.forEach((ball) => {
        const trail = [2, 1].map((back) => {
          const pb = frameAt(t - back / FPS).balls.find((b) => b.id === ball.id) || ball;
          return [pb.x, pb.y];
        });
        objects.push({ y: ball.y + 0.5, draw: () => drawBall(p, ball.x, ball.y, t, trail) });
      });
      objects.sort((a, b) => a.y - b.y).forEach((o) => o.draw());
      drawEffects(p, t);

      // jackpot celebration
      if (t >= PIN.jackpot) {
        const age = t - PIN.jackpot;
        if (age < 0.3) p.alpha(0.7 * (1 - age / 0.3), () => p.clear("#ffffff"));
        const pop = backOut(prog(t, PIN.jackpot, PIN.jackpot + 0.3));
        const size = Math.max(8, Math.round((format === "portrait" ? 32 : 24) * clamp(pop, 0.3, 1.2) / 8) * 8);
        const y = format === "portrait" ? 330 : 150;
        p.text("JACKPOT!", L.cx, y, RAINBOW[Math.floor(t * 12) % RAINBOW.length], { size, align: "center", shadow: "#000000", shadowOffset: 3 });
      }
      if (format === "landscape") drawPanelLandscape(p, t, f);
      else drawHudPortrait(p, t, f);

      // story overlay while the agent is stopped
      const stop = sim.events.find((e) => e.type === "agentStop");
      if (stop && t >= stop.t && t < kickback) {
        const y = format === "portrait" ? 250 : 120;
        p.panel(L.cx - 90, y, 180, 30, "#000000", "#ff4a3d", 2);
        p.text(t >= saveAt ? "LOOPER: CONTINUE >" : "AGENT STOPPED.", L.cx, y + 11, t >= saveAt ? "#7fe3ff" : "#ff4a3d", { align: "center" });
      }
      stageBanner(p, t, SCENE_START + 0.1, PIN.launch + 0.8, "STAGE 1", "LOOPER CADET", format === "portrait" ? 190 : 110, 16, L.cx);
    },
  };
}
