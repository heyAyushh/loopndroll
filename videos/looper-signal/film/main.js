// "Signal": the agents' work as light. Random points, one intention.
import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { BokehPass } from "three/addons/postprocessing/BokehPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { RoomEnvironment } from "three/addons/environments/RoomEnvironment.js";
import { BAR, BEATS, DURATION, FPS, at, smooth } from "./story.js";
import { createField } from "./field.js";
import { createOrb } from "./orb.js";
import { drawType } from "./type.js";
import { frameCamera } from "./camera.js";

const WIDTH = 1920, HEIGHT = 1080;
let renderer, composer, bloom, bokeh, scene, camera, field, orb, envelope, night, nightContext, nightTexture;
const frame = document.createElement("canvas");
frame.width = WIDTH; frame.height = HEIGHT;
const g = frame.getContext("2d");

function setup(energy) {
  envelope = energy;
  renderer = new THREE.WebGLRenderer({ antialias: true, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1);
  renderer.setSize(WIDTH, HEIGHT);
  renderer.toneMapping = THREE.AgXToneMapping;
  renderer.toneMappingExposure = 1.1;
  scene = new THREE.Scene();
  scene.environment = new THREE.PMREMGenerator(renderer).fromScene(new RoomEnvironment(), 0.02).texture;
  // The background: dead black while they are the loop; a lavender night once Looper is.
  night = document.createElement("canvas");
  night.width = 320; night.height = 180;
  nightContext = night.getContext("2d");
  nightTexture = new THREE.CanvasTexture(night);
  nightTexture.colorSpace = THREE.SRGBColorSpace;
  scene.background = nightTexture;
  camera = new THREE.PerspectiveCamera(34, WIDTH / HEIGHT, 0.05, 200);
  field = createField();
  orb = createOrb();
  scene.add(field.group, orb.group);
  composer = new EffectComposer(renderer);
  composer.addPass(new RenderPass(scene, camera));
  bokeh = new BokehPass(scene, camera, { focus: 5, aperture: 0.002, maxblur: 0.012 });
  composer.addPass(bokeh);
  bloom = new UnrealBloomPass(new THREE.Vector2(WIDTH, HEIGHT), 0.8, 0.32, 0.72);
  composer.addPass(bloom);
  composer.addPass(new OutputPass());
  return Math.round(DURATION * FPS);
}

function paintNight(t, sub) {
  const warmth = smooth(BEATS.drop, BEATS.drop + BAR * 2, t) * (1 - smooth(at(31), at(32), t) * 0.6);
  const glow = nightContext.createRadialGradient(160, 95, 0, 160, 95, 200);
  glow.addColorStop(0, `rgb(${Math.round(34 * warmth + 3)}, ${Math.round(22 * warmth + 3)}, ${Math.round(64 * warmth + 5)})`);
  glow.addColorStop(0.6, `rgb(${Math.round(12 * warmth + 3)}, ${Math.round(8 * warmth + 3)}, ${Math.round(26 * warmth + 5)})`);
  glow.addColorStop(1, "rgb(3,3,5)");
  nightContext.fillStyle = glow; nightContext.fillRect(0, 0, 320, 180);
  if (warmth > 0 && sub > 0) { nightContext.fillStyle = `rgba(120, 90, 200, ${0.05 * sub * warmth})`; nightContext.fillRect(0, 0, 320, 180); }
  nightTexture.needsUpdate = true;
}

function paint(index) {
  const t = index / FPS;
  const sub = envelope.sub[index] ?? 0;
  field.update(t, sub);
  orb.update(t, scene);
  paintNight(t, sub);
  const lens = frameCamera(camera, t);
  bokeh.enabled = lens.aperture > 0;
  bokeh.uniforms.focus.value = lens.focus;
  bokeh.uniforms.aperture.value = lens.aperture;
  bloom.strength = t >= BEATS.drop && t < BEATS.end ? 0.8 + sub * 0.35 : 0.75;
  composer.render();
  g.globalAlpha = 1; g.filter = "none";
  g.drawImage(renderer.domElement, 0, 0);
  drawType(g, t, WIDTH, HEIGHT);
  const vignette = g.createRadialGradient(WIDTH / 2, HEIGHT / 2, HEIGHT * 0.35, WIDTH / 2, HEIGHT / 2, HEIGHT * 1.0);
  vignette.addColorStop(0, "rgba(0,0,0,0)"); vignette.addColorStop(1, "rgba(0,0,0,0.5)");
  g.fillStyle = vignette; g.fillRect(0, 0, WIDTH, HEIGHT);
  const fade = smooth(at(31.3), at(32), t);
  if (fade > 0) { g.fillStyle = `rgba(0,0,0,${fade})`; g.fillRect(0, 0, WIDTH, HEIGHT); }
}

window.setup = setup;
window.renderFrame = (index, quality = 0.95) => { paint(index); return frame.toDataURL("image/jpeg", quality).split(",")[1]; };
window.filmReady = true;
