// Entry: the Reel. Drawings that disintegrate into their own light and reassemble as the next moment.
import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { DURATION, FPS, at, smooth } from "./story.js";
import { CAMERA_Z, HEIGHT, WIDTH, createReel } from "./reel.js";
import { drawScreen, drawType } from "./type.js";

let renderer, composer, bloom, camera, reel, envelope, screenQuads;
const frame = document.createElement("canvas");
frame.width = WIDTH; frame.height = HEIGHT;
const g = frame.getContext("2d");

async function loadImage(url) { const image = new Image(); image.src = url; await image.decode(); return image; }

async function setup(energy, assets) {
  envelope = energy;
  screenQuads = assets.screens;
  const plates = {};
  await Promise.all(Object.entries(assets.plates).map(async ([name, url]) => { plates[name] = await loadImage(url); }));
  const points = {};
  let count = Infinity;
  await Promise.all(Object.entries(assets.points).map(async ([name, url]) => {
    points[name] = new Float32Array(await (await fetch(url)).arrayBuffer());
    count = Math.min(count, points[name].length / 5);
  }));
  await document.fonts.ready;
  renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1);
  renderer.setSize(WIDTH, HEIGHT);
  renderer.toneMapping = THREE.NeutralToneMapping;
  renderer.toneMappingExposure = 1.05;
  reel = createReel({ plates, crops: assets.crops, points, count });
  reel.scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.02).texture;
  camera = new THREE.PerspectiveCamera(30, WIDTH / HEIGHT, 0.1, 200);
  composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(reel.scene, camera));
  bloom = new UnrealBloomPass(new THREE.Vector2(WIDTH, HEIGHT), 0.7, 0.35, 0.86);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());
  return Math.round(DURATION * FPS);
}

// Put a screen's content on the drawing's own screen: a quad following the drawing's zoom.
function placeScreen(result, t) {
  const { name, options, state } = result;
  const kind = name === "monitor" ? (options.resumed ? "resumed" : "monitor") : name === "couch" ? "phone" : null;
  const { mesh, canvas, context, texture } = reel.screen;
  mesh.visible = !!kind && result.morph > 0.7;
  if (!mesh.visible) return;
  drawScreen(context, canvas.width, canvas.height, t, kind);
  texture.needsUpdate = true;
  const quad = screenQuads[name];
  const corners = quad.map(([u, v]) => reel.toWorld(u, v, state, new THREE.Vector3()));
  const [tl, tr, br, bl] = corners;
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([tl, tr, br, tl, br, bl].flatMap((p) => [p.x, p.y, 0.02]), 3));
  geometry.setAttribute("uv", new THREE.Float32BufferAttribute([0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0], 2));
  mesh.geometry.dispose();
  mesh.geometry = geometry;
  mesh.position.set(0, 0, 0);
  mesh.material.opacity = smooth(0.7, 1, result.morph);
}

function paint(index) {
  const t = index / FPS;
  const sub = envelope.sub[index] ?? 0;
  const result = reel.update(t, sub);
  placeScreen(result, t);
  // The camera breathes with the kick after the drop; before, it only drifts.
  const after = t >= at(10);
  camera.position.set(Math.sin(t * 0.3) * 0.25, Math.cos(t * 0.23) * 0.25, CAMERA_Z - (after ? sub * 0.35 : 0));
  camera.lookAt(0, 0, 0);
  bloom.strength = after ? 0.75 + sub * 0.4 : 0.6;
  composer.render();
  g.globalAlpha = 1; g.filter = "none";
  g.drawImage(renderer.domElement, 0, 0);
  drawType(g, t, WIDTH, HEIGHT);
  const fade = smooth(DURATION - 0.8, DURATION, t);
  if (fade > 0) { g.fillStyle = `rgba(0,0,0,${fade})`; g.fillRect(0, 0, WIDTH, HEIGHT); }
}

window.setup = setup;
window.renderFrame = (index, quality = 0.95) => { paint(index); return frame.toDataURL("image/jpeg", quality).split(",")[1]; };
window.filmReady = true;
