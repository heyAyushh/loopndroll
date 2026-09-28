/* Legend of the Looper — the scroll's layout, beats (cut to the narration), camera, and painted stations. */
(function () {
  'use strict';

  const L = window.Legend;
  const {
    COLOR, RGB, clamp, lerp, progress, smooth, easeInOut, easeOut, easeIn, easeOutBack, pulseAt, lerpPoint, seededRandom,
    fillCircle, fillEllipse, strokeLine, catmullRom, quadraticPoints, pointOnPath, drawGlow, brushStroke,
  } = L;

  // ---------------------------------------------------------------- beats
  const NARRATION = window.LEGEND_NARRATION || [];
  const line = (id) => NARRATION.find((entry) => entry.id === id) || { start: 0, duration: 0 };
  const said = (id) => line(id).start;
  const done = (id) => line(id).start + line(id).duration;

  const BEAT = {
    candle: 0.9,
    cordUntie: [3.0, 4.5],
    unroll: [4.3, 8.3],
    dive: [7.7, 10.3],
    titleBrush: 9.4,
    titleSeal: said('legend') + 2.7,
    titleEnglish: said('untouched') - 0.8,
    toIthaca: [done('untouched') - 0.5, said('desk') + 0.4],
    pairing: [said('thread') + 0.3, said('thread') + 3.0],
    departWalk: [done('thread') - 0.6, done('thread') + 1.8],
    toSea: [done('thread') + 1.2, said('sea') + 0.2],
    years: [said('sea') - 0.2, said('lotus') - 1.4],
    yearsSeal: said('sea') + 1.8,
    toLotus: [done('sea') + 0.4, said('lotus') - 0.2],
    toCave: [done('lotus') + 0.4, said('cave') + 0.2],
    craneQueued: said('cave') + 2.4,
    sunrise: done('cave') - 1.6,
    craneFlight: [done('cave') - 0.9, done('cave') + 1.9],
    toCirce: [done('cave') + 0.6, said('circe') + 0.2],
    askScroll: said('circe') + 0.6,
    approve: said('circe') + 3.5,
    toSirens: [done('circe') + 0.3, said('sirens') + 0.2],
    song: [said('sirens') + 1.4, said('muted') + 0.2],
    muteSeal: said('muted') + 0.35,
    toProteus: [done('muted') + 0.4, said('shapes') + 0.1],
    proteusHits: [0.9, 2.2, 3.5, 4.8].map((offset) => said('shapes') + offset),
    sameSeal: said('shapes') + 5.2,
    toStorm: [done('shapes') + 0.4, said('dragon') - 0.4],
    dragon: [said('dragon') - 0.2, said('broke') + 0.6],
    lightning: [said('dragon') + 1.1, said('dragon') + 3.0, said('broke') - 0.1],
    snap: said('broke') + 1.1,
    silence: [said('broke') + 1.25, said('switch') - 1.6],
    inkDrop: [said('broke') + 2.2, said('broke') + 3.4],
    toNight: [said('switch') - 1.9, said('switch') - 0.1],
    reach: [said('switch') + 0.6, said('wait') - 0.1],
    grab: said('wait') + 0.05,
    waitCharacter: said('wait') + 0.2,
    reweave: [said('remembered') + 0.2, done('remembered') + 0.1],
    lightsOn: done('remembered') + 0.2,
    toBow: [done('remembered') + 0.5, said('arrow') - 0.1],
    draw: [said('arrow') + 0.4, done('arrow') - 0.1],
    release: done('arrow') + 0.1,
    lastRing: said('rings') + 5.1,
    doneSeal: said('rings') + 5.4,
    toContinue: [done('rings') + 0.2, said('word') - 0.1],
    craneHome: [said('word') + 1.0, done('word') + 2.2],
    arrival: done('word') + 2.3,
    toLeave: [done('word') + 2.0, said('goes-on') - 0.6],
    rise: said('goes-on') - 0.4,
    walk: [said('goes-on') + 0.2, done('so-can-you') - 0.4],
    pullBack: [said('goes-on') + 1.2, done('so-can-you') + 0.1],
    rollUp: [done('so-can-you') + 0.1, done('so-can-you') + 2.9],
    tie: [done('so-can-you') + 2.4, done('so-can-you') + 3.9],
    title: done('so-can-you') + 3.0,
    fadeOut: [L.DURATION_SECONDS - 0.7, L.DURATION_SECONDS],
  };

  // ------------------------------------------------------------- the world
  // The scroll reads right to left: the story walks toward negative x.
  const STATION = {
    title: 0, ithaca: -2600, sea: -6000, lotus: -9200, cave: -12400, circe: -15600, sirens: -18800,
    proteus: -22000, storm: -25200, night: -28900, bow: -32200, continue: -39000, leave: -42500, colophon: -44300,
  };
  const SCROLL_RIGHT_EDGE = 1100;
  const SCROLL_LEFT_EDGE = -45000;
  const HORIZON = 760;
  const GROUND = 836;

  const SEA_RANGES = [[-11400, -3500], [-14600, -13400], [-21000, -16600], [-27400, -23000], [-41000, -37000]];

  const RING = { count: 12, firstX: STATION.bow - 300, spacing: 330, y: 430, speed: 560 };
  const ringX = (index) => RING.firstX - index * RING.spacing;
  const arrowTip = (t) => (t < BEAT.release ? STATION.bow + 180 : STATION.bow + 180 - (t - BEAT.release) * RING.speed);
  const ringPass = (index) => BEAT.release + (STATION.bow + 180 - ringX(index)) / RING.speed;

  // ---------------------------------------------------------------- camera
  // Keyframes: [time, x, y, zoom, depth(0 flat scroll → 1 living world), roll].
  const TABLE_ZOOM = 0.62;
  const KEYS = [
    [0, 700, 540, TABLE_ZOOM, 0, 0],
    [BEAT.unroll[0], 700, 540, TABLE_ZOOM, 0, 0],
    [BEAT.dive[0], -250, 540, TABLE_ZOOM, 0, 0],
    [BEAT.dive[1], 100, 540, 1.02, 1, 0],
    [BEAT.toIthaca[0], -140, 540, 1.02, 1, 0],
    [BEAT.toIthaca[1], STATION.ithaca + 360, 540, 1.02, 1, 0],
    [BEAT.pairing[0], STATION.ithaca + 220, 520, 1.1, 1, 0],
    [BEAT.pairing[1], STATION.ithaca + 120, 520, 1.1, 1, 0],
    [BEAT.toSea[0], STATION.ithaca - 300, 540, 1.02, 1, 0],
    [BEAT.toSea[1], STATION.sea + 200, 540, 1.02, 1, 0],
    [BEAT.years[1], STATION.sea - 200, 540, 1.04, 1, 0],
    [BEAT.toLotus[1], STATION.lotus + 150, 560, 1.08, 1, 0],
    [BEAT.toCave[0], STATION.lotus - 150, 560, 1.1, 1, 0],
    [BEAT.toCave[1], STATION.cave + 60, 570, 1.2, 1, 0],
    [BEAT.sunrise, STATION.cave + 20, 570, 1.24, 1, 0],
    [BEAT.craneFlight[1], STATION.cave - 60, 540, 1.04, 1, 0],
    [BEAT.toCirce[1], STATION.circe + 120, 540, 1.06, 1, 0],
    [BEAT.approve + 1.0, STATION.circe - 60, 540, 1.12, 1, 0],
    [BEAT.toSirens[1], STATION.sirens + 200, 540, 1.02, 1, 0],
    [BEAT.muteSeal, STATION.sirens - 60, 520, 1.14, 1, 0],
    [BEAT.toProteus[1], STATION.proteus + 100, 540, 1.04, 1, 0],
    [BEAT.sameSeal + 0.6, STATION.proteus - 60, 540, 1.08, 1, 0],
    [BEAT.toStorm[1], STATION.storm + 200, 540, 1.03, 1, 0],
    [BEAT.snap, STATION.storm - 100, 540, 1.06, 1, 0],
    [BEAT.silence[1], STATION.storm - 140, 540, 1.04, 1, 0],
    [BEAT.toNight[1], STATION.night + 160, 540, 1.06, 1, 0],
    [BEAT.grab, STATION.night + 40, 540, 1.16, 1, 0],
    [BEAT.lightsOn, STATION.night - 20, 540, 1.08, 1, 0],
    [BEAT.toBow[1], STATION.bow + 320, 540, 1.02, 1, 0],
    [BEAT.release - 0.3, STATION.bow + 380, 500, 1.26, 1, 0],
    [BEAT.release + 0.6, STATION.bow - 200, 500, 1.06, 1, 0],
    [BEAT.lastRing, ringX(RING.count - 1) + 380, 500, 1.06, 1, 0],
    [BEAT.doneSeal + 1.2, ringX(RING.count - 1) + 200, 520, 1.04, 1, 0],
    [BEAT.toContinue[1], STATION.continue - 500, 540, 1.03, 1, 0],
    [BEAT.arrival, STATION.continue + 260, 540, 1.03, 1, 0],
    [BEAT.toLeave[1], STATION.leave + 200, 540, 1.04, 1, 0],
    [BEAT.pullBack[0], STATION.leave - 220, 540, 1.04, 1, 0],
    [BEAT.pullBack[1], STATION.leave - 1300, 540, TABLE_ZOOM, 0, 0],
    [BEAT.rollUp[1], STATION.leave - 1300, 540, TABLE_ZOOM, 0, 0],
    [L.DURATION_SECONDS, STATION.leave - 1300, 540, TABLE_ZOOM * 0.97, 0, 0],
  ];

  function cameraAt(t) {
    let index = 0;
    while (index < KEYS.length - 2 && t >= KEYS[index + 1][0]) index++;
    const a = KEYS[index];
    const b = KEYS[index + 1];
    const p = easeInOut(progress(t, a[0], b[0]));
    const zoom = a[3] * Math.pow(b[3] / a[3], p);
    let x = lerp(a[1], b[1], p);
    let y = lerp(a[2], b[2], p);
    let roll = lerp(a[5], b[5], p);
    const stormShake = progress(t, BEAT.dragon[0], BEAT.dragon[0] + 1) * (1 - progress(t, BEAT.snap, BEAT.snap + 0.2));
    if (stormShake > 0) {
      x += Math.sin(t * 17.3) * 9 * stormShake;
      y += Math.cos(t * 13.1) * 7 * stormShake;
      roll += Math.sin(t * 0.9) * 0.02 * stormShake;
    }
    const hitShake = BEAT.proteusHits.reduce((sum, at) => sum + Math.exp(-Math.max(0, t - at) * 9) * (t >= at ? 1 : 0), 0);
    x += Math.sin(t * 41) * 10 * hitShake;
    y += Math.cos(t * 37) * 8 * hitShake;
    const stampShake = [BEAT.muteSeal, BEAT.sameSeal, BEAT.doneSeal, BEAT.titleSeal].reduce((sum, at) => sum + Math.exp(-Math.max(0, t - at) * 14) * (t >= at ? 1 : 0), 0);
    y += Math.sin(t * 60) * 5 * stampShake;
    const depth = lerp(a[4], b[4], p);
    if (depth > 0.5) {
      // Never let the living painting reveal the scroll's edges.
      const halfHeight = L.HEIGHT / 2 / zoom;
      const halfWidth = L.WIDTH / 2 / zoom;
      y = zoom >= 1 ? clamp(y, halfHeight, L.PAINT_HEIGHT - halfHeight) : L.PAINT_HEIGHT / 2;
      x = clamp(x, SCROLL_LEFT_EDGE + halfWidth, SCROLL_RIGHT_EDGE - halfWidth);
    }
    return { x, y, zoom, depth, roll };
  }

  // ---------------------------------------------------------- environment
  /** 0 = bright day paper, 1 = deep night; drives washes, text colour, glow. */
  function darknessAt(t) {
    const cave = smooth(progress(t, BEAT.toCave[0] + 0.8, BEAT.toCave[1] + 0.4)) * (1 - smooth(progress(t, BEAT.sunrise - 0.2, BEAT.sunrise + 1.0)));
    const storm = smooth(progress(t, BEAT.toStorm[0], BEAT.dragon[0] + 0.8)) * (1 - smooth(progress(t, BEAT.silence[0], BEAT.silence[0] + 1.2)));
    const night = smooth(progress(t, BEAT.toNight[0], BEAT.toNight[1])) * (1 - smooth(progress(t, BEAT.toBow[0] - 0.4, BEAT.toBow[1])));
    const dusk = smooth(progress(t, BEAT.toContinue[0], BEAT.toContinue[1])) * (1 - smooth(progress(t, BEAT.toLeave[0], BEAT.toLeave[1]))) * 0.8;
    const years = BEAT.years[0] < t && t < BEAT.years[1] ? yearsNight(t) * 0.55 : 0;
    return clamp(Math.max(cave * 0.9, storm * 0.85, night * 0.8, dusk, years));
  }

  function yearsNight(t) {
    const cycle = (t - BEAT.years[0]) / yearsCycle();
    return Math.pow(Math.max(0, -Math.sin(cycle * Math.PI * 2)), 1.5) * progress(t, BEAT.years[0], BEAT.years[0] + 0.8) * (1 - progress(t, BEAT.years[1] - 0.6, BEAT.years[1]));
  }
  function yearsCycle() {
    return 0.9;
  }

  /** Silence after the thread breaks: colour drains until only paper remains. */
  function blanknessAt(t) {
    return smooth(progress(t, BEAT.silence[0], BEAT.silence[0] + 0.9)) * (1 - smooth(progress(t, BEAT.silence[1] - 0.2, BEAT.toNight[1])));
  }

  function inSea(x) {
    return SEA_RANGES.some(([from, to]) => x >= from && x <= to);
  }

  // ------------------------------------------------------------ beasts
  function guardianLion(ctx, cx, groundY, t, alpha) {
    L.withAlpha(ctx, alpha, () => {
      const breathe = Math.sin(t * 3) * 4;
      brushStroke(ctx, catmullRom([[cx + 260, groundY - 200], [cx + 60, groundY - 240 + breathe], [cx - 140, groundY - 250 + breathe]], 10), 150, { taperStart: 0.1, taperEnd: 0.25, dry: 0.4, seed: 301 });
      [[-120, 1], [-40, -1], [170, 1], [240, -1]].forEach(([offset, side], i) => {
        brushStroke(ctx, [[cx + offset, groundY - 200], [cx + offset + side * 10, groundY - 100], [cx + offset + side * 4, groundY - 8]], 44, { taperStart: 0.05, taperEnd: 0.2, dry: 0.4, seed: 310 + i });
        for (let c = 0; c < 3; c++) L.inkDot(ctx, cx + offset + side * 4 - 14 + c * 14, groundY - 4, 9, 1, 320 + i * 3 + c);
      });
      const head = [cx - 210, groundY - 330 + breathe];
      for (let ring = 0; ring < 3; ring++) {
        for (let k = 0; k < 14; k++) {
          const angle = (k / 14) * Math.PI * 2 + ring * 0.3;
          const r = 90 + ring * 34;
          ctx.strokeStyle = COLOR.ink;
          ctx.lineWidth = 7 - ring * 1.5;
          const sx = head[0] + Math.cos(angle) * r;
          const sy = head[1] + Math.sin(angle) * r * 0.85;
          ctx.beginPath();
          for (let a = 0; a < Math.PI * 1.6; a += 0.25) {
            const rr = 22 * (1 - a / (Math.PI * 2));
            const px = sx + Math.cos(a + angle) * rr;
            const py = sy + Math.sin(a + angle) * rr;
            if (a === 0) ctx.moveTo(px, py);
            else ctx.lineTo(px, py);
          }
          ctx.stroke();
        }
      }
      ctx.fillStyle = 'rgba(26, 21, 18, 0.92)';
      fillEllipse(ctx, head[0], head[1], 96, 84);
      ctx.fillStyle = COLOR.paperLight;
      fillEllipse(ctx, head[0] - 40, head[1] - 20, 16, 12);
      fillEllipse(ctx, head[0] + 16, head[1] - 22, 16, 12);
      ctx.fillStyle = COLOR.gold;
      fillCircle(ctx, head[0] - 40, head[1] - 20, 6);
      fillCircle(ctx, head[0] + 16, head[1] - 22, 6);
      ctx.fillStyle = COLOR.paperLight;
      ctx.beginPath();
      ctx.moveTo(head[0] - 60, head[1] + 30);
      ctx.quadraticCurveTo(head[0] - 10, head[1] + 70, head[0] + 40, head[1] + 28);
      ctx.fill();
      ctx.fillStyle = COLOR.ink;
      for (let tooth = 0; tooth < 5; tooth++) L.fillPolygon(ctx, [[head[0] - 52 + tooth * 20, head[1] + 32], [head[0] - 44 + tooth * 20, head[1] + 48], [head[0] - 36 + tooth * 20, head[1] + 32]]);
      brushStroke(ctx, catmullRom([[cx + 270, groundY - 220], [cx + 360, groundY - 330], [cx + 330, groundY - 420]], 8), 26, { taperEnd: 0.6, dry: 0.4, seed: 330 });
    });
  }

  function whiteSerpent(ctx, cx, groundY, t, alpha) {
    L.withAlpha(ctx, alpha, () => {
      const spine = [];
      for (let i = 0; i <= 60; i++) {
        const u = i / 60;
        spine.push([cx - 320 + u * 640 + Math.sin(u * 9 + t * 2) * 30 * (1 - u), groundY - 60 - u * u * 420 + Math.sin(u * Math.PI * 3 + t * 2.4) * 70 * (1 - u * 0.6)]);
      }
      spine.reverse();
      ctx.save();
      brushStroke(ctx, spine, 96, { color: 'rgba(26, 21, 18, 0.95)', taperStart: 0.06, taperEnd: 0.7, dry: 0, seed: 350 });
      brushStroke(ctx, spine, 80, { color: 'rgba(245, 240, 228, 0.97)', taperStart: 0.06, taperEnd: 0.72, dry: 0, seed: 350 });
      ctx.strokeStyle = 'rgba(26, 21, 18, 0.35)';
      ctx.lineWidth = 1.6;
      for (let i = 3; i < spine.length - 6; i += 2) {
        const [px, py] = spine[i];
        const width = 34 * (1 - i / spine.length);
        ctx.beginPath();
        ctx.arc(px, py, width * 0.6, 0.4, Math.PI - 0.4);
        ctx.stroke();
      }
      const head = spine[0];
      ctx.fillStyle = 'rgba(245, 240, 228, 1)';
      ctx.strokeStyle = COLOR.ink;
      ctx.lineWidth = 4;
      ctx.beginPath();
      ctx.ellipse(head[0] + 30, head[1] - 10, 70, 44, -0.25, 0, Math.PI * 2);
      ctx.fill();
      ctx.stroke();
      ctx.fillStyle = COLOR.cinnabar;
      fillCircle(ctx, head[0] + 52, head[1] - 22, 7);
      ctx.strokeStyle = COLOR.ink;
      ctx.lineWidth = 3;
      const flick = Math.sin(t * 16) > 0 ? 1 : 0.5;
      L.strokePolyline(ctx, [[head[0] + 98, head[1] - 4], [head[0] + 98 + 40 * flick, head[1] + 2], [head[0] + 112 + 40 * flick, head[1] - 10]]);
      L.strokePolyline(ctx, [[head[0] + 98 + 40 * flick, head[1] + 2], [head[0] + 114 + 40 * flick, head[1] + 12]]);
      ctx.restore();
    });
  }

  function waterSpirit(ctx, cx, groundY, t, alpha) {
    L.withAlpha(ctx, alpha, () => {
      const rise = Math.sin(t * 2.4) * 10;
      const crest = [[cx + 320, groundY], [cx + 300, groundY - 300], [cx + 160, groundY - 560 + rise], [cx - 40, groundY - 600 + rise], [cx - 210, groundY - 520 + rise], [cx - 250, groundY - 400], [cx - 170, groundY - 350], [cx - 120, groundY - 420]];
      ctx.save();
      const body = ctx.createLinearGradient(0, groundY - 600, 0, groundY);
      body.addColorStop(0, 'rgba(40, 56, 84, 0.85)');
      body.addColorStop(1, 'rgba(40, 56, 84, 0.3)');
      ctx.fillStyle = body;
      const outline = catmullRom(crest.concat([[cx - 60, groundY - 300], [cx + 40, groundY - 150], [cx - 260, groundY]]), 10, true);
      L.polygonPath(ctx, outline);
      ctx.fill();
      brushStroke(ctx, catmullRom(crest, 12), 20, { taperStart: 0.05, taperEnd: 0.2, dry: 0.5, seed: 360 });
      for (let k = 0; k < 6; k++) {
        const strand = crest.slice(0, 5).map(([x, y], i) => [x - k * 40 + i * 6, y + k * 48 + 30]);
        brushStroke(ctx, catmullRom(strand, 8), 5, { alpha: 0.7, taperEnd: 0.7, dry: 0.3, seed: 361 + k });
      }
      ctx.fillStyle = COLOR.paperLight;
      fillEllipse(ctx, cx - 30, groundY - 470 + rise, 20, 11);
      fillEllipse(ctx, cx + 40, groundY - 480 + rise, 20, 11);
      ctx.fillStyle = COLOR.ink;
      fillCircle(ctx, cx - 26, groundY - 470 + rise, 6);
      fillCircle(ctx, cx + 44, groundY - 480 + rise, 6);
      const random = seededRandom(4);
      for (let i = 0; i < 24; i++) {
        const angle = Math.PI * (1 + random());
        const reach = 60 + ((t * 90 + random() * 200) % 200);
        fillCircle(ctx, cx - 180 + Math.cos(angle) * reach, groundY - 520 + Math.sin(angle) * reach * 0.6 + rise, 3 + random() * 5);
      }
      ctx.restore();
    });
  }

  function firePhoenix(ctx, cx, groundY, t, alpha) {
    L.withAlpha(ctx, alpha, () => {
      const lift = Math.sin(t * 3) * 14;
      const body = [cx - 60, groundY - 360 + lift];
      drawGlow(ctx, body[0], body[1], 420, RGB.gold, 0.55);
      const flap = Math.sin(t * 6);
      [-1, 1].forEach((side) => {
        for (let f = 0; f < 7; f++) {
          const angle = -Math.PI / 2 + side * (0.4 + f * 0.2) + flap * 0.2 * side;
          const length = 200 + f * 26;
          brushStroke(ctx, [[body[0], body[1]], [body[0] + Math.cos(angle) * length * 0.5, body[1] + Math.sin(angle) * length * 0.6], [body[0] + Math.cos(angle) * length, body[1] + Math.sin(angle) * length * 0.9]], 22, { color: f % 2 ? 'rgba(200, 120, 40, 0.8)' : 'rgba(26, 21, 18, 0.85)', taperStart: 0.1, taperEnd: 0.8, dry: 0.3, seed: 370 + f + (side > 0 ? 10 : 0) });
        }
      });
      for (let k = 0; k < 5; k++) {
        const tail = [];
        for (let s = 0; s <= 14; s++) {
          const u = s / 14;
          tail.push([body[0] + 40 + u * (380 + k * 40), body[1] + 40 + u * (160 + k * 30) + Math.sin(u * 6 + t * 3 + k) * 30 * u]);
        }
        brushStroke(ctx, tail, 18 - k * 2, { color: k % 2 ? 'rgba(214, 150, 60, 0.8)' : 'rgba(26, 21, 18, 0.85)', taperStart: 0.05, taperEnd: 0.5, dry: 0.3, seed: 390 + k });
        const end = tail[tail.length - 1];
        drawGlow(ctx, end[0], end[1], 60, RGB.gold, 0.8);
        ctx.fillStyle = 'rgba(26, 21, 18, 0.9)';
        fillEllipse(ctx, end[0], end[1], 20, 12, 0.5);
        ctx.fillStyle = 'rgba(226, 179, 90, 0.95)';
        fillCircle(ctx, end[0], end[1], 6);
      }
      ctx.fillStyle = 'rgba(26, 21, 18, 0.95)';
      fillEllipse(ctx, body[0], body[1], 70, 40, -0.3);
      brushStroke(ctx, [[body[0] - 40, body[1] - 20], [body[0] - 90, body[1] - 90], [body[0] - 120, body[1] - 110]], 26, { taperEnd: 0.4, dry: 0.2, seed: 399 });
      ctx.fillStyle = COLOR.gold;
      fillCircle(ctx, body[0] - 124, body[1] - 118, 7);
      L.fillPolygon(ctx, [[body[0] - 140, body[1] - 116], [body[0] - 176, body[1] - 104], [body[0] - 140, body[1] - 104]]);
      for (let i = 0; i < 26; i++) {
        const random = seededRandom(i + Math.floor(t * 12));
        drawGlow(ctx, body[0] + (random() - 0.5) * 500, body[1] + 100 - random() * 400, 10 + random() * 16, RGB.gold, 0.7);
      }
    });
  }

  // ------------------------------------------------------------- stations
  function paintGround(ctx, from, to, y, seed) {
    brushStroke(ctx, [[from, y + 4], [(from + to) / 2, y - 2], [to, y + 6]], 9, { dry: 0.5, taperStart: 0.02, taperEnd: 0.05, seed, alpha: 0.85 });
    const random = seededRandom(seed);
    for (let x = from; x < to; x += 90 + random() * 120) {
      for (let k = 0; k < 4; k++) {
        const angle = -Math.PI / 2 + (random() - 0.5) * 1.1;
        const length = 10 + random() * 22;
        ctx.strokeStyle = `rgba(26, 21, 18, ${0.35 + random() * 0.4})`;
        ctx.lineWidth = 1.6;
        strokeLine(ctx, x, y, x + Math.cos(angle) * length, y + Math.sin(angle) * length);
      }
    }
  }

  function stationTitle(ctx, t) {
    L.verticalCalligraphy(ctx, '循環', 440, 250, 250, t, BEAT.titleBrush, 0.9, { font: L.FONT.brush });
    L.verticalCalligraphy(ctx, '傳說', 180, 300, 96, t, BEAT.titleBrush + 1.6, 0.45, { font: L.FONT.running, alpha: 0.85 });
    L.seal(ctx, 300, 790, 124, 'LOOP', progress(t, BEAT.titleSeal, BEAT.titleSeal + 0.7), { font: L.FONT.serif, weight: 700, style: 'yin', columns: 1, textSize: 40 });
    const englishReveal = progress(t, BEAT.titleEnglish, BEAT.titleEnglish + 1.6);
    if (englishReveal > 0) {
      ctx.save();
      ctx.font = `600 50px ${L.FONT.serif}`;
      ctx.letterSpacing = '20px';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      ctx.globalAlpha = smooth(englishReveal);
      if (englishReveal < 1) ctx.filter = `blur(${((1 - englishReveal) * 6).toFixed(2)}px)`;
      ctx.fillStyle = COLOR.ink;
      ctx.fillText('THE LEGEND OF THE LOOPER', -560, 470);
      ctx.font = `italic 500 30px ${L.FONT.serif}`;
      ctx.letterSpacing = '4px';
      ctx.fillStyle = 'rgba(26, 21, 18, 0.75)';
      ctx.fillText('as painted on a scroll, and told by the fire', -560, 540);
      ctx.restore();
    }
    for (let i = 0; i < 7; i++) {
      const x = 700 - ((t * 60 + i * 140) % 2200);
      const y = 260 + Math.sin(i * 1.7) * 60 + Math.sin(t + i) * 8;
      const flap = Math.sin(t * 5 + i) * 6;
      ctx.strokeStyle = 'rgba(26, 21, 18, 0.7)';
      ctx.lineWidth = 2.2;
      ctx.beginPath();
      ctx.moveTo(x - 14, y - flap);
      ctx.quadraticCurveTo(x - 6, y - 6, x, y);
      ctx.quadraticCurveTo(x + 6, y - 6, x + 14, y - flap);
      ctx.stroke();
    }
    paintGround(ctx, -1400, 1100, GROUND + 40, 11);
    L.pineTree(ctx, 820, GROUND + 40, 1.1, 12);
  }

  function ithacaThreadPath(loomPoint, orb, hand) {
    const hanging = quadraticPoints(loomPoint, orb, 70, 40);
    const rising = quadraticPoints(orb, hand, -110, 40);
    return hanging.concat(rising.slice(1));
  }

  function stationIthaca(ctx, t) {
    paintGround(ctx, -620, 1500, GROUND, 21);
    L.pavilion(ctx, 1160, GROUND - 6, 0.72, 0.5 + 0.2 * Math.sin(t * 2), 7);
    L.olivePainting(ctx, 330, GROUND, 0.95, t, 5);
    L.writingDesk(ctx, 300, 690, GROUND, 360);
    const laptop = L.inkLaptop(ctx, 300, 684, 0.52, { t, glow: 1 });
    const loom = L.loom(ctx, 760, GROUND, 0.9, t, 0);
    L.drawPuppet(ctx, { x: 600, y: GROUND, costume: 'queen', facing: -1, t, seed: 3, pose: { head: 0.08, reachNear: [70, -300], reachFar: [40, -270] } });
    const walking = t > BEAT.departWalk[0];
    const walkP = progress(t, BEAT.departWalk[0], BEAT.departWalk[1]);
    const kingX = walking ? lerp(-60, -560, walkP) : -60;
    const king = L.drawPuppet(ctx, {
      x: kingX, y: GROUND, costume: 'king', facing: walking ? -1 : 1, t, seed: 1,
      pose: walking ? { walkPhase: t * 7 } : { head: -0.05, reachNear: [150, -420], reachFar: [40, -250] },
    });
    const hand = [king.handNear[0], king.handNear[1] - 14];
    L.inkPhone(ctx, hand[0], hand[1], 22, { glow: progress(t, BEAT.pairing[1] - 0.4, BEAT.pairing[1]) + 0.25, icon: t > BEAT.pairing[1] - 0.3 ? 'running' : 'none' });
    const reveal = easeInOut(progress(t, BEAT.pairing[0], BEAT.pairing[1]));
    if (reveal > 0) {
      const path = ithacaThreadPath(loom.clothBottom, laptop.orb, hand);
      const tip = L.redThread(ctx, path, { reveal });
      if (reveal < 1 && tip) drawGlow(ctx, tip[0], tip[1], 50, RGB.thread, 0.9);
    }
    const ripple = progress(t, BEAT.pairing[1] - 0.5, BEAT.pairing[1] + 1.6);
    if (ripple > 0 && ripple < 1) {
      for (let k = 0; k < 3; k++) {
        const r = (ripple * 1.2 - k * 0.15) * 520;
        if (r <= 0) continue;
        ctx.strokeStyle = `rgba(95, 211, 154, ${0.6 * (1 - ripple)})`;
        ctx.lineWidth = 3;
        ctx.beginPath();
        ctx.arc(laptop.orb[0], laptop.orb[1], r, 0, Math.PI * 2);
        ctx.stroke();
      }
      L.jadeDisc(ctx, laptop.orb[0], laptop.orb[1] - 180, 70 * easeOutBack(clamp(ripple * 2)), 1 - ripple, t);
    }
    L.galley(ctx, -1100, HORIZON + 30, 0.8, t, { sail: true, oars: false, seed: 5 });
  }

  function stationSea(ctx, t) {
    const sailing = progress(t, BEAT.toSea[0], BEAT.toLotus[1]);
    const shipX = lerp(900, -1300, sailing);
    const bob = Math.sin(t * 1.4) * 8;
    if (t < BEAT.toSea[0] - 0.5) return;
    const ship = L.galley(ctx, shipX, HORIZON + 40 + bob, 0.9, t, { tilt: Math.sin(t * 1.1) * 0.03, seed: 6 });
    L.redThread(ctx, catmullRom([ship.stern, [ship.stern[0] + 600, ship.stern[1] - 120], [ship.stern[0] + 1400, ship.stern[1] - 60], [ship.stern[0] + 3200, ship.stern[1] - 140]], 16), { reveal: easeOut(progress(t, BEAT.toSea[0] - 0.5, BEAT.toSea[1])) });
    if (t > BEAT.years[0] - 0.5 && t < BEAT.years[1] + 0.5) {
      const cycle = (t - BEAT.years[0]) / yearsCycle();
      const fade = progress(t, BEAT.years[0] - 0.3, BEAT.years[0] + 0.5) * (1 - progress(t, BEAT.years[1] - 0.2, BEAT.years[1] + 0.5));
      const sunAngle = Math.PI * (cycle % 1) * 2;
      const sunPos = [Math.cos(sunAngle + Math.PI) * 820, HORIZON - Math.sin(sunAngle) * 560];
      if (Math.sin(sunAngle) > -0.05) L.sunDisc(ctx, sunPos[0], sunPos[1], 56, fade);
      const moonAngle = sunAngle + Math.PI;
      const moonPos = [Math.cos(moonAngle + Math.PI) * 820, HORIZON - Math.sin(moonAngle) * 560];
      if (Math.sin(moonAngle) > -0.05) L.moon(ctx, moonPos[0], moonPos[1], 46, fade);
      const passed = Math.min(10, Math.floor(clamp(cycle / ((BEAT.years[1] - BEAT.years[0]) / yearsCycle())) * 10 + 1));
      L.brushCharacter(ctx, ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十'][passed - 1], 560, 240, 120, fade, { font: L.FONT.running, alpha: 0.85 });
    }
    L.seal(ctx, 700, 420, 120, '十年', progress(t, BEAT.yearsSeal, BEAT.yearsSeal + 0.7), { style: 'yin', columns: 1 });
    const englishReveal = progress(t, BEAT.yearsSeal + 0.5, BEAT.yearsSeal + 1.6);
    if (englishReveal > 0) {
      ctx.save();
      ctx.font = `600 34px ${L.FONT.serif}`;
      ctx.letterSpacing = '12px';
      ctx.textAlign = 'center';
      ctx.globalAlpha = englishReveal;
      ctx.fillStyle = COLOR.ink;
      ctx.fillText('TEN YEARS AT SEA', 700, 520);
      ctx.restore();
    }
  }

  function stationLotus(ctx, t) {
    const random = seededRandom(51);
    for (let i = 0; i < 16; i++) {
      const x = -1200 + random() * 2400;
      const y = HORIZON + 60 + random() * 240;
      L.lotusLeaf(ctx, x, y, 60 + random() * 90, 0.28 + (y - HORIZON) / 1200, i + 1);
    }
    for (let i = 0; i < 9; i++) {
      const x = -1100 + random() * 2200;
      const baseY = HORIZON + 80 + random() * 180;
      const height = 120 + random() * 200;
      const sway = Math.sin(t * 1.2 + i) * 12;
      brushStroke(ctx, [[x, baseY], [x + sway * 0.4, baseY - height * 0.5], [x + sway, baseY - height]], 5, { taperEnd: 0.1, dry: 0, alpha: 0.8, seed: 60 + i });
      L.lotusFlower(ctx, x + sway, baseY - height, 46 + random() * 20, 0.8 + 0.2 * Math.sin(t + i), i);
    }
    const ship = L.galley(ctx, 40, HORIZON + 50 + Math.sin(t * 1.2) * 5, 0.95, t, { sail: false, oars: false, seed: 7 });
    L.drawPuppet(ctx, {
      x: ship.deck[0] + 60, y: ship.deck[1], costume: 'king', facing: -1, t, seed: 1, kind: {},
      pose: { hip: [0, -50], lean: -0.9, head: 0.6, footNear: [200, -6], footFar: [180, 0], reachFar: [-150, -10], reachNear: (a) => [a.headCenter[0] + 40, a.headCenter[1] + 14], elbowNear: 1 },
      decorate: (g, anchors) => L.lotusFlower(g, anchors.armNear.ex + 4, anchors.armNear.ey + 8, 26, 1, 3),
    });
    [-150, -260].forEach((offset, i) => L.drawPuppet(ctx, {
      x: ship.deck[0] + offset, y: ship.deck[1], costume: 'sailor', facing: 1, t, seed: 10 + i, kind: { eyesClosed: true },
      pose: { hip: [0, -60], lean: 0.5, head: 0.6, footNear: [110, -4], footFar: [96, 0], reachNear: [100, -100], reachFar: [90, -90] },
    }));
    L.inkPhone(ctx, ship.deck[0] + 330, ship.deck[1] - 20, 16, { glow: 0.9 + 0.1 * Math.sin(t * 3), icon: 'running', angle: Math.PI / 2 });
    uiTag(ctx, ship.deck[0] + 340, ship.deck[1] - 110, 'Codex · running', true, progress(t, BEAT.toLotus[1] + 1, BEAT.toLotus[1] + 1.6));
    for (let i = 0; i < 14; i++) {
      const r = seededRandom(700 + i);
      const x = -900 + r() * 1800 - ((t * 30) % 400);
      const y = 200 + ((t * 40 + r() * 600) % 600);
      ctx.save();
      ctx.translate(x, y);
      ctx.rotate(t * (0.5 + r()) + i);
      ctx.fillStyle = 'rgba(214, 150, 150, 0.6)';
      fillEllipse(ctx, 0, 0, 9, 4);
      ctx.restore();
    }
  }

  function uiTag(ctx, x, y, text, running, reveal = 1) {
    if (reveal <= 0) return;
    ctx.save();
    ctx.globalAlpha *= reveal;
    ctx.font = `600 22px ${L.FONT.ui}`;
    const width = ctx.measureText(text).width + (running ? 64 : 40);
    ctx.fillStyle = 'rgba(18, 16, 14, 0.88)';
    ctx.beginPath();
    ctx.roundRect(x - width / 2, y - 22, width, 44, 22);
    ctx.fill();
    if (running) {
      drawGlow(ctx, x - width / 2 + 26, y, 26, RGB.jade, 0.9);
      ctx.fillStyle = COLOR.jadeGlow;
      fillCircle(ctx, x - width / 2 + 26, y, 7);
    }
    ctx.fillStyle = COLOR.paperLight;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    ctx.fillText(text, x - width / 2 + (running ? 44 : 20), y + 1);
    ctx.restore();
  }

  function stationCave(ctx, t) {
    L.rockMass(ctx, [[-980, GROUND + 20], [-820, 360], [-560, 170], [-200, 110], [180, 140], [520, 230], [820, 420], [980, GROUND + 20]], { tone: 0.9, seed: 81, texture: 40 });
    const mouthLight = smooth(progress(t, BEAT.sunrise - 0.3, BEAT.sunrise + 0.8));
    ctx.save();
    ctx.fillStyle = 'rgba(8, 6, 5, 0.97)';
    ctx.beginPath();
    ctx.moveTo(-620, GROUND + 10);
    ctx.bezierCurveTo(-640, 400, -300, 300, 0, 300);
    ctx.bezierCurveTo(300, 300, 640, 400, 620, GROUND + 10);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
    if (mouthLight > 0) {
      for (let k = 0; k < 6; k++) {
        ctx.save();
        ctx.globalCompositeOperation = 'screen';
        const gradient = ctx.createLinearGradient(700, 300, 0, GROUND);
        gradient.addColorStop(0, `rgba(255, 210, 130, ${0.28 * mouthLight})`);
        gradient.addColorStop(1, 'rgba(255, 210, 130, 0)');
        ctx.fillStyle = gradient;
        ctx.beginPath();
        ctx.moveTo(700, 260 + k * 30);
        ctx.lineTo(-200 + k * 90, GROUND);
        ctx.lineTo(-100 + k * 90, GROUND);
        ctx.closePath();
        ctx.fill();
        ctx.restore();
      }
    }
    for (let i = 0; i < 5; i++) {
      const flicker = Math.sin(t * 9 + i * 2) * 10;
      drawGlow(ctx, 60, GROUND - 40, 260 + flicker, RGB.candle, 0.5);
    }
    ctx.save();
    ctx.translate(60, GROUND);
    for (let i = 0; i < 7; i++) {
      const h = 70 + Math.abs(Math.sin(t * 7 + i * 1.3)) * 60;
      const x = -54 + i * 18;
      ctx.fillStyle = i % 2 ? 'rgba(226, 150, 60, 0.9)' : 'rgba(255, 210, 120, 0.9)';
      ctx.beginPath();
      ctx.moveTo(x - 14, 0);
      ctx.quadraticCurveTo(x - 12, -h * 0.5, x + Math.sin(t * 5 + i) * 8, -h);
      ctx.quadraticCurveTo(x + 12, -h * 0.4, x + 14, 0);
      ctx.fill();
    }
    ctx.restore();
    L.drawPuppet(ctx, {
      x: -380, y: GROUND, costume: 'giant', scale: 1.9, facing: 1, t, seed: 20, glow: 0.8,
      pose: { hip: [0, -100], lean: 0.25, head: 0.1, footNear: [150, -6], footFar: [120, 0], reachNear: [170, -130], reachFar: [130, -120] },
    });
    const out = smooth(progress(t, BEAT.sunrise, BEAT.sunrise + 1.2));
    const kingX = lerp(300, 620, out);
    const king = L.drawPuppet(ctx, {
      x: kingX, y: GROUND, costume: 'king', facing: out > 0.05 ? 1 : -1, t, seed: 1,
      pose: out > 0.05 && out < 0.95 ? { walkPhase: t * 7 } : { head: 0.3, reachNear: [60, -320], reachFar: [44, -300] },
    });
    const phone = [king.handNear[0], king.handNear[1] - 12];
    L.inkPhone(ctx, phone[0], phone[1], 18, { icon: 'queued', glow: 0.7 });
    const queued = t >= BEAT.craneQueued;
    if (queued) {
      const flight = easeInOut(progress(t, BEAT.craneFlight[0], BEAT.craneFlight[1]));
      const waitingSpot = [phone[0] - 20, phone[1] - 170 + Math.sin(t * 3) * 10];
      const path = catmullRom([waitingSpot, [720, 360], [1200, 200], [2000, 80]], 12);
      const { point, angle } = flight > 0 ? pointOnPath(path, flight) : { point: waitingSpot, angle: 0 };
      const fold = easeOutBack(progress(t, BEAT.craneQueued, BEAT.craneQueued + 0.8));
      L.paperCrane(ctx, point[0], point[1], 0.9 * fold, flight > 0 ? angle * 0.3 : Math.sin(t * 2) * 0.05, flight > 0 ? t * 14 : 0.3 + Math.sin(t * 1.5) * 0.1, { glow: flight > 0 ? 0.8 : 0.2 });
      if (flight <= 0) uiTag(ctx, waitingSpot[0], waitingSpot[1] - 80, 'queued · ship the migration', false, clamp(fold));
      if (flight > 0) L.redThread(ctx, catmullRom([phone, lerpPoint(phone, point, 0.5).map((v, i) => v - (i ? 60 : 0)), point], 12), { alpha: 0.8 });
    }
  }

  function stationCirce(ctx, t) {
    paintGround(ctx, -1000, 1000, GROUND, 91);
    L.pavilion(ctx, 0, GROUND - 4, 1.25, 0.9 + 0.1 * Math.sin(t * 3), 17);
    for (let i = 0; i < 6; i++) L.lantern(ctx, -390 + i * 156, 180 + (i % 2) * 20, 34, t, 1);
    L.writingDesk(ctx, -40, 740, GROUND, 260);
    const king = L.drawPuppet(ctx, {
      x: -220, y: GROUND, costume: 'king', facing: 1, t, seed: 1,
      pose: { hip: [0, -110], lean: -0.1, head: 0.1, footNear: [120, -4], footFar: [96, 0], reachNear: [110, -290], reachFar: [140, -200] },
    });
    ctx.save();
    ctx.fillStyle = 'rgba(26, 21, 18, 0.9)';
    const cup = [king.handNear[0] + 6, king.handNear[1] - 16];
    ctx.beginPath();
    ctx.moveTo(cup[0] - 22, cup[1]);
    ctx.quadraticCurveTo(cup[0], cup[1] + 26, cup[0] + 22, cup[1]);
    ctx.fill();
    ctx.fillRect(cup[0] - 2, cup[1] + 12, 4, 14);
    ctx.restore();
    const circe = L.drawPuppet(ctx, {
      x: 240, y: GROUND, costume: 'enchantress', facing: -1, t, seed: 4,
      pose: { head: 0.12, reachNear: [110, -300], reachFar: [70, -340] },
    });
    const pour = [circe.handNear[0], circe.handNear[1]];
    ctx.save();
    ctx.fillStyle = 'rgba(40, 32, 60, 0.9)';
    fillEllipse(ctx, pour[0] - 10, pour[1] - 10, 22, 28, -0.6);
    ctx.strokeStyle = 'rgba(120, 60, 60, 0.7)';
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(pour[0] - 30, pour[1] + 4);
    ctx.quadraticCurveTo(pour[0] - 80, pour[1] + 40, cup[0] + 6, cup[1] - 2);
    ctx.stroke();
    ctx.restore();
    const unfurl = easeOut(progress(t, BEAT.askScroll, BEAT.askScroll + 0.8));
    const rollAway = easeIn(progress(t, BEAT.approve + 1.4, BEAT.approve + 2.2));
    if (unfurl > 0 && rollAway < 1) {
      const sx = 40;
      const sy = 220;
      const height = 360 * unfurl * (1 - rollAway);
      const approved = t >= BEAT.approve;
      if (approved) drawGlow(ctx, sx, sy + height / 2, 280, RGB.jade, 0.6 * (1 - rollAway));
      ctx.save();
      ctx.fillStyle = COLOR.paperLight;
      ctx.strokeStyle = 'rgba(26, 21, 18, 0.7)';
      ctx.lineWidth = 2;
      ctx.fillRect(sx - 120, sy, 240, height);
      ctx.strokeRect(sx - 120, sy, 240, height);
      L.brocadeBorder(ctx, sx - 132, sx + 132, sy - 18, 18);
      L.brocadeBorder(ctx, sx - 132, sx + 132, sy + height, 18, true);
      ctx.beginPath();
      ctx.rect(sx - 120, sy, 240, height);
      ctx.clip();
      ctx.fillStyle = COLOR.ink;
      ctx.textAlign = 'center';
      ctx.font = `600 26px ${L.FONT.serif}`;
      ctx.fillText('Claude asks', sx, sy + 56);
      ctx.font = `italic 500 30px ${L.FONT.serif}`;
      ctx.fillText('may I run', sx, sy + 110);
      ctx.font = `600 26px ${L.FONT.mono}`;
      ctx.fillText('cargo test', sx, sy + 156);
      ctx.restore();
      L.seal(ctx, sx, sy + 258, 96, '准', progress(t, BEAT.approve, BEAT.approve + 0.6), { style: 'yin' });
      if (approved) uiTag(ctx, sx, sy + 340, 'Allowed · running', true, progress(t, BEAT.approve + 0.3, BEAT.approve + 0.7) * (1 - rollAway));
    }
  }

  const SIREN_SONG = ['come home…', 'open the laptop…', 'fix it yourself…', 'nobody does it like you…', 'just check once…', 'come home…', 'it needs you…', 'open it…', 'come home…'];

  function karstPillar(ctx, x, baseY, height, width, seed, tone = 0.8) {
    const points = [[x - width * 0.5, baseY], [x - width * 0.46, baseY - height * 0.5], [x - width * 0.38, baseY - height * 0.86], [x - width * 0.1, baseY - height], [x + width * 0.24, baseY - height * 0.95], [x + width * 0.4, baseY - height * 0.7], [x + width * 0.5, baseY]];
    L.rockMass(ctx, points, { tone, seed, texture: 14 });
    L.pineTree(ctx, x - width * 0.1, baseY - height + 6, 0.35, seed);
  }

  function stationSirens(ctx, t) {
    karstPillar(ctx, -600, HORIZON + 40, 440, 280, 111, 0.75);
    karstPillar(ctx, 430, HORIZON + 40, 400, 250, 112, 0.7);
    karstPillar(ctx, 860, HORIZON + 40, 300, 200, 113, 0.55);
    L.mistBand(ctx, -1400, 1400, HORIZON - 40, t, { alpha: 0.9, seed: 5, height: 160 });
    const shipX = lerp(260, -160, progress(t, BEAT.toSirens[0], BEAT.toProteus[0]));
    const ship = L.galley(ctx, shipX, HORIZON + 50 + Math.sin(t * 1.3) * 5, 0.95, t, { sail: false, seed: 8 });
    const hero = L.drawPuppet(ctx, {
      x: ship.mastBase[0] + 30, y: ship.mastBase[1], costume: 'king', facing: -1, t, seed: 1, kind: { eyesClosed: true },
      pose: { head: 0.3, reachNear: [-30, -170], reachFar: [-36, -180], elbowNear: 1, elbowFar: 1 },
      decorate: (g) => {
        g.strokeStyle = 'rgba(214, 180, 120, 0.95)';
        g.lineWidth = 4;
        [-240, -200, -150].forEach((y) => strokeLine(g, -50, y, 40, y + 20));
      },
    });
    const dome = hero.head;
    const domeRadius = 230;
    const muted = t >= BEAT.muteSeal;
    const perches = [[-600, HORIZON + 40 - 440], [430, HORIZON + 40 - 400], [860, HORIZON + 40 - 300]];
    const mouths = perches.map(([px, py], i) => L.drawSiren(ctx, px + Math.sin(t * 0.8 + i) * 30, py - 40 + Math.sin(t * 1.3 + i) * 16, 0.85, t, px < 0 ? 1 : -1, i + 1));
    ctx.save();
    ctx.strokeStyle = `rgba(214, 170, 90, ${muted ? 0.8 : 0.35})`;
    ctx.setLineDash([10, 12]);
    ctx.lineDashOffset = -t * 30;
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    ctx.arc(dome[0], dome[1], domeRadius, 0, Math.PI * 2);
    ctx.stroke();
    ctx.restore();
    const songEnd = muted ? BEAT.muteSeal : Infinity;
    SIREN_SONG.forEach((word, index) => {
      const spawn = BEAT.song[0] + index * 0.55;
      const age = t - spawn;
      if (age < 0 || spawn > songEnd) return;
      const mouth = mouths[index % mouths.length];
      const flight = clamp(age / 1.6);
      const control = [(mouth[0] + dome[0]) / 2, Math.min(mouth[1], dome[1]) - 160];
      const path = quadraticPoints(mouth, dome, control[1] - (mouth[1] + dome[1]) / 2, 40);
      const lengths = L.polylineLengths(path);
      const total = lengths[lengths.length - 1];
      const stopFraction = clamp((total - domeRadius) / total);
      const along = Math.min(flight, stopFraction);
      const { point, angle } = pointOnPath(path, along, lengths);
      const shatter = muted ? progress(t, BEAT.muteSeal, BEAT.muteSeal + 0.6) : clamp((flight - stopFraction) / 0.25);
      const ribbonTail = pointOnPath(path, Math.max(0, along - 0.2), lengths).point;
      ctx.save();
      ctx.globalAlpha = 1 - shatter;
      brushStroke(ctx, [ribbonTail, lerpPoint(ribbonTail, point, 0.5), point], 5, { alpha: 0.35, dry: 0, taperStart: 0.4, taperEnd: 0.1, seed: index });
      ctx.translate(point[0], point[1]);
      ctx.rotate(clamp(angle, -0.5, 0.5));
      ctx.font = `italic 600 50px ${L.FONT.serif}`;
      ctx.letterSpacing = `${shatter * 30}px`;
      ctx.shadowColor = 'rgba(243, 235, 216, 0.95)';
      ctx.shadowBlur = 10;
      ctx.textAlign = 'center';
      ctx.fillStyle = COLOR.ink;
      if (shatter > 0) ctx.filter = `blur(${(shatter * 6).toFixed(1)}px)`;
      ctx.fillText(word, 0, -12);
      ctx.restore();
    });
    const stamp = progress(t, BEAT.muteSeal, BEAT.muteSeal + 0.7);
    if (stamp > 0) {
      ctx.save();
      const press = easeOut(clamp(stamp / 0.35));
      const radius = domeRadius * lerp(1.4, 1, press);
      ctx.strokeStyle = COLOR.cinnabar;
      ctx.globalAlpha = press;
      ctx.lineWidth = 14;
      ctx.beginPath();
      ctx.arc(dome[0], dome[1], radius, 0, Math.PI * 2);
      ctx.stroke();
      ctx.lineWidth = 4;
      ctx.beginPath();
      ctx.arc(dome[0], dome[1], radius - 20, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();
      L.seal(ctx, dome[0], dome[1] - domeRadius - 10, 104, '靜', stamp, { style: 'yin', rounded: 0.5 });
    }
    L.inkPhone(ctx, hero.hip[0] - 20, hero.hip[1] + 20, 14, { icon: 'running', glow: 0.6 });
  }

  const PROTEUS_FORMS = [guardianLion, whiteSerpent, waterSpirit, firePhoenix];

  function stationProteus(ctx, t) {
    paintGround(ctx, -1100, 1100, GROUND, 131);
    L.rockMass(ctx, [[480, GROUND + 10], [560, GROUND - 130], [700, GROUND - 170], [860, GROUND - 90], [920, GROUND + 10]], { tone: 0.7, seed: 132 });
    let form = -1;
    BEAT.proteusHits.forEach((at, i) => {
      if (t >= at) form = i;
    });
    const cx = 60;
    if (form < 0) {
      L.drawPuppet(ctx, { x: cx, y: GROUND, costume: 'sailor', facing: -1, t, seed: 30, scale: 1.2, pose: { hip: [0, -90], lean: 0.35, head: 0.3, footNear: [110, -4], footFar: [90, 0], reachNear: [120, -130], reachFar: [100, -120] } });
    } else {
      PROTEUS_FORMS[form](ctx, cx, GROUND, t, 1);
    }
    BEAT.proteusHits.forEach((at, i) => {
      const bloom = progress(t, at - 0.14, at + 0.28);
      if (bloom > 0 && bloom < 1) L.inkSplash(ctx, cx, GROUND - 320, 420, bloom < 0.5 ? bloom * 2 : 1, 500 + i, bloom < 0.5 ? 1 : 1 - (bloom - 0.5) * 2);
    });
    if (form >= 0) L.seal(ctx, cx + 500, 250, 140, L.SESSION_NAMES[form], progress(t, BEAT.proteusHits[form] + 0.1, BEAT.proteusHits[form] + 0.6), { font: L.FONT.serif, weight: 700, style: 'yang', columns: 1, textSize: form === 1 ? 34 : 40 });
    uiTag(ctx, cx, 150, 'session · ody-7f3a', true, progress(t, BEAT.toProteus[1] - 0.2, BEAT.toProteus[1] + 0.4));
    const king = L.drawPuppet(ctx, { x: -620, y: GROUND, costume: 'king', facing: 1, t, seed: 1, pose: { head: 0.05, reachNear: [80, -300], reachFar: [20, -240] } });
    L.inkPhone(ctx, king.handNear[0], king.handNear[1] - 12, 18, { icon: 'running', glow: 0.9 });
    const same = progress(t, BEAT.sameSeal, BEAT.sameSeal + 0.7);
    if (same > 0) {
      L.seal(ctx, cx + 500, 480, 150, '同', same, { style: 'yin' });
      ctx.save();
      ctx.font = `600 38px ${L.FONT.serif}`;
      ctx.letterSpacing = '14px';
      ctx.textAlign = 'center';
      ctx.globalAlpha = progress(t, BEAT.sameSeal + 0.3, BEAT.sameSeal + 1.0);
      ctx.fillStyle = COLOR.ink;
      ctx.fillText('SAME SESSION', cx + 500, 610);
      ctx.restore();
    }
  }

  function dragonSpine(t) {
    const p = progress(t, BEAT.dragon[0], BEAT.dragon[1] + 1.4);
    const headX = lerp(-1900, 2300, easeInOut(p));
    const spine = [];
    for (let i = 0; i <= 70; i++) {
      const back = i * 34;
      const x = headX - back;
      const y = 380 + Math.sin(x * 0.0028 + t * 1.2) * 190 + Math.sin(x * 0.006 - t * 2) * 50 - 90 * Math.sin(p * Math.PI);
      spine.push([x, y]);
    }
    return spine;
  }

  function stationStorm(ctx, t) {
    const frozen = t >= BEAT.snap && t < BEAT.silence[1];
    const tt = frozen ? BEAT.snap : t;
    const storm = progress(t, BEAT.toStorm[0], BEAT.dragon[0] + 0.6);
    for (let i = 0; i < 9; i++) {
      const r = seededRandom(900 + i);
      L.ruyiCloud(ctx, -1600 + i * 420 + Math.sin(tt * 0.4 + i) * 60, 140 + r() * 260, 1.2 + r() * 0.8, { fill: `rgba(70, 64, 60, ${0.85 * storm})`, line: `rgba(20, 16, 14, ${storm})`, alpha: storm, seed: i, curls: 3 + (i % 2) });
    }
    const flash = BEAT.lightning.reduce((max, at) => Math.max(max, t >= at ? Math.exp(-(t - at) * 9) : 0), 0);
    if (flash > 0.02 && !frozen) {
      ctx.save();
      ctx.globalCompositeOperation = 'screen';
      ctx.fillStyle = `rgba(255, 230, 170, ${0.35 * flash})`;
      ctx.fillRect(-2400, 0, 4800, L.HEIGHT);
      ctx.restore();
      const which = BEAT.lightning.findIndex((at, i) => t >= at && (i === BEAT.lightning.length - 1 || t < BEAT.lightning[i + 1]));
      L.lightningBolt(ctx, [300 - which * 500, 60], [100 - which * 380, HORIZON], 40 + which, flash);
    }
    if (t < BEAT.toStorm[0] - 0.5) return;
    const ship = L.galley(ctx, -260, HORIZON + 40 + Math.sin(tt * 2.4) * 18, 0.82, tt, { sail: false, tilt: Math.sin(tt * 2) * 0.12, seed: 9 });
    const home = [2600, 380];
    if (t < BEAT.snap) {
      const tension = Math.sin(t * 40) * 6 * progress(t, BEAT.dragon[0], BEAT.snap);
      L.redThread(ctx, quadraticPoints(ship.stern, home, -60 + tension, 60));
    } else {
      const dt = t - BEAT.snap;
      const breakPoint = L.pointOnPath(quadraticPoints(ship.stern, home, -60, 60), 0.42).point;
      const recoil = 1 - Math.exp(-dt * 6) * Math.cos(dt * 14);
      const fall = Math.min(1, dt * 0.55);
      const leftEnd = [lerp(breakPoint[0], ship.stern[0], 0.4 * recoil), breakPoint[1] + 360 * fall * fall + 40 * recoil];
      const rightEnd = [lerp(breakPoint[0], home[0], 0.3 * recoil), breakPoint[1] + 400 * fall * fall + 40 * recoil];
      L.redThread(ctx, quadraticPoints(ship.stern, leftEnd, 60 + 160 * fall));
      L.redThread(ctx, quadraticPoints(rightEnd, home, 60 + 200 * fall));
      if (dt < 0.35) {
        drawGlow(ctx, breakPoint[0], breakPoint[1], 200 * (1 - dt / 0.35), RGB.gold, 1 - dt / 0.35);
        ctx.strokeStyle = `rgba(255, 220, 150, ${1 - dt / 0.35})`;
        ctx.lineWidth = 3;
        for (let i = 0; i < 12; i++) {
          const angle = (i / 12) * Math.PI * 2;
          strokeLine(ctx, breakPoint[0], breakPoint[1], breakPoint[0] + Math.cos(angle) * (30 + dt * 600), breakPoint[1] + Math.sin(angle) * (30 + dt * 600));
        }
      }
    }
    if (t > BEAT.dragon[0] && t < BEAT.dragon[1] + 1.6) {
      const presence = progress(t, BEAT.dragon[0], BEAT.dragon[0] + 0.6) * (1 - progress(t, BEAT.dragon[1] + 0.8, BEAT.dragon[1] + 1.6));
      L.stormDragon(ctx, dragonSpine(frozen ? BEAT.snap : t), tt, { width: 118, alpha: presence, eyeGlow: presence });
    }
    if (!frozen) L.rain(ctx, -1400, 1400, 0, L.HEIGHT, t, 0.35 * storm);
  }

  function stationNight(ctx, t) {
    L.moon(ctx, -1000, 220, 90, 1);
    paintGround(ctx, -1300, 1300, GROUND, 141);
    L.pavilion(ctx, 520, GROUND - 6, 1.0, 0.7, 7);
    const loom = L.loom(ctx, 900, GROUND, 0.9, t, t > BEAT.reweave[0] && t < BEAT.reweave[1] ? 1 : 0);
    L.olivePainting(ctx, 40, GROUND, 0.95, t, 5);
    L.writingDesk(ctx, 0, 690, GROUND, 360);
    const lit = [0, 1, 2, 3].map((i) => t >= BEAT.lightsOn + i * 0.14);
    const laptop = L.inkLaptop(ctx, 0, 684, 0.52, { t, glow: 0.5, running: lit });
    const reach = easeInOut(progress(t, BEAT.reach[0], BEAT.reach[1]));
    const pulledBack = easeInOut(progress(t, BEAT.grab + 0.3, BEAT.grab + 1.4));
    const prince = L.drawPuppet(ctx, {
      x: -330, y: GROUND, costume: 'prince', facing: 1, t, seed: 5,
      pose: { head: 0.1, lean: 0.12 * reach * (1 - pulledBack), reachNear: [lerp(100, 250, reach * (1 - pulledBack * 0.8)), lerp(-280, -205, reach * (1 - pulledBack))], reachFar: [40, -250] },
    });
    const penelopeIn = easeOut(progress(t, BEAT.grab - 0.9, BEAT.grab));
    L.drawPuppet(ctx, {
      x: lerp(-900, -560, penelopeIn), y: GROUND, costume: 'queen', facing: 1, t, seed: 3,
      pose: { head: 0, lean: 0.1, reachNear: penelopeIn > 0.9 ? [prince.handNear[0] - lerp(-900, -560, penelopeIn) - 20, prince.handNear[1] - GROUND + 10] : [60, -260], reachFar: [40, -300] },
    });
    const gapFrom = 0.35;
    const gapTo = 0.62;
    const path = catmullRom([[-1600, 700], [-1000, 640], [-560, 720], [-200, 700], laptop.orb], 16);
    const heal = progress(t, BEAT.reweave[0], BEAT.reweave[1]);
    L.redThread(ctx, L.slicePath(path, gapFrom), { alpha: 0.9 });
    const tail = path.slice(Math.floor(path.length * gapTo));
    L.redThread(ctx, tail, { alpha: 0.9 });
    if (heal > 0) {
      const gapPath = path.slice(Math.floor(path.length * gapFrom), Math.ceil(path.length * gapTo) + 1);
      L.redThread(ctx, gapPath, { reveal: heal, width: 4.2 });
      const tip = L.pointOnPath(gapPath, heal).point;
      if (heal < 1) {
        drawGlow(ctx, tip[0], tip[1], 70, RGB.gold, 0.9);
        const shuttle = [lerp(loom.clothBottom[0], tip[0], 0.5 + 0.5 * Math.sin(t * 12)), lerp(loom.clothBottom[1], tip[1], 0.5 + 0.5 * Math.sin(t * 12))];
        ctx.fillStyle = 'rgba(214, 170, 90, 0.95)';
        fillEllipse(ctx, shuttle[0], shuttle[1], 18, 5, 0.3);
      }
    } else {
      const flicker = 0.5 + 0.5 * Math.sin(t * 7);
      const from = L.pointOnPath(path, gapFrom).point;
      const to = L.pointOnPath(path, gapTo).point;
      ctx.save();
      ctx.setLineDash([6, 10]);
      ctx.strokeStyle = `rgba(201, 40, 31, ${0.25 * flicker})`;
      ctx.lineWidth = 2;
      L.strokePolyline(ctx, [from, to]);
      ctx.restore();
    }
    const wait = progress(t, BEAT.waitCharacter, BEAT.waitCharacter + 0.9);
    if (wait > 0) {
      L.brushCharacter(ctx, '等', -560, 330, 360, wait, { font: L.FONT.running, color: 'rgba(236, 228, 208, 0.95)', alpha: 1 - progress(t, BEAT.reweave[0] + 0.4, BEAT.reweave[1]) * 0.7 });
    }
    if (t >= BEAT.lightsOn) uiTag(ctx, 60, 440, 'recovered · nothing lost', true, progress(t, BEAT.lightsOn, BEAT.lightsOn + 0.5));
  }

  function stationBow(ctx, t) {
    paintGround(ctx, -5200, 1200, GROUND, 151);
    const local = (worldX) => worldX - STATION.bow;
    for (let i = 0; i < 10; i++) {
      const x = 700 - i * 560;
      const column = ctx.createLinearGradient(x - 24, 0, x + 24, 0);
      column.addColorStop(0, 'rgba(40, 32, 28, 0.08)');
      column.addColorStop(0.5, 'rgba(40, 32, 28, 0.26)');
      column.addColorStop(1, 'rgba(40, 32, 28, 0.1)');
      ctx.fillStyle = column;
      ctx.fillRect(x - 24, 150, 48, GROUND - 150);
      ctx.fillStyle = 'rgba(40, 32, 28, 0.3)';
      ctx.fillRect(x - 40, 130, 80, 22);
      ctx.fillRect(x - 34, GROUND - 26, 68, 26);
    }
    const drawAmount = easeInOut(progress(t, BEAT.draw[0], BEAT.draw[1]));
    const released = t >= BEAT.release;
    const beggarX = 420;
    const nockX = released ? beggarX - 150 : beggarX - 150 + drawAmount * 90;
    const beggar = L.drawPuppet(ctx, {
      x: beggarX, y: GROUND, costume: 'beggar', facing: -1, t, seed: 2,
      pose: { head: -0.05, lean: 0.05, legNear: [0.3, -0.1], legFar: [-0.28, -0.04], reachFar: [250, RING.y - GROUND], reachNear: [beggarX - nockX, RING.y - GROUND + 6] },
    });
    const grip = beggar.handFar;
    const bowShape = L.bow(ctx, grip, released ? 0 : drawAmount, t);
    const vibration = released ? Math.sin((t - BEAT.release) * 60) * 16 * Math.exp(-(t - BEAT.release) * 4) : 0;
    const stringMid = released ? [bowShape.top[0] + 44 + vibration, RING.y] : beggar.handNear;
    L.redThread(ctx, [bowShape.top, stringMid, bowShape.bottom], { width: 2.6 });
    for (let i = 0; i < RING.count; i++) {
      const lit = progress(t, ringPass(i), ringPass(i) + 0.4);
      const x = local(ringX(i));
      const bob = Math.sin(t * 1.1 + i * 0.7) * 10;
      L.jadeDisc(ctx, x, RING.y + bob, 62, lit, t);
      if (lit > 0) {
        const pop = easeOutBack(lit);
        drawGlow(ctx, x, RING.y, 260 * pop * (1 - lit * 0.4), RGB.jade, 1 - lit * 0.5);
        L.brushCharacter(ctx, ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十', '十一', '十二'][i], x, RING.y - 140, 70, lit, { font: L.FONT.running, alpha: 0.9 });
      }
    }
    const tip = released ? local(arrowTip(t)) : nockX - 170;
    if (released) L.redThread(ctx, [stringMid, [tip + 190, RING.y]], { width: 2.8 });
    L.arrow(ctx, tip, RING.y, Math.PI);
    const done = progress(t, BEAT.doneSeal, BEAT.doneSeal + 0.7);
    if (done > 0) L.seal(ctx, local(ringX(RING.count - 1)) - 40, 690, 170, '成', done, { style: 'yin' });
  }

  function stationContinue(ctx, t) {
    L.sunDisc(ctx, -1100, HORIZON - 20, 90, 0.9);
    L.galley(ctx, -960, HORIZON + 8, 0.3, t, { sail: true, oars: false, seed: 10 });
    ctx.fillStyle = 'rgba(26, 21, 18, 0.85)';
    ctx.beginPath();
    ctx.moveTo(300, HORIZON + 20);
    ctx.quadraticCurveTo(440, HORIZON - 160, 660, HORIZON - 170);
    ctx.quadraticCurveTo(880, HORIZON - 150, 1080, HORIZON + 20);
    ctx.closePath();
    ctx.fill();
    const arrived = t >= BEAT.arrival;
    const glow = arrived ? 1 - Math.exp(-(t - BEAT.arrival) * 4) * 0.4 : 0.3;
    L.pavilion(ctx, 660, HORIZON - 150, 0.42, glow, 27);
    const homeWindow = [660, HORIZON - 150 - 70];
    if (arrived) {
      drawGlow(ctx, homeWindow[0], homeWindow[1], 380, RGB.jade, 0.7 * glow);
      for (let i = 0; i < 4; i++) L.jadeDisc(ctx, homeWindow[0] - 120 + i * 80, homeWindow[1] - 170, 22, easeOutBack(progress(t, BEAT.arrival + i * 0.12, BEAT.arrival + 0.4 + i * 0.12)), t);
    }
    const shipStern = [-960 + 0.3 * 318, HORIZON + 8 - 0.3 * 96];
    const path = catmullRom([shipStern, [-600, 300], [-50, 230], [420, 330], homeWindow], 16);
    L.redThread(ctx, path, { alpha: 0.85 });
    const flight = easeInOut(progress(t, BEAT.craneHome[0], BEAT.craneHome[1]));
    if (flight > 0 && flight < 1) {
      const { point, angle } = pointOnPath(path, flight);
      L.paperCrane(ctx, point[0], point[1], 0.8, angle * 0.4, t * 12, { glow: 0.7 });
      ctx.save();
      ctx.font = `italic 600 44px ${L.FONT.serif}`;
      ctx.textAlign = 'center';
      ctx.fillStyle = COLOR.paperLight;
      ctx.shadowColor = 'rgba(255, 240, 210, 0.9)';
      ctx.shadowBlur = 16;
      ctx.fillText('continue.', point[0], point[1] + 70);
      ctx.restore();
    }
    if (arrived) {
      ctx.save();
      ctx.font = `italic 600 64px ${L.FONT.serif}`;
      ctx.textAlign = 'center';
      ctx.globalAlpha = progress(t, BEAT.arrival, BEAT.arrival + 0.6);
      ctx.fillStyle = COLOR.paperLight;
      ctx.shadowColor = 'rgba(95, 211, 154, 0.8)';
      ctx.shadowBlur = 24;
      ctx.fillText('continue.', 120, 200);
      ctx.restore();
    }
  }

  function stationLeave(ctx, t) {
    L.sunDisc(ctx, -860, HORIZON - 90, 70, 0.8);
    paintGround(ctx, -700, 1400, GROUND, 171);
    L.olivePainting(ctx, 420, GROUND, 0.95, t, 5);
    L.writingDesk(ctx, 390, 690, GROUND, 360);
    L.inkLaptop(ctx, 390, 684, 0.52, { t, glow: 1 });
    const standing = t > BEAT.rise;
    const walk = progress(t, BEAT.walk[0], BEAT.walk[1]);
    L.drawPuppet(ctx, {
      x: standing ? lerp(120, -620, walk) : 120, y: GROUND, costume: 'prince', facing: -1, t, seed: 5,
      pose: !standing ? { hip: [0, -110], lean: -0.15, footNear: [100, -4], footFar: [80, 0], reachNear: [180, -250], reachFar: [160, -240] } : walk > 0 && walk < 1 ? { walkPhase: t * 7 } : { head: -0.1 },
    });
    L.verticalCalligraphy(ctx, '循環不息', -1500, 240, 110, t, BEAT.pullBack[0] + 0.6, 0.35, { font: L.FONT.running });
    L.seal(ctx, -1720, 620, 136, 'LOOP', progress(t, BEAT.pullBack[0] + 2.2, BEAT.pullBack[0] + 2.9), { font: L.FONT.serif, weight: 700, style: 'yin', columns: 1, textSize: 44 });
  }

  const STATIONS = [
    { id: 'title', x: STATION.title, from: -1600, to: 1200, draw: stationTitle },
    { id: 'ithaca', x: STATION.ithaca, from: -1400, to: 1600, draw: stationIthaca },
    { id: 'sea', x: STATION.sea, from: -1700, to: 3400, draw: stationSea },
    { id: 'lotus', x: STATION.lotus, from: -1400, to: 1400, draw: stationLotus },
    { id: 'cave', x: STATION.cave, from: -1200, to: 2200, draw: stationCave },
    { id: 'circe', x: STATION.circe, from: -1200, to: 1200, draw: stationCirce },
    { id: 'sirens', x: STATION.sirens, from: -1600, to: 1600, draw: stationSirens },
    { id: 'proteus', x: STATION.proteus, from: -1300, to: 1300, draw: stationProteus },
    { id: 'storm', x: STATION.storm, from: -2600, to: 2900, draw: stationStorm },
    { id: 'night', x: STATION.night, from: -1600, to: 1400, draw: stationNight },
    { id: 'bow', x: STATION.bow, from: -5400, to: 1300, draw: stationBow },
    { id: 'continue', x: STATION.continue, from: -1600, to: 1600, draw: stationContinue },
    { id: 'leave', x: STATION.leave, from: -2200, to: 1500, draw: stationLeave },
  ];

  Object.assign(L, {
    BEAT, STATION, STATIONS, SEA_RANGES, HORIZON, GROUND, SCROLL_RIGHT_EDGE, SCROLL_LEFT_EDGE, RING, ringPass,
    cameraAt, darknessAt, blanknessAt, inSea, NARRATION,
  });
  void easeIn; void pulseAt;
})();
