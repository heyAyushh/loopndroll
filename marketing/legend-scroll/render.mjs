#!/usr/bin/env node
/*
 * Frame-exact export of "The Legend of the Looper".
 *
 *   node render.mjs                        # 1080p60 MP4
 *   node render.mjs --stills 12,40.5,88    # PNG stills for review
 *   node render.mjs --fps 30 --workers 5   # faster draft
 *   node render.mjs --score-only           # re-mix audio into the existing MP4
 *   node render.mjs --mix-only             # write out/mix.wav only
 *
 * The folder is served over a local HTTP server (fonts, narration), headless
 * Chrome paints each frame, frames stream in order into ffmpeg, and the score
 * is mixed with the narration (voice chain + sidechain ducking).
 */
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import puppeteer from 'puppeteer-core';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = path.join(HERE, 'out');
const DEFAULT_CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const PENDING_FRAME_LIMIT = 90;
const MIME = { '.html': 'text/html', '.js': 'text/javascript', '.ttf': 'font/ttf', '.mp3': 'audio/mpeg', '.txt': 'text/plain' };
const VOICE_GAIN = { narrator: 1.55, penelope: 1.35 };

function parseArgs(argv) {
  const options = { fps: 60, workers: 4, quality: 0.93, stills: null, scoreOnly: false, mixOnly: false, out: path.join(OUT_DIR, 'legend-of-the-looper.mp4') };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === '--score-only' || flag === '--mix-only') {
      options[flag === '--score-only' ? 'scoreOnly' : 'mixOnly'] = true;
      continue;
    }
    if (flag === '--fps') options.fps = Number(value);
    else if (flag === '--workers') options.workers = Number(value);
    else if (flag === '--quality') options.quality = Number(value);
    else if (flag === '--stills') options.stills = value.split(',').map(Number);
    else if (flag === '--out') options.out = path.resolve(value);
    else continue;
    i++;
  }
  return options;
}

function serveFolder() {
  const server = createServer(async (request, response) => {
    const url = new URL(request.url, 'http://localhost');
    const file = path.join(HERE, decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname));
    if (!file.startsWith(HERE)) {
      response.writeHead(403).end();
      return;
    }
    try {
      const body = await readFile(file);
      response.writeHead(200, { 'content-type': MIME[path.extname(file)] ?? 'application/octet-stream' }).end(body);
    } catch {
      response.writeHead(404).end();
    }
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

async function openTrailerPage(browser, origin) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  page.on('pageerror', (error) => console.error('[page]', error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') console.error('[console]', message.text());
  });
  await page.goto(`${origin}/index.html?export`);
  await page.evaluate(() => window.LegendTrailer.init());
  return page;
}

async function captureFrame(page, time, type, quality) {
  const dataUrl = await page.evaluate((t, mime, q) => window.LegendTrailer.captureFrame(t, mime, q), time, type, quality);
  return Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64');
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'inherit', 'inherit'] });
    child.on('error', reject);
    child.on('exit', (code) => (code === 0 ? resolve() : reject(new Error(`${command} exited with ${code}`))));
  });
}

async function renderStills(browser, origin, times) {
  const page = await openTrailerPage(browser, origin);
  const directory = path.join(OUT_DIR, 'stills');
  await mkdir(directory, { recursive: true });
  for (const time of times) {
    const file = path.join(directory, `still-${time.toFixed(2).padStart(6, '0')}.png`);
    const started = Date.now();
    await writeFile(file, await captureFrame(page, time, 'image/png', 1));
    console.log(`${file} (${Date.now() - started} ms)`);
  }
}

/** Score + narration → one stereo WAV. Voice gets warmth, compression and a hall; the score ducks under it. */
async function renderMix(page) {
  const scorePath = path.join(OUT_DIR, 'score.wav');
  const base64 = await page.evaluate(() => window.LegendScore.renderWavBase64(window.LegendTrailer.cues()));
  await writeFile(scorePath, Buffer.from(base64, 'base64'));
  const narration = await page.evaluate(() => window.LegendTrailer.NARRATION);
  const duration = await page.evaluate(() => window.LegendTrailer.DURATION);
  const inputs = ['-i', scorePath];
  const voiceChains = [];
  narration.forEach((entry, index) => {
    inputs.push('-i', path.join(HERE, entry.file));
    const delay = Math.round(entry.start * 1000);
    voiceChains.push(`[${index + 1}:a]aresample=48000,aformat=channel_layouts=stereo,volume=${VOICE_GAIN[entry.speaker] ?? 1},adelay=${delay}|${delay}[v${index}]`);
  });
  const voiceLabels = narration.map((_, index) => `[v${index}]`).join('');
  const graph = [
    ...voiceChains,
    `${voiceLabels}amix=inputs=${narration.length}:normalize=0:duration=longest,apad=whole_dur=${duration}[voiceRaw]`,
    '[voiceRaw]highpass=f=75,equalizer=f=160:t=q:w=1.2:g=3,equalizer=f=3200:t=q:w=1.5:g=1.5,acompressor=threshold=-20dB:ratio=3:attack=6:release=160:makeup=2,aecho=0.8:0.55:70|130|210:0.22|0.14|0.08[voice]',
    '[voice]asplit=2[voiceMix][voiceKey]',
    '[0:a][voiceKey]sidechaincompress=threshold=0.025:ratio=5:attack=30:release=520:makeup=1[musicDucked]',
    `[musicDucked][voiceMix]amix=inputs=2:normalize=0:duration=first,atrim=0:${duration},alimiter=limit=0.94:level=disabled[out]`,
  ].join(';');
  const mixPath = path.join(OUT_DIR, 'mix.wav');
  await run('ffmpeg', ['-y', '-loglevel', 'error', ...inputs, '-filter_complex', graph, '-map', '[out]', '-c:a', 'pcm_s16le', '-ar', '48000', mixPath]);
  return mixPath;
}

async function remux(browser, origin, options) {
  if (!existsSync(options.out)) throw new Error(`${options.out} does not exist; render the video first`);
  const audioPath = await renderMix(await openTrailerPage(browser, origin));
  const remuxed = options.out.replace(/\.mp4$/, '.remux.mp4');
  await run('ffmpeg', ['-y', '-loglevel', 'error', '-i', options.out, '-i', audioPath, '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '320k', '-shortest', '-movflags', '+faststart', remuxed]);
  await rename(remuxed, options.out);
  console.log(`re-scored ${options.out}`);
}

async function renderVideo(browser, origin, options) {
  const pages = await Promise.all(Array.from({ length: options.workers }, () => openTrailerPage(browser, origin)));
  const audioPath = await renderMix(pages[0]);
  const duration = await pages[0].evaluate(() => window.LegendTrailer.DURATION);
  const frameCount = Math.round(duration * options.fps);
  await mkdir(path.dirname(options.out), { recursive: true });
  const encoder = spawn('ffmpeg', [
    '-y', '-loglevel', 'error',
    '-f', 'image2pipe', '-framerate', String(options.fps), '-c:v', 'mjpeg', '-i', '-',
    '-i', audioPath, '-map', '0:v', '-map', '1:a',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '16', '-pix_fmt', 'yuv420p', '-tune', 'film',
    '-c:a', 'aac', '-b:a', '320k', '-shortest', '-movflags', '+faststart', options.out,
  ], { stdio: ['pipe', 'inherit', 'inherit'] });
  const encoded = new Promise((resolve, reject) => {
    encoder.on('error', reject);
    encoder.on('exit', (code) => (code === 0 ? resolve() : reject(new Error(`ffmpeg exited with ${code}`))));
  });
  const pending = new Map();
  let nextToWrite = 0;
  let nextToRender = 0;
  const startedAt = Date.now();
  const waitForRoom = () => new Promise((resolve) => {
    const check = () => (nextToRender - nextToWrite < PENDING_FRAME_LIMIT ? resolve() : setTimeout(check, 5));
    check();
  });
  const flush = async () => {
    while (pending.has(nextToWrite)) {
      const frame = pending.get(nextToWrite);
      pending.delete(nextToWrite);
      nextToWrite += 1;
      if (!encoder.stdin.write(frame)) await new Promise((resolve) => encoder.stdin.once('drain', resolve));
      if (nextToWrite % (options.fps * 5) === 0) {
        const seconds = (Date.now() - startedAt) / 1000;
        console.log(`frame ${nextToWrite}/${frameCount} · ${(nextToWrite / seconds).toFixed(1)} fps`);
      }
    }
  };
  let flushing = Promise.resolve();
  await Promise.all(pages.map(async (page) => {
    while (nextToRender < frameCount) {
      await waitForRoom();
      const index = nextToRender++;
      if (index >= frameCount) break;
      pending.set(index, await captureFrame(page, index / options.fps, 'image/jpeg', options.quality));
      flushing = flushing.then(flush);
    }
  }));
  await flushing;
  encoder.stdin.end();
  await encoded;
  console.log(`wrote ${options.out}`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const executablePath = process.env.CHROME_PATH ?? DEFAULT_CHROME;
  if (!existsSync(executablePath)) throw new Error(`Chrome not found at ${executablePath}; set CHROME_PATH`);
  await mkdir(OUT_DIR, { recursive: true });
  const server = await serveFolder();
  const origin = `http://127.0.0.1:${server.address().port}`;
  const browser = await puppeteer.launch({ executablePath, headless: true, protocolTimeout: 600000, args: ['--force-color-profile=srgb', '--disable-gpu-sandbox'] });
  try {
    if (options.stills) await renderStills(browser, origin, options.stills);
    else if (options.scoreOnly) await remux(browser, origin, options);
    else if (options.mixOnly) console.log(`wrote ${await renderMix(await openTrailerPage(browser, origin))}`);
    else await renderVideo(browser, origin, options);
  } finally {
    await browser.close();
    server.close();
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
