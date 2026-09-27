// Renders the Looper arcade launch video without any video framework:
// scenes paint low-res frames, ffmpeg upscales them with hard pixels and
// lays a CRT scanline/vignette pass on top.
//
//   node render.mjs --format landscape|portrait            -> build/<format>-video.mp4 (silent)
//   node render.mjs --format portrait --stills 8,12.5,30   -> build/stills/*.png
//   node render.mjs --events                               -> build/events.json (pinball sim log for synth.py)
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { createCanvas } from "@napi-rs/canvas";
import { T, Painter, loadAssets, pixelWipe } from "./src/core.js";
import { createBoot } from "./src/boot.js";
import { createPinball, simulatePinball } from "./src/pinball.js";
import { createPalace } from "./src/palace.js";
import { createRace } from "./src/race.js";
import { createFinale } from "./src/finale.js";

const FORMATS = {
  landscape: { W: 640, H: 360, scale: 3 },
  portrait: { W: 360, H: 640, scale: 3 },
};
const SCANLINE_ALPHA = 0.22;
const VIGNETTE_ALPHA = 0.55;

const args = process.argv.slice(2);
const arg = (name) => {
  const i = args.indexOf(`--${name}`);
  return i < 0 ? null : args[i + 1] ?? true;
};
mkdirSync("build/stills", { recursive: true });

if (arg("events")) {
  writeFileSync("build/events.json", JSON.stringify(simulatePinball().events));
  console.log("wrote build/events.json");
  process.exit(0);
}

const format = arg("format") || "landscape";
const F = FORMATS[format];
const outW = F.W * F.scale;
const outH = F.H * F.scale;
await loadAssets();

const scenes = [
  { range: T.scenes.boot, scene: createBoot(format) },
  { range: T.scenes.pinball, scene: createPinball(format) },
  { range: T.scenes.palace, scene: createPalace(format) },
  { range: T.scenes.race, scene: createRace(format) },
  { range: T.scenes.finale, scene: createFinale(format) },
];
const WIPES = [
  [T.boot.zoom, T.scenes.pinball[0]],
  [T.pinball.wipe + 0.2, T.scenes.palace[0]],
  [T.palace.wipe + 0.2, T.scenes.race[0]],
  [T.race.wipe + 0.2, T.scenes.finale[0]],
];

const painter = new Painter(F.W, F.H);
function drawFrame(t) {
  const entry = scenes.find(({ range }) => t >= range[0] && t < range[1]) || scenes[scenes.length - 1];
  painter.ctx.globalAlpha = 1;
  entry.scene.draw(painter, t);
  for (const [cover, reveal] of WIPES) pixelWipe(painter, t, cover, reveal);
}

function crtOverlay() {
  const canvas = createCanvas(outW, outH);
  const c = canvas.getContext("2d");
  c.fillStyle = `rgba(0,0,0,${SCANLINE_ALPHA})`;
  for (let y = F.scale - 1; y < outH; y += F.scale) c.fillRect(0, y, outW, 1);
  const g = c.createRadialGradient(outW / 2, outH / 2, Math.min(outW, outH) * 0.35, outW / 2, outH / 2, Math.hypot(outW, outH) / 2);
  g.addColorStop(0, "rgba(0,0,0,0)");
  g.addColorStop(1, `rgba(0,0,0,${VIGNETTE_ALPHA})`);
  c.fillStyle = g;
  c.fillRect(0, 0, outW, outH);
  return canvas;
}

const stills = arg("stills");
if (stills) {
  const overlay = crtOverlay();
  const out = createCanvas(outW, outH);
  const oc = out.getContext("2d");
  oc.imageSmoothingEnabled = false;
  for (const value of String(stills).split(",")) {
    const t = Number(value);
    drawFrame(t);
    oc.drawImage(painter.canvas, 0, 0, outW, outH);
    oc.drawImage(overlay, 0, 0);
    const file = `build/stills/${format}-${t.toFixed(2)}.png`;
    writeFileSync(file, out.toBuffer("image/png"));
    console.log(file);
  }
  process.exit(0);
}

const overlayFile = `build/crt-${format}.png`;
writeFileSync(overlayFile, crtOverlay().toBuffer("image/png"));
const output = `build/${format}-video.mp4`;
const ffmpeg = spawn(
  "ffmpeg",
  [
    "-y", "-v", "error",
    "-f", "rawvideo", "-pix_fmt", "rgba", "-s", `${F.W}x${F.H}`, "-r", String(T.fps), "-i", "-",
    "-i", overlayFile,
    "-filter_complex", `[0:v]scale=${outW}:${outH}:flags=neighbor[v];[v][1:v]overlay=0:0,format=yuv420p`,
    "-c:v", "libx264", "-preset", "slow", "-crf", "14", "-tune", "animation", "-movflags", "+faststart",
    output,
  ],
  { stdio: ["pipe", "inherit", "inherit"] },
);
const totalFrames = Math.round(T.total * T.fps);
const started = Date.now();
for (let i = 0; i < totalFrames; i++) {
  drawFrame(i / T.fps);
  const pixels = painter.ctx.getImageData(0, 0, F.W, F.H).data;
  if (!ffmpeg.stdin.write(Buffer.from(pixels.buffer, pixels.byteOffset, pixels.byteLength))) {
    await new Promise((resolve) => ffmpeg.stdin.once("drain", resolve));
  }
  if (i % (T.fps * 5) === 0) console.log(`${format}: ${(i / T.fps).toFixed(0)}s / ${T.total}s`);
}
ffmpeg.stdin.end();
await new Promise((resolve, reject) => ffmpeg.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`)))));
console.log(`wrote ${output} in ${((Date.now() - started) / 1000).toFixed(1)}s`);
