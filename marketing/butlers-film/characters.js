/* The cast, built on rig.js: three butlers, the developer and the friend. Same construction, same outline and
 * shading, same proportions, in front / side / back views. */
'use strict';

const CAST_COLORS = {
  coat: '#4B2A60', trouser: '#2A2230', waist: '#D2A544', shirt: '#FBF6EA', gold: '#C9A23A', goldDark: '#8E6F1E',
  skin: '#F1CFB2', devSkin: '#E7E0C8', hoodie: '#8B9698', hoodieDark: '#6F7A7C', jeans: '#4F6B8E',
  tan: '#C27F4A', shirtCoral: '#EE7E60', shorts: '#E7D5A8',
};

const CARDS = {
  codex: ['ALL DONE,', 'SIR.'],
  claude: ['SIR,', 'TABS OR', 'SPACES?'],
  cursor: ['SHALL I DROP', 'THE PRODUCTION', 'DATABASE?'],
};

function cardSVG(lines, id) {
  const lineHeight = 15;
  const top = -34 - (lines.length - 1) * lineHeight / 2 + 4;
  const text = lines.map((l, i) => `<tspan x="0" y="${top + i * lineHeight}">${l}</tspan>`).join('');
  return `<g id="${id || ''}"><polygon points="-58,0 58,0 52,-72 -52,-72" fill="#E4DCC9" stroke="${INK}" stroke-width="2.5"/><rect x="-56" y="-74" width="112" height="70" fill="#FFFDF6" stroke="${INK}" stroke-width="3"/>
    <text font-family="'Courier New', monospace" font-weight="700" font-size="12.5" fill="#2A2320" text-anchor="middle">${text}</text></g>`;
}

function traySVG() {
  return `<ellipse cx="0" cy="4" rx="98" ry="15" fill="#A9A9AD" stroke="${INK}" stroke-width="3.5"/><ellipse cx="0" cy="0" rx="98" ry="15" fill="#E2E2E6" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="-30" cy="-3" rx="40" ry="4" fill="#FFFFFF" opacity="0.6"/>`;
}

// ---------------------------------------------------------------- butlers
const BUTLER_COAT_FRONT = 'M-78,-486 L78,-486 L88,-250 L66,-166 L26,-246 L-26,-246 L-66,-166 L-88,-250 Z';
const BUTLER_COAT_SIDE = 'M-40,-486 Q0,-496 38,-484 L48,-392 L42,-258 L-34,-258 L-64,-150 L-80,-158 L-54,-270 L-48,-392 Z';
const BUTLER_COAT_BACK = 'M-78,-486 L78,-486 L86,-262 L70,-150 L8,-150 L0,-262 L-8,-150 L-70,-150 L-86,-262 Z';

function butlerSpec(butler, { cardKey = butler.id } = {}) {
  const c = CAST_COLORS;
  const plateWidth = butler.name.length > 8 ? 176 : 128;
  const plateFont = butler.name.length > 8 ? 15 : 20;
  const id = butler.id;
  const front = `${shaded(`${id}-coatF`, BUTLER_COAT_FRONT, c.coat, 34)}
    <path d="M-30,-486 L-42,-300 L0,-278 L42,-300 L30,-486 Z" fill="${c.waist}" stroke="${INK}" stroke-width="3.5"/>
    <circle cx="0" cy="-360" r="4" fill="${c.goldDark}"/><circle cx="0" cy="-330" r="4" fill="${c.goldDark}"/>
    <path d="M-30,-486 L30,-486 L0,-392 Z" fill="${c.shirt}" stroke="${INK}" stroke-width="3"/>
    <path d="M-30,-486 L-58,-486 L-24,-360 Z M30,-486 L58,-486 L24,-360 Z" fill="#361D47" stroke="${INK}" stroke-width="3"/>
    <path d="M0,-480 L-22,-492 L-22,-468 Z M0,-480 L22,-492 L22,-468 Z" fill="${butler.tie}" stroke="${INK}" stroke-width="2.5"/><circle cx="0" cy="-480" r="5" fill="${butler.tie}" stroke="${INK}" stroke-width="2"/>
    <g transform="translate(0,-440)"><rect x="${-plateWidth / 2}" y="-15" width="${plateWidth}" height="30" rx="3" fill="${c.gold}" stroke="${INK}" stroke-width="3"/>
      <text y="7" font-family="Futura, sans-serif" font-weight="700" font-size="${plateFont}" letter-spacing="1.5" text-anchor="middle" fill="#3B2A0C">${butler.name}</text></g>
    <g id="${id}-tray" transform="translate(0,-338)">${traySVG()}<g transform="translate(0,-2)">${cardSVG(CARDS[cardKey], `${id}-card`)}</g></g>`;
  const side = `${shaded(`${id}-coatS`, BUTLER_COAT_SIDE, c.coat, 10)}
    <path d="M26,-472 L46,-392 L42,-300 L30,-300 L20,-470 Z" fill="${c.waist}" stroke="${INK}" stroke-width="3"/>
    <path d="M22,-488 L40,-486 L30,-446 Z" fill="${c.shirt}" stroke="${INK}" stroke-width="2.5"/><path d="M36,-482 L50,-490 L50,-472 Z" fill="${butler.tie}" stroke="${INK}" stroke-width="2"/>`;
  const back = `${shaded(`${id}-coatB`, BUTLER_COAT_BACK, c.coat, 30)}<line x1="0" y1="-470" x2="0" y2="-262" stroke="${INK}" stroke-width="3"/>
    <circle cx="-14" cy="-270" r="5" fill="${c.goldDark}"/><circle cx="14" cy="-270" r="5" fill="${c.goldDark}"/>`;
  const backHead = `<rect x="-18" y="-506" width="36" height="34" fill="#DDB08E" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="-48" cy="-542" rx="10" ry="16" fill="${c.skin}" stroke="${INK}" stroke-width="3.5"/><ellipse cx="48" cy="-542" rx="10" ry="16" fill="${c.skin}" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="0" cy="-545" rx="47" ry="58" fill="${{ part: '#8E8E90', walrus: c.skin, chops: '#B8612E' }[butler.hair]}" stroke="${INK}" stroke-width="4"/>
    ${butler.hair === 'walrus' ? `<path d="M-46,-540 Q0,-520 46,-540 L44,-512 Q0,-496 -44,-512 Z" fill="#CFCACA" stroke="${INK}" stroke-width="3"/>` : ''}`;
  const sideTray = `<g transform="translate(40,-6)">${traySVG().replace(/rx="98"/g, 'rx="62"').replace(/rx="40"/g, 'rx="24"')}<g transform="translate(0,-2) scale(0.22,1)">${cardSVG(CARDS[cardKey], `${id}-card-side`)}</g></g>`;
  return {
    thigh: c.trouser, shin: c.trouser, feet: ['#15111a', '#15111a'], legWidth: 34,
    sleeve: c.coat, hand: '#FFFFFF', armWidth: 30, handRadius: 17, shadowWidth: 86,
    torso: { front, side, back },
    head: { front: butlerHeadFront(butler.hair), side: butlerHeadSide(butler.hair), back: backHead },
    props: { sideNear: sideTray },
  };
}

/** Front-view arms holding the tray with both hands (forearms foreshortened toward camera). */
const TRAY_HOLD = { armL: [12, -80, 0.46], armR: [-12, 80, 0.46] };

// ---------------------------------------------------------------- the developer (look unchanged; now on the rig)
function devSpec(id) {
  const c = CAST_COLORS;
  const hoodieFront = 'M-86,-436 Q-92,-300 -70,-236 L70,-236 Q92,-300 86,-436 Q60,-462 0,-464 Q-60,-462 -86,-436 Z';
  const hoodieSide = 'M-50,-470 Q-8,-500 36,-474 Q58,-380 46,-236 L-46,-236 Q-60,-360 -50,-470 Z';
  const stubble = Array.from({ length: 26 }, (_, i) => `<circle cx="${-34 + seeded(i) * 68}" cy="${-470 + seeded(i + 40) * 22}" r="1.8" fill="#8C8270"/>`).join('');
  const hair = 'M-52,-512 L-58,-548 L-40,-538 L-38,-574 L-18,-552 L-8,-590 L6,-556 L24,-584 L28,-548 L50,-566 L46,-532 L56,-514 Q30,-556 0,-552 Q-30,-556 -52,-512 Z';
  const headFront = `<path d="M-58,-470 Q-62,-420 -40,-440 L40,-440 Q62,-420 58,-470 Q40,-450 0,-452 Q-40,-450 -58,-470 Z" fill="${c.hoodieDark}" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="0" cy="-500" rx="50" ry="58" fill="${c.devSkin}" stroke="${INK}" stroke-width="4"/>
    <path d="${hair}" fill="#4A3526" stroke="${INK}" stroke-width="3.5" stroke-linejoin="round"/>
    <path d="M-34,-484 Q-20,-474 -6,-484 M6,-484 Q20,-474 34,-484" fill="none" stroke="#9B84A8" stroke-width="5" stroke-linecap="round"/>
    <circle cx="-20" cy="-503" r="15" fill="#FFFFFF" stroke="${INK}" stroke-width="3"/><circle cx="20" cy="-503" r="15" fill="#FFFFFF" stroke="${INK}" stroke-width="3"/>
    <g id="${id}-look"><g class="eyes" data-cy="-503"><circle cx="-20" cy="-503" r="3.6" fill="#141414"/><circle cx="20" cy="-503" r="3.6" fill="#141414"/></g></g>
    <line x1="-12" y1="-466" x2="12" y2="-466" stroke="#7B5B4A" stroke-width="4" stroke-linecap="round"/>${stubble}`;
  const headSide = `<path d="M-44,-470 Q-60,-500 -44,-530 L-20,-450 Z" fill="${c.hoodieDark}" stroke="${INK}" stroke-width="3.5"/>
    <path d="M-40,-512 Q-44,-556 2,-558 Q44,-556 48,-516 L56,-506 Q60,-498 50,-492 L48,-478 Q42,-446 6,-444 Q-38,-448 -40,-490 Z" fill="${c.devSkin}" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>
    <ellipse cx="-6" cy="-500" rx="10" ry="15" fill="#DCD3B8" stroke="${INK}" stroke-width="3.5"/>
    <path d="M-44,-506 L-56,-540 L-36,-532 L-34,-574 L-12,-552 L0,-590 L10,-556 L28,-582 L30,-548 L50,-560 L42,-528 Q10,-548 -20,-540 Z" fill="#4A3526" stroke="${INK}" stroke-width="3.5" stroke-linejoin="round"/>
    <circle cx="28" cy="-503" r="14" fill="#FFFFFF" stroke="${INK}" stroke-width="3"/><g class="eyes" data-cy="-503"><circle cx="33" cy="-503" r="3.6" fill="#141414"/></g>
    <path d="M18,-484 Q28,-476 38,-484" fill="none" stroke="#9B84A8" stroke-width="5" stroke-linecap="round"/>`;
  const headBack = `<ellipse cx="-50" cy="-500" rx="9" ry="14" fill="${c.devSkin}" stroke="${INK}" stroke-width="3.5"/><ellipse cx="50" cy="-500" rx="9" ry="14" fill="${c.devSkin}" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="0" cy="-505" rx="50" ry="58" fill="#4A3526" stroke="${INK}" stroke-width="4"/>
    <path d="M-50,-540 L-40,-590 L-20,-556 L0,-596 L18,-558 L40,-588 L50,-540 Z" fill="#4A3526" stroke="${INK}" stroke-width="3.5" stroke-linejoin="round"/>`;
  return {
    thigh: c.jeans, shin: c.jeans, feet: ['#F2A7B8', '#F4D35E'], legWidth: 38,
    sleeve: c.hoodie, hand: c.devSkin, armWidth: 30, handRadius: 15, shadowWidth: 84,
    torso: raiseDevTorso({
      front: `${shaded(`${id}-hoodF`, hoodieFront, c.hoodie, 36)}<path d="M-40,-300 L40,-300 L34,-262 L-34,-262 Z" fill="${c.hoodieDark}" stroke="${INK}" stroke-width="3"/>
        <line x1="-14" y1="-452" x2="-18" y2="-372" stroke="#EEE" stroke-width="4"/><line x1="14" y1="-452" x2="18" y2="-372" stroke="#EEE" stroke-width="4"/>`,
      side: `${shaded(`${id}-hoodS`, hoodieSide, c.hoodie, 14)}<path d="M4,-300 L44,-300 L42,-262 L4,-262 Z" fill="${c.hoodieDark}" stroke="${INK}" stroke-width="3"/>`,
      back: `${shaded(`${id}-hoodB`, hoodieFront, c.hoodie, 36)}<path d="M-50,-464 Q0,-420 50,-464 Q30,-400 0,-396 Q-30,-400 -50,-464 Z" fill="${c.hoodieDark}" stroke="${INK}" stroke-width="3.5"/>`,
    }),
    head: { front: raiseDevHead(headFront), side: raiseDevHead(headSide), back: raiseDevHead(headBack) },
  };
}

const DEV_HEAD_RAISE = 32;
const DEV_TORSO_STRETCH = (494 - 236) / (464 - 236);
function raiseDevHead(svg) { return `<g transform="translate(0,${-DEV_HEAD_RAISE})">${svg}</g>`; }
function raiseDevTorso(torso) {
  const wrap = svg => `<g transform="translate(0,-236) scale(1,${DEV_TORSO_STRETCH.toFixed(3)}) translate(0,236)">${svg}</g>`;
  return { front: wrap(torso.front), side: wrap(torso.side), back: wrap(torso.back) };
}

// ---------------------------------------------------------------- the friend
function friendSpec(id) {
  const c = CAST_COLORS;
  const shirtFront = 'M-80,-470 Q-86,-360 -66,-270 L66,-270 Q86,-360 80,-470 Q56,-492 0,-494 Q-56,-492 -80,-470 Z';
  const shirtSide = 'M-46,-478 Q0,-500 40,-480 Q58,-380 46,-270 L-44,-270 Q-58,-380 -46,-478 Z';
  const flowers = (clip, spread) => `<g clip-path="url(#${clip})">${Array.from({ length: 16 }, (_, i) => `<circle cx="${-spread + seeded(i + 90) * spread * 2}" cy="${-470 + seeded(i + 120) * 200}" r="${7 + seeded(i + 7) * 5}" fill="${i % 3 ? '#FFF1DC' : '#F7CF58'}" stroke="${INK}" stroke-width="1.5"/>`).join('')}</g>`;
  const coconut = `<g transform="translate(0,-8)"><circle r="36" fill="#6E4527" stroke="${INK}" stroke-width="4"/><ellipse cx="-8" cy="-12" rx="12" ry="7" fill="#8B5A34"/>
    <line x1="6" y1="-30" x2="22" y2="-80" stroke="#F0668A" stroke-width="5" stroke-linecap="round"/><path d="M-4,-72 Q20,-102 44,-72 Z" fill="#7FD1C0" stroke="${INK}" stroke-width="2.5"/></g>`;
  const phone = `<g id="${id}-phone" transform="translate(0,-20)"><rect x="-18" y="-34" width="36" height="68" rx="8" fill="#1E1E22" stroke="${INK}" stroke-width="3"/><rect x="-13" y="-28" width="26" height="56" rx="4" fill="#9ED8E6"/></g>`;
  const headFront = `<ellipse cx="-47" cy="-536" rx="9" ry="14" fill="${c.tan}" stroke="${INK}" stroke-width="3.5"/><ellipse cx="47" cy="-536" rx="9" ry="14" fill="${c.tan}" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="0" cy="-535" rx="48" ry="56" fill="${c.tan}" stroke="${INK}" stroke-width="4"/>
    <path d="M-50,-548 Q-54,-604 0,-602 Q54,-604 50,-548 Q40,-580 0,-582 Q-40,-580 -50,-548 Z" fill="#EBCB7A" stroke="${INK}" stroke-width="3.5"/>
    <path d="M-44,-586 L44,-586 L40,-568 L6,-568 L0,-576 L-6,-568 L-40,-568 Z" fill="#1d1d22" stroke="${INK}" stroke-width="2.5"/>
    <g class="eyes" data-cy="-538"><ellipse cx="-17" cy="-538" rx="4.8" ry="4.8" fill="#1d1a1a"/><ellipse cx="17" cy="-538" rx="4.8" ry="4.8" fill="#1d1a1a"/></g>
    <path d="M-14,-506 Q0,-498 14,-506" fill="none" stroke="#6E3B22" stroke-width="4" stroke-linecap="round"/>`;
  const headSide = `<path d="M-40,-552 Q-44,-596 4,-596 Q44,-594 46,-550 L52,-540 Q62,-532 50,-524 L48,-510 Q42,-482 8,-480 Q-36,-482 -40,-520 Z" fill="${c.tan}" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>
    <ellipse cx="-6" cy="-536" rx="9" ry="14" fill="#B06F3E" stroke="${INK}" stroke-width="3.5"/>
    <path d="M-42,-544 Q-50,-606 6,-604 Q44,-602 46,-570 Q20,-582 -6,-578 L-18,-552 Z" fill="#EBCB7A" stroke="${INK}" stroke-width="3.5"/>
    <path d="M0,-588 L44,-588 L40,-572 L4,-574 Z" fill="#1d1d22" stroke="${INK}" stroke-width="2.5"/>
    <g class="eyes" data-cy="-540"><ellipse cx="28" cy="-540" rx="4.6" ry="4.6" fill="#1d1a1a"/></g>
    <path d="M30,-506 Q38,-502 44,-508" fill="none" stroke="#6E3B22" stroke-width="4" stroke-linecap="round"/>`;
  return {
    thigh: c.shorts, shin: c.tan, feet: ['#7B4B2A', '#7B4B2A'], legWidth: 32,
    sleeve: c.shirtCoral, forearm: c.tan, hand: c.tan, armWidth: 28, handRadius: 14, shadowWidth: 80,
    torso: {
      front: `${shaded(`${id}-shirtF`, shirtFront, c.shirtCoral, 34)}${flowers(`${id}-shirtF-clip`, 70)}<path d="M-22,-492 L0,-450 L22,-492" fill="${c.tan}" stroke="${INK}" stroke-width="3"/>`,
      side: `${shaded(`${id}-shirtS`, shirtSide, c.shirtCoral, 14)}${flowers(`${id}-shirtS-clip`, 44)}`,
      back: `${shaded(`${id}-shirtB`, shirtFront, c.shirtCoral, 34)}${flowers(`${id}-shirtB-clip`, 70)}`,
    },
    head: { front: headFront, side: headSide, back: headFront },
    props: { left: coconut, right: phone, sideFar: coconut },
  };
}

/** Screen position (person units) of a hand in a pose, for anchoring flying objects to it. */
function handPosition(view, side, pose) {
  const x = ARM_X[view][side === 'L' ? 0 : 1];
  const [shoulder, elbow, foreshorten] = pose[side === 'L' ? 'armL' : 'armR'];
  const a1 = shoulder * DEG, a2 = (shoulder + elbow) * DEG;
  const upper = RIG.elbowY - RIG.shoulderY, fore = (RIG.wristY + 8 - RIG.elbowY) * (foreshorten ?? 1);
  return [x - Math.sin(a1) * upper - Math.sin(a2) * fore, RIG.shoulderY + Math.cos(a1) * upper + Math.cos(a2) * fore + (pose.drop || 0)];
}
