/* Character rig shared by every figure in the film.
 * One construction for all characters: outlined capsule limbs on real joints (hip → knee, shoulder → elbow),
 * shaded torsos, and three views (front, side, back). Person units: feet at (0,0), ~620 tall, side view faces +x.
 * Poses are plain numbers so every frame stays a pure function of time. */
'use strict';

const INK = '#3A2330';
const OUTLINE = 7;                 // total outline added to a limb's width (3.5 px each side)
const RIG = { hipY: -262, kneeY: -137, ankleY: -14, shoulderY: -466, elbowY: -364, wristY: -266 };
const THIGH = RIG.kneeY - RIG.hipY;
const SHIN = RIG.ankleY - RIG.kneeY;
const LEG_LENGTH = THIGH + SHIN;
const LEG_X = { front: [-22, 22], side: [-5, 5], back: [22, -22] };   // [left, right] hip x per view
const ARM_X = { front: [-80, 80], side: [-6, 4], back: [80, -80] };
const DEG = Math.PI / 180;

function shadeColor(hex, factor) {
  const n = parseInt(hex.slice(1), 16);
  const channel = shift => Math.max(0, Math.min(255, Math.round(((n >> shift) & 255) * factor)));
  return `rgb(${channel(16)},${channel(8)},${channel(0)})`;
}

/** Outlined capsule with a thin highlight on the lit (upper-left) side. */
function limb(x1, y1, x2, y2, width, fill) {
  const nx = -(y2 - y1), ny = x2 - x1, len = Math.hypot(nx, ny) || 1;
  const hx = (nx / len) * width * 0.24, hy = (ny / len) * width * 0.24;
  return `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${INK}" stroke-width="${width + OUTLINE}" stroke-linecap="round"/>
    <line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${fill}" stroke-width="${width}" stroke-linecap="round"/>
    <line x1="${x1 - hx}" y1="${y1 - hy + 6}" x2="${x2 - hx}" y2="${y2 - hy - 6}" stroke="#FFFFFF" stroke-opacity="0.16" stroke-width="${width * 0.2}" stroke-linecap="round"/>`;
}

/** A closed shape with ink outline and a darker shade on its right third (light from the upper left). */
function shaded(id, d, fill, shadeX = 30) {
  return `<clipPath id="${id}-clip"><path d="${d}"/></clipPath>
    <path d="${d}" fill="${fill}" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>
    <rect x="${shadeX}" y="-800" width="300" height="1000" fill="${shadeColor(fill, 0.8)}" clip-path="url(#${id}-clip)"/>
    <path d="${d}" fill="none" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>`;
}

function footSVG(x, view, color) {
  if (view === 'side') {
    return `<path d="M${x - 16},${RIG.ankleY - 10} L${x + 12},${RIG.ankleY - 10} Q${x + 44},${RIG.ankleY - 6} ${x + 44},${RIG.ankleY + 10} L${x - 18},${RIG.ankleY + 10} Z" fill="${color}" stroke="${INK}" stroke-width="3.5" stroke-linejoin="round"/>`;
  }
  return `<ellipse cx="${x}" cy="${RIG.ankleY + 4}" rx="27" ry="12" fill="${color}" stroke="${INK}" stroke-width="3.5"/>`;
}

function legSVG(id, x, view, spec, side, far) {
  const tone = far ? 0.8 : 1;
  const thighColor = shadeColor(spec.thigh, tone), shinColor = shadeColor(spec.shin, tone);
  const foot = shadeColor(spec.feet[side], tone);
  return `<g id="${id}">${limb(x, RIG.hipY, x, RIG.kneeY, spec.legWidth, thighColor)}
    <g id="${id}-knee">${limb(x, RIG.kneeY, x, RIG.ankleY, spec.legWidth * 0.9, shinColor)}${footSVG(x, view, foot)}</g></g>`;
}

function armSVG(id, x, spec, far, prop = '') {
  const tone = far ? 0.8 : 1;
  const sleeve = shadeColor(spec.sleeve, tone);
  const hand = shadeColor(spec.hand, tone);
  return `<g id="${id}">${limb(x, RIG.shoulderY, x, RIG.elbowY, spec.armWidth, sleeve)}
    <g id="${id}-elbow">${limb(x, RIG.elbowY, x, RIG.wristY, spec.armWidth * 0.86, spec.forearm ? shadeColor(spec.forearm, tone) : sleeve)}
      <circle cx="${x}" cy="${RIG.wristY + 8}" r="${spec.handRadius}" fill="${hand}" stroke="${INK}" stroke-width="3.5"/>
      <g id="${id}-prop" transform="translate(${x},${RIG.wristY + 8})">${prop}</g></g></g>`;
}

function contactShadow(width) {
  return `<ellipse cx="0" cy="0" rx="${width}" ry="${width * 0.17}" fill="#3A1020" opacity="0.22" filter="url(#soft-shadow)"/>`;
}

/**
 * Build a character with all three views. spec: { thigh, shin, feet: [l, r], legWidth, sleeve, forearm?, hand,
 * armWidth, handRadius, torso: {front, side, back}, head: {front, side, back}, props: {left, right, sideNear, sideFar},
 * frontOverlay? } — torso/head entries are SVG strings in person units.
 */
function characterSVG(id, spec) {
  const views = ['front', 'side', 'back'].map(view => {
    const pre = `${id}-${view}`;
    const [lx, rx] = LEG_X[view];
    const [alx, arx] = ARM_X[view];
    let parts;
    if (view === 'side') {
      parts = [
        armSVG(`${pre}-armR`, alx, spec, true, spec.props?.sideFar || ''),
        legSVG(`${pre}-legR`, lx, view, spec, 1, true),
        spec.torso.side,
        legSVG(`${pre}-legL`, rx, view, spec, 0, false),
        `<g id="${pre}-head">${spec.head.side}</g>`,
        armSVG(`${pre}-armL`, arx, spec, false, spec.props?.sideNear || ''),
      ];
    } else {
      parts = [
        legSVG(`${pre}-legL`, lx, view, spec, 0, false),
        legSVG(`${pre}-legR`, rx, view, spec, 1, false),
        spec.torso[view],
        armSVG(`${pre}-armL`, alx, spec, false, view === 'front' ? spec.props?.left || '' : ''),
        armSVG(`${pre}-armR`, arx, spec, false, view === 'front' ? spec.props?.right || '' : ''),
        `<g id="${pre}-head">${spec.head[view]}</g>`,
        view === 'front' ? spec.frontOverlay || '' : '',
      ];
    }
    return `<g id="${pre}" style="display:${view === 'front' ? 'inline' : 'none'}"><g id="${pre}-body">${parts.join('')}</g></g>`;
  }).join('');
  return `<g id="${id}">${contactShadow(spec.shadowWidth || 80)}${views}</g>`;
}

// ---------------------------------------------------------------- poses
const STAND = { legL: [0, 0], legR: [0, 0], armL: [0, 0, 1], armR: [0, 0, 1], lean: 0, drop: 0, headTilt: 0 };

/** Walk (amp 1) or run (amp ≈ 1.6) in side view, facing +x. Phase advances with distance so feet don't slide. */
function gaitPose(phase, run = false) {
  const hipAmp = run ? 40 : 24, kneeAmp = run ? 95 : 40;
  const swing = hipAmp * Math.sin(phase);
  const nearKnee = kneeAmp * Math.max(0, Math.cos(phase)) + (run ? 25 : 4);
  const farKnee = kneeAmp * Math.max(0, -Math.cos(phase)) + (run ? 25 : 4);
  const armSwing = (run ? 1.1 : 0.8) * swing;
  const pose = {
    legL: [-swing, nearKnee], legR: [swing, farKnee],
    armL: [armSwing, run ? -88 : -18, 1], armR: [-armSwing, run ? -88 : -18, 1],
    lean: run ? 11 : 2, headTilt: run ? -4 : 0, drop: 0,
  };
  const extent = ([hip, knee]) => THIGH * Math.cos(hip * DEG) + SHIN * Math.cos((hip + knee) * DEG);
  // keep the lowest foot on the ground; running adds a little flight
  pose.drop = LEG_LENGTH - Math.max(extent(pose.legL), extent(pose.legR)) - (run ? 14 * Math.abs(Math.sin(phase * 2)) : 0);
  return pose;
}
const GAIT_CYCLE = { walk: 2 * 2 * LEG_LENGTH * Math.sin(24 * DEG), run: 1.35 * 2 * 2 * LEG_LENGTH * Math.sin(40 * DEG) };
/** Gait phase for a distance walked, in person units. */
const gaitPhase = (distance, run = false) => (distance / (run ? GAIT_CYCLE.run : GAIT_CYCLE.walk)) * 2 * Math.PI;

/** Front view "knees up" jog for figures seen from behind or in front. */
function jogFrontPose(phase) {
  const liftL = Math.max(0, Math.sin(phase)), liftR = Math.max(0, -Math.sin(phase));
  return { legL: [0, 0], legR: [0, 0], liftL, liftR, armL: [150 + 12 * Math.sin(phase), 0, 1], armR: [-150 - 12 * Math.sin(phase), 0, 1], lean: 0, drop: -10 * Math.abs(Math.sin(phase)), headTilt: 0 };
}

function setAttr(id, name, value) { const el = document.getElementById(id); if (el) el.setAttribute(name, value); }

/**
 * Apply a pose to one view of a character. facing: +1 / -1 (side view mirrors; front/back never mirror).
 * Extra: blink (0..1), breathe (0..1 cycle), bow (degrees, front view folds forward).
 */
function applyPose(id, view, pose, { blink = 0, breathe = 0, bow = 0 } = {}) {
  ['front', 'side', 'back'].forEach(v => { const el = document.getElementById(`${id}-${v}`); if (el) el.style.display = v === view ? 'inline' : 'none'; });
  const pre = `${id}-${view}`;
  const [lx, rx] = LEG_X[view];
  const [alx, arx] = ARM_X[view];
  const legs = [['legL', lx, pose.legL, pose.liftL], ['legR', rx, pose.legR, pose.liftR]];
  legs.forEach(([name, x, [hip, knee], lift]) => {
    const liftScale = lift ? 1 - lift * 0.35 : 1;
    setAttr(`${pre}-${name}`, 'transform', `rotate(${hip},${x},${RIG.hipY}) translate(0,${RIG.hipY}) scale(1,${liftScale}) translate(0,${-RIG.hipY})`);
    setAttr(`${pre}-${name}-knee`, 'transform', `rotate(${knee},${x},${RIG.kneeY})`);
  });
  [['armL', alx, pose.armL], ['armR', arx, pose.armR]].forEach(([name, x, [shoulder, elbow, foreshorten]]) => {
    setAttr(`${pre}-${name}`, 'transform', `rotate(${shoulder},${x},${RIG.shoulderY})`);
    setAttr(`${pre}-${name}-elbow`, 'transform', `rotate(${elbow},${x},${RIG.elbowY}) translate(${x},${RIG.elbowY}) scale(1,${foreshorten ?? 1}) translate(${-x},${-RIG.elbowY})`);
  });
  const breath = 1 + Math.sin(breathe * Math.PI * 2) * 0.006;
  const fold = view === 'front' ? `translate(0,${RIG.hipY}) scale(1,${(1 - bow * 0.0085) * breath}) translate(0,${-RIG.hipY})` : `translate(0,${RIG.hipY}) scale(1,${breath}) translate(0,${-RIG.hipY})`;
  setAttr(`${pre}-body`, 'transform', `translate(0,${pose.drop || 0}) rotate(${pose.lean || 0},0,${RIG.hipY}) ${fold}`);
  setAttr(`${pre}-head`, 'transform', `rotate(${pose.headTilt || 0},0,-500) translate(0,${view === 'front' ? bow * 1.4 : 0})`);
  const eyes = document.querySelectorAll(`#${pre}-head .eyes`);
  eyes.forEach(e => e.setAttribute('transform', blink > 0 ? `translate(0,${e.dataset.cy}) scale(1,${1 - blink * 0.88}) translate(0,${-e.dataset.cy})` : ''));
}

// ---------------------------------------------------------------- shared look: heads
function butlerHeadFront(variant) {
  const eyes = `<g class="eyes" data-cy="-548"><ellipse cx="-17" cy="-548" rx="4.8" ry="4.8" fill="#1d1a1a"/><ellipse cx="17" cy="-548" rx="4.8" ry="4.8" fill="#1d1a1a"/></g>`;
  const face = `<rect x="-18" y="-506" width="36" height="34" fill="#DDB08E" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="-48" cy="-542" rx="10" ry="16" fill="#F1CFB2" stroke="${INK}" stroke-width="3.5"/><ellipse cx="48" cy="-542" rx="10" ry="16" fill="#F1CFB2" stroke="${INK}" stroke-width="3.5"/>
    <ellipse cx="0" cy="-545" rx="47" ry="58" fill="#F1CFB2" stroke="${INK}" stroke-width="4"/>
    <path d="M20,-590 Q46,-570 44,-520 Q40,-500 26,-492 Q42,-540 20,-590 Z" fill="#DDB08E" opacity="0.7"/>
    ${eyes}<line x1="-28" y1="-559" x2="-8" y2="-559" stroke="#3a2a22" stroke-width="4.5" stroke-linecap="round"/><line x1="8" y1="-559" x2="28" y2="-559" stroke="#3a2a22" stroke-width="4.5" stroke-linecap="round"/>
    <path d="M0,-546 L-6,-527 L4,-527" fill="none" stroke="#C99A78" stroke-width="3.5" stroke-linecap="round"/>`;
  const hair = {
    part: `<path d="M-48,-560 Q-50,-612 0,-608 Q46,-606 48,-566 L40,-582 Q10,-596 -14,-590 L-22,-600 Q-40,-590 -48,-560 Z" fill="#8E8E90" stroke="${INK}" stroke-width="3.5"/><line x1="-14" y1="-592" x2="-20" y2="-604" stroke="#D8D2CC" stroke-width="3"/>
       <line x1="-10" y1="-513" x2="10" y2="-513" stroke="#8a4f3f" stroke-width="3.5" stroke-linecap="round"/>`,
    walrus: `<path d="M-49,-548 Q-52,-582 -38,-590 L-40,-560 Z M49,-548 Q52,-582 38,-590 L40,-560 Z" fill="#CFCACA" stroke="${INK}" stroke-width="3"/>
       <path d="M-34,-522 Q-18,-532 0,-524 Q18,-532 34,-522 Q30,-500 14,-508 Q0,-500 -14,-508 Q-30,-500 -34,-522 Z" fill="#D8D4D2" stroke="${INK}" stroke-width="3.5"/>`,
    chops: `<path d="M-48,-566 Q-44,-614 0,-612 Q44,-614 48,-566 Q30,-592 0,-590 Q-30,-592 -48,-566 Z" fill="#B8612E" stroke="${INK}" stroke-width="3.5"/>
       <path d="M-47,-560 L-44,-506 Q-34,-496 -26,-516 L-36,-560 Z M47,-560 L44,-506 Q34,-496 26,-516 L36,-560 Z" fill="#B8612E" stroke="${INK}" stroke-width="3"/>
       <line x1="-10" y1="-513" x2="10" y2="-513" stroke="#8a4f3f" stroke-width="3.5" stroke-linecap="round"/>`,
  }[variant];
  return face + hair;
}

function butlerHeadSide(variant) {
  const base = `<rect x="-14" y="-506" width="30" height="34" fill="#DDB08E" stroke="${INK}" stroke-width="3.5"/>
    <path d="M-40,-560 Q-44,-604 4,-604 Q44,-602 46,-556 L50,-548 Q62,-540 50,-530 L48,-516 Q44,-490 10,-488 Q-36,-490 -40,-530 Z" fill="#F1CFB2" stroke="${INK}" stroke-width="4" stroke-linejoin="round"/>
    <ellipse cx="-6" cy="-544" rx="10" ry="15" fill="#E7BE9C" stroke="${INK}" stroke-width="3.5"/>
    <g class="eyes" data-cy="-550"><ellipse cx="28" cy="-550" rx="4.6" ry="4.6" fill="#1d1a1a"/></g>
    <line x1="18" y1="-561" x2="38" y2="-561" stroke="#3a2a22" stroke-width="4.5" stroke-linecap="round"/>`;
  const hair = {
    part: `<path d="M-42,-548 Q-50,-612 6,-608 Q40,-606 44,-576 Q20,-590 -4,-584 L-16,-560 Q-30,-560 -42,-548 Z" fill="#8E8E90" stroke="${INK}" stroke-width="3.5"/>
      <line x1="30" y1="-512" x2="42" y2="-512" stroke="#8a4f3f" stroke-width="3.5" stroke-linecap="round"/>`,
    walrus: `<path d="M-42,-540 Q-46,-580 -30,-588 L-26,-556 Q-34,-552 -42,-540 Z" fill="#CFCACA" stroke="${INK}" stroke-width="3"/>
      <path d="M22,-524 Q40,-532 54,-522 Q52,-504 36,-508 Q26,-504 22,-524 Z" fill="#D8D4D2" stroke="${INK}" stroke-width="3.5"/>`,
    chops: `<path d="M-42,-552 Q-46,-614 6,-612 Q44,-610 46,-574 Q20,-590 -4,-588 L-18,-560 Z" fill="#B8612E" stroke="${INK}" stroke-width="3.5"/>
      <path d="M-14,-560 L-8,-500 Q4,-494 8,-512 L4,-560 Z" fill="#B8612E" stroke="${INK}" stroke-width="3"/>
      <line x1="30" y1="-512" x2="42" y2="-512" stroke="#8a4f3f" stroke-width="3.5" stroke-linecap="round"/>`,
  }[variant];
  return base + hair;
}
