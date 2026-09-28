// The developer: a jointed figure in a hoodie and glasses, acted by keyframes on the story clock.
import * as THREE from "three";
import { BAR, BEAT, BEATS, at, clamp, easeInOut, lerp, smooth } from "./story.js";

const HOODIE = new THREE.MeshStandardMaterial({ color: 0x2b2a33, roughness: 0.95 });
const PANTS = new THREE.MeshStandardMaterial({ color: 0x16161b, roughness: 0.9 });
const SKIN = new THREE.MeshStandardMaterial({ color: 0x6b4c40, roughness: 0.7 });
const HAIR = new THREE.MeshStandardMaterial({ color: 0x0c0a0b, roughness: 0.8 });
const SHOES = new THREE.MeshStandardMaterial({ color: 0x0e0e11, roughness: 0.6 });
const FRAME = new THREE.MeshStandardMaterial({ color: 0x050506, roughness: 0.3, metalness: 0.4 });

function capsule(radius, length, material, offsetY, scale = [1, 1, 1]) {
  const mesh = new THREE.Mesh(new THREE.CapsuleGeometry(radius, length, 6, 14), material);
  mesh.position.y = offsetY;
  mesh.scale.set(...scale);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

function joint(parent, x, y, z) {
  const group = new THREE.Group();
  group.position.set(x, y, z);
  parent.add(group);
  return group;
}

// Pose angles per joint: [x, y, z]. Local forward is +z; limbs hang along -y.
// Positive spine/neck x leans forward; negative thigh/upper-arm x lifts forward.
const SEATED_LEGS = { thighL: [-1.5, 0.05, 0.04], thighR: [-1.5, -0.05, -0.04], shinL: [1.6, 0, 0], shinR: [1.55, 0, 0], footL: [-0.1, 0, 0], footR: [-0.1, 0, 0] };
const POSES = {
  typing: { pelvis: [0, 0, 0], spine: [0.12, 0, 0], chest: [0.1, 0, 0], neck: [0.08, 0, 0], head: [0.06, 0, 0], upperArmL: [-0.92, 0, 0.14], upperArmR: [-0.92, 0, -0.14], forearmL: [-1.02, -0.28, 0], forearmR: [-1.02, 0.28, 0], handL: [-0.1, 0, 0], handR: [-0.1, 0, 0], ...SEATED_LEGS },
  stare: { pelvis: [0, 0, 0], spine: [0.04, 0, 0], chest: [0.02, 0, 0], neck: [0.02, 0, 0], head: [-0.02, 0, 0], upperArmL: [-0.8, 0, 0.2], upperArmR: [-0.8, 0, -0.2], forearmL: [-0.95, -0.35, 0], forearmR: [-0.95, 0.35, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  rubEyes: { pelvis: [-0.08, 0, 0], spine: [-0.2, 0, 0], chest: [-0.12, 0, 0], neck: [0.1, 0, 0], head: [0.25, 0, 0], upperArmL: [-1.75, 0, 0.42], upperArmR: [-1.75, 0, -0.42], forearmL: [-2.25, 0.4, 0], forearmR: [-2.25, -0.4, 0], handL: [-0.3, 0, 0], handR: [-0.3, 0, 0], ...SEATED_LEGS },
  slumpTyping: { pelvis: [0.05, 0, 0], spine: [0.3, 0, 0], chest: [0.25, 0, 0], neck: [0.2, 0, 0], head: [0.1, 0, 0], upperArmL: [-0.62, 0, 0.16], upperArmR: [-0.62, 0, -0.16], forearmL: [-0.95, -0.28, 0], forearmR: [-0.95, 0.28, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  phoneTap: { pelvis: [0.05, 0, 0], spine: [0.3, 0.25, 0], chest: [0.25, 0.2, 0], neck: [0.3, 0.1, 0], head: [0.35, 0.2, 0], upperArmL: [-0.85, 0, 0.55], upperArmR: [-0.6, 0, -0.18], forearmL: [-0.8, 0.1, 0], forearmR: [-0.9, -0.2, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  asleepDesk: { pelvis: [0.1, 0, 0], spine: [0.55, 0, 0], chest: [0.5, 0, 0], neck: [0.35, 0, 0], head: [0.45, 0.35, 0.15], upperArmL: [-1.05, 0.3, 0.5], upperArmR: [-1.05, -0.3, -0.5], forearmL: [-1.2, 1.1, 0], forearmR: [-1.2, -1.1, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  hover: { pelvis: [0, 0, 0], spine: [0.1, 0, 0], chest: [0.08, 0, 0], neck: [0.05, 0, 0], head: [0.1, 0, 0], upperArmL: [-0.95, 0, 0.14], upperArmR: [-0.95, 0, -0.14], forearmL: [-1.12, -0.28, 0], forearmR: [-1.12, 0.28, 0], handL: [0.3, 0, 0], handR: [0.3, 0, 0], ...SEATED_LEGS },
  lookWindow: { pelvis: [0, -0.1, 0], spine: [0.02, -0.25, 0], chest: [0, -0.3, 0], neck: [0, -0.35, 0], head: [-0.08, -0.5, 0], upperArmL: [-0.45, 0, 0.18], upperArmR: [-0.4, 0, -0.18], forearmL: [-0.9, -0.3, 0], forearmR: [-0.8, 0.3, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  lookOrb: { pelvis: [0, -0.05, 0], spine: [0.1, -0.12, 0], chest: [0.08, -0.15, 0], neck: [0.2, -0.2, 0], head: [0.3, -0.35, 0], upperArmL: [-0.45, 0, 0.18], upperArmR: [-0.5, 0, -0.2], forearmL: [-0.9, -0.3, 0], forearmR: [-0.9, 0.3, 0], handL: [0, 0, 0], handR: [0, 0, 0], ...SEATED_LEGS },
  mouse: { pelvis: [0, 0, 0], spine: [0.1, -0.05, 0], chest: [0.08, -0.05, 0], neck: [0.05, 0.05, 0], head: [0.04, 0.08, 0], upperArmL: [-0.7, 0, 0.2], upperArmR: [-0.9, 0, -0.42], forearmL: [-0.95, -0.35, 0], forearmR: [-1.0, 0.05, 0], handL: [0, 0, 0], handR: [-0.1, 0, 0], ...SEATED_LEGS },
  standing: { pelvis: [0, 0, 0], spine: [0.02, 0, 0], chest: [0, 0, 0], neck: [0, 0, 0], head: [0.03, 0, 0], upperArmL: [0.05, 0, 0.1], upperArmR: [0.05, 0, -0.1], forearmL: [-0.25, 0, 0], forearmR: [-0.35, 0, 0], handL: [0, 0, 0], handR: [0, 0, 0], thighL: [0, 0, 0.02], thighR: [0, 0, -0.02], shinL: [0.03, 0, 0], shinR: [0.03, 0, 0], footL: [0, 0, 0], footR: [0, 0, 0] },
  risingFromChair: { pelvis: [0.35, 0, 0], spine: [0.45, 0, 0], chest: [0.2, 0, 0], neck: [0, 0, 0], head: [-0.1, 0, 0], upperArmL: [-0.5, 0, 0.2], upperArmR: [-0.5, 0, -0.2], forearmL: [-0.4, 0, 0], forearmR: [-0.4, 0, 0], handL: [0, 0, 0], handR: [0, 0, 0], thighL: [-0.9, 0, 0.04], thighR: [-0.9, 0, -0.04], shinL: [0.9, 0, 0], shinR: [0.9, 0, 0], footL: [0, 0, 0], footR: [0, 0, 0] },
  lying: { pelvis: [-1.52, 0, 0], spine: [-0.05, 0, 0], chest: [0, 0, 0], neck: [-0.1, 0, 0], head: [-0.15, 0.35, 0], upperArmL: [-0.2, 0, 0.35], upperArmR: [-0.35, 0, -0.1], forearmL: [-1.9, 0.3, 0], forearmR: [-1.4, -0.4, 0], handL: [0, 0, 0], handR: [0, 0, 0], thighL: [-0.55, 0, 0.05], thighR: [-0.25, 0, -0.05], shinL: [1.1, 0, 0], shinR: [0.35, 0, 0], footL: [0.3, 0, 0], footR: [0.2, 0, 0] },
  lyingPhone: { pelvis: [-1.52, 0, 0], spine: [-0.05, 0, 0], chest: [0, 0, 0], neck: [0.05, 0, 0], head: [0.1, 0.1, 0], upperArmL: [-0.2, 0, 0.35], upperArmR: [-1.55, 0, -0.05], forearmL: [-1.9, 0.3, 0], forearmR: [-1.35, 0, 0], handL: [0, 0, 0], handR: [-0.3, 0, 0], thighL: [-0.55, 0, 0.05], thighR: [-0.25, 0, -0.05], shinL: [1.1, 0, 0], shinR: [0.35, 0, 0], footL: [0.3, 0, 0], footR: [0.2, 0, 0] },
  sittingStretch: { pelvis: [0, 0, 0], spine: [-0.12, 0, 0], chest: [-0.1, 0, 0], neck: [-0.15, 0, 0], head: [-0.25, 0, 0], upperArmL: [-2.95, 0, 0.35], upperArmR: [-2.95, 0, -0.35], forearmL: [-0.3, 0, 0], forearmR: [-0.3, 0, 0], handL: [0, 0, 0], handR: [0, 0, 0], thighL: [-1.45, 0.1, 0.1], thighR: [-1.45, -0.1, -0.1], shinL: [1.45, 0, 0], shinR: [1.45, 0, 0], footL: [0, 0, 0], footR: [0, 0, 0] },
};

// The acting track: [time, pose, [x, y, z] root position, root yaw]. Desk chair faces -z (yaw π).
const DESK = [-2.2, 0.0, -1.62];
const DESK_YAW = Math.PI;
const SEAT_HEIGHT = 0.5;
const COUCH = [2.55, 0.0, -2.25];
const TRACK = [
  [0, "typing", DESK, DESK_YAW],
  [BEATS.firstStop - 0.2, "typing", DESK, DESK_YAW],
  [BEATS.firstStop + 0.25, "stare", DESK, DESK_YAW],
  [BEATS.rubEyes, "stare", DESK, DESK_YAW],
  [BEATS.rubEyes + 0.5, "rubEyes", DESK, DESK_YAW],
  [BEATS.firstYes - 0.35, "rubEyes", DESK, DESK_YAW],
  [BEATS.firstYes - 0.05, "typing", DESK, DESK_YAW],
  [at(12), "typing", DESK, DESK_YAW],
  [at(12, 0.6), "slumpTyping", DESK, DESK_YAW],
  [at(13), "slumpTyping", DESK, DESK_YAW],
  [at(13, 0.6), "phoneTap", DESK, DESK_YAW],
  [at(14), "phoneTap", DESK, DESK_YAW],
  [at(14, 0.6), "slumpTyping", DESK, DESK_YAW],
  [at(15, 0.5), "slumpTyping", DESK, DESK_YAW],
  [BEATS.asleep + 0.4, "asleepDesk", DESK, DESK_YAW],
  [BEATS.buzz, "asleepDesk", DESK, DESK_YAW],
  [BEATS.buzz + 0.25, "stare", DESK, DESK_YAW],
  [BEATS.hesitate, "stare", DESK, DESK_YAW],
  [BEATS.hesitate + 0.3, "hover", DESK, DESK_YAW],
  [at(27, 2), "hover", DESK, DESK_YAW],
  [at(28), "stare", DESK, DESK_YAW],
  [BEATS.lookWindow, "lookWindow", DESK, DESK_YAW],
  [BEATS.lookOrb - 0.2, "lookWindow", DESK, DESK_YAW],
  [BEATS.lookOrb + 0.2, "lookOrb", DESK, DESK_YAW],
  [BEATS.pointer, "lookOrb", DESK, DESK_YAW],
  [BEATS.pointer + 0.4, "mouse", DESK, DESK_YAW],
  [BEATS.click + 0.15, "mouse", DESK, DESK_YAW],
  [BEATS.standUp - 0.3, "stare", DESK, DESK_YAW],
  [BEATS.standUp + 0.35, "risingFromChair", [DESK[0], 0.2, DESK[2] - 0.05], DESK_YAW],
  [BEATS.standUp + 0.9, "standing", [DESK[0], 0.42, DESK[2] + 0.1], DESK_YAW],
  [BEATS.walkStart, "standing", [DESK[0] + 0.05, 0.42, DESK[2] + 0.3], DESK_YAW - 1.2],
  [BEATS.onCouch - 0.8, "walk", [COUCH[0] - 0.6, 0.42, COUCH[2] + 0.75], -Math.PI / 2 + 0.5],
  [BEATS.onCouch, "standing", [COUCH[0], 0.42, COUCH[2] + 0.55], 0],
  [BEATS.onCouch + 0.7, "lying", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [at(36, 2.8), "lying", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [at(37) - 0.02, "lyingPhone", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [BEATS.asleepCouch - 0.2, "lyingPhone", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [BEATS.asleepCouch + 0.4, "lying", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [BEATS.wake, "lying", [COUCH[0] + 0.2, 0.08, COUCH[2] + 0.1], Math.PI / 2],
  [BEATS.wake + 0.6, "sittingStretch", [COUCH[0], 0.02, COUCH[2] + 0.25], 0],
  [BEATS.toWindow - 0.1, "sittingStretch", [COUCH[0], 0.02, COUCH[2] + 0.25], 0],
  [BEATS.toWindow + 0.7, "standing", [COUCH[0] - 0.9, 0.42, COUCH[2] + 0.7], Math.PI],
  [at(48), "standing", [COUCH[0] - 0.9, 0.42, COUCH[2] + 0.7], Math.PI],
];

export function createDeveloper(lensTexture) {
  const root = new THREE.Group();
  const j = {};
  j.pelvis = joint(root, 0, SEAT_HEIGHT, 0);
  j.pelvis.add(capsule(0.15, 0.08, PANTS, 0.02, [1.15, 0.75, 0.85]));
  j.spine = joint(j.pelvis, 0, 0.08, 0);
  j.spine.add(capsule(0.145, 0.1, HOODIE, 0.08, [1.1, 1, 0.8]));
  j.chest = joint(j.spine, 0, 0.18, 0);
  j.chest.add(capsule(0.175, 0.14, HOODIE, 0.1, [1.12, 1, 0.78]));
  const hood = new THREE.Mesh(new THREE.SphereGeometry(0.14, 20, 14, 0, Math.PI * 2, 0, Math.PI * 0.55), HOODIE);
  hood.position.set(0, 0.24, -0.08); hood.rotation.x = -0.6; hood.castShadow = true;
  j.chest.add(hood);
  j.neck = joint(j.chest, 0, 0.26, 0.01);
  j.neck.add(capsule(0.048, 0.05, SKIN, 0.03));
  j.head = joint(j.neck, 0, 0.08, 0);
  const skull = new THREE.Mesh(new THREE.SphereGeometry(0.1, 28, 20), SKIN);
  skull.scale.set(0.93, 1.15, 1.02); skull.position.y = 0.1; skull.castShadow = true;
  j.head.add(skull);
  const hair = new THREE.Mesh(new THREE.SphereGeometry(0.104, 28, 20, 0, Math.PI * 2, 0, Math.PI * 0.55), HAIR);
  hair.scale.set(0.97, 1.12, 1.05); hair.position.set(0, 0.115, -0.012); hair.rotation.x = -0.35;
  j.head.add(hair);
  const nose = new THREE.Mesh(new THREE.SphereGeometry(0.018, 10, 8), SKIN);
  nose.position.set(0, 0.09, 0.1); nose.scale.set(0.8, 1.2, 1);
  j.head.add(nose);
  // Eyes and ears: enough face to be a person, not a mannequin.
  const EYE = new THREE.MeshStandardMaterial({ color: 0x0a0808, roughness: 0.15 });
  for (const side of [-1, 1]) {
    const eye = new THREE.Mesh(new THREE.SphereGeometry(0.011, 12, 10), EYE);
    eye.position.set(side * 0.034, 0.118, 0.088); eye.scale.set(1.2, 0.8, 0.6);
    j.head.add(eye);
    const brow = new THREE.Mesh(new THREE.CapsuleGeometry(0.004, 0.022, 3, 6), HAIR);
    brow.rotation.z = Math.PI / 2 + side * 0.12; brow.position.set(side * 0.034, 0.148, 0.093);
    j.head.add(brow);
    const ear = new THREE.Mesh(new THREE.SphereGeometry(0.022, 10, 8), SKIN);
    ear.position.set(side * 0.094, 0.105, -0.005); ear.scale.set(0.45, 1, 0.8);
    j.head.add(ear);
  }
  const mouth = new THREE.Mesh(new THREE.CapsuleGeometry(0.003, 0.022, 3, 6), new THREE.MeshStandardMaterial({ color: 0x4a3230, roughness: 0.6 }));
  mouth.rotation.z = Math.PI / 2; mouth.position.set(0, 0.052, 0.094);
  j.head.add(mouth);
  const lensMaterial = new THREE.MeshBasicMaterial({ map: lensTexture, transparent: true, opacity: 0.93, depthWrite: false, toneMapped: false });
  const glasses = new THREE.Group();
  glasses.position.set(0, 0.115, 0.096);
  for (const side of [-1, 1]) {
    const rim = new THREE.Mesh(new THREE.TorusGeometry(0.028, 0.0038, 8, 36), FRAME);
    rim.position.x = side * 0.034; glasses.add(rim);
    const lens = new THREE.Mesh(new THREE.CircleGeometry(0.027, 36), lensMaterial);
    lens.position.set(side * 0.034, 0, 0.002); glasses.add(lens);
    const temple = new THREE.Mesh(new THREE.BoxGeometry(0.003, 0.004, 0.1), FRAME);
    temple.position.set(side * 0.064, 0.005, -0.05); glasses.add(temple);
  }
  const bridge = new THREE.Mesh(new THREE.BoxGeometry(0.014, 0.003, 0.003), FRAME);
  bridge.position.y = 0.006; glasses.add(bridge);
  j.head.add(glasses);

  for (const [side, sign] of [["L", 1], ["R", -1]]) {
    j[`upperArm${side}`] = joint(j.chest, sign * 0.2, 0.2, -0.01);
    j[`upperArm${side}`].add(capsule(0.052, 0.2, HOODIE, -0.13));
    j[`forearm${side}`] = joint(j[`upperArm${side}`], 0, -0.28, 0);
    j[`forearm${side}`].add(capsule(0.045, 0.19, HOODIE, -0.12));
    j[`hand${side}`] = joint(j[`forearm${side}`], 0, -0.26, 0);
    j[`hand${side}`].add(capsule(0.034, 0.05, SKIN, -0.05, [1, 1, 0.55]));
    j[`thigh${side}`] = joint(j.pelvis, sign * 0.095, -0.02, 0);
    j[`thigh${side}`].add(capsule(0.078, 0.3, PANTS, -0.21));
    j[`shin${side}`] = joint(j[`thigh${side}`], 0, -0.43, 0);
    j[`shin${side}`].add(capsule(0.06, 0.32, PANTS, -0.21));
    j[`foot${side}`] = joint(j[`shin${side}`], 0, -0.43, 0);
    const foot = new THREE.Mesh(new THREE.BoxGeometry(0.1, 0.07, 0.25), SHOES);
    foot.position.set(0, -0.02, 0.06); foot.castShadow = true;
    j[`foot${side}`].add(foot);
  }

  const blendPose = (a, b, k) => {
    for (const name of Object.keys(POSES.standing)) {
      const from = (POSES[a] ?? POSES.standing)[name] ?? [0, 0, 0];
      const to = (POSES[b] ?? POSES.standing)[name] ?? [0, 0, 0];
      j[name].rotation.set(lerp(from[0], to[0], k), lerp(from[1], to[1], k), lerp(from[2], to[2], k));
    }
  };

  return {
    root,
    joints: j,
    glasses,
    update(t, sub) {
      let index = TRACK.findIndex(([time]) => time > t);
      if (index === -1) index = TRACK.length - 1;
      const [t0, poseA, posA, yawA] = TRACK[Math.max(0, index - 1)];
      const [t1, poseB, posB, yawB] = TRACK[index];
      const k = t1 > t0 ? easeInOut(clamp((t - t0) / (t1 - t0))) : 1;
      const walking = poseA === "walk" || poseB === "walk";
      blendPose(poseA === "walk" ? "standing" : poseA, poseB === "walk" ? "standing" : poseB, k);
      root.position.set(lerp(posA[0], posB[0], k), lerp(posA[1], posB[1], k), lerp(posA[2], posB[2], k));
      root.rotation.y = lerp(yawA, yawB, k);
      if (walking) {
        const phase = t * Math.PI * 2 / (BEAT * 2);
        const stride = 0.45;
        j.thighL.rotation.x = -Math.sin(phase) * stride; j.thighR.rotation.x = Math.sin(phase) * stride;
        j.shinL.rotation.x = Math.max(0, Math.sin(phase + 1.2)) * 0.8; j.shinR.rotation.x = Math.max(0, -Math.sin(phase + 1.2)) * 0.8;
        j.upperArmL.rotation.x = Math.sin(phase) * 0.3; j.upperArmR.rotation.x = -Math.sin(phase) * 0.3;
        root.position.y += Math.abs(Math.cos(phase)) * 0.025;
      }
      // Typing on the grid while the agent works; stillness when it stops.
      const typingWeight = (poseA === "typing" && poseB === "typing") ? 1 : 0;
      if (typingWeight && t < BEATS.firstStop) {
        const tap = Math.sin(t * Math.PI * 2 / (BEAT / 2));
        j.forearmL.rotation.x += tap * 0.05; j.forearmR.rotation.x -= tap * 0.05;
        j.head.rotation.x += Math.sin(t * 1.3) * 0.02;
      }
      // Breathing; deeper and slower once asleep.
      const asleep = ["asleepDesk", "lying"].includes(poseB) && ["asleepDesk", "lying"].includes(poseA);
      const breath = Math.sin(t * Math.PI * 2 / (asleep ? 4.2 : 3.2));
      j.chest.scale.set(1 + breath * 0.012, 1 + breath * 0.008, 1 + breath * 0.02);
    },
  };
}

// The office chair: rolls back and turns, empty, after they stand.
export function chairState(t) {
  const push = smooth(BEATS.standUp, BEATS.standUp + 1.0, t);
  const spin = t > BEATS.standUp ? (1 - Math.exp(-(t - BEATS.standUp) * 0.9)) * Math.PI * 1.35 : 0;
  return { z: push * 0.45, x: push * 0.12, yaw: spin };
}

// Where the phone is: on the desk, then in their hand.
export function phoneCarried(t) {
  return t > BEATS.standUp + 0.6;
}
