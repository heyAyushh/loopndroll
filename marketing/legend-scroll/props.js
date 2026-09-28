/* Legend of the Looper — painted props: the desk machine, phone, jade, ships, halls, the scroll itself. */
(function () {
  'use strict';

  const L = window.Legend;
  const { COLOR, RGB, clamp, lerp, easeOut, seededRandom, createSurface, fillCircle, fillEllipse, polygonPath, fillPolygon, strokeLine, strokePolyline,
    catmullRom, drawGlow, brushStroke } = L;

  const SESSION_NAMES = ['CODEX', 'CLAUDE', 'ZED', 'GROK'];

  /** The machine at home, painted in ink; four jade lights for four running sessions. */
  function inkLaptop(ctx, x, y, scale, state = {}) {
    const running = state.running ?? [true, true, true, true];
    const t = state.t ?? 0;
    const screenGlow = state.glow ?? 1;
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    drawGlow(ctx, 0, -150, 420, RGB.jade, 0.22 * screenGlow);
    ctx.fillStyle = 'rgba(26, 21, 18, 0.92)';
    ctx.beginPath();
    ctx.roundRect(-200, -290, 400, 272, 14);
    ctx.fill();
    const display = ctx.createLinearGradient(0, -280, 0, -30);
    display.addColorStop(0, 'rgba(24, 40, 34, 1)');
    display.addColorStop(1, 'rgba(14, 22, 20, 1)');
    ctx.fillStyle = display;
    ctx.fillRect(-186, -276, 372, 246);
    const random = seededRandom(5);
    for (let pane = 0; pane < 4; pane++) {
      const px = -178 + (pane % 2) * 182;
      const py = -266 + Math.floor(pane / 2) * 118;
      ctx.strokeStyle = 'rgba(123, 227, 174, 0.18)';
      ctx.lineWidth = 1.5;
      ctx.strokeRect(px, py, 174, 108);
      ctx.fillStyle = 'rgba(210, 230, 215, 0.55)';
      ctx.font = `600 13px ${L.FONT.mono}`;
      ctx.textBaseline = 'middle';
      ctx.fillText(SESSION_NAMES[pane], px + 10, py + 14);
      for (let line = 0; line < 5; line++) {
        const growth = running[pane] ? (Math.sin(t * 1.3 + pane + line) * 0.5 + 0.5) * 30 : 0;
        ctx.fillStyle = 'rgba(123, 227, 174, 0.16)';
        ctx.fillRect(px + 10, py + 32 + line * 14, 36 + random() * 90 + growth, 5);
      }
      const lit = running[pane];
      if (lit) {
        drawGlow(ctx, px + 158, py + 14, 34, RGB.jade, 0.9);
        ctx.fillStyle = COLOR.jadeGlow;
      } else {
        ctx.fillStyle = 'rgba(210, 200, 180, 0.3)';
      }
      fillCircle(ctx, px + 158, py + 14, 5.5);
    }
    ctx.fillStyle = 'rgba(26, 21, 18, 0.95)';
    fillPolygon(ctx, [[-214, -16], [214, -16], [238, 2], [-238, 2]]);
    brushStroke(ctx, [[-236, 2], [0, 6], [236, 2]], 5, { dry: 0.2, taperStart: 0.05, taperEnd: 0.1, seed: 4 });
    const orb = [168, -283];
    ctx.fillStyle = COLOR.paperLight;
    fillCircle(ctx, orb[0], orb[1], 4);
    ctx.restore();
    return { orb: [x + orb[0] * scale, y + orb[1] * scale], screen: [x, y - 150 * scale] };
  }

  function inkPhone(ctx, x, y, size, state = {}) {
    const { glow = 1, angle = 0, icon = 'running' } = state;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(angle);
    if (glow > 0) drawGlow(ctx, 0, 0, size * 4, icon === 'queued' ? RGB.paper : RGB.jade, 0.8 * glow);
    ctx.fillStyle = 'rgba(20, 16, 14, 0.96)';
    ctx.beginPath();
    ctx.roundRect(-size / 2, -size, size, size * 2, size * 0.22);
    ctx.fill();
    ctx.strokeStyle = 'rgba(230, 215, 180, 0.8)';
    ctx.lineWidth = Math.max(1.2, size * 0.06);
    ctx.stroke();
    if (icon === 'running') {
      ctx.fillStyle = COLOR.jadeGlow;
      fillCircle(ctx, 0, -size * 0.25, size * 0.2);
    } else if (icon === 'queued') {
      ctx.strokeStyle = COLOR.paperLight;
      ctx.lineWidth = Math.max(1, size * 0.07);
      ctx.beginPath();
      ctx.arc(0, -size * 0.1, size * 0.26, 0, Math.PI * 2);
      ctx.stroke();
      strokeLine(ctx, 0, -size * 0.1, 0, -size * 0.3);
      strokeLine(ctx, 0, -size * 0.1, size * 0.14, -size * 0.1);
    }
    ctx.restore();
  }

  /** A carved jade bi disc; `lit` 0→1 wakes it with an inner glow. */
  function jadeDisc(ctx, x, y, radius, lit = 0, t = 0) {
    if (lit > 0) drawGlow(ctx, x, y, radius * 3.4, RGB.jade, lit);
    ctx.save();
    ctx.translate(x, y);
    const body = ctx.createRadialGradient(-radius * 0.3, -radius * 0.3, radius * 0.1, 0, 0, radius);
    body.addColorStop(0, lit > 0 ? `rgba(160, 240, 196, ${0.6 + lit * 0.4})` : 'rgba(150, 170, 150, 0.85)');
    body.addColorStop(1, lit > 0 ? `rgba(40, 150, 100, ${0.7 + lit * 0.3})` : 'rgba(70, 90, 76, 0.9)');
    ctx.fillStyle = body;
    ctx.beginPath();
    ctx.arc(0, 0, radius, 0, Math.PI * 2);
    ctx.arc(0, 0, radius * 0.38, 0, Math.PI * 2, true);
    ctx.fill('evenodd');
    ctx.strokeStyle = 'rgba(20, 30, 24, 0.7)';
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.arc(0, 0, radius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(0, 0, radius * 0.38, 0, Math.PI * 2);
    ctx.stroke();
    ctx.strokeStyle = 'rgba(20, 40, 30, 0.35)';
    ctx.lineWidth = 1.4;
    for (let ring = 0; ring < 3; ring++) {
      const r = radius * (0.5 + ring * 0.15);
      for (let k = 0; k < 10; k++) {
        const a = (k / 10) * Math.PI * 2 + ring * 0.3 + t * 0.2;
        ctx.beginPath();
        ctx.arc(Math.cos(a) * r, Math.sin(a) * r, radius * 0.05, 0, Math.PI * 1.6);
        ctx.stroke();
      }
    }
    ctx.restore();
  }

  function loom(ctx, x, groundY, scale, t, weaving = 0) {
    ctx.save();
    ctx.translate(x, groundY);
    ctx.scale(scale, scale);
    brushStroke(ctx, [[-120, 0], [-118, -300], [-120, -560]], 14, { dry: 0.4, seed: 1, taperStart: 0.02, taperEnd: 0.05 });
    brushStroke(ctx, [[120, 0], [118, -300], [120, -560]], 14, { dry: 0.4, seed: 2, taperStart: 0.02, taperEnd: 0.05 });
    brushStroke(ctx, [[-150, -556], [0, -564], [150, -556]], 16, { dry: 0.4, seed: 3, taperStart: 0.02, taperEnd: 0.05 });
    const clothTop = -540;
    const clothBottom = -360;
    ctx.fillStyle = 'rgba(38, 54, 100, 0.55)';
    ctx.fillRect(-108, clothTop, 216, clothBottom - clothTop);
    ctx.strokeStyle = 'rgba(214, 170, 90, 0.7)';
    ctx.lineWidth = 2;
    for (let row = 0; row < 3; row++) {
      const y = clothTop + 28 + row * 56;
      ctx.beginPath();
      for (let i = 0; i < 8; i++) {
        const bx = -96 + i * 25;
        ctx.moveTo(bx, y + 22);
        ctx.lineTo(bx, y);
        ctx.lineTo(bx + 18, y);
        ctx.lineTo(bx + 18, y + 14);
        ctx.lineTo(bx + 7, y + 14);
      }
      ctx.stroke();
    }
    ctx.strokeStyle = 'rgba(26, 21, 18, 0.55)';
    ctx.lineWidth = 1.2;
    for (let i = 0; i < 16; i++) strokeLine(ctx, -104 + i * 14, clothBottom, -104 + i * 14, -130);
    ctx.fillStyle = 'rgba(26, 21, 18, 0.85)';
    for (let i = 0; i < 16; i++) fillPolygon(ctx, [[-110 + i * 14, -130], [-100 + i * 14, -130], [-98 + i * 14, -100], [-112 + i * 14, -100]]);
    if (weaving > 0) {
      const shuttleX = Math.sin(t * 9) * 100;
      ctx.fillStyle = 'rgba(214, 170, 90, 0.95)';
      fillEllipse(ctx, shuttleX, clothBottom + 20, 22, 6);
    }
    ctx.restore();
    return { clothBottom: [x, groundY + (clothBottom + 20) * scale], shuttleY: groundY + (clothBottom + 20) * scale };
  }

  function writingDesk(ctx, x, deskY, groundY, width) {
    brushStroke(ctx, [[x - width / 2, deskY], [x, deskY + 2], [x + width / 2, deskY]], 16, { dry: 0.3, taperStart: 0.02, taperEnd: 0.05, seed: 21 });
    [-1, 1].forEach((side) => {
      const legX = x + side * (width / 2 - 26);
      brushStroke(ctx, catmullRom([[legX, deskY + 6], [legX + side * 12, (deskY + groundY) / 2], [legX + side * 4, groundY - 10], [legX + side * 20, groundY]], 6), 10, { dry: 0.3, taperStart: 0.02, taperEnd: 0.3, seed: 22 + side });
    });
  }

  /** A curved-roof pavilion; `glow` lights the interior. */
  function pavilion(ctx, x, groundY, scale, glow = 0, seed = 7) {
    ctx.save();
    ctx.translate(x, groundY);
    ctx.scale(scale, scale);
    if (glow > 0) drawGlow(ctx, 0, -170, 360, RGB.candle, glow * 0.8);
    ctx.fillStyle = 'rgba(40, 32, 28, 0.25)';
    ctx.fillRect(-260, -40, 520, 40);
    brushStroke(ctx, [[-280, -2], [0, 2], [280, -2]], 10, { dry: 0.3, seed });
    brushStroke(ctx, [[-250, -40], [0, -36], [250, -40]], 8, { dry: 0.3, seed: seed + 1 });
    for (let i = 0; i < 4; i++) {
      const cx = -180 + i * 120;
      brushStroke(ctx, [[cx, -40], [cx + 2, -170], [cx, -300]], 14, { dry: 0.35, seed: seed + 2 + i, taperStart: 0.02, taperEnd: 0.05 });
    }
    ctx.fillStyle = glow > 0 ? `rgba(255, 210, 140, ${0.18 + glow * 0.35})` : 'rgba(40, 32, 28, 0.12)';
    ctx.fillRect(-170, -290, 340, 250);
    ctx.strokeStyle = 'rgba(26, 21, 18, 0.7)';
    ctx.lineWidth = 2;
    for (let i = 0; i < 9; i++) strokeLine(ctx, -250 + i * 62, -60, -250 + i * 62, -100);
    strokeLine(ctx, -256, -100, 256, -100);
    ctx.fillStyle = 'rgba(22, 18, 16, 0.92)';
    ctx.beginPath();
    ctx.moveTo(-390, -296);
    ctx.quadraticCurveTo(-320, -326, -250, -326);
    ctx.lineTo(250, -326);
    ctx.quadraticCurveTo(320, -326, 390, -296);
    ctx.quadraticCurveTo(230, -356, 140, -470);
    ctx.lineTo(-140, -470);
    ctx.quadraticCurveTo(-230, -356, -390, -296);
    ctx.closePath();
    ctx.fill();
    ctx.strokeStyle = 'rgba(214, 170, 90, 0.45)';
    ctx.lineWidth = 2;
    for (let i = -7; i <= 7; i++) strokeLine(ctx, i * 18, -466, i * 34, -330);
    ctx.fillStyle = 'rgba(22, 18, 16, 0.95)';
    ctx.fillRect(-260, -336, 520, 12);
    brushStroke(ctx, [[-160, -474], [0, -480], [160, -474]], 12, { seed: seed + 9, taperStart: 0.02, taperEnd: 0.05 });
    [-1, 1].forEach((side) => brushStroke(ctx, [[side * 150, -476], [side * 176, -498], [side * 168, -520]], 9, { seed: seed + 10 + side, taperEnd: 0.8 }));
    ctx.restore();
  }

  function lantern(ctx, x, y, size, t, lit = 1) {
    const sway = Math.sin(t * 1.4 + x * 0.01) * 0.06;
    ctx.save();
    ctx.translate(x, y);
    ctx.rotate(sway);
    ctx.strokeStyle = 'rgba(26, 21, 18, 0.8)';
    ctx.lineWidth = 1.5;
    strokeLine(ctx, 0, -size * 1.4, 0, -size * 0.8);
    if (lit > 0) drawGlow(ctx, 0, 0, size * 4, RGB.candle, 0.7 * lit);
    const body = ctx.createRadialGradient(0, 0, size * 0.1, 0, 0, size);
    body.addColorStop(0, `rgba(255, 224, 160, ${0.5 + lit * 0.5})`);
    body.addColorStop(1, `rgba(210, 140, 60, ${0.6 + lit * 0.3})`);
    ctx.fillStyle = body;
    fillEllipse(ctx, 0, 0, size * 0.7, size * 0.85);
    ctx.strokeStyle = 'rgba(26, 21, 18, 0.7)';
    for (let i = -2; i <= 2; i++) {
      ctx.beginPath();
      ctx.ellipse(0, 0, size * 0.7 * Math.cos((i / 3) * Math.PI * 0.5), size * 0.85, 0, -Math.PI / 2, Math.PI / 2);
      ctx.stroke();
    }
    ctx.fillStyle = 'rgba(26, 21, 18, 0.9)';
    ctx.fillRect(-size * 0.35, -size * 0.92, size * 0.7, size * 0.14);
    ctx.fillRect(-size * 0.35, size * 0.8, size * 0.7, size * 0.14);
    strokeLine(ctx, 0, size * 0.94, 0, size * 1.4);
    ctx.restore();
  }

  /** The king's galley, painted with a few loaded strokes. */
  function galley(ctx, x, waterY, scale, t, options = {}) {
    const { sail = true, tilt = 0, oars = true, seed = 3 } = options;
    ctx.save();
    ctx.translate(x, waterY);
    ctx.rotate(tilt);
    ctx.scale(scale, scale);
    if (oars) {
      ctx.strokeStyle = 'rgba(26, 21, 18, 0.8)';
      ctx.lineWidth = 4;
      ctx.lineCap = 'round';
      for (let i = 0; i < 9; i++) {
        const ox = -200 + i * 46;
        const phase = t * 4.4 + i * 0.15;
        strokeLine(ctx, ox, -34, ox + 64 + Math.sin(phase) * 26, 66);
      }
    }
    if (sail) {
      brushStroke(ctx, [[0, -40], [2, -250], [0, -440]], 11, { dry: 0.3, seed: seed + 1, taperStart: 0.02, taperEnd: 0.05 });
      brushStroke(ctx, [[-170, -410], [0, -418], [170, -410]], 8, { dry: 0.3, seed: seed + 2 });
      const billow = Math.sin(t * 0.8) * 10;
      ctx.fillStyle = 'rgba(214, 196, 160, 0.9)';
      ctx.beginPath();
      ctx.moveTo(-164, -404);
      ctx.lineTo(164, -404);
      ctx.quadraticCurveTo(190 + billow, -300, 156, -216);
      ctx.lineTo(-156, -216);
      ctx.quadraticCurveTo(-130 + billow, -300, -164, -404);
      ctx.fill();
      ctx.strokeStyle = 'rgba(26, 21, 18, 0.65)';
      ctx.lineWidth = 2;
      ctx.stroke();
      for (let i = -3; i <= 3; i++) strokeLine(ctx, i * 44, -400, i * 44 + billow * 0.4, -220);
      ctx.fillStyle = 'rgba(26, 21, 18, 0.18)';
      fillCircle(ctx, billow * 0.4, -310, 46);
    }
    ctx.fillStyle = 'rgba(22, 18, 16, 0.94)';
    ctx.beginPath();
    ctx.moveTo(320, -96);
    ctx.quadraticCurveTo(350, -60, 300, -34);
    ctx.lineTo(-250, -40);
    ctx.lineTo(-300, -80);
    ctx.lineTo(-318, -76);
    ctx.lineTo(-298, -34);
    ctx.lineTo(-356, -12);
    ctx.lineTo(-296, 6);
    ctx.quadraticCurveTo(0, 26, 290, -4);
    ctx.quadraticCurveTo(326, -40, 312, -92);
    ctx.closePath();
    ctx.fill();
    brushStroke(ctx, [[-290, -30], [0, -34], [296, -30]], 4, { color: 'rgba(214, 170, 90, 0.8)', dry: 0, seed: seed + 3 });
    ctx.fillStyle = 'rgba(240, 225, 190, 0.95)';
    fillEllipse(ctx, -262, -22, 10, 5);
    ctx.fillStyle = COLOR.ink;
    fillCircle(ctx, -264, -22, 3);
    ctx.restore();
    const toParent = (lx, ly) => [x + scale * (Math.cos(tilt) * lx - Math.sin(tilt) * ly), waterY + scale * (Math.sin(tilt) * lx + Math.cos(tilt) * ly)];
    return { stern: toParent(318, -96), deck: toParent(-30, -40), mastBase: toParent(0, -40) };
  }

  function bow(ctx, grip, draw, t, stringColor = COLOR.thread) {
    const top = [grip[0] - 16, grip[1] - 170];
    const bottom = [grip[0] - 16, grip[1] + 170];
    const limbPoints = [];
    for (let i = 0; i <= 16; i++) {
      const u = i / 16;
      const bend = Math.sin(u * Math.PI) * (50 + draw * 20);
      const recurve = (u < 0.12 || u > 0.88) ? -14 : 0;
      limbPoints.push([lerp(top[0], bottom[0], u) + bend + recurve, lerp(top[1], bottom[1], u)]);
    }
    brushStroke(ctx, limbPoints, 14, { taperStart: 0.1, taperEnd: 0.1, dry: 0.3, seed: 44, minWidth: 0.3 });
    return { top: limbPoints[0], bottom: limbPoints[limbPoints.length - 1], stringColor, t };
  }

  function arrow(ctx, tipX, y, angle = 0) {
    ctx.save();
    ctx.translate(tipX, y);
    ctx.rotate(angle);
    brushStroke(ctx, [[-190, 0], [-100, 0], [-16, 0]], 6, { taperStart: 0.02, taperEnd: 0.02, dry: 0.2, seed: 61, minWidth: 0.5 });
    ctx.fillStyle = COLOR.ink;
    fillPolygon(ctx, [[0, 0], [-30, -11], [-22, 0], [-30, 11]]);
    ctx.fillStyle = 'rgba(214, 170, 90, 0.85)';
    fillPolygon(ctx, [[-166, 0], [-194, -18], [-176, -18], [-148, 0]]);
    fillPolygon(ctx, [[-166, 0], [-194, 18], [-176, 18], [-148, 0]]);
    ctx.restore();
  }

  function redThread(ctx, points, options = {}) {
    const { reveal = 1, width = 3.4, alpha = 1, glow = 0.55 } = options;
    if (reveal <= 0 || points.length < 2) return;
    const path = reveal < 1 ? L.slicePath(points, reveal) : points;
    ctx.save();
    ctx.globalAlpha *= alpha;
    ctx.strokeStyle = COLOR.thread;
    ctx.lineWidth = width;
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.shadowColor = `rgba(220, 50, 40, ${glow})`;
    ctx.shadowBlur = 14;
    strokePolyline(ctx, path);
    ctx.shadowBlur = 0;
    ctx.strokeStyle = 'rgba(255, 170, 150, 0.35)';
    ctx.lineWidth = Math.max(1, width * 0.3);
    strokePolyline(ctx, path);
    ctx.restore();
    return path[path.length - 1];
  }

  // ------------------------------------------------------- the object scroll
  function lacquerTable(ctx, t, candleLight, candleScreen = [1560, 360]) {
    const { WIDTH, HEIGHT } = L;
    const wood = ctx.createLinearGradient(0, 0, 0, HEIGHT);
    wood.addColorStop(0, '#0B0504');
    wood.addColorStop(0.5, '#1F0D08');
    wood.addColorStop(1, '#0A0403');
    ctx.fillStyle = wood;
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.save();
    ctx.globalAlpha = 0.12;
    ctx.strokeStyle = '#5A2A18';
    ctx.lineWidth = 1;
    for (let i = 0; i < 70; i++) {
      const y = (i / 70) * HEIGHT;
      ctx.beginPath();
      for (let x = 0; x <= WIDTH; x += 40) ctx.lineTo(x, y + Math.sin(x * 0.004 + i) * 6 + L.noise1(x * 0.01 + i) * 4);
      ctx.stroke();
    }
    ctx.restore();
    const flicker = 0.85 + 0.15 * (Math.sin(t * 13) * 0.5 + Math.sin(t * 7.3) * 0.5);
    drawGlow(ctx, candleScreen[0], candleScreen[1], 1300, RGB.candle, 0.5 * candleLight * flicker);
    drawGlow(ctx, candleScreen[0], candleScreen[1], 320, RGB.candle, 0.45 * candleLight * flicker);
  }

  function candle(ctx, x, y, t, lit) {
    ctx.save();
    const wax = ctx.createLinearGradient(x - 26, 0, x + 26, 0);
    wax.addColorStop(0, '#6E5A40');
    wax.addColorStop(0.5, '#D8C49A');
    wax.addColorStop(1, '#6E5A40');
    ctx.fillStyle = wax;
    ctx.fillRect(x - 26, y, 52, 180);
    ctx.fillStyle = '#2A1A10';
    fillEllipse(ctx, x, y + 184, 60, 14);
    ctx.fillStyle = '#E8D8B0';
    fillEllipse(ctx, x, y, 26, 7);
    ctx.strokeStyle = '#1A0E08';
    ctx.lineWidth = 2;
    strokeLine(ctx, x, y, x + 1, y - 14);
    if (lit > 0) {
      const flicker = Math.sin(t * 17) * 2 + Math.sin(t * 11) * 1.5;
      drawGlow(ctx, x, y - 40, 160 * lit, RGB.candle, 0.9 * lit);
      ctx.globalAlpha = lit;
      const flame = ctx.createRadialGradient(x, y - 32, 2, x, y - 32, 30);
      flame.addColorStop(0, 'rgba(255, 250, 230, 1)');
      flame.addColorStop(0.5, 'rgba(255, 200, 100, 0.9)');
      flame.addColorStop(1, 'rgba(255, 140, 40, 0)');
      ctx.fillStyle = flame;
      ctx.beginPath();
      ctx.moveTo(x - 10, y - 16);
      ctx.quadraticCurveTo(x - 12 + flicker * 0.3, y - 44, x + flicker, y - 70);
      ctx.quadraticCurveTo(x + 12, y - 40, x + 10, y - 16);
      ctx.closePath();
      ctx.fill();
    }
    ctx.restore();
  }

  function incenseSmoke(ctx, x, y, t, alpha = 1) {
    ctx.save();
    ctx.strokeStyle = `rgba(200, 190, 175, ${0.18 * alpha})`;
    ctx.lineCap = 'round';
    for (let strand = 0; strand < 3; strand++) {
      ctx.lineWidth = 3 - strand;
      ctx.beginPath();
      for (let s = 0; s <= 40; s++) {
        const u = s / 40;
        const px = x + Math.sin(u * 7 + t * 0.9 + strand) * 30 * u + L.noise1(u * 4 + t * 0.4 + strand * 5) * 50 * u;
        const py = y - u * 520;
        if (s === 0) ctx.moveTo(px, py);
        else ctx.lineTo(px, py);
      }
      ctx.stroke();
    }
    ctx.restore();
    ctx.fillStyle = '#3A2414';
    ctx.fillRect(x - 3, y, 6, 70);
    drawGlow(ctx, x, y, 14, RGB.candle, 0.9 * alpha);
  }

  /** A roller (the rolled part of the scroll) with jade end knobs. */
  function scrollRoller(ctx, x, top, bottom, radius) {
    ctx.save();
    const body = ctx.createLinearGradient(x - radius, 0, x + radius, 0);
    body.addColorStop(0, '#6F5E44');
    body.addColorStop(0.45, '#EEE3C8');
    body.addColorStop(1, '#6A5A40');
    ctx.fillStyle = body;
    ctx.fillRect(x - radius, top, radius * 2, bottom - top);
    ctx.strokeStyle = 'rgba(80, 60, 40, 0.35)';
    ctx.lineWidth = 1;
    for (let i = 1; i < 6; i++) strokeLine(ctx, x - radius + (i / 6) * radius * 2, top, x - radius + (i / 6) * radius * 2, bottom);
    [top - 16, bottom + 16].forEach((cy) => {
      const knob = ctx.createRadialGradient(x - 6, cy - 6, 2, x, cy, radius * 0.9);
      knob.addColorStop(0, '#B8E8CC');
      knob.addColorStop(1, '#2E7A55');
      ctx.fillStyle = knob;
      fillEllipse(ctx, x, cy, radius * 0.75, 20);
    });
    ctx.restore();
  }

  function brocadeBorder(ctx, left, right, y, height, flip = false) {
    ctx.save();
    const gradient = ctx.createLinearGradient(0, y, 0, y + height);
    gradient.addColorStop(flip ? 1 : 0, '#3B2A1E');
    gradient.addColorStop(flip ? 0 : 1, '#6B4E2E');
    ctx.fillStyle = gradient;
    ctx.fillRect(left, y, right - left, height);
    ctx.strokeStyle = 'rgba(226, 179, 90, 0.35)';
    ctx.lineWidth = 1.5;
    const step = 46;
    for (let x = left - (left % step); x < right; x += step) {
      ctx.beginPath();
      ctx.arc(x, y + height / 2, height * 0.26, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }

  Object.assign(L, {
    SESSION_NAMES, inkLaptop, inkPhone, jadeDisc, loom, writingDesk, pavilion, lantern, galley, bow, arrow, redThread,
    lacquerTable, candle, incenseSmoke, scrollRoller, brocadeBorder,
  });
  void clamp; void easeOut; void createSurface; void polygonPath;
})();
