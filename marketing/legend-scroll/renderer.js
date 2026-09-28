/* Legend of the Looper — frame compositor: table, scroll, multiplane painting, light, words. */
(function () {
  'use strict';

  const L = window.Legend;
  const { WIDTH, HEIGHT, COLOR, RGB, clamp, lerp, progress, smooth, easeInOut, easeOut, seededRandom, drawGlow, catmullRom, BEAT } = L;

  const PAPER_OVERSCAN = 0;
  const TABLE_CANDLE_START = [1950, 250];
  const TABLE_CANDLE_END = [-42500, 250];
  const MOTE_COUNT = 70;
  const GRAIN_FRAMES = 6;
  const GRAIN_FPS = 24;

  const surfaces = {};
  let layers = null;
  let ready = null;

  function buildGrain() {
    const random = seededRandom(31);
    return Array.from({ length: GRAIN_FRAMES }, () => {
      const canvas = L.createSurface(WIDTH / 2, HEIGHT / 2);
      const g = canvas.getContext('2d');
      const image = g.createImageData(canvas.width, canvas.height);
      for (let i = 0; i < image.data.length; i += 4) {
        const value = 128 + (random() - 0.5) * 90;
        image.data[i] = value;
        image.data[i + 1] = value;
        image.data[i + 2] = value;
        image.data[i + 3] = 255;
      }
      g.putImageData(image, 0, 0);
      return canvas;
    });
  }

  function buildVignette() {
    const canvas = L.createSurface(WIDTH, HEIGHT);
    const g = canvas.getContext('2d');
    const gradient = g.createRadialGradient(WIDTH / 2, HEIGHT * 0.48, HEIGHT * 0.3, WIDTH / 2, HEIGHT / 2, WIDTH * 0.66);
    gradient.addColorStop(0, 'rgb(255, 255, 255)');
    gradient.addColorStop(0.7, 'rgb(236, 222, 200)');
    gradient.addColorStop(1, 'rgb(150, 112, 80)');
    g.fillStyle = gradient;
    g.fillRect(0, 0, WIDTH, HEIGHT);
    return canvas;
  }

  async function init() {
    if (ready) return ready;
    ready = (async () => {
      await Promise.all([
        document.fonts.load(`80px ${L.FONT.brush}`, '循環傳說十年准靜同成等'),
        document.fonts.load(`80px ${L.FONT.running}`, '一二三四五六七八九十等傳說循環不息'),
        document.fonts.load(`600 40px ${L.FONT.serif}`),
        document.fonts.load(`italic 500 40px ${L.FONT.serif}`),
        document.fonts.load(`italic 600 40px ${L.FONT.serif}`),
        document.fonts.load(`700 40px ${L.FONT.serif}`),
      ]).catch(() => {});
      surfaces.paperTile = L.buildPaperTile();
      surfaces.grain = buildGrain();
      surfaces.vignette = buildVignette();
      const style = { spacing: 520, heights: [230, 470], widths: [260, 540], tone: 0.2, texture: 6, dots: 4, blur: 5, height: 600 };
      const midStyle = { spacing: 430, heights: [140, 330], widths: [200, 380], tone: 0.34, texture: 12, dots: 10, blur: 2.2, height: 480 };
      layers = {
        far: { depth: 0.2, baseline: 700, ...L.buildMountainLayer({ seed: 1, from: -11500, to: 2800, ...style }) },
        mid: { depth: 0.45, baseline: 772, ...L.buildMountainLayer({ seed: 2, from: -22400, to: 3200, ...midStyle }) },
        flatStart: { depth: 1, baseline: 772, ...L.buildMountainLayer({ seed: 3, from: -2400, to: 3400, ...midStyle }) },
        flatEnd: { depth: 1, baseline: 772, ...L.buildMountainLayer({ seed: 4, from: -46400, to: -38400, ...midStyle }) },
      };
    })();
    return ready;
  }

  // ------------------------------------------------------------- camera
  function rollerAt(t) {
    const opening = easeInOut(progress(t, BEAT.unroll[0], BEAT.unroll[1]));
    if (t < BEAT.rollUp[0]) return { side: 'left', x: lerp(L.SCROLL_RIGHT_EDGE, -1700, opening), active: t < BEAT.unroll[1] + 0.01 };
    const closing = easeInOut(progress(t, BEAT.rollUp[0], BEAT.rollUp[1]));
    return { side: 'right', x: lerp(L.SCROLL_LEFT_EDGE, -41600, closing), active: true };
  }

  function cameraFor(t) {
    const camera = L.cameraAt(t);
    if (t > BEAT.rollUp[0]) {
      const roller = rollerAt(t);
      camera.x = lerp(camera.x, roller.x + 620, smooth(progress(t, BEAT.rollUp[0], BEAT.rollUp[0] + 1.2)));
    }
    return camera;
  }

  /** Transform for a layer at parallax `depth` (1 = the paper plane). */
  function layerMatrix(camera, depth, anchorX = 0) {
    const zoom = 1 + (camera.zoom - 1) * depth;
    const offsetX = camera.x * depth + anchorX * (1 - depth);
    const offsetY = camera.y * depth + 540 * (1 - depth);
    const base = new DOMMatrix().translate(WIDTH / 2, HEIGHT / 2).rotate((camera.roll * 180) / Math.PI).translate(-WIDTH / 2, -HEIGHT / 2);
    return base.multiply(new DOMMatrix([zoom, 0, 0, zoom, WIDTH / 2 - zoom * offsetX, HEIGHT / 2 - zoom * offsetY]));
  }

  function visibleRange(camera, depth, anchorX = 0) {
    const zoom = 1 + (camera.zoom - 1) * depth;
    const center = camera.x * depth + anchorX * (1 - depth);
    const half = (WIDTH / 2 / zoom) * 1.25;
    return [center - half, center + half];
  }

  // ------------------------------------------------------------- painting
  function paintPaper(ctx, camera, clip, alive) {
    ctx.setTransform(layerMatrix(camera, 1));
    const top = lerp(0, -PAPER_OVERSCAN, alive);
    const bottom = lerp(L.PAINT_HEIGHT, L.PAINT_HEIGHT + PAPER_OVERSCAN, alive);
    if (!surfaces.paperPattern) surfaces.paperPattern = ctx.createPattern(surfaces.paperTile, 'repeat');
    ctx.fillStyle = surfaces.paperPattern;
    ctx.fillRect(clip[0], top, clip[1] - clip[0], bottom - top);
  }

  function paintSeaBands(ctx, camera, t) {
    ctx.setTransform(layerMatrix(camera, 1));
    const [left, right] = visibleRange(camera, 1);
    const storming = progress(t, BEAT.toStorm[0], BEAT.dragon[0]) * (1 - progress(t, BEAT.silence[0], BEAT.silence[0] + 0.6));
    L.SEA_RANGES.forEach(([from, to]) => {
      const a = Math.max(from, left);
      const b = Math.min(to, right);
      if (a >= b) return;
      const inStorm = from === -27400;
      L.chineseSea(ctx, a, b, L.HORIZON, L.PAINT_HEIGHT + PAPER_OVERSCAN, t, {
        amplitude: inStorm ? 1 + storming * 2.4 : 1,
        wash: inStorm ? `rgba(30, 40, 60, ${0.16 + storming * 0.3})` : 'rgba(46, 60, 84, 0.16)',
        rowScale: inStorm ? 1 + storming * 0.5 : 1,
      });
    });
  }

  function paintMountains(ctx, camera, t, alive) {
    const drawLayer = (layer, alpha) => {
      if (alpha <= 0.01) return;
      ctx.setTransform(layerMatrix(camera, layer.depth));
      const [left, right] = visibleRange(camera, layer.depth);
      L.withAlpha(ctx, alpha, () => L.drawMountainLayer(ctx, layer, left, right, layer.baseline));
    };
    drawLayer(layers.far, alive);
    ctx.setTransform(layerMatrix(camera, layers.far.depth));
    const [farLeft, farRight] = visibleRange(camera, layers.far.depth);
    L.withAlpha(ctx, alive, () => L.mistBand(ctx, farLeft, farRight, 640, t, { alpha: 0.9, speed: 8, seed: 21, height: 220 }));
    drawLayer(layers.mid, alive);
    drawLayer(layers.flatStart, 1 - alive);
    drawLayer(layers.flatEnd, 1 - alive);
    ctx.setTransform(layerMatrix(camera, layers.mid.depth));
    const [midLeft, midRight] = visibleRange(camera, layers.mid.depth);
    L.withAlpha(ctx, 0.3 + 0.7 * alive, () => L.mistBand(ctx, midLeft, midRight, 740, t, { alpha: 0.95, speed: 14, seed: 22, height: 200 }));
  }

  function paintStations(ctx, camera, t) {
    const [left, right] = visibleRange(camera, 1);
    L.STATIONS.forEach((station) => {
      if (station.x + station.to < left || station.x + station.from > right) return;
      ctx.setTransform(layerMatrix(camera, 1).translate(station.x, 0));
      ctx.save();
      station.draw(ctx, t);
      ctx.restore();
    });
  }

  function paintScrollObject(ctx, camera, t, clip, alive) {
    const flat = 1 - alive;
    if (flat <= 0.01) return;
    ctx.setTransform(layerMatrix(camera, 1));
    L.withAlpha(ctx, flat, () => {
      L.brocadeBorder(ctx, clip[0], clip[1], -64, 64);
      L.brocadeBorder(ctx, clip[0], clip[1], L.PAINT_HEIGHT, 64, true);
      const roller = rollerAt(t);
      if (roller.active) L.scrollRoller(ctx, roller.x, -70, L.PAINT_HEIGHT + 70, 46);
      if (t < BEAT.rollUp[0]) {
        L.scrollRoller(ctx, L.SCROLL_RIGHT_EDGE + 18, -80, L.PAINT_HEIGHT + 80, 18);
        paintCord(ctx, t, roller.x);
      } else {
        L.scrollRoller(ctx, L.SCROLL_LEFT_EDGE - 18, -80, L.PAINT_HEIGHT + 80, 18);
        paintTie(ctx, t, roller.x);
      }
    });
  }

  function paintCord(ctx, t, rollerX) {
    const untie = easeInOut(progress(t, BEAT.cordUntie[0], BEAT.cordUntie[1]));
    const anchor = [L.SCROLL_RIGHT_EDGE + 18, 540];
    if (untie < 1) {
      for (let loop = 0; loop < 3; loop++) {
        const spread = untie * 260;
        const y = 460 + loop * 70 + spread * (loop - 1) * 0.6;
        L.redThread(ctx, [[rollerX - 50, y + 12], [rollerX, y - 6 - spread * 0.2], [rollerX + 50, y + 12]], { width: 7, alpha: 1 - untie * 0.6 });
      }
    }
    const loose = catmullRom([anchor, [anchor[0] + 120, 620 + untie * 200], [anchor[0] + 260 + untie * 200, 760 + untie * 380], [anchor[0] + 180 + untie * 520, 820 + untie * 520]], 14);
    L.redThread(ctx, loose, { width: 7 });
  }

  function paintTie(ctx, t, rollerX) {
    const tie = progress(t, BEAT.tie[0], BEAT.tie[1]);
    if (tie <= 0) return;
    for (let loop = 0; loop < 3; loop++) {
      const reveal = clamp(tie * 3 - loop);
      if (reveal <= 0) continue;
      const y = 470 + loop * 70;
      L.redThread(ctx, [[rollerX - 50, y + 12], [rollerX, y - 8], [rollerX + 50, y + 12]], { width: 7, reveal });
    }
    const knot = progress(t, BEAT.tie[0] + 0.8, BEAT.tie[1]);
    if (knot > 0) {
      const infinity = [];
      for (let i = 0; i <= 80; i++) {
        const a = (i / 80) * Math.PI * 2;
        const scale = 90 / (1 + Math.sin(a) * Math.sin(a));
        infinity.push([rollerX + Math.cos(a) * scale * 1.2, 540 + Math.sin(a) * Math.cos(a) * scale * 1.2]);
      }
      L.redThread(ctx, infinity, { width: 7, reveal: easeInOut(knot) });
      if (knot > 0.95) drawGlow(ctx, rollerX, 540, 260, RGB.thread, 0.5 * (knot - 0.95) * 20 * 0.2);
    }
  }

  function paintTable(ctx, t, camera, alive) {
    const flat = 1 - alive;
    if (flat <= 0.01) return;
    const candleAt = t < 70 ? TABLE_CANDLE_START : TABLE_CANDLE_END;
    const candleScreen = layerMatrix(camera, 1).transformPoint(new DOMPoint(candleAt[0], candleAt[1]));
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    L.withAlpha(ctx, flat, () => L.lacquerTable(ctx, t, clamp(progress(t, BEAT.candle, BEAT.candle + 1.6) + (t > 70 ? 1 : 0)), [candleScreen.x, candleScreen.y]));
    ctx.setTransform(layerMatrix(camera, 1));
    const lit = progress(t, BEAT.candle, BEAT.candle + 0.4);
    L.withAlpha(ctx, flat, () => {
      L.candle(ctx, candleAt[0], candleAt[1], t, lit);
      L.incenseSmoke(ctx, candleAt[0] + (t < 70 ? 520 : -520), 980, t, lit);
    });
  }

  function paintCandleShade(ctx, t, camera, alive) {
    const flat = 1 - alive;
    if (flat <= 0.01) return;
    const candleAt = t < 70 ? TABLE_CANDLE_START : TABLE_CANDLE_END;
    const matrix = layerMatrix(camera, 1);
    const screen = matrix.transformPoint(new DOMPoint(candleAt[0], candleAt[1]));
    const lit = clamp(progress(t, BEAT.candle, BEAT.candle + 1.8) + (t > 70 ? 1 : 0));
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    const shade = ctx.createRadialGradient(screen.x, screen.y, 80, screen.x - 500, screen.y + 300, 1700);
    shade.addColorStop(0, `rgba(255, 236, 200, ${1 - 0.1 * flat})`);
    shade.addColorStop(0.45, `rgba(200, 150, 100, ${1 - 0.1 * flat})`);
    shade.addColorStop(1, `rgba(40, 20, 12, ${1})`);
    ctx.save();
    ctx.globalCompositeOperation = 'multiply';
    ctx.globalAlpha = flat * (0.35 + 0.65 * lit) + flat * (1 - lit) * 0.65;
    ctx.fillStyle = shade;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    if (lit < 1) {
      ctx.globalCompositeOperation = 'source-over';
      ctx.globalAlpha = flat * (1 - lit);
      ctx.fillStyle = '#000';
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }
    ctx.restore();
  }

  // ------------------------------------------------------------- lighting
  function applyDarkness(ctx, dark, strength) {
    if (dark <= 0.01) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    ctx.globalCompositeOperation = 'multiply';
    ctx.globalAlpha = dark * strength;
    ctx.fillStyle = 'rgb(48, 56, 92)';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.restore();
  }

  function applyBackglow(ctx, alive, dark) {
    if (alive <= 0.01) return;
    drawGlow(ctx, WIDTH * 0.52, HEIGHT * 0.45, WIDTH * 0.62, RGB.candle, 0.16 * alive * (1 - dark * 0.5), 'soft-light');
  }

  function paintMotes(ctx, t, camera, amount) {
    if (amount <= 0.01) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    const random = seededRandom(707);
    for (let i = 0; i < MOTE_COUNT; i++) {
      const baseX = random() * (WIDTH + 400);
      const baseY = random() * HEIGHT;
      const speed = 8 + random() * 22;
      const size = 1.2 + random() * 2.6;
      const parallax = 0.3 + random() * 0.6;
      const x = ((((baseX - camera.x * parallax * 0.4 + Math.sin(t * 0.5 + i) * 30) % (WIDTH + 400)) + WIDTH + 400) % (WIDTH + 400)) - 200;
      const y = ((((baseY - t * speed) % HEIGHT) + HEIGHT) % HEIGHT);
      const twinkle = 0.5 + 0.5 * Math.sin(t * (1 + random() * 3) + i);
      drawGlow(ctx, x, y, size * 6, RGB.gold, amount * twinkle * 0.6);
    }
  }

  function paintNarration(ctx, t, dark) {
    const entry = L.NARRATION.find((line) => t >= line.start - 0.2 && t <= line.start + line.duration + 0.9);
    if (!entry) return;
    const appear = progress(t, entry.start - 0.2, entry.start + 0.35);
    const vanish = 1 - progress(t, entry.start + entry.duration + 0.4, entry.start + entry.duration + 0.9);
    const alpha = smooth(appear) * smooth(vanish);
    if (alpha <= 0.01) return;
    const penelope = entry.speaker === 'penelope';
    const text = penelope ? '“Wait.”' : entry.text;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    ctx.font = penelope ? `italic 600 48px ${L.FONT.serif}` : `italic 500 42px ${L.FONT.serif}`;
    ctx.letterSpacing = '0.5px';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.globalAlpha = alpha;
    if (appear < 1) ctx.filter = `blur(${((1 - appear) * 5).toFixed(2)}px)`;
    const light = dark > 0.4;
    ctx.shadowColor = light ? 'rgba(0, 0, 0, 0.8)' : 'rgba(243, 235, 216, 0.9)';
    ctx.shadowBlur = 14;
    ctx.fillStyle = light ? 'rgba(243, 235, 216, 0.96)' : 'rgba(26, 21, 18, 0.92)';
    ctx.fillText(text, WIDTH / 2, HEIGHT - 92);
    ctx.restore();
  }

  function paintTitleCard(ctx, t) {
    const appear = progress(t, BEAT.title, BEAT.title + 1.2);
    if (appear <= 0) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    ctx.globalAlpha = 0.55 * smooth(appear);
    ctx.fillStyle = '#050201';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.restore();
    ctx.save();
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.globalAlpha = smooth(appear);
    if (appear < 1) ctx.filter = `blur(${((1 - appear) * 10).toFixed(1)}px)`;
    ctx.font = `600 150px ${L.FONT.serif}`;
    ctx.letterSpacing = '64px';
    ctx.shadowColor = 'rgba(226, 179, 90, 0.7)';
    ctx.shadowBlur = 40;
    ctx.fillStyle = '#F3EBD8';
    ctx.fillText('LOOPER', WIDTH / 2 + 32, 430);
    ctx.shadowBlur = 0;
    ctx.font = `italic 500 44px ${L.FONT.serif}`;
    ctx.letterSpacing = '2px';
    ctx.fillStyle = 'rgba(243, 235, 216, 0.85)';
    ctx.fillText('The work goes on. So can you.', WIDTH / 2, 560);
    ctx.restore();
    L.seal(ctx, WIDTH / 2, 710, 120, '循環', progress(t, BEAT.title + 0.9, BEAT.title + 1.6), { style: 'yin', columns: 1 });
  }

  function applyGrade(ctx, t, alive, blank) {
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    ctx.globalCompositeOperation = 'multiply';
    ctx.globalAlpha = 0.75 * alive + 0.25;
    ctx.drawImage(surfaces.vignette, 0, 0);
    ctx.globalCompositeOperation = 'overlay';
    ctx.globalAlpha = 0.07;
    ctx.drawImage(surfaces.grain[Math.floor(t * GRAIN_FPS) % GRAIN_FRAMES], 0, 0, WIDTH, HEIGHT);
    ctx.restore();
    void blank;
  }

  function applyBlank(ctx, blank) {
    if (blank <= 0.01) return;
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.save();
    ctx.globalAlpha = blank * 0.9;
    ctx.fillStyle = COLOR.paperLight;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.restore();
  }

  function paintBrokenThreadOverlay(ctx, camera, t, blank) {
    if (blank <= 0.01) return;
    ctx.setTransform(layerMatrix(camera, 1).translate(L.STATION.storm, 0));
    const drop = progress(t, BEAT.inkDrop[0], BEAT.inkDrop[1]);
    L.withAlpha(ctx, blank, () => {
      const threadEnd = [700, 880];
      L.redThread(ctx, catmullRom([[-500, 1000], [-200, 820], [100, 900], threadEnd], 12), { width: 3.4 });
      L.redThread(ctx, catmullRom([[1500, 360], [1300, 700], [1100, 960]], 12), { width: 3.4 });
      if (drop > 0) {
        const fallY = lerp(-60, 600, clamp(drop / 0.6) * clamp(drop / 0.6));
        if (drop < 0.6) {
          ctx.fillStyle = COLOR.ink;
          L.fillEllipse(ctx, 0, fallY, 12, 20);
        } else {
          L.inkSplash(ctx, 0, 600, 280, (drop - 0.6) / 0.4, 777, 0.9);
        }
      }
    });
  }

  function renderFrame(ctx, time) {
    const t = clamp(time, 0, L.DURATION_SECONDS - 1e-6);
    const camera = cameraFor(t);
    const alive = clamp(camera.depth);
    const dark = L.darknessAt(t);
    const blank = L.blanknessAt(t);
    const roller = rollerAt(t);
    const clip = roller.side === 'left' && roller.active ? [roller.x, L.SCROLL_RIGHT_EDGE] : roller.side === 'right' ? [roller.x, -38000] : [L.SCROLL_LEFT_EDGE, L.SCROLL_RIGHT_EDGE];

    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    ctx.filter = 'none';
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    paintTable(ctx, t, camera, alive);

    ctx.save();
    {
      ctx.setTransform(layerMatrix(camera, 1));
      ctx.beginPath();
      const top = lerp(0, -PAPER_OVERSCAN, alive);
      const bottom = lerp(L.PAINT_HEIGHT, L.PAINT_HEIGHT + PAPER_OVERSCAN, alive);
      ctx.rect(clip[0], top, clip[1] - clip[0], bottom - top);
      ctx.clip();
    }
    paintPaper(ctx, camera, clip, alive);
    paintMountains(ctx, camera, t, alive);
    paintSeaBands(ctx, camera, t);
    applyDarkness(ctx, dark, 0.8);
    applyBackglow(ctx, alive, dark);
    paintStations(ctx, camera, t);
    applyDarkness(ctx, dark, 0.3);
    ctx.restore();

    paintScrollObject(ctx, camera, t, clip, alive);
    paintCandleShade(ctx, t, camera, alive);
    applyBlank(ctx, blank);
    paintBrokenThreadOverlay(ctx, camera, t, blank);
    paintMotes(ctx, t, camera, (0.5 + 0.5 * (1 - alive)) * (1 - blank) * (t > BEAT.candle ? 1 : 0));
    applyGrade(ctx, t, alive, blank);
    paintNarration(ctx, t, Math.max(dark, 1 - alive));
    paintTitleCard(ctx, t);
    const fade = progress(t, BEAT.fadeOut[0], BEAT.fadeOut[1]);
    if (fade > 0) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = `rgba(0, 0, 0, ${fade})`;
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }
    ctx.restore();
  }

  const exportCanvas = { surface: null };
  function captureFrame(time, type = 'image/jpeg', quality = 0.94) {
    if (!exportCanvas.surface) exportCanvas.surface = L.createSurface(WIDTH, HEIGHT);
    renderFrame(exportCanvas.surface.getContext('2d'), time);
    return exportCanvas.surface.toDataURL(type, quality);
  }

  window.LegendTrailer = {
    WIDTH, HEIGHT, DURATION: L.DURATION_SECONDS, BEAT, NARRATION: L.NARRATION,
    init, renderFrame, captureFrame,
    cues: () => ({ beat: BEAT, narration: L.NARRATION, ringPasses: Array.from({ length: L.RING.count }, (_, i) => L.ringPass(i)), duration: L.DURATION_SECONDS }),
  };
  void easeOut; void COLOR;
})();
