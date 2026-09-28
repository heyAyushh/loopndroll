// Stage 2: "Prince of Prompts" — a Prince-of-Persia-style dungeon run.
// The prince is a jointed skeleton (angles + two-bone IK) so his motion is
// fluid like the rotoscoped original, then rasterised as chunky pixels.
import { T, Painter, clamp, lerp, prog, easeOut, easeInOut, blinkOn, seeded, hash, scalarAt, orb, stageBanner, DEG } from "./core.js";

const Q = T.palace;
const [SCENE_START] = T.scenes.palace;
const WORLD_W = 900;
const FLOOR = 262;
const FLOOR_FRONT = 18;
const UPPER = 150;
const TOP = 40;
const LOWER = 366;
const TILE = 32;
const MAIN_FLOORS = [[0, 520], [600, WORLD_W]];
const LOOSE = [176, 208];
const SPIKES = [400, 432];
const LEDGE = [600, FLOOR];
const DOOR_X = 812;
const GUARD_X = 708;
const UPPER_FLOORS = [[0, 140], [250, 470], [640, WORLD_W]];
const TOP_FLOORS = [[0, 300], [420, WORLD_W]];
const LOWER_FLOORS = [[0, WORLD_W]];
const TORCHES = [[120, 212], [330, 212], [560, 212], [740, 212], [200, 104], [700, 104]];
const COLUMNS = [60, 316, 572];

// The dungeon renders at half resolution and is doubled with hard pixels,
// which puts the prince at Prince-of-Persia proportions (about a fifth of the screen).
const ZOOM = 2;
const VIEW = {
  landscape: { top: 140, lead: 110 },
  portrait: { top: 62, lead: 70 },
};

// ---------- skeleton ----------
const BONE = { torso: 15, head: 4.5, thigh: 11, shin: 11, upper: 8.5, fore: 8.5, sword: 15 };

function limb(sx, sy, a1, a2, l1, l2, f) {
  const mx = sx + f * Math.sin(a1 * DEG) * l1;
  const my = sy + Math.cos(a1 * DEG) * l1;
  return [[mx, my], [mx + f * Math.sin(a2 * DEG) * l2, my + Math.cos(a2 * DEG) * l2]];
}
function ik(sx, sy, tx, ty, l1, l2, bend) {
  const dx = tx - sx;
  const dy = ty - sy;
  const d = clamp(Math.hypot(dx, dy), 0.01, l1 + l2 - 0.01);
  const a = Math.acos(clamp((l1 * l1 + d * d - l2 * l2) / (2 * l1 * d), -1, 1));
  const base = Math.atan2(dy, dx);
  const mid = [sx + Math.cos(base + bend * a) * l1, sy + Math.sin(base + bend * a) * l1];
  const len = Math.hypot(dx, dy);
  const reach = Math.min(len, l1 + l2);
  return [mid, [sx + (dx / len) * reach, sy + (dy / len) * reach]];
}

// pose: { hip:[x,y], lean, f, legs:[front, back], arms:[front, back], sword }
// limb spec: { a, b } angles (deg from straight down, + = forward) or { to:[x,y] } IK target
function solve(pose) {
  const f = pose.f;
  const [hx, hy] = pose.hip;
  const lean = pose.lean || 0;
  const neck = [hx + f * Math.sin(lean * DEG) * BONE.torso, hy - Math.cos(lean * DEG) * BONE.torso];
  const head = [neck[0] + f * Math.sin(lean * DEG) * (BONE.head + 2), neck[1] - Math.cos(lean * DEG) * (BONE.head + 2)];
  const shoulder = [neck[0] - f * 1, neck[1] + 1];
  const legs = pose.legs.map((leg) =>
    leg.to ? ik(hx, hy, leg.to[0], leg.to[1], BONE.thigh, BONE.shin, f > 0 ? -1 : 1) : limb(hx, hy, leg.a, leg.a - leg.b, BONE.thigh, BONE.shin, f),
  );
  const arms = pose.arms.map((arm) =>
    arm.to ? ik(shoulder[0], shoulder[1], arm.to[0], arm.to[1], BONE.upper, BONE.fore, f > 0 ? 1 : -1) : limb(shoulder[0], shoulder[1], arm.a, arm.a + arm.b, BONE.upper, BONE.fore, f),
  );
  return { f, hip: [hx, hy], neck, head, shoulder, legs, arms, sword: pose.sword, lean };
}
function mixJoints(a, b, u) {
  const mp = (p, q) => [lerp(p[0], q[0], u), lerp(p[1], q[1], u)];
  return {
    f: b.f,
    hip: mp(a.hip, b.hip),
    neck: mp(a.neck, b.neck),
    head: mp(a.head, b.head),
    shoulder: mp(a.shoulder, b.shoulder),
    legs: a.legs.map((leg, i) => [mp(leg[0], b.legs[i][0]), mp(leg[1], b.legs[i][1])]),
    arms: a.arms.map((arm, i) => [mp(arm[0], b.arms[i][0]), mp(arm[1], b.arms[i][1])]),
    sword: a.sword == null ? b.sword : b.sword == null ? a.sword : lerp(a.sword, b.sword, u),
    lean: lerp(a.lean, b.lean, u),
  };
}
function keyed(frames, t) {
  if (t <= frames[0][0]) return frames[0][1];
  for (let i = 1; i < frames.length; i++) {
    if (t <= frames[i][0]) return mixJoints(frames[i - 1][1], frames[i][1], easeInOut((t - frames[i - 1][0]) / (frames[i][0] - frames[i - 1][0])));
  }
  return frames[frames.length - 1][1];
}

const STAND_LEGS = [{ a: 4, b: 3 }, { a: -4, b: 3 }];
const STAND_ARMS = [{ a: 6, b: 10 }, { a: -6, b: 8 }];
const standPose = (x, f = 1, extra = {}) => solve({ hip: [x, FLOOR - 21], lean: 0, f, legs: STAND_LEGS, arms: STAND_ARMS, ...extra });

function runPose(x, phase, f = 1) {
  const legs = [0, Math.PI].map((off) => {
    const psi = phase + off;
    return { a: 34 * Math.sin(psi), b: 12 + 62 * Math.max(0, Math.cos(psi)) };
  });
  const arms = [Math.PI, 0].map((off) => ({ a: 32 * Math.sin(phase + off), b: 38 }));
  return solve({ hip: [x, FLOOR - 20 + 1.5 * Math.cos(2 * phase)], lean: 12, f, legs, arms });
}
function stepPose(x, u, f = 1) {
  // PoP "careful step": one foot slides forward, weight shifts, body upright.
  const s = Math.sin(Math.PI * u);
  return solve({
    hip: [x, FLOOR - 21 + s * 0.8],
    lean: 3,
    f,
    legs: [{ a: 4 + 18 * s, b: 6 + 12 * s }, { a: -4 - 10 * s, b: 4 }],
    arms: [{ a: 12 + 8 * s, b: 20 }, { a: -8 - 6 * s, b: 12 }],
  });
}
const EN_GARDE = (x, f) =>
  solve({ hip: [x, FLOOR - 19], lean: 5, f, legs: [{ a: 22, b: 22 }, { a: -26, b: 14 }], arms: [{ a: 62, b: 26 }, { a: 150, b: 60 }], sword: 92 });
const LUNGE = (x, f) =>
  solve({ hip: [x + 7 * f, FLOOR - 17], lean: 18, f, legs: [{ a: 50, b: 38 }, { a: -46, b: 4 }], arms: [{ a: 86, b: 2 }, { a: 160, b: 40 }], sword: 90 });
const PARRY = (x, f) =>
  solve({ hip: [x - 2 * f, FLOOR - 20], lean: -6, f, legs: [{ a: 16, b: 18 }, { a: -30, b: 16 }], arms: [{ a: 104, b: 58 }, { a: 140, b: 50 }], sword: 162 });

// ---------- timeline of the prince ----------
const RUN_KEYS = [
  [Q.run, 60],
  [Q.spikesUp, 385],
];
const RUN2 = [
  [Q.carefulSteps[2] + 0.15, 439],
  [Q.jump, 505],
];
const STRIDE = 34;
const CAREFUL = [385, 403, 421, 439];
const HANG = { hip: [LEDGE[0] - 4, LEDGE[1] + 32] };

function hangPose(swing, oneHand, drop) {
  const hip = [HANG.hip[0] + swing, HANG.hip[1] + drop];
  const hands = [{ to: [LEDGE[0] + 1, LEDGE[1] + 1] }, oneHand ? { a: 8 + swing * 3, b: 6 } : { to: [LEDGE[0] - 2, LEDGE[1] + 1] }];
  return solve({ hip, lean: -2 + swing, f: 1, legs: [{ a: 8 - swing * 4, b: 8 }, { a: -5 - swing * 4, b: 14 }], arms: hands });
}
const L0 = LEDGE[0];
const L1 = LEDGE[1];
const CLIMB = [
  [0, () => hangPose(0, false, 0)],
  [0.35, () => solve({ hip: [L0 - 2, L1 + 14], lean: 12, f: 1, legs: [{ a: 55, b: 75 }, { a: 25, b: 55 }], arms: [{ to: [L0 + 1, L1 + 1] }, { to: [L0 - 2, L1 + 1] }] })],
  [0.6, () => solve({ hip: [L0 + 5, L1 - 4], lean: 38, f: 1, legs: [{ to: [L0 + 11, L1 - 1] }, { a: -12, b: 70 }], arms: [{ to: [L0 + 7, L1] }, { to: [L0 + 10, L1] }] })],
  [0.82, () => solve({ hip: [L0 + 11, L1 - 12], lean: 22, f: 1, legs: [{ to: [L0 + 18, L1] }, { to: [L0 + 7, L1] }], arms: [{ a: 24, b: 30 }, { a: 10, b: 20 }] })],
  [1, () => solve({ hip: [L0 + 14, L1 - 21], lean: 0, f: 1, legs: [{ to: [L0 + 17, L1] }, { to: [L0 + 11, L1] }], arms: STAND_ARMS })],
];
const CLIMB_FRAMES = CLIMB.map(([u, make]) => [lerp(Q.climb, Q.climbEnd, u), make()]);
const FIGHT_X = 660;
const DUEL = [
  [31.6, EN_GARDE(L0 + 14, 1)],
  [31.95, EN_GARDE(FIGHT_X, 1)],
  [Q.clashes[0] - 0.05, EN_GARDE(FIGHT_X, 1)],
  [Q.clashes[0], LUNGE(FIGHT_X, 1)],
  [Q.clashes[0] + 0.18, EN_GARDE(FIGHT_X, 1)],
  [Q.clashes[1], PARRY(FIGHT_X, 1)],
  [Q.clashes[1] + 0.18, EN_GARDE(FIGHT_X, 1)],
  [Q.clashes[2], LUNGE(FIGHT_X, 1)],
  [Q.clashes[2] + 0.12, EN_GARDE(FIGHT_X, 1)],
  [Q.hit, LUNGE(FIGHT_X + 3, 1)],
  [Q.hit + 0.35, EN_GARDE(FIGHT_X + 3, 1)],
  [Q.guardDown + 0.1, standPose(FIGHT_X + 3, 1)],
];
const EXIT_RUN = [
  [Q.door - 0.1, FIGHT_X + 3],
  [Q.stairs, DOOR_X - 10],
];

function jumpPose(t) {
  const u = prog(t, Q.jump, Q.grab);
  const x = lerp(505, HANG.hip[0], u);
  const y = lerp(FLOOR - 20, HANG.hip[1], u) - Math.sin(Math.PI * Math.min(1, u * 1.3)) * 26;
  const reach = prog(u, 0.55, 1);
  return solve({
    hip: [x, y],
    lean: lerp(22, 0, reach),
    f: 1,
    legs: [{ a: lerp(62, 12, reach), b: lerp(24, 12, reach) }, { a: lerp(-48, -8, reach), b: lerp(30, 18, reach) }],
    arms: [{ a: lerp(130, 176, reach), b: lerp(10, 0, reach) }, { a: lerp(110, 172, reach), b: lerp(20, 0, reach) }],
  });
}

function princeAt(t) {
  if (t < Q.run) return standPose(60 + 0.4 * Math.sin(t * 2));
  if (t < Q.spikesUp) {
    const x = scalarAt(RUN_KEYS, t);
    const settle = prog(t, Q.spikesUp - 0.2, Q.spikesUp);
    const run = runPose(x, (x / STRIDE) * Math.PI);
    return settle > 0 ? mixJoints(run, standPose(x), settle) : run;
  }
  if (t < Q.carefulSteps[2] + 0.25) {
    for (let i = Q.carefulSteps.length - 1; i >= 0; i--) {
      const start = Q.carefulSteps[i];
      if (t >= start) {
        const u = prog(t, start, start + 0.25);
        return stepPose(lerp(CAREFUL[i], CAREFUL[i + 1], easeInOut(u)), u);
      }
    }
    return standPose(CAREFUL[0]);
  }
  if (t < Q.jump) {
    const x = scalarAt(RUN2, t);
    return runPose(x, (x / STRIDE) * Math.PI);
  }
  if (t < Q.grab) return jumpPose(t);
  if (t < Q.climb) {
    const swing = Math.sin((t - Q.grab) * 5) * 2.5 * Math.exp(-(t - Q.grab) * 1.2);
    const oneHand = t >= Q.slip && t < Q.regrab;
    const drop = oneHand ? 3 : t >= Q.regrab ? 3 * (1 - prog(t, Q.regrab, Q.regrab + 0.12)) : 0;
    const shake = oneHand ? Math.sin(t * 60) * 0.6 : 0;
    return hangPose(swing + shake, oneHand, drop);
  }
  if (t < Q.climbEnd) return keyed(CLIMB_FRAMES, t);
  if (t < Q.door - 0.1) return keyed([[Q.climbEnd, CLIMB_FRAMES[CLIMB_FRAMES.length - 1][1]], ...DUEL], t);
  if (t < Q.stairs) {
    const x = scalarAt(EXIT_RUN, t);
    return runPose(x, (x / STRIDE) * Math.PI);
  }
  // up the stairs, into the light
  const u = prog(t, Q.stairs, Q.stairs + 0.6);
  const x = lerp(DOOR_X - 10, DOOR_X + 12, u);
  const pose = runPose(x, (x / (STRIDE * 0.6)) * Math.PI);
  const rise = u * 18;
  const shift = (pt) => [pt[0], pt[1] - rise];
  return {
    ...pose,
    hip: shift(pose.hip),
    neck: shift(pose.neck),
    head: shift(pose.head),
    shoulder: shift(pose.shoulder),
    legs: pose.legs.map((l) => l.map(shift)),
    arms: pose.arms.map((a) => a.map(shift)),
  };
}

function guardAt(t) {
  const f = -1;
  if (t < Q.guardAlert) return standPose(GUARD_X, f, { arms: [{ a: 4, b: 30 }, { a: -8, b: 10 }] });
  const keys = [
    [Q.guardAlert, standPose(GUARD_X, f)],
    [Q.guardAlert + 0.25, EN_GARDE(GUARD_X, f)],
    [Q.clashes[0], PARRY(GUARD_X, f)],
    [Q.clashes[0] + 0.18, EN_GARDE(GUARD_X, f)],
    [Q.clashes[1] - 0.05, EN_GARDE(GUARD_X, f)],
    [Q.clashes[1], LUNGE(GUARD_X, f)],
    [Q.clashes[1] + 0.18, EN_GARDE(GUARD_X, f)],
    [Q.clashes[2], PARRY(GUARD_X, f)],
    [Q.clashes[2] + 0.12, EN_GARDE(GUARD_X, f)],
    [Q.hit, EN_GARDE(GUARD_X + 3, f)],
    [Q.hit + 0.18, solve({ hip: [GUARD_X + 8, FLOOR - 18], lean: -30, f, legs: [{ a: 30, b: 40 }, { a: -10, b: 20 }], arms: [{ a: 150, b: 20 }, { a: 120, b: 30 }], sword: 170 })],
    [Q.guardDown, solve({ hip: [GUARD_X + 16, FLOOR - 5], lean: -86, f, legs: [{ a: 80, b: 20 }, { a: 70, b: 10 }], arms: [{ a: 170, b: 10 }, { a: 150, b: 20 }], sword: null })],
  ];
  return keyed(keys, t);
}

// ---------- drawing ----------
const PRINCE_COLORS = { cloth: "#f4f1e8", clothBack: "#b9b4a4", skin: "#e0a878", skinBack: "#b3804f", hair: "#2a1a10", sash: "#c8302a", shoe: "#5a3a22" };
const GUARD_COLORS = { cloth: "#8f5bd8", clothBack: "#5b3a96", skin: "#b37a4a", skinBack: "#8a5a32", hair: "#e6dfc6", sash: "#f5c542", shoe: "#2a1a3a" };

function drawFigure(p, j, colors, ox, oy, flashColor = null) {
  const X = (pt) => pt[0] - ox;
  const Y = (pt) => pt[1] - oy;
  const c = flashColor ? { cloth: flashColor, clothBack: flashColor, skin: flashColor, skinBack: flashColor, hair: flashColor, sash: flashColor, shoe: flashColor } : colors;
  const drawLeg = (leg, back) => {
    const [knee, foot] = leg;
    p.line(back ? c.clothBack : c.cloth, X(j.hip), Y(j.hip), X(knee), Y(knee), 4);
    p.line(back ? c.clothBack : c.cloth, X(knee), Y(knee), X(foot), Y(foot), 3);
    p.line(c.shoe, X(foot), Y(foot), X(foot) + j.f * 3, Y(foot), 2);
  };
  const drawArm = (arm, back) => {
    const [elbow, hand] = arm;
    p.line(back ? c.clothBack : c.cloth, X(j.shoulder), Y(j.shoulder), X(elbow), Y(elbow), 3);
    p.line(back ? c.skinBack : c.skin, X(elbow), Y(elbow), X(hand), Y(hand), 2);
  };
  drawArm(j.arms[1], true);
  drawLeg(j.legs[1], true);
  // torso as a tapered slab
  const dx = j.neck[0] - j.hip[0];
  const dy = j.neck[1] - j.hip[1];
  const len = Math.hypot(dx, dy) || 1;
  const nx = -dy / len;
  const ny = dx / len;
  p.poly(c.cloth, [
    [X(j.hip) + nx * 2.5, Y(j.hip) + ny * 2.5],
    [X(j.neck) + nx * 3.5, Y(j.neck) + ny * 3.5],
    [X(j.neck) - nx * 3.5, Y(j.neck) - ny * 3.5],
    [X(j.hip) - nx * 2.5, Y(j.hip) - ny * 2.5],
  ]);
  p.line(c.sash, X(j.hip) + nx * 3 + dx * 0.12, Y(j.hip) + ny * 3 + dy * 0.12, X(j.hip) - nx * 3 + dx * 0.12, Y(j.hip) - ny * 3 + dy * 0.12, 2);
  drawLeg(j.legs[0], false);
  // head: skin disc, hair on the back half, eye facing forward
  p.disc(c.skin, X(j.head), Y(j.head), BONE.head);
  p.rect(c.hair, X(j.head) - (j.f > 0 ? 4 : -1), Y(j.head) - 5, 4, 7);
  p.rect(c.hair, X(j.head) - 4, Y(j.head) - 5, 8, 2);
  if (!flashColor) p.rect("#1a1020", X(j.head) + j.f * 2, Y(j.head) - 1, 1, 1);
  if (j.sword != null) {
    const [, hand] = j.arms[0];
    const a = j.sword * DEG;
    const tip = [hand[0] + j.f * Math.sin(a) * BONE.sword, hand[1] + Math.cos(a) * BONE.sword];
    p.line("#e8ecf4", X(hand), Y(hand), X(tip), Y(tip), 1);
    p.line("#f5c542", X(hand) - 1, Y(hand) - 1, X(hand) + 1, Y(hand) + 1, 2);
  }
  drawArm(j.arms[0], false);
  return j.sword != null ? j : null;
}
function swordTip(j) {
  if (j.sword == null) return null;
  const [, hand] = j.arms[0];
  const a = j.sword * DEG;
  return [hand[0] + j.f * Math.sin(a) * BONE.sword, hand[1] + Math.cos(a) * BONE.sword];
}

export function createPalace(format) {
  const V = VIEW[format];
  const torchSeed = TORCHES.map((_, i) => hash(i + 40) * 10);
  let world = null;

  function camera(p, t) {
    const x = princeAt(t).hip[0];
    return clamp(Math.round(x - V.lead), 0, WORLD_W - p.W);
  }

  function wall(p, ox, oy, top, bottom, base, light, dark, mortar) {
    const rowH = 16;
    for (let wy = Math.floor(top / rowH) * rowH; wy < bottom; wy += rowH) {
      const row = Math.round(wy / rowH);
      const offset = row % 2 ? 16 : 0;
      const startX = Math.floor((ox - offset) / TILE) * TILE + offset;
      for (let wx = startX; wx < ox + p.W; wx += TILE) {
        const sx = wx - ox;
        const sy = wy - oy;
        const v = hash(row * 131 + Math.round(wx / TILE) * 17);
        p.rect(mortar, sx, sy, TILE, rowH);
        p.rect(v < 0.25 ? dark : v > 0.85 ? light : base, sx + 1, sy + 1, TILE - 2, rowH - 2);
        p.rect(light, sx + 1, sy + 1, TILE - 2, 1);
        if (v > 0.6 && v < 0.64) p.rect(mortar, sx + 12, sy + 5, 6, 1);
      }
    }
  }

  function floorSlab(p, ox, oy, x0, x1, y, lower = false) {
    const sx = x0 - ox;
    const w = x1 - x0;
    const sy = y - oy;
    p.rect(lower ? "#5a6392" : "#7c86b8", sx, sy - 3, w, 4);
    p.rect(lower ? "#7c86b8" : "#a9b2dc", sx, sy - 3, w, 1);
    p.rect(lower ? "#262c52" : "#3b4577", sx, sy + 1, w, FLOOR_FRONT);
    for (let x = x0; x < x1; x += TILE) p.rect("#1e2446", x - ox, sy + 1, 1, FLOOR_FRONT);
    p.rect("#1e2446", sx, sy + FLOOR_FRONT, w, 1);
    p.rect("#10142a", sx, sy + FLOOR_FRONT + 1, w, 2);
  }

  function torch(p, ox, oy, x, y, t, seed) {
    const sx = x - ox;
    const sy = y - oy;
    const glow = 30 + Math.sin(t * 13 + seed) * 2;
    for (let r = glow; r > 6; r -= 6) p.alpha(0.07, () => p.disc("#ffb45a", sx + 2, sy - 6, r));
    p.rect("#6b4a2a", sx, sy, 5, 3);
    p.rect("#4a3018", sx + 1, sy + 3, 3, 6);
    const frame = Math.floor(t * 10 + seed) % 3;
    const h = [9, 11, 8][frame];
    p.rect("#ff6a1f", sx - 1 + (frame === 1 ? 1 : 0), sy - h, 7, h);
    p.rect("#ffb42a", sx, sy - h + 2, 5, h - 2);
    p.rect("#fff3a0", sx + 1, sy - 4, 3, 3);
  }

  function spikes(p, ox, oy, t) {
    const up = t >= Q.spikesUp ? easeOut(prog(t, Q.spikesUp, Q.spikesUp + 0.08)) : 0;
    const sy = FLOOR - oy;
    p.rect("#10142a", SPIKES[0] - ox, sy - 3, SPIKES[1] - SPIKES[0], 3);
    if (up <= 0) return;
    for (let x = SPIKES[0] + 3; x < SPIKES[1] - 2; x += 6) {
      const h = 11 * up;
      p.poly("#c8ccda", [[x - ox, sy - 2], [x + 2 - ox, sy - 2 - h], [x + 4 - ox, sy - 2]]);
      p.rect("#ffffff", x + 1 - ox, sy - 1 - h, 1, 2);
    }
  }

  function looseTile(p, ox, oy, t) {
    const sx = LOOSE[0] - ox;
    if (t < Q.crumbleFall) {
      const shake = t >= Q.crumbleShake ? (Math.floor(t * 30) % 2 ? 1 : -1) : 0;
      floorSlab(p, ox, oy - shake, LOOSE[0], LOOSE[1], FLOOR);
      return;
    }
    // hole left behind
    p.rect("#0a0d1c", sx, FLOOR - oy - 3, TILE, FLOOR_FRONT + 4);
    if (t < Q.crumbleCrash) {
      const u = prog(t, Q.crumbleFall, Q.crumbleCrash);
      const y = lerp(FLOOR, LOWER - 4, u * u);
      p.rect("#7c86b8", sx + u * 2, y - oy - 3, TILE, 4);
      p.rect("#3b4577", sx + u * 2, y - oy + 1, TILE, 10);
      return;
    }
    // rubble + dust
    const r = seeded(11);
    for (let i = 0; i < 9; i++) p.rect(i % 2 ? "#3b4577" : "#7c86b8", sx - 4 + r() * 36, LOWER - oy - 3 - r() * 4, 3 + r() * 5, 3);
    const age = t - Q.crumbleCrash;
    if (age < 0.8) {
      for (let i = 0; i < 16; i++) {
        const a = Math.PI + r() * Math.PI;
        const d = age * (30 + r() * 30);
        p.alpha(1 - age / 0.8, () => p.rect("#c8ccda", sx + 16 + Math.cos(a) * d, LOWER - oy - 4 + Math.sin(a) * d * 0.5, 2, 2));
      }
    }
  }

  function exitDoor(p, ox, oy, t) {
    const sx = DOOR_X - ox - 24;
    const sy = FLOOR - oy;
    p.rect("#2b3363", sx - 8, sy - 84, 64, 84);
    p.rect("#4a5590", sx - 8, sy - 84, 64, 4);
    p.rect("#05060d", sx, sy - 74, 48, 74);
    // stairs climbing into the light
    const open = easeInOut(prog(t, Q.door, Q.door + 0.6));
    p.alpha(open, () => {
      p.bands(sy - 74, sy, "#fff3a0", "#ffb45a", 6, sx, 48);
      for (let i = 0; i < 5; i++) p.rect("#c28a4a", sx + 10 + i * 8, sy - 4 - i * 5, 48 - 10 - i * 8, 3);
    });
    return { sx, sy, open };
  }
  function exitGate(p, door) {
    const lift = 70 * door.open;
    for (let i = 0; i < 6; i++) p.rect("#8a91a8", door.sx + 3 + i * 8, door.sy - 74 - lift, 3, 74);
    p.rect("#8a91a8", door.sx, door.sy - 50 - lift, 48, 3);
    p.rect("#8a91a8", door.sx, door.sy - 22 - lift, 48, 3);
    p.rect("#2b3363", door.sx - 8, door.sy - 84, 64, 10);
    p.text("EXIT", door.sx + 24, door.sy - 98, "#7dff6a", { align: "center", shadow: "#000000" });
  }

  function label(p, str, x, y, color) {
    p.text(str, x, y, color, { align: "center", shadow: "#000000" });
  }
  function box(p, str, cx, y, color) {
    const w = p.textWidth(str) + 16;
    p.panel(Math.round(cx - w / 2), y, w, 20, "#000000", color, 2);
    p.text(str, cx, y + 6, color, { align: "center" });
  }

  return {
    draw(screen, t) {
      if (!world) world = new Painter(screen.W / ZOOM, screen.H / ZOOM);
      let p = world;
      const ox = camera(p, t);
      const oy = V.top;
      p.clear("#070913");
      wall(p, ox, oy, oy, oy + p.H, "#28305c", "#36407a", "#20274c", "#141a36");
      // lower chamber is darker
      p.alpha(0.55, () => p.rect("#05060d", 0, FLOOR + FLOOR_FRONT - oy, p.W, p.H));
      COLUMNS.forEach((cx) => {
        [UPPER, FLOOR].forEach((floorY, level) => {
          const top = level === 0 ? TOP + FLOOR_FRONT : UPPER + FLOOR_FRONT;
          const sx = cx - ox;
          p.rect("#3d4a86", sx, top - oy, 18, floorY - top - 3);
          p.rect("#56629e", sx + 2, top - oy, 4, floorY - top - 3);
          p.rect("#2a3466", sx + 14, top - oy, 4, floorY - top - 3);
          p.rect("#56629e", sx - 3, floorY - oy - 9, 24, 6);
          p.rect("#56629e", sx - 3, top - oy, 24, 5);
        });
      });
      TORCHES.forEach(([x, y], i) => torch(p, ox, oy, x, y, t, torchSeed[i]));
      TOP_FLOORS.forEach(([a, b]) => floorSlab(p, ox, oy, a, b, TOP));
      UPPER_FLOORS.forEach(([a, b]) => floorSlab(p, ox, oy, a, b, UPPER));
      LOWER_FLOORS.forEach(([a, b]) => floorSlab(p, ox, oy, a, b, LOWER, true));
      MAIN_FLOORS.forEach(([a, b]) => {
        if (a === 0) {
          floorSlab(p, ox, oy, a, LOOSE[0], FLOOR);
          floorSlab(p, ox, oy, LOOSE[1], b, FLOOR);
        } else floorSlab(p, ox, oy, a, b, FLOOR);
      });
      looseTile(p, ox, oy, t);
      spikes(p, ox, oy, t);
      label(p, "CONTEXT", 560 - ox, FLOOR + 12 - oy, "#ff8a1f");
      label(p, "LIMIT", 560 - ox, FLOOR + 22 - oy, "#ff8a1f");
      if (t >= Q.crumbleShake - 0.1 && t < Q.crumbleCrash + 0.8) label(p, "FLAKY TEST", (LOOSE[0] + LOOSE[1]) / 2 - ox, FLOOR + 6 - oy, "#ff4a3d");
      if (t >= Q.spikesUp && t < Q.jump + 0.3) label(p, "RATE LIMIT", (SPIKES[0] + SPIKES[1]) / 2 - ox, FLOOR + 6 - oy, "#ff4a3d");

      const door = exitDoor(p, ox, oy, t);
      // guard
      const guard = guardAt(t);
      const guardFlash = t >= Q.hit && t < Q.hit + 0.16 && blinkOn(t, 16) ? "#ffffff" : null;
      drawFigure(p, guard, GUARD_COLORS, ox, oy, guardFlash);
      // prince
      const prince = princeAt(t);
      const fadeIn = 1 - prog(t, Q.stairs + 0.35, Q.stairs + 0.6);
      const powered = t >= Q.cont && t < Q.climbEnd && blinkOn(t, 7) ? "#bff4ff" : null;
      p.alpha(fadeIn, () => drawFigure(p, prince, PRINCE_COLORS, ox, oy, powered));
      exitGate(p, door);
      // sword clash sparks
      Q.clashes.concat([Q.hit]).forEach((tc) => {
        const age = t - tc;
        if (age < 0 || age > 0.25) return;
        const tips = [swordTip(princeAt(tc)), swordTip(guardAt(tc))].filter(Boolean);
        if (!tips.length) return;
        const cx = tips.reduce((s, q) => s + q[0], 0) / tips.length - ox;
        const cy = tips.reduce((s, q) => s + q[1], 0) / tips.length - oy;
        const r = seeded(Math.round(tc * 100));
        for (let i = 0; i < 10; i++) {
          const a = r() * Math.PI * 2;
          const d = age * (50 + r() * 50);
          p.rect(i % 2 ? "#ffffff" : "#ffd23f", cx + Math.cos(a) * d, cy + Math.sin(a) * d, 2, 2);
        }
        if (age < 0.05) p.disc("#ffffff", cx, cy, 4);
      });
      // the Looper orb
      if (t >= Q.orb) {
        const arrive = easeOut(prog(t, Q.orb, Q.cont));
        let target;
        if (t < Q.climbEnd) target = [LEDGE[0] + 14, LEDGE[1] - 26];
        else target = [prince.hip[0] - 16 * prince.f, prince.hip[1] - 32];
        const start = [ox + 20, oy - 20];
        const x = lerp(start[0], target[0], arrive);
        const y = lerp(start[1], target[1], arrive) + Math.sin(t * 5) * 2;
        const alpha = 1 - prog(t, Q.stairs + 0.35, Q.stairs + 0.6);
        p.alpha(alpha, () => {
          for (let r = 18; r > 6; r -= 4) p.alpha(0.12, () => p.disc("#7fe3ff", x - ox, y - oy, r));
          p.image(orb(12), x - ox - 6, y - oy - 6);
          if (t >= Q.cont && t < Q.climb) {
            // beam of light down to the prince's hands
            for (let yy = y + 6; yy < LEDGE[1]; yy += 2) p.rect("#bff4ff", x - ox - 1 + (yy % 4 === 0 ? 1 : 0), yy - oy, 2, 1);
          }
        });
      }
      // story text
      if (t >= Q.stopped && t < Q.cont) box(p, "AGENT STOPPED.", p.W / 2, LEDGE[1] - 78 - oy, "#ff4a3d");
      if (t >= Q.cont && t < Q.climbEnd) box(p, "LOOPER: CONTINUE >", p.W / 2, LEDGE[1] - 78 - oy, "#7fe3ff");
      if (t >= Q.guardAlert && t < Q.hit) label(p, "BUG #412", GUARD_X - ox, FLOOR - 62 - oy, "#ff4a3d");
      if (t >= Q.hit && t < Q.hit + 0.9) label(p, "SMASHED!", GUARD_X + 10 - ox, FLOOR - 62 - oy - (t - Q.hit) * 16, blinkOn(t, 8) ? "#ffd23f" : "#ffffff");
      screen.image(p.canvas, 0, 0, screen.W, screen.H);
      p = screen;
      if (t >= Q.clear) {
        const size = format === "portrait" ? 24 : 32;
        p.text("LEVEL CLEAR", p.W / 2, format === "portrait" ? 250 : 110, "#ffd23f", { size, align: "center", shadow: "#b3261e", shadowOffset: 3 });
      }
      // PoP-style status bar
      const barY = p.H - 14;
      p.rect("#000000", 0, barY, p.W, 14);
      for (let i = 0; i < 3; i++) {
        const lit = !(t >= Q.stopped && t < Q.cont && i === 2 && blinkOn(t, 4));
        p.poly(lit ? "#e8402f" : "#3a1010", [[6 + i * 12, barY + 11], [11 + i * 12, barY + 3], [16 + i * 12, barY + 11]]);
      }
      if (t >= Q.guardAlert && t < Q.guardDown + 0.6) {
        for (let i = 0; i < 3; i++) {
          const alive = t < Q.hit;
          p.poly(alive ? "#5b8cff" : "#10203a", [[p.W - 16 - i * 12, barY + 11], [p.W - 11 - i * 12, barY + 3], [p.W - 6 - i * 12, barY + 11]]);
        }
      }
      const status = t < Q.run + 1.2 ? "LEVEL 2" : "INFINITE MINUTES LEFT";
      p.text(status, p.W / 2, barY + 3, "#ffffff", { align: "center" });
      stageBanner(p, t, Q.title, Q.title + 1.8, "STAGE 2", "PRINCE OF PROMPTS", format === "portrait" ? 150 : 40, format === "portrait" ? 16 : 24);
    },
  };
}
