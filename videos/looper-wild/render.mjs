// Renders the p5 sketch frame by frame in headless Chrome and pipes it into ffmpeg (1080x1920 Reel).
//   node render.mjs                 -> renders/looper-wild.mp4
//   node render.mjs --stills 1,4.2  -> renders/stills/*.jpg
import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { serve } from "./serve.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const CHROME = process.env.CHROME_PATH ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const FPS = 30;
const argument = (name) => { const i = process.argv.indexOf(name); return i > -1 ? process.argv[i + 1] : null; };
const stills = argument("--stills")?.split(",").map(Number);

const server = await serve(HERE);
const browser = await chromium.launch({ executablePath: CHROME, headless: true, args: ["--use-angle=metal", "--enable-gpu", "--ignore-gpu-blocklist"] });
const FORMAT = argument("--format") ?? "reel"; // reel = 1080x1920, x = 1920x1080
const SIZE = FORMAT === "x" ? { width: 1920, height: 1080 } : { width: 1080, height: 1920 };
const page = await browser.newPage({ viewport: SIZE });
await page.addInitScript((format) => { window.FILM_FORMAT = format; }, FORMAT);
page.on("pageerror", (error) => console.error("page error:", error.message));
page.on("console", (message) => { if (message.type() === "error" && !message.text().includes("404")) console.error("console:", message.text()); });
await page.goto(`${server.url}film/index.html`);
await page.waitForFunction(() => window.filmReady === true, null, { timeout: 60000 });
const envelope = JSON.parse(readFileSync(join(HERE, "assets", "envelope.json"), "utf8"));
const frameCount = await page.evaluate((env) => window.setup(env), envelope);

if (stills) {
  mkdirSync(join(HERE, "renders", "stills"), { recursive: true });
  for (const seconds of stills) {
    const jpeg = await page.evaluate((frame) => window.renderFrame(frame, 0.9), Math.round(seconds * FPS));
    writeFileSync(join(HERE, "renders", "stills", `t${seconds.toFixed(2).padStart(6, "0")}.jpg`), Buffer.from(jpeg, "base64"));
  }
  console.log(`wrote ${stills.length} stills`);
} else {
  const output = join(HERE, "renders", FORMAT === "x" ? "looper-wild-x.mp4" : "looper-wild.mp4");
  mkdirSync(dirname(output), { recursive: true });
  const ffmpeg = spawn("ffmpeg", ["-y", "-v", "error", "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "mjpeg", "-i", "-",
    "-i", join(HERE, "assets", "score.wav"), "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-maxrate", "14M", "-bufsize", "28M",
    "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-c:a", "aac", "-b:a", "256k", "-shortest", output], { stdio: ["pipe", "inherit", "inherit"] });
  for (let frame = 0; frame < frameCount; frame += 1) {
    const jpeg = await page.evaluate((index) => window.renderFrame(index, 0.94), frame);
    if (!ffmpeg.stdin.write(Buffer.from(jpeg, "base64"))) await new Promise((resolve) => ffmpeg.stdin.once("drain", resolve));
  }
  ffmpeg.stdin.end();
  await new Promise((resolve, reject) => ffmpeg.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited ${code}`)))));
  console.log(`wrote ${output}`);
}
await browser.close();
server.close();
