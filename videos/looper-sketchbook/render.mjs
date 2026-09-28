// Renders the sketchbook film frame by frame in headless Chrome (GPU) and pipes it into ffmpeg.
//   node render.mjs                     -> renders/looper-sketchbook.mp4
//   node render.mjs --midpoints         -> one still per shot in renders/stills/
//   node render.mjs --stills 2.5,12     -> stills at those seconds
//   node render.mjs --from 58 --to 66   -> renders/segment.mp4
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { serve } from "./serve.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CHROME = process.env.CHROME_PATH ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const FPS = 30;
const argument = (name) => { const i = process.argv.indexOf(name); return i > -1 ? process.argv[i + 1] : null; };
let stills = argument("--stills")?.split(",").map(Number);
const midpoints = process.argv.includes("--midpoints");
const from = Number(argument("--from") ?? 0);
const to = argument("--to") ? Number(argument("--to")) : null;

const readJson = (path) => (existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : {});
// PNG masters when present locally; otherwise the committed JPEG copies.
const plateFiles = readdirSync(join(HERE, "plates")).filter((f) => /\.(png|jpg)$/.test(f)).sort((a, b) => (a.endsWith(".png") ? 1 : -1) - (b.endsWith(".png") ? 1 : -1));
const plates = Object.fromEntries(plateFiles.map((f) => [f.slice(0, -4), `/plates/${f}`]));
const assets = { plates, orb: "/assets/orb.png", quads: readJson(join(HERE, "plates", "quads.json")), lines: readJson(join(HERE, "plates", "lines.json")) };

const server = await serve(HERE);
const browser = await chromium.launch({ executablePath: CHROME, headless: true, args: ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"] });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 } });
page.on("pageerror", (error) => console.error("page error:", error.message));
page.on("console", (message) => { if (message.type() === "error" && !message.text().includes("404")) console.error("console:", message.text()); });
await page.goto(`${server.url}film/index.html`);
await page.waitForFunction(() => window.filmReady === true, null, { timeout: 60000 });
const envelope = JSON.parse(readFileSync(join(HERE, "assets", "envelope.json"), "utf8"));
const frameCount = await page.evaluate(([env, a]) => window.setup(env, a), [envelope, assets]);
if (midpoints) stills = [...new Set([...(stills ?? []), ...(await page.evaluate(() => window.shotMidpoints()))])].sort((x, y) => x - y);

if (stills) {
  mkdirSync(join(HERE, "renders", "stills"), { recursive: true });
  for (const seconds of stills) {
    const jpeg = await page.evaluate((frame) => window.renderFrame(frame, 0.9), Math.round(seconds * FPS));
    writeFileSync(join(HERE, "renders", "stills", `t${seconds.toFixed(2).padStart(6, "0")}.jpg`), Buffer.from(jpeg, "base64"));
  }
  console.log(`wrote ${stills.length} stills`);
} else {
  const segment = to !== null;
  const first = Math.round(from * FPS);
  const last = segment ? Math.min(frameCount, Math.round(to * FPS)) : frameCount;
  const output = join(HERE, "renders", segment ? "segment.mp4" : "looper-sketchbook.mp4");
  mkdirSync(dirname(output), { recursive: true });
  const ffmpeg = spawn("ffmpeg", [
    "-y", "-v", "error",
    "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "mjpeg", "-i", "-",
    "-ss", String(from), "-i", join(HERE, "assets", "score.wav"),
    "-c:v", "libx264", "-preset", "medium", "-crf", "19", "-maxrate", "12M", "-bufsize", "24M", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    "-c:a", "aac", "-b:a", "256k", "-shortest", output,
  ], { stdio: ["pipe", "inherit", "inherit"] });
  const started = Date.now();
  for (let frame = first; frame < last; frame += 1) {
    const jpeg = await page.evaluate((index) => window.renderFrame(index, 0.95), frame);
    if (!ffmpeg.stdin.write(Buffer.from(jpeg, "base64"))) await new Promise((resolve) => ffmpeg.stdin.once("drain", resolve));
    if ((frame - first) % 300 === 0) console.log(`frame ${frame}/${last} (${((Date.now() - started) / 1000).toFixed(0)}s)`);
  }
  ffmpeg.stdin.end();
  await new Promise((resolve, reject) => ffmpeg.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`)))));
  console.log(`wrote ${output}`);
}
await browser.close();
server.close();
