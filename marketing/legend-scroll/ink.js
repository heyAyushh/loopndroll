/* Legend of the Looper — ink painting toolkit (paper, brush, landscape, weather, seals). */
(function () {
  'use strict';

  const L = window.Legend;
  const { COLOR, RGB, clamp, lerp, progress, smooth, easeOut, seededRandom, noise1, noise2, fbm1, createSurface, fillCircle, fillEllipse,
    polygonPath, strokePolyline, catmullRom, polylineLengths, pointOnPath, slicePath, drawGlow } = L;

  // ------------------------------------------------------------------ paper
  const PAPER_TILE = 1024;

  function periodicNoise(x, y, period) {
    const ix = Math.floor(x);
    const iy = Math.floor(y);
    const fx = smooth(x - ix);
    const fy = smooth(y - iy);
    const h = (a, b) => {
      const v = Math.sin((((a % period) + period) % period) * 127.1 + (((b % period) + period) % period) * 311.7) * 43758.5453123;
      return v - Math.floor(v);
    };
    return lerp(lerp(h(ix, iy), h(ix + 1, iy), fx), lerp(h(ix, iy + 1), h(ix + 1, iy + 1), fx), fy) * 2 - 1;
  }

  /** Seamless xuan-paper tile: periodic cloudiness, wrapped fibres, soft stains. */
  function buildPaperTile() {
    const tile = createSurface(PAPER_TILE, PAPER_TILE);
    const g = tile.getContext('2d');
    const random = seededRandom(404);
    g.fillStyle = COLOR.paper;
    g.fillRect(0, 0, PAPER_TILE, PAPER_TILE);
    const image = g.getImageData(0, 0, PAPER_TILE, PAPER_TILE);
    for (let y = 0; y < PAPER_TILE; y++) {
      for (let x = 0; x < PAPER_TILE; x++) {
        const i = (y * PAPER_TILE + x) * 4;
        const cloud = periodicNoise(x / 128, y / 128, 8) * 6 + periodicNoise(x / 32, y / 32, 32) * 3;
        const grain = (random() - 0.5) * 9;
        image.data[i] += cloud + grain;
        image.data[i + 1] += cloud + grain * 0.9;
        image.data[i + 2] += cloud * 0.8 + grain * 0.8;
      }
    }
    g.putImageData(image, 0, 0);
    const wrapped = (paint) => {
      for (const dx of [-PAPER_TILE, 0, PAPER_TILE]) {
        for (const dy of [-PAPER_TILE, 0, PAPER_TILE]) {
          g.save();
          g.translate(dx, dy);
          paint();
          g.restore();
        }
      }
    };
    g.lineCap = 'round';
    for (let i = 0; i < 2600; i++) {
      const x = random() * PAPER_TILE;
      const y = random() * PAPER_TILE;
      const angle = random() * Math.PI;
      const length = 6 + random() * 30;
      const color = random() < 0.5 ? `rgba(150, 120, 80, ${0.05 + random() * 0.08})` : `rgba(255, 250, 235, ${0.08 + random() * 0.1})`;
      const width = 0.6 + random() * 0.8;
      const bend = (random() - 0.5) * 6;
      wrapped(() => {
        g.strokeStyle = color;
        g.lineWidth = width;
        g.beginPath();
        g.moveTo(x, y);
        g.quadraticCurveTo(x + Math.cos(angle) * length * 0.5 + bend, y + Math.sin(angle) * length * 0.5, x + Math.cos(angle) * length, y + Math.sin(angle) * length);
        g.stroke();
      });
    }
    for (let i = 0; i < 18; i++) {
      const x = random() * PAPER_TILE;
      const y = random() * PAPER_TILE;
      const radius = 60 + random() * 160;
      const alpha = 0.025 + random() * 0.03;
      wrapped(() => {
        const stain = g.createRadialGradient(x, y, 0, x, y, radius);
        stain.addColorStop(0, `rgba(150, 110, 60, ${alpha})`);
        stain.addColorStop(1, 'rgba(150, 110, 60, 0)');
        g.fillStyle = stain;
        g.fillRect(x - radius, y - radius, radius * 2, radius * 2);
      });
    }
    return tile;
  }

  // ------------------------------------------------------------------ brush
  /**
   * A pressure-sensitive brush stroke along `points`: tapered entry and exit,
   * wobbling edges, and optional "flying white" dry streaks near the tail.
   */
  function brushStroke(ctx, points, width, options = {}) {
    const {
      color = COLOR.ink, alpha = 1, taperStart = 0.2, taperEnd = 0.4, wobble = 0.18, dry = 0.2,
      dryColor = COLOR.paper, seed = 1, reveal = 1, minWidth = 0.08,
    } = options;
    if (reveal <= 0 || points.length < 2) return;
    const fullLengths = polylineLengths(points);
    const fullTotal = fullLengths[fullLengths.length - 1] || 1;
    const path = reveal < 1 ? slicePath(points, reveal) : points;
    if (path.length < 2) return;
    const lengths = polylineLengths(path);
    const left = [];
    const right = [];
    for (let i = 0; i < path.length; i++) {
      const a = path[Math.max(0, i - 1)];
      const b = path[Math.min(path.length - 1, i + 1)];
      const angle = Math.atan2(b[1] - a[1], b[0] - a[0]);
      const u = lengths[i] / fullTotal;
      let profile = 1;
      if (u < taperStart) profile = Math.sin((u / taperStart) * Math.PI * 0.5);
      if (u > 1 - taperEnd) profile = Math.min(profile, Math.cos(((u - (1 - taperEnd)) / taperEnd) * Math.PI * 0.5));
      const half = (width * Math.max(minWidth, profile) * (1 + wobble * noise1(u * 11 + seed * 3.1))) / 2;
      const nx = -Math.sin(angle);
      const ny = Math.cos(angle);
      left.push([path[i][0] + nx * half, path[i][1] + ny * half]);
      right.push([path[i][0] - nx * half, path[i][1] - ny * half]);
    }
    ctx.save();
    ctx.globalAlpha *= alpha;
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.moveTo(left[0][0], left[0][1]);
    for (let i = 1; i < left.length; i++) ctx.lineTo(left[i][0], left[i][1]);
    for (let i = right.length - 1; i >= 0; i--) ctx.lineTo(right[i][0], right[i][1]);
    ctx.closePath();
    ctx.fill();
    if (dry > 0 && width > 6) {
      const random = seededRandom(seed * 977 + 13);
      ctx.strokeStyle = dryColor;
      ctx.lineCap = 'round';
      const streaks = Math.round(3 + width / 6);
      for (let k = 0; k < streaks; k++) {
        const offset = (random() - 0.5) * 0.8;
        const startU = 0.35 + random() * 0.4;
        ctx.globalAlpha = alpha * dry * (0.4 + random() * 0.6);
        ctx.lineWidth = 0.6 + random() * Math.max(1, width * 0.05);
        ctx.beginPath();
        let started = false;
        for (let i = 0; i < path.length; i++) {
          const u = lengths[i] / fullTotal;
          if (u < startU) continue;
          const x = lerp(right[i][0], left[i][0], 0.5 + offset);
          const y = lerp(right[i][1], left[i][1], 0.5 + offset);
          if (!started) {
            ctx.moveTo(x, y);
            started = true;
          } else ctx.lineTo(x, y);
        }
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  function inkDot(ctx, x, y, radius, alpha = 1, seed = 1) {
    const random = seededRandom(seed);
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.globalAlpha *= alpha;
    ctx.beginPath();
    for (let i = 0; i <= 14; i++) {
      const angle = (i / 14) * Math.PI * 2;
      const r = radius * (0.8 + random() * 0.35);
      const px = x + Math.cos(angle) * r;
      const py = y + Math.sin(angle) * r * 0.85;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  // --------------------------------------------------------------- landscape
  const CHUNK_WIDTH = 2048;
  const CHUNK_MARGIN = 140;

  /** Peaks along an infinite line, deterministic per layer seed. */
  function peaksBetween(seed, from, to, spacing, heightRange, widthRange) {
    const peaks = [];
    const start = Math.floor(from / spacing) - 2;
    const end = Math.ceil(to / spacing) + 2;
    for (let k = start; k <= end; k++) {
      const r1 = L.seededRandom(seed * 7919 + k * 104729)();
      const r2 = L.seededRandom(seed * 3571 + k * 1299709)();
      const r3 = L.seededRandom(seed * 6007 + k * 15485863)();
      peaks.push({
        x: (k + r1 * 0.8) * spacing,
        height: lerp(heightRange[0], heightRange[1], Math.pow(r2, 0.8)),
        width: lerp(widthRange[0], widthRange[1], r3),
        lean: (r1 - 0.5) * 0.5,
        seed: k,
      });
    }
    return peaks.sort((a, b) => a.height - b.height);
  }

  function peakProfile(peak, x) {
    const u = (x - peak.x) / peak.width;
    const leaned = u - peak.lean * (1 - Math.abs(u));
    if (Math.abs(leaned) >= 1) return 0;
    const body = Math.pow(1 - leaned * leaned, 1.6);
    const craggy = 1 + 0.08 * noise1(x / 38 + peak.seed) + 0.04 * noise1(x / 11 + peak.seed * 2);
    return peak.height * body * craggy;
  }

  function paintPeaks(g, originX, width, baseY, config) {
    const peaks = peaksBetween(config.seed, originX - 400, originX + width + 400, config.spacing, config.heights, config.widths);
    g.globalCompositeOperation = 'multiply';
    peaks.forEach((peak) => {
      const left = peak.x - peak.width - originX;
      const right = peak.x + peak.width - originX;
      if (right < -50 || left > width + 50) return;
      const top = baseY - peak.height * 1.1;
      const gradient = g.createLinearGradient(0, top, 0, baseY);
      gradient.addColorStop(0, `rgba(26, 21, 18, ${config.tone})`);
      gradient.addColorStop(0.45, `rgba(40, 34, 30, ${config.tone * 0.6})`);
      gradient.addColorStop(1, 'rgba(40, 34, 30, 0)');
      g.fillStyle = gradient;
      g.beginPath();
      g.moveTo(left, baseY);
      for (let x = left; x <= right; x += 4) g.lineTo(x, baseY - peakProfile(peak, x + originX));
      g.lineTo(right, baseY);
      g.closePath();
      g.fill();
      const random = seededRandom(peak.seed * 31 + config.seed);
      g.strokeStyle = `rgba(26, 21, 18, ${config.tone * 0.9})`;
      g.lineCap = 'round';
      for (let s = 0; s < config.texture; s++) {
        const sx = lerp(left + peak.width * 0.3, right - peak.width * 0.3, random());
        const ridge = baseY - peakProfile(peak, sx + originX);
        const length = 20 + random() * peak.height * 0.4;
        const slant = (sx - (peak.x - originX)) / peak.width;
        g.lineWidth = 0.8 + random() * 2;
        g.beginPath();
        g.moveTo(sx, ridge + 4 + random() * 10);
        g.quadraticCurveTo(sx + slant * 18 + (random() - 0.5) * 10, ridge + length * 0.5, sx + slant * 34, ridge + length);
        g.stroke();
      }
      g.fillStyle = `rgba(20, 16, 14, ${Math.min(0.9, config.tone * 1.6)})`;
      for (let d = 0; d < config.dots; d++) {
        const dx = lerp(left + peak.width * 0.2, right - peak.width * 0.2, random());
        const dy = baseY - peakProfile(peak, dx + originX) + random() * 14;
        fillEllipse(g, dx, dy, 2 + random() * 3.5, 1.5 + random() * 2.5, random());
      }
    });
    g.globalCompositeOperation = 'source-over';
  }

  /** Pre-rendered, pre-blurred mountain chunks for one parallax layer. */
  function buildMountainLayer(config) {
    const chunks = [];
    for (let x0 = config.from; x0 < config.to; x0 += CHUNK_WIDTH) {
      const width = CHUNK_WIDTH + CHUNK_MARGIN * 2;
      const raw = createSurface(width, config.height);
      const g = raw.getContext('2d');
      paintPeaks(g, x0 - CHUNK_MARGIN, width, config.height - 10, config);
      const chunk = createSurface(width, config.height);
      const c = chunk.getContext('2d');
      c.filter = `blur(${config.blur}px)`;
      c.drawImage(raw, 0, 0);
      c.filter = 'none';
      chunks.push({ x: x0, canvas: chunk });
    }
    return { ...config, chunks };
  }

  function drawMountainLayer(ctx, layer, viewLeft, viewRight, baseline) {
    layer.chunks.forEach((chunk) => {
      if (chunk.x + CHUNK_WIDTH < viewLeft || chunk.x > viewRight) return;
      ctx.drawImage(chunk.canvas, CHUNK_MARGIN, 0, CHUNK_WIDTH, layer.height, chunk.x, baseline - layer.height, CHUNK_WIDTH, layer.height);
    });
  }

  // -------------------------------------------------------------------- mist
  let mistSprite = null;
  function buildMistSprite() {
    const sprite = createSurface(1024, 256);
    const g = sprite.getContext('2d');
    const random = seededRandom(88);
    g.filter = 'blur(18px)';
    for (let i = 0; i < 60; i++) {
      const x = 80 + random() * 864;
      const y = 128 + (random() - 0.5) * 70;
      const rx = 60 + random() * 160;
      const ry = 18 + random() * 36;
      const edge = Math.min(1, x / 260, (1024 - x) / 260);
      g.fillStyle = `rgba(245, 239, 224, ${0.16 * edge})`;
      fillEllipse(g, x, y, rx, ry);
    }
    g.filter = 'none';
    mistSprite = sprite;
    return sprite;
  }

  function drawMist(ctx, x, y, width, height, alpha = 1) {
    if (!mistSprite) buildMistSprite();
    L.withAlpha(ctx, alpha, () => ctx.drawImage(mistSprite, x - width / 2, y - height / 2, width, height));
  }

  /** A drifting band of mist across a span, deterministic in time. */
  function mistBand(ctx, left, right, y, t, options = {}) {
    const { alpha = 0.9, speed = 12, height = 180, seed = 1, spacing = 520 } = options;
    const random = seededRandom(seed);
    const phase = (t * speed) % spacing;
    for (let x = left - spacing; x < right + spacing; x += spacing) {
      const jitter = (random() - 0.5) * spacing * 0.4;
      const bob = Math.sin(t * 0.3 + x * 0.01) * 8;
      drawMist(ctx, x + phase + jitter, y + bob + (random() - 0.5) * height * 0.3, spacing * 2.1, height * (0.8 + random() * 0.5), alpha);
    }
  }

  // ------------------------------------------------------------------ clouds
  /** Auspicious ruyi cloud: stacked spiral curls with a trailing tail. */
  function ruyiCloud(ctx, x, y, scale, options = {}) {
    const { fill = COLOR.paperLight, line = COLOR.ink, alpha = 1, curls = 3, seed = 1, tailLength = 1 } = options;
    const random = seededRandom(seed);
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    ctx.globalAlpha *= alpha;
    const centers = [];
    for (let i = 0; i < curls; i++) centers.push([i * 58 - (curls - 1) * 29, -Math.sin((i / Math.max(1, curls - 1)) * Math.PI) * 26 - random() * 8, 34 + random() * 14]);
    const tail = [[-(curls - 1) * 29 - 30, 22], [-(curls - 1) * 29 - 120 * tailLength, 30], [-(curls - 1) * 29 - 220 * tailLength, 14]];
    ctx.fillStyle = fill;
    ctx.strokeStyle = line;
    ctx.lineWidth = 3.2;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(tail[2][0], tail[2][1]);
    ctx.quadraticCurveTo(tail[1][0], tail[1][1] + 12, tail[0][0], tail[0][1] + 14);
    ctx.lineTo(centers[centers.length - 1][0] + 30, 26);
    ctx.lineTo(tail[0][0] + 10, 6);
    ctx.quadraticCurveTo(tail[1][0], tail[1][1] - 10, tail[2][0], tail[2][1]);
    ctx.fill();
    ctx.stroke();
    centers.forEach(([cx, cy, r]) => {
      ctx.beginPath();
      ctx.arc(cx, cy, r, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.beginPath();
      for (let a = 0; a <= Math.PI * 3.2; a += 0.2) {
        const rr = r * (0.82 - a / (Math.PI * 4.4));
        const px = cx + Math.cos(a + Math.PI) * rr;
        const py = cy + Math.sin(a + Math.PI) * rr;
        if (a === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.stroke();
    });
    ctx.restore();
  }

  // -------------------------------------------------------------------- sea
  /**
   * Classical fish-scale sea (haishui): rows of nested arcs that grow toward
   * the viewer, each row drifting at its own speed. A curling crest line tops it.
   */
  function chineseSea(ctx, left, right, top, bottom, t, options = {}) {
    const { wash = 'rgba(46, 60, 84, 0.16)', line = COLOR.ink, lineAlpha = 0.62, amplitude = 1, crest = true, paper = COLOR.paperLight, rowScale = 1 } = options;
    ctx.save();
    const washGradient = ctx.createLinearGradient(0, top, 0, bottom);
    washGradient.addColorStop(0, wash);
    washGradient.addColorStop(1, 'rgba(46, 60, 84, 0.04)');
    ctx.fillStyle = paper;
    ctx.fillRect(left, top, right - left, bottom - top);
    ctx.fillStyle = washGradient;
    ctx.fillRect(left, top, right - left, bottom - top);
    let y = top + 18;
    let row = 0;
    while (y < bottom + 40) {
      const depth = (y - top) / Math.max(1, bottom - top);
      const unit = (46 + depth * 70) * rowScale;
      const drift = (t * (14 + depth * 26) * (row % 2 ? 1 : -0.6)) % unit;
      const bob = Math.sin(t * 1.3 + row * 0.9) * 3 * amplitude;
      const offset = (row % 2) * unit * 0.5 + drift;
      for (let x = left - unit * 2 + (offset % unit); x < right + unit; x += unit) {
        const cy = y + bob;
        ctx.fillStyle = paper;
        ctx.beginPath();
        ctx.arc(x, cy, unit * 0.5, Math.PI, 0);
        ctx.lineTo(x + unit * 0.5, cy + unit * 0.5);
        ctx.lineTo(x - unit * 0.5, cy + unit * 0.5);
        ctx.fill();
        ctx.strokeStyle = line;
        for (let k = 0; k < 3; k++) {
          ctx.globalAlpha = lineAlpha * (1 - k * 0.22);
          ctx.lineWidth = 2.2 - k * 0.5;
          ctx.beginPath();
          ctx.arc(x, cy, unit * (0.5 - k * 0.13), Math.PI * 1.02, Math.PI * 1.98);
          ctx.stroke();
        }
        ctx.globalAlpha = 1;
      }
      y += unit * 0.34;
      row++;
    }
    if (crest) {
      ctx.strokeStyle = line;
      const crestUnit = 180;
      const drift = (t * 40) % crestUnit;
      for (let x = left - crestUnit + drift; x < right + crestUnit; x += crestUnit) {
        const cy = top + 6 + Math.sin(t * 1.1 + x * 0.01) * 5 * amplitude;
        const curl = [[x - 80, cy + 14], [x - 30, cy - 6], [x + 10, cy - 28 * amplitude], [x + 38, cy - 18 * amplitude], [x + 30, cy - 4]];
        brushStroke(ctx, catmullRom(curl, 8), 9, { alpha: 0.85, dry: 0.3, dryColor: paper, seed: Math.round(x) });
        ctx.fillStyle = COLOR.ink;
        for (let d = 0; d < 3; d++) fillCircle(ctx, x + 44 + d * 9, cy - 26 * amplitude + d * 6, 2.5 - d * 0.5);
      }
    }
    ctx.restore();
  }

  // ------------------------------------------------------------ plant life
  function olivePainting(ctx, x, groundY, scale, t, seed = 5) {
    const random = seededRandom(seed);
    ctx.save();
    ctx.translate(x, groundY);
    ctx.scale(scale, scale);
    const trunkPaths = [
      [[-30, 0], [-10, -90], [-44, -190], [-8, -300], [-30, -380]],
      [[30, 0], [20, -110], [48, -200], [16, -310], [30, -390]],
      [[0, 0], [8, -120], [-14, -240], [10, -340]],
    ];
    trunkPaths.forEach((path, i) => brushStroke(ctx, catmullRom(path, 10), 46 - i * 10, { taperStart: 0.02, taperEnd: 0.5, dry: 0.5, seed: seed + i }));
    const branches = [
      [[-30, -370], [-120, -430], [-230, -460]], [[20, -380], [120, -450], [240, -470]],
      [[0, -360], [-20, -470], [10, -560]], [[-20, -300], [-110, -330], [-170, -380]], [[26, -310], [110, -340], [180, -390]],
    ];
    branches.forEach((branch, i) => brushStroke(ctx, catmullRom(branch, 8), 18, { taperStart: 0.05, taperEnd: 0.8, dry: 0.4, seed: seed * 3 + i }));
    const sway = Math.sin(t * 0.9) * 3;
    const clusters = branches.map((branch) => branch[branch.length - 1]).concat([[-60, -420], [70, -430], [0, -500]]);
    clusters.forEach(([cx, cy]) => {
      for (let i = 0; i < 38; i++) {
        const angle = random() * Math.PI * 2;
        const r = random() * 90;
        ctx.fillStyle = `rgba(34, 40, 30, ${0.25 + random() * 0.45})`;
        fillEllipse(ctx, cx + Math.cos(angle) * r + sway, cy + Math.sin(angle) * r * 0.55, 14 + random() * 8, 4 + random() * 2, random() * Math.PI);
      }
    });
    ctx.restore();
  }

  function pineTree(ctx, x, groundY, scale, seed = 9) {
    const random = seededRandom(seed);
    ctx.save();
    ctx.translate(x, groundY);
    ctx.scale(scale, scale);
    brushStroke(ctx, catmullRom([[0, 0], [14, -120], [-10, -240], [20, -360]], 10), 26, { taperStart: 0.02, taperEnd: 0.6, dry: 0.5, seed });
    for (let level = 0; level < 5; level++) {
      const y = -120 - level * 60;
      const side = level % 2 ? 1 : -1;
      const length = 120 - level * 16;
      brushStroke(ctx, [[side * 4, y], [side * length * 0.5, y - 10], [side * length, y - 4]], 8, { taperEnd: 0.9, dry: 0.3, seed: seed + level });
      for (let n = 0; n < 7; n++) {
        const nx = side * (length * (0.35 + n * 0.1));
        const ny = y - 10 + (random() - 0.5) * 8;
        ctx.strokeStyle = `rgba(26, 21, 18, ${0.5 + random() * 0.4})`;
        ctx.lineWidth = 1.6;
        for (let k = -5; k <= 5; k++) {
          const angle = -Math.PI / 2 + k * 0.28;
          L.strokeLine(ctx, nx, ny, nx + Math.cos(angle) * 22, ny + Math.sin(angle) * 14);
        }
      }
    }
    ctx.restore();
  }

  function reeds(ctx, x, groundY, count, t, seed = 3) {
    const random = seededRandom(seed);
    ctx.save();
    for (let i = 0; i < count; i++) {
      const baseX = x + (random() - 0.5) * 160;
      const height = 90 + random() * 120;
      const sway = Math.sin(t * 1.2 + i) * 12 * (height / 200);
      brushStroke(ctx, [[baseX, groundY], [baseX + sway * 0.4, groundY - height * 0.6], [baseX + sway, groundY - height]], 5, { taperStart: 0.05, taperEnd: 0.8, alpha: 0.7, dry: 0 });
    }
    ctx.restore();
  }

  function lotusLeaf(ctx, x, y, radius, tilt, seed = 1) {
    const random = seededRandom(seed);
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(1, tilt);
    const gradient = ctx.createRadialGradient(0, 0, radius * 0.1, 0, 0, radius);
    gradient.addColorStop(0, 'rgba(40, 52, 40, 0.75)');
    gradient.addColorStop(0.7, 'rgba(46, 58, 44, 0.5)');
    gradient.addColorStop(1, 'rgba(46, 58, 44, 0.2)');
    ctx.fillStyle = gradient;
    ctx.beginPath();
    for (let i = 0; i <= 40; i++) {
      const angle = (i / 40) * Math.PI * 2;
      const r = radius * (0.92 + 0.08 * Math.sin(angle * 7 + seed) + (random() - 0.5) * 0.04);
      const px = Math.cos(angle) * r;
      const py = Math.sin(angle) * r;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
    ctx.fill();
    ctx.strokeStyle = 'rgba(240, 232, 210, 0.35)';
    ctx.lineWidth = 2 / tilt;
    for (let i = 0; i < 11; i++) {
      const angle = (i / 11) * Math.PI * 2 + seed;
      L.strokeLine(ctx, 0, 0, Math.cos(angle) * radius * 0.9, Math.sin(angle) * radius * 0.9);
    }
    ctx.restore();
  }

  function lotusFlower(ctx, x, y, size, openness = 1, seed = 1) {
    ctx.save();
    ctx.translate(x, y);
    const petals = 7;
    for (let layer = 0; layer < 2; layer++) {
      for (let i = 0; i < petals; i++) {
        const spread = (i / (petals - 1) - 0.5) * (1.6 + layer * 0.5) * openness;
        const length = size * (1 - layer * 0.25);
        ctx.save();
        ctx.rotate(spread);
        const gradient = ctx.createLinearGradient(0, 0, 0, -length);
        gradient.addColorStop(0, 'rgba(246, 236, 226, 0.95)');
        gradient.addColorStop(0.7, 'rgba(226, 168, 170, 0.8)');
        gradient.addColorStop(1, 'rgba(196, 110, 118, 0.9)');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.moveTo(0, 0);
        ctx.quadraticCurveTo(-length * 0.32, -length * 0.55, 0, -length);
        ctx.quadraticCurveTo(length * 0.32, -length * 0.55, 0, 0);
        ctx.fill();
        ctx.strokeStyle = 'rgba(80, 40, 40, 0.55)';
        ctx.lineWidth = 1.4;
        ctx.stroke();
        ctx.restore();
      }
    }
    ctx.fillStyle = 'rgba(200, 160, 60, 0.9)';
    fillEllipse(ctx, 0, -size * 0.15, size * 0.18, size * 0.1);
    ctx.restore();
    void seed;
  }

  // ------------------------------------------------------------------- rock
  function rockMass(ctx, points, options = {}) {
    const { tone = 0.8, seed = 1, texture = 24, fill = null } = options;
    const random = seededRandom(seed);
    const outline = catmullRom(points, 10, true);
    let minY = Infinity;
    let maxY = -Infinity;
    outline.forEach(([, y]) => {
      minY = Math.min(minY, y);
      maxY = Math.max(maxY, y);
    });
    ctx.save();
    polygonPath(ctx, outline);
    const gradient = ctx.createLinearGradient(0, minY, 0, maxY);
    gradient.addColorStop(0, fill || `rgba(30, 25, 22, ${tone * 0.9})`);
    gradient.addColorStop(1, `rgba(60, 50, 44, ${tone * 0.2})`);
    ctx.fillStyle = gradient;
    ctx.fill();
    ctx.clip();
    for (let i = 0; i < texture; i++) {
      const p = outline[Math.floor(random() * outline.length)];
      const length = 30 + random() * 90;
      brushStroke(ctx, [[p[0], p[1]], [p[0] + (random() - 0.5) * 30, p[1] + length * 0.5], [p[0] + (random() - 0.5) * 50, p[1] + length]], 3 + random() * 6, { alpha: 0.5, dry: 0.3, seed: seed + i });
    }
    ctx.restore();
    brushStroke(ctx, outline.slice(0, Math.floor(outline.length * 0.55)), 6, { alpha: 0.85, dry: 0.4, seed: seed * 7, taperStart: 0.05, taperEnd: 0.3 });
  }

  // ------------------------------------------------------------- weather
  function rain(ctx, left, right, top, bottom, t, alpha = 0.5, seed = 12) {
    const random = seededRandom(seed);
    ctx.save();
    ctx.strokeStyle = `rgba(30, 26, 24, ${alpha})`;
    ctx.lineWidth = 1.4;
    ctx.lineCap = 'round';
    const count = Math.round((right - left) / 14);
    for (let i = 0; i < count; i++) {
      const x0 = left + random() * (right - left + 300);
      const speed = 900 + random() * 500;
      const span = bottom - top + 200;
      const y = top - 100 + ((random() * span + t * speed) % span);
      const x = x0 - (y - top) * 0.35;
      L.strokeLine(ctx, x, y, x - 14, y + 42);
    }
    ctx.restore();
  }

  function lightningBolt(ctx, from, to, seed, alpha) {
    if (alpha <= 0.01) return;
    const random = seededRandom(seed);
    const points = [from];
    const segments = 14;
    for (let i = 1; i < segments; i++) {
      const p = L.lerpPoint(from, to, i / segments);
      points.push([p[0] + (random() - 0.5) * 90, p[1] + (random() - 0.5) * 30]);
    }
    points.push(to);
    ctx.save();
    ctx.globalCompositeOperation = 'screen';
    ctx.strokeStyle = `rgba(255, 226, 150, ${alpha})`;
    ctx.shadowColor = 'rgba(255, 200, 100, 1)';
    ctx.shadowBlur = 30;
    ctx.lineWidth = 5;
    ctx.lineJoin = 'round';
    strokePolyline(ctx, points);
    ctx.lineWidth = 2;
    ctx.strokeStyle = `rgba(255, 250, 230, ${alpha})`;
    strokePolyline(ctx, points);
    const branchAt = points[5];
    strokePolyline(ctx, [branchAt, [branchAt[0] + 120, branchAt[1] + 80], [branchAt[0] + 160, branchAt[1] + 190]]);
    ctx.restore();
  }

  function moon(ctx, x, y, radius, alpha = 1) {
    drawGlow(ctx, x, y, radius * 5, RGB.moon, 0.35 * alpha);
    ctx.save();
    ctx.globalAlpha *= alpha;
    const gradient = ctx.createRadialGradient(x - radius * 0.3, y - radius * 0.3, radius * 0.1, x, y, radius);
    gradient.addColorStop(0, 'rgba(250, 247, 235, 1)');
    gradient.addColorStop(1, 'rgba(226, 218, 196, 1)');
    ctx.fillStyle = gradient;
    fillCircle(ctx, x, y, radius);
    ctx.fillStyle = 'rgba(150, 140, 120, 0.15)';
    fillCircle(ctx, x + radius * 0.25, y - radius * 0.1, radius * 0.22);
    fillCircle(ctx, x - radius * 0.3, y + radius * 0.3, radius * 0.14);
    ctx.restore();
  }

  function sunDisc(ctx, x, y, radius, alpha = 1) {
    drawGlow(ctx, x, y, radius * 6, RGB.gold, 0.45 * alpha);
    ctx.save();
    ctx.globalAlpha *= alpha;
    ctx.fillStyle = 'rgba(236, 196, 120, 0.95)';
    fillCircle(ctx, x, y, radius);
    ctx.restore();
  }

  // ------------------------------------------------------------ the dragon
  /**
   * The storm dragon: an ink body along a travelling sine, scaled, finned,
   * clawed and whiskered, with a gold eye. `phase` animates the undulation.
   */
  function stormDragon(ctx, spine, t, options = {}) {
    const { width = 90, alpha = 1, eyeGlow = 1 } = options;
    if (spine.length < 4) return;
    const lengths = polylineLengths(spine);
    ctx.save();
    ctx.globalAlpha *= alpha;
    ctx.filter = 'blur(26px)';
    ctx.strokeStyle = 'rgba(20, 18, 20, 0.5)';
    ctx.lineWidth = width * 2.6;
    ctx.lineCap = 'round';
    strokePolyline(ctx, spine);
    ctx.filter = 'none';
    // dorsal fins
    for (let i = 4; i < spine.length - 6; i += 3) {
      const { point, angle } = pointOnPath(spine, lengths[i] / lengths[lengths.length - 1], lengths);
      const u = i / spine.length;
      const half = width * 0.5 * (0.35 + 0.65 * Math.sin(Math.PI * Math.min(1, u * 1.3)));
      const nx = Math.cos(angle - Math.PI / 2);
      const ny = Math.sin(angle - Math.PI / 2);
      const tip = [point[0] + nx * (half + 34 + 10 * Math.sin(i + t * 6)), point[1] + ny * (half + 34)];
      ctx.fillStyle = 'rgba(22, 18, 16, 0.9)';
      polygonPath(ctx, [[point[0] + nx * half - Math.cos(angle) * 12, point[1] + ny * half - Math.sin(angle) * 12], tip, [point[0] + nx * half + Math.cos(angle) * 14, point[1] + ny * half + Math.sin(angle) * 14]]);
      ctx.fill();
    }
    brushStroke(ctx, spine, width, { taperStart: 0.04, taperEnd: 0.55, dry: 0.35, dryColor: 'rgba(200, 190, 170, 1)', wobble: 0.08, seed: 71 });
    // scales
    ctx.strokeStyle = 'rgba(220, 205, 175, 0.4)';
    ctx.lineWidth = 1.6;
    for (let i = 3; i < spine.length - 4; i++) {
      const u = i / spine.length;
      const { point, angle } = pointOnPath(spine, lengths[i] / lengths[lengths.length - 1], lengths);
      const half = width * 0.5 * (1 - Math.max(0, u - 0.45) / 0.55) * (u < 0.04 ? u / 0.04 : 1);
      for (let row = -1; row <= 1; row++) {
        const cx = point[0] + Math.cos(angle + Math.PI / 2) * row * half * 0.5;
        const cy = point[1] + Math.sin(angle + Math.PI / 2) * row * half * 0.5;
        ctx.beginPath();
        ctx.arc(cx, cy, half * 0.32, angle + Math.PI * 0.6, angle + Math.PI * 1.4);
        ctx.stroke();
      }
    }
    // claws
    [0.2, 0.32, 0.55, 0.68].forEach((u, k) => {
      const { point, angle } = pointOnPath(spine, u, lengths);
      const side = k % 2 ? 1 : -1;
      const reach = width * 1.1;
      const knee = [point[0] + Math.cos(angle + side * 1.9) * reach * 0.6, point[1] + Math.sin(angle + side * 1.9) * reach * 0.6];
      const foot = [knee[0] + Math.cos(angle + side * 2.6) * reach * 0.5, knee[1] + Math.sin(angle + side * 2.6) * reach * 0.5];
      brushStroke(ctx, [point, knee, foot], width * 0.28, { taperStart: 0.05, taperEnd: 0.4, dry: 0.2, seed: 90 + k });
      for (let c = -1; c <= 1; c++) {
        const clawAngle = angle + side * 2.6 + c * 0.45;
        brushStroke(ctx, [foot, [foot[0] + Math.cos(clawAngle) * 26, foot[1] + Math.sin(clawAngle) * 26], [foot[0] + Math.cos(clawAngle + 0.5) * 40, foot[1] + Math.sin(clawAngle + 0.5) * 40]], 7, { taperEnd: 0.9, dry: 0, seed: 100 + k * 3 + c });
      }
    });
    // head at spine[0]
    const head = spine[0];
    const { angle: headAngle } = pointOnPath(spine, 0.02, lengths);
    const forward = headAngle + Math.PI;
    ctx.save();
    ctx.translate(head[0], head[1]);
    ctx.rotate(forward);
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(-width * 0.4, -width * 0.55);
    ctx.quadraticCurveTo(width * 0.6, -width * 0.7, width * 1.25, -width * 0.2);
    ctx.lineTo(width * 1.3, width * 0.05);
    ctx.quadraticCurveTo(width * 0.8, width * 0.1, width * 0.7, width * 0.3);
    ctx.lineTo(width * 1.1, width * 0.45);
    ctx.quadraticCurveTo(width * 0.4, width * 0.7, -width * 0.4, width * 0.55);
    ctx.closePath();
    ctx.fill();
    brushStroke(ctx, [[0, -width * 0.5], [-width * 0.5, -width * 1.1], [-width * 1.2, -width * 1.4]], 12, { taperEnd: 0.9, dry: 0.3, seed: 120 });
    brushStroke(ctx, [[width * 0.2, -width * 0.55], [-width * 0.2, -width * 1.2], [-width * 0.8, -width * 1.7]], 10, { taperEnd: 0.9, dry: 0.3, seed: 121 });
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 3;
    for (let w = 0; w < 2; w++) {
      ctx.beginPath();
      ctx.moveTo(width * 1.1, width * 0.1 * (w ? 1 : -1));
      for (let s = 1; s <= 12; s++) ctx.lineTo(width * 1.1 - s * 26, width * (0.2 + w * 0.3) + Math.sin(t * 5 + s * 0.6 + w) * 24 + s * 6);
      ctx.stroke();
    }
    ctx.fillStyle = 'rgba(255, 230, 190, 0.95)';
    fillCircle(ctx, width * 0.62, width * 0.34, width * 0.12);
    ctx.fillStyle = 'rgba(240, 236, 220, 0.9)';
    for (let tooth = 0; tooth < 4; tooth++) L.fillPolygon(ctx, [[width * (0.75 + tooth * 0.1), width * 0.1], [width * (0.8 + tooth * 0.1), width * 0.24], [width * (0.85 + tooth * 0.1), width * 0.1]]);
    ctx.restore();
    const eye = [head[0] + Math.cos(forward) * width * 0.35 - Math.sin(forward) * width * 0.2, head[1] + Math.sin(forward) * width * 0.35 + Math.cos(forward) * width * 0.2];
    drawGlow(ctx, eye[0], eye[1], width * 1.4, RGB.gold, eyeGlow);
    ctx.fillStyle = COLOR.gold;
    fillCircle(ctx, eye[0], eye[1], width * 0.11);
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, eye[0], eye[1], width * 0.03, width * 0.09, forward);
    ctx.restore();
  }

  // ---------------------------------------------------------- ink splashes
  /** Ink blooming into wet paper: a soft core, feathered edge, a few runs and droplets. */
  function inkSplash(ctx, x, y, radius, bloom, seed = 1, alpha = 1) {
    if (bloom <= 0) return;
    const random = seededRandom(seed);
    const grow = easeOut(clamp(bloom));
    ctx.save();
    ctx.globalAlpha *= alpha;
    const core = ctx.createRadialGradient(x, y, 0, x, y, radius * 0.75 * grow);
    core.addColorStop(0, 'rgba(22, 18, 16, 0.95)');
    core.addColorStop(0.55, 'rgba(26, 21, 18, 0.8)');
    core.addColorStop(0.85, 'rgba(40, 34, 30, 0.3)');
    core.addColorStop(1, 'rgba(40, 34, 30, 0)');
    ctx.fillStyle = core;
    ctx.filter = 'blur(2.5px)';
    ctx.beginPath();
    for (let i = 0; i <= 90; i++) {
      const angle = (i / 90) * Math.PI * 2;
      const r = radius * 0.75 * grow * (0.82 + 0.18 * L.noise1(angle * 2.3 + seed));
      const px = x + Math.cos(angle) * r;
      const py = y + Math.sin(angle) * r * 0.9;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.closePath();
    ctx.fill();
    ctx.filter = 'blur(3px)';
    for (let i = 0; i < 7; i++) {
      const angle = random() * Math.PI * 2;
      const reach = radius * (0.55 + random() * 0.35) * grow;
      const lobe = ctx.createRadialGradient(x + Math.cos(angle) * reach, y + Math.sin(angle) * reach, 0, x + Math.cos(angle) * reach, y + Math.sin(angle) * reach, radius * 0.3 * grow);
      lobe.addColorStop(0, 'rgba(26, 21, 18, 0.7)');
      lobe.addColorStop(1, 'rgba(26, 21, 18, 0)');
      ctx.fillStyle = lobe;
      fillEllipse(ctx, x + Math.cos(angle) * reach, y + Math.sin(angle) * reach, radius * 0.3 * grow, radius * 0.2 * grow, angle);
    }
    ctx.filter = 'none';
    ctx.fillStyle = 'rgba(22, 18, 16, 0.9)';
    for (let i = 0; i < 14; i++) {
      const angle = random() * Math.PI * 2;
      const distance = radius * (0.9 + random() * 0.6) * grow;
      fillCircle(ctx, x + Math.cos(angle) * distance, y + Math.sin(angle) * distance * 0.9, radius * (0.012 + random() * 0.03));
    }
    ctx.restore();
  }

  // ----------------------------------------------------------- calligraphy
  /** One character, brushed in with an ink bleed (blur → sharp, pale → black). */
  function brushCharacter(ctx, character, x, y, size, reveal, options = {}) {
    if (reveal <= 0) return;
    const { font = L.FONT.brush, color = COLOR.ink, alpha = 1 } = options;
    const bleed = 1 - clamp(reveal);
    ctx.save();
    ctx.font = `${size}px ${font}`;
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.globalAlpha *= alpha * clamp(reveal * 1.6);
    if (bleed > 0.02) ctx.filter = `blur(${(bleed * size * 0.08).toFixed(2)}px)`;
    ctx.fillStyle = color;
    const clipHeight = size * 1.3 * smooth(clamp(reveal * 1.2));
    ctx.beginPath();
    ctx.rect(x - size, y - size * 0.65, size * 2, clipHeight);
    ctx.clip();
    ctx.fillText(character, x, y);
    ctx.restore();
  }

  function verticalCalligraphy(ctx, text, x, top, size, t, start, perCharacter, options = {}) {
    [...text].forEach((character, index) => {
      const reveal = progress(t, start + index * perCharacter, start + index * perCharacter + perCharacter * 1.4);
      brushCharacter(ctx, character, x, top + index * size * 1.08, size, reveal, options);
    });
  }

  /** A carved cinnabar seal impression; `stamp` 0→1 animates the press. */
  function seal(ctx, x, y, size, text, stamp = 1, options = {}) {
    if (stamp <= 0) return;
    const { font = L.FONT.brush, weight = 400, style = 'yin', seed = 3, rounded = 0.12, textSize = null, columns = null, alpha = 1 } = options;
    const press = easeOut(clamp(stamp / 0.35));
    const settle = clamp((stamp - 0.35) / 0.65);
    const scale = lerp(1.35, 1, press);
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    ctx.globalAlpha *= alpha * lerp(0.2, 1, press);
    const half = size / 2;
    const random = seededRandom(seed);
    ctx.fillStyle = COLOR.cinnabar;
    ctx.strokeStyle = COLOR.cinnabar;
    if (style === 'yin') {
      L.polygonPath(ctx, [[-half, -half], [half, -half], [half, half], [-half, half]]);
      ctx.beginPath();
      ctx.roundRect(-half, -half, size, size, size * rounded);
      ctx.fill();
      ctx.fillStyle = COLOR.paperLight;
    } else {
      ctx.lineWidth = size * 0.07;
      ctx.beginPath();
      ctx.roundRect(-half + size * 0.04, -half + size * 0.04, size * 0.92, size * 0.92, size * rounded);
      ctx.stroke();
    }
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    const latin = /^[\x20-\x7E]+$/.test(text);
    const glyphs = latin ? [text] : [...text];
    const layout = latin ? 1 : columns || (glyphs.length <= 1 ? 1 : 2);
    const rows = Math.ceil(glyphs.length / layout);
    const cell = size * 0.86 / Math.max(layout, rows);
    let fontSize = textSize || cell * 0.95;
    ctx.font = `${weight} ${fontSize}px ${font}`;
    if (latin) {
      const fit = (size * 0.8) / Math.max(1, ctx.measureText(text).width);
      if (fit < 1) fontSize *= fit;
      ctx.font = `${weight} ${fontSize}px ${font}`;
    }
    glyphs.forEach((glyph, index) => {
      const column = layout - 1 - Math.floor(index / rows);
      const row = index % rows;
      const gx = (column - (layout - 1) / 2) * cell;
      const gy = (row - (rows - 1) / 2) * cell;
      ctx.fillText(glyph, gx, gy + cell * 0.04);
    });
    ctx.globalCompositeOperation = 'destination-out';
    for (let i = 0; i < 40; i++) {
      ctx.fillStyle = `rgba(0, 0, 0, ${0.2 + random() * 0.5})`;
      fillCircle(ctx, (random() - 0.5) * size, (random() - 0.5) * size, 0.6 + random() * size * 0.02);
    }
    ctx.restore();
    if (settle > 0 && settle < 1) {
      ctx.save();
      ctx.strokeStyle = `rgba(183, 38, 28, ${0.35 * (1 - settle)})`;
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.roundRect(x - half * (1 + settle * 0.5), y - half * (1 + settle * 0.5), size * (1 + settle * 0.5), size * (1 + settle * 0.5), size * 0.14);
      ctx.stroke();
      ctx.restore();
    }
  }

  Object.assign(L, {
    buildPaperTile, brushStroke, inkDot, buildMountainLayer, drawMountainLayer, CHUNK_WIDTH,
    drawMist, mistBand, ruyiCloud, chineseSea, olivePainting, pineTree, reeds, lotusLeaf, lotusFlower, rockMass,
    rain, lightningBolt, moon, sunDisc, stormDragon, inkSplash, brushCharacter, verticalCalligraphy, seal,
  });
  void fbm1;
})();
