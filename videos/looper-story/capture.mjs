// Deterministic capture: seek window.render(t) per frame and screenshot; never uses wall-clock time.
// Usage: node capture.mjs stills t1 t2 ...   |   node capture.mjs frames [fromFrame] [toFrame]
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join } from 'node:path';

const FPS = Number(process.env.FPS || 60);
const FILM_SECONDS = 50;
const PORT = Number(process.env.PORT || 8791);
const ROOT = new URL('.', import.meta.url).pathname;
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const TYPES = { '.html': 'text/html', '.png': 'image/png', '.mjs': 'text/javascript' };

const server = createServer(async (req, res) => {
  const path = decodeURIComponent(req.url.split('?')[0]);
  try {
    const body = await readFile(join(ROOT, path));
    res.writeHead(200, { 'content-type': TYPES[extname(path)] || 'application/octet-stream' });
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
}).listen(PORT);

const [mode, ...args] = process.argv.slice(2);
const browser = await chromium.launch({ executablePath: CHROME });
const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
page.on('pageerror', e => console.error('PAGE ERROR', e.message));
await page.goto(`http://localhost:${PORT}/film.html`);
await page.evaluate(() => window.ready);

async function shot(t, path, type) {
  await page.evaluate(t => window.render(t), t);
  await page.screenshot({ path, type, quality: type === 'jpeg' ? 92 : undefined });
}
if (mode === 'stills') {
  mkdirSync('build/stills', { recursive: true });
  for (const t of args) await shot(Number(t), `build/stills/t${t}.png`, 'png');
} else {
  const framesDir = process.env.FRAMES_DIR || 'build/frames';
  mkdirSync(framesDir, { recursive: true });
  const total = Math.round(FPS * FILM_SECONDS);
  const from = Number(args[0] ?? 0), to = Number(args[1] ?? total);
  for (let f = from; f < to; f++) await shot(f / FPS, `${framesDir}/f${String(f).padStart(5, '0')}.jpg`, 'jpeg');
}
await browser.close();
server.close();
