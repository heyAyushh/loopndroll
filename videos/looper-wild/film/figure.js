// The developer: a gestural line figure on a 2D skeleton. Every stroke is re-drawn 12 times a second with
// a tremble, so the person is always alive; fatigue makes the hand that draws them shakier.
import { clamp, easeInOut, lerp } from "./story.js";

// Joints in canvas pixels, profile facing left toward the monitor.
const POSES = {
  typing: { head: [700, 1105], neck: [742, 1180], hip: [790, 1440], elbow: [650, 1330], hand: [560, 1302], knee: [640, 1470], foot: [650, 1690], elbow2: [668, 1336], hand2: [585, 1306] },
  slump: { head: [676, 1150], neck: [728, 1210], hip: [792, 1440], elbow: [655, 1345], hand: [575, 1305], knee: [640, 1470], foot: [650, 1690], elbow2: [672, 1350], hand2: [598, 1308] },
  asleep: { head: [612, 1262], neck: [690, 1250], hip: [792, 1440], elbow: [640, 1300], hand: [560, 1296], knee: [640, 1470], foot: [650, 1690], elbow2: [660, 1296], hand2: [590, 1290] },
  jolt: { head: [736, 1080], neck: [760, 1165], hip: [795, 1440], elbow: [700, 1320], hand: [640, 1300], knee: [640, 1470], foot: [650, 1690], elbow2: [720, 1325], hand2: [660, 1302] },
  look: { head: [690, 1140], neck: [738, 1200], hip: [792, 1440], elbow: [668, 1330], hand: [610, 1300], knee: [640, 1470], foot: [650, 1690], elbow2: [690, 1334], hand2: [640, 1304] },
  stand: { head: [800, 930], neck: [800, 1010], hip: [805, 1300], elbow: [820, 1150], hand: [835, 1270], knee: [800, 1490], foot: [805, 1690], elbow2: [790, 1155], hand2: [780, 1275] },
  lie: { head: [300, 1560], neck: [370, 1575], hip: [640, 1590], elbow: [430, 1500], hand: [420, 1430], knee: [760, 1520], foot: [880, 1600], elbow2: [470, 1590], hand2: [540, 1580] },
  stretch: { head: [600, 1310], neck: [610, 1390], hip: [640, 1620], elbow: [560, 1230], hand: [590, 1110], knee: [760, 1600], foot: [840, 1700], elbow2: [680, 1230], hand2: [650, 1110] },
};

function blend(a, b, k) {
  const out = {};
  for (const key of Object.keys(POSES.typing)) out[key] = [lerp(a[key][0], b[key][0], k), lerp(a[key][1], b[key][1], k)];
  return out;
}

// Keyframes: [seconds, pose, dx]. Walk is a moving stand with swinging legs.
export function poseAt(t, track) {
  let i = track.findIndex(([time]) => time > t);
  if (i === -1) i = track.length - 1;
  const [t0, a, dx0 = 0] = track[Math.max(0, i - 1)];
  const [t1, b, dx1 = 0] = track[i];
  const k = t1 > t0 ? easeInOut(clamp((t - t0) / (t1 - t0))) : 1;
  const pose = blend(POSES[a] ?? POSES.stand, POSES[b] ?? POSES.stand, k);
  const dx = lerp(dx0, dx1, k);
  for (const key of Object.keys(pose)) pose[key][0] += dx;
  const walking = a === "walk" || b === "walk";
  if (walking) {
    const swing = Math.sin(t * Math.PI * 2 / 0.94) * 70;
    pose.knee = [pose.knee[0] + swing * 0.5, pose.knee[1]]; pose.foot = [pose.foot[0] + swing, pose.foot[1] - Math.max(0, Math.sin(t * 6.7)) * 30];
    pose.hand = [pose.hand[0] - swing * 0.4, pose.hand[1]]; pose.hand2 = [pose.hand2[0] + swing * 0.4, pose.hand2[1]];
  }
  return pose;
}
POSES.walk = POSES.stand;

// A trembling stroke through points, drawn twice (a wide soft pass and a sharp one).
export function tremble(p, points, t, { weight = 4, colour = [235, 235, 245], alpha = 230, shake = 1.5, seed = 0 } = {}) {
  const frame = Math.floor(t * 12);
  for (const [w, a] of [[weight * 3, alpha * 0.12], [weight, alpha]]) {
    p.stroke(colour[0], colour[1], colour[2], a); p.strokeWeight(w); p.noFill();
    p.beginShape();
    points.forEach(([x, y], j) => {
      const nx = (p.noise(seed + j * 3.1, frame * 0.37) - 0.5) * shake * 2;
      const ny = (p.noise(seed + j * 5.7 + 40, frame * 0.37) - 0.5) * shake * 2;
      p.curveVertex(x + nx, y + ny);
      if (j === 0 || j === points.length - 1) p.curveVertex(x + nx, y + ny);
    });
    p.endShape();
  }
}

export function drawFigure(p, pose, t, { fatigue = 0, colour, alpha = 235 } = {}) {
  const shake = 1.2 + fatigue * 7;
  const o = { colour, alpha, shake };
  const { head, neck, hip, elbow, hand, knee, foot, elbow2, hand2 } = pose;
  // A solid hoodie silhouette under the line: the person reads as a shape at a glance, then as a drawing.
  p.push();
  p.noStroke(); p.fill(10, 10, 16, alpha * 0.92);
  p.beginShape();
  for (const [x, y] of [[head[0] + 20, head[1] - 62], [head[0] + 66, head[1] - 8], [neck[0] + 30, neck[1] + 10], [lerp(neck[0], hip[0], 0.5) + 36, lerp(neck[1], hip[1], 0.5)], [hip[0] + 20, hip[1] + 10], [hip[0] - 50, hip[1] + 8], [lerp(neck[0], hip[0], 0.55) - 38, lerp(neck[1], hip[1], 0.55)], [neck[0] - 24, neck[1] + 6]]) p.curveVertex(x, y);
  p.endShape(p.CLOSE);
  p.circle(head[0], head[1], 116);
  p.pop();
  const heavy = 1.7;
  // Hood and back: one long curve from the crown down the spine.
  tremble(p, [[head[0] + 18, head[1] - 60], [head[0] + 62, head[1] - 10], neck, [lerp(neck[0], hip[0], 0.5) + 28, lerp(neck[1], hip[1], 0.5)], hip], t, { ...o, weight: 6 * heavy, seed: 1 });
  // Head: an open circle and a scribble of hair.
  const r = 58;
  const ring = Array.from({ length: 11 }, (_, k) => { const a = -0.6 + k * 0.62; return [head[0] + Math.cos(a) * r, head[1] + Math.sin(a) * r]; });
  tremble(p, ring, t, { ...o, weight: 4.5 * heavy, seed: 2 });
  tremble(p, Array.from({ length: 9 }, (_, k) => [head[0] - 40 + k * 12 + Math.sin(k * 2.3) * 8, head[1] - 52 - Math.abs(Math.sin(k * 1.7)) * 26]), t, { ...o, weight: 3 * heavy, seed: 3 });
  // Glasses: the one detail that says "developer at 3am".
  tremble(p, Array.from({ length: 8 }, (_, k) => { const a = k * 0.9; return [head[0] - 34 + Math.cos(a) * 15, head[1] - 4 + Math.sin(a) * 12]; }), t, { ...o, weight: 2.5 * heavy, seed: 4 });
  // Front of the body, arms, legs.
  tremble(p, [neck, [lerp(neck[0], hip[0], 0.5) - 30, lerp(neck[1], hip[1], 0.55)], [hip[0] - 40, hip[1]]], t, { ...o, weight: 5 * heavy, seed: 5 });
  tremble(p, [neck, elbow, hand], t, { ...o, weight: 5 * heavy, seed: 6 });
  tremble(p, [[neck[0] + 8, neck[1] + 14], elbow2, hand2], t, { ...o, weight: 4 * heavy, seed: 7, alpha: alpha * 0.7 });
  tremble(p, [hip, knee, foot, [foot[0] - 50, foot[1] + 4]], t, { ...o, weight: 5.5 * heavy, seed: 8 });
}
