// The edit: every cut lands on the score's grid; every camera move has a motive (see DIRECTION.md).
import * as THREE from "three";
import { BAR, BEATS, at, easeInOut, easeOut, lerp } from "./story.js";
import { LAYOUT } from "./room.js";

const v = (x, y, z) => new THREE.Vector3(x, y, z);
const mix = (a, b, k) => a.clone().lerp(b, k);

// A locked-off or moving shot between two camera states.
const move = (fromPos, toPos, fromTarget, toTarget, fov, ease = easeInOut) => (k) => ({ position: mix(fromPos, toPos, ease(k)), target: mix(fromTarget, toTarget, ease(k)), fov });

// Lens close-up: the camera sits just in front of the glasses, looking back into the left lens.
function lensShot(ctx, distance, drift, reveal = 0) {
  return (k) => {
    const glasses = ctx.room.developer.glasses;
    glasses.updateWorldMatrix(true, true);
    const lens = glasses.children[1].getWorldPosition(new THREE.Vector3());
    const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(glasses.getWorldQuaternion(new THREE.Quaternion()));
    const up = new THREE.Vector3(0, 1, 0);
    const side = forward.clone().cross(up).normalize();
    const pull = reveal * easeInOut(k);
    const position = lens.clone().addScaledVector(forward, distance - drift * k + pull).addScaledVector(side, 0.012 + pull * 0.35).addScaledVector(up, -0.006 + pull * 0.1);
    return { position, target: lens.clone().addScaledVector(side, pull * 0.12), fov: 15 + pull * 40, focus: position.distanceTo(lens) };
  };
}

function agentShot(ctx, index, offset, lookAhead, fov) {
  return () => {
    const agent = ctx.machine.agentPosition(index, ctx.t);
    return { position: agent.clone().add(offset), target: agent.clone().add(lookAhead), fov };
  };
}

export function createShots(ctx) {
  const room = (look = {}) => ({ world: "room", bloom: 0.55, aperture: 0.0, ...look });
  const machine = (look = {}) => ({ world: "machine", bloom: 1.0, bloomThreshold: 0.5, aperture: 0.0, ...look });
  const titles = (look = {}) => ({ world: "titles", bloom: 1.0, bloomThreshold: 0.3, aperture: 0, ...look });
  const M = LAYOUT.monitor;
  const gate = (k, offset) => () => ({ position: ctx.machine.leadGate(k).add(offset), target: ctx.machine.leadGate(k).add(v(0, 0.3, 4)), fov: 40 });

  // [startBar, endBar, look, camera(k)]
  const list = [
    // ACT I — the work
    [0, 1, titles(), null],
    [1, 3, room({ aperture: 0.05, maxBlur: 0.02 }), lensShot(ctx, 0.17, 0.0, 0.42)],
    [3, 4, machine(), agentShot(ctx, 0, v(1.5, 0.7, 3.4), v(-0.4, 0.1, -6), 46)],
    [4, 6, room({ aperture: 0.0012 }), move(v(-4.7, 1.6, 2.4), v(-0.4, 1.45, 2.8), v(-1.6, 1.05, -2.4), v(-0.6, 1.1, -2.4), 38)],
    [6, 7, room({ aperture: 0.03, maxBlur: 0.02, focusFrom: 0.3, focusTo: 1.35 }), move(v(-2.62, 1.0, -2.3), v(-2.58, 1.06, -2.34), LAYOUT.note.clone(), LAYOUT.clock.clone(), 40)],
    [7, 8, machine(), agentShot(ctx, 0, v(0.25, 0.65, 3.2), v(0, 0.2, -12), 44)],
    // The Stop
    [8, 8.5, machine(), gate(0, v(0.9, 0.55, -3.8))],
    [8.5, 9.5, room({ aperture: 0.004 }), move(v(-2.43, 1.2, -2.04), v(-2.44, 1.2, -2.13), v(-2.45, 1.19, -2.62), v(-2.45, 1.19, -2.62), 36)],
    [9.5, 10, room({ aperture: 0.08, maxBlur: 0.02 }), lensShot(ctx, 0.15, 0.02)],
    // ACT II — they become the loop
    [10, 11, room({ aperture: 0.004 }), move(v(-1.05, 1.2, -1.35), v(-1.2, 1.18, -1.45), v(-2.2, 1.12, -1.75), v(-2.2, 1.12, -1.75), 40)],
    [11, 11.5, machine(), agentShot(ctx, 0, v(-1.5, 0.7, 3.4), v(0.4, 0.1, -6), 46)],
    [11.5, 12, machine(), gate(1, v(0.9, 0.55, -3.8))],
    [12, 12.5, room({ aperture: 0.004 }), move(v(-2.47, 1.19, -2.22), v(-2.47, 1.19, -2.27), v(-2.47, 1.18, -2.62), v(-2.47, 1.18, -2.62), 30)],
    [12.5, 13, room({ aperture: 0.002 }), move(v(-1.55, 1.48, -0.85), v(-1.6, 1.46, -0.95), v(-2.25, 1.12, -2.5), v(-2.25, 1.12, -2.5), 42)],
    [13, 14, room({ aperture: 0.012 }), move(v(-2.79, 0.99, -2.2), v(-2.8, 0.96, -2.23), LAYOUT.phoneOnDesk.clone(), LAYOUT.phoneOnDesk.clone(), 38)],
    [14, 14.5, room({ aperture: 0.0 }), move(v(2.4, 2.0, -2.5), v(2.4, 2.0, -2.5), LAYOUT.neighbour.clone(), LAYOUT.neighbour.clone().add(v(0, -0.05, 0)), 6.5)],
    [14.5, 15, room({ aperture: 0.002 }), move(v(-1.55, 1.48, -0.85), v(-1.62, 1.44, -1.0), v(-2.25, 1.12, -2.5), v(-2.25, 1.12, -2.5), 42)],
    [15, 16, room({ aperture: 0.003 }), move(v(-2.2, 1.55, -0.45), v(-2.2, 1.45, -0.7), v(-2.2, 0.95, -2.4), v(-2.2, 0.92, -2.4), 40)],
    [16, 19, machine(), (k) => {
      const lead = ctx.machine.agentPosition(0, ctx.t);
      const rise = easeInOut(k);
      return { position: lead.clone().add(v(lerp(1.6, 4, rise), lerp(0.6, 26, rise), lerp(2.2, 20, rise))), target: lead.clone().add(v(lerp(0, -1, rise), 0, lerp(-2, -10, rise))), fov: lerp(46, 44, rise) };
    }],
    [19, 21, room({ aperture: 0.0 }), move(v(3.3, 2.7, 3.3), v(3.1, 2.6, 3.0), v(-0.9, 0.3, -1.3), v(-1.1, 0.35, -1.4), 52, (k) => k)],
    [21, 23, room({ aperture: 0.03, maxBlur: 0.018 }), move(v(-1.28, 0.97, -1.92), v(-1.4, 0.88, -2.13), LAYOUT.orb.clone(), LAYOUT.orb.clone(), 30, (k) => k)],
    [23, 24, machine(), (k) => {
      // While they sleep: four agents pulse at four gates, a slow dolly along the line of the trapped.
      const a = ctx.machine.agentPosition(3, ctx.t), b = ctx.machine.agentPosition(1, ctx.t);
      const along = a.clone().lerp(b, easeInOut(k));
      return { position: along.clone().add(v(8, 2.6, 5)), target: along.clone().add(v(-1.5, 0.4, -2)), fov: 36 };
    }],
    // ACT III — crisis
    [24, 25, room({ aperture: 0.004 }), move(v(-1.05, 1.2, -1.35), v(-1.1, 1.22, -1.38), v(-2.2, 1.1, -1.8), v(-2.2, 1.15, -1.75), 40, easeOut)],
    [25, 26, room({ aperture: 0.002 }), move(v(-1.5, 1.4, -0.95), v(-1.58, 1.38, -1.08), v(-2.4, 1.28, -2.62), v(-2.4, 1.28, -2.62), 40)],
    [26, 27, room({ aperture: 0.015 }), move(v(-2.2, 0.97, -2.52), v(-2.2, 0.96, -2.5), v(-2.2, 0.82, -2.02), v(-2.2, 0.82, -2.02), 62)],
    [27, 28, machine(), gate(5, v(0.8, 0.45, -3.2))],
    [28, 28.6, room({ aperture: 0.004 }), move(v(-1.1, 1.22, -1.5), v(-1.15, 1.22, -1.52), v(-2.2, 1.18, -1.75), v(-2.2, 1.18, -1.75), 38)],
    [28.6, 29.3, room({ aperture: 0.004 }), move(v(-2.62, 1.36, -1.22), v(-2.58, 1.37, -1.26), v(2.2, 1.9, -3.0), v(2.4, 2.0, -3.0), 44)],
    [29.3, 30, room({ aperture: 0.02 }), move(v(-1.95, 1.2, -1.95), v(-1.9, 1.17, -2.0), LAYOUT.orb.clone(), LAYOUT.orb.clone(), 16)],
    [30, 32, room({ aperture: 0.004 }), move(v(-1.94, 1.36, -2.3), v(-1.9, 1.34, -2.34), v(-1.94, 1.36, -2.62), v(-1.9, 1.33, -2.62), 30)],
    // The loop closes
    [32, 33, machine(), (k) => {
      const center = ctx.machine.ringCenter;
      const angle = lerp(-0.6, 0.5, easeOut(k));
      const radius = lerp(24, 17, easeOut(k));
      return { position: center.clone().add(v(Math.sin(angle) * radius, lerp(3, 7, k), Math.cos(angle) * radius)), target: center.clone().add(v(0, 2, 0)), fov: 48 };
    }],
    [33, 34, room({ aperture: 0.0 }), move(v(-0.55, 0.95, -1.15), v(-0.6, 1.05, -1.2), v(-2.2, 1.0, -1.62), v(-2.2, 1.3, -1.55), 52)],
    [34, 35, room({ aperture: 0.004 }), move(v(-2.2, 1.2, -1.95), v(-2.2, 1.2, -2.04), v(-2.2, 1.19, -2.62), v(-2.2, 1.19, -2.62), 44)],
    [35, 36, room({ aperture: 0.004 }), () => {
      const walker = ctx.room.developer.joints.chest.getWorldPosition(new THREE.Vector3());
      return { position: v(walker.x * 0.6 + 0.2, 1.05, 0.9), target: walker.clone().add(v(0.35, 0.1, 0)), fov: 48, focus: 2.4 };
    }],
    [36, 37, machine(), (k) => {
      const lead = ctx.machine.agentPosition(0, ctx.t);
      const center = ctx.machine.ringCenter;
      const outward = lead.clone().sub(center).setY(0).normalize();
      const tangent = new THREE.Vector3(outward.z, 0, -outward.x);
      return { position: lead.clone().addScaledVector(outward, 2.2).add(v(0, 0.7, 0)).addScaledVector(tangent, -1.5), target: lead.clone().addScaledVector(tangent, -6).add(v(0, 0.2, 0)), fov: 55 };
    }],
    [37, 38, room({ aperture: 0.01 }), () => {
      const phone = ctx.room.phone.position.clone();
      const head = ctx.room.developer.joints.head.getWorldPosition(new THREE.Vector3()).add(v(0, 0.1, 0));
      const position = head.clone().add(v(0, 0.04, 0.1));
      return { position, target: phone, fov: 42, up: new THREE.Vector3(0, 0, -1).applyQuaternion(ctx.room.phone.quaternion) };
    }],
    [38, 39, room({ aperture: 0.0 }), move(v(1.9, 1.75, 3.9), v(1.9, 1.75, 3.9), v(0.2, 1.0, -2.3), v(0.2, 1.0, -2.3), 44)],
    [39, 40, machine(), (k) => ({ position: ctx.machine.ringCenter.clone().add(v(0, lerp(26, 18, easeInOut(k)), 0.01)), target: ctx.machine.ringCenter.clone(), fov: 52 })],
    // Morning
    [40, 41.5, room({ aperture: 0.0 }), move(v(-1.7, 1.35, 2.3), v(-1.4, 1.3, 2.0), v(1.5, 1.6, -3.0), v(1.8, 1.7, -3.0), 50)],
    [41.5, 42.5, room({ aperture: 0.003 }), move(v(0.45, 1.05, -1.0), v(0.52, 1.08, -1.1), v(2.55, 1.0, -2.3), v(2.55, 1.05, -2.3), 42)],
    [42.5, 43.2, room({ aperture: 0.0 }), move(v(1.5, 1.45, 1.3), v(1.55, 1.48, 1.0), v(1.9, 1.8, -3.0), v(1.9, 1.85, -3.0), 44)],
    [43.2, 44, room({ aperture: 0.08, maxBlur: 0.02 }), lensShot(ctx, 0.16, 0.03)],
    [44, 48, titles(), null],
  ];

  const shots = list.map(([startBar, endBar, look, camera]) => ({ start: at(startBar), end: at(endBar), look, camera }));

  return {
    shots,
    find(t) { return shots.find((shot) => t >= shot.start && t < shot.end) ?? shots[shots.length - 1]; },
    // Flashes and fades that belong to the cut, not the shot.
    transitions(t) {
      let flash = 0, fade = 0, flashColor = 0xffffff;
      if (t >= BEATS.click && t < BEATS.click + 0.35) { flash = 0.85 * (1 - (t - BEATS.click) / 0.35); flashColor = 0xe6ddff; }
      if (t >= at(39.5) && t < at(40)) flash = Math.pow((t - at(39.5)) / (BAR / 2), 2);
      if (t >= at(40) && t < at(40) + 0.9) { flash = 1 - (t - at(40)) / 0.9; flashColor = 0xfff0d8; }
      if (t >= at(8) - 0.25 && t < at(8)) fade = (t - (at(8) - 0.25)) / 0.25 * 0.5;
      if (t >= at(47.3)) fade = Math.min(1, (t - at(47.3)) / (at(48) - at(47.3)));
      return { flash, fade, flashColor };
    },
  };
}
