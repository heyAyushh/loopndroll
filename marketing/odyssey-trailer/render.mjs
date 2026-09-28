#!/usr/bin/env node
/*
 * Frame-exact export of the Looper: The Odyssey trailer.
 *
 *   node render.mjs                       # full MP4 at 60 fps
 *   node render.mjs --stills 8.5,23,43    # PNG stills for review
 *   node render.mjs --fps 30 --workers 6  # faster draft
 *   node render.mjs --score-only          # re-mix audio into the existing MP4
 *
 * Headless Chrome paints each frame (every frame is a pure function of time),
 * frames stream in order into ffmpeg, and the synthesized score plus a
 * macOS `say -v Whisper` line are muxed in.
 */
import { spawn, execFileSync } from 'node:child_process';
import { mkdir, writeFile, rm, rename } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import puppeteer from 'puppeteer-core';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const OUT_DIR = path.join(HERE, 'out');
const DEFAULT_CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const PENDING_FRAME_LIMIT = 90;
const WHISPER_LINE = 'Wait.';
const WHISPER_GAIN = 2.4;

function parseArgs(argv) {
  const options = { fps: 60, workers: 4, quality: 0.94, stills: null, scoreOnly: false, out: path.join(OUT_DIR, 'looper-odyssey-trailer.mp4') };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (flag === '--score-only') {
      options.scoreOnly = true;
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

async function openTrailerPage(browser) {
  const context = await browser.createBrowserContext();
  const page = await context.newPage();
  page.on('pageerror', (error) => console.error('[page]', error.message));
  await page.goto(`${pathToFileURL(path.join(HERE, 'index.html')).href}?export`);
  await page.evaluate(() => window.LooperTrailer.init());
  return page;
}

async function captureFrame(page, time, type, quality) {
  const dataUrl = await page.evaluate((t, mime, q) => window.LooperTrailer.captureFrame(t, mime, q), time, type, quality);
  return Buffer.from(dataUrl.slice(dataUrl.indexOf(',') + 1), 'base64');
}

function run(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: ['ignore', 'inherit', 'inherit'] });
    child.on('error', reject);
    child.on('exit', (code) => (code === 0 ? resolve() : reject(new Error(`${command} exited with ${code}`))));
  });
}

async function renderStills(browser, times) {
  const page = await openTrailerPage(browser);
  const directory = path.join(OUT_DIR, 'stills');
  await mkdir(directory, { recursive: true });
  for (const time of times) {
    const file = path.join(directory, `still-${time.toFixed(2).padStart(5, '0')}.png`);
    await writeFile(file, await captureFrame(page, time, 'image/png', 1));
    console.log(file);
  }
}

async function renderScore(page) {
  const scorePath = path.join(OUT_DIR, 'score.wav');
  const base64 = await page.evaluate(() => window.LooperTrailerAudio.renderWavBase64(window.LooperTrailer.CUES));
  await writeFile(scorePath, Buffer.from(base64, 'base64'));
  const cues = await page.evaluate(() => window.LooperTrailer.CUES);
  const whisperPath = path.join(OUT_DIR, 'wait.aiff');
  const mixPath = path.join(OUT_DIR, 'mix.wav');
  try {
    execFileSync('say', ['-v', 'Whisper', '-o', whisperPath, WHISPER_LINE]);
  } catch {
    console.warn('macOS Whisper voice unavailable; exporting the score without the line.');
    return scorePath;
  }
  const delayMs = Math.round(cues.wait * 1000);
  await run('ffmpeg', [
    '-y', '-loglevel', 'error', '-i', scorePath, '-i', whisperPath,
    '-filter_complex',
    `[1:a]aresample=48000,aformat=channel_layouts=stereo,volume=${WHISPER_GAIN},adelay=${delayMs}|${delayMs}[line];[0:a][line]amix=inputs=2:duration=first:normalize=0[out]`,
    '-map', '[out]', '-c:a', 'pcm_s16le', mixPath,
  ]);
  return mixPath;
}

async function remuxScore(browser, options) {
  if (!existsSync(options.out)) throw new Error(`${options.out} does not exist; render the video first`);
  const audioPath = await renderScore(await openTrailerPage(browser));
  const remuxed = options.out.replace(/\.mp4$/, '.remux.mp4');
  await run('ffmpeg', [
    '-y', '-loglevel', 'error', '-i', options.out, '-i', audioPath,
    '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '256k', '-shortest', '-movflags', '+faststart', remuxed,
  ]);
  await rename(remuxed, options.out);
  console.log(`re-scored ${options.out}`);
}

async function renderVideo(browser, options) {
  const pages = await Promise.all(Array.from({ length: options.workers }, () => openTrailerPage(browser)));
  const audioPath = await renderScore(pages[0]);
  const duration = await pages[0].evaluate(() => window.LooperTrailer.DURATION);
  const frameCount = Math.round(duration * options.fps);
  await mkdir(path.dirname(options.out), { recursive: true });

  const encoder = spawn('ffmpeg', [
    '-y', '-loglevel', 'error',
    '-f', 'image2pipe', '-framerate', String(options.fps), '-c:v', 'mjpeg', '-i', '-',
    '-i', audioPath,
    '-map', '0:v', '-map', '1:a',
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '15', '-pix_fmt', 'yuv420p', '-tune', 'grain',
    '-c:a', 'aac', '-b:a', '256k', '-shortest', '-movflags', '+faststart',
    options.out,
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
      if (nextToWrite % options.fps === 0) {
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
  const browser = await puppeteer.launch({ executablePath, headless: true, args: ['--allow-file-access-from-files', '--force-color-profile=srgb'] });
  try {
    if (options.stills) await renderStills(browser, options.stills);
    else if (options.scoreOnly) await remuxScore(browser, options);
    else await renderVideo(browser, options);
  } finally {
    await browser.close();
    await rm(path.join(OUT_DIR, 'wait.aiff'), { force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
