/* The Butlers Who Would Not Proceed — a deadpan 75 s Looper film.
 * Deterministic: window.seek(t) sets every element from t alone (motion-video-kit rule 4). */
'use strict';

const QUERY = new URLSearchParams(location.search);
const W = Number(QUERY.get('w')) || 1920;
const H = Number(QUERY.get('h')) || 1080;
const PORTRAIT = H > W;
const DURATION = 75;
/** Short side, used to scale type so both aspect ratios read the same on a phone. */
const UNIT = Math.min(W, H) / 1080;

const C = {
  wall: '#EBA9A6', wallStripe: '#E49A98', wainscot: '#8C2E3B', wainscotPanel: '#A03C49',
  cornice: '#F4E3C8', ceiling: '#EED6BD', sideWall: '#D98F8D', sideWainscot: '#76262F',
  floorA: '#F3E3C3', floorB: '#C9A06B', gold: '#C9A23A', goldDark: '#8E6F1E', wood: '#6B3A2A', woodLight: '#85503A',
  coat: '#4B2A60', coatDark: '#361D47', trouser: '#231C26', waist: '#D2A544', shirt: '#FBF6EA', glove: '#FFFFFF',
  skin: '#F1CFB2', skinShade: '#DDB08E', devSkin: '#E7E0C8', hoodie: '#8B9698', hoodieDark: '#6F7A7C', jeans: '#4F6B8E',
  tan: '#C27F4A', tanShade: '#A8683A', shirtCoral: '#EE7E60', shorts: '#E7D5A8',
  ink: '#2B3F5C', paper: '#F6EFDD', stampRed: '#C8402F', stampBlue: '#2F5DA8', green: '#3E8E5A', burgundy: '#6E2230',
};

// ---------------------------------------------------------------- timing helpers
const clamp = (v, lo = 0, hi = 1) => Math.min(hi, Math.max(lo, v));
const lerp = (a, b, p) => a + (b - a) * p;
const progress = (t, start, end) => clamp((t - start) / (end - start));
const EASE = {
  linear: p => p,
  inout: p => p * p * p * (p * (p * 6 - 15) + 10), // smootherstep: no velocity pop at either end
  out: p => 1 - Math.pow(1 - p, 3),
  in: p => p * p * p,
  snap: p => 1 - Math.pow(1 - p, 5),
  back: p => { const s = 1.6; const q = p - 1; return 1 + q * q * ((s + 1) * q + s); },
};
const window01 = (t, start, end, fadeIn = 0.25, fadeOut = 0.25) =>
  Math.min(progress(t, start, start + fadeIn), 1 - progress(t, end - fadeOut, end));

/** Interpolate keyframes [[time, ...values, easeName?]]. Two keys at the same time make a hard cut. */
function keyed(t, keys) {
  if (t <= keys[0][0]) return valuesOf(keys[0]);
  for (let i = 1; i < keys.length; i++) {
    const next = keys[i];
    if (t < next[0]) {
      const prev = keys[i - 1];
      const easeName = typeof next[next.length - 1] === 'string' ? next[next.length - 1] : 'inout';
      const p = EASE[easeName](progress(t, prev[0], next[0]));
      const a = valuesOf(prev), b = valuesOf(next);
      return a.map((v, j) => lerp(v, b[j], p));
    }
  }
  return valuesOf(keys[keys.length - 1]);
}
function valuesOf(key) { return key.slice(1).filter(v => typeof v === 'number'); }

/** Deterministic pseudo-random in [0,1) for static decoration. */
function seeded(n) { const x = Math.sin(n * 127.1 + 311.7) * 43758.5453; return x - Math.floor(x); }

// ---------------------------------------------------------------- script (voice cue times, seconds)
const VOICE_CUES = [
  ['n_chair', 5.9], ['n_butlers', 12.2], ['n_codex', 15.9], ['n_claude', 17.3], ['n_cursor', 18.7],
  ['n_stopped', 20.1], ['n_question', 26.5], ['n_beach', 32.3], ['f_still', 36.4],
  ['f_looper', 40.4], ['f_done', 43.2], ['f_back', 45.9], ['f_phone', 48.8],
  ['f_spaces', 56.2], ['f_no', 57.5], ['n_free', 60.9], ['n_day', 64.6],
];
const SUBTITLES = [
  [5.9, 9.2, 'The developer had not left the chair in three days.'],
  [12.2, 15.7, 'Behind him stood three of the finest butlers available.'],
  [20.1, 22.4, 'They had done a great deal of work. Then they stopped.'],
  [22.4, 26.0, 'They would not proceed without permission.'],
  [26.5, 28.4, 'One of them had a question.'],
  [32.3, 35.8, 'On the fourth day, a friend returned from the beach.'],
  [36.4, 37.5, 'You’re still here?'],
  [38.0, 39.7, 'They won’t proceed without permission.', 'silent'],
  [40.4, 42.7, 'Get Looper. It’s an app on your Mac.'],
  [43.2, 45.2, 'It checks whether they’re really done.'],
  [45.9, 48.5, 'Tests fail? It sends them back to work.'],
  [48.8, 51.6, 'Real questions come to your phone. Anywhere.'],
  [56.2, 57.2, 'Spaces.'], [57.5, 58.8, 'Absolutely not.'],
  [60.9, 62.4, 'The developer stood up.'],
  [64.6, 67.9, 'It was, by all accounts, a lovely day.'],
];
const CHAPTERS = [
  [4.8, 'CHAPTER ONE', 'The Chair'], [31.8, 'CHAPTER TWO', 'The Coconut'],
  [60.5, 'CHAPTER FOUR', 'The Door'],
];
const CHAPTER_HOLD = 1.9;

const CARDS = {
  codex: ['ALL DONE,', 'SIR.'],
  claude: ['SIR,', 'TABS OR', 'SPACES?'],
  cursor: ['SHALL I DROP', 'THE PRODUCTION', 'DATABASE?'],
};
const BUTLERS = [
  { id: 'codex', name: 'CODEX', x: 780, tie: '#111111', hair: 'part' },
  { id: 'claude', name: 'CLAUDE CODE', x: 1200, tie: '#D97757', hair: 'walrus' },
  { id: 'cursor', name: 'CURSOR', x: 1620, tie: '#5A5F66', hair: 'chops' },
];

// ---------------------------------------------------------------- room geometry (one-point perspective)
const VANISH_Y = 900;
const FLOOR_SPAN = 600;          // floor y = VANISH_Y + FLOOR_SPAN * depthScale
const BUTLER_DEPTH = 1.2333;     // depth scale where characters are drawn at 1:1
const BACK_WALL = { left: 500, right: 1900, top: 260, bottom: 1500 };
const depthAt = feetY => (feetY - VANISH_Y) / FLOOR_SPAN;
const scaleAt = feetY => depthAt(feetY) / BUTLER_DEPTH;
const BUTLER_FEET_Y = 1640;
const DESK = { left: 950, right: 1450, top: 1880, front: 2090 };
const DEV_HOME = { x: 1200, feetY: 2060 };
const FRIEND_MARK = { x: 1640, feetY: 1990 };
const DOOR = { nearDepth: 1.43, farDepth: 1.13, heightRatio: 0.56 };

function rightWallPoint(depth, heightRatio) {
  const floorY = VANISH_Y + FLOOR_SPAN * depth;
  const ceilingY = VANISH_Y - 640 * depth;
  return [1200 + 700 * depth, lerp(floorY, ceilingY, heightRatio)];
}
const pts = list => list.map(p => p.join(',')).join(' ');

function roomBackdropSVG() {
  const parts = [];
  // ceiling, side walls, back wall
  parts.push(`<polygon points="-400,-300 2800,-300 1900,260 500,260" fill="${C.ceiling}"/>`);
  for (let i = 1; i < 6; i++) { const y = 260 - i * 95; const k = (900 - y) / 640; parts.push(`<line x1="${1200 - 700 * k}" y1="${y}" x2="${1200 + 700 * k}" y2="${y}" stroke="#E2C6A8" stroke-width="5"/>`); }
  parts.push(`<polygon points="-400,-450 500,260 500,1500 -400,2700" fill="${C.sideWall}"/>`);
  parts.push(`<polygon points="2800,-450 1900,260 1900,1500 2800,2700" fill="${C.sideWall}"/>`);
  // side wainscots
  const wainscotTop = 1180 / 1500;
  parts.push(`<polygon points="-400,${lerp(-450, 2700, 0.745)} 500,1180 500,1500 -400,2700" fill="${C.sideWainscot}"/>`);
  parts.push(`<polygon points="2800,${lerp(-450, 2700, 0.745)} 1900,1180 1900,1500 2800,2700" fill="${C.sideWainscot}"/>`);
  void wainscotTop;
  parts.push(`<rect x="500" y="260" width="1400" height="1240" fill="${C.wall}"/>`);
  for (let x = 535; x < 1900; x += 70) parts.push(`<rect x="${x}" y="300" width="14" height="880" fill="${C.wallStripe}"/>`);
  parts.push(`<rect x="500" y="260" width="1400" height="46" fill="${C.cornice}"/><rect x="500" y="306" width="1400" height="10" fill="#DCC3A0"/>`);
  parts.push(`<rect x="500" y="1180" width="1400" height="320" fill="${C.wainscot}"/><rect x="500" y="1172" width="1400" height="14" fill="${C.cornice}"/>`);
  for (let i = 0; i < 7; i++) parts.push(`<rect x="${530 + i * 196}" y="1225" width="170" height="235" fill="none" stroke="${C.wainscotPanel}" stroke-width="8"/>`);
  parts.push('<g transform="translate(0,140)">');
  // paintings: a rubber duck and a single semicolon, in gold frames
  parts.push(framedPainting(620, 420, 260, 330, duckPainting()));
  parts.push(framedPainting(1520, 420, 260, 330, `<rect width="260" height="330" fill="#E7EFE6"/><text x="130" y="232" font-family="Didot, serif" font-size="230" text-anchor="middle" fill="#2B3F5C">;</text>`));
  // wall clock with pendulum case
  parts.push(`<rect x="1150" y="560" width="100" height="330" rx="12" fill="${C.wood}" stroke="${C.goldDark}" stroke-width="6"/><rect x="1168" y="600" width="64" height="270" rx="8" fill="#3E2119"/>`);
  parts.push(`<g id="pendulum"><line x1="1200" y1="600" x2="1200" y2="820" stroke="${C.gold}" stroke-width="6"/><circle cx="1200" cy="830" r="22" fill="${C.gold}" stroke="${C.goldDark}" stroke-width="4"/></g>`);
  parts.push(`<circle cx="1200" cy="470" r="128" fill="${C.gold}"/><circle cx="1200" cy="470" r="112" fill="#FBF3E1" stroke="${C.goldDark}" stroke-width="4"/>`);
  for (let i = 0; i < 12; i++) { const a = i / 12 * Math.PI * 2; parts.push(`<line x1="${1200 + Math.sin(a) * 92}" y1="${470 - Math.cos(a) * 92}" x2="${1200 + Math.sin(a) * 104}" y2="${470 - Math.cos(a) * 104}" stroke="${C.ink}" stroke-width="${i % 3 ? 4 : 8}"/>`); }
  parts.push(`<line id="clock-hour" x1="1200" y1="470" x2="1200" y2="410" stroke="${C.ink}" stroke-width="10" stroke-linecap="round"/>`);
  parts.push(`<line id="clock-minute" x1="1200" y1="470" x2="1200" y2="382" stroke="${C.ink}" stroke-width="6" stroke-linecap="round"/><circle cx="1200" cy="470" r="9" fill="${C.ink}"/>`);
  parts.push('</g>');
  // floor: checker tiles in perspective
  parts.push(`<polygon points="500,1500 1900,1500 2800,2700 -400,2700" fill="${C.floorA}"/>`);
  const rows = [1, 1.2, 1.44, 1.73, 2.07, 2.49, 2.99];
  for (let r = 0; r < rows.length - 1; r++) {
    for (let c = -14; c < 14; c++) {
      if ((r + c) % 2 === 0) continue;
      const u0 = c * 140, u1 = (c + 1) * 140, s0 = rows[r], s1 = rows[r + 1];
      parts.push(`<polygon points="${pts([[1200 + s0 * u0, VANISH_Y + FLOOR_SPAN * s0], [1200 + s0 * u1, VANISH_Y + FLOOR_SPAN * s0], [1200 + s1 * u1, VANISH_Y + FLOOR_SPAN * s1], [1200 + s1 * u0, VANISH_Y + FLOOR_SPAN * s1]])}" fill="${C.floorB}"/>`);
    }
  }
  // oval rug under the desk
  parts.push(`<ellipse cx="1200" cy="2050" rx="560" ry="150" fill="#7C2935"/><ellipse cx="1200" cy="2050" rx="520" ry="126" fill="none" stroke="${C.gold}" stroke-width="8"/>`);
  // door on the right wall, the sunlight behind it, and the light wedge on the floor
  const doorFrame = [rightWallPoint(DOOR.farDepth, 0), rightWallPoint(DOOR.nearDepth, 0), rightWallPoint(DOOR.nearDepth, DOOR.heightRatio), rightWallPoint(DOOR.farDepth, DOOR.heightRatio)];
  parts.push(`<polygon points="${pts(doorFrame)}" fill="#FFF5CF" stroke="${C.cornice}" stroke-width="22"/>`);
  parts.push(`<polygon id="door-sky" points="${pts(doorFrame)}" fill="#9FD6E3"/>`);
  parts.push(`<polygon id="door-light" points="" fill="#FFF3C0" opacity="0"/>`);
  parts.push(`<polygon id="door-leaf" points="${pts(doorFrame)}" fill="#2F6E6B" stroke="#24524F" stroke-width="6"/>`);
  return parts.join('');
}

function framedPainting(x, y, w, h, inner) {
  return `<g transform="translate(${x},${y})"><rect x="-22" y="-22" width="${w + 44}" height="${h + 44}" fill="${C.gold}" stroke="${C.goldDark}" stroke-width="6"/>
    <svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${inner}</svg></g>`;
}
function duckPainting() {
  return `<rect width="260" height="330" fill="#DDEBF0"/><ellipse cx="130" cy="265" rx="110" ry="22" fill="#9CC3D1"/>
    <ellipse cx="135" cy="215" rx="82" ry="55" fill="#F4C542"/><circle cx="92" cy="140" r="46" fill="#F4C542"/>
    <path d="M48,140 L18,150 L48,160 Z" fill="#E9803A"/><circle cx="84" cy="128" r="7" fill="#222"/>`;
}

// ---------------------------------------------------------------- characters (person units: feet at 0,0, ~620 tall)
function cardSVG(lines, id) {
  const lineHeight = 15;
  const top = -34 - (lines.length - 1) * lineHeight / 2 + 4;
  const text = lines.map((l, i) => `<tspan x="0" y="${top + i * lineHeight}">${l}</tspan>`).join('');
  return `<g id="${id || ''}"><polygon points="-58,0 58,0 52,-72 -52,-72" fill="#EDE6D6"/><rect x="-56" y="-74" width="112" height="70" fill="#FFFDF6" stroke="#B9AE98" stroke-width="2"/>
    <text font-family="'Courier New', monospace" font-weight="700" font-size="12.5" fill="#2A2320" text-anchor="middle">${text}</text></g>`;
}

function butlerHead(variant) {
  const face = `<rect x="-17" y="-506" width="34" height="30" fill="${C.skinShade}"/>
    <ellipse cx="-48" cy="-542" rx="10" ry="16" fill="${C.skin}"/><ellipse cx="48" cy="-542" rx="10" ry="16" fill="${C.skin}"/>
    <ellipse cx="0" cy="-545" rx="47" ry="58" fill="${C.skin}"/>`;
  const eyes = `<g class="eyes"><ellipse cx="-17" cy="-548" rx="4.5" ry="4.5" fill="#1d1a1a"/><ellipse cx="17" cy="-548" rx="4.5" ry="4.5" fill="#1d1a1a"/></g>
    <line x1="-27" y1="-558" x2="-8" y2="-558" stroke="#3a2a22" stroke-width="4"/><line x1="8" y1="-558" x2="27" y2="-558" stroke="#3a2a22" stroke-width="4"/>
    <path d="M0,-545 L-6,-527 L4,-527" fill="none" stroke="${C.skinShade}" stroke-width="3"/>`;
  const hair = {
    part: `<path d="M-48,-560 Q-50,-612 0,-608 Q46,-606 48,-566 L40,-582 Q10,-596 -14,-590 L-22,-600 Q-40,-590 -48,-560 Z" fill="#8E8E90"/><line x1="-14" y1="-592" x2="-20" y2="-604" stroke="#D8D2CC" stroke-width="3"/>
       <line x1="-10" y1="-512" x2="10" y2="-512" stroke="#8a4f3f" stroke-width="3"/>`,
    walrus: `<path d="M-49,-548 Q-52,-582 -38,-590 L-40,-560 Z M49,-548 Q52,-582 38,-590 L40,-560 Z" fill="#CFCACA"/>
       <path d="M-34,-522 Q-18,-532 0,-524 Q18,-532 34,-522 Q30,-500 14,-508 Q0,-500 -14,-508 Q-30,-500 -34,-522 Z" fill="#D8D4D2"/>`,
    chops: `<path d="M-48,-566 Q-44,-614 0,-612 Q44,-614 48,-566 Q30,-592 0,-590 Q-30,-592 -48,-566 Z" fill="#B8612E"/>
       <path d="M-47,-560 L-44,-506 Q-34,-496 -26,-516 L-36,-560 Z M47,-560 L44,-506 Q34,-496 26,-516 L36,-560 Z" fill="#B8612E"/>
       <line x1="-10" y1="-513" x2="10" y2="-513" stroke="#8a4f3f" stroke-width="3"/>`,
  }[variant];
  return face + eyes + hair;
}

function butlerSVG(butler) {
  const id = butler.id;
  const plateWidth = butler.name.length > 8 ? 176 : 128;
  const plateFont = butler.name.length > 8 ? 17 : 20;
  return `<g id="${id}">
    <ellipse cx="0" cy="0" rx="78" ry="14" fill="rgba(60,20,30,0.22)"/>
    <g id="${id}-legL"><rect x="-40" y="-262" width="33" height="256" fill="${C.trouser}"/><ellipse cx="-27" cy="-6" rx="31" ry="10" fill="#0e0c0e"/></g>
    <g id="${id}-legR"><rect x="7" y="-262" width="33" height="256" fill="${C.trouser}"/><ellipse cx="27" cy="-6" rx="31" ry="10" fill="#0e0c0e"/></g>
    <g id="${id}-upper">
      <path d="M-78,-486 L78,-486 L88,-250 L66,-170 L26,-246 L-26,-246 L-66,-170 L-88,-250 Z" fill="${C.coat}"/>
      <path d="M-30,-486 L-42,-300 L0,-280 L42,-300 L30,-486 Z" fill="${C.waist}"/>
      <circle cx="0" cy="-360" r="4" fill="${C.goldDark}"/><circle cx="0" cy="-330" r="4" fill="${C.goldDark}"/>
      <path d="M-30,-486 L30,-486 L0,-392 Z" fill="${C.shirt}"/>
      <path d="M-30,-486 L-58,-486 L-24,-360 Z M30,-486 L58,-486 L24,-360 Z" fill="${C.coatDark}"/>
      <path d="M0,-480 L-22,-492 L-22,-468 Z M0,-480 L22,-492 L22,-468 Z" fill="${butler.tie}"/><circle cx="0" cy="-480" r="5" fill="${butler.tie}"/>
      <g transform="translate(0,-440)"><rect x="${-plateWidth / 2}" y="-15" width="${plateWidth}" height="30" rx="3" fill="${C.gold}" stroke="${C.goldDark}" stroke-width="2.5"/>
        <text y="7" font-family="Futura, sans-serif" font-weight="700" font-size="${plateFont}" letter-spacing="1.5" text-anchor="middle" fill="#3B2A0C">${butler.name}</text></g>
      <g id="${id}-head">${butlerHead(butler.hair)}</g>
      <path d="M-80,-478 Q-104,-400 -62,-338" fill="none" stroke="${C.coat}" stroke-width="30" stroke-linecap="round"/>
      <path d="M80,-478 Q104,-400 62,-338" fill="none" stroke="${C.coat}" stroke-width="30" stroke-linecap="round"/>
      <g id="${id}-tray">
        <ellipse cx="0" cy="-334" rx="98" ry="15" fill="#A9A9AD"/><ellipse cx="0" cy="-338" rx="98" ry="15" fill="#DCDCE0" stroke="#9A9AA0" stroke-width="2"/>
        <g transform="translate(0,-340)">${cardSVG(CARDS[butler.cardKey || id], id + '-card')}</g>
      </g>
      <circle cx="-62" cy="-336" r="17" fill="${C.glove}" stroke="#ddd" stroke-width="2"/><circle cx="62" cy="-336" r="17" fill="${C.glove}" stroke="#ddd" stroke-width="2"/>
    </g>
  </g>`;
}

function developerSVG() {
  return `<g id="dev">
    <ellipse cx="0" cy="0" rx="80" ry="13" fill="rgba(60,20,30,0.2)"/>
    <g id="dev-legL"><rect x="-44" y="-252" width="38" height="236" rx="10" fill="${C.jeans}"/><rect x="-46" y="-26" width="44" height="26" rx="10" fill="#F2A7B8"/></g>
    <g id="dev-legR"><rect x="6" y="-252" width="38" height="236" rx="10" fill="${C.jeans}"/><rect x="2" y="-26" width="44" height="26" rx="10" fill="#F4D35E"/></g>
    <path d="M-86,-430 Q-92,-300 -70,-236 L70,-236 Q92,-300 86,-430 Q60,-458 0,-460 Q-60,-458 -86,-430 Z" fill="${C.hoodie}"/>
    <path d="M-40,-300 L40,-300 L34,-262 L-34,-262 Z" fill="${C.hoodieDark}"/>
    <line x1="-14" y1="-452" x2="-18" y2="-370" stroke="#EEE" stroke-width="4"/><line x1="14" y1="-452" x2="18" y2="-370" stroke="#EEE" stroke-width="4"/>
    <g id="dev-armL"><path d="M-80,-420 Q-110,-340 -80,-300" fill="none" stroke="${C.hoodie}" stroke-width="30" stroke-linecap="round"/><circle cx="-80" cy="-298" r="15" fill="${C.devSkin}"/></g>
    <g id="dev-armR"><path d="M80,-420 Q110,-340 80,-300" fill="none" stroke="${C.hoodie}" stroke-width="30" stroke-linecap="round"/><circle cx="80" cy="-298" r="15" fill="${C.devSkin}"/></g>
    <g id="dev-head">
      <path d="M-58,-470 Q-62,-420 -40,-440 L40,-440 Q62,-420 58,-470 Q40,-450 0,-452 Q-40,-450 -58,-470 Z" fill="${C.hoodieDark}"/>
      <ellipse cx="0" cy="-500" rx="50" ry="58" fill="${C.devSkin}"/>
      <path d="M-52,-512 L-58,-548 L-40,-538 L-38,-574 L-18,-552 L-8,-590 L6,-556 L24,-584 L28,-548 L50,-566 L46,-532 L56,-514 Q30,-556 0,-552 Q-30,-556 -52,-512 Z" fill="#4A3526"/>
      <path d="M-34,-484 Q-20,-474 -6,-484 M6,-484 Q20,-474 34,-484" fill="none" stroke="#9B84A8" stroke-width="5"/>
      <circle cx="-20" cy="-503" r="15" fill="#FFFFFF" stroke="#C9C1AA" stroke-width="2"/><circle cx="20" cy="-503" r="15" fill="#FFFFFF" stroke="#C9C1AA" stroke-width="2"/>
      <g id="dev-pupils"><circle cx="-20" cy="-503" r="3.6" fill="#141414"/><circle cx="20" cy="-503" r="3.6" fill="#141414"/></g>
      <line id="dev-mouth" x1="-12" y1="-466" x2="12" y2="-466" stroke="#7B5B4A" stroke-width="4" stroke-linecap="round"/>
      ${Array.from({ length: 26 }, (_, i) => `<circle cx="${-34 + seeded(i) * 68}" cy="${-470 + seeded(i + 40) * 22}" r="1.8" fill="#8C8270"/>`).join('')}
    </g>
  </g>`;
}

function friendSVG() {
  const flowers = Array.from({ length: 14 }, (_, i) => `<circle cx="${-60 + seeded(i + 90) * 120}" cy="${-460 + seeded(i + 120) * 200}" r="${7 + seeded(i + 7) * 5}" fill="${i % 3 ? '#FFF1DC' : '#F7CF58'}"/>`).join('');
  return `<g id="friend">
    <ellipse cx="0" cy="0" rx="74" ry="13" fill="rgba(60,20,30,0.2)"/>
    <g id="friend-legL"><rect x="-38" y="-250" width="30" height="240" rx="12" fill="${C.tan}"/><rect x="-46" y="-12" width="44" height="12" rx="5" fill="#7B4B2A"/></g>
    <g id="friend-legR"><rect x="8" y="-250" width="30" height="240" rx="12" fill="${C.tan}"/><rect x="2" y="-12" width="44" height="12" rx="5" fill="#7B4B2A"/></g>
    <path d="M-62,-300 L62,-300 L66,-176 L6,-176 L0,-200 L-6,-176 L-66,-176 Z" fill="${C.shorts}"/>
    <path d="M-80,-470 Q-86,-360 -66,-290 L66,-290 Q86,-360 80,-470 Q56,-492 0,-494 Q-56,-492 -80,-470 Z" fill="${C.shirtCoral}"/>
    <clipPath id="shirt-clip"><path d="M-80,-470 Q-86,-360 -66,-290 L66,-290 Q86,-360 80,-470 Q56,-492 0,-494 Q-56,-492 -80,-470 Z"/></clipPath>
    <g clip-path="url(#shirt-clip)">${flowers}</g>
    <path d="M-22,-492 L0,-450 L22,-492" fill="${C.tan}"/>
    <g id="friend-armL"><path d="M-76,-460 Q-104,-380 -84,-332" fill="none" stroke="${C.tan}" stroke-width="26" stroke-linecap="round"/>
      <g transform="translate(-86,-340)"><circle r="36" fill="#6E4527"/><circle r="36" fill="none" stroke="#4E2F18" stroke-width="4"/><ellipse cx="-6" cy="-10" rx="12" ry="7" fill="#8B5A34"/>
        <line x1="6" y1="-30" x2="22" y2="-78" stroke="#F0668A" stroke-width="5"/><path d="M-4,-70 Q20,-100 44,-70 Z" fill="#7FD1C0"/><circle cx="-84" cy="-340" r="0"/></g>
      <circle cx="-74" cy="-326" r="13" fill="${C.tan}"/></g>
    <g id="friend-armR"><path id="friend-armR-path" d="M76,-460 Q104,-380 90,-320" fill="none" stroke="${C.tan}" stroke-width="26" stroke-linecap="round"/>
      <g id="friend-phone"><rect x="-17" y="-34" width="34" height="66" rx="7" fill="#1E1E22"/><rect x="-13" y="-29" width="26" height="56" rx="4" fill="#9ED8E6"/><circle id="friend-phone-glow" r="0" fill="#FFF"/></g>
      <circle id="friend-hand" cx="90" cy="-320" r="13" fill="${C.tan}"/></g>
    <g id="friend-head">
      <ellipse cx="0" cy="-535" rx="48" ry="56" fill="${C.tan}"/>
      <path d="M-50,-548 Q-54,-604 0,-602 Q54,-604 50,-548 Q40,-580 0,-582 Q-40,-580 -50,-548 Z" fill="#EBCB7A"/>
      <path d="M-44,-586 L44,-586 L40,-568 L6,-568 L0,-576 L-6,-568 L-40,-568 Z" fill="#1d1d22"/>
      <ellipse cx="-17" cy="-538" rx="4.5" ry="4.5" fill="#1d1a1a"/><ellipse cx="17" cy="-538" rx="4.5" ry="4.5" fill="#1d1a1a"/>
      <path d="M-14,-506 Q0,-498 14,-506" fill="none" stroke="#6E3B22" stroke-width="4" stroke-linecap="round"/>
    </g>
  </g>`;
}

function deskSVG() {
  return `<g id="desk">
    <polygon points="${DESK.left - 20},${DESK.top} ${DESK.right + 20},${DESK.top} ${DESK.right + 40},${DESK.top + 34} ${DESK.left - 40},${DESK.top + 34}" fill="${C.woodLight}"/>
    <rect x="${DESK.left - 40}" y="${DESK.top + 34}" width="${DESK.right - DESK.left + 80}" height="${DESK.front - DESK.top - 34}" fill="${C.wood}"/>
    <rect x="${DESK.left - 10}" y="${DESK.top + 60}" width="210" height="130" fill="none" stroke="${C.woodLight}" stroke-width="6"/>
    <rect x="${DESK.right - 200}" y="${DESK.top + 60}" width="210" height="130" fill="none" stroke="${C.woodLight}" stroke-width="6"/>
    <g transform="translate(1200,${DESK.top + 56})"><rect x="-92" y="-28" width="184" height="56" rx="4" fill="${C.gold}" stroke="${C.goldDark}" stroke-width="3"/>
      <text id="hour-plaque" y="11" font-family="Futura, sans-serif" font-weight="700" font-size="30" letter-spacing="3" text-anchor="middle" fill="#3B2A0C">HOUR 71</text></g>
    <g transform="translate(1200,${DESK.top + 4})"><rect x="-110" y="-128" width="220" height="130" rx="8" fill="#B9BCC2" stroke="#8E9197" stroke-width="3"/><circle cx="0" cy="-64" r="12" fill="#D6D8DC"/></g>
    <rect id="laptop-glow" x="1080" y="1640" width="240" height="120" fill="#BFE3FF" opacity="0.0"/>
    ${Array.from({ length: 6 }, (_, i) => `<g transform="translate(${1010 + (i % 2) * 6},${DESK.top - i * 38})"><rect x="-26" y="-36" width="52" height="36" rx="4" fill="#FFFFFF" stroke="#C9BFAE" stroke-width="2"/><path d="M26,-28 Q42,-20 26,-8" fill="none" stroke="#C9BFAE" stroke-width="5"/></g>`).join('')}
    ${[[1370, 0], [1410, 0], [1450, 0], [1390, 1], [1430, 1], [1410, 2]].map(([x, row]) => `<g transform="translate(${x},${DESK.top - row * 50})"><rect x="-17" y="-50" width="34" height="50" rx="5" fill="#3FA56B"/><rect x="-17" y="-38" width="34" height="14" fill="#F4E04D"/></g>`).join('')}
  </g>`;
}

function chairSVG() {
  return `<g id="chair"><g id="chair-back"><rect x="-120" y="-380" width="240" height="330" rx="60" fill="#7C2935"/>
    ${[[-60, -300], [0, -300], [60, -300], [-60, -220], [0, -220], [60, -220], [-60, -140], [0, -140], [60, -140]].map(([x, y]) => `<circle cx="${x}" cy="${y}" r="7" fill="#5A1A24"/>`).join('')}</g>
    <rect x="-100" y="-60" width="200" height="40" rx="14" fill="#6B2230"/></g>`;
}

// ---------------------------------------------------------------- build DOM
const stage = document.getElementById('stage');
stage.style.width = W + 'px';
stage.style.height = H + 'px';

const px = n => `${Math.round(n * UNIT)}px`;

stage.innerHTML = `
<div class="layer" id="room-layer"><svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <g id="world">${roomBackdropSVG()}
    <g id="butler-row">${BUTLERS.map(butlerSVG).join('')}</g>
    <g id="chair-pos">${chairSVG()}</g>
    <g id="dev-pos">${developerSVG()}</g>
    ${deskSVG()}
    <g id="friend-pos">${friendSVG()}</g>
    <g id="dev-front-pos"></g>
    <g id="flying-cards">${['claude', 'cursor'].map(id => `<g id="fly-${id}">${cardSVG(CARDS[id])}</g>`).join('')}</g>
    <g id="chandelier">${chandelierSVG()}</g>
  </g>
</svg></div>
<div class="layer" id="triptych-layer"></div>
<div class="layer" id="diagram-layer"></div>
<div class="layer" id="phone-layer"></div>
<div class="layer" id="exterior-layer"></div>
<div class="layer" id="title-layer"></div>
<div class="layer" id="end-layer"></div>
<div id="chapter"><div id="chapter-plaque" style="display:inline-block;background:rgba(110,34,48,0.9);border:3px solid #E9C66A;padding:${px(14)} ${px(40)} ${px(18)};border-radius:6px"><div class="chapter-number" style="font-size:${px(30)}"></div><div class="chapter-name" style="font-size:${px(70)}"></div></div></div>
<div id="subtitle"><span></span></div>
<div class="layer" id="vignette" style="background:radial-gradient(ellipse at center, rgba(0,0,0,0) 62%, rgba(60,10,25,0.28) 100%);pointer-events:none"></div>
<div class="layer" id="iris" style="pointer-events:none"></div>
`;

function chandelierSVG() {
  const arms = [-1, -0.5, 0, 0.5, 1].map(k => `<path d="M1200,120 Q${1200 + k * 160},170 ${1200 + k * 190},110" fill="none" stroke="${C.gold}" stroke-width="8"/>
    <rect x="${1200 + k * 190 - 7}" y="72" width="14" height="38" fill="#FFF8E6"/><ellipse cx="${1200 + k * 190}" cy="64" rx="7" ry="12" fill="#FFD36B"/>`).join('');
  return `<line x1="1200" y1="-400" x2="1200" y2="110" stroke="${C.goldDark}" stroke-width="6"/>${arms}<circle cx="1200" cy="126" r="22" fill="${C.gold}"/>
    <path d="M1180,146 L1200,190 L1220,146 Z" fill="${C.gold}"/>`;
}

const $ = id => document.getElementById(id);

// ---------------------------------------------------------------- title card
const titleFrameInset = PORTRAIT ? 70 : 60;
$('title-layer').innerHTML = `<div id="title-card" style="position:absolute;inset:0;background:#F4C7C3">
  <div class="card-frame" style="inset:${titleFrameInset}px"></div>
  <div class="card-frame" style="inset:${titleFrameInset + 22}px;border-width:2px;border-style:solid"></div>
  <div class="card-inner" id="title-inner">
    <div class="eyebrow" style="font-size:${px(30)};margin-bottom:${px(10)}">Looper Pictures</div>
    <div class="title-sub" style="font-size:${px(40)};margin-bottom:${px(40)}">presents</div>
    <div class="title-main" style="font-size:${px(PORTRAIT ? 100 : 96)}">The Butlers<br>Who Would Not<br>Proceed</div>
    <div style="width:${px(220)};height:4px;background:#7C2935;margin:${px(44)} 0 ${px(30)}"></div>
    <div class="title-sub" style="font-size:${px(38)}">A comedy in four chapters</div>
  </div></div>`;

// ---------------------------------------------------------------- triptych (tray close-ups)
const TRIPTYCH_COLORS = ['#F3D9C9', '#D8E8DA', '#F4E3A8'];
function triptychPanel(butler, i) {
  const viewBox = PORTRAIT ? '-175 -548 350 212' : '-125 -610 250 420';
  return `<div class="tri-panel" id="tri-${butler.id}" style="position:relative;flex:1;background:${TRIPTYCH_COLORS[i]};overflow:hidden;border:${px(10)} solid #F7EEDC">
    <svg id="tri-svg-${butler.id}" viewBox="${viewBox}" preserveAspectRatio="xMidYMid slice" style="position:absolute;inset:0;width:100%;height:100%">
      <rect x="-400" y="-900" width="800" height="1200" fill="${TRIPTYCH_COLORS[i]}"/>
      <rect x="-400" y="-360" width="800" height="600" fill="rgba(0,0,0,0.04)"/>
      ${butlerSVG({ ...butler, id: 'tri-' + butler.id, cardKey: butler.id })}
    </svg></div>`;
}
$('triptych-layer').innerHTML = `<div style="position:absolute;inset:0;background:#F7EEDC;display:flex;flex-direction:${PORTRAIT ? 'column' : 'row'}">${BUTLERS.map(triptychPanel).join('')}</div>`;

// ---------------------------------------------------------------- diagram
const DIAGRAM_INSET = PORTRAIT ? 50 : 44;
const FIG_W = W - DIAGRAM_INSET * 2 - 40;
const FIG_H = H - DIAGRAM_INSET * 2 - (PORTRAIT ? 700 : 330);
function macSVG() {
  const rows = [['CODEX', 'says it’s done'], ['CLAUDE CODE', 'has a question'], ['CURSOR', 'has a question']];
  return `<svg viewBox="${PORTRAIT ? '90 20 720 560' : '0 0 900 620'}" width="100%" height="100%">
    <rect x="110" y="40" width="680" height="440" rx="26" fill="#DDE3EA" stroke="${C.ink}" stroke-width="6"/>
    <rect x="136" y="66" width="628" height="388" rx="8" fill="#FBF7EC" stroke="${C.ink}" stroke-width="3"/>
    <path d="M40,500 L860,500 L820,552 L80,552 Z" fill="#C9D1DB" stroke="${C.ink}" stroke-width="6"/>
    <rect x="136" y="66" width="628" height="40" fill="#EFE6D2" stroke="${C.ink}" stroke-width="3"/>
    <image href="assets/looper-icon.png" x="176" y="124" width="64" height="64"/>
    <text x="252" y="168" font-family="Futura" font-weight="700" font-size="34" fill="${C.ink}" letter-spacing="3">LOOPER</text>
    ${rows.map(([n, s], i) => `<g class="mac-row" id="mac-row-${i}" transform="translate(176,${218 + i * 72})"><rect width="548" height="56" rx="10" fill="#FFFFFF" stroke="${C.ink}" stroke-width="2.5"/>
      <circle cx="30" cy="28" r="10" fill="${i === 0 ? '#E0A030' : '#5B8DEF'}"/>
      <text x="54" y="38" font-family="Futura" font-weight="700" font-size="30" fill="${C.ink}">${n}</text>
      <text x="532" y="38" font-family="Didot" font-style="italic" font-size="30" fill="${C.ink}" text-anchor="end">${s}</text></g>`).join('')}
  </svg>`;
}
/** Diagram geometry per orientation: portrait stacks the card above the test report. */
const FIG_LAYOUT = PORTRAIT ? {
  viewBox: '0 0 640 1000', card: [320, 300], stamp: [320, 160], report: [120, 400], typewriter: [470, 970],
  butlerY: 990, butlerScale: 0.36, marchEnd: 330, magnifier: [[190, 170], [430, 220]],
} : {
  viewBox: '0 0 1000 620', card: [230, 420], stamp: [230, 280], report: [600, 30], typewriter: [900, 610],
  butlerY: 612, butlerScale: 0.42, marchEnd: 770, magnifier: [[100, 280], [310, 340]],
};
function inspectionSVG() {
  const tests = ['test_login', 'test_checkout', 'test_refund', 'test_logout'];
  const L = FIG_LAYOUT;
  return `<svg viewBox="${L.viewBox}" width="100%" height="100%">
    <g transform="translate(${L.card[0]},${L.card[1]}) scale(2.6)">${cardSVG(CARDS.codex)}</g>
    <g id="inspect-butler" transform="translate(-900,${L.butlerY}) scale(${L.butlerScale})">${butlerSVG({ id: 'fig-codex', name: 'CODEX', tie: '#111', hair: 'part', cardKey: 'codex' })}</g>
    <g id="typewriter" transform="translate(${L.typewriter[0]},${L.typewriter[1]})"><rect x="-80" y="-60" width="160" height="60" rx="10" fill="#3A3A40"/><rect x="-60" y="-110" width="120" height="56" fill="#FFFDF6" stroke="${C.ink}" stroke-width="2"/></g>
    <g id="magnifier"><circle r="70" fill="rgba(200,230,255,0.35)" stroke="${C.ink}" stroke-width="10"/><line x1="50" y1="50" x2="120" y2="120" stroke="${C.ink}" stroke-width="18" stroke-linecap="round"/></g>
    <g transform="translate(${L.report[0]},${L.report[1]})"><rect width="400" height="330" rx="14" fill="#FFFFFF" stroke="${C.ink}" stroke-width="4"/>
      <text x="24" y="52" font-family="Futura" font-weight="700" font-size="28" fill="${C.ink}" letter-spacing="2">TEST REPORT</text>
      ${tests.map((name, i) => `<g transform="translate(24,${104 + i * 52})"><text id="test-mark-${i}" font-family="Futura" font-weight="700" font-size="32" fill="${C.stampRed}">✗</text>
        <text x="46" y="-2" font-family="'Courier New'" font-weight="700" font-size="26" fill="${C.ink}">${name}</text></g>`).join('')}
      <text id="test-summary" x="24" y="310" font-family="Futura" font-weight="700" font-size="30" fill="${C.stampRed}">4 FAILING</text></g>
    <g id="stamp-notdone" transform="translate(${L.stamp[0]},${L.stamp[1]}) rotate(-14)"><rect x="-190" y="-54" width="380" height="108" rx="10" fill="rgba(246,239,221,0.88)" stroke="${C.stampRed}" stroke-width="10"/>
      <text y="26" font-family="Futura" font-weight="700" font-size="72" letter-spacing="6" text-anchor="middle" fill="${C.stampRed}">NOT DONE</text></g>
    <g id="stamp-back" transform="translate(${L.stamp[0]},${L.stamp[1]}) rotate(8)"><rect x="-210" y="-48" width="420" height="96" rx="10" fill="rgba(246,239,221,0.85)" stroke="${C.stampBlue}" stroke-width="10"/>
      <text y="22" font-family="Futura" font-weight="700" font-size="58" letter-spacing="4" text-anchor="middle" fill="${C.stampBlue}">BACK TO WORK</text></g>
  </svg>`;
}
function beachPhoneSVG() {
  if (PORTRAIT) return beachPhonePortraitSVG();
  return `<svg viewBox="0 0 1000 620" width="100%" height="100%">
    <rect x="0" y="0" width="1000" height="330" fill="#BFE3EA"/><circle cx="820" cy="110" r="62" fill="#F7D774"/>
    ${[0, 1, 2, 3].map(i => `<rect x="0" y="${330 + i * 34}" width="1000" height="34" fill="${['#5FA8B8', '#74B8C5', '#8CC7D0', '#A5D5DA'][i]}"/>`).join('')}
    <rect x="0" y="466" width="1000" height="154" fill="#F2DDB0"/>
    <path d="M140,470 Q150,300 190,200" fill="none" stroke="#7A5230" stroke-width="16"/>
    <path d="M190,200 Q120,170 70,210 M190,200 Q240,150 300,180 M190,200 Q170,130 120,110 M190,200 Q250,210 280,260" fill="none" stroke="#3F8F5A" stroke-width="18" stroke-linecap="round"/>
    <g transform="translate(90,470)"><image href="assets/looper-icon.png" x="-10" y="-190" width="0" height="0"/></g>
    <path id="beam-path" d="M120,120 Q420,-40 600,190" fill="none" stroke="${C.ink}" stroke-width="5" stroke-dasharray="4 16" stroke-linecap="round"/>
    <g transform="translate(60,60)"><rect x="0" y="20" width="120" height="78" rx="8" fill="#DDE3EA" stroke="${C.ink}" stroke-width="4"/><path d="M-14,104 L134,104 L124,116 L-4,116 Z" fill="#C9D1DB" stroke="${C.ink}" stroke-width="4"/>
      <image href="assets/looper-icon.png" x="38" y="36" width="44" height="44"/></g>
    <g id="beach-phone" transform="translate(620,120)"><rect width="230" height="440" rx="36" fill="#1E1E22"/><rect x="12" y="12" width="206" height="416" rx="26" fill="#FDE6D3"/>
      <g transform="translate(22,90)"><rect width="186" height="96" rx="16" fill="#FFFFFF"/>
        <image href="assets/looper-icon.png" x="10" y="10" width="26" height="26"/><text x="42" y="30" font-family="Futura" font-weight="700" font-size="15" fill="#555">LOOPER</text>
        <text x="12" y="58" font-family="Futura" font-weight="700" font-size="15" fill="#111">Claude Code asks:</text>
        <text x="12" y="80" font-family="Futura" font-size="15" fill="#222">Sir, tabs or spaces?</text></g></g>
    <g transform="translate(340,560)"><rect x="-70" y="-10" width="140" height="10" fill="#E2735E"/><path d="M-60,-10 L-40,-80 L60,-80 L40,-10" fill="none" stroke="#8C5A33" stroke-width="8"/>
      <path d="M-40,-80 L60,-80 L50,-30 L-30,-30 Z" fill="#F4A7A0"/><path d="M-40,-80 L60,-80 L50,-30 L-30,-30 Z" fill="none" stroke="#fff" stroke-width="3" stroke-dasharray="12 12"/></g>
  </svg>`;
}
function beachPhonePortraitSVG() {
  return `<svg viewBox="0 0 640 1000" width="100%" height="100%">
    <rect width="640" height="560" fill="#BFE3EA"/><circle cx="520" cy="120" r="60" fill="#F7D774"/>
    ${[0, 1, 2, 3].map(i => `<rect x="0" y="${560 + i * 40}" width="640" height="40" fill="${['#5FA8B8', '#74B8C5', '#8CC7D0', '#A5D5DA'][i]}"/>`).join('')}
    <rect y="720" width="640" height="280" fill="#F2DDB0"/>
    <path d="M90,740 Q100,560 140,440" fill="none" stroke="#7A5230" stroke-width="16"/>
    <path d="M140,440 Q70,410 20,450 M140,440 Q190,390 250,420 M140,440 Q120,370 70,350 M140,440 Q200,450 230,500" fill="none" stroke="#3F8F5A" stroke-width="18" stroke-linecap="round"/>
    <path id="beam-path" d="M130,150 Q300,40 380,300" fill="none" stroke="${C.ink}" stroke-width="5" stroke-dasharray="4 16" stroke-linecap="round"/>
    <g transform="translate(40,70)"><rect x="0" y="20" width="120" height="78" rx="8" fill="#DDE3EA" stroke="${C.ink}" stroke-width="4"/><path d="M-14,104 L134,104 L124,116 L-4,116 Z" fill="#C9D1DB" stroke="${C.ink}" stroke-width="4"/>
      <image href="assets/looper-icon.png" x="38" y="36" width="44" height="44"/></g>
    <g id="beach-phone" transform="translate(300,320)"><g transform="scale(1.3)"><rect width="230" height="440" rx="36" fill="#1E1E22"/><rect x="12" y="12" width="206" height="416" rx="26" fill="#FDE6D3"/>
      <g transform="translate(22,90)"><rect width="186" height="96" rx="16" fill="#FFFFFF"/>
        <image href="assets/looper-icon.png" x="10" y="10" width="26" height="26"/><text x="42" y="30" font-family="Futura" font-weight="700" font-size="15" fill="#555">LOOPER</text>
        <text x="12" y="58" font-family="Futura" font-weight="700" font-size="15" fill="#111">Claude Code asks:</text>
        <text x="12" y="80" font-family="Futura" font-size="15" fill="#222">Sir, tabs or spaces?</text></g></g></g>
    <g transform="translate(170,900)"><rect x="-70" y="-10" width="140" height="10" fill="#E2735E"/><path d="M-60,-10 L-40,-80 L60,-80 L40,-10" fill="none" stroke="#8C5A33" stroke-width="8"/>
      <path d="M-40,-80 L60,-80 L50,-30 L-30,-30 Z" fill="#F4A7A0"/></g>
  </svg>`;
}
const BEACH_PHONE_BASE = PORTRAIT ? [300, 320] : [620, 120];
const FIGS = [
  { id: 'fig-mac', label: 'FIG. 1', caption: 'Looper, on your Mac', svg: macSVG() },
  { id: 'fig-inspect', label: 'FIG. 2', caption: 'Is it really done?', svg: inspectionSVG() },
  { id: 'fig-beach', label: 'FIG. 3', caption: 'Real questions, anywhere', svg: beachPhoneSVG() },
];
$('diagram-layer').innerHTML = `<div class="paper"></div>
  <div class="diagram-border" style="inset:${DIAGRAM_INSET}px"></div>
  <div class="diagram-head" style="top:${DIAGRAM_INSET + (PORTRAIT ? 70 : 38)}px;font-size:${px(34)}">CHAPTER THREE \u00b7 THE EXPLANATION</div>
  <div id="fig-strip" style="position:absolute;left:0;top:0;width:${W * FIGS.length}px;height:${H}px">
  ${FIGS.map((f, i) => `<div class="fig" id="${f.id}" style="left:${i * W}px;width:${W}px;height:${H}px">
      <div style="position:absolute;left:${DIAGRAM_INSET + 20}px;top:${DIAGRAM_INSET + (PORTRAIT ? 330 : 200)}px;width:${FIG_W}px;height:${FIG_H}px">${f.svg}</div>
      <div class="fig-caption" id="${f.id}-caption" style="top:${DIAGRAM_INSET + (PORTRAIT ? 130 : 84)}px;font-size:${px(PORTRAIT ? 50 : 44)};padding:0 ${px(60)}"><b style="font-size:${px(24)};margin-bottom:${px(4)}">${f.label}</b>${f.caption}</div>
    </div>`).join('')}</div>`;

// ---------------------------------------------------------------- phone close-up
const PHONE_H = PORTRAIT ? 1160 : 1000;
const PHONE_W = PHONE_H * 0.49;
const PHONE_X = PORTRAIT ? (W - PHONE_W) / 2 : W * 0.5 - PHONE_W / 2;
const PHONE_Y = PORTRAIT ? 250 : (H - PHONE_H) / 2 + 20;
const NOTIF_FONT = PHONE_W / 470;
function notificationHTML(id, agent, question, actions) {
  const f = n => `${Math.round(n * NOTIF_FONT)}px`;
  return `<div class="notif" id="${id}" style="padding:${f(18)} ${f(20)} 0">
    <div class="notif-row" style="gap:${f(12)};margin-bottom:${f(8)}"><img class="notif-icon" src="assets/looper-icon.png" style="width:${f(40)};height:${f(40)}">
      <div class="notif-app" style="font-size:${f(19)}">Looper</div><div style="margin-left:auto;font-size:${f(18)};color:#77777d">now</div></div>
    <div class="notif-title" style="font-size:${f(25)}">${agent} asks</div>
    <div class="notif-body" id="${id}-body" style="font-size:${f(25)};margin:${f(4)} 0 ${f(16)}">${question}</div>
    <div class="notif-actions" style="margin:0 ${f(-20)}">${actions.map((a, i) => `<div id="${id}-a${i}" style="padding:${f(16)} 0;font-size:${f(24)}">${a}</div>`).join('')}</div>
  </div>`;
}
$('phone-layer').innerHTML = `<div style="position:absolute;inset:0;background:repeating-linear-gradient(90deg,#F4C7C3 0 ${px(90)},#F7D7D0 ${px(90)} ${px(180)})"></div>
  ${PORTRAIT ? '' : `<div style="position:absolute;left:${W * 0.05}px;top:${H * 0.3}px;width:${W * 0.25}px;text-align:right" class="status-line" id="phone-left"><div style="font-size:${px(38)}">From your Mac</div><div style="font-size:${px(84)};font-weight:700;letter-spacing:0.04em;line-height:1.05">TO YOUR<br>PHONE</div><div style="font-size:${px(38)};margin-top:${px(10)}">wherever you are</div></div>`}
  <div id="phone-device" style="position:absolute;left:${PHONE_X}px;top:${PHONE_Y}px;width:${PHONE_W}px;height:${PHONE_H}px;border-radius:${PHONE_W * 0.16}px;background:#1C1C20;box-shadow:0 30px 60px rgba(80,20,40,0.35)">
    <div style="position:absolute;inset:${PHONE_W * 0.035}px;border-radius:${PHONE_W * 0.13}px;overflow:hidden;background:linear-gradient(#BFE3EA 0 46%,#6FB3C2 46% 60%,#F2DDB0 60%)">
      <div style="position:absolute;top:${PHONE_H * 0.08}px;left:0;right:0;text-align:center;color:#fff;font-family:-apple-system,'SF Pro Display',sans-serif;font-weight:600;font-size:${PHONE_W * 0.2}px;text-shadow:0 2px 8px rgba(0,0,0,0.15)">10:09</div>
      <div id="notif-stack" style="position:absolute;left:0;right:0;top:${PHONE_H * 0.3}px">
        ${notificationHTML('notif-claude', 'Claude Code', 'Sir, tabs or spaces?', ['Spaces', 'Tabs'])}
        ${notificationHTML('notif-cursor', 'Cursor', 'Shall I drop the production database?', ['Yes', 'No'])}
      </div>
    </div>
    <div id="thumb" style="position:absolute;width:${PHONE_W * 0.22}px;height:${PHONE_W * 0.3}px;border-radius:50% 50% 45% 45%;background:${C.tan};opacity:0.0;box-shadow:0 6px 14px rgba(0,0,0,0.25)"></div>
  </div>
  <div id="phone-status" class="status-line" style="position:absolute;${PORTRAIT ? `left:0;right:0;top:60px;text-align:center` : `left:${W * 0.7}px;top:${H * 0.32}px;width:${W * 0.26}px`}">
    <div id="status-claude" style="font-size:${px(48)};margin-bottom:${px(22)}">Claude Code · <b>back to work</b></div>
    <div id="status-cursor" style="font-size:${px(48)}">Cursor · <b>back to work</b></div>
  </div>`;

// ---------------------------------------------------------------- exterior
$('exterior-layer').innerHTML = `<svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}"><g id="exterior-world">${exteriorSVG()}</g></svg>`;
function exteriorSVG() {
  const horizon = H * 0.42;
  const huts = Array.from({ length: 7 }, (_, i) => {
    const x = W * (0.08 + i * 0.14);
    const w = W * 0.09, h = w * 1.25;
    return `<g transform="translate(${x},${horizon + H * 0.14})"><rect x="${-w / 2}" y="${-h}" width="${w}" height="${h}" fill="${i % 2 ? '#F4B6B0' : '#9ED3D8'}"/>
      ${[0, 1, 2].map(k => `<rect x="${-w / 2 + w * (0.1 + k * 0.32)}" y="${-h}" width="${w * 0.14}" height="${h}" fill="#FFF6E6"/>`).join('')}
      <path d="M${-w * 0.62},${-h} L0,${-h - w * 0.5} L${w * 0.62},${-h} Z" fill="#8C2E3B"/></g>`;
  }).join('');
  return `<rect width="${W}" height="${horizon}" fill="#BFE3EA"/><circle cx="${W / 2}" cy="${horizon * 0.42}" r="${Math.min(W, H) * 0.11}" fill="#F7D774"/>
    ${[0, 1, 2, 3].map(i => `<rect y="${horizon + i * H * 0.03}" width="${W}" height="${H * 0.03}" fill="${['#4F9DB0', '#63AEBF', '#7ABFCB', '#95CFD5'][i]}"/>`).join('')}
    <rect y="${horizon + H * 0.12}" width="${W}" height="${H}" fill="#F2DDB0"/>
    ${huts}
    <g id="runner">${developerSVG().replace(/id="dev/g, 'id="run')}</g>`;
}

// ---------------------------------------------------------------- end card
$('end-layer').innerHTML = `<div style="position:absolute;inset:0;background:#F4C7C3">
  <div class="card-frame" style="inset:${titleFrameInset}px"></div>
  <div class="card-frame" style="inset:${titleFrameInset + 22}px;border-width:2px;border-style:solid"></div>
  <div class="card-inner" id="end-inner" style="padding:0 ${px(PORTRAIT ? 110 : 120)}">
    <img id="end-orb" src="assets/looper-icon.png" style="width:${px(220)};height:${px(220)};margin-bottom:${px(20)};mix-blend-mode:multiply">
    <div class="end-line" id="end-1" style="font-size:${px(PORTRAIT ? 62 : 60)};max-width:${PORTRAIT ? W * 0.8 : W * 0.8}px;font-weight:700;line-height:1.15;margin-bottom:${px(30)}">Your coding agents stop<br>before the work is done.</div>
    <div class="end-line" id="end-2" style="font-size:${px(PORTRAIT ? 46 : 42)};line-height:1.3;margin-bottom:${px(50)}">Looper sends them back, and brings the real decisions to your phone.</div>
    <div class="end-url" id="end-url" style="font-size:${px(PORTRAIT ? 118 : 112)}">looper.fyi</div>
  </div></div>`;

// ---------------------------------------------------------------- shared pose writers
function setTransform(id, value) { const el = $(id); if (el) el.setAttribute('transform', value); }
function show(id, visible) { $(id).classList.toggle('hidden', !visible); }
function setOpacity(id, value) { $(id).style.opacity = value; }

/** Walk/march cycle: leg swing angle and vertical bob. */
function gait(phase, amplitude) {
  return { swing: Math.sin(phase) * amplitude, bob: -Math.abs(Math.sin(phase)) * amplitude * 0.5 };
}

// ---------------------------------------------------------------- room scene
const ROOM_CAMERA = [
  [4.3, 1200, 1345, 2800, 1300],
  [12.0, 1200, 1400, 2500, 1200],
  [14.8, 1200, 1330, 1650, 1150],
  [15.78, 1200, 1320, 1620, 1140, 'linear'],
  [15.96, 780, 1225, 480, 420, 'snap'], [17.18, 780, 1218, 455, 400, 'linear'],
  [17.36, 1200, 1225, 480, 420, 'snap'], [18.58, 1200, 1218, 455, 400, 'linear'],
  [18.76, 1620, 1225, 480, 420, 'snap'], [19.9, 1620, 1218, 455, 400, 'linear'],
  [20.08, 1200, 1360, 1900, 1260, 'snap'],
  [26.2, 1200, 1390, 1720, 1160, 'linear'],
  [31.8, 1200, 1345, 2800, 1300], [35.6, 1210, 1355, 2700, 1290, 'linear'],
  [39.8, 1400, 1580, 1500, 1120],
  [51.5, 1360, 1340, 2500, 1260], [54.7, 1420, 1360, 2200, 1180, 'linear'],
  [55.4, 1812, 1122, 150, 150, 'in'],
  [58.9, 1200, 1345, 2800, 1300], [62.3, 1200, 1355, 2700, 1280, 'linear'],
  [63.9, 1560, 1355, 2700, 1280, 'in'],
];

function friendPose(t) {
  // enters through the right-wall door, walks to the mark by the desk
  const doorPoint = rightWallPoint((DOOR.farDepth + DOOR.nearDepth) / 2, 0);
  const walk = EASE.inout(progress(t, 32.4, 35.4));
  const feetY = lerp(doorPoint[1] + 20, FRIEND_MARK.feetY, walk);
  const x = lerp(doorPoint[0] - 40, FRIEND_MARK.x, walk);
  const walking = t > 32.4 && t < 35.4;
  const raise = Math.max(EASE.inout(progress(t, 39.1, 39.7)), t > 51 && t < 58.9 ? 1 : 0) * (t < 58.9 ? 1 : 0);
  return { x, feetY, walking, phase: (t - 32.4) * 9, raise, visible: t >= 32.1 };
}

/** World position of the friend's phone (anchor for the flying cards and the camera zoom). */
function phoneWorld(pose) {
  const sc = scaleAt(pose.feetY);
  const hand = raisedHand(pose.raise);
  return [pose.x + hand[0] * sc, pose.feetY + hand[1] * sc];
}
function raisedHand(raise) { return [lerp(90, 118, raise), lerp(-320, -590, raise)]; }

function applyFriend(t) {
  const pose = friendPose(t);
  show('friend-pos', pose.visible && !(t > 40.2 && t < 51.3));
  const sc = scaleAt(pose.feetY);
  const g = pose.walking ? gait(pose.phase, 22) : { swing: 0, bob: 0 };
  setTransform('friend-pos', `translate(${pose.x},${pose.feetY + g.bob * sc}) scale(${sc})`);
  setTransform('friend-legL', `rotate(${g.swing},-23,-250)`);
  setTransform('friend-legR', `rotate(${-g.swing},23,-250)`);
  const hand = raisedHand(pose.raise);
  $('friend-armR-path').setAttribute('d', `M76,-460 Q${lerp(104, 140, pose.raise)},${lerp(-380, -520, pose.raise)} ${hand[0]},${hand[1]}`);
  $('friend-hand').setAttribute('cx', hand[0]); $('friend-hand').setAttribute('cy', hand[1]);
  setTransform('friend-phone', `translate(${hand[0]},${hand[1] - 24}) scale(${lerp(0.6, 1.2, pose.raise)})`);
  setOpacity('friend-phone', pose.raise > 0.05 ? 1 : 0);
  return pose;
}

function devPose(t) {
  const stand = EASE.back(progress(t, 60.9, 61.6));
  const sidestep = EASE.inout(progress(t, 61.7, 62.2));
  const run = EASE.in(progress(t, 62.2, 63.5));
  const x = DEV_HOME.x + sidestep * 330 + run * 1100;
  const feetY = DEV_HOME.feetY - run * 380;
  const sink = 330 * (1 - stand);
  const look = t > 35.2 && t < 40 ? EASE.inout(progress(t, 35.2, 35.6)) * 6 : t > 51.4 && t < 58.9 ? 6 : 0;
  return { x, feetY, sink, look, running: t > 62.2, runPhase: (t - 62.2) * 16, stand };
}

function applyDeveloper(t) {
  const pose = devPose(t);
  const sc = scaleAt(pose.feetY) * 1.02;
  const g = pose.running ? gait(pose.runPhase, 34) : { swing: 0, bob: 0 };
  setTransform('dev-pos', `translate(${pose.x},${pose.feetY + pose.sink + g.bob * sc}) scale(${sc})`);
  setTransform('dev-legL', `rotate(${g.swing},-25,-252)`);
  setTransform('dev-legR', `rotate(${-g.swing},25,-252)`);
  const legsVisible = pose.stand > 0.6 ? 1 : 0;
  setOpacity('dev-legL', legsVisible); setOpacity('dev-legR', legsVisible);
  // typing: tiny hand jitter while seated; running: big arm swing
  const typing = pose.stand < 0.1 && (t < 31.8 || (t > 58.9 && t < 60.9)) ? Math.sin(t * 38) * 3 : 0;
  setTransform('dev-armL', pose.running ? `rotate(${-g.swing * 1.4},-80,-420)` : `translate(${typing},0)`);
  setTransform('dev-armR', pose.running ? `rotate(${g.swing * 1.4},80,-420)` : `translate(${-typing},0)`);
  // pupils: stare straight, dart to the friend, one slow blink per shot
  $('dev-pupils').setAttribute('transform', `translate(${pose.look},${t > 60.9 && t < 61.6 ? -3 : 0})`);
  // the developer is behind the desk until he steps out to the side
  const inFront = pose.x > DESK.right + 40;
  const parent = inFront ? $('dev-front-pos') : $('world');
  const devPos = $('dev-pos');
  if (inFront && devPos.parentNode !== parent) parent.appendChild(devPos);
  if (!inFront && devPos.parentNode !== $('world')) $('world').insertBefore(devPos, $('desk'));
  $('laptop-glow').setAttribute('opacity', 0);
  return pose;
}

function applyChair(t) {
  const sc = scaleAt(DEV_HOME.feetY - 60);
  const spin = t > 62.3 ? (t - 62.3) * 7 * Math.exp(-(t - 62.3) * 0.25) : 0;
  const width = Math.cos(spin);
  setTransform('chair-pos', `translate(${DEV_HOME.x},${DEV_HOME.feetY - 60}) scale(${sc})`);
  setTransform('chair-back', `scale(${Math.max(0.12, Math.abs(width))},1)`);
  $('chair-back').querySelector('rect').setAttribute('fill', width < 0 ? '#5E1D28' : '#7C2935');
}

function applyButlers(t) {
  BUTLERS.forEach((b, i) => {
    let x = b.x, facing = 1, bow = 0, marchPhase = 0, marching = false;
    // Codex was sent back to work in the diagram: marches off stage left in the next room shot
    if (b.id === 'codex' && t > 51.3) { marching = true; marchPhase = (t - 51.3) * 8; x = b.x - EASE.in(progress(t, 51.3, 53.8)) * 900; facing = -1; }
    // the others bow in unison once answered, then march off to work
    if (b.id !== 'codex' && t > 58.9) {
      bow = Math.sin(Math.PI * progress(t, 59.0, 59.9)) * 24;
      const leave = progress(t, 59.9, 61.4);
      if (leave > 0) { marching = true; marchPhase = (t - 59.9) * 8; x = b.x + (b.id === 'cursor' ? 1 : -1) * EASE.in(leave) * 1300; facing = b.id === 'cursor' ? 1 : -1; }
    }
    const g = marching ? gait(marchPhase, 18) : { swing: 0, bob: 0 };
    void facing;
    setTransform(b.id, `translate(${x},${BUTLER_FEET_Y + g.bob}) scale(0.97)`);
    setTransform(`${b.id}-legL`, `rotate(${g.swing},-23,-262)`);
    setTransform(`${b.id}-legR`, `rotate(${-g.swing},23,-262)`);
    setTransform(`${b.id}-upper`, `translate(0,-262) scale(1,${1 - bow * 0.009}) translate(0,262)`);
    // deadpan blink: one per butler, staggered
    const blinkAt = [13.4, 21.7, 24.1][i];
    const blink = Math.abs(t - blinkAt) < 0.07 || Math.abs(t - (blinkAt + 6.3)) < 0.07;
    $(b.id + '-head').querySelector('.eyes').setAttribute('transform', blink ? `translate(0,-548) scale(1,0.15) translate(0,548)` : '');
  });
  // question cards leave the trays when they start flying
  ['claude', 'cursor'].forEach(id => setOpacity(id + '-card', t > CARD_FLIGHTS[id].start && t < 58.9 ? 0 : 1));
  setOpacity('codex-card', t > 51.3 ? 0 : 1);
}

const CARD_FLIGHTS = { claude: { start: 52.0, end: 54.1 }, cursor: { start: 52.5, end: 54.6 } };
function applyFlyingCards(t, friend) {
  const target = phoneWorld(friend);
  ['claude', 'cursor'].forEach(id => {
    const flight = CARD_FLIGHTS[id];
    const p = progress(t, flight.start, flight.end);
    const flying = t > flight.start && t < flight.end + 0.05 && t < 55.4;
    show('fly-' + id, flying);
    if (!flying) return;
    const butler = BUTLERS.find(b => b.id === id);
    const from = [butler.x, BUTLER_FEET_Y - 340 * 0.97];
    const e = EASE.inout(p);
    // quadratic arc that rises over the room, then dives into the phone
    const control = [lerp(from[0], target[0], 0.35), Math.min(from[1], target[1]) - 520];
    const x = (1 - e) * (1 - e) * from[0] + 2 * (1 - e) * e * control[0] + e * e * target[0];
    const y = (1 - e) * (1 - e) * from[1] + 2 * (1 - e) * e * control[1] + e * e * target[1];
    const scale = 0.97 * (1 + Math.sin(Math.PI * e) * 4.2) * lerp(1, 0.28, EASE.in(p));
    const spin = Math.sin(Math.PI * e) * (id === 'claude' ? -16 : 14);
    setTransform('fly-' + id, `translate(${x},${y + 36 * scale}) rotate(${spin}) scale(${scale})`);
  });
}

function applyClockAndProps(t) {
  setTransform('pendulum', `rotate(${Math.sin(t * Math.PI / 0.6036) * 7},1200,600)`);
  const minutes = 17 + t * 0.9;
  setTransform('clock-minute', `rotate(${minutes * 6},1200,470)`);
  setTransform('clock-hour', `rotate(${(9 + minutes / 60) * 30},1200,470)`);
  setTransform('chandelier', `rotate(${Math.sin(t * 0.9) * 0.6},1200,-400)`);
  $('hour-plaque').textContent = t < 31.8 ? 'HOUR 71' : 'HOUR 72';
  // door: flies open as the friend arrives, stays open, light pours in
  const open = EASE.snap(progress(t, 31.8, 32.3));
  const hinge = [rightWallPoint(DOOR.nearDepth, 0), rightWallPoint(DOOR.nearDepth, DOOR.heightRatio)];
  const farDepth = lerp(DOOR.farDepth, DOOR.nearDepth + 0.25, open);
  const leaf = [rightWallPoint(farDepth, 0), hinge[0], hinge[1], rightWallPoint(farDepth, DOOR.heightRatio)];
  if (open > 0) { leaf[0][0] -= open * 90; leaf[3][0] -= open * 90; }
  $('door-leaf').setAttribute('points', pts(leaf));
  const sill = [rightWallPoint(DOOR.farDepth, 0), rightWallPoint(DOOR.nearDepth, 0)];
  $('door-light').setAttribute('points', pts([sill[0], sill[1], [sill[1][0] - 900, sill[1][1] + 520], [sill[0][0] - 1000, sill[0][1] + 180]]));
  $('door-light').setAttribute('opacity', open * 0.55);
}

function applyRoom(t) {
  const [x, y, wL, wP] = keyed(t, ROOM_CAMERA);
  const zoom = W / (PORTRAIT ? wP : wL);
  setTransform('world', `translate(${W / 2},${H / 2}) scale(${zoom}) translate(${-x},${-y})`);
  applyClockAndProps(t);
  applyButlers(t);
  applyChair(t);
  applyDeveloper(t);
  const friend = applyFriend(t);
  applyFlyingCards(t, friend);
}

// ---------------------------------------------------------------- triptych scene (26.2 – 31.8)
function applyTriptych(t) {
  const focus = [[26.2, 27.9, 'claude'], [27.9, 29.3, 'codex'], [29.3, 30.7, 'cursor']];
  BUTLERS.forEach((b, i) => {
    const active = focus.find(([a, e, id]) => id === b.id && t >= a && t < e);
    const settled = t >= 30.7;
    const target = active || settled ? 1 : 0.55;
    $('tri-' + b.id).style.filter = `saturate(${lerp(0.35, 1, target)}) brightness(${lerp(0.8, 1, target)})`;
    const push = 1 + progress(t, 26.2, 31.8) * 0.06 + (active ? 0.03 * EASE.out(progress(t, active[0], active[0] + 0.4)) : 0);
    $('tri-svg-' + b.id).style.transform = `scale(${push})`;
    // one mechanical blink per butler
    const blink = Math.abs(t - (27.2 + i * 1.3)) < 0.07;
    $('tri-' + b.id + '-head').querySelector('.eyes').setAttribute('transform', blink ? 'translate(0,-548) scale(1,0.15) translate(0,548)' : '');
  });
}

// ---------------------------------------------------------------- diagram scene (40.0 – 51.5)
const FIG_TIMES = [40.0, 43.0, 48.6];
function applyDiagram(t) {
  // slide the strip left one figure at a time; figure 2 persists through the "back to work" beat
  const index = FIG_TIMES.reduce((acc, start, i) => acc + (i === 0 ? 0 : EASE.inout(progress(t, start - 0.2, start + 0.25))), 0);
  $('fig-strip').style.transform = `translateX(${-index * W}px)`;
  FIGS.forEach((f, i) => {
    const since = progress(t, FIG_TIMES[i], FIG_TIMES[i] + (i === 1 ? 5.6 : 3.2));
    $(f.id).firstElementChild.style.transform = `scale(${1 + since * 0.05})`;
  });
  for (let i = 0; i < 3; i++) {
    const p = EASE.back(progress(t, 40.35 + i * 0.35, 40.7 + i * 0.35));
    const row = $('mac-row-' + i);
    row.setAttribute('opacity', clamp(p * 1.5));
    row.setAttribute('transform', `translate(${176 + (1 - p) * 60},${218 + i * 72})`);
  }
  const caption = $('fig-inspect-caption');
  caption.innerHTML = t < 45.6 ? '<b>FIG. 2</b>Is it really done?' : '<b>FIG. 2</b>Tests fail: back to work';
  caption.querySelector('b').style.cssText = `font-size:${px(24)};margin-bottom:${px(4)}`;
  // magnifier sweeps over the card
  const sweep = EASE.inout(progress(t, 43.3, 44.4));
  const [m0, m1] = FIG_LAYOUT.magnifier;
  setTransform('magnifier', `translate(${lerp(m0[0], m1[0], sweep)},${lerp(m0[1], m1[1], sweep) + Math.sin(sweep * Math.PI) * -60})`);
  setOpacity('magnifier', 1 - progress(t, 44.5, 44.8));
  // NOT DONE stamp slams
  const slam = progress(t, 44.6, 44.75);
  setTransform('stamp-notdone', `translate(${FIG_LAYOUT.stamp[0]},${FIG_LAYOUT.stamp[1]}) rotate(-14) scale(${t < 44.6 ? 0 : lerp(1.8, 1, EASE.out(slam))})`);
  setOpacity('stamp-notdone', t < 44.6 ? 0 : (t > 45.6 ? 1 - progress(t, 45.6, 45.9) : 1));
  const slamBack = progress(t, 45.9, 46.05);
  setTransform('stamp-back', `translate(${FIG_LAYOUT.stamp[0]},${FIG_LAYOUT.stamp[1] + 30}) rotate(8) scale(${t < 45.9 ? 0 : lerp(1.8, 1, EASE.out(slamBack))})`);
  setOpacity('stamp-back', t < 45.9 ? 0 : 1 - progress(t, 48.0, 48.4));
  // Codex marches in from the left to the typewriter, then types
  const march = EASE.inout(progress(t, 45.9, 47.2));
  const marchX = lerp(-900, FIG_LAYOUT.marchEnd, march);
  const bob = march > 0 && march < 1 ? -Math.abs(Math.sin((t - 45.9) * 9)) * 10 : 0;
  const typing = march >= 1 ? Math.sin(t * 40) * 2 : 0;
  setTransform('inspect-butler', `translate(${marchX},${FIG_LAYOUT.butlerY + bob + typing}) scale(${FIG_LAYOUT.butlerScale})`);
  setTransform('fig-codex-legL', `rotate(${march > 0 && march < 1 ? Math.sin((t - 45.9) * 9) * 18 : 0},-23,-262)`);
  setTransform('fig-codex-legR', `rotate(${march > 0 && march < 1 ? -Math.sin((t - 45.9) * 9) * 18 : 0},23,-262)`);
  // tests flip to green one by one once he types
  let passing = 0;
  for (let i = 0; i < 4; i++) {
    const flipped = t > 47.3 + i * 0.3;
    if (flipped) passing++;
    const mark = $('test-mark-' + i);
    mark.textContent = flipped ? '✓' : '✗';
    mark.setAttribute('fill', flipped ? C.green : C.stampRed);
  }
  const summary = $('test-summary');
  summary.textContent = passing === 4 ? '4 PASSING' : passing ? `${4 - passing} FAILING` : '4 FAILING';
  summary.setAttribute('fill', passing === 4 ? C.green : C.stampRed);
  // beam dashes travel from the Mac to the phone
  $('beam-path').setAttribute('stroke-dashoffset', -t * 60);
  setTransform('beach-phone', `translate(${BEACH_PHONE_BASE[0]},${BEACH_PHONE_BASE[1] + (1 - EASE.back(progress(t, 48.7, 49.3))) * 60 + Math.sin((t - 48.7) * 2.4) * 10}) rotate(${Math.sin((t - 48.7) * 1.7) * 2})`);
}

// ---------------------------------------------------------------- phone scene (55.4 – 58.9)
function applyPhone(t) {
  const arrive = [55.45, 55.85];
  ['notif-claude', 'notif-cursor'].forEach((id, i) => {
    const p = EASE.back(progress(t, arrive[i], arrive[i] + 0.35));
    const el = $(id);
    const top = i === 0 ? 0 : el.previousElementSibling.offsetHeight + 24 * NOTIF_FONT;
    el.style.top = `${top}px`;
    el.style.transform = `translateY(${(1 - p) * -160}px) scale(${lerp(0.9, 1, p)})`;
    el.style.opacity = clamp(p * 1.4);
  });
  const taps = [{ id: 'notif-claude', button: 0, at: 56.05, reply: 'Replied: Spaces' }, { id: 'notif-cursor', button: 1, at: 57.35, reply: 'Replied: Absolutely not' }];
  const thumb = $('thumb');
  let thumbTarget = null;
  taps.forEach(tap => {
    const button = $(`${tap.id}-a${tap.button}`);
    const pressed = t > tap.at && t < tap.at + 0.25;
    button.classList.toggle('pressed', pressed);
    const replied = t >= tap.at + 0.25;
    $(`${tap.id}-body`).innerHTML = replied ? `<span class="notif-replied">✓ ${tap.reply}</span>` : (tap.id === 'notif-claude' ? 'Sir, tabs or spaces?' : 'Shall I drop the production database?');
    $(`${tap.id}-a0`).parentElement.style.display = replied ? 'none' : 'flex';
    if (t > tap.at - 0.5 && t < tap.at + 0.45) thumbTarget = { button, tap };
  });
  if (thumbTarget) {
    const device = $('phone-device').getBoundingClientRect();
    const notif = $(thumbTarget.tap.id).getBoundingClientRect();
    const r = { left: notif.left + notif.width * (thumbTarget.tap.button === 0 ? 0 : 0.5), width: notif.width / 2, top: notif.top + notif.height - 70 * NOTIF_FONT, height: 60 * NOTIF_FONT };
    const approach = EASE.out(progress(t, thumbTarget.tap.at - 0.5, thumbTarget.tap.at));
    const leave = EASE.in(progress(t, thumbTarget.tap.at + 0.1, thumbTarget.tap.at + 0.4));
    thumb.style.left = `${r.left - device.left + r.width / 2 - PHONE_W * 0.11}px`;
    thumb.style.top = `${r.top - device.top + r.height * 0.3 + (1 - approach) * 220 + leave * 260}px`;
    thumb.style.opacity = Math.min(approach * 1.5, 1 - leave) * 0.95;
  } else thumb.style.opacity = 0;
  $('status-claude').style.opacity = t > 56.3 ? EASE.out(progress(t, 56.3, 56.6)) : 0;
  $('status-cursor').style.opacity = t > 57.6 ? EASE.out(progress(t, 57.6, 57.9)) : 0;
  const push = 1 + progress(t, 55.4, 58.9) * 0.07;
  $('phone-device').style.transform = `scale(${push})`;
}

// ---------------------------------------------------------------- exterior (63.8 – 68.5)
function applyExterior(t) {
  const p = progress(t, 63.8, 68.5);
  const run = EASE.out(progress(t, 63.8, 67.6));
  const baseScale = (PORTRAIT ? 1.6 : 1.35) * UNIT;
  const sc = lerp(baseScale, baseScale * 0.28, run);
  const g = gait((t - 63.8) * 15, 34);
  const x = W / 2 + Math.sin(p * 3) * 20;
  const y = lerp(H * 1.02, H * 0.62, run);
  setTransform('runner', `translate(${x},${y + g.bob * sc}) scale(${sc})`);
  setTransform('run-legL', `rotate(${g.swing},-25,-252)`);
  setTransform('run-legR', `rotate(${-g.swing},25,-252)`);
  // arms up in triumph
  setTransform('run-armL', `rotate(${150 + Math.sin(t * 14) * 10},-80,-420)`);
  setTransform('run-armR', `rotate(${-150 - Math.sin(t * 14) * 10},80,-420)`);
  setTransform('exterior-world', `translate(${W / 2},${H / 2}) scale(${1.04 - p * 0.04}) translate(${-W / 2},${-H / 2})`);
}

// ---------------------------------------------------------------- title and end card
function applyTitle(t) {
  const push = 1 + t * 0.014;
  $('title-inner').style.transform = `scale(${push}) translateY(${-t * 3 * UNIT}px)`;
  // the card lifts away like a curtain, the room already in place underneath
  const lift = EASE.in(progress(t, 4.3, 4.8));
  $('title-card').style.transform = `translateY(${-lift * H * 1.05}px)`;
}
function applyEnd(t) {
  const items = [['end-orb', 68.45], ['end-1', 68.5], ['end-2', 69.1], ['end-url', 69.8]];
  items.forEach(([id, at]) => {
    const p = EASE.out(progress(t, at, at + 0.3));
    const el = $(id);
    el.style.opacity = p;
    el.style.transform = `translateY(${(1 - p) * 30 * UNIT}px)`;
  });
  $('end-inner').style.transform = `scale(${1 + progress(t, 68.5, 75) * 0.06})`;
  $('end-orb').style.transform += ` rotate(${(t - 68.5) * 8}deg)`;
}

// ---------------------------------------------------------------- overlays
function applyChapter(t) {
  const chapter = [...CHAPTERS].reverse().find(([at]) => t >= at);
  const el = $('chapter');
  if (!chapter || t > chapter[0] + CHAPTER_HOLD) { el.style.opacity = 0; return; }
  el.style.top = `${PORTRAIT ? H * 0.06 : H * 0.05}px`;
  const plaque = el.firstElementChild;
  plaque.children[0].textContent = chapter[1];
  plaque.children[1].textContent = chapter[2];
  const p = window01(t, chapter[0] + 0.05, chapter[0] + CHAPTER_HOLD, 0.2, 0.3);
  el.style.opacity = p;
  el.style.transform = `translateY(${(1 - EASE.out(progress(t, chapter[0], chapter[0] + 0.35))) * -20}px)`;
}
function applySubtitle(t) {
  const cue = SUBTITLES.find(([a, b]) => t >= a && t < b);
  const box = $('subtitle');
  const span = box.firstElementChild;
  box.style.bottom = `${PORTRAIT ? H * 0.135 : H * 0.025}px`;   // portrait: below the HOUR plaque, above the Reels UI zone
  if (!cue) { box.style.opacity = 0; return; }
  if (span.textContent !== cue[2]) span.textContent = cue[2];
  span.className = cue[3] || '';
  span.style.fontSize = px(PORTRAIT ? 50 : 42);
  span.style.padding = `${px(12)} ${px(26)}`;
  span.style.maxWidth = `${W * (PORTRAIT ? 0.84 : 0.7)}px`;
  box.style.opacity = window01(t, cue[0], cue[1], 0.12, 0.15);
}

// ---------------------------------------------------------------- transitions
/** Whip pan: outgoing layer races left with horizontal blur, incoming arrives from the right. */
function whipOffset(t, at, half = 0.2) {
  if (t < at - half || t > at + half) return null;
  const p = progress(t, at - half, at + half);
  return { outgoing: -EASE.in(clamp(p * 2)) * W * 1.1, incoming: (1 - EASE.out(clamp(p * 2 - 1))) * W * 1.1, blur: Math.sin(Math.PI * p) * 60 * UNIT, phase: p };
}
const WHIPS = [
  { at: 40.0, from: 'room-layer', to: 'diagram-layer' },
  { at: 51.5, from: 'diagram-layer', to: 'room-layer' },
  { at: 63.8, from: 'room-layer', to: 'exterior-layer' },
];

const LAYER_TIMES = {
  'title-layer': [[0, 4.8]],
  'room-layer': [[4.3, 26.2], [31.8, 40.2], [51.3, 55.4], [58.9, 64.0]],
  'triptych-layer': [[26.2, 31.8]],
  'diagram-layer': [[39.8, 51.7]],
  'phone-layer': [[55.4, 58.9]],
  'exterior-layer': [[63.6, 68.5]],
  'end-layer': [[68.5, DURATION + 1]],
};

function applyLayers(t) {
  Object.entries(LAYER_TIMES).forEach(([id, ranges]) => {
    const on = ranges.some(([a, b]) => t >= a && t < b);
    show(id, on);
    $(id).style.transform = '';
    $(id).style.filter = '';
  });
  WHIPS.forEach(w => {
    const whip = whipOffset(t, w.at);
    if (!whip) return;
    $('whip-blur-amount').setAttribute('stdDeviation', `${whip.blur} 0`);
    show(w.from, whip.phase < 0.5); show(w.to, whip.phase >= 0.5);
    $(w.from).style.transform = `translateX(${whip.outgoing}px)`;
    $(w.to).style.transform = `translateX(${whip.incoming}px)`;
    $(w.from).style.filter = 'url(#whip-blur)';
    $(w.to).style.filter = 'url(#whip-blur)';
  });
  // iris close on the running developer, Anderson style
  const iris = $('iris');
  if (t > 67.7 && t < 68.5) {
    const p = EASE.in(progress(t, 67.7, 68.45));
    const r = lerp(Math.hypot(W, H) * 0.6, 0, p);
    iris.style.background = `radial-gradient(circle at 50% ${PORTRAIT ? 62 : 62}%, transparent ${r}px, #F4C7C3 ${r + 1}px)`;
    iris.style.opacity = 1;
  } else iris.style.opacity = 0;
}

// ---------------------------------------------------------------- seek
window.seek = function seek(t) {
  applyLayers(t);
  if (t < 4.8) applyTitle(t);
  applyRoom(t);
  if (t >= 26.2 && t < 31.8) applyTriptych(t);
  if (t >= 39.8 && t < 51.7) applyDiagram(t);
  if (t >= 55.4 && t < 58.9) applyPhone(t);
  if (t >= 63.6 && t < 68.5) applyExterior(t);
  if (t >= 68.5) applyEnd(t);
  applyChapter(t);
  applySubtitle(t);
};
window.FILM = { W, H, DURATION, VOICE_CUES };
window.filmReady = Promise.all([document.fonts.ready, ...[...document.images].map(img => img.decode().catch(() => null))]).then(() => { window.seek(0); return true; });
