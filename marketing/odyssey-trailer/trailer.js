/* Looper: The Odyssey — a 60-second trailer painted as black-figure pottery.
 *
 * Every frame is a pure function of time, so the live player (player.js) and
 * the frame-exact exporter (render.mjs) paint identical pictures.
 *
 * Visual rules:
 *   - Each scene is a panel painted on the curved belly of an amphora; cuts are
 *     the vase turning (a cylindrical warp between two panels).
 *   - Figures move on a 12 fps "shadow puppet" clock.
 *   - Only two colours exist outside clay, glaze and bone: thread-red (the
 *     connection between phone and desk) and running-green (work in progress).
 *     They are also the only things that move smoothly — they never stop.
 */
(function () {
  'use strict';

  // ------------------------------------------------------------------ canvas
  const WIDTH = 1920;
  const HEIGHT = 1080;
  const DURATION_SECONDS = 60;
  const PUPPET_FPS = 12;
  const GRAIN_FPS = 24;
  const GRAIN_FRAME_COUNT = 6;

  // Visible arc of the vase surface across the frame; larger bends the edges more.
  const CYLINDER_HALF_ANGLE = 0.62;
  const WARP_SLICE_WIDTH = 2;

  const BORDER_HEIGHT = 58;
  const GROUND_Y = 872;
  const VASE_BAND_WIDTH = 640;
  const VASE_BAND_HEIGHT = (VASE_BAND_WIDTH * HEIGHT) / WIDTH;
  const FULL_ZOOM = WIDTH / VASE_BAND_WIDTH;

  const COLOR = {
    terracotta: '#C8632B',
    terracottaShadow: '#A9501F',
    terracottaNight: '#6E2F12',
    ink: '#141010',
    bone: '#EDE3D0',
    groove: '#4A1F0B',
    grooveShadow: '#2A1105',
    thread: '#D81E2C',
    running: '#34C759',
    screen: '#0E0C0C',
  };

  const FONT = {
    carved: '"Gill Sans", "Gill Sans MT", "Avenir Next", sans-serif',
    script: 'Cochin, Palatino, serif',
    ui: '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif',
    mono: '"SF Mono", Menlo, monospace',
  };

  // Black-figure convention: men are glaze-black; women's skin is added white.
  // Night scenes invert to red-figure: terracotta bodies on black glaze.
  const LOOK = {
    black: { fill: COLOR.ink, line: COLOR.bone, skin: COLOR.ink, skinLine: COLOR.bone },
    woman: { fill: COLOR.ink, line: COLOR.bone, skin: COLOR.bone, skinLine: COLOR.ink },
    red: { fill: COLOR.terracotta, line: COLOR.ink, skin: COLOR.terracotta, skinLine: COLOR.ink },
  };

  const SESSION_NAMES = ['CODEX', 'CLAUDE', 'ZED', 'GROK'];

  // The hero shot: an arrow through twelve axe-rings, one ring per shipped session.
  const ARROW = {
    releaseLocal: 0.6,
    startX: 380,
    speed: 700,
    ringStartX: 900,
    ringSpacing: 270,
    ringCount: 12,
    holeY: GROUND_Y - 330,
  };

  const PROTEUS_HITS_LOCAL = [0.45, 1.3, 2.15, 3.0];
  const STORM_SNAP_LOCAL = 1.0;
  const NIGHT = { heartbeatStart: 0.2, heartbeatPeriod: 0.82, grab: 4.1, wait: 4.3, healStart: 5.0, healEnd: 6.2, release: 6.3 };
  const CONTINUE_ARRIVAL_LOCAL = 2.4;

  // ------------------------------------------------------------------- maths
  const clamp = (value, low = 0, high = 1) => Math.min(high, Math.max(low, value));
  const lerp = (from, to, amount) => from + (to - from) * amount;
  const progress = (t, start, end) => clamp((t - start) / (end - start));
  const easeInOut = (p) => (p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2);
  const easeOut = (p) => 1 - Math.pow(1 - p, 3);
  const easeIn = (p) => p * p * p;
  const easeOutBack = (p) => {
    const overshoot = 1.7;
    return 1 + (overshoot + 1) * Math.pow(p - 1, 3) + overshoot * Math.pow(p - 1, 2);
  };
  const stepped = (t) => Math.floor(t * PUPPET_FPS + 1e-6) / PUPPET_FPS;
  const zoomTween = (from, to, p) => from * Math.pow(to / from, p);
  const lerpPoint = (a, b, p) => [lerp(a[0], b[0], p), lerp(a[1], b[1], p)];

  function seededRandom(seed) {
    let state = seed >>> 0;
    return function next() {
      state = (state + 0x6d2b79f5) >>> 0;
      let r = Math.imul(state ^ (state >>> 15), 1 | state);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }

  // ------------------------------------------------------------ primitives
  function createSurface(width, height) {
    const canvas = document.createElement('canvas');
    canvas.width = width;
    canvas.height = height;
    return canvas;
  }

  function fillCircle(ctx, x, y, radius) {
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fill();
  }

  function fillEllipse(ctx, x, y, radiusX, radiusY, rotation = 0) {
    ctx.beginPath();
    ctx.ellipse(x, y, radiusX, radiusY, rotation, 0, Math.PI * 2);
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

  function roundRectPath(ctx, x, y, width, height, radius) {
    ctx.beginPath();
    ctx.roundRect(x, y, width, height, radius);
  }

  function taperedStroke(ctx, ax, ay, bx, by, radiusA, radiusB) {
    const steps = 4;
    ctx.lineCap = 'round';
    for (let i = 0; i < steps; i++) {
      const p0 = i / steps;
      const p1 = (i + 1) / steps;
      ctx.lineWidth = 2 * lerp(radiusA, radiusB, (p0 + p1) / 2);
      strokeLine(ctx, lerp(ax, bx, p0), lerp(ay, by, p0), lerp(ax, bx, p1), lerp(ay, by, p1));
    }
  }

  function taperedPath(ctx, points, radiusStart, radiusEnd) {
    ctx.lineCap = 'round';
    for (let i = 0; i < points.length - 1; i++) {
      ctx.lineWidth = 2 * lerp(radiusStart, radiusEnd, (i + 0.5) / (points.length - 1));
      strokeLine(ctx, points[i][0], points[i][1], points[i + 1][0], points[i + 1][1]);
    }
  }

  function strokeSpiral(ctx, cx, cy, radius, turns, startAngle = 0, direction = 1) {
    ctx.beginPath();
    const segments = Math.ceil(turns * 28);
    for (let i = 0; i <= segments; i++) {
      const p = i / segments;
      const angle = startAngle + direction * p * turns * Math.PI * 2;
      const r = radius * (1 - p * 0.92);
      const x = cx + Math.cos(angle) * r;
      const y = cy + Math.sin(angle) * r;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }

  function quadraticPoints(p0, p1, sag, samples = 48) {
    const control = [(p0[0] + p1[0]) / 2, (p0[1] + p1[1]) / 2 + sag];
    const points = [];
    for (let i = 0; i <= samples; i++) {
      const u = i / samples;
      const a = (1 - u) * (1 - u);
      const b = 2 * (1 - u) * u;
      const c = u * u;
      points.push([a * p0[0] + b * control[0] + c * p1[0], a * p0[1] + b * control[1] + c * p1[1]]);
    }
    return points;
  }

  function pointAlong(points, fraction) {
    const scaled = clamp(fraction) * (points.length - 1);
    const index = Math.min(points.length - 2, Math.floor(scaled));
    return lerpPoint(points[index], points[index + 1], scaled - index);
  }

  function periodicCurvePoints(knots, period, samplesPerSpan = 24) {
    // Closed Catmull-Rom through knots that repeat every `period` pixels.
    const extended = [
      [knots[knots.length - 2][0] - period, knots[knots.length - 2][1]],
      ...knots,
      [knots[1][0] + period, knots[1][1]],
    ];
    const points = [];
    for (let i = 1; i < extended.length - 2; i++) {
      const [p0, p1, p2, p3] = [extended[i - 1], extended[i], extended[i + 1], extended[i + 2]];
      for (let s = 0; s < samplesPerSpan; s++) {
        const u = s / samplesPerSpan;
        const u2 = u * u;
        const u3 = u2 * u;
        points.push([0, 1].map((axis) =>
          0.5 * (2 * p1[axis] + (-p0[axis] + p2[axis]) * u + (2 * p0[axis] - 5 * p1[axis] + 4 * p2[axis] - p3[axis]) * u2 + (-p0[axis] + 3 * p1[axis] - 3 * p2[axis] + p3[axis]) * u3)));
      }
    }
    points.push(knots[knots.length - 1]);
    return points;
  }

  // ------------------------------------------------------------- the thread
  function drawThread(ctx, points, reveal = 1, width = 3.4) {
    if (reveal <= 0 || points.length < 2) return;
    const scaled = clamp(reveal) * (points.length - 1);
    const lastIndex = Math.floor(scaled);
    ctx.save();
    ctx.strokeStyle = COLOR.thread;
    ctx.lineWidth = width;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.shadowColor = 'rgba(216, 30, 44, 0.6)';
    ctx.shadowBlur = 12;
    ctx.beginPath();
    ctx.moveTo(points[0][0], points[0][1]);
    for (let i = 1; i <= lastIndex; i++) ctx.lineTo(points[i][0], points[i][1]);
    if (lastIndex < points.length - 1) {
      const tip = lerpPoint(points[lastIndex], points[lastIndex + 1], scaled - lastIndex);
      ctx.lineTo(tip[0], tip[1]);
    }
    ctx.stroke();
    ctx.restore();
  }

  // ------------------------------------------------------------ carved type
  function carveText(ctx, text, x, y, options = {}) {
    const {
      size = 64,
      weight = 600,
      font = FONT.carved,
      style = '',
      align = 'center',
      spacing = 0,
      reveal = 1,
      surface = 'clay',
      alpha = 1,
    } = options;
    ctx.save();
    ctx.font = `${style} ${weight} ${size}px ${font}`;
    ctx.letterSpacing = `${spacing}px`;
    ctx.textAlign = align;
    ctx.textBaseline = 'middle';
    const width = ctx.measureText(text).width;
    const left = align === 'center' ? x - width / 2 : align === 'right' ? x - width : x;
    if (reveal < 1) {
      ctx.beginPath();
      ctx.rect(left - 20, y - size * 1.2, (width + 40) * clamp(reveal), size * 2.4);
      ctx.clip();
    }
    ctx.globalAlpha = alpha;
    const lip = Math.max(1, size * 0.028);
    if (surface === 'clay') {
      ctx.fillStyle = 'rgba(246, 212, 176, 0.6)';
      ctx.fillText(text, x, y + lip);
      ctx.fillStyle = COLOR.grooveShadow;
      ctx.fillText(text, x, y - lip * 0.7);
      ctx.fillStyle = COLOR.groove;
      ctx.fillText(text, x, y);
    } else {
      ctx.fillStyle = 'rgba(0, 0, 0, 0.7)';
      ctx.fillText(text, x, y + lip);
      ctx.fillStyle = COLOR.bone;
      ctx.fillText(text, x, y);
    }
    ctx.restore();
    return width;
  }

  function drawChapter(ctx, text, dark = false) {
    carveText(ctx, text, 150, 142, { size: 40, align: 'left', spacing: 16, surface: dark ? 'glaze' : 'clay' });
  }

  function drawStatusPill(ctx, x, y, text, options = {}) {
    const { dot = null, size = 22, textColor = COLOR.bone, align = 'center', font = FONT.ui } = options;
    ctx.save();
    ctx.font = `600 ${size}px ${font}`;
    ctx.letterSpacing = '0.5px';
    const textWidth = ctx.measureText(text).width;
    const padding = size * 0.85;
    const dotSpace = dot ? size * 1.05 : 0;
    const width = textWidth + padding * 2 + dotSpace;
    const height = size * 1.95;
    const left = align === 'center' ? x - width / 2 : align === 'right' ? x - width : x;
    ctx.fillStyle = 'rgba(12, 10, 10, 0.93)';
    roundRectPath(ctx, left, y - height / 2, width, height, height / 2);
    ctx.fill();
    ctx.strokeStyle = 'rgba(237, 227, 208, 0.35)';
    ctx.lineWidth = 1.5;
    ctx.stroke();
    if (dot) {
      ctx.save();
      ctx.shadowColor = dot;
      ctx.shadowBlur = 12;
      ctx.fillStyle = dot;
      fillCircle(ctx, left + padding + size * 0.32, y, size * 0.32);
      ctx.restore();
    }
    ctx.fillStyle = textColor;
    ctx.textBaseline = 'middle';
    ctx.fillText(text, left + padding + dotSpace, y + 1);
    ctx.restore();
    return width;
  }

  function drawCheckBadge(ctx, x, y, radius, scale = 1) {
    if (scale <= 0) return;
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    ctx.shadowColor = 'rgba(52, 199, 89, 0.7)';
    ctx.shadowBlur = 18;
    ctx.fillStyle = COLOR.running;
    fillCircle(ctx, 0, 0, radius);
    ctx.shadowBlur = 0;
    ctx.strokeStyle = '#0B1F10';
    ctx.lineWidth = radius * 0.2;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(-radius * 0.42, 0);
    ctx.lineTo(-radius * 0.1, radius * 0.32);
    ctx.lineTo(radius * 0.46, -radius * 0.34);
    ctx.stroke();
    ctx.restore();
  }

  function drawBellSlash(ctx, x, y, size, color) {
    ctx.save();
    ctx.translate(x, y);
    ctx.strokeStyle = color;
    ctx.fillStyle = color;
    ctx.lineWidth = size * 0.11;
    ctx.lineCap = 'round';
    ctx.beginPath();
    ctx.moveTo(-size * 0.42, size * 0.28);
    ctx.quadraticCurveTo(-size * 0.3, size * 0.1, -size * 0.3, -size * 0.1);
    ctx.quadraticCurveTo(-size * 0.28, -size * 0.46, 0, -size * 0.48);
    ctx.quadraticCurveTo(size * 0.28, -size * 0.46, size * 0.3, -size * 0.1);
    ctx.quadraticCurveTo(size * 0.3, size * 0.1, size * 0.42, size * 0.28);
    ctx.closePath();
    ctx.stroke();
    fillCircle(ctx, 0, size * 0.4, size * 0.08);
    strokeLine(ctx, -size * 0.5, -size * 0.5, size * 0.5, size * 0.5);
    ctx.restore();
  }

  // ------------------------------------------------------ vase decorations
  function drawMeanderStrip(ctx, top, height, ground, ink) {
    ctx.fillStyle = ground;
    ctx.fillRect(0, top, WIDTH, height);
    const railInset = 4;
    ctx.fillStyle = ink;
    ctx.fillRect(0, top + railInset, WIDTH, 3);
    ctx.fillRect(0, top + height - railInset - 3, WIDTH, 3);
    const y0 = top + 13;
    const y1 = top + height - 13;
    const count = Math.round(WIDTH / (y1 - y0) / 1.1);
    const step = WIDTH / count;
    const sx = (base, v) => base + v * step;
    const sy = (v) => y0 + v * (y1 - y0);
    ctx.strokeStyle = ink;
    ctx.lineWidth = Math.max(3, step * 0.085);
    ctx.lineCap = 'butt';
    ctx.lineJoin = 'miter';
    ctx.beginPath();
    ctx.moveTo(0, sy(1));
    ctx.lineTo(WIDTH, sy(1));
    for (let i = 0; i < count; i++) {
      const base = i * step;
      ctx.moveTo(sx(base, 0.08), sy(1));
      ctx.lineTo(sx(base, 0.08), sy(0));
      ctx.lineTo(sx(base, 0.9), sy(0));
      ctx.lineTo(sx(base, 0.9), sy(0.72));
      ctx.lineTo(sx(base, 0.36), sy(0.72));
      ctx.lineTo(sx(base, 0.36), sy(0.36));
      ctx.lineTo(sx(base, 0.62), sy(0.36));
    }
    ctx.stroke();
  }

  function drawRayStrip(ctx, top, height, ground, ink) {
    ctx.fillStyle = ground;
    ctx.fillRect(0, top, WIDTH, height);
    ctx.fillStyle = ink;
    ctx.fillRect(0, top + 4, WIDTH, 3);
    ctx.fillRect(0, top + height - 7, WIDTH, 3);
    const count = 48;
    const step = WIDTH / count;
    for (let i = 0; i < count; i++) {
      const x = i * step;
      fillPolygon(ctx, [[x + step * 0.12, top + height - 7], [x + step * 0.5, top + 12], [x + step * 0.88, top + height - 7]]);
    }
  }

  function drawPanelBorders(ctx, dark) {
    const ground = dark ? COLOR.ink : COLOR.terracotta;
    const ink = dark ? COLOR.terracotta : COLOR.ink;
    drawMeanderStrip(ctx, 0, BORDER_HEIGHT, ground, ink);
    drawRayStrip(ctx, HEIGHT - BORDER_HEIGHT, BORDER_HEIGHT, ground, ink);
    ctx.fillStyle = ink;
    ctx.fillRect(0, BORDER_HEIGHT, WIDTH, 5);
    ctx.fillRect(0, HEIGHT - BORDER_HEIGHT - 5, WIDTH, 5);
  }

  function paintClay(ctx, dark = false) {
    ctx.fillStyle = dark ? COLOR.ink : COLOR.terracotta;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    const gradient = ctx.createLinearGradient(0, 0, 0, HEIGHT);
    if (dark) {
      gradient.addColorStop(0, 'rgba(60, 25, 12, 0.35)');
      gradient.addColorStop(1, 'rgba(0, 0, 0, 0.3)');
    } else {
      gradient.addColorStop(0, 'rgba(120, 48, 18, 0.12)');
      gradient.addColorStop(0.5, 'rgba(255, 196, 150, 0.06)');
      gradient.addColorStop(1, 'rgba(90, 34, 12, 0.16)');
    }
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
  }

  function drawGroundLine(ctx, y, color, left = 0, right = WIDTH) {
    ctx.fillStyle = color;
    ctx.fillRect(left, y, right - left, 7);
    ctx.fillRect(left, y + 15, right - left, 3);
  }

  // --------------------------------------------------------------- figures
  const BODY = { thigh: 98, shin: 96, upperArm: 68, forearm: 62, torso: 130, headOffset: 31, handRadius: 8.5 };
  const TORSO_MALE = [[-22, 4], [22, 4], [17, -46], [31, -100], [24, -128], [6, -137], [-16, -133], [-29, -104], [-15, -48]];
  const TORSO_FEMALE = [[-19, 4], [20, 4], [13, -50], [25, -96], [18, -126], [2, -132], [-15, -128], [-23, -100], [-13, -50]];

  function limb(x, y, angle, length, bend, length2) {
    const jx = x + Math.sin(angle) * length;
    const jy = y + Math.cos(angle) * length;
    const endAngle = angle + bend;
    return { ax: x, ay: y, jx, jy, ex: jx + Math.sin(endAngle) * length2, ey: jy + Math.cos(endAngle) * length2 };
  }

  // Two-bone IK in limb-angle space (angle 0 points down, positive swings forward).
  function solveReach(sx, sy, target, length1, length2, bendSign) {
    const dx = target[0] - sx;
    const dy = target[1] - sy;
    const distance = clamp(Math.hypot(dx, dy), 1e-3, length1 + length2 - 1e-3);
    const base = Math.atan2(dx, dy);
    const inner = Math.acos(clamp((length1 * length1 + distance * distance - length2 * length2) / (2 * length1 * distance), -1, 1));
    const upper = base + bendSign * inner;
    const jx = sx + Math.sin(upper) * length1;
    const jy = sy + Math.cos(upper) * length1;
    return [upper, Math.atan2(target[0] - jx, target[1] - jy) - upper];
  }

  function paintLimb(ctx, segment, radii, fill, line, near) {
    const [root, joint, end] = radii;
    if (near) {
      ctx.save();
      ctx.globalCompositeOperation = 'source-atop';
      ctx.strokeStyle = line;
      taperedStroke(ctx, segment.ax, segment.ay, segment.jx, segment.jy, root + 2.4, joint + 2.4);
      taperedStroke(ctx, segment.jx, segment.jy, segment.ex, segment.ey, joint + 2.4, end + 2.4);
      ctx.restore();
    }
    ctx.strokeStyle = fill;
    taperedStroke(ctx, segment.ax, segment.ay, segment.jx, segment.jy, root, joint);
    taperedStroke(ctx, segment.jx, segment.jy, segment.ex, segment.ey, joint, end);
  }

  function paintFoot(ctx, x, y, fill) {
    ctx.fillStyle = fill;
    fillPolygon(ctx, [[x - 8, y - 7], [x + 6, y - 8], [x + 31, y + 3], [x + 31, y + 9], [x - 10, y + 9]]);
  }

  function paintHead(ctx, look, kind) {
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, -9, -5, 22, 25);
    if (kind.bun) fillCircle(ctx, -27, -19, 12);
    ctx.fillStyle = look.skin;
    fillEllipse(ctx, 4, 0, 19, 23);
    fillPolygon(ctx, [[14, -10], [30, 5], [27, 8], [15, 10]]);
    if (kind.beard) {
      ctx.fillStyle = COLOR.ink;
      fillPolygon(ctx, [[-3, 7], [21, 11], [26, 28], [13, 47], [-7, 32]]);
    } else {
      fillEllipse(ctx, 11, 15, 11, 9);
    }
    ctx.lineCap = 'round';
    ctx.strokeStyle = look.skinLine;
    ctx.fillStyle = look.skinLine;
    ctx.lineWidth = 2.2;
    if (kind.cyclops) {
      ctx.strokeStyle = COLOR.bone;
      ctx.fillStyle = COLOR.bone;
      fillEllipse(ctx, 8, -14, 12, 7);
      ctx.fillStyle = COLOR.ink;
      fillCircle(ctx, 10, -14, 4);
    } else if (kind.eyesClosed) {
      ctx.beginPath();
      ctx.arc(11, -7, 6, 0.25, Math.PI - 0.25);
      ctx.stroke();
    } else {
      ctx.beginPath();
      ctx.ellipse(11, -6, 7, 3.4, 0, 0, Math.PI * 2);
      ctx.stroke();
      fillCircle(ctx, 13, -6, 2.1);
    }
    if (kind.mouthOpen) {
      ctx.fillStyle = look.skin === COLOR.bone ? COLOR.ink : COLOR.bone;
      fillEllipse(ctx, 21, 14, 5, 3.5, 0.3);
    }
    ctx.strokeStyle = look.line;
    ctx.lineWidth = 1.8;
    ctx.beginPath();
    ctx.arc(-6, -2, 17, -2.4, -0.5);
    ctx.stroke();
    if (kind.beard) {
      strokeLine(ctx, 4, 16, 10, 38);
      strokeLine(ctx, 12, 16, 17, 34);
    }
    if (kind.pilos) {
      ctx.fillStyle = COLOR.ink;
      fillPolygon(ctx, [[-27, -11], [22, -15], [6, -58], [-9, -59]]);
      ctx.strokeStyle = look.line;
      strokeLine(ctx, -25, -16, 20, -19);
    }
    if (kind.helmet) {
      ctx.fillStyle = COLOR.ink;
      fillPolygon(ctx, [[-30, -4], [-26, -24], [-6, -30], [16, -24], [24, -10], [20, 2], [6, -2], [-4, 26], [-26, 20]]);
      fillPolygon(ctx, [[-8, -28], [4, -66], [-26, -78], [-64, -52], [-50, -44], [-26, -56], [-14, -30]]);
      ctx.strokeStyle = look.line;
      for (let i = 0; i < 5; i++) strokeLine(ctx, -44 + i * 9, -52 + i * 2, -34 + i * 9, -64 + i * 1);
    }
    if (kind.fillet) {
      ctx.strokeStyle = look.line;
      ctx.lineWidth = 2.4;
      ctx.beginPath();
      ctx.arc(-6, -4, 21, -2.6, -0.9);
      ctx.stroke();
    }
  }

  function paintTunic(ctx, hip, lean, look) {
    ctx.save();
    ctx.translate(hip[0], hip[1]);
    ctx.rotate(lean);
    ctx.fillStyle = look.fill;
    fillPolygon(ctx, [[-25, -14], [24, -14], [34, 52], [-31, 54]]);
    ctx.strokeStyle = look.line;
    ctx.lineWidth = 1.8;
    [-14, 0, 14].forEach((x) => strokeLine(ctx, x, -6, x * 1.2, 48));
    strokeLine(ctx, -29, 46, 31, 44);
    ctx.restore();
  }

  function paintDress(ctx, hip, look, swing) {
    const [hx, hy] = hip;
    ctx.fillStyle = look.fill;
    fillPolygon(ctx, [[hx - 22, hy - 12], [hx + 22, hy - 12], [hx + 38 + swing, 0], [hx - 36 + swing * 0.4, 0]]);
    ctx.strokeStyle = look.line;
    ctx.lineWidth = 1.8;
    for (let i = -2; i <= 2; i++) strokeLine(ctx, hx + i * 8, hy, hx + i * 14 + swing * 0.5, -10);
    ctx.lineWidth = 2.4;
    ctx.beginPath();
    for (let x = hx - 32; x <= hx + 34; x += 11) {
      ctx.lineTo(x, -22);
      ctx.lineTo(x + 5.5, -30);
    }
    ctx.stroke();
  }

  let figureLayer = null;

  // Figures paint onto a scratch layer so near limbs can be incised only where
  // they overlap the body (source-atop), exactly like black-figure incision.
  function withFigureLayer(ctx, paint) {
    const layer = figureLayer.getContext('2d');
    layer.setTransform(1, 0, 0, 1, 0, 0);
    layer.clearRect(0, 0, WIDTH, HEIGHT);
    layer.setTransform(ctx.getTransform());
    paint(layer);
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.drawImage(figureLayer, 0, 0);
    ctx.restore();
  }

  function walkPose(phase, stride = 0.42) {
    const swing = Math.sin(phase);
    const lift = Math.cos(phase);
    return {
      legNear: [stride * swing, -Math.max(0, -lift) * 0.75 - 0.05],
      legFar: [-stride * swing, -Math.max(0, lift) * 0.75 - 0.05],
      armNear: [-0.4 * swing, -0.3],
      armFar: [0.4 * swing, -0.3],
      lean: 0.06,
    };
  }

  /**
   * Paints a black-figure person with feet at (x, y). Pose angles use limb
   * space (0 = hanging down, positive swings toward the facing direction).
   * `reachNear` / `reachFar` / `footNear` / `footFar` accept local targets
   * (x forward, y up negative) or functions of anchors, solved with IK.
   * Returns world-space anchors for props (hands, head, hip).
   */
  function drawPerson(ctx, spec) {
    const look = spec.look ?? LOOK.black;
    const kind = spec.kind ?? {};
    const scale = spec.scale ?? 1;
    const facing = spec.facing ?? 1;
    const pose = Object.assign(
      { lean: 0, head: 0, armNear: [0.2, -0.3], armFar: [-0.15, -0.2], legNear: [0.06, -0.05], legFar: [-0.08, -0.05] },
      spec.pose,
    );

    let legNear = limb(0, 0, pose.legNear[0], BODY.thigh, pose.legNear[1], BODY.shin);
    let legFar = limb(0, 0, pose.legFar[0], BODY.thigh, pose.legFar[1], BODY.shin);
    const hip = spec.hip ?? [0, -(Math.max(legNear.ey, legFar.ey) + 9)];
    const legFrom = (angles, target, sign) => {
      const solved = target ? solveReach(hip[0], hip[1], target, BODY.thigh, BODY.shin, sign) : angles;
      return limb(hip[0], hip[1], solved[0], BODY.thigh, solved[1], BODY.shin);
    };
    legNear = legFrom(pose.legNear, pose.footNear, pose.kneeNear ?? 1);
    legFar = legFrom(pose.legFar, pose.footFar, pose.kneeFar ?? 1);

    const lean = pose.lean;
    const shoulder = [hip[0] + Math.sin(lean) * (BODY.torso - 18), hip[1] - Math.cos(lean) * (BODY.torso - 18)];
    const neck = [hip[0] + Math.sin(lean) * BODY.torso, hip[1] - Math.cos(lean) * BODY.torso];
    const headAngle = lean + pose.head;
    const headCenter = [neck[0] + Math.sin(headAngle) * BODY.headOffset, neck[1] - Math.cos(headAngle) * BODY.headOffset];
    const anchors = { hip, shoulder, neck, headCenter };
    const armFrom = (origin, angles, target, sign) => {
      const resolved = typeof target === 'function' ? target(anchors) : target;
      const solved = resolved ? solveReach(origin[0], origin[1], resolved, BODY.upperArm, BODY.forearm, sign) : angles;
      return limb(origin[0], origin[1], solved[0], BODY.upperArm, solved[1], BODY.forearm);
    };
    const armNear = armFrom(shoulder, pose.armNear, pose.reachNear, pose.elbowNear ?? -1);
    const armFar = armFrom([shoulder[0] - 4, shoulder[1] + 2], pose.armFar, pose.reachFar, pose.elbowFar ?? -1);

    withFigureLayer(ctx, (g) => {
      g.save();
      g.translate(spec.x, spec.y);
      g.scale(facing * scale, scale);

      paintLimb(g, armFar, [9.5, 7.5, 6], look.skin, look.skinLine, false);
      g.fillStyle = look.skin;
      fillCircle(g, armFar.ex, armFar.ey, BODY.handRadius);
      if (!kind.dress) {
        paintLimb(g, legFar, [16, 10.5, 6.5], look.skin, look.line, false);
        paintFoot(g, legFar.ex, legFar.ey, look.skin);
      }

      g.save();
      g.translate(hip[0], hip[1]);
      g.rotate(lean);
      g.fillStyle = kind.dress ? look.fill : look.skin;
      fillPolygon(g, kind.female ? TORSO_FEMALE : TORSO_MALE);
      g.strokeStyle = kind.dress ? look.line : look.skinLine;
      g.lineWidth = 2;
      if (!kind.female) {
        g.beginPath();
        g.moveTo(4, -98);
        g.quadraticCurveTo(22, -82, 30, -96);
        g.stroke();
      }
      strokeLine(g, -19, -10, 18, -10);
      g.restore();

      if (!kind.dress) {
        paintLimb(g, legNear, [16, 10.5, 6.5], look.skin, look.line, true);
        paintFoot(g, legNear.ex, legNear.ey, look.skin);
      }
      if (kind.tunic) paintTunic(g, hip, lean, look);
      if (kind.dress) paintDress(g, hip, look, spec.dressSwing ?? 0);

      g.save();
      g.translate(headCenter[0], headCenter[1]);
      g.rotate(headAngle);
      paintHead(g, look, kind);
      g.restore();

      paintLimb(g, armNear, [10, 8, 6], look.skin, look.skinLine === COLOR.ink && look.skin === COLOR.bone ? COLOR.ink : look.line, true);
      g.fillStyle = look.skin;
      fillCircle(g, armNear.ex, armNear.ey, BODY.handRadius);

      if (spec.decorate) spec.decorate(g, { ...anchors, armNear, armFar, legNear, legFar });
      g.restore();
    });

    const toWorld = ([lx, ly]) => [spec.x + facing * scale * lx, spec.y + scale * ly];
    return {
      handNear: toWorld([armNear.ex, armNear.ey]),
      handFar: toWorld([armFar.ex, armFar.ey]),
      head: toWorld(headCenter),
      hip: toWorld(hip),
      shoulder: toWorld(shoulder),
      scale,
      facing,
    };
  }

  // ------------------------------------------------------------------ props
  function drawHandheldPhone(ctx, x, y, size, state = {}, angle = 0) {
    const width = size;
    const height = size * 2.05;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle);
    if (state.glow) {
      const rgb = state.glow === 'green' ? '52, 199, 89' : '237, 227, 208';
      const glow = ctx.createRadialGradient(0, 0, size * 0.3, 0, 0, size * 3.2);
      glow.addColorStop(0, `rgba(${rgb}, 0.45)`);
      glow.addColorStop(1, `rgba(${rgb}, 0)`);
      ctx.fillStyle = glow;
      ctx.fillRect(-size * 3.2, -size * 3.2, size * 6.4, size * 6.4);
    }
    ctx.fillStyle = '#0B0A0A';
    roundRectPath(ctx, -width / 2, -height / 2, width, height, width * 0.24);
    ctx.fill();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = Math.max(1.3, width * 0.065);
    ctx.stroke();
    const lineWidth = Math.max(1.2, width * 0.07);
    ctx.lineWidth = lineWidth;
    ctx.lineCap = 'round';
    switch (state.icon) {
      case 'running':
        ctx.fillStyle = COLOR.running;
        fillCircle(ctx, 0, -height * 0.12, width * 0.18);
        ctx.fillStyle = 'rgba(237, 227, 208, 0.6)';
        ctx.fillRect(-width * 0.28, height * 0.1, width * 0.56, lineWidth);
        break;
      case 'queued':
        ctx.strokeStyle = COLOR.bone;
        ctx.beginPath();
        ctx.arc(0, -height * 0.06, width * 0.24, 0, Math.PI * 2);
        ctx.stroke();
        strokeLine(ctx, 0, -height * 0.06, 0, -height * 0.06 - width * 0.15);
        strokeLine(ctx, 0, -height * 0.06, width * 0.12, -height * 0.06);
        break;
      case 'muted':
        drawBellSlash(ctx, 0, -height * 0.04, width * 0.55, 'rgba(237, 227, 208, 0.75)');
        break;
      default:
        break;
    }
    ctx.restore();
  }

  function drawLaptop(ctx, x, y, scale, state = {}) {
    const running = state.running ?? [false, false, false, false];
    const pulse = state.pulse ?? 0;
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    const glow = ctx.createRadialGradient(0, -150, 30, 0, -150, 460);
    glow.addColorStop(0, 'rgba(237, 227, 208, 0.2)');
    glow.addColorStop(1, 'rgba(237, 227, 208, 0)');
    ctx.fillStyle = glow;
    ctx.fillRect(-460, -610, 920, 700);
    ctx.fillStyle = COLOR.ink;
    roundRectPath(ctx, -202, -296, 404, 282, 12);
    ctx.fill();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.fillStyle = COLOR.screen;
    ctx.fillRect(-190, -284, 380, 258);
    ctx.fillStyle = '#1E1A19';
    ctx.fillRect(-190, -284, 380, 16);
    const orb = [172, -276];
    ctx.fillStyle = COLOR.bone;
    fillCircle(ctx, orb[0], orb[1], 4.5);
    ctx.strokeStyle = `rgba(237, 227, 208, ${0.5 * (1 - (pulse % 1))})`;
    ctx.lineWidth = 1.5;
    ctx.beginPath();
    ctx.arc(orb[0], orb[1], 4.5 + (pulse % 1) * 9, 0, Math.PI * 2);
    ctx.stroke();
    const random = seededRandom(5);
    for (let pane = 0; pane < 4; pane++) {
      const px = -184 + (pane % 2) * 186;
      const py = -262 + Math.floor(pane / 2) * 118;
      ctx.fillStyle = '#171414';
      ctx.fillRect(px, py, 180, 112);
      ctx.fillStyle = 'rgba(237, 227, 208, 0.75)';
      ctx.font = `600 12px ${FONT.mono}`;
      ctx.textBaseline = 'middle';
      ctx.fillText(SESSION_NAMES[pane], px + 10, py + 14);
      ctx.fillStyle = 'rgba(237, 227, 208, 0.2)';
      for (let line = 0; line < 5; line++) ctx.fillRect(px + 10, py + 32 + line * 14, 40 + random() * 120, 5);
      if (running[pane]) {
        ctx.save();
        ctx.shadowColor = COLOR.running;
        ctx.shadowBlur = 10;
        ctx.fillStyle = COLOR.running;
        fillCircle(ctx, px + 164, py + 14, 5 + Math.sin(pulse * Math.PI * 2 + pane) * 0.8);
        ctx.restore();
      } else {
        ctx.strokeStyle = 'rgba(237, 227, 208, 0.5)';
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(px + 164, py + 14, 5, 0, Math.PI * 2);
        ctx.stroke();
      }
    }
    ctx.fillStyle = COLOR.ink;
    fillPolygon(ctx, [[-214, -16], [214, -16], [236, 0], [-236, 0]]);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    strokeLine(ctx, -212, -14, 212, -14);
    ctx.restore();
    return [x + orb[0] * scale, y + orb[1] * scale];
  }

  function drawShip(ctx, x, y, scale, t, look = LOOK.black, options = {}) {
    const tilt = options.tilt ?? 0;
    const toParent = (lx, ly) => [
      x + scale * (Math.cos(tilt) * lx - Math.sin(tilt) * ly),
      y + scale * (Math.sin(tilt) * lx + Math.cos(tilt) * ly),
    ];
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(tilt);
    ctx.scale(scale, scale);
    ctx.strokeStyle = look.fill;
    ctx.lineWidth = 5;
    ctx.lineCap = 'round';
    for (let i = 0; i < 10; i++) {
      const oarX = -210 + i * 42;
      const phase = t * Math.PI * 2 * 0.7 + i * 0.12;
      strokeLine(ctx, oarX, -38, oarX - 60 + Math.sin(phase) * 28, 72);
    }
    if (options.mast !== false) {
      ctx.fillStyle = look.fill;
      ctx.fillRect(-5, -430, 10, 400);
      ctx.lineWidth = 7;
      strokeLine(ctx, -180, -405, 180, -405);
      ctx.lineWidth = 2;
      strokeLine(ctx, 0, -428, -318, -110);
      strokeLine(ctx, 0, -428, 300, -70);
      if (options.sail !== false) {
        fillPolygon(ctx, [[-170, -398], [170, -398], [160, -212], [-160, -212]]);
        ctx.strokeStyle = look.line;
        ctx.lineWidth = 1.8;
        for (let i = -3; i <= 3; i++) strokeLine(ctx, i * 44, -394, i * 42, -216);
        strokeLine(ctx, -166, -334, 166, -334);
        strokeLine(ctx, -163, -272, 163, -272);
      }
    }
    ctx.fillStyle = look.fill;
    ctx.beginPath();
    ctx.moveTo(-330, -118);
    ctx.quadraticCurveTo(-352, -60, -300, -34);
    ctx.lineTo(250, -40);
    ctx.lineTo(300, -76);
    ctx.lineTo(315, -72);
    ctx.lineTo(296, -34);
    ctx.lineTo(352, -12);
    ctx.lineTo(296, 4);
    ctx.quadraticCurveTo(0, 22, -290, -6);
    ctx.quadraticCurveTo(-322, -40, -318, -112);
    ctx.closePath();
    ctx.fill();
    for (let i = 0; i < 9; i++) fillCircle(ctx, -190 + i * 44, -52, 11);
    ctx.strokeStyle = look.line;
    ctx.lineWidth = 2.2;
    strokeLine(ctx, -286, -28, 286, -30);
    strokeLine(ctx, -270, -14, 280, -16);
    ctx.beginPath();
    ctx.arc(-330, -126, 9, 0.5, Math.PI * 1.6);
    ctx.stroke();
    ctx.fillStyle = look.line;
    fillEllipse(ctx, 266, -24, 9, 4.5);
    ctx.fillStyle = look.fill;
    fillCircle(ctx, 268, -24, 2.6);
    ctx.restore();
    return { stern: toParent(-328, -116), deck: toParent(30, -40), bow: toParent(350, -12) };
  }

  /** Sea with incised curls: a filled ink body under a peaked wave crest. */
  function drawSea(ctx, options) {
    const {
      level,
      amplitude = 26,
      wavelength = 160,
      phase = 0,
      fill = COLOR.ink,
      line = COLOR.bone,
      bottom = HEIGHT,
      left = -20,
      right = WIDTH + 20,
      curls = true,
      swells = true,
    } = options;
    const crestY = (x) => {
      const u = (((x / wavelength + phase) % 1) + 1) % 1;
      return level - amplitude * Math.pow(Math.sin(Math.PI * u), 3);
    };
    ctx.fillStyle = fill;
    ctx.beginPath();
    ctx.moveTo(left, bottom);
    for (let x = left; x <= right; x += 6) ctx.lineTo(x, crestY(x));
    ctx.lineTo(right, crestY(right));
    ctx.lineTo(right, bottom);
    ctx.closePath();
    ctx.fill();
    ctx.save();
    ctx.strokeStyle = line;
    if (curls) {
      ctx.lineWidth = 2.4;
      ctx.globalAlpha = 0.8;
      const first = Math.floor(left / wavelength + phase) - 1;
      for (let k = first; (k - phase) * wavelength < right + wavelength; k++) {
        const cx = (k + 0.5 - phase) * wavelength + wavelength * 0.05;
        if (cx < left + 10 || cx > right - 10) continue;
        strokeSpiral(ctx, cx, level - amplitude * 0.2 + 6, Math.min(amplitude * 0.55, wavelength * 0.18), 1.4, -Math.PI / 2, 1);
      }
    }
    if (swells) {
      ctx.lineWidth = 2;
      ctx.globalAlpha = 0.28;
      for (let row = 1; row <= 4; row++) {
        const y = level + 18 + row * 38;
        if (y > bottom) break;
        ctx.beginPath();
        for (let x = left; x <= right; x += 12) {
          const yy = y + Math.sin(x / (wavelength * 0.6) + row * 1.7 + phase * 6) * 5;
          if (x === left) ctx.moveTo(x, yy);
          else ctx.lineTo(x, yy);
        }
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  function drawDolphin(ctx, x, y, scale, fill, line) {
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    ctx.fillStyle = fill;
    ctx.beginPath();
    ctx.moveTo(-70, 8);
    ctx.quadraticCurveTo(-10, -34, 60, -6);
    ctx.lineTo(82, -2);
    ctx.lineTo(60, 6);
    ctx.quadraticCurveTo(0, 22, -70, 8);
    ctx.fill();
    fillPolygon(ctx, [[-66, 8], [-92, -12], [-84, 8], [-94, 26]]);
    fillPolygon(ctx, [[-4, -18], [8, -40], [18, -14]]);
    ctx.fillStyle = line;
    fillCircle(ctx, 48, -6, 2.4);
    ctx.restore();
  }

  function drawLotus(ctx, x, groundY, height, sway) {
    ctx.save();
    ctx.strokeStyle = COLOR.ink;
    ctx.fillStyle = COLOR.ink;
    ctx.lineWidth = 5;
    const top = [x + sway, groundY - height];
    ctx.beginPath();
    ctx.moveTo(x, groundY);
    ctx.quadraticCurveTo(x - 24, groundY - height * 0.55, top[0], top[1]);
    ctx.stroke();
    for (let i = -2; i <= 2; i++) {
      const angle = i * 0.42;
      fillEllipse(ctx, top[0] + Math.sin(angle) * 24, top[1] - Math.cos(angle) * 24, 9, 28, angle);
    }
    fillPolygon(ctx, [[top[0] - 16, top[1] + 4], [top[0] + 16, top[1] + 4], [top[0], top[1] + 22]]);
    const budBase = [x - 8, groundY - height * 0.42];
    const bud = [x + 46 + sway * 0.6, groundY - height * 0.62];
    ctx.beginPath();
    ctx.moveTo(budBase[0], budBase[1]);
    ctx.quadraticCurveTo(bud[0] - 10, budBase[1] - 10, bud[0], bud[1]);
    ctx.stroke();
    fillEllipse(ctx, bud[0], bud[1] - 14, 10, 20, 0.3);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.6;
    for (let i = -2; i <= 2; i++) {
      const angle = i * 0.42;
      strokeLine(ctx, top[0] + Math.sin(angle) * 10, top[1] - Math.cos(angle) * 10, top[0] + Math.sin(angle) * 40, top[1] - Math.cos(angle) * 40);
    }
    ctx.restore();
  }

  function drawLotusFlower(ctx, x, y, size) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    for (let i = -2; i <= 2; i++) {
      const angle = i * 0.45;
      fillEllipse(ctx, x + Math.sin(angle) * size * 0.5, y - Math.cos(angle) * size * 0.5, size * 0.22, size * 0.6, angle);
    }
    ctx.restore();
  }

  function drawLoom(ctx, x, groundY) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(x - 10, groundY - 580, 16, 580);
    ctx.fillRect(x + 236, groundY - 580, 16, 580);
    ctx.fillRect(x - 24, groundY - 594, 290, 20);
    ctx.fillRect(x + 6, groundY - 572, 230, 190);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    for (let row = 0; row < 3; row++) {
      const y = groundY - 552 + row * 58;
      ctx.beginPath();
      for (let i = 0; i < 9; i++) {
        const bx = x + 18 + i * 24;
        ctx.moveTo(bx, y + 34);
        ctx.lineTo(bx, y);
        ctx.lineTo(bx + 18, y);
        ctx.lineTo(bx + 18, y + 22);
        ctx.lineTo(bx + 7, y + 22);
      }
      ctx.stroke();
    }
    ctx.lineWidth = 1.5;
    ctx.globalAlpha = 0.85;
    for (let i = 0; i < 14; i++) strokeLine(ctx, x + 14 + i * 16, groundY - 382, x + 14 + i * 16, groundY - 150);
    ctx.globalAlpha = 1;
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(x, groundY - 330, 240, 8);
    for (let i = 0; i < 14; i++) fillPolygon(ctx, [[x + 8 + i * 16, groundY - 150], [x + 20 + i * 16, groundY - 150], [x + 22 + i * 16, groundY - 118], [x + 6 + i * 16, groundY - 118]]);
    ctx.restore();
  }

  function drawTable(ctx, x, topY, width) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(x - width / 2, topY, width, 13);
    ctx.strokeStyle = COLOR.ink;
    [-1, 1].forEach((side) => {
      const legX = x + side * (width / 2 - 26);
      taperedPath(ctx, [[legX, topY + 10], [legX + side * 10, topY + 80], [legX - side * 6, GROUND_Y - 30], [legX + side * 16, GROUND_Y]], 9, 6);
    });
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.6;
    strokeLine(ctx, x - width / 2 + 6, topY + 6, x + width / 2 - 6, topY + 6);
    ctx.restore();
  }

  function drawKline(ctx, x, groundY, width, height) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    const top = groundY - height;
    ctx.fillRect(x - width / 2, top, width, 30);
    fillEllipse(ctx, x - width / 2 + 30, top - 12, 46, 22, -0.2);
    [-1, 1].forEach((side) => {
      const legX = x + side * (width / 2 - 22);
      ctx.fillRect(legX - 8, top + 20, 16, height - 20);
      fillEllipse(ctx, legX, top + 60, 14, 9);
      fillEllipse(ctx, legX, groundY - 20, 12, 8);
    });
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.8;
    for (let i = 1; i < 6; i++) strokeLine(ctx, x - width / 2 + i * (width / 6), top + 4, x - width / 2 + i * (width / 6) + 10, top + 26);
    ctx.restore();
    return top;
  }

  function drawKylix(ctx, x, y, size) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(x - size, y);
    ctx.quadraticCurveTo(x, y + size * 0.9, x + size, y);
    ctx.closePath();
    ctx.fill();
    ctx.fillRect(x - 2, y + size * 0.4, 4, size * 0.4);
    fillEllipse(ctx, x, y + size * 0.82, size * 0.4, size * 0.1);
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.arc(x - size * 1.05, y + size * 0.1, size * 0.22, Math.PI * 0.5, Math.PI * 1.5);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(x + size * 1.05, y + size * 0.1, size * 0.22, -Math.PI * 0.5, Math.PI * 0.5);
    ctx.stroke();
    ctx.restore();
  }

  function drawOliveDesk(ctx, x, groundY) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    fillPolygon(ctx, [
      [x - 70, groundY], [x - 34, groundY - 120], [x - 52, groundY - 262], [x - 22, groundY - 420],
      [x - 44, groundY - 560], [x + 12, groundY - 604], [x + 34, groundY - 430], [x + 54, groundY - 270],
      [x + 30, groundY - 130], [x + 78, groundY],
    ]);
    ctx.strokeStyle = COLOR.ink;
    taperedStroke(ctx, x - 20, groundY - 560, x - 210, groundY - 700, 20, 7);
    taperedStroke(ctx, x + 10, groundY - 580, x + 180, groundY - 690, 18, 6);
    taperedStroke(ctx, x - 4, groundY - 590, x + 30, groundY - 770, 16, 6);
    const random = seededRandom(77);
    const clusters = [[x - 210, groundY - 700], [x + 180, groundY - 690], [x + 30, groundY - 770], [x - 90, groundY - 660], [x + 90, groundY - 640]];
    clusters.forEach(([cx, cy]) => {
      for (let i = 0; i < 26; i++) fillEllipse(ctx, cx + (random() - 0.5) * 170, cy + (random() - 0.5) * 90, 14, 5, random() * Math.PI);
    });
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.8;
    ctx.globalAlpha = 0.85;
    for (let i = 0; i < 5; i++) {
      ctx.beginPath();
      ctx.moveTo(x - 30 + i * 13, groundY - 20);
      ctx.bezierCurveTo(x - 50 + i * 12, groundY - 160, x - 10 + i * 12, groundY - 320, x - 30 + i * 10, groundY - 480);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    const deskY = groundY - 236;
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(x - 250, deskY, 540, 18);
    ctx.fillRect(x + 268, deskY + 10, 14, groundY - deskY - 10);
    ctx.fillRect(x - 238, deskY + 10, 14, groundY - deskY - 10);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.6;
    strokeLine(ctx, x - 244, deskY + 8, x + 284, deskY + 8);
    ctx.restore();
    return deskY;
  }

  function drawHouse(ctx, x, groundY, scale, glow) {
    ctx.save();
    ctx.translate(x, groundY);
    ctx.scale(scale, scale);
    if (glow > 0) {
      const light = ctx.createRadialGradient(0, -120, 10, 0, -120, 260);
      light.addColorStop(0, `rgba(52, 199, 89, ${0.55 * glow})`);
      light.addColorStop(1, 'rgba(52, 199, 89, 0)');
      ctx.fillStyle = light;
      ctx.fillRect(-260, -380, 520, 520);
    }
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(-190, -20, 380, 20);
    ctx.fillRect(-172, -36, 344, 16);
    for (let i = 0; i < 4; i++) ctx.fillRect(-150 + i * 92, -236, 26, 200);
    ctx.fillRect(-176, -262, 352, 26);
    fillPolygon(ctx, [[-184, -262], [184, -262], [0, -340]]);
    ctx.fillStyle = glow > 0 ? `rgba(237, 227, 208, ${0.35 + 0.65 * glow})` : COLOR.terracottaShadow;
    ctx.fillRect(-32, -180, 64, 90);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    strokeLine(ctx, -176, -250, 176, -250);
    ctx.restore();
    return [x, groundY - 135 * scale];
  }

  function drawSun(ctx, x, horizonY, radius) {
    ctx.save();
    ctx.beginPath();
    ctx.rect(x - radius * 2, horizonY - radius * 2, radius * 4, radius * 2);
    ctx.clip();
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 3;
    for (let i = 0; i < 16; i++) {
      const angle = Math.PI + (i / 15) * Math.PI;
      strokeLine(ctx, x + Math.cos(angle) * radius * 1.18, horizonY + Math.sin(angle) * radius * 1.18, x + Math.cos(angle) * radius * 1.55, horizonY + Math.sin(angle) * radius * 1.55);
    }
    ctx.fillStyle = COLOR.bone;
    fillCircle(ctx, x, horizonY, radius);
    ctx.restore();
  }

  function drawSiren(ctx, x, y, scale, flap, facing, mouthOpen) {
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(facing * scale, scale);
    ctx.fillStyle = COLOR.ink;
    fillPolygon(ctx, [[-56, -6], [-124, -34], [-130, 12], [-56, 16]]);
    const wing = (rotation) => {
      ctx.save();
      ctx.translate(-8, -18);
      ctx.rotate(rotation);
      fillPolygon(ctx, [[0, 0], [-46, -118], [-12, -126], [30, -64], [40, 0]]);
      ctx.strokeStyle = COLOR.bone;
      ctx.lineWidth = 1.6;
      for (let i = 0; i < 4; i++) strokeLine(ctx, 6 + i * 8, -6, -30 + i * 10, -100 + i * 12);
      ctx.restore();
    };
    ctx.fillStyle = COLOR.ink;
    wing(-0.5 - flap * 0.8);
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, 0, 0, 62, 30);
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 5;
    strokeLine(ctx, -8, 24, -4, 52);
    strokeLine(ctx, 14, 24, 18, 52);
    ctx.lineWidth = 3;
    [-4, 18].forEach((footX) => {
      strokeLine(ctx, footX, 52, footX + 12, 58);
      strokeLine(ctx, footX, 52, footX - 8, 60);
    });
    ctx.fillStyle = COLOR.bone;
    fillPolygon(ctx, [[36, -18], [54, -40], [64, -30], [50, -6]]);
    ctx.save();
    ctx.translate(58, -60);
    ctx.scale(0.9, 0.9);
    paintHead(ctx, LOOK.woman, { bun: true, mouthOpen });
    ctx.restore();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 1.6;
    for (let i = 0; i < 4; i++) strokeLine(ctx, -110 + i * 14, -24 + i * 4, -60, 4);
    ctx.fillStyle = COLOR.ink;
    wing(-0.2 + flap * 0.6);
    ctx.restore();
    return [x + facing * scale * 78, y + scale * -48];
  }

  function drawLion(ctx, cx, groundY, t) {
    ctx.save();
    const breathe = Math.sin(t * 6) * 3;
    ctx.fillStyle = COLOR.ink;
    ctx.strokeStyle = COLOR.ink;
    fillEllipse(ctx, cx + 20, groundY - 200 + breathe, 190, 70);
    [[-110, 1], [-60, -1], [120, 1], [170, -1]].forEach(([offset, sign]) => {
      taperedStroke(ctx, cx + offset, groundY - 190, cx + offset + sign * 12, groundY - 12, 24, 11);
      fillEllipse(ctx, cx + offset + sign * 12 - 8, groundY - 8, 20, 9);
    });
    ctx.lineWidth = 12;
    ctx.beginPath();
    ctx.moveTo(cx + 200, groundY - 220);
    ctx.bezierCurveTo(cx + 290, groundY - 240, cx + 280, groundY - 360, cx + 330, groundY - 380);
    ctx.stroke();
    fillEllipse(ctx, cx + 336, groundY - 386, 16, 26, 0.5);
    const head = [cx - 190, groundY - 280 + breathe];
    const mane = [];
    for (let i = 0; i < 44; i++) {
      const angle = (i / 44) * Math.PI * 2;
      const radius = i % 2 ? 84 : 116;
      mane.push([head[0] + 18 + Math.cos(angle) * radius, head[1] + Math.sin(angle) * radius]);
    }
    fillPolygon(ctx, mane);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    for (let i = 0; i < 22; i++) {
      const angle = (i / 22) * Math.PI * 2;
      strokeLine(ctx, head[0] + 18 + Math.cos(angle) * 66, head[1] + Math.sin(angle) * 66, head[0] + 18 + Math.cos(angle) * 98, head[1] + Math.sin(angle) * 98);
    }
    ctx.fillStyle = COLOR.ink;
    fillCircle(ctx, head[0], head[1], 60);
    fillPolygon(ctx, [[head[0] - 40, head[1] - 10], [head[0] - 96, head[1] + 8], [head[0] - 88, head[1] + 44], [head[0] - 30, head[1] + 40]]);
    ctx.fillStyle = COLOR.bone;
    fillPolygon(ctx, [[head[0] - 90, head[1] + 20], [head[0] - 40, head[1] + 22], [head[0] - 82, head[1] + 34]]);
    fillEllipse(ctx, head[0] - 30, head[1] - 22, 11, 5, -0.2);
    ctx.fillStyle = COLOR.ink;
    fillCircle(ctx, head[0] - 34, head[1] - 22, 3.5);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(cx + 20, groundY - 200 + breathe, 120, 2.6, 3.6);
    ctx.stroke();
    ctx.restore();
  }

  function drawSerpent(ctx, cx, groundY, t) {
    const points = [];
    for (let i = 0; i <= 64; i++) {
      const u = i / 64;
      points.push([cx + 300 - u * 560, groundY - 60 - Math.sin(u * Math.PI * 2.3 + t * 3) * 70 * (1 - u * 0.4) - u * u * 300]);
    }
    ctx.save();
    ctx.strokeStyle = COLOR.ink;
    taperedPath(ctx, points, 8, 44);
    const head = points[points.length - 1];
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, head[0] - 34, head[1] - 6, 58, 34, -0.15);
    ctx.fillStyle = COLOR.bone;
    fillEllipse(ctx, head[0] - 46, head[1] - 16, 9, 5);
    ctx.fillStyle = COLOR.ink;
    fillCircle(ctx, head[0] - 48, head[1] - 16, 3);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 3;
    const tongue = Math.sin(t * 20) > 0 ? 1 : 0.6;
    strokeLine(ctx, head[0] - 90, head[1] + 2, head[0] - 90 - 40 * tongue, head[1] - 4);
    strokeLine(ctx, head[0] - 90 - 40 * tongue, head[1] - 4, head[0] - 108 - 40 * tongue, head[1] - 16);
    strokeLine(ctx, head[0] - 90 - 40 * tongue, head[1] - 4, head[0] - 106 - 40 * tongue, head[1] + 8);
    ctx.lineWidth = 1.8;
    for (let i = 4; i < points.length - 4; i += 3) {
      const [x0, y0] = points[i - 1];
      const [x1, y1] = points[i + 1];
      const angle = Math.atan2(y1 - y0, x1 - x0) + Math.PI / 2;
      const width = lerp(4, 26, i / points.length);
      const [px, py] = points[i];
      ctx.beginPath();
      ctx.moveTo(px + Math.cos(angle) * width, py + Math.sin(angle) * width);
      ctx.lineTo(px - Math.cos(angle - Math.PI / 2) * 8, py - Math.sin(angle - Math.PI / 2) * 8);
      ctx.lineTo(px - Math.cos(angle) * width, py - Math.sin(angle) * width);
      ctx.stroke();
    }
    ctx.restore();
  }

  function drawWaterSpirit(ctx, cx, groundY, t) {
    ctx.save();
    const rise = Math.sin(t * 4) * 8;
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(cx + 270, groundY);
    ctx.bezierCurveTo(cx + 270, groundY - 270, cx + 190, groundY - 540 + rise, cx - 20, groundY - 540 + rise);
    ctx.bezierCurveTo(cx - 210, groundY - 540 + rise, cx - 250, groundY - 390, cx - 160, groundY - 336);
    ctx.bezierCurveTo(cx - 96, groundY - 300, cx - 40, groundY - 360, cx - 84, groundY - 404);
    ctx.bezierCurveTo(cx - 30, groundY - 450, cx + 70, groundY - 390, cx + 44, groundY - 284);
    ctx.bezierCurveTo(cx + 20, groundY - 150, cx - 120, groundY - 40, cx - 270, groundY);
    ctx.closePath();
    ctx.fill();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2.6;
    strokeSpiral(ctx, cx - 116, groundY - 404, 44, 1.6, 0, 1);
    for (let i = 0; i < 4; i++) {
      ctx.beginPath();
      ctx.moveTo(cx + 230 - i * 36, groundY - 20);
      ctx.bezierCurveTo(cx + 230 - i * 30, groundY - 250, cx + 150 - i * 30, groundY - 470 + i * 30, cx - 20, groundY - 500 + i * 34 + rise);
      ctx.stroke();
    }
    ctx.fillStyle = COLOR.ink;
    const random = seededRandom(9);
    for (let i = 0; i < 14; i++) {
      const angle = Math.PI * (1.05 + random() * 0.9);
      const distance = 40 + ((t * 120 + random() * 200) % 160);
      fillCircle(ctx, cx - 40 + Math.cos(angle) * (200 + distance), groundY - 480 + Math.sin(angle) * distance * 0.7 + rise, 5 + random() * 6);
    }
    ctx.restore();
  }

  function drawFireSpirit(ctx, cx, groundY, t, fill = COLOR.ink, line = COLOR.bone) {
    ctx.save();
    const random = seededRandom(31);
    const flicker = Math.floor(t * PUPPET_FPS);
    for (let i = 0; i < 9; i++) {
      const baseX = cx - 230 + i * 58;
      const seed = random();
      const height = 260 + 200 * Math.abs(Math.sin(seed * 12 + flicker * 0.9)) + (i === 4 ? 140 : 0);
      const sway = Math.sin(flicker * 0.7 + i) * 30;
      ctx.fillStyle = fill;
      ctx.beginPath();
      ctx.moveTo(baseX - 52, groundY);
      ctx.quadraticCurveTo(baseX - 50, groundY - height * 0.5, baseX + sway, groundY - height);
      ctx.quadraticCurveTo(baseX + 44, groundY - height * 0.45, baseX + 52, groundY);
      ctx.closePath();
      ctx.fill();
      ctx.strokeStyle = line;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(baseX - 12, groundY - 20);
      ctx.quadraticCurveTo(baseX - 14, groundY - height * 0.4, baseX + sway * 0.6, groundY - height * 0.7);
      ctx.stroke();
    }
    ctx.fillStyle = COLOR.bone;
    for (let i = 0; i < 16; i++) {
      const x = cx - 260 + random() * 520;
      const y = groundY - 200 - ((t * 180 + random() * 500) % 500);
      fillCircle(ctx, x + Math.sin(t * 3 + i) * 10, y, 2.5 + random() * 2);
    }
    ctx.restore();
  }

  function drawOldManOfTheSea(ctx, cx, groundY, t) {
    drawPerson(ctx, {
      x: cx, y: groundY, facing: -1, scale: 1.2,
      kind: { beard: true, fillet: true },
      hip: [0, -110],
      pose: { lean: 0.3, head: 0.2, footNear: [110, -8], footFar: [80, -4], reachNear: [110, -130], reachFar: [60, -120] },
    });
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(cx + 40, groundY - 100);
    ctx.bezierCurveTo(cx + 180, groundY - 90, cx + 230, groundY - 20, cx + 310, groundY - 60 + Math.sin(t * 5) * 10);
    ctx.lineTo(cx + 330, groundY - 10);
    ctx.bezierCurveTo(cx + 240, groundY + 4, cx + 120, groundY, cx + 30, groundY - 60);
    ctx.fill();
    ctx.restore();
  }

  function drawAxe(ctx, x, holeY, groundY, holeFill) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    fillPolygon(ctx, [[x - 8, groundY], [x + 8, groundY], [x + 5, holeY + 34], [x - 5, holeY + 34]]);
    ctx.translate(x, holeY);
    fillPolygon(ctx, [[-26, -12], [-66, -52], [-80, 0], [-66, 52], [-26, 12]]);
    fillPolygon(ctx, [[26, -12], [66, -52], [80, 0], [66, 52], [26, 12]]);
    fillCircle(ctx, 0, 0, 32);
    ctx.fillStyle = holeFill;
    fillCircle(ctx, 0, 0, 19);
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2.2;
    ctx.beginPath();
    ctx.arc(0, 0, 23, 0, Math.PI * 2);
    ctx.stroke();
    strokeLine(ctx, -62, -40, -74, 0);
    strokeLine(ctx, 62, -40, 74, 0);
    ctx.restore();
  }

  function drawAxeRingFront(ctx, x, holeY) {
    ctx.save();
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 11;
    ctx.beginPath();
    ctx.arc(x, holeY, 25.5, 0.15, Math.PI - 0.15);
    ctx.stroke();
    ctx.restore();
  }

  function drawArrow(ctx, tipX, y) {
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 5;
    strokeLine(ctx, tipX - 170, y, tipX - 18, y);
    fillPolygon(ctx, [[tipX, y], [tipX - 26, y - 10], [tipX - 20, y], [tipX - 26, y + 10]]);
    fillPolygon(ctx, [[tipX - 150, y], [tipX - 176, y - 17], [tipX - 160, y - 17], [tipX - 132, y]]);
    fillPolygon(ctx, [[tipX - 150, y], [tipX - 176, y + 17], [tipX - 160, y + 17], [tipX - 132, y]]);
    ctx.restore();
  }

  /** A close-up black-figure hand whose index finger points at `tip`. */
  function drawPointingHand(ctx, tip, direction, skin, line) {
    const [dx, dy] = direction;
    const px = -dy;
    const py = dx;
    const wrist = [tip[0] - dx * 170, tip[1] - dy * 170];
    const elbow = [tip[0] - dx * 560, tip[1] - dy * 560];
    const at = (along, across) => [wrist[0] + dx * along + px * across, wrist[1] + dy * along + py * across];
    ctx.save();
    ctx.strokeStyle = skin;
    taperedStroke(ctx, elbow[0], elbow[1], wrist[0], wrist[1], 40, 30);
    ctx.fillStyle = skin;
    const palm = at(52, 0);
    fillEllipse(ctx, palm[0], palm[1], 58, 42, Math.atan2(dy, dx));
    const knuckle = at(96, -8);
    taperedStroke(ctx, knuckle[0], knuckle[1], tip[0], tip[1], 14, 10.5);
    for (let k = 0; k < 3; k++) {
      const curled = at(92 - k * 6, 16 + k * 17);
      fillCircle(ctx, curled[0], curled[1], 15);
    }
    const thumbBase = at(38, -34);
    const thumbTip = at(84, -46);
    taperedStroke(ctx, thumbBase[0], thumbBase[1], thumbTip[0], thumbTip[1], 14, 10);
    ctx.strokeStyle = line;
    ctx.lineWidth = 2.2;
    const joint = lerpPoint(knuckle, tip, 0.5);
    strokeLine(ctx, joint[0] - px * 10, joint[1] - py * 10, joint[0] + px * 10, joint[1] + py * 10);
    for (let k = 0; k < 3; k++) {
      const curled = at(92 - k * 6, 16 + k * 17);
      ctx.beginPath();
      ctx.arc(curled[0], curled[1], 9, 0, Math.PI);
      ctx.stroke();
    }
    const cuffA = at(-40, -32);
    const cuffB = at(-40, 32);
    ctx.lineWidth = 3;
    strokeLine(ctx, cuffA[0], cuffA[1], cuffB[0], cuffB[1]);
    ctx.restore();
    return wrist;
  }

  /** Penelope's hand closing around a wrist; `closing` 0→1 animates the grip. */
  function drawGrippingHand(ctx, wrist, closing) {
    const reach = easeOut(closing);
    const from = [wrist[0] - 700, wrist[1] + 520];
    const palm = lerpPoint(from, [wrist[0] - 10, wrist[1] + 34], reach);
    const direction = [palm[0] - from[0], palm[1] - from[1]];
    const length = Math.hypot(direction[0], direction[1]) || 1;
    const elbow = [palm[0] - (direction[0] / length) * 520, palm[1] - (direction[1] / length) * 520];
    ctx.save();
    ctx.strokeStyle = COLOR.ink;
    taperedStroke(ctx, elbow[0], elbow[1], lerp(elbow[0], palm[0], 0.55), lerp(elbow[1], palm[1], 0.55), 58, 44);
    ctx.strokeStyle = COLOR.bone;
    taperedStroke(ctx, lerp(elbow[0], palm[0], 0.5), lerp(elbow[1], palm[1], 0.5), palm[0], palm[1], 34, 28);
    ctx.fillStyle = COLOR.bone;
    fillEllipse(ctx, palm[0], palm[1], 50, 38, -0.5);
    for (let k = 0; k < 4; k++) {
      const baseX = palm[0] - 34 + k * 22;
      const baseY = palm[1] - 22 - k * 4;
      const curl = 26 + 40 * reach;
      ctx.strokeStyle = COLOR.bone;
      taperedPath(ctx, [[baseX, baseY], [baseX + 10, baseY - curl * 0.8], [baseX + 34 * reach, baseY - curl * 1.1]], 12, 9);
    }
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 2;
    for (let k = 0; k < 4; k++) {
      const baseX = palm[0] - 34 + k * 22;
      const baseY = palm[1] - 22 - k * 4;
      strokeLine(ctx, baseX - 6, baseY - 12, baseX + 10, baseY - 16);
    }
    const band = lerpPoint(palm, elbow, 0.2);
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, band[0], band[1], 16, 40, -0.6);
    ctx.fillStyle = COLOR.bone;
    for (let k = -2; k <= 2; k++) fillCircle(ctx, band[0] + k * 3, band[1] + k * 12, 3);
    ctx.restore();
  }

  function drawOwlEye(ctx, cx, cy, radius, t) {
    if (radius < 2) return;
    ctx.save();
    ctx.translate(cx, cy);
    ctx.strokeStyle = COLOR.bone;
    for (let ring = 0; ring < 5; ring++) {
      const r = radius * (0.46 + ring * 0.13);
      ctx.lineWidth = Math.max(1, radius * 0.012);
      ctx.setLineDash([radius * 0.09, radius * 0.05]);
      ctx.lineDashOffset = (ring % 2 ? 1 : -1) * t * radius * 0.6;
      ctx.beginPath();
      ctx.arc(0, 0, r, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.setLineDash([]);
    ctx.fillStyle = COLOR.ink;
    for (let i = 0; i < 36; i++) {
      const angle = (i / 36) * Math.PI * 2 + t * 0.4;
      fillEllipse(ctx, Math.cos(angle) * radius * 1.12, Math.sin(angle) * radius * 1.12, radius * 0.08, radius * 0.03, angle);
    }
    fillCircle(ctx, 0, 0, radius * 0.4);
    ctx.lineWidth = Math.max(1, radius * 0.01);
    for (let i = 0; i < 28; i++) {
      const angle = (i / 28) * Math.PI * 2 - t * 0.8;
      strokeLine(ctx, Math.cos(angle) * radius * 0.17, Math.sin(angle) * radius * 0.17, Math.cos(angle) * radius * 0.37, Math.sin(angle) * radius * 0.37);
    }
    fillCircle(ctx, 0, 0, radius * 0.14);
    ctx.fillStyle = COLOR.bone;
    fillCircle(ctx, -radius * 0.05, -radius * 0.05, radius * 0.035);
    ctx.restore();
  }

  function drawLockScreen(ctx, cx, cy, L) {
    const width = 390;
    const height = 800;
    const left = cx - width / 2;
    const top = cy - height / 2;
    ctx.save();
    ctx.fillStyle = '#0A0909';
    roundRectPath(ctx, left, top, width, height, 64);
    ctx.fill();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2.5;
    ctx.stroke();
    ctx.save();
    roundRectPath(ctx, left + 12, top + 12, width - 24, height - 24, 54);
    ctx.clip();
    const wallpaper = ctx.createLinearGradient(0, top, 0, top + height);
    wallpaper.addColorStop(0, '#141111');
    wallpaper.addColorStop(1, '#1D1614');
    ctx.fillStyle = wallpaper;
    ctx.fillRect(left, top, width, height);
    drawSea(ctx, { level: top + 560, amplitude: 18, wavelength: 96, phase: L * 0.15, fill: '#1B1412', line: '#3A2B26', left, right: left + width, bottom: top + height });
    ctx.fillStyle = '#000';
    roundRectPath(ctx, cx - 62, top + 24, 124, 36, 18);
    ctx.fill();
    ctx.save();
    ctx.shadowColor = COLOR.running;
    ctx.shadowBlur = 10;
    ctx.fillStyle = COLOR.running;
    fillCircle(ctx, cx + 42, top + 42, 5.5 + Math.sin(L * 5) * 0.8);
    ctx.restore();
    ctx.fillStyle = 'rgba(237, 227, 208, 0.8)';
    ctx.font = `600 17px ${FONT.ui}`;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    ctx.fillText('Aegean', left + 40, top + 43);
    for (let bar = 0; bar < 4; bar++) {
      ctx.fillStyle = bar === 0 ? COLOR.bone : 'rgba(237, 227, 208, 0.25)';
      ctx.fillRect(left + width - 70 + bar * 7, top + 49 - (bar + 1) * 4, 4.5, (bar + 1) * 4);
    }
    ctx.textAlign = 'center';
    ctx.fillStyle = 'rgba(237, 227, 208, 0.8)';
    ctx.font = `500 22px ${FONT.ui}`;
    ctx.fillText('Year 3 at sea', cx, top + 118);
    ctx.fillStyle = COLOR.bone;
    ctx.font = `200 124px ${FONT.ui}`;
    ctx.fillText('3:47', cx, top + 200);

    const cardTop = top + height - 290;
    ctx.fillStyle = 'rgba(30, 27, 26, 0.94)';
    roundRectPath(ctx, left + 18, cardTop, width - 36, 176, 30);
    ctx.fill();
    ctx.strokeStyle = 'rgba(237, 227, 208, 0.12)';
    ctx.lineWidth = 1;
    ctx.stroke();
    ctx.fillStyle = COLOR.ink;
    roundRectPath(ctx, left + 36, cardTop + 20, 46, 46, 12);
    ctx.fill();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(left + 59, cardTop + 43, 12, 0, Math.PI * 2);
    ctx.stroke();
    ctx.fillStyle = COLOR.thread;
    fillCircle(ctx, left + 59, cardTop + 43, 4);
    ctx.textAlign = 'left';
    ctx.fillStyle = COLOR.bone;
    ctx.font = `700 25px ${FONT.ui}`;
    ctx.fillText('Codex', left + 96, cardTop + 34);
    ctx.fillStyle = 'rgba(237, 227, 208, 0.62)';
    ctx.font = `500 17px ${FONT.ui}`;
    ctx.fillText('ship the harbour release', left + 96, cardTop + 60);
    ctx.textAlign = 'right';
    ctx.fillStyle = COLOR.running;
    ctx.font = `700 21px ${FONT.ui}`;
    ctx.fillText('Running', left + width - 40, cardTop + 34);
    const barLeft = left + 36;
    const barWidth = width - 72;
    const fraction = lerp(0.42, 0.63, easeInOut(progress(L, 0, 2)));
    ctx.fillStyle = '#2E2A29';
    roundRectPath(ctx, barLeft, cardTop + 92, barWidth, 10, 5);
    ctx.fill();
    ctx.save();
    ctx.shadowColor = COLOR.running;
    ctx.shadowBlur = 10;
    ctx.fillStyle = COLOR.running;
    roundRectPath(ctx, barLeft, cardTop + 92, barWidth * fraction, 10, 5);
    ctx.fill();
    ctx.restore();
    const continued = L > 1.15 ? 4 : 3;
    const flash = Math.exp(-Math.max(0, L - 1.15) * 5) * (L > 1.15 ? 1 : 0);
    ctx.textAlign = 'left';
    ctx.fillStyle = `rgba(237, 227, 208, ${0.55 + flash * 0.45})`;
    ctx.font = `500 16px ${FONT.ui}`;
    ctx.fillText(`auto-continued ×${continued}  ·  turn 14 of 40`, barLeft, cardTop + 138);
    ctx.fillStyle = 'rgba(237, 227, 208, 0.6)';
    roundRectPath(ctx, cx - 66, top + height - 34, 132, 6, 3);
    ctx.fill();
    ctx.restore();
    ctx.restore();
    return { left, top, width, height };
  }

  function drawHoldingHands(ctx, phone, front) {
    const { left, top, width, height } = phone;
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    ctx.strokeStyle = COLOR.ink;
    if (!front) {
      taperedStroke(ctx, left - 60, top + height + 260, left + 10, top + height - 120, 70, 56);
      taperedStroke(ctx, left + width + 60, top + height + 260, left + width - 10, top + height - 120, 70, 56);
      fillEllipse(ctx, left + 4, top + height - 170, 60, 110, 0.15);
      fillEllipse(ctx, left + width - 4, top + height - 170, 60, 110, -0.15);
      for (let k = 0; k < 3; k++) {
        taperedStroke(ctx, left + width - 20, top + height - 260 - k * 64, left + width + 18, top + height - 250 - k * 64, 17, 14);
      }
    } else {
      taperedStroke(ctx, left - 30, top + height - 110, left + 34, top + height - 250, 20, 16);
      ctx.strokeStyle = COLOR.bone;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(left + 20, top + height - 222, 10, 0.4, 2.4);
      ctx.stroke();
      ctx.strokeStyle = COLOR.ink;
      taperedStroke(ctx, left + width + 30, top + height - 100, left + width - 30, top + height - 220, 20, 16);
    }
    ctx.restore();
  }

  // ---------------------------------------------------------------- scenes
  // Each scene paints one panel; returning { dark: true } flips the borders to
  // glaze. `L` is scene-local seconds; puppet motion uses stepped(L).

  function paintLoopFrieze(ctx) {
    paintClay(ctx);
    const sea = 820;
    drawSea(ctx, { level: sea, wavelength: 160, amplitude: 24, phase: 0 });
    drawDolphin(ctx, 170, 930, 0.9, COLOR.terracotta, COLOR.ink);
    drawDolphin(ctx, 1840, 950, 0.7, COLOR.terracotta, COLOR.ink);
    const ship = drawShip(ctx, 420, sea + 4, 0.78, 0);
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(880, sea + 30);
    ctx.quadraticCurveTo(920, sea - 36, 1000, sea - 40);
    ctx.lineTo(1640, sea - 40);
    ctx.quadraticCurveTo(1700, sea - 30, 1740, sea + 30);
    ctx.closePath();
    ctx.fill();
    ctx.save();
    ctx.translate(1110, sea - 40);
    ctx.scale(0.8, 0.8);
    ctx.translate(-1110, -(sea - 40));
    const deskY = drawOliveDesk(ctx, 1110, sea - 40);
    const orb = drawLaptop(ctx, 1230, deskY, 0.42, { running: [true, true, true, true], pulse: 0.3 });
    ctx.restore();
    const orbWorld = [1110 + (orb[0] - 1110) * 0.8, sea - 40 + (orb[1] - (sea - 40)) * 0.8];
    const youth = drawPerson(ctx, { x: 1510, y: sea - 40, facing: 1, scale: 0.8, kind: { tunic: true }, pose: walkPose(1.1) });
    const knots = [[0, 560], ship.stern, [760, 470], orbWorld, [youth.hip[0] + 6, youth.hip[1] - 20], [WIDTH, 560]];
    drawThread(ctx, periodicCurvePoints(knots, WIDTH));
  }

  function scenePairing(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawChapter(ctx, 'ΙΘΑΚΗ');
    drawGroundLine(ctx, GROUND_Y, COLOR.ink);
    drawLoom(ctx, 150, GROUND_Y);
    drawPerson(ctx, {
      x: 520, y: GROUND_Y, look: LOOK.woman,
      kind: { female: true, dress: true, bun: true },
      pose: { head: 0.05, reachNear: [40, -300], reachFar: [30, -270] },
    });
    const tableTop = 690;
    drawTable(ctx, 930, tableTop, 360);
    const orb = drawLaptop(ctx, 930, tableTop, 0.64, { running: [true, true, true, true], pulse: L * 0.8 });
    const turned = T >= 2.5;
    const heroX = turned ? 1340 + (T - 2.5) * 190 : 1340;
    const hero = drawPerson(ctx, {
      x: heroX, y: GROUND_Y, facing: turned ? 1 : -1,
      kind: { beard: true, pilos: true, tunic: true },
      pose: turned ? walkPose(T * 9) : { head: -0.06, reachNear: [150, -400], reachFar: [20, -250] },
    });
    const paired = L >= 1.9;
    const phone = [hero.handNear[0], hero.handNear[1] - 18];
    drawHandheldPhone(ctx, phone[0], phone[1], 30, { glow: paired ? 'green' : 'bone', icon: paired ? 'running' : null });
    if (L > 0.35 && L < 1.95) {
      const grow = easeOut(progress(L, 0.35, 1.15));
      const collapse = easeInOut(progress(L, 1.4, 1.9));
      const center = lerpPoint(orb, phone, collapse);
      drawOwlEye(ctx, center[0], center[1], 250 * grow * (1 - collapse * 0.96), L);
    }
    if (paired) {
      drawThread(ctx, quadraticPoints(orb, phone, -170), easeOut(progress(L, 1.9, 2.35)));
      carveText(ctx, 'PAIRED', (orb[0] + phone[0]) / 2, 330, { size: 34, spacing: 14, reveal: progress(T, 2.0, 2.4) });
    }
  }

  function sceneCard(title, subtitle) {
    return function paintCard(ctx, L) {
      const T = stepped(L);
      paintClay(ctx);
      ctx.strokeStyle = COLOR.ink;
      ctx.lineWidth = 5;
      ctx.strokeRect(190, 210, WIDTH - 380, HEIGHT - 420);
      ctx.lineWidth = 2;
      ctx.strokeRect(206, 226, WIDTH - 412, HEIGHT - 452);
      carveText(ctx, title, WIDTH / 2, 510, { size: 112, spacing: 18, reveal: progress(T, 0.05, 0.6) });
      carveText(ctx, subtitle, WIDTH / 2, 640, { size: 38, spacing: 24, reveal: progress(T, 0.55, 1.0) });
    };
  }

  function sceneLotus(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawChapter(ctx, 'ΛΩΤΟΣ');
    drawGroundLine(ctx, GROUND_Y, COLOR.ink);
    [[250, 470], [400, 380], [1480, 420], [1640, 520], [1790, 400]].forEach(([x, height], i) => drawLotus(ctx, x, GROUND_Y, height, Math.sin(T * 2 + i) * 10));
    ctx.fillStyle = COLOR.ink;
    [560, 760].forEach((x) => fillEllipse(ctx, x - 6, GROUND_Y - 24, 54, 30));
    [560, 760].forEach((x, i) => drawPerson(ctx, {
      x, y: GROUND_Y, facing: 1, kind: { beard: i === 0, tunic: true, eyesClosed: true },
      hip: [0, -52],
      pose: { lean: 0.45, head: 0.55, footNear: [120, -4], footFar: [100, 0], reachNear: [110, -120], reachFar: [96, -110] },
    }));
    const hero = drawPerson(ctx, {
      x: 1120, y: GROUND_Y, facing: 1, kind: { beard: true, pilos: true, tunic: true },
      hip: [0, -44],
      pose: {
        lean: -1.15, head: 0.6,
        footNear: [210, -8], footFar: [196, -2],
        reachFar: [-150, -6],
        reachNear: (anchors) => [anchors.headCenter[0] + 44, anchors.headCenter[1] + 6],
        elbowNear: 1,
      },
    });
    drawLotusFlower(ctx, hero.handNear[0] + 8, hero.handNear[1] - 4, 30);
    drawHandheldPhone(ctx, 1380, GROUND_Y - 16, 26, { glow: 'green', icon: 'running' }, Math.PI / 2);
    drawStatusPill(ctx, 1390, GROUND_Y - 110, 'Codex · Running', { dot: COLOR.running, size: 22 });
  }

  function sceneCyclops(ctx, L) {
    const T = stepped(L);
    paintClay(ctx, true);
    ctx.fillStyle = COLOR.terracottaNight;
    ctx.beginPath();
    ctx.moveTo(1500, GROUND_Y);
    ctx.bezierCurveTo(1500, 380, 1640, 300, 1760, 300);
    ctx.bezierCurveTo(1880, 300, 1960, 420, 1960, GROUND_Y);
    ctx.closePath();
    ctx.fill();
    drawChapter(ctx, 'ΚΥΚΛΩΨ', true);
    drawGroundLine(ctx, GROUND_Y, COLOR.terracotta);
    ctx.save();
    ctx.translate(930, GROUND_Y);
    ctx.scale(0.32, 0.32);
    drawFireSpirit(ctx, 0, 0, T, COLOR.terracotta, COLOR.ink);
    ctx.restore();
    ctx.fillStyle = COLOR.terracottaNight;
    [-70, -24, 24, 70].forEach((offset) => fillEllipse(ctx, 930 + offset, GROUND_Y - 8, 24, 14));
    drawPerson(ctx, {
      x: 420, y: GROUND_Y, facing: 1, scale: 1.7, look: LOOK.red,
      kind: { beard: true, cyclops: true },
      hip: [0, -110],
      pose: { lean: 0.28, head: 0.1, footNear: [150, -6], footFar: [120, -2], reachNear: [150, -120], reachFar: [110, -110] },
      decorate: (g) => {
        g.strokeStyle = COLOR.terracotta;
        taperedStroke(g, 150, -120, 250, -290, 6, 11);
      },
    });
    const hero = drawPerson(ctx, {
      x: 1250, y: GROUND_Y, facing: -1, look: LOOK.red,
      kind: { beard: true, pilos: true, tunic: true },
      pose: { head: 0.35, reachNear: [52, -290], reachFar: [40, -280] },
    });
    drawHandheldPhone(ctx, hero.handNear[0], hero.handNear[1] - 16, 30, { glow: 'bone', icon: 'queued' });
    drawStatusPill(ctx, hero.handNear[0] + 20, hero.handNear[1] - 150, 'queued · ship the migration', { size: 21, font: FONT.mono });
    return { dark: true };
  }

  function sceneCirce(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawChapter(ctx, 'ΚΙΡΚΗ');
    drawGroundLine(ctx, GROUND_Y, COLOR.ink);
    const couchTop = drawKline(ctx, 820, GROUND_Y, 520, 190);
    const hero = drawPerson(ctx, {
      x: 700, y: couchTop, facing: 1, kind: { beard: true, pilos: true, tunic: true },
      hip: [0, -30],
      pose: {
        lean: -0.62, head: 0.45,
        footNear: [250, -4], footFar: [230, 2],
        reachFar: [-100, -10],
        reachNear: [40, -250],
      },
    });
    drawKylix(ctx, hero.handNear[0], hero.handNear[1] - 16, 32);
    drawPerson(ctx, {
      x: 1480, y: GROUND_Y, facing: -1, look: LOOK.woman,
      kind: { female: true, dress: true, bun: true },
      pose: { head: 0.1, reachNear: [80, -250], reachFar: [60, -300] },
    });
    drawTable(ctx, 1200, 700, 200);
    drawHandheldPhone(ctx, 1200, 690, 26, { glow: L > 0.55 ? 'green' : 'bone', icon: L > 0.55 ? 'running' : null }, Math.PI / 2);
    if (L < 0.55) {
      drawStatusPill(ctx, 1200, 600, 'Claude · run cargo test?   Allow', { size: 21 });
    } else {
      drawStatusPill(ctx, 1200, 600, 'Allowed · Running', { dot: COLOR.running, size: 21 });
      const ripple = progress(L, 0.55, 0.95);
      ctx.save();
      ctx.strokeStyle = `rgba(237, 227, 208, ${1 - ripple})`;
      ctx.lineWidth = 3;
      ctx.beginPath();
      ctx.arc(1200, 690, 20 + ripple * 70, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
    }
    ctx.save();
    ctx.fillStyle = COLOR.ink;
    fillEllipse(ctx, 1140, 688, 28, 10);
    fillEllipse(ctx, 1262, 684, 22, 14);
    ctx.restore();
    void T;
  }

  const MONTAGE = [sceneLotus, sceneCyclops, sceneCirce];
  const MONTAGE_CUTS_PER_SECOND = 15;

  function sceneMontage(ctx, L) {
    const index = Math.floor(L * MONTAGE_CUTS_PER_SECOND) % MONTAGE.length;
    return MONTAGE[index](ctx, 0.9);
  }

  function sceneLockScreen(ctx, L) {
    paintClay(ctx);
    const phone = { left: WIDTH / 2 - 195, top: 540 - 400, width: 390, height: 800 };
    drawHoldingHands(ctx, phone, false);
    drawLockScreen(ctx, WIDTH / 2, 540, L);
    drawHoldingHands(ctx, phone, true);
    carveText(ctx, 'ΛΩΤΟΣ · ΕΤΟΣ Γ', 380, 540, { size: 34, spacing: 12, alpha: 0.9 });
    carveText(ctx, 'NOBODY AT THE DESK', WIDTH - 380, 540, { size: 30, spacing: 10, alpha: 0.9 });
  }

  const SIREN_WORDS = ['come home…', 'open the laptop…', 'fix it yourself…', 'nobody does it like you…', 'just check once…', 'come home…', 'it needs you…', 'open it…'];

  function sceneSirens(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawChapter(ctx, 'ΣΕΙΡΗΝΕΣ');
    const seaLevel = 800;
    const ship = drawShip(ctx, 840 + T * 18, seaLevel + Math.sin(T * 1.6) * 6, 0.92, T, LOOK.black, { sail: false });
    const hero = drawPerson(ctx, {
      x: ship.deck[0] - 20, y: ship.deck[1] + 4, facing: 1,
      kind: { beard: true, pilos: true, tunic: true, eyesClosed: true },
      pose: { head: 0.28, reachNear: [-28, -150], reachFar: [-32, -162], elbowNear: 1, elbowFar: 1 },
      decorate: (g) => {
        g.strokeStyle = COLOR.bone;
        g.lineWidth = 2.6;
        [-190, -160, -120].forEach((y) => strokeLine(g, -44, y, 34, y + 18));
      },
    });
    drawHandheldPhone(ctx, hero.hip[0] + 30, hero.hip[1] + 10, 18, { icon: 'muted' });
    drawSea(ctx, { level: seaLevel + 30, wavelength: 170, amplitude: 28, phase: T * 0.25 });

    const domeRadius = 215;
    const dome = hero.head;
    ctx.save();
    ctx.strokeStyle = 'rgba(237, 227, 208, 0.55)';
    ctx.lineWidth = 2.5;
    ctx.setLineDash([14, 12]);
    ctx.lineDashOffset = -L * 30;
    ctx.beginPath();
    ctx.arc(dome[0], dome[1], domeRadius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    drawBellSlash(ctx, dome[0], dome[1] - domeRadius - 34, 36, COLOR.bone);

    const approach = easeInOut(progress(L, 0, 6));
    const sirenSpots = [
      [lerp(300, 420, approach), 330 + Math.sin(T * 2.1) * 24, 1],
      [lerp(1560, 1420, approach), 280 + Math.sin(T * 1.7 + 1) * 24, -1],
      [lerp(1780, 1620, approach), 560 + Math.sin(T * 2.4 + 2) * 20, -1],
    ];
    const mouths = sirenSpots.map(([x, y, facing], i) => drawSiren(ctx, x, y, 1.05, Math.sin(T * 9 + i * 2), facing, true));

    ctx.save();
    ctx.beginPath();
    ctx.rect(0, 0, WIDTH, HEIGHT);
    ctx.arc(dome[0], dome[1], domeRadius, 0, Math.PI * 2, true);
    ctx.clip('evenodd');
    ctx.strokeStyle = 'rgba(237, 227, 208, 0.6)';
    ctx.lineWidth = 3;
    mouths.forEach((mouth) => {
      const toward = Math.atan2(dome[1] - mouth[1], dome[0] - mouth[0]);
      for (let k = 0; k < 4; k++) {
        const radius = (L * 170 + k * 90) % 360 + 20;
        ctx.globalAlpha = 1 - radius / 380;
        ctx.beginPath();
        ctx.arc(mouth[0], mouth[1], radius, toward - 0.42, toward + 0.42);
        ctx.stroke();
      }
    });
    ctx.restore();

    SIREN_WORDS.forEach((word, index) => {
      const spawn = 0.3 + index * 0.66;
      const age = L - spawn;
      if (age < 0 || age > 1.9) return;
      const mouth = mouths[index % mouths.length];
      const flight = easeIn(clamp(age / 1.3)) * 0.6 + clamp(age / 1.3) * 0.4;
      let position = lerpPoint(mouth, dome, flight);
      const distance = Math.hypot(position[0] - dome[0], position[1] - dome[1]);
      let shatter = 0;
      if (distance < domeRadius + 10) {
        const angle = Math.atan2(position[1] - dome[1], position[0] - dome[0]);
        position = [dome[0] + Math.cos(angle) * (domeRadius + 10), dome[1] + Math.sin(angle) * (domeRadius + 10)];
        const impact = 0.3 + 0.7 * ((Math.hypot(mouth[0] - dome[0], mouth[1] - dome[1]) - domeRadius - 10) / Math.hypot(mouth[0] - dome[0], mouth[1] - dome[1]));
        shatter = clamp((age / 1.3 - impact) / 0.35);
      }
      ctx.save();
      ctx.font = `italic 600 34px ${FONT.script}`;
      ctx.letterSpacing = `${shatter * 26}px`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.globalAlpha = 1 - shatter;
      ctx.fillStyle = 'rgba(20, 16, 16, 0.55)';
      ctx.fillText(word, position[0] + 2, position[1] + 2);
      ctx.fillStyle = COLOR.bone;
      ctx.fillText(word, position[0], position[1]);
      ctx.restore();
    });
  }

  function proteusHitIndex(L) {
    let index = -1;
    PROTEUS_HITS_LOCAL.forEach((hit, i) => {
      if (L >= hit) index = i;
    });
    return index;
  }

  function proteusHitDecay(L) {
    const index = proteusHitIndex(L);
    return index < 0 ? 0 : Math.exp(-(L - PROTEUS_HITS_LOCAL[index]) * 9);
  }

  function sceneProteus(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawChapter(ctx, 'ΠΡΩΤΕΥΣ');
    drawGroundLine(ctx, GROUND_Y, COLOR.ink);
    const index = proteusHitIndex(L);
    const shapeX = 1180;
    const shapes = [drawLion, drawSerpent, drawWaterSpirit, drawFireSpirit];
    if (index < 0) drawOldManOfTheSea(ctx, shapeX - 60, GROUND_Y, T);
    else shapes[index](ctx, shapeX, GROUND_Y, T);
    drawPerson(ctx, {
      x: 560, y: GROUND_Y, facing: 1,
      kind: { beard: true, helmet: true, tunic: true },
      pose: {
        lean: 0.42, head: -0.2,
        legNear: [0.62, -0.62], legFar: [-0.5, 0.1],
        reachNear: [300, -250], reachFar: [290, -300],
      },
    });
    drawStatusPill(ctx, shapeX, 196, 'session · ody-7f3a', { dot: COLOR.running, size: 24, font: FONT.mono });
    if (index >= 0) carveText(ctx, SESSION_NAMES[index], shapeX, 950, { size: 50, spacing: 22 });
    if (L > 3.35) carveText(ctx, 'SAME SESSION.', shapeX, 280, { size: 58, spacing: 16, reveal: progress(T, 3.35, 3.75) });
    const flash = proteusHitDecay(L);
    if (flash > 0.01) {
      ctx.fillStyle = `rgba(237, 227, 208, ${0.55 * flash})`;
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }
  }

  function sceneStorm(ctx, L) {
    const snapped = L >= STORM_SNAP_LOCAL;
    const silenceEnd = STORM_SNAP_LOCAL + 1;
    const frozen = L < STORM_SNAP_LOCAL ? stepped(L) : L < silenceEnd ? STORM_SNAP_LOCAL : STORM_SNAP_LOCAL + (stepped(L) - silenceEnd) * 0.45;
    paintClay(ctx, true);
    drawChapter(ctx, 'ΚΑΤΑΙΓΙΣ', true);
    const lightning = [0.35, 0.85, 2.9].map((at) => Math.exp(-Math.max(0, L - at) * 14) * (L >= at ? 1 : 0));
    const flash = Math.max(lightning[0], lightning[1], lightning[2] * 0.4);
    if (flash > 0.02) {
      ctx.fillStyle = `rgba(237, 227, 208, ${0.3 * flash})`;
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
      const random = seededRandom(lightning[0] > lightning[1] ? 3 : 4);
      ctx.save();
      ctx.strokeStyle = COLOR.bone;
      ctx.lineWidth = 5;
      ctx.shadowColor = COLOR.bone;
      ctx.shadowBlur = 20;
      ctx.globalAlpha = flash;
      ctx.beginPath();
      let x = 1260;
      ctx.moveTo(x, 70);
      for (let y = 70; y < 640; y += 60) {
        x += (random() - 0.5) * 120;
        ctx.lineTo(x, y);
      }
      ctx.stroke();
      ctx.restore();
    }
    const seaLevel = 720;
    const ship = drawShip(ctx, 560, seaLevel + Math.sin(frozen * 2.6) * 14, 0.72, frozen, LOOK.red, { tilt: Math.sin(frozen * 2.2) * 0.13, sail: false });
    const home = [WIDTH + 40, 470];
    if (!snapped) {
      const tension = Math.sin(L * 42) * 5 * progress(L, 0, 1);
      drawThread(ctx, quadraticPoints(ship.stern, home, 34 + tension));
    } else {
      const dt = L - STORM_SNAP_LOCAL;
      const breakPoint = pointAlong(quadraticPoints(ship.stern, home, 34), 0.58);
      const recoil = 1 - Math.exp(-dt * 7) * Math.cos(dt * 16);
      const fall = Math.min(1, dt * 0.7);
      const leftEnd = [lerp(breakPoint[0], ship.stern[0], 0.42 * recoil), breakPoint[1] + 280 * fall * fall + 40 * recoil];
      const rightEnd = [lerp(breakPoint[0], home[0], 0.36 * recoil), breakPoint[1] + 320 * fall * fall + 40 * recoil];
      drawThread(ctx, quadraticPoints(ship.stern, leftEnd, 60 + 150 * fall));
      drawThread(ctx, quadraticPoints(rightEnd, home, 60 + 170 * fall));
      if (dt < 0.2) {
        ctx.save();
        ctx.strokeStyle = COLOR.bone;
        ctx.lineWidth = 2;
        ctx.globalAlpha = 1 - dt / 0.2;
        for (let i = 0; i < 10; i++) {
          const angle = (i / 10) * Math.PI * 2;
          strokeLine(ctx, breakPoint[0] + Math.cos(angle) * 10, breakPoint[1] + Math.sin(angle) * 10, breakPoint[0] + Math.cos(angle) * (20 + dt * 300), breakPoint[1] + Math.sin(angle) * (20 + dt * 300));
        }
        ctx.restore();
      }
    }
    drawSea(ctx, { level: seaLevel + 40, amplitude: 78, wavelength: 280, phase: frozen * 0.55, fill: COLOR.terracotta, line: COLOR.ink });
    if (L < STORM_SNAP_LOCAL || L >= silenceEnd) {
      const random = seededRandom(Math.floor(frozen * PUPPET_FPS) + 101);
      ctx.save();
      ctx.strokeStyle = 'rgba(237, 227, 208, 0.3)';
      ctx.lineWidth = 2;
      for (let i = 0; i < 90; i++) {
        const x = random() * (WIDTH + 200);
        const y = 70 + random() * 800;
        strokeLine(ctx, x, y, x - 28, y + 64);
      }
      ctx.restore();
    }
    if (L >= silenceEnd) {
      ctx.fillStyle = `rgba(0, 0, 0, ${0.35 * progress(L, silenceEnd, silenceEnd + 1)})`;
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }
    return { dark: true };
  }

  function heartbeatPulse(L) {
    let pulse = 0;
    for (let beat = NIGHT.heartbeatStart; beat < 7; beat += NIGHT.heartbeatPeriod) {
      pulse += Math.exp(-Math.pow((L - beat) / 0.05, 2)) * 0.012 + Math.exp(-Math.pow((L - beat - 0.18) / 0.05, 2)) * 0.007;
    }
    return pulse;
  }

  function sceneNightDesk(ctx, L) {
    const T = stepped(L);
    paintClay(ctx, true);
    drawChapter(ctx, 'ΙΘΑΚΗ · ΝΥΞ', true);
    const screen = { x: 330, y: 170, w: 1260, h: 400 };
    const healed = L >= NIGHT.healEnd;
    const healProgress = progress(L, NIGHT.healStart, NIGHT.healEnd);

    const spill = ctx.createRadialGradient(960, 600, 100, 960, 700, 900);
    spill.addColorStop(0, 'rgba(237, 227, 208, 0.10)');
    spill.addColorStop(1, 'rgba(237, 227, 208, 0)');
    ctx.fillStyle = spill;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);

    ctx.fillStyle = COLOR.terracottaNight;
    fillPolygon(ctx, [[250, 616], [1670, 616], [1900, 1040], [20, 1040]]);
    const keyRows = 5;
    const keyColumns = 14;
    const keyCorner = (column, row) => {
      const y = lerp(646, 1000, row / keyRows);
      const leftEdge = lerp(270, 70, (y - 616) / 424);
      const rightEdge = lerp(1650, 1850, (y - 616) / 424);
      return [lerp(leftEdge, rightEdge, column / keyColumns), y];
    };
    let powerKey = null;
    ctx.fillStyle = COLOR.ink;
    for (let row = 0; row < keyRows; row++) {
      for (let column = 0; column < keyColumns; column++) {
        const a = keyCorner(column + 0.08, row + 0.1);
        const b = keyCorner(column + 0.92, row + 0.1);
        const c = keyCorner(column + 0.92, row + 0.85);
        const d = keyCorner(column + 0.08, row + 0.85);
        fillPolygon(ctx, [a, b, c, d]);
        if (row === 0 && column === keyColumns - 1) powerKey = [(a[0] + c[0]) / 2, (a[1] + c[1]) / 2];
      }
    }
    ctx.save();
    ctx.strokeStyle = COLOR.bone;
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.arc(powerKey[0], powerKey[1] + 2, 16, -Math.PI * 0.3, Math.PI * 1.3);
    ctx.stroke();
    strokeLine(ctx, powerKey[0], powerKey[1] - 18, powerKey[0], powerKey[1] - 2);
    ctx.restore();

    ctx.fillStyle = '#050404';
    roundRectPath(ctx, screen.x - 20, screen.y - 20, screen.w + 40, screen.h + 40, 24);
    ctx.fill();
    ctx.strokeStyle = 'rgba(237, 227, 208, 0.4)';
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.fillStyle = COLOR.screen;
    ctx.fillRect(screen.x, screen.y, screen.w, screen.h);
    ctx.save();
    ctx.font = `500 22px ${FONT.mono}`;
    ctx.textBaseline = 'middle';
    ctx.fillStyle = 'rgba(237, 227, 208, 0.6)';
    ctx.fillText('session stream', screen.x + 60, screen.y + 58);
    ctx.textAlign = 'right';
    ctx.fillText('seq 4812 → 4907', screen.x + screen.w - 60, screen.y + 58);
    const cells = 44;
    const gapStart = 18;
    const gapEnd = 30;
    for (let i = 0; i < cells; i++) {
      const x = screen.x + 60 + i * 26;
      const y = screen.y + 110;
      const inGap = i >= gapStart && i < gapEnd;
      const filledAt = (i - gapStart + 1) / (gapEnd - gapStart);
      if (inGap && healProgress < filledAt) {
        const blink = 0.3 + 0.3 * Math.sin(L * 9);
        ctx.strokeStyle = `rgba(237, 227, 208, ${blink})`;
        ctx.lineWidth = 1.5;
        ctx.setLineDash([4, 4]);
        ctx.strokeRect(x, y, 18, 18);
        ctx.setLineDash([]);
      } else {
        const fresh = inGap ? Math.exp(-(L - (NIGHT.healStart + filledAt * (NIGHT.healEnd - NIGHT.healStart))) * 6) : 0;
        ctx.fillStyle = `rgba(237, 227, 208, ${0.75 + 0.25 * fresh})`;
        ctx.fillRect(x, y, 18, 18);
      }
    }
    ctx.textAlign = 'left';
    ctx.font = `600 26px ${FONT.mono}`;
    ctx.fillStyle = COLOR.bone;
    const status = healed ? 'recovered from snapshot · nothing lost' : L >= NIGHT.healStart ? 'fetching snapshot…' : 'gap · 95 events missing';
    ctx.fillText(status, screen.x + 60, screen.y + 176);
    SESSION_NAMES.forEach((name, row) => {
      const y = screen.y + 236 + row * 40;
      if (healed) {
        ctx.save();
        ctx.shadowColor = COLOR.running;
        ctx.shadowBlur = 10;
        ctx.fillStyle = COLOR.running;
        fillCircle(ctx, screen.x + 72, y, 8);
        ctx.restore();
      } else {
        ctx.strokeStyle = 'rgba(237, 227, 208, 0.5)';
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.arc(screen.x + 72, y, 8, 0, Math.PI * 2);
        ctx.stroke();
      }
      ctx.font = `500 22px ${FONT.mono}`;
      ctx.fillStyle = 'rgba(237, 227, 208, 0.8)';
      ctx.fillText(name.toLowerCase(), screen.x + 100, y);
      ctx.fillStyle = healed ? COLOR.running : 'rgba(237, 227, 208, 0.45)';
      ctx.fillText(healed ? 'running' : 'waiting', screen.x + 260, y);
    });
    ctx.restore();

    const approach = easeOut(progress(T, 0.1, 2.2));
    const lift = easeInOut(progress(L, NIGHT.release, NIGHT.release + 0.7));
    const tremble = L < NIGHT.grab ? Math.sin(L * 37) * 2.5 + Math.sin(L * 23) * 1.5 : 0;
    const tip = [powerKey[0] + tremble + lift * 60, powerKey[1] - lerp(260, 34, approach) - lift * 240];
    const wrist = drawPointingHand(ctx, tip, [-0.42, 0.91], COLOR.terracotta, COLOR.ink);
    if (L >= NIGHT.grab) {
      drawGrippingHand(ctx, [wrist[0] - 60, wrist[1] - 70], progress(L, NIGHT.grab, NIGHT.grab + 0.22));
    }
    if (L >= NIGHT.wait) {
      carveText(ctx, 'WAIT.', 520, 800, { size: 170, spacing: 24, surface: 'glaze', reveal: progress(T, NIGHT.wait, NIGHT.wait + 0.3) });
    }
    return { dark: true };
  }

  function arrowTipX(L) {
    return L < ARROW.releaseLocal ? ARROW.startX : ARROW.startX + (L - ARROW.releaseLocal) * ARROW.speed;
  }

  function ringX(index) {
    return ARROW.ringStartX + index * ARROW.ringSpacing;
  }

  function ringPassLocal(index) {
    return ARROW.releaseLocal + (ringX(index) - ARROW.startX) / ARROW.speed;
  }

  function sceneAxes(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    const worldWidth = ringX(ARROW.ringCount - 1) + 500;
    const tipX = arrowTipX(L);
    const camera = clamp(tipX - 760, 0, worldWidth - WIDTH);
    const holeY = ARROW.holeY;

    ctx.save();
    ctx.translate(-camera * 0.55, 0);
    ctx.fillStyle = COLOR.terracottaShadow;
    for (let i = 0; i < 8; i++) {
      const x = 140 + i * 420;
      ctx.fillRect(x, 150, 50, GROUND_Y - 150);
      ctx.fillRect(x - 16, 136, 82, 22);
    }
    ctx.restore();

    ctx.save();
    ctx.translate(-camera, 0);
    drawGroundLine(ctx, GROUND_Y, COLOR.ink, -100, worldWidth + 100);
    const released = L >= ARROW.releaseLocal;
    const nock = released ? [300, holeY] : [300 - easeOut(progress(T, 0, 0.5)) * 70, holeY];
    const beggar = drawPerson(ctx, {
      x: 170, y: GROUND_Y, facing: 1,
      kind: { beard: true, tunic: true },
      pose: {
        lean: 0.05, head: 0,
        legNear: [0.34, -0.1], legFar: [-0.3, -0.04],
        reachFar: [200, holeY - GROUND_Y],
        reachNear: [nock[0] - 170 - (released ? -30 : 0), holeY - GROUND_Y + 4],
      },
    });
    const grip = beggar.handFar;
    const bowTop = [grip[0] - 20, grip[1] - 150];
    const bowBottom = [grip[0] - 20, grip[1] + 150];
    ctx.strokeStyle = COLOR.ink;
    const bowPoints = [];
    for (let i = 0; i <= 16; i++) {
      const u = i / 16;
      bowPoints.push([lerp(bowTop[0], bowBottom[0], u) + Math.sin(u * Math.PI) * 44, lerp(bowTop[1], bowBottom[1], u)]);
    }
    for (let i = 0; i < bowPoints.length - 1; i++) {
      ctx.lineWidth = 2 * lerp(4, 9, Math.sin(((i + 0.5) / 16) * Math.PI));
      strokeLine(ctx, bowPoints[i][0], bowPoints[i][1], bowPoints[i + 1][0], bowPoints[i + 1][1]);
    }
    const vibration = released ? Math.sin((L - ARROW.releaseLocal) * 70) * 22 * Math.exp(-(L - ARROW.releaseLocal) * 4) : 0;
    const stringMiddle = released ? [bowTop[0] + vibration, holeY] : [beggar.handNear[0], beggar.handNear[1]];
    drawThread(ctx, [bowTop, stringMiddle, bowBottom], 1, 3);

    for (let i = 0; i < ARROW.ringCount; i++) drawAxe(ctx, ringX(i), holeY, GROUND_Y, COLOR.terracotta);

    const tailX = released ? tipX - 170 : nock[0];
    if (released) drawThread(ctx, [[stringMiddle[0], stringMiddle[1]], [tailX, holeY]], 1, 3);
    drawArrow(ctx, released ? tipX : nock[0] + 170, holeY);
    let passed = 0;
    for (let i = 0; i < ARROW.ringCount; i++) {
      if (tipX > ringX(i)) {
        drawAxeRingFront(ctx, ringX(i), holeY);
        passed += 1;
      }
      const pop = progress(L, ringPassLocal(i), ringPassLocal(i) + 0.35);
      if (pop > 0) drawCheckBadge(ctx, ringX(i), holeY - 150, 30, easeOutBack(pop));
    }
    ctx.restore();

    drawChapter(ctx, 'ΤΟΞΟΝ');
    const label = passed === ARROW.ringCount ? '12 / 12 · shipped' : `${passed} / 12`;
    drawStatusPill(ctx, WIDTH - 160, 960, label, { dot: passed ? COLOR.running : null, size: 26, align: 'right', font: FONT.mono });
  }

  function sceneContinue(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    const horizon = 640;
    drawSun(ctx, 240, horizon, 120);
    drawSea(ctx, { level: horizon, amplitude: 12, wavelength: 120, phase: T * 0.1, curls: false });
    ctx.fillStyle = COLOR.ink;
    ctx.beginPath();
    ctx.moveTo(1420, horizon + 20);
    ctx.quadraticCurveTo(1560, horizon - 150, 1720, horizon - 140);
    ctx.quadraticCurveTo(1860, horizon - 130, 1960, horizon - 40);
    ctx.lineTo(1960, horizon + 20);
    ctx.closePath();
    ctx.fill();
    const arrived = L >= CONTINUE_ARRIVAL_LOCAL;
    const glow = arrived ? 1 - Math.exp(-(L - CONTINUE_ARRIVAL_LOCAL) * 5) * 0.5 : 0;
    const houseWindow = drawHouse(ctx, 1700, horizon - 136, 0.62, glow);
    const ship = drawShip(ctx, 390, horizon + 2, 0.24, T, LOOK.black, { sail: true });
    const path = quadraticPoints(ship.stern, houseWindow, -560, 96);
    drawThread(ctx, path, easeInOut(progress(L, 0.05, 0.5)));
    const travel = easeInOut(progress(L, 0.4, CONTINUE_ARRIVAL_LOCAL));
    if (L >= 0.4 && !arrived) {
      const at = pointAlong(path, travel);
      ctx.save();
      ctx.font = `italic 600 52px ${FONT.script}`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.shadowColor = 'rgba(237, 227, 208, 0.8)';
      ctx.shadowBlur = 16;
      ctx.fillStyle = COLOR.bone;
      ctx.fillText('continue.', at[0], at[1] - 44);
      ctx.restore();
    }
    if (arrived) {
      SESSION_NAMES.forEach((_, i) => drawCheckBadge(ctx, houseWindow[0] - 105 + i * 70, houseWindow[1] - 150, 22, easeOutBack(progress(L, CONTINUE_ARRIVAL_LOCAL + i * 0.08, CONTINUE_ARRIVAL_LOCAL + 0.3 + i * 0.08))));
      carveText(ctx, 'continue.', WIDTH / 2, 300, { size: 64, font: FONT.script, style: 'italic', weight: 600, reveal: progress(T, CONTINUE_ARRIVAL_LOCAL, CONTINUE_ARRIVAL_LOCAL + 0.4) });
    }
  }

  function sceneLeave(ctx, L) {
    const T = stepped(L);
    paintClay(ctx);
    drawGroundLine(ctx, GROUND_Y, COLOR.ink, 0, 1640);
    drawSea(ctx, { level: GROUND_Y + 10, left: 1600, amplitude: 22, wavelength: 150, phase: T * 0.3 });
    const deskY = drawOliveDesk(ctx, 380, GROUND_Y);
    drawLaptop(ctx, 520, deskY, 0.5, { running: [true, true, true, true], pulse: L * 0.8 });
    const standAt = 0.5;
    const walkStart = 1.0;
    const walkEnd = 3.4;
    let spec;
    if (T < standAt) {
      spec = {
        x: 820, y: GROUND_Y, facing: -1, kind: { tunic: true }, hip: [0, -170],
        pose: { lean: 0.2, footNear: [40, -4], footFar: [20, 0], reachNear: [150, -260], reachFar: [130, -250] },
      };
    } else if (T < walkStart) {
      spec = { x: 830, y: GROUND_Y, facing: 1, kind: { tunic: true }, pose: { head: -0.05, reachNear: [10, -190], reachFar: [-10, -230] } };
    } else if (T < walkEnd) {
      const x = lerp(830, 1520, progress(T, walkStart, walkEnd));
      spec = { x, y: GROUND_Y, facing: 1, kind: { tunic: true }, pose: walkPose(T * 8.5) };
    } else {
      spec = { x: 1520, y: GROUND_Y, facing: 1, kind: { tunic: true }, pose: { head: -0.12, reachNear: [0, -200], reachFar: [-20, -210] } };
    }
    if (T < standAt) {
      ctx.fillStyle = COLOR.ink;
      ctx.fillRect(760, GROUND_Y - 176, 110, 14);
      ctx.fillRect(772, GROUND_Y - 170, 10, 170);
      ctx.fillRect(848, GROUND_Y - 170, 10, 170);
    }
    drawPerson(ctx, spec);
  }

  // ----------------------------------------------------------- main layers
  function drawColdOpen(ctx, L) {
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    if (L < 1.5 || L > 2.9) return;
    const halfWidth = 880 * easeOut(progress(L, 1.5, 1.66));
    ctx.globalAlpha = 1 - progress(L, 2.0, 2.9);
    drawThread(ctx, [[WIDTH / 2 - halfWidth, HEIGHT / 2], [WIDTH / 2 + halfWidth, HEIGHT / 2]], 1, 2.6);
    ctx.globalAlpha = 1;
  }

  function drawLogo(ctx, L) {
    const clay = surfaces.panelA.getContext('2d');
    clay.setTransform(1, 0, 0, 1, 0, 0);
    paintClay(clay);
    drawMeanderStrip(clay, 170, 52, COLOR.terracotta, COLOR.ink);
    drawRayStrip(clay, HEIGHT - 222, 52, COLOR.terracotta, COLOR.ink);
    const T = stepped(L);
    carveText(clay, 'ΛΟΟΠΕΡ', WIDTH / 2, 470, { size: 230, spacing: 54, reveal: progress(T, 0.1, 0.8) });
    carveText(clay, 'LOOPER', WIDTH / 2, 640, { size: 34, spacing: 34, reveal: progress(T, 0.7, 1.0) });
    carveText(clay, 'The work goes on. So can you.', WIDTH / 2, 745, { size: 54, font: FONT.script, style: 'italic', weight: 600, reveal: progress(T, 1.05, 1.7) });
    const threadReveal = easeInOut(progress(L, 0.45, 1.35));
    const threadPoints = [];
    for (let i = 0; i <= 80; i++) {
      const u = i / 80;
      threadPoints.push([lerp(520, 1330, u), 580 + Math.sin(u * Math.PI * 2) * 6]);
    }
    for (let i = 0; i <= 40; i++) {
      const angle = Math.PI / 2 - (i / 40) * Math.PI * 2;
      threadPoints.push([1330 + Math.cos(angle) * 36 + 0, 544 + Math.sin(angle) * 36]);
    }
    drawThread(clay, threadPoints, threadReveal, 4);
    clay.globalCompositeOperation = 'multiply';
    clay.drawImage(surfaces.clay, 0, 0);
    clay.globalCompositeOperation = 'source-over';
    ctx.drawImage(surfaces.panelA, 0, 0);
    ctx.globalCompositeOperation = 'multiply';
    ctx.drawImage(surfaces.shading, 0, 0, WIDTH, HEIGHT);
    ctx.globalCompositeOperation = 'source-over';
    const appear = easeOut(progress(L, 0, 0.6));
    ctx.fillStyle = `rgba(0, 0, 0, ${1 - appear})`;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    const slide = easeIn(progress(L, 2.0, 2.35));
    if (slide > 0) {
      const edge = WIDTH * (1 - slide);
      ctx.fillStyle = '#000';
      ctx.fillRect(edge, 0, WIDTH - edge + 1, HEIGHT);
      const shadow = ctx.createLinearGradient(edge - 40, 0, edge, 0);
      shadow.addColorStop(0, 'rgba(0, 0, 0, 0)');
      shadow.addColorStop(1, 'rgba(0, 0, 0, 0.6)');
      ctx.fillStyle = shadow;
      ctx.fillRect(edge - 40, 0, 40, HEIGHT);
    }
  }

  // ------------------------------------------------------------- timeline
  const SCENES = [
    { id: 'coldOpen', start: 0, end: 3, main: drawColdOpen },
    {
      id: 'vaseReveal', start: 3, end: 7, panel: paintLoopFrieze,
      rotation: (L) => easeOut(progress(L, 0, 3.4)),
      view: (L) => ({ zoom: zoomTween(1, FULL_ZOOM, easeInOut(progress(L, 2.55, 3.85))), light: easeInOut(progress(L, 0, 2.2)) }),
    },
    { id: 'pairing', start: 7, end: 10, turnIn: 0.5, panel: scenePairing },
    { id: 'cardTenYears', start: 10, end: 12, turnIn: 0.45, panel: sceneCard('TEN YEARS AT SEA.', 'ΔΕΚΑ ΕΤΗ ΣΤΗ ΘΑΛΑΣΣΑ') },
    { id: 'lotus', start: 12, end: 13.2, turnIn: 0.32, panel: sceneLotus },
    { id: 'cyclops', start: 13.2, end: 14.4, turnIn: 0.26, panel: sceneCyclops },
    { id: 'circe', start: 14.4, end: 15.6, turnIn: 0.22, panel: sceneCirce },
    { id: 'montage', start: 15.6, end: 16, turnIn: 0.12, panel: sceneMontage },
    { id: 'lockScreen', start: 16, end: 18, turnIn: 0.36, panel: sceneLockScreen },
    { id: 'cardZeroCommits', start: 18, end: 20, turnIn: 0.45, panel: sceneCard('ZERO COMMITS BY HAND.', 'ΟΥΔΕΝ ΧΕΙΡΙ') },
    { id: 'sirens', start: 20, end: 26, turnIn: 0.5, panel: sceneSirens, view: (L) => ({ push: 0.05 * (L / 6), shake: 5 * progress(L, 4.2, 6) }) },
    { id: 'proteus', start: 26, end: 30, turnIn: 0.4, panel: sceneProteus, view: (L) => ({ push: 0.015 * (L / 4), shake: 16 * proteusHitDecay(L) }) },
    { id: 'storm', start: 30, end: 34, turnIn: 0.3, panel: sceneStorm, view: (L) => ({ push: 0.03 * (L / 4), shake: L < STORM_SNAP_LOCAL ? 7 : 0 }) },
    { id: 'nightDesk', start: 34, end: 41, turnIn: 0.4, panel: sceneNightDesk, view: (L) => ({ push: 0.03 * (L / 7) + heartbeatPulse(L) }) },
    { id: 'axes', start: 41, end: 47, turnIn: 0.45, panel: sceneAxes },
    { id: 'continue', start: 47, end: 50, panel: sceneContinue },
    { id: 'leave', start: 50, end: 54, turnIn: 0.5, panel: sceneLeave },
    {
      id: 'vaseOutro', start: 54, end: 57, turnIn: 0.6, panel: paintLoopFrieze,
      rotation: (L) => Math.max(0, L - 0.6) * 0.16,
      view: (L) => ({ zoom: zoomTween(FULL_ZOOM, 1, easeInOut(progress(L, 0.1, 1.9))), light: 1 - progress(L, 2.3, 3.0) }),
    },
    { id: 'logo', start: 57, end: 60, main: drawLogo },
  ];

  function sceneIndexAt(t) {
    for (let i = SCENES.length - 1; i >= 0; i--) if (t >= SCENES[i].start) return i;
    return 0;
  }

  function sceneStart(id) {
    return SCENES.find((scene) => scene.id === id).start;
  }

  // Sound cues derive from the same constants the pictures use, so the score
  // can never drift from the edit.
  function buildCues() {
    const montageClacks = [];
    for (let t = sceneStart('lotus'), interval = 0.42; t < sceneStart('lockScreen'); interval *= 0.86) {
      montageClacks.push(t);
      t += Math.max(0.07, interval);
    }
    const nightStart = sceneStart('nightDesk');
    const heartbeats = [];
    for (let beat = NIGHT.heartbeatStart; nightStart + beat < sceneStart('axes'); beat += NIGHT.heartbeatPeriod) heartbeats.push(nightStart + beat);
    const axesStart = sceneStart('axes');
    return {
      duration: DURATION_SECONDS,
      coldOpenClack: 1.5,
      pairingBass: sceneStart('pairing') + 0.15,
      owlSweep: [sceneStart('pairing') + 0.35, sceneStart('pairing') + 1.9],
      pairedChime: sceneStart('pairing') + 1.9,
      cardClack: sceneStart('cardTenYears'),
      montageClacks,
      breath: sceneStart('lockScreen') + 0.3,
      autoContinueTick: sceneStart('lockScreen') + 1.15,
      silence: [sceneStart('cardZeroCommits'), sceneStart('sirens')],
      shepard: { start: sceneStart('sirens'), cut: sceneStart('storm') + STORM_SNAP_LOCAL, resume: sceneStart('storm') + STORM_SNAP_LOCAL + 1, swell: axesStart, peak: sceneStart('continue') },
      sirenSea: [sceneStart('sirens'), sceneStart('proteus')],
      proteusHits: PROTEUS_HITS_LOCAL.map((hit) => sceneStart('proteus') + hit),
      storm: { start: sceneStart('storm'), lightning: [0.35, 0.85, 2.9].map((at) => sceneStart('storm') + at), snap: sceneStart('storm') + STORM_SNAP_LOCAL, end: sceneStart('nightDesk') },
      heartbeats,
      wait: nightStart + NIGHT.wait,
      heal: nightStart + NIGHT.healEnd,
      arrowRelease: axesStart + ARROW.releaseLocal,
      ringPasses: Array.from({ length: ARROW.ringCount }, (_, i) => axesStart + ringPassLocal(i)),
      hardCut: sceneStart('continue'),
      arrival: sceneStart('continue') + CONTINUE_ARRIVAL_LOCAL,
      seaBeds: [
        [sceneStart('vaseReveal'), sceneStart('pairing'), 0.5],
        [sceneStart('continue') + 0.3, sceneStart('leave'), 0.14],
        [sceneStart('leave'), sceneStart('vaseOutro'), 0.5],
        [sceneStart('vaseOutro'), sceneStart('logo'), 0.3],
      ],
      outroClack: sceneStart('vaseOutro') + 1.0,
      logoSwell: sceneStart('logo') + 0.2,
      paperSlide: sceneStart('logo') + 2.0,
      woodKnock: sceneStart('logo') + 2.35,
    };
  }

  // ------------------------------------------------------------ compositor
  const surfaces = {};
  let warpTable = null;

  function buildWarpTable() {
    const table = [];
    const sinHalf = Math.sin(CYLINDER_HALF_ANGLE);
    const surfaceAt = (x) => {
      const phi = Math.asin((x / WIDTH) * 2 * sinHalf - sinHalf);
      return (phi / CYLINDER_HALF_ANGLE + 1) / 2;
    };
    for (let x = 0; x < WIDTH; x += WARP_SLICE_WIDTH) table.push([x, surfaceAt(x), surfaceAt(Math.min(WIDTH, x + WARP_SLICE_WIDTH))]);
    return table;
  }

  function buildClayTexture() {
    const canvas = createSurface(WIDTH, HEIGHT);
    const g = canvas.getContext('2d');
    const random = seededRandom(11);
    g.fillStyle = '#fff';
    g.fillRect(0, 0, WIDTH, HEIGHT);
    for (let i = 0; i < 1400; i++) {
      const x = random() * WIDTH;
      const y = random() * HEIGHT;
      const radius = 20 + random() * 150;
      const gradient = g.createRadialGradient(x, y, 0, x, y, radius);
      gradient.addColorStop(0, `rgba(110, 50, 25, ${0.02 + random() * 0.05})`);
      gradient.addColorStop(1, 'rgba(110, 50, 25, 0)');
      g.fillStyle = gradient;
      g.fillRect(x - radius, y - radius, radius * 2, radius * 2);
    }
    for (let i = 0; i < 9000; i++) {
      g.fillStyle = `rgba(70, 34, 16, ${0.05 + random() * 0.12})`;
      g.fillRect(random() * WIDTH, random() * HEIGHT, 1 + random() * 2, 1 + random() * 2);
    }
    g.strokeStyle = 'rgba(80, 38, 18, 0.12)';
    g.lineWidth = 1;
    for (let i = 0; i < 60; i++) {
      let x = random() * WIDTH;
      let y = random() * HEIGHT;
      g.beginPath();
      g.moveTo(x, y);
      for (let j = 0; j < 8; j++) {
        x += (random() - 0.5) * 120;
        y += (random() - 0.5) * 120;
        g.lineTo(x, y);
      }
      g.stroke();
    }
    return canvas;
  }

  function buildGrainFrames() {
    const random = seededRandom(23);
    return Array.from({ length: GRAIN_FRAME_COUNT }, () => {
      const canvas = createSurface(WIDTH / 2, HEIGHT / 2);
      const g = canvas.getContext('2d');
      const image = g.createImageData(canvas.width, canvas.height);
      for (let i = 0; i < image.data.length; i += 4) {
        const value = 128 + (random() - 0.5) * 120;
        image.data[i] = value;
        image.data[i + 1] = value;
        image.data[i + 2] = value;
        image.data[i + 3] = 255;
      }
      g.putImageData(image, 0, 0);
      return canvas;
    });
  }

  function buildShading() {
    const canvas = createSurface(WIDTH, 1);
    const g = canvas.getContext('2d');
    const sinHalf = Math.sin(CYLINDER_HALF_ANGLE);
    for (let x = 0; x < WIDTH; x++) {
      const phi = Math.asin((x / WIDTH) * 2 * sinHalf - sinHalf);
      const brightness = 0.7 + 0.3 * Math.cos(phi * 1.5 + 0.12);
      const value = Math.round(255 * brightness);
      g.fillStyle = `rgb(${value}, ${Math.round(value * 0.97)}, ${Math.round(value * 0.94)})`;
      g.fillRect(x, 0, 1, 1);
    }
    return canvas;
  }

  function buildVignette() {
    const canvas = createSurface(WIDTH, HEIGHT);
    const g = canvas.getContext('2d');
    const gradient = g.createRadialGradient(WIDTH / 2, HEIGHT / 2, HEIGHT * 0.35, WIDTH / 2, HEIGHT / 2, WIDTH * 0.62);
    gradient.addColorStop(0, 'rgba(0, 0, 0, 0)');
    gradient.addColorStop(1, 'rgba(0, 0, 0, 0.5)');
    g.fillStyle = gradient;
    g.fillRect(0, 0, WIDTH, HEIGHT);
    return canvas;
  }

  function paintPanel(canvas, scene, L) {
    const ctx = canvas.getContext('2d');
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    ctx.save();
    ctx.beginPath();
    ctx.rect(0, BORDER_HEIGHT, WIDTH, HEIGHT - BORDER_HEIGHT * 2);
    ctx.clip();
    const result = scene.panel(ctx, L) || {};
    ctx.restore();
    drawPanelBorders(ctx, Boolean(result.dark));
    ctx.globalCompositeOperation = 'multiply';
    ctx.drawImage(surfaces.clay, 0, 0);
    ctx.globalCompositeOperation = 'source-over';
  }

  function warpPanels(first, second, offset) {
    const ctx = surfaces.band.getContext('2d');
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    for (const [x, u0, u1] of warpTable) {
      const source0 = (offset + u0) * WIDTH;
      const source1 = (offset + u1) * WIDTH;
      if (source1 <= WIDTH) {
        ctx.drawImage(first, source0, 0, Math.max(0.5, source1 - source0), HEIGHT, x, 0, WARP_SLICE_WIDTH, HEIGHT);
      } else if (source0 >= WIDTH) {
        ctx.drawImage(second, source0 - WIDTH, 0, Math.max(0.5, source1 - source0), HEIGHT, x, 0, WARP_SLICE_WIDTH, HEIGHT);
      } else {
        const split = (WIDTH - source0) / (source1 - source0);
        ctx.drawImage(first, source0, 0, Math.max(0.5, WIDTH - source0), HEIGHT, x, 0, WARP_SLICE_WIDTH * split, HEIGHT);
        ctx.drawImage(second, 0, 0, Math.max(0.5, source1 - WIDTH), HEIGHT, x + WARP_SLICE_WIDTH * split, 0, WARP_SLICE_WIDTH * (1 - split), HEIGHT);
      }
    }
    ctx.globalCompositeOperation = 'multiply';
    ctx.drawImage(surfaces.shading, 0, 0, WIDTH, HEIGHT);
    ctx.globalCompositeOperation = 'screen';
    const gloss = ctx.createLinearGradient(WIDTH * 0.2, 0, WIDTH * 0.5, 0);
    gloss.addColorStop(0, 'rgba(255, 236, 214, 0)');
    gloss.addColorStop(0.5, 'rgba(255, 236, 214, 0.07)');
    gloss.addColorStop(1, 'rgba(255, 236, 214, 0)');
    ctx.fillStyle = gloss;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.globalCompositeOperation = 'source-over';
  }

  function composeBand(index, t) {
    const scene = SCENES[index];
    const L = t - scene.start;
    const turnIn = scene.turnIn ?? 0;
    const previous = SCENES[index - 1];
    if (turnIn > 0 && L < turnIn && previous && previous.panel) {
      paintPanel(surfaces.panelA, previous, t - previous.start);
      paintPanel(surfaces.panelB, scene, L);
      warpPanels(surfaces.panelA, surfaces.panelB, easeInOut(L / turnIn));
      return;
    }
    paintPanel(surfaces.panelA, scene, L);
    const rotation = scene.rotation ? scene.rotation(L) : 0;
    warpPanels(surfaces.panelA, surfaces.panelA, ((rotation % 1) + 1) % 1);
  }

  function vaseBodyPath(ctx) {
    ctx.beginPath();
    ctx.moveTo(-200, -430);
    ctx.lineTo(200, -430);
    ctx.lineTo(186, -402);
    ctx.lineTo(124, -384);
    ctx.lineTo(118, -262);
    ctx.bezierCurveTo(200, -250, 420, -214, 432, -60);
    ctx.bezierCurveTo(444, 100, 392, 220, 300, 312);
    ctx.bezierCurveTo(232, 376, 150, 396, 124, 410);
    ctx.lineTo(190, 446);
    ctx.lineTo(190, 470);
    ctx.lineTo(-190, 470);
    ctx.lineTo(-190, 446);
    ctx.lineTo(-124, 410);
    ctx.bezierCurveTo(-150, 396, -232, 376, -300, 312);
    ctx.bezierCurveTo(-392, 220, -444, 100, -432, -60);
    ctx.bezierCurveTo(-420, -214, -200, -250, -118, -262);
    ctx.lineTo(-124, -384);
    ctx.lineTo(-186, -402);
    ctx.closePath();
  }

  function drawVaseView(ctx, zoom) {
    ctx.fillStyle = '#060404';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    const backlight = ctx.createRadialGradient(WIDTH / 2, HEIGHT / 2, 60, WIDTH / 2, HEIGHT / 2, 950);
    backlight.addColorStop(0, 'rgba(130, 56, 24, 0.4)');
    backlight.addColorStop(1, 'rgba(0, 0, 0, 0)');
    ctx.fillStyle = backlight;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.save();
    ctx.translate(WIDTH / 2, HEIGHT / 2);
    ctx.scale(zoom, zoom);
    ctx.strokeStyle = COLOR.ink;
    ctx.lineWidth = 24;
    ctx.lineCap = 'round';
    [-1, 1].forEach((side) => {
      ctx.beginPath();
      ctx.moveTo(side * 120, -360);
      ctx.bezierCurveTo(side * 260, -390, side * 336, -330, side * 322, -236);
      ctx.stroke();
    });
    vaseBodyPath(ctx);
    const body = ctx.createLinearGradient(-440, 0, 440, 0);
    body.addColorStop(0, '#6A2A0F');
    body.addColorStop(0.35, COLOR.terracotta);
    body.addColorStop(0.55, '#D2733A');
    body.addColorStop(1, '#5C240C');
    ctx.fillStyle = body;
    ctx.fill();
    ctx.save();
    vaseBodyPath(ctx);
    ctx.clip();
    ctx.fillStyle = COLOR.ink;
    ctx.fillRect(-460, -440, 920, 196);
    ctx.fillRect(-460, 236, 920, 240);
    ctx.fillStyle = COLOR.terracotta;
    ctx.fillRect(-460, 300, 920, 6);
    for (let i = -9; i <= 9; i++) {
      ctx.fillStyle = i % 2 ? COLOR.ink : '#3A1608';
      fillEllipse(ctx, i * 44, -214, 18, 26);
    }
    for (let i = -10; i <= 10; i++) {
      ctx.fillStyle = COLOR.ink;
      fillPolygon(ctx, [[i * 40 - 16, 236], [i * 40, 186], [i * 40 + 16, 236]]);
    }
    ctx.drawImage(surfaces.band, -VASE_BAND_WIDTH / 2, -VASE_BAND_HEIGHT / 2, VASE_BAND_WIDTH, VASE_BAND_HEIGHT);
    const shade = ctx.createLinearGradient(-440, 0, 440, 0);
    shade.addColorStop(0, 'rgba(0, 0, 0, 0.55)');
    shade.addColorStop(0.3, 'rgba(0, 0, 0, 0)');
    shade.addColorStop(0.7, 'rgba(0, 0, 0, 0)');
    shade.addColorStop(1, 'rgba(0, 0, 0, 0.6)');
    ctx.fillStyle = shade;
    ctx.fillRect(-460, -440, 920, 920);
    const gloss = ctx.createLinearGradient(-260, 0, -60, 0);
    gloss.addColorStop(0, 'rgba(255, 240, 220, 0)');
    gloss.addColorStop(0.5, 'rgba(255, 240, 220, 0.12)');
    gloss.addColorStop(1, 'rgba(255, 240, 220, 0)');
    ctx.fillStyle = gloss;
    ctx.fillRect(-460, -440, 920, 920);
    ctx.restore();
    ctx.restore();
  }

  function drawBandFull(ctx, view, t) {
    const scale = 1 + (view.push ?? 0);
    const shake = view.shake ?? 0;
    const offsetX = shake ? Math.sin(t * 93.1) * shake + Math.sin(t * 51.7) * shake * 0.5 : 0;
    const offsetY = shake ? Math.cos(t * 87.3) * shake * 0.7 : 0;
    ctx.drawImage(surfaces.band, (WIDTH - WIDTH * scale) / 2 + offsetX, (HEIGHT - HEIGHT * scale) / 2 + offsetY, WIDTH * scale, HEIGHT * scale);
  }

  function renderFrame(ctx, time) {
    const t = clamp(time, 0, DURATION_SECONDS - 1e-6);
    const index = sceneIndexAt(t);
    const scene = SCENES[index];
    const L = t - scene.start;
    ctx.save();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    let light = 1;
    if (scene.main) {
      scene.main(ctx, L);
    } else {
      composeBand(index, t);
      const view = scene.view ? scene.view(L) : { push: 0.02 * (L / (scene.end - scene.start)) };
      const zoom = view.zoom ?? FULL_ZOOM;
      if (zoom >= FULL_ZOOM - 1e-3) drawBandFull(ctx, view, t);
      else drawVaseView(ctx, zoom);
      light = view.light ?? 1;
    }
    ctx.globalCompositeOperation = 'multiply';
    ctx.drawImage(surfaces.vignette, 0, 0);
    ctx.globalCompositeOperation = 'overlay';
    ctx.globalAlpha = 0.1;
    ctx.drawImage(surfaces.grain[Math.floor(t * GRAIN_FPS) % GRAIN_FRAME_COUNT], 0, 0, WIDTH, HEIGHT);
    ctx.globalAlpha = 1;
    ctx.globalCompositeOperation = 'source-over';
    if (light < 1) {
      ctx.fillStyle = `rgba(0, 0, 0, ${1 - light})`;
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }
    ctx.restore();
  }

  // ------------------------------------------------------------------- API
  async function init() {
    if (warpTable) return;
    await Promise.all([
      document.fonts.load(`600 40px ${FONT.carved}`),
      document.fonts.load(`italic 600 40px ${FONT.script}`),
      document.fonts.load(`600 20px ${FONT.mono}`),
    ]).catch(() => {});
    surfaces.panelA = createSurface(WIDTH, HEIGHT);
    surfaces.panelB = createSurface(WIDTH, HEIGHT);
    surfaces.band = createSurface(WIDTH, HEIGHT);
    surfaces.export = createSurface(WIDTH, HEIGHT);
    surfaces.clay = buildClayTexture();
    surfaces.grain = buildGrainFrames();
    surfaces.shading = buildShading();
    surfaces.vignette = buildVignette();
    figureLayer = createSurface(WIDTH, HEIGHT);
    warpTable = buildWarpTable();
  }

  function captureFrame(time, type = 'image/jpeg', quality = 0.94) {
    const ctx = surfaces.export.getContext('2d');
    renderFrame(ctx, time);
    return surfaces.export.toDataURL(type, quality);
  }

  window.LooperTrailer = {
    WIDTH,
    HEIGHT,
    DURATION: DURATION_SECONDS,
    CUES: buildCues(),
    SCENES: SCENES.map(({ id, start, end }) => ({ id, start, end })),
    init,
    renderFrame,
    captureFrame,
  };
})();
