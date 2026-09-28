// Entry: a p5 sketch driven frame by frame (not by the clock), so every render is identical.
import { DURATION, FPS, H, STUTTERS, W, at } from "./story.js";
import { createScene } from "./scene.js";

let sketch, scene, envelope;
const FREEZES = [[at(2), at(2.25)], ...STUTTERS.map((b) => [at(b + 0.25), at(b + 0.32)])];

new window.p5((p) => {
  p.setup = () => {
    p.createCanvas(W, H);
    p.pixelDensity(1);
    p.noiseSeed(47);
    p.noLoop();
    sketch = p;
  };
}, document.body);

// A Stop tears the frame: slices of the image jump sideways for a few frames.
function glitch(t) {
  const hit = FREEZES.find(([a]) => t >= a && t < a + 0.2);
  if (!hit) return;
  const strength = 1 - (t - hit[0]) / 0.2;
  const g = sketch.drawingContext;
  const canvas = g.canvas;
  for (let k = 0; k < 14; k += 1) {
    const y = ((k * 137 + Math.floor(t * 60) * 53) % H);
    const h = 20 + ((k * 71) % 90);
    const dx = (((k * 97 + Math.floor(t * 60) * 31) % 200) - 100) * strength;
    g.drawImage(canvas, 0, y, W, h, dx, y, W, h);
  }
}

window.setup = async (energy) => {
  envelope = energy;
  while (!sketch) await new Promise((resolve) => setTimeout(resolve, 20));
  await document.fonts.ready;
  scene = createScene(sketch);
  return Math.round(DURATION * FPS);
};
window.renderFrame = (index, quality = 0.94) => {
  const t = index / FPS;
  scene.draw(t, envelope.sub[index] ?? 0);
  glitch(t);
  return sketch.drawingContext.canvas.toDataURL("image/jpeg", quality).split(",")[1];
};
window.filmReady = true;
