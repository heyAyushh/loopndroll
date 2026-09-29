// Renders film.html frame by frame in headless Chrome and pipes the frames to ffmpeg.
//   node render.mjs --format x|reel|square|feed           -> renders/looper-v1-<format>.mp4
//   node render.mjs --format reel --stills 2.5,12,44      -> renders/stills/<format>-*.jpg (review frames)
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { chromium } from "playwright-core";

const HERE = dirname(fileURLToPath(import.meta.url));
const CHROME = process.env.CHROME_PATH ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const FPS = 30;
// x 1920x1080 (X/Twitter, YouTube) · reel 1080x1920 (Reels, TikTok, Shorts) · square 1080x1080 · feed 1080x1350 (IG feed)
const FORMATS = { x: [1920, 1080], reel: [1080, 1920], square: [1080, 1080], feed: [1080, 1350] };
const formatIndex = process.argv.indexOf("--format");
const FORMAT = formatIndex > -1 ? process.argv[formatIndex + 1] : "x";
if (!FORMATS[FORMAT]) throw new Error(`unknown format ${FORMAT}; use ${Object.keys(FORMATS).join(", ")}`);
const [WIDTH, HEIGHT] = FORMATS[FORMAT];
const OUTPUT = join(HERE, "renders", `looper-v1-${FORMAT}.mp4`);

const stillsArgument = process.argv.indexOf("--stills");
const stills = stillsArgument > -1 ? process.argv[stillsArgument + 1].split(",").map(Number) : null;

const browser = await chromium.launch({ executablePath: CHROME, headless: true });
const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT } });
await page.goto(`${pathToFileURL(join(HERE, "film.html")).href}?w=${WIDTH}&h=${HEIGHT}`);
const envelope = JSON.parse(readFileSync(join(HERE, "assets", "envelope.json"), "utf8"));
const orb = `data:image/png;base64,${readFileSync(join(HERE, "assets", "orb.png")).toString("base64")}`;
const frameCount = await page.evaluate(([env, image]) => window.setup(env, image), [envelope, orb]);

if (stills) {
  mkdirSync(join(HERE, "renders", "stills"), { recursive: true });
  for (const seconds of stills) {
    const jpeg = await page.evaluate((frame) => window.renderFrame(frame, 0.9), Math.round(seconds * FPS));
    writeFileSync(join(HERE, "renders", "stills", `${FORMAT}-t${String(seconds).padStart(5, "0")}.jpg`), Buffer.from(jpeg, "base64"));
  }
  console.log(`wrote ${stills.length} stills`);
} else {
  mkdirSync(dirname(OUTPUT), { recursive: true });
  const ffmpeg = spawn("ffmpeg", [
    "-y", "-v", "error",
    "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "mjpeg", "-i", "-",
    "-i", join(HERE, "assets", "score.wav"),
    "-c:v", "libx264", "-preset", "slow", "-crf", "17", "-pix_fmt", "yuv420p", "-movflags", "+faststart",
    "-c:a", "aac", "-b:a", "256k", "-shortest", OUTPUT,
  ], { stdio: ["pipe", "inherit", "inherit"] });
  const started = Date.now();
  for (let frame = 0; frame < frameCount; frame += 1) {
    const jpeg = await page.evaluate((index) => window.renderFrame(index, 0.95), frame);
    if (!ffmpeg.stdin.write(Buffer.from(jpeg, "base64"))) await new Promise((resolve) => ffmpeg.stdin.once("drain", resolve));
    if (frame % 150 === 0) console.log(`frame ${frame}/${frameCount} (${((Date.now() - started) / 1000).toFixed(0)}s)`);
  }
  ffmpeg.stdin.end();
  await new Promise((resolve, reject) => ffmpeg.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`)))));
  console.log(`wrote ${OUTPUT}`);
}
await browser.close();
