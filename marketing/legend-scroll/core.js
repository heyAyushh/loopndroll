/* Legend of the Looper — shared constants, maths, noise and canvas helpers. */
(function () {
  'use strict';

  const WIDTH = 1920;
  const HEIGHT = 1080;
  const DURATION_SECONDS = 140;
  const PAINT_HEIGHT = 1080;

  // Palette discipline: ink on xuan paper. Cinnabar belongs only to the red
  // thread and seals; jade only to work that is running; gold is light.
  const COLOR = {
    paper: '#E8DDC4',
    paperDeep: '#D9CAA8',
    paperLight: '#F3EBD8',
    ink: '#1A1512',
    inkWarm: '#2A211B',
    cinnabar: '#B7261C',
    thread: '#C9281F',
    jade: '#3FB47D',
    jadeGlow: '#7BE3AE',
    gold: '#E2B35A',
    goldDeep: '#A8792C',
    candle: '#FFC777',
    lacquer: '#1A0B07',
    indigo: 'rgba(38, 52, 92, 0.82)',
    ochre: 'rgba(176, 116, 44, 0.8)',
    amber: 'rgba(214, 142, 52, 0.72)',
    umber: 'rgba(92, 54, 30, 0.86)',
    rose: 'rgba(206, 134, 136, 0.55)',
  };

  const FONT = {
    brush: 'MaShanZheng, "Hiragino Mincho ProN", serif',
    running: 'ZhiMangXing, MaShanZheng, serif',
    serif: 'Cormorant, "Cormorant Garamond", Didot, serif',
    ui: '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif',
    mono: '"SF Mono", Menlo, monospace',
  };

  const clamp = (value, low = 0, high = 1) => Math.min(high, Math.max(low, value));
  const lerp = (from, to, amount) => from + (to - from) * amount;
  const progress = (t, start, end) => (end === start ? (t >= end ? 1 : 0) : clamp((t - start) / (end - start)));
  const smooth = (p) => p * p * (3 - 2 * p);
  const easeInOut = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
  const easeOut = (p) => 1 - Math.pow(1 - p, 3);
  const easeIn = (p) => p * p * p;
  const easeOutBack = (p) => 1 + 2.7 * Math.pow(p - 1, 3) + 1.7 * Math.pow(p - 1, 2);
  const easeOutElastic = (p) => (p <= 0 ? 0 : p >= 1 ? 1 : Math.pow(2, -10 * p) * Math.sin((p * 10 - 0.75) * ((2 * Math.PI) / 3)) + 1);
  const pulseAt = (t, at, width) => Math.exp(-Math.pow((t - at) / width, 2));
  const lerpPoint = (a, b, p) => [lerp(a[0], b[0], p), lerp(a[1], b[1], p)];
  const distance = (a, b) => Math.hypot(a[0] - b[0], a[1] - b[1]);

  function seededRandom(seed) {
    let state = seed >>> 0;
    return function next() {
      state = (state + 0x6d2b79f5) >>> 0;
      let r = Math.imul(state ^ (state >>> 15), 1 | state);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }

  // Deterministic value noise: smooth, repeatable, cheap.
  function hash1(n) {
    const x = Math.sin(n * 127.1 + 311.7) * 43758.5453123;
    return x - Math.floor(x);
  }
  function noise1(x) {
    const i = Math.floor(x);
    const f = x - i;
    return lerp(hash1(i), hash1(i + 1), smooth(f)) * 2 - 1;
  }
  function hash2(x, y) {
    const v = Math.sin(x * 127.1 + y * 311.7) * 43758.5453123;
    return v - Math.floor(v);
  }
  function noise2(x, y) {
    const ix = Math.floor(x);
    const iy = Math.floor(y);
    const fx = smooth(x - ix);
    const fy = smooth(y - iy);
    const top = lerp(hash2(ix, iy), hash2(ix + 1, iy), fx);
    const bottom = lerp(hash2(ix, iy + 1), hash2(ix + 1, iy + 1), fx);
    return lerp(top, bottom, fy) * 2 - 1;
  }
  function fbm1(x, octaves = 4) {
    let value = 0;
    let amplitude = 0.5;
    let frequency = 1;
    for (let i = 0; i < octaves; i++) {
      value += amplitude * noise1(x * frequency + i * 17.3);
      amplitude *= 0.5;
      frequency *= 2.03;
    }
    return value;
  }

  function createSurface(width, height) {
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(width));
    canvas.height = Math.max(1, Math.round(height));
    return canvas;
  }

  function fillCircle(ctx, x, y, radius) {
    ctx.beginPath();
    ctx.arc(x, y, Math.max(0, radius), 0, Math.PI * 2);
    ctx.fill();
  }

  function fillEllipse(ctx, x, y, radiusX, radiusY, rotation = 0) {
    ctx.beginPath();
    ctx.ellipse(x, y, Math.max(0, radiusX), Math.max(0, radiusY), rotation, 0, Math.PI * 2);
    ctx.fill();
  }

  function polygonPath(ctx, points) {
    ctx.beginPath();
    points.forEach(([x, y], index) => (index ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.closePath();
  }

  function fillPolygon(ctx, points) {
    polygonPath(ctx, points);
    ctx.fill();
  }

  function strokeLine(ctx, x1, y1, x2, y2) {
    ctx.beginPath();
    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.stroke();
  }

  function strokePolyline(ctx, points) {
    if (points.length < 2) return;
    ctx.beginPath();
    ctx.moveTo(points[0][0], points[0][1]);
    for (let i = 1; i < points.length; i++) ctx.lineTo(points[i][0], points[i][1]);
    ctx.stroke();
  }

  function catmullRom(knots, samplesPerSpan = 16, closed = false) {
    if (knots.length < 2) return knots.slice();
    const points = [];
    const count = knots.length;
    const get = (i) => (closed ? knots[(i + count) % count] : knots[clamp(i, 0, count - 1)]);
    const spans = closed ? count : count - 1;
    for (let i = 0; i < spans; i++) {
      const p0 = get(i - 1);
      const p1 = get(i);
      const p2 = get(i + 1);
      const p3 = get(i + 2);
      for (let s = 0; s < samplesPerSpan; s++) {
        const u = s / samplesPerSpan;
        const u2 = u * u;
        const u3 = u2 * u;
        points.push([0, 1].map((axis) =>
          0.5 * (2 * p1[axis] + (-p0[axis] + p2[axis]) * u + (2 * p0[axis] - 5 * p1[axis] + 4 * p2[axis] - p3[axis]) * u2 + (-p0[axis] + 3 * p1[axis] - 3 * p2[axis] + p3[axis]) * u3)));
      }
    }
    points.push(closed ? knots[0] : knots[count - 1]);
    return points;
  }

  function quadraticPoints(p0, p1, sag, samples = 48) {
    const control = [(p0[0] + p1[0]) / 2, (p0[1] + p1[1]) / 2 + sag];
    const points = [];
    for (let i = 0; i <= samples; i++) {
      const u = i / samples;
      points.push([
        (1 - u) * (1 - u) * p0[0] + 2 * (1 - u) * u * control[0] + u * u * p1[0],
        (1 - u) * (1 - u) * p0[1] + 2 * (1 - u) * u * control[1] + u * u * p1[1],
      ]);
    }
    return points;
  }

  function polylineLengths(points) {
    const lengths = [0];
    for (let i = 1; i < points.length; i++) lengths.push(lengths[i - 1] + distance(points[i - 1], points[i]));
    return lengths;
  }

  /** Point and tangent angle at a 0..1 fraction of arc length. */
  function pointOnPath(points, fraction, lengths = polylineLengths(points)) {
    const total = lengths[lengths.length - 1] || 1;
    const target = clamp(fraction) * total;
    let index = 1;
    while (index < lengths.length - 1 && lengths[index] < target) index++;
    const span = lengths[index] - lengths[index - 1] || 1;
    const local = (target - lengths[index - 1]) / span;
    const a = points[index - 1];
    const b = points[index];
    return { point: lerpPoint(a, b, local), angle: Math.atan2(b[1] - a[1], b[0] - a[0]) };
  }

  function slicePath(points, fraction) {
    if (fraction >= 1) return points;
    const lengths = polylineLengths(points);
    const total = lengths[lengths.length - 1];
    const target = clamp(fraction) * total;
    const out = [points[0]];
    for (let i = 1; i < points.length; i++) {
      if (lengths[i] <= target) out.push(points[i]);
      else {
        const span = lengths[i] - lengths[i - 1] || 1;
        out.push(lerpPoint(points[i - 1], points[i], (target - lengths[i - 1]) / span));
        break;
      }
    }
    return out;
  }

  function withAlpha(ctx, alpha, paint) {
    if (alpha <= 0.001) return;
    const previous = ctx.globalAlpha;
    ctx.globalAlpha = previous * clamp(alpha);
    paint();
    ctx.globalAlpha = previous;
  }

  /** Cached radial glow sprite (white core → transparent), tinted at draw time. */
  const glowCache = new Map();
  function glowSprite(rgb) {
    if (glowCache.has(rgb)) return glowCache.get(rgb);
    const size = 256;
    const sprite = createSurface(size, size);
    const g = sprite.getContext('2d');
    const gradient = g.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
    gradient.addColorStop(0, `rgba(${rgb}, 1)`);
    gradient.addColorStop(0.25, `rgba(${rgb}, 0.55)`);
    gradient.addColorStop(0.6, `rgba(${rgb}, 0.14)`);
    gradient.addColorStop(1, `rgba(${rgb}, 0)`);
    g.fillStyle = gradient;
    g.fillRect(0, 0, size, size);
    glowCache.set(rgb, sprite);
    return sprite;
  }

  function drawGlow(ctx, x, y, radius, rgb, alpha = 1, mode = 'screen') {
    if (alpha <= 0.001 || radius <= 0) return;
    ctx.save();
    ctx.globalCompositeOperation = mode;
    ctx.globalAlpha *= clamp(alpha);
    ctx.drawImage(glowSprite(rgb), x - radius, y - radius, radius * 2, radius * 2);
    ctx.restore();
  }

  const RGB = {
    jade: '95, 211, 154',
    gold: '226, 179, 90',
    candle: '255, 190, 110',
    thread: '220, 50, 40',
    paper: '243, 235, 216',
    moon: '236, 232, 214',
    ink: '26, 21, 18',
  };

  window.Legend = window.Legend || {};
  Object.assign(window.Legend, {
    WIDTH, HEIGHT, DURATION_SECONDS, PAINT_HEIGHT, COLOR, FONT, RGB,
    clamp, lerp, progress, smooth, easeInOut, easeOut, easeIn, easeOutBack, easeOutElastic, pulseAt, lerpPoint, distance,
    seededRandom, noise1, noise2, fbm1,
    createSurface, fillCircle, fillEllipse, polygonPath, fillPolygon, strokeLine, strokePolyline,
    catmullRom, quadraticPoints, polylineLengths, pointOnPath, slicePath, withAlpha, glowSprite, drawGlow,
  });
})();
