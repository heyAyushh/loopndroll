// Entry: builds the three worlds and renders any frame on demand (window.renderFrame).
import * as THREE from "three";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { DURATION, FPS } from "./story.js";
import { createRoom } from "./room.js";
import { createMachine } from "./machine.js";
import { createTitles } from "./endcard.js";
import { createShots } from "./shots.js";
import { createPost, HEIGHT, WIDTH } from "./post.js";

let renderer, post, worlds, shots, envelope;
const ctx = { t: 0 };
const cameras = {};

async function setup(energy, orbDataUrl) {
  envelope = energy;
  const orbImage = new Image();
  orbImage.src = orbDataUrl;
  await orbImage.decode();
  await document.fonts.ready;

  renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true, powerPreference: "high-performance" });
  renderer.setPixelRatio(1);
  renderer.setSize(WIDTH, HEIGHT);
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  document.body.appendChild(renderer.domElement);

  const environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.04).texture;
  const room = createRoom(orbImage);
  room.scene.environment = environment;
  room.scene.environmentIntensity = 0.12;
  const machine = createMachine();
  machine.scene.environment = environment;
  machine.scene.environmentIntensity = 0.3;
  const titles = createTitles(orbImage);
  worlds = { room, machine, titles };
  ctx.room = room;
  ctx.machine = machine;
  for (const name of ["room", "machine"]) cameras[name] = new THREE.PerspectiveCamera(40, WIDTH / HEIGHT, 0.004, 400);
  shots = createShots(ctx);
  post = createPost(renderer);
  return Math.round(DURATION * FPS);
}

function paint(frame) {
  const t = frame / FPS;
  const sub = envelope.sub[frame] ?? 0;
  ctx.t = t;
  const shot = shots.find(t);
  const k = (t - shot.start) / (shot.end - shot.start);
  const world = worlds[shot.look.world];
  world.update(t, sub);
  let camera;
  let focus = 3;
  if (shot.look.world === "titles") {
    camera = world.camera;
  } else {
    camera = cameras[shot.look.world];
    const state = shot.camera(k);
    camera.position.copy(state.position);
    camera.up.copy(state.up ?? new THREE.Vector3(0, 1, 0));
    camera.lookAt(state.target);
    camera.fov = state.fov;
    camera.updateProjectionMatrix();
    focus = shot.look.focusFrom !== undefined
      ? THREE.MathUtils.lerp(shot.look.focusFrom, shot.look.focusTo, THREE.MathUtils.smootherstep(k, 0.3, 0.8))
      : state.focus ?? state.position.distanceTo(state.target);
  }
  post.render(world.scene, camera, { ...shot.look, focus, time: t, ...shots.transitions(t) });
}

window.setup = setup;
window.renderFrame = (frame, quality = 0.94) => {
  paint(frame);
  return renderer.domElement.toDataURL("image/jpeg", quality).split(",")[1];
};
window.shotMidpoints = () => shots.shots.map((shot) => Number(((shot.start + shot.end) / 2).toFixed(2)));
window.probe = (t) => {
  worlds.room.update(t, 0);
  const out = {};
  for (const name of ["handL", "handR", "head", "pelvis"]) { const p = worlds.room.developer.joints[name].getWorldPosition(new THREE.Vector3()); out[name] = [p.x, p.y, p.z].map((n) => +n.toFixed(3)); }
  return out;
};
window.filmReady = true;
