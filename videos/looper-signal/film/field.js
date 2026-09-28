// The field: the agents' work as a river of light. Each point is random; the whole has intention.
// It freezes when the agents stop, falls like dust overnight, then gathers into one ring: the loop.
import * as THREE from "three";
import { BAR, BEAT, BEATS, at, clamp, easeInOut, easeOut, lerp, prng, smooth, workTime } from "./story.js";

export const COUNT = 30000;
const SPAN = 26;           // river length (x)
const RIVER_SPEED = 3.2;   // world units per second of work
const TRAIL = 0.22;        // seconds of motion each streak shows
export const RING_RADIUS = 2.35;
export const RING_TILT = 1.12;

const vertexShader = `
attribute float alpha;
attribute vec3 tint;
varying float vAlpha;
varying vec3 vTint;
void main() {
  vAlpha = alpha; vTint = tint;
  gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;
const fragmentShader = `
varying float vAlpha;
varying vec3 vTint;
void main() { gl_FragColor = vec4(vTint * vAlpha, vAlpha); }`;
const pointVertex = `
attribute float alpha;
attribute vec3 tint;
attribute float size;
varying float vAlpha;
varying vec3 vTint;
void main() {
  vAlpha = alpha; vTint = tint;
  vec4 view = modelViewMatrix * vec4(position, 1.0);
  gl_PointSize = size * (300.0 / -view.z);
  gl_Position = projectionMatrix * view;
}`;
const pointFragment = `
varying float vAlpha;
varying vec3 vTint;
void main() {
  float d = length(gl_PointCoord - 0.5);
  float glow = smoothstep(0.5, 0.0, d);
  gl_FragColor = vec4(vTint * vAlpha * glow, vAlpha * glow);
}`;

export function createField() {
  const random = prng(2026);
  const particles = Array.from({ length: COUNT }, (_, i) => {
    const gaussian = () => (random() + random() + random() - 1.5) / 1.5;
    return {
      u: random(),
      lane: i === 0 ? 0 : gaussian() * 2.1,
      depth: i === 0 ? 0 : gaussian() * 2.8,
      speed: i === 0 ? 1 : 0.55 + random() * 0.9,
      phase: random() * Math.PI * 2,
      phase2: random() * Math.PI * 2,
      bright: i === 0 ? 1.6 : random() < 0.07 ? 0.45 + random() * 0.45 : 0.035 + random() * 0.08, // a few leads, a quiet body
      fall: 0.4 + random() * 0.9,
      delay: random(),
      angle: (i / COUNT) * Math.PI * 2 + (random() - 0.5) * 0.02,
      radial: gaussian() * 0.2,
      vertical: gaussian() * 0.1,
      drift: 1 + (random() - 0.5) * 0.05,
    };
  });

  // The river at a given amount of work W.
  const river = (p, W, out) => {
    let x = ((p.u * SPAN + p.speed * RIVER_SPEED * W) % SPAN) - SPAN / 2;
    if (p === particles[0]) x = -6.5 + W * RIVER_SPEED * 0.55; // the first light, alone, crossing the dark
    const y = p.lane + 0.55 * Math.sin(x * 0.33 + p.phase + W * 0.25) + 0.22 * Math.sin(x * 0.9 + p.phase2) * Math.sin(W * 0.4 + p.lane);
    const z = p.depth + 0.35 * Math.sin(x * 0.21 + p.phase2);
    return out.set(x, y, z);
  };
  // The loop: a ring around the orb, textured by each point's own small randomness.
  const ring = (p, loop, out) => {
    const theta = p.angle + loop * p.drift;
    const r = RING_RADIUS + p.radial;
    out.set(Math.cos(theta) * r, p.vertical, Math.sin(theta) * r);
    const cy = Math.cos(RING_TILT), sy = Math.sin(RING_TILT);
    return out.set(out.x, out.y * cy - out.z * sy, out.y * sy + out.z * cy);
  };

  const linePositions = new Float32Array(COUNT * 6);
  const lineAlpha = new Float32Array(COUNT * 2);
  const lineTint = new Float32Array(COUNT * 6);
  const lines = new THREE.BufferGeometry();
  lines.setAttribute("position", new THREE.BufferAttribute(linePositions, 3));
  lines.setAttribute("alpha", new THREE.BufferAttribute(lineAlpha, 1));
  lines.setAttribute("tint", new THREE.BufferAttribute(lineTint, 3));
  const lineMaterial = new THREE.ShaderMaterial({ vertexShader, fragmentShader, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending });
  const streaks = new THREE.LineSegments(lines, lineMaterial);
  streaks.frustumCulled = false;

  const pointPositions = new Float32Array(COUNT * 3);
  const pointAlpha = new Float32Array(COUNT);
  const pointTint = new Float32Array(COUNT * 3);
  const pointSize = new Float32Array(COUNT);
  const points = new THREE.BufferGeometry();
  points.setAttribute("position", new THREE.BufferAttribute(pointPositions, 3));
  points.setAttribute("alpha", new THREE.BufferAttribute(pointAlpha, 1));
  points.setAttribute("tint", new THREE.BufferAttribute(pointTint, 3));
  points.setAttribute("size", new THREE.BufferAttribute(pointSize, 1));
  const dots = new THREE.Points(points, new THREE.ShaderMaterial({ vertexShader: pointVertex, fragmentShader: pointFragment, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
  dots.frustumCulled = false;

  const group = new THREE.Group();
  group.add(streaks, dots);

  const COLD = new THREE.Color(0.86, 0.9, 1.0);
  const STILL = new THREE.Color(0.55, 0.57, 0.62);
  const LOOP = new THREE.Color(0.66, 0.56, 1.0);
  const WARM = new THREE.Color(1.0, 0.86, 0.72);
  const now = new THREE.Vector3(), before = new THREE.Vector3(), target = new THREE.Vector3(), fallen = new THREE.Vector3();
  const tint = new THREE.Color();

  return {
    group,
    update(t, sub) {
      const W = workTime(t);
      const Wbefore = workTime(Math.max(0, t - TRAIL));
      // How many points exist: the film opens on one, and the river fills by bar 4.
      const alive = t < at(0.25) ? 0 : Math.max(1, Math.floor(COUNT * Math.pow(smooth(at(1), at(4.5), t), 2.2)));
      const fall = smooth(BEATS.fall, BEATS.crisis, t);                 // the work collapses overnight
      const dark = smooth(BEATS.crisis - BEAT, BEATS.crisis, t) * (1 - smooth(BEATS.reveal, BEATS.reveal + BAR, t));
      const loopTime = Math.max(0, t - BEATS.drop) * Math.PI * 2 / (BAR * 2) + Math.max(0, t - BEATS.reveal) * 0.25;
      const loopBefore = Math.max(0, t - TRAIL * 1.5 - BEATS.drop) * Math.PI * 2 / (BAR * 2) + Math.max(0, t - TRAIL * 1.5 - BEATS.reveal) * 0.25;
      const end = smooth(BEATS.end, BEATS.end + BAR * 2, t);
      const pulseAngle = ((t - BEATS.drop) / BAR) * Math.PI * 2; // a check travels the loop once per bar
      for (let i = 0; i < COUNT; i += 1) {
        const p = particles[i];
        river(p, W, now);
        river(p, Wbefore, before);
        const wrapped = Math.abs(now.x - before.x) > SPAN / 2;
        let a = i < alive ? p.bright : 0;
        let color = COLD;
        if (t >= BEATS.firstStop && W - Wbefore < 1e-4) { color = STILL; a = Math.min(1, a * 5 + 0.08); } // a photograph of chaos: every point exactly where it stopped
        if (fall > 0) {
          const drop = fall * fall * p.fall * 5.5;
          now.y -= drop; before.y -= drop;
          a *= 1 - fall * 0.7;
        }
        if (t >= BEATS.reveal) {
          // Gather: each point leaves the fallen work at its own moment, spirals in, and locks on the drop.
          fallen.copy(now);
          const leave = BEATS.reveal + p.delay * (BEATS.drop - BEATS.reveal) * 0.8;
          let k = easeInOut(smooth(leave, BEATS.drop, t));
          if (t >= BEATS.drop) k = 1;
          ring(p, loopTime, target);
          const swirl = (1 - k) * 2.4;
          target.applyAxisAngle(THREE.Object3D.DEFAULT_UP, swirl);
          now.lerpVectors(fallen, target, k);
          ring(p, loopBefore, before);
          before.applyAxisAngle(THREE.Object3D.DEFAULT_UP, (1 - k) * 2.4);
          if (k < 1) before.copy(now);
          a = p.bright * lerp(0.35, 1, k) * (i % 3 === 0 || k > 0.99 ? 1 : 0.6);
          color = LOOP;
          if (t >= BEATS.drop) {
            const theta = p.angle + loopTime * p.drift;
            const nearPulse = Math.cos(theta - pulseAngle) > 0.985 ? 1 : 0;
            a *= 0.75 + 0.25 * sub + nearPulse * 1.8;
          }
          if (end > 0) color = tint.copy(LOOP).lerp(WARM, end * 0.5);
        }
        a *= 1 - dark;
        const j = i * 6;
        linePositions[j] = wrapped ? now.x : before.x; linePositions[j + 1] = wrapped ? now.y : before.y; linePositions[j + 2] = wrapped ? now.z : before.z;
        linePositions[j + 3] = now.x; linePositions[j + 4] = now.y; linePositions[j + 5] = now.z;
        lineAlpha[i * 2] = 0; lineAlpha[i * 2 + 1] = a * 0.9;
        lineTint[j] = lineTint[j + 3] = color.r; lineTint[j + 1] = lineTint[j + 4] = color.g; lineTint[j + 2] = lineTint[j + 5] = color.b;
        pointPositions[i * 3] = now.x; pointPositions[i * 3 + 1] = now.y; pointPositions[i * 3 + 2] = now.z;
        pointAlpha[i] = a; pointSize[i] = i === 0 ? 0.28 : (0.05 + p.bright * 0.06) * (W - Wbefore < 1e-4 && t >= BEATS.firstStop && t < BEATS.reveal ? 1.6 : 1);
        pointTint[i * 3] = color.r; pointTint[i * 3 + 1] = color.g; pointTint[i * 3 + 2] = color.b;
      }
      for (const geometry of [lines, points]) for (const name of ["position", "alpha", "tint"]) geometry.attributes[name].needsUpdate = true;
      points.attributes.size.needsUpdate = true;
    },
  };
}
