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

const BUTLERS = [
  { id: 'codex', name: 'CODEX', x: 780, tie: '#111111', hair: 'part' },
  { id: 'claude', name: 'CLAUDE CODE', x: 1200, tie: '#D97757', hair: 'walrus' },
  { id: 'cursor', name: 'CURSOR', x: 1620, tie: '#5A5F66', hair: 'chops' },
];

// ---------------------------------------------------------------- build DOM
const stage = document.getElementById('stage');
stage.style.width = W + 'px';
stage.style.height = H + 'px';

const px = n => `${Math.round(n * UNIT)}px`;

stage.innerHTML = `
<div class="layer" id="room-layer"><svg width="${W}" height="${H}" viewBox="0 0 ${W} ${H}">
  <defs>${guestDoorClipSVG()}${serviceDoorClipSVG()}</defs>
  <g id="world">${roomShellSVG()}
    <g id="door-leaf" clip-path="url(#guest-door-clip)"></g>
    <g id="actors">
      ${BUTLERS.map(b => `<g id="clip-${b.id}"><g id="pos-${b.id}">${characterSVG(b.id, butlerSpec(b))}</g></g>`).join('')}
      <g id="pos-chair"><g id="chair-body"></g></g>
      <g id="clip-dev"><g id="pos-dev">${characterSVG('dev', devSpec('dev'))}</g></g>
      <g id="pos-desk">${deskSVG()}</g>
      <g id="clip-friend"><g id="pos-friend">${characterSVG('friend', friendSpec('friend'))}</g></g>
    </g>
    <g id="flying-cards">${['claude', 'cursor'].map(id => `<g id="fly-${id}">${cardSVG(CARDS[id])}</g>`).join('')}</g>
    <g id="foreground" filter="url(#fg-blur)">${foregroundSVG()}</g>
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
      ${characterSVG('tri-' + butler.id, butlerSpec({ ...butler, id: 'tri-' + butler.id }, { cardKey: butler.id }))}
    </svg></div>`;
}
$('triptych-layer').innerHTML = `<div style="position:absolute;inset:0;background:#F7EEDC;display:flex;flex-direction:${PORTRAIT ? 'column' : 'row'}">${BUTLERS.map(triptychPanel).join('')}</div>`;
BUTLERS.forEach(b => applyPose('tri-' + b.id, 'front', { ...STAND, ...TRAY_HOLD }));

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
    <g id="inspect-butler" transform="translate(-900,${L.butlerY}) scale(${L.butlerScale})">${characterSVG('figc', butlerSpec({ id: 'figc', name: 'CODEX', tie: '#111', hair: 'part' }, { cardKey: 'codex' }))}</g>
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
    <g id="runner">${characterSVG('run', devSpec('run'))}</g>`;
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


// ---------------------------------------------------------------- room scene
/** [time, x, y, landscape width, portrait width, portrait x offset, ease?] in room screen space. */
const ROOM_CAMERA_KEYS = () => [
  [4.3, 1200, 1240, 2900, 1350, 0],
  [12.0, 1200, 1270, 2500, 1250, 0],
  [14.8, 1200, 1080, 1550, 1050, 0],
  [15.78, 1200, 1075, 1520, 1040, 0, 'linear'],
  [15.96, 816, 955, 430, 390, 0, 'snap'], [17.18, 816, 950, 410, 370, 0, 'linear'],
  [17.36, 1200, 955, 430, 390, 0, 'snap'], [18.58, 1200, 950, 410, 370, 0, 'linear'],
  [18.76, 1584, 955, 430, 390, 0, 'snap'], [19.9, 1584, 950, 410, 370, 0, 'linear'],
  [20.08, 1200, 1150, 1900, 1200, 0, 'snap'], [26.2, 1200, 1170, 1700, 1120, 0, 'linear'],
  [31.8, 1200, 1240, 2900, 1350, 260], [33.6, 1200, 1240, 2860, 1350, 120, 'inout'], [35.9, 1180, 1250, 2800, 1350, -260, 'inout'],
  [39.8, 840, 1330, 1750, 1180, 0],
  [51.5, 1080, 1180, 2500, 1300, -120], [54.7, 1000, 1200, 2300, 1250, -170, 'linear'],
  [55.4, PHONE_ANCHOR[0], PHONE_ANCHOR[1], 150, 150, 0, 'in'],
  [58.9, 1200, 1240, 2900, 1350, 0], [61.6, 1220, 1250, 2850, 1340, 60, 'linear'], [62.4, 1300, 1250, 2800, 1330, 250],
  [63.9, 1700, 1250, 2800, 1330, 450, 'in'],
];
let ROOM_CAMERA = null;

const WALK_SPEED = 330;       // world units per second
const TURN_SECONDS = 0.12;

/** Place an actor standing on the floor (or dais) at world X, Z. */
function placeActor(id, X, Z, floorY, facing = 1) {
  const [x, y] = project(X, floorY, Z);
  const s = personScale(Z);
  setTransform(id, `translate(${x.toFixed(2)},${y.toFixed(2)}) scale(${(s * facing).toFixed(4)},${s.toFixed(4)})`);
  return { x, y, s };
}

/** Walk along waypoints [[t, X, Z], ...]; returns position, heading and distance walked (for gait phase). */
function followPath(t, points) {
  if (t <= points[0][0]) return { X: points[0][1], Z: points[0][2], moving: false, dx: 0, distance: 0 };
  let distance = 0;
  for (let i = 1; i < points.length; i++) {
    const [t0, x0, z0] = points[i - 1], [t1, x1, z1] = points[i];
    const segment = Math.hypot(x1 - x0, z1 - z0);
    if (t < t1) {
      const p = (t - t0) / (t1 - t0);
      return { X: lerp(x0, x1, p), Z: lerp(z0, z1, p), moving: true, dx: x1 - x0, dz: z1 - z0, distance: distance + segment * p };
    }
    distance += segment;
  }
  const last = points[points.length - 1];
  return { X: last[1], Z: last[2], moving: false, dx: 0, distance };
}

// butler exits: walk along the dais to a service door, turn, walk through it
const BUTLER_EXITS = {
  codex: { start: 51.3, door: 'left' },
  claude: { start: 59.95, door: 'left' },
  cursor: { start: 59.95, door: 'right' },
};
/** Waypoints [[X, Z], ...] walked at a constant speed from `start`, as [[t, X, Z], ...]. */
function timedPath(start, speed, points) {
  let t = start;
  return points.map((p, i) => { if (i > 0) t += Math.hypot(p[0] - points[i - 1][0], p[1] - points[i - 1][1]) / speed; return [t, p[0], p[1]]; });
}
function butlerExitPath(id) {
  const exit = BUTLER_EXITS[id], X0 = MARKS.butlerX[id], X1 = SERVICE_DOORS[exit.door];
  const alongSeconds = Math.abs(X1 - X0) / WALK_SPEED;
  const t0 = exit.start + TURN_SECONDS, t1 = t0 + alongSeconds, t2 = t1 + TURN_SECONDS, t3 = t2 + 200 / WALK_SPEED;
  return { t0, t1, t2, t3, points: [[t0, X0, MARKS.butlerZ], [t1, X1, MARKS.butlerZ], [t2, X1, MARKS.butlerZ], [t3, X1, MARKS.butlerZ + 200]] };
}

function butlerFrame(b, t) {
  const exit = BUTLER_EXITS[b.id];
  const blinkAt = { codex: [13.4, 19.7, 37.1], claude: [21.7, 28.0, 45.0], cursor: [24.1, 30.4, 36.2] }[b.id];
  const blink = blinkAt.some(at => Math.abs(t - at) < 0.08) ? 1 : 0;
  const breathe = t / 3.1 + b.x;
  const bow = b.id !== 'codex' && t > 58.9 ? Math.sin(Math.PI * progress(t, 59.0, 59.85)) * 26 : 0;
  if (t < exit.start) return { X: MARKS.butlerX[b.id], Z: MARKS.butlerZ, view: 'front', pose: { ...STAND, ...TRAY_HOLD }, extra: { blink, breathe, bow } };
  const path = butlerExitPath(b.id);
  const pos = followPath(t, path.points);
  if (t < path.t2) {
    const facing = Math.sign(SERVICE_DOORS[exit.door] - MARKS.butlerX[b.id]) || 1;
    const pose = gaitPose(gaitPhase(pos.distance / PERSON_WORLD));
    pose.armL = [-14, -76, 1];     // near hand carries the tray forward
    return { X: pos.X, Z: pos.Z, view: 'side', facing, pose, extra: { breathe } };
  }
  const phase = gaitPhase(pos.distance / PERSON_WORLD);
  return { X: pos.X, Z: pos.Z, view: 'back', pose: { ...STAND, liftL: 0.35 * Math.max(0, Math.sin(phase)), liftR: 0.35 * Math.max(0, -Math.sin(phase)) }, extra: {}, clip: pos.Z > MARKS.butlerZ + 90 };
}

function applyButlers(t) {
  const depths = {};
  BUTLERS.forEach(b => {
    const frame = butlerFrame(b, t);
    placeActor(`pos-${b.id}`, frame.X, frame.Z, DAIS.top, frame.view === 'side' ? frame.facing : 1);
    applyPose(b.id, frame.view, frame.pose, frame.extra);
    $(`clip-${b.id}`).setAttribute('clip-path', frame.clip ? 'url(#service-door-clip)' : '');
    show(`clip-${b.id}`, frame.Z < MARKS.butlerZ + 199);
    depths[`clip-${b.id}`] = frame.Z;
  });
  // question cards leave the trays when they fly; Codex's card went back to work with him
  ['claude', 'cursor'].forEach(id => {
    const gone = t > CARD_FLIGHTS[id].start;
    setOpacity(`${id}-card`, gone ? 0 : 1); setOpacity(`${id}-card-side`, gone ? 0 : 1);
  });
  // service doors swing open while a butler passes through
  ['left', 'right'].forEach(side => {
    const users = BUTLERS.filter(b => BUTLER_EXITS[b.id].door === side).map(b => butlerExitPath(b.id));
    const open = Math.max(0, ...users.map(p => Math.min(EASE.out(progress(t, p.t1 - 0.25, p.t1 + 0.05)), 1 - EASE.inout(progress(t, p.t3 + 0.1, p.t3 + 0.45)))));
    const cx = SERVICE_DOORS[side], hw = SERVICE_DOORS.halfWidth, hinge = side === 'left' ? cx - hw : cx + hw, free = side === 'left' ? cx + hw : cx - hw;
    const edge = lerp(free, hinge, open * 0.92);
    $(`service-leaf-${side}`).setAttribute('points', projPoly([[hinge, SERVICE_DOORS.topY, ROOM.back], [edge, SERVICE_DOORS.topY - open * 8, ROOM.back], [edge, DAIS.top + open * 8, ROOM.back], [hinge, DAIS.top, ROOM.back]]));
  });
  return depths;
}

// ---------------------------------------------------------------- friend
const FRIEND_PATH = timedPath(32.2, 400, [[800, 800], [600, 660], [400, 460], [-360, 440], [MARKS.friend.X, MARKS.friend.Z]]);
const FRIEND_REST = { armL: [10, -46, 1], armR: [-4, -10, 1] };
const FRIEND_RAISED = { armL: [10, -46, 1], armR: [-160, -22, 1] };
function friendFrame(t) {
  const pos = followPath(t, FRIEND_PATH);
  const blink = [36.9, 44.0, 57.0].some(at => Math.abs(t - at) < 0.08) ? 1 : 0;
  if (pos.moving) {
    const pose = gaitPose(gaitPhase(pos.distance / PERSON_WORLD));
    return { X: pos.X, Z: pos.Z, view: 'side', facing: -1, pose, extra: {}, clip: pos.X > ROOM.wallX - 10 };
  }
  const raise = t < 58.9 ? Math.max(EASE.inout(progress(t, 39.1, 39.7)), t > 51 ? 1 : 0) : 0;
  const pose = { ...STAND, armL: FRIEND_REST.armL, armR: FRIEND_REST.armR.map((v, i) => lerp(v, FRIEND_RAISED.armR[i], raise)) };
  return { X: pos.X, Z: pos.Z, view: 'front', facing: 1, pose, extra: { blink, breathe: t / 3.4 }, clip: false };
}
/** Screen position of the friend's raised phone (anchor for the flying cards and the zoom into the phone). */
function friendPhoneScreen(t) {
  const frame = friendFrame(t);
  const [x, y] = project(frame.X, ROOM.floor, frame.Z), s = personScale(frame.Z);
  const [hx, hy] = handPosition('front', 'R', frame.pose);
  return [x + hx * s, y + (hy - 20) * s];
}
const PHONE_ANCHOR = friendPhoneScreen(54.5);

function applyFriend(t) {
  const frame = friendFrame(t);
  const visible = t >= 32.2 && !(t > 40.2 && t < 51.3);
  show('clip-friend', visible);
  placeActor('pos-friend', frame.X, frame.Z, ROOM.floor, frame.view === 'side' ? frame.facing : 1);
  applyPose('friend', frame.view, frame.pose, frame.extra);
  $('clip-friend').setAttribute('clip-path', frame.clip ? 'url(#guest-door-clip)' : '');
  setOpacity('friend-phone', t > 38.9 && t < 58.9 ? 1 : 0);
  return { 'clip-friend': frame.Z };
}

// ---------------------------------------------------------------- developer
const DEV_SEAT_DROP = 112;                // seated hip height below standing, person units
const DEV_WALK = [[61.62, MARKS.dev.X, MARKS.dev.Z], [62.35, 400, 660]];
const DEV_RUN = [[62.35, 400, 660], [63.35, 830, 800]];
function devFrame(t) {
  const look = (t > 35.2 && t < 40) || (t > 51.4 && t < 58.9) ? 6 * EASE.inout(progress(t, 35.2, 35.5)) || 6 : 0;
  const blink = [10.2, 18.9, 29.5, 47.0].some(at => Math.abs(t - at) < 0.08) ? 1 : 0;
  if (t < 61.62) {
    const stand = EASE.back(progress(t, 60.9, 61.45));
    const typing = t < 31.8 || (t > 58.9 && t < 60.9) ? Math.sin(t * 36) * 5 : 0;
    const pose = { ...STAND, armL: [10, -72 + typing, 0.5], armR: [-10, 72 - typing, 0.5], drop: DEV_SEAT_DROP * (1 - stand) };
    return { X: MARKS.dev.X, Z: MARKS.dev.Z, view: 'front', facing: 1, pose, extra: { blink, breathe: t / 2.6 }, look, eyesUp: t > 60.9 && t < 61.5 };
  }
  const running = t >= 62.35;
  const pos = followPath(t, running ? DEV_RUN : DEV_WALK);
  const walked = running ? Math.hypot(400 - MARKS.dev.X, 660 - MARKS.dev.Z) + pos.distance : pos.distance;
  const pose = gaitPose(gaitPhase(walked / PERSON_WORLD, running), running);
  return { X: pos.X, Z: pos.Z, view: 'side', facing: 1, pose, extra: {}, look: 0, clip: pos.X > ROOM.wallX - 10 };
}
function applyDeveloper(t) {
  const frame = devFrame(t);
  placeActor('pos-dev', frame.X, frame.Z, ROOM.floor, 1);
  applyPose('dev', frame.view, frame.pose, frame.extra);
  setTransform('dev-look', `translate(${frame.look},${frame.eyesUp ? -3 : 0})`);
  $('clip-dev').setAttribute('clip-path', frame.clip ? 'url(#guest-door-clip)' : '');
  show('clip-dev', frame.X < ROOM.wallX + 140);
  return { 'clip-dev': frame.Z };
}

function applyChair(t) {
  const since = Math.max(0, t - 61.75);
  const spin = (9 / 0.55) * (1 - Math.exp(-0.55 * since));
  placeActor('pos-chair', MARKS.chair.X, MARKS.chair.Z, ROOM.floor);
  $('chair-body').innerHTML = chairSVG(spin);
  return { 'pos-chair': MARKS.chair.Z };
}

const CARD_FLIGHTS = { claude: { start: 52.0, end: 54.1 }, cursor: { start: 52.5, end: 54.6 } };
function applyFlyingCards(t) {
  const target = friendPhoneScreen(Math.min(Math.max(t, 51.5), 58.8));
  ['claude', 'cursor'].forEach(id => {
    const flight = CARD_FLIGHTS[id];
    const p = progress(t, flight.start, flight.end);
    const flying = t > flight.start && t < flight.end + 0.05 && t < 55.4;
    show('fly-' + id, flying);
    if (!flying) return;
    const [bx, by] = project(MARKS.butlerX[id], DAIS.top, MARKS.butlerZ), s = personScale(MARKS.butlerZ);
    const from = [bx, by - 340 * s];
    const e = EASE.inout(p);
    const control = [lerp(from[0], target[0], 0.35), Math.min(from[1], target[1]) - 480];
    const x = (1 - e) * (1 - e) * from[0] + 2 * (1 - e) * e * control[0] + e * e * target[0];
    const y = (1 - e) * (1 - e) * from[1] + 2 * (1 - e) * e * control[1] + e * e * target[1];
    const scale = s * (1 + Math.sin(Math.PI * e) * 4.2) * lerp(1, 0.28, EASE.in(p));
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
  // guest door swings outward on its near hinge; sunlight pours in
  const open = EASE.snap(progress(t, 31.8, 32.35));
  $('door-leaf').innerHTML = guestDoorLeafSVG(-open * 100);
  $('door-light').setAttribute('points', doorLightPoints());
  $('door-light').setAttribute('opacity', open * 0.5);
}

/** Painter's order: far actors first. Re-append only when the order changes. */
let lastActorOrder = '';
function sortActors(depths) {
  const order = Object.entries(depths).sort((a, b) => b[1] - a[1]).map(([id]) => id);
  const key = order.join('|');
  if (key === lastActorOrder) return;
  const container = $('actors');
  order.forEach(id => container.appendChild($(id)));
  lastActorOrder = key;
}

function applyRoom(t) {
  ROOM_CAMERA = ROOM_CAMERA || ROOM_CAMERA_KEYS();
  const [x, y, wL, wP, dxP] = keyed(t, ROOM_CAMERA);
  const camX = x + (PORTRAIT ? dxP : 0);
  const zoom = W / (PORTRAIT ? wP : wL);
  setTransform('world', `translate(${W / 2},${H / 2}) scale(${zoom}) translate(${-camX},${-y})`);
  // foreground sits nearer the lens than the set, so it slides further as the camera moves
  setTransform('foreground', `translate(${-(camX - 1200) * 0.35},${-(y - 1240) * 0.25})`);
  applyClockAndProps(t);
  const depths = { ...applyButlers(t), ...applyChair(t), ...applyDeveloper(t), ...applyFriend(t), 'pos-desk': DESK_BOX.backZ };
  sortActors(depths);
  applyFlyingCards(t);
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
    applyPose('tri-' + b.id, 'front', { ...STAND, ...TRAY_HOLD }, { blink: blink ? 1 : 0, breathe: t / 3.1 + i });
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
  const bob = 0, typing = 0;
  setTransform('inspect-butler', `translate(${marchX},${FIG_LAYOUT.butlerY + bob + typing}) scale(${FIG_LAYOUT.butlerScale})`);
  if (march < 1) {
    const pose = march > 0 ? gaitPose(gaitPhase((marchX + 900) / FIG_LAYOUT.butlerScale)) : { ...STAND };
    pose.armL = [-14, -76, 1];
    applyPose('figc', 'side', pose);
  } else {
    applyPose('figc', 'side', { ...STAND, armL: [-40, -50 + Math.sin(t * 40) * 6, 1], armR: [-30, -60 - Math.sin(t * 40) * 6, 1] });
  }
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
  const x = W / 2 + Math.sin(p * 3) * 20;
  const y = lerp(H * 1.02, H * 0.62, run);
  setTransform('runner', `translate(${x},${y}) scale(${sc})`);
  applyPose('run', 'back', jogFrontPose((t - 63.8) * 15));
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
