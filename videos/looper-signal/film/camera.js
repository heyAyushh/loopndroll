// The edit. Cuts land on the bar; each shot changes scale or angle for a reason.
import * as THREE from "three";
import { BAR, BEAT, BEATS, at, easeInOut, easeOut, lerp, smooth } from "./story.js";
import { RING_RADIUS, RING_TILT } from "./field.js";

const v = (x, y, z) => new THREE.Vector3(x, y, z);
const mix = (a, b, k) => a.clone().lerp(b, k);

// [startBar, endBar, (k, t) => { position, target, fov, focus?, aperture? }]
const SHOTS = [
  // The first light, alone in the dark.
  [0, 1.5, () => ({ position: v(-4.6, 0.3, 9), target: v(-4.6, 0, 0), fov: 34 })],
  // Inside the river: the work rushing past the lens.
  [1.5, 4, (k) => ({ position: v(lerp(-1, 1.5, k), 0.2, 2.4), target: v(lerp(4, 6, k), -0.2, -1.2), fov: 44, focus: 4.5, aperture: 0.004 })],
  // Rise: the river as a whole, a shape made of random paths.
  [4, 7.75, (k) => ({ position: mix(v(0, 1.2, 9), v(0.6, 3.2, 16), easeInOut(k)), target: v(0.4, -0.2, 0), fov: 36 })],
  // Bullet time: the work is frozen; you are not. The camera drifts through the stopped points.
  [7.75, 10, (k) => ({ position: mix(v(0.6, 3.2, 16), v(-1.2, 0.5, 6.5), easeInOut(k)), target: mix(v(0.4, -0.2, 0), v(1.5, 0, 0), k), fov: 38, focus: 6, aperture: 0.002 })],
  [10, 11, (k) => ({ position: v(4.5, 0.4, 7.5), target: v(0, 0, 0), fov: 40 })],
  [11, 11.75, (k) => ({ position: mix(v(-3, -0.6, 4.2), v(-2.6, -0.5, 3.8), k), target: v(1, 0.2, -1), fov: 44, focus: 4, aperture: 0.004 })],
  [11.75, 12.5, () => ({ position: v(0, 6, 9), target: v(0, 0, 0), fov: 38 })],
  [12.5, 13, (k) => ({ position: mix(v(2.2, 0.3, 3.4), v(2.0, 0.3, 3.1), k), target: v(-2, 0, -1), fov: 46, focus: 4, aperture: 0.004 })],
  [13, 13.5, () => ({ position: v(-6, 1, 10), target: v(0, 0, 0), fov: 36 })],
  [13.5, 14, (k) => ({ position: mix(v(0.4, 0.1, 2.2), v(0.3, 0.1, 1.9), k), target: v(0.4, 0, -4), fov: 50, focus: 3, aperture: 0.005 })],
  // The work collapses; dust rains past a camera that looks up into it.
  [14, 16, (k) => ({ position: mix(v(0, -3.6, 9), v(0, -4.4, 8.4), k), target: v(0, -1.5, 0), fov: 42 })],
  // Darkness (the camera waits where the orb will be found).
  [16, 17.5, () => ({ position: v(0.3, 0.1, 2.3), target: v(0, 0, 0), fov: 30 })],
  // Macro on glass, light sliding across it; then back to the whole object.
  [17.5, 19, (k) => ({ position: mix(v(0.55, 0.35, 2.05), v(0.4, 0.3, 2.6), k), target: v(0.2, 0.25, 0), fov: 30, focus: 1.35, aperture: 0.006 })],
  [19, 20, (k) => ({ position: mix(v(0, 0.2, 5.5), v(0, 0.3, 8.8), easeOut(k)), target: v(0, 0, 0), fov: 34 })],
  // The drop: one decisive swing as the loop locks.
  [20, 21, (k) => { const a = lerp(0, Math.PI / 2, easeOut(k * 1.6)); return { position: v(Math.sin(a) * 8.8, lerp(0.3, 1.6, easeOut(k)), Math.cos(a) * 8.8), target: v(0, 0, 0), fov: 36 }; }],
  // Riding the loop: the same points as the river, now going around (a rhyme).
  [21, 22.75, (k, t) => {
    const theta = (t - BEATS.drop) * Math.PI * 2 / (BAR * 2) * 0.35 + 0.8;
    const onRing = (a, r) => { const p = v(Math.cos(a) * r, 0, Math.sin(a) * r); p.applyAxisAngle(v(1, 0, 0), RING_TILT); return p; };
    return { position: onRing(theta, RING_RADIUS + 0.7).add(v(0, 0.25, 0)), target: onRing(theta + 0.9, RING_RADIUS), fov: 50, focus: 2.2, aperture: 0.003 };
  }],
  [22.75, 28, (k, t) => { const a = Math.PI / 2 + (t - at(22.75)) * 0.09; return { position: v(Math.sin(a) * lerp(8.6, 9.8, k), lerp(1.6, 0.8, k), Math.cos(a) * lerp(8.6, 9.8, k)), target: v(0, 0, 0), fov: 36 }; }],
  // The name.
  [28, 32, (k, t) => { const a = Math.PI / 2 + (t - at(22.75)) * 0.09; const settle = easeInOut(smooth(0, 0.32, k)); const r = lerp(9.8, 15.5, settle); return { position: v(Math.sin(a) * r, 0.9, Math.cos(a) * r), target: v(0, lerp(0, -1.9, settle), 0), fov: 36 }; }],
];

export function frameCamera(camera, t) {
  const shot = SHOTS.find(([a, b]) => t >= at(a) && t < at(b)) ?? SHOTS[SHOTS.length - 1];
  const [a, b, fn] = shot;
  const state = fn((t - at(a)) / (at(b) - at(a)), t);
  camera.position.copy(state.position);
  camera.lookAt(state.target);
  camera.fov = state.fov;
  camera.updateProjectionMatrix();
  return { focus: state.focus ?? state.position.distanceTo(state.target), aperture: state.aperture ?? 0 };
}
