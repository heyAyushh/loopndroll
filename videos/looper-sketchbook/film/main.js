// Entry: loads the drawings and composes each frame: page (2D) + ink (2D) -> pencil shader (WebGL).
import * as THREE from "three";
import { DURATION, FPS } from "./story.js";
import { makeCanvas } from "./page.js";
import { createPencil } from "./pencil.js";
import { createShots } from "./shots.js";

THREE.ColorManagement.enabled = false; // the pages are already display-referred drawings

let renderer, pencil, shots, envelope;
const page = makeCanvas();
const ink = makeCanvas();
const pageContext = page.getContext("2d");
const inkContext = ink.getContext("2d");

async function loadImage(url) {
  const image = new Image();
  image.src = url;
  await image.decode();
  return image;
}

async function setup(energy, assets) {
  envelope = energy;
  const plates = {};
  await Promise.all(Object.entries(assets.plates).map(async ([name, url]) => { plates[name] = await loadImage(url); }));
  const orb = await loadImage(assets.orb);
  await document.fonts.ready;
  renderer = new THREE.WebGLRenderer({ antialias: false, preserveDrawingBuffer: true });
  renderer.setPixelRatio(1);
  renderer.setSize(1920, 1080);
  document.body.appendChild(renderer.domElement);
  pencil = createPencil(renderer, page, ink);
  shots = createShots({ plates, orb, quads: assets.quads, lines: assets.lines });
  return Math.round(DURATION * FPS);
}

function paint(frame) {
  const t = frame / FPS;
  const sub = envelope.sub[frame] ?? 0;
  pageContext.setTransform(1, 0, 0, 1, 0, 0);
  inkContext.setTransform(1, 0, 0, 1, 0, 0);
  inkContext.clearRect(0, 0, ink.width, ink.height);
  const look = shots.draw(pageContext, inkContext, t, sub);
  pencil.render({ time: t, reveal: 1, boil: 1, fatigue: 0, riso: 0, flash: 0, fade: 0, mode: 0, inkGlow: 0, shake: 0, paper: 0xece7dc, flashColor: 0xffffff, ...look });
}

window.setup = setup;
window.renderFrame = (frame, quality = 0.94) => {
  paint(frame);
  return renderer.domElement.toDataURL("image/jpeg", quality).split(",")[1];
};
window.shotMidpoints = () => shots.list.map((shot) => Number(((shot.start + shot.end) / 2).toFixed(2)));
window.filmReady = true;
