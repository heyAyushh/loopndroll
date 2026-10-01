// Frame-accurate renderer: seeks the film page to every frame time and pipes PNG frames into ffmpeg.
// Usage:
//   node render.js video <w> <h> <fps> <out.mp4> [start] [end]
//   node render.js stills <w> <h> <outDir> <t1,t2,...>
const { chromium } = require('playwright-core');
const { spawn } = require('child_process');
const path = require('path');
const fs = require('fs');

const PAGE = 'file://' + path.join(__dirname, 'index.html');

async function openFilm(width, height) {
  const browser = await chromium.launch({ channel: 'chrome', args: ['--allow-file-access-from-files', '--force-color-profile=srgb'] });
  const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: 1 });
  page.on('pageerror', e => console.error('PAGE ERROR', e.message));
  page.on('console', m => { if (m.type() === 'error') console.error('CONSOLE', m.text()); });
  await page.goto(`${PAGE}?w=${width}&h=${height}`);
  await page.evaluate(() => window.filmReady);
  return { browser, page };
}

/** A busy machine can stall one screenshot; wait longer and retry instead of losing the whole render. */
async function screenshotWithRetry(page, attempts = 3) {
  for (let i = 1; ; i++) {
    try { return await page.screenshot({ type: 'png', timeout: 180000 }); }
    catch (error) { if (i >= attempts) throw error; console.error(`screenshot retry ${i}: ${error.message.split('\n')[0]}`); }
  }
}

async function renderVideo(width, height, fps, out, start, end) {
  const { browser, page } = await openFilm(width, height);
  const duration = await page.evaluate(() => window.FILM.DURATION);
  const first = Math.round((start ?? 0) * fps);
  const last = Math.round((end ?? duration) * fps);
  const ffmpeg = spawn('ffmpeg', ['-loglevel', 'error', '-y', '-f', 'image2pipe', '-framerate', String(fps), '-c:v', 'png', '-i', '-',
    '-c:v', 'libx264', '-preset', 'medium', '-crf', '16', '-pix_fmt', 'yuv420p', '-movflags', '+faststart', out], { stdio: ['pipe', 'inherit', 'inherit'] });
  const began = Date.now();
  for (let frame = first; frame < last; frame++) {
    await page.evaluate(t => window.seek(t), frame / fps);
    const png = await screenshotWithRetry(page);
    if (!ffmpeg.stdin.write(png)) await new Promise(r => ffmpeg.stdin.once('drain', r));
    if (frame % (fps * 5) === 0) console.log(`${out}: ${(frame / fps).toFixed(1)}s  (${((Date.now() - began) / 1000).toFixed(0)}s elapsed)`);
  }
  ffmpeg.stdin.end();
  await new Promise(r => ffmpeg.on('close', r));
  await browser.close();
}

async function renderStills(width, height, outDir, times) {
  fs.mkdirSync(outDir, { recursive: true });
  const { browser, page } = await openFilm(width, height);
  for (const t of times) {
    await page.evaluate(x => window.seek(x), t);
    await page.screenshot({ path: path.join(outDir, `t${t.toFixed(2).padStart(6, '0')}.png`) });
  }
  await browser.close();
}

(async () => {
  const [mode, ...args] = process.argv.slice(2);
  if (mode === 'video') {
    const [w, h, fps, out, start, end] = args;
    await renderVideo(+w, +h, +fps, out, start === undefined ? undefined : +start, end === undefined ? undefined : +end);
  } else if (mode === 'stills') {
    const [w, h, outDir, list] = args;
    await renderStills(+w, +h, outDir, list.split(',').map(Number));
  } else {
    console.error('usage: node render.js video|stills ...');
    process.exit(1);
  }
})();
