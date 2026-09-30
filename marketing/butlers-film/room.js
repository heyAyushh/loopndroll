/* The drawing room as a real 3D set seen through one pinhole camera.
 * World units: X right, Y down, Z away from the camera. The back wall sits at Z = 1000 (drawn 1:1), floor at Y = 600.
 * Every prop and character is placed with project(), so sizes, doors and occlusion stay logically consistent. */
'use strict';

const CAMERA = { centerX: 1200, horizonY: 900, focal: 1000 };
const ROOM = { floor: 600, ceiling: -640, wallX: 700, back: 1000 };
const DAIS = { top: 400, stepY: 500, frontZ: 800, stepZ: 850 };
const PERSON_WORLD = 0.786;                 // person units → world units at the back wall
const GUEST_DOOR = { nearZ: 700, farZ: 880, topY: -110 };
const SERVICE_DOORS = { left: -540, right: 540, halfWidth: 82, topY: 20 };
const DESK_BOX = { halfWidth: 330, frontZ: 490, backZ: 570, topY: 403 };   // ~250 person units tall: a real desk
const MARKS = {
  butlerZ: 860, butlerX: { codex: -330, claude: 0, cursor: 330 },
  dev: { X: 0, Z: 600 }, chair: { X: 0, Z: 650 }, friend: { X: -430, Z: 560 },
};

const depthScale = Z => CAMERA.focal / Z;
function project(X, Y, Z) { const s = depthScale(Z); return [CAMERA.centerX + X * s, CAMERA.horizonY + Y * s]; }
const personScale = Z => PERSON_WORLD * depthScale(Z);
const poly = points => points.map(p => p.map(v => v.toFixed(1)).join(',')).join(' ');
const projPoly = points3d => poly(points3d.map(p => project(...p)));

const ROOM_COLORS = {
  wall: '#EBA9A6', wallStripe: '#E49A98', wainscot: '#8C2E3B', panel: '#A03C49', cornice: '#F4E3C8', ceiling: '#EED6BD',
  sideWall: '#D98F8D', sideWainscot: '#76262F', floorA: '#F3E3C3', floorB: '#C9A06B', gold: '#C9A23A', goldDark: '#8E6F1E',
  wood: '#6B3A2A', woodLight: '#85503A', carpet: '#8C2E3B', ink: '#2B3F5C',
};

// ---------------------------------------------------------------- static backdrop
function roomShellSVG() {
  const c = ROOM_COLORS, parts = [];
  const { wallX: X, floor: F, ceiling: Cy, back: B } = ROOM;
  parts.push(`<polygon points="${projPoly([[-X * 4, Cy, 250], [X * 4, Cy, 250], [X, Cy, B], [-X, Cy, B]])}" fill="${c.ceiling}"/>`);
  for (let z = 400; z < B; z += 120) parts.push(`<polygon points="${projPoly([[-X, Cy, z], [X, Cy, z], [X, Cy, z + 8], [-X, Cy, z + 8]])}" fill="#E2C6A8"/>`);
  [-1, 1].forEach(side => {
    parts.push(`<polygon points="${projPoly([[side * X, Cy, 250], [side * X, Cy, B], [side * X, F, B], [side * X, F, 250]])}" fill="${c.sideWall}"/>`);
    parts.push(`<polygon points="${projPoly([[side * X, 330, 250], [side * X, 330, B], [side * X, F, B], [side * X, F, 250]])}" fill="${c.sideWainscot}"/>`);
    parts.push(`<polygon points="${projPoly([[side * X, 318, 250], [side * X, 318, B], [side * X, 334, B], [side * X, 334, 250]])}" fill="${c.cornice}"/>`);
  });
  // back wall with stripes, cornice and wainscot above the dais
  parts.push(`<rect x="500" y="260" width="1400" height="1240" fill="${c.wall}"/>`);
  for (let x = 535; x < 1900; x += 70) parts.push(`<rect x="${x}" y="306" width="14" height="800" fill="${c.wallStripe}"/>`);
  parts.push(`<rect x="500" y="260" width="1400" height="46" fill="${c.cornice}"/><rect x="500" y="306" width="1400" height="10" fill="#DCC3A0"/>`);
  parts.push(`<rect x="500" y="1100" width="1400" height="200" fill="${c.wainscot}"/><rect x="500" y="1092" width="1400" height="14" fill="${c.cornice}"/>`);
  for (let i = 0; i < 7; i++) parts.push(`<rect x="${530 + i * 196}" y="1130" width="170" height="146" fill="none" stroke="${c.panel}" stroke-width="7"/>`);
  parts.push(framedPainting(620, 420, 260, 330, duckPainting()));
  parts.push(framedPainting(1520, 420, 260, 330, `<rect width="260" height="330" fill="#E7EFE6"/><text x="130" y="232" font-family="Didot, serif" font-size="230" text-anchor="middle" fill="#2B3F5C">;</text>`));
  // wall clock with pendulum case (hidden behind Claude Code's head below the face)
  parts.push(`<rect x="1150" y="560" width="100" height="300" rx="12" fill="${c.wood}" stroke="${c.goldDark}" stroke-width="6"/><rect x="1168" y="600" width="64" height="240" rx="8" fill="#3E2119"/>`);
  parts.push(`<g id="pendulum"><line x1="1200" y1="600" x2="1200" y2="790" stroke="${c.gold}" stroke-width="6"/><circle cx="1200" cy="800" r="22" fill="${c.gold}" stroke="${c.goldDark}" stroke-width="4"/></g>`);
  parts.push(`<circle cx="1200" cy="470" r="128" fill="${c.gold}"/><circle cx="1200" cy="470" r="112" fill="#FBF3E1" stroke="${c.goldDark}" stroke-width="4"/>`);
  for (let i = 0; i < 12; i++) { const a = i / 12 * Math.PI * 2; parts.push(`<line x1="${1200 + Math.sin(a) * 92}" y1="${470 - Math.cos(a) * 92}" x2="${1200 + Math.sin(a) * 104}" y2="${470 - Math.cos(a) * 104}" stroke="${c.ink}" stroke-width="${i % 3 ? 4 : 8}"/>`); }
  parts.push(`<line id="clock-hour" x1="1200" y1="470" x2="1200" y2="410" stroke="${c.ink}" stroke-width="10" stroke-linecap="round"/>`);
  parts.push(`<line id="clock-minute" x1="1200" y1="470" x2="1200" y2="382" stroke="${c.ink}" stroke-width="6" stroke-linecap="round"/><circle cx="1200" cy="470" r="9" fill="${c.ink}"/>`);
  // service doors in the back wall, at dais height
  ['left', 'right'].forEach(side => {
    const cx = SERVICE_DOORS[side], hw = SERVICE_DOORS.halfWidth;
    const frame = [[cx - hw - 14, SERVICE_DOORS.topY - 14, B], [cx + hw + 14, SERVICE_DOORS.topY - 14, B], [cx + hw + 14, DAIS.top, B], [cx - hw - 14, DAIS.top, B]];
    const opening = [[cx - hw, SERVICE_DOORS.topY, B], [cx + hw, SERVICE_DOORS.topY, B], [cx + hw, DAIS.top, B], [cx - hw, DAIS.top, B]];
    parts.push(`<polygon points="${projPoly(frame)}" fill="${c.cornice}" stroke="#CDB28E" stroke-width="3"/>`);
    parts.push(`<polygon points="${projPoly(opening)}" fill="#2A1620"/>`);
    parts.push(`<polygon id="service-leaf-${side}" points="${projPoly(opening)}" fill="#7A3B2C" stroke="${c.ink}" stroke-width="3"/>`);
  });
  // floor tiles in true perspective
  parts.push(`<polygon points="${projPoly([[-X * 4, F, 250], [X * 4, F, 250], [X, F, B], [-X, F, B]])}" fill="${c.floorA}"/>`);
  const rowsZ = [800, 690, 600, 520, 452, 392, 340, 295, 255];
  for (let r = 0; r < rowsZ.length - 1; r++) {
    for (let col = -20; col < 20; col++) {
      if ((r + col) % 2 === 0) continue;
      parts.push(`<polygon points="${projPoly([[col * 140, F, rowsZ[r]], [(col + 1) * 140, F, rowsZ[r]], [(col + 1) * 140, F, rowsZ[r + 1]], [col * 140, F, rowsZ[r + 1]]])}" fill="${c.floorB}"/>`);
    }
  }
  parts.push(daisSVG());
  parts.push(rugSVG());
  parts.push(guestDoorFrameSVG());
  return parts.join('');
}

function daisSVG() {
  const c = ROOM_COLORS, X = ROOM.wallX;
  return `<polygon points="${projPoly([[-X, DAIS.top, DAIS.stepZ], [X, DAIS.top, DAIS.stepZ], [X, DAIS.top, ROOM.back], [-X, DAIS.top, ROOM.back]])}" fill="${c.carpet}"/>
    <polygon points="${projPoly([[-X, DAIS.top, DAIS.stepZ], [X, DAIS.top, DAIS.stepZ], [X, DAIS.stepY, DAIS.stepZ], [-X, DAIS.stepY, DAIS.stepZ]])}" fill="${c.wood}"/>
    <polygon points="${projPoly([[-X, DAIS.top - 6, DAIS.stepZ], [X, DAIS.top - 6, DAIS.stepZ], [X, DAIS.top + 8, DAIS.stepZ], [-X, DAIS.top + 8, DAIS.stepZ]])}" fill="${c.gold}"/>
    <polygon points="${projPoly([[-X, DAIS.stepY, DAIS.frontZ], [X, DAIS.stepY, DAIS.frontZ], [X, DAIS.stepY, DAIS.stepZ], [-X, DAIS.stepY, DAIS.stepZ]])}" fill="${c.carpet}"/>
    <polygon points="${projPoly([[-X, DAIS.stepY, DAIS.frontZ], [X, DAIS.stepY, DAIS.frontZ], [X, ROOM.floor, DAIS.frontZ], [-X, ROOM.floor, DAIS.frontZ]])}" fill="#57291F"/>
    <polygon points="${projPoly([[-X, DAIS.stepY - 6, DAIS.frontZ], [X, DAIS.stepY - 6, DAIS.frontZ], [X, DAIS.stepY + 8, DAIS.frontZ], [-X, DAIS.stepY + 8, DAIS.frontZ]])}" fill="${c.gold}"/>
    <rect x="500" y="1286" width="1400" height="16" fill="#3A1020" opacity="0.25"/>`;
}

function rugSVG() {
  const center = { X: 0, Z: 570 }, radiusX = 600, radiusZ = 140;
  const ring = (rx, rz) => poly(Array.from({ length: 40 }, (_, i) => { const a = i / 40 * Math.PI * 2; return project(center.X + Math.cos(a) * rx, ROOM.floor, center.Z + Math.sin(a) * rz); }));
  return `<polygon points="${ring(radiusX, radiusZ)}" fill="#7C2935"/><polygon points="${ring(radiusX - 40, radiusZ - 16)}" fill="none" stroke="${ROOM_COLORS.gold}" stroke-width="7"/>`;
}

function guestDoorOpening() {
  const { nearZ, farZ, topY } = GUEST_DOOR, X = ROOM.wallX;
  return [[X, ROOM.floor, farZ], [X, ROOM.floor, nearZ], [X, topY, nearZ], [X, topY, farZ]];
}
function guestDoorFrameSVG() {
  const X = ROOM.wallX, { nearZ, farZ, topY } = GUEST_DOOR;
  const frame = [[X, ROOM.floor, farZ + 22], [X, ROOM.floor, nearZ - 22], [X, topY - 40, nearZ - 22], [X, topY - 40, farZ + 22]];
  return `<polygon points="${projPoly(frame)}" fill="${ROOM_COLORS.cornice}" stroke="#CDB28E" stroke-width="3"/>
    <polygon points="${projPoly(guestDoorOpening())}" fill="#FFF3C8"/>
    <polygon points="${projPoly([[X, 150, farZ], [X, 150, nearZ], [X, ROOM.floor, nearZ], [X, ROOM.floor, farZ]])}" fill="#9CCB8A"/>
    <polygon points="${projPoly([[X, topY, farZ], [X, topY, nearZ], [X, 150, nearZ], [X, 150, farZ]])}" fill="#A9DBE6"/>
    <polygon id="door-light" points="" fill="#FFF3C0" opacity="0"/>`;
}

/** The guest door leaf, hinged on its near edge, swinging into the room by `angle` degrees. */
function guestDoorLeafSVG(angle) {
  const X = ROOM.wallX, { nearZ, farZ, topY } = GUEST_DOOR, width = farZ - nearZ;
  const a = angle * DEG;
  const farX = X - Math.sin(a) * width, farEdgeZ = nearZ + Math.cos(a) * width;
  const leaf = [[X, ROOM.floor, nearZ], [farX, ROOM.floor, farEdgeZ], [farX, topY, farEdgeZ], [X, topY, nearZ]];
  const inset = (u, v) => { const x = lerp(X, farX, u), z = lerp(nearZ, farEdgeZ, u), y = lerp(topY, ROOM.floor, v); return [x, y, z]; };
  const panel = (u0, u1, v0, v1) => `<polygon points="${projPoly([inset(u0, v0), inset(u1, v0), inset(u1, v1), inset(u0, v1)])}" fill="none" stroke="#24524F" stroke-width="5"/>`;
  const thickness = [[farX, ROOM.floor, farEdgeZ], [farX - Math.cos(a) * 12, ROOM.floor, farEdgeZ - Math.sin(a) * 12], [farX - Math.cos(a) * 12, topY, farEdgeZ - Math.sin(a) * 12], [farX, topY, farEdgeZ]];
  const knob = project(...inset(0.86, 0.55));
  return `<polygon points="${projPoly(thickness)}" fill="#1F4744"/><polygon points="${projPoly(leaf)}" fill="#2F6E6B" stroke="#1E3F3D" stroke-width="4"/>
    ${panel(0.15, 0.85, 0.08, 0.45)}${panel(0.15, 0.85, 0.55, 0.92)}<circle cx="${knob[0]}" cy="${knob[1]}" r="${7 * depthScale(farEdgeZ)}" fill="${ROOM_COLORS.gold}" stroke="#1E3F3D" stroke-width="2"/>`;
}

/** Sunlight through the open guest door, cast across the floor toward the camera-left. */
function doorLightPoints() {
  const X = ROOM.wallX, { nearZ, farZ } = GUEST_DOOR;
  return projPoly([[X, ROOM.floor, farZ], [X, ROOM.floor, nearZ], [X - 520, ROOM.floor, nearZ - 190], [X - 620, ROOM.floor, farZ - 150]]);
}

// ---------------------------------------------------------------- desk, laptop, clutter
function deskSVG() {
  const c = ROOM_COLORS, { halfWidth: hw, frontZ, backZ, topY } = DESK_BOX;
  const lip = 26;
  const top = projPoly([[-hw, topY, backZ], [hw, topY, backZ], [hw, topY, frontZ], [-hw, topY, frontZ]]);
  const frontFace = projPoly([[-hw, topY, frontZ], [hw, topY, frontZ], [hw, ROOM.floor, frontZ], [-hw, ROOM.floor, frontZ]]);
  const lipFace = projPoly([[-hw, topY, frontZ], [hw, topY, frontZ], [hw, topY + lip, frontZ], [-hw, topY + lip, frontZ]]);
  const drawer = (x0, x1) => projPoly([[x0, topY + 60, frontZ], [x1, topY + 60, frontZ], [x1, topY + 170, frontZ], [x0, topY + 170, frontZ]]);
  const [plateX, plateY] = project(0, topY + 58, frontZ);
  const plateScale = depthScale(frontZ);
  const mugs = Array.from({ length: 4 }, (_, i) => { const [x, y] = project(-250 + (i % 2) * 4, topY, 530); const s = depthScale(530) * PERSON_WORLD * 0.72;
    return `<g transform="translate(${x},${y - i * 40 * s}) scale(${s})"><path d="M26,-30 Q46,-26 44,-16 Q42,-6 26,-8" fill="none" stroke="${INK}" stroke-width="6"/><path d="M26,-30 Q46,-26 44,-16 Q42,-6 26,-8" fill="none" stroke="#FFFFFF" stroke-width="2.5"/>
      <path d="M-26,-38 L26,-38 L22,0 L-22,0 Z" fill="#FFFFFF" stroke="${INK}" stroke-width="3"/><ellipse cx="0" cy="-38" rx="26" ry="6" fill="#EDE6DA" stroke="${INK}" stroke-width="3"/>
      <rect x="-22" y="-26" width="44" height="8" fill="#C8402F"/></g>`; }).join('');
  const blotter = projPoly([[-170, topY, 548], [170, topY, 548], [170, topY, 505], [-170, topY, 505]]);
  const papers = [0, 1, 2].map(i => projPoly([[-300 + i * 3, topY - i * 3, 520], [-190 + i * 3, topY - i * 3, 520], [-190 + i * 3, topY - i * 3, 500], [-300 + i * 3, topY - i * 3, 500]]));
  const cans = [[215, 0], [250, 0], [285, 0], [232, 1], [268, 1], [250, 2]].map(([X, row]) => { const [x, y] = project(X, topY, 530); const s = depthScale(530) * PERSON_WORLD * 0.8; return `<g transform="translate(${x},${y - row * 50 * s}) scale(${s})"><rect x="-17" y="-50" width="34" height="50" rx="5" fill="#3FA56B" stroke="${INK}" stroke-width="3"/><rect x="-17" y="-38" width="34" height="14" fill="#F4E04D"/></g>`; }).join('');
  const laptopBase = projPoly([[-90, topY, 545], [90, topY, 545], [90, topY - 38, 545], [-90, topY - 38, 545]]);
  return `<g id="desk">
    <polygon points="${top}" fill="${c.woodLight}" stroke="${INK}" stroke-width="3"/>
    <polygon points="${blotter}" fill="#2F5D4A" stroke="${INK}" stroke-width="3"/>
    ${papers.map(p => `<polygon points="${p}" fill="#FFFDF6" stroke="${INK}" stroke-width="2"/>`).join('')}
    <polygon points="${laptopBase}" fill="#B9BCC2" stroke="${INK}" stroke-width="3"/>
    ${mugs}${cans}
    <polygon points="${frontFace}" fill="${c.wood}" stroke="${INK}" stroke-width="3"/>
    <polygon points="${lipFace}" fill="#7E4632"/>
    <polygon points="${drawer(-hw + 30, -70)}" fill="none" stroke="${c.woodLight}" stroke-width="6"/>
    <polygon points="${drawer(70, hw - 30)}" fill="none" stroke="${c.woodLight}" stroke-width="6"/>
    <g transform="translate(${plateX},${plateY}) scale(${plateScale * 0.62})"><rect x="-92" y="-28" width="184" height="56" rx="4" fill="${c.gold}" stroke="${INK}" stroke-width="3"/>
      <text id="hour-plaque" y="11" font-family="Futura, sans-serif" font-weight="700" font-size="30" letter-spacing="3" text-anchor="middle" fill="#3B2A0C">HOUR 71</text></g>
  </g>`;
}

// ---------------------------------------------------------------- swivel chair (rebuilt each frame from its spin angle)
function chairSVG(spin) {
  const leather = '#7C2935', leatherBack = '#5E1D28', metal = '#3B3B42';
  const cos = Math.cos(spin), sin = Math.sin(spin);
  const star = Array.from({ length: 5 }, (_, k) => { const a = spin + k * (Math.PI * 2 / 5); return [Math.cos(a) * 78, -16 + Math.sin(a) * 20, Math.sin(a)]; });
  const legs = star.map(([x, y]) => `<line x1="0" y1="-18" x2="${x}" y2="${y}" stroke="${metal}" stroke-width="10" stroke-linecap="round"/>`).join('');
  const casters = star.map(([x, y]) => `<circle cx="${x}" cy="${y + 6}" r="8" fill="#1d1d22" stroke="${INK}" stroke-width="2"/>`).join('');
  const backWidth = 180 * Math.abs(cos), backX = sin * 58;
  const back = cos >= 0
    ? `<rect x="${backX - backWidth / 2}" y="-480" width="${backWidth}" height="300" rx="${Math.min(60, backWidth / 2)}" fill="${leather}" stroke="${INK}" stroke-width="4"/>
       ${[-1, 0, 1].flatMap(i => [-400, -330, -260].map(y => `<circle cx="${backX + i * 52 * cos}" cy="${y}" r="7" fill="#5A1A24"/>`)).join('')}`
    : `<rect x="${backX - backWidth / 2}" y="-480" width="${backWidth}" height="300" rx="${Math.min(60, backWidth / 2)}" fill="${leatherBack}" stroke="${INK}" stroke-width="4"/>
       <line x1="${backX}" y1="-470" x2="${backX}" y2="-190" stroke="#3E1219" stroke-width="3"/>`;
  const edge = `<rect x="${backX + (sin > 0 ? backWidth / 2 - 4 : -backWidth / 2 - 18 * Math.abs(sin) + 4)}" y="-476" width="${18 * Math.abs(sin)}" height="292" fill="#4A1620"/>`;
  const arm = side => { const x = side * 104 * cos, depth = -side * sin; return { depth, svg: `<g><rect x="${x - 7}" y="-250" width="14" height="80" fill="${metal}"/><rect x="${x - 26}" y="-266" width="52" height="20" rx="8" fill="#2C2C32" stroke="${INK}" stroke-width="3"/></g>` }; };
  const arms = [arm(-1), arm(1)];
  const seat = `<rect x="-10" y="-172" width="20" height="150" fill="${metal}" stroke="${INK}" stroke-width="3"/>
    <rect x="-100" y="-186" width="200" height="30" rx="14" fill="#6B2230" stroke="${INK}" stroke-width="4"/><ellipse cx="0" cy="-186" rx="100" ry="24" fill="${leather}" stroke="${INK}" stroke-width="4"/>`;
  const behind = cos >= 0 ? back + edge : '';
  const inFront = cos < 0 ? back + edge : '';
  return `${legs}${casters}${arms.filter(a => a.depth > 0).map(a => a.svg).join('')}${behind}${seat}${inFront}${arms.filter(a => a.depth <= 0).map(a => a.svg).join('')}`;
}

// ---------------------------------------------------------------- foreground (depth + parallax)
function foregroundSVG() {
  const stanchion = X => {
    const [x, y] = project(X, ROOM.floor, 470); const s = personScale(470);
    return `<g transform="translate(${x},${y}) scale(${s})"><ellipse cx="0" cy="0" rx="46" ry="12" fill="${ROOM_COLORS.goldDark}" stroke="${INK}" stroke-width="4"/>
      <rect x="-9" y="-330" width="18" height="330" fill="${ROOM_COLORS.gold}" stroke="${INK}" stroke-width="4"/><circle cx="0" cy="-340" r="20" fill="${ROOM_COLORS.gold}" stroke="${INK}" stroke-width="4"/></g>`;
  };
  const [lx, ly] = project(-560, ROOM.floor - 330 * PERSON_WORLD, 470), [rx] = project(560, 0, 470);
  const leftRope = `<path d="M${lx},${ly} Q${lx - 160},${ly + 140} ${lx - 420},${ly + 60}" fill="none" stroke="#7C1D2C" stroke-width="22" stroke-linecap="round"/>`;
  const rightRope = `<path d="M${rx},${ly} Q${rx + 160},${ly + 140} ${rx + 420},${ly + 60}" fill="none" stroke="#7C1D2C" stroke-width="22" stroke-linecap="round"/>`;
  return `${leftRope}${rightRope}${stanchion(-560)}${stanchion(560)}`;
}

// ---------------------------------------------------------------- doorway clips
/** Everything outside the right wall's face, plus the guest door opening: a figure beyond the wall shows only through the door. */
function guestDoorClipSVG() {
  const far = 20000, [cx0, cy0] = project(ROOM.wallX, ROOM.ceiling, ROOM.back), [fx0, fy0] = project(ROOM.wallX, ROOM.floor, ROOM.back);
  const ceilingEdge = project(ROOM.wallX, ROOM.ceiling, 60), floorEdge = project(ROOM.wallX, ROOM.floor, 60);
  const outside = poly([[-far, -far], [far, -far], ceilingEdge, [cx0, cy0], [fx0, fy0], floorEdge, [far, far], [-far, far]]);
  return `<clipPath id="guest-door-clip" clipPathUnits="userSpaceOnUse"><polygon points="${outside}"/><polygon points="${projPoly(guestDoorOpening())}"/></clipPath>`;
}
/** Everything but the back wall, plus the service door openings. */
function serviceDoorClipSVG() {
  const far = 5000;
  const outside = poly([[-far, -far], [far, -far], [far, 260], [500, 260], [500, 1300], [1900, 1300], [1900, 260], [far, 260], [far, far], [-far, far]]);
  const openings = ['left', 'right'].map(side => { const cx = SERVICE_DOORS[side], hw = SERVICE_DOORS.halfWidth; return `<polygon points="${projPoly([[cx - hw, SERVICE_DOORS.topY, ROOM.back], [cx + hw, SERVICE_DOORS.topY, ROOM.back], [cx + hw, DAIS.top, ROOM.back], [cx - hw, DAIS.top, ROOM.back]])}"/>`; }).join('');
  return `<clipPath id="service-door-clip" clipPathUnits="userSpaceOnUse"><polygon points="${outside}"/>${openings}</clipPath>`;
}

function framedPainting(x, y, w, h, inner) {
  return `<g transform="translate(${x},${y})"><rect x="-22" y="-22" width="${w + 44}" height="${h + 44}" fill="${ROOM_COLORS.gold}" stroke="${ROOM_COLORS.goldDark}" stroke-width="6"/>
    <svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">${inner}</svg></g>`;
}
function duckPainting() {
  return `<rect width="260" height="330" fill="#DDEBF0"/><ellipse cx="130" cy="265" rx="110" ry="22" fill="#9CC3D1"/>
    <ellipse cx="135" cy="215" rx="82" ry="55" fill="#F4C542"/><circle cx="92" cy="140" r="46" fill="#F4C542"/>
    <path d="M48,140 L18,150 L48,160 Z" fill="#E9803A"/><circle cx="84" cy="128" r="7" fill="#222"/>`;
}
function chandelierSVG() {
  const arms = [-1, -0.5, 0, 0.5, 1].map(k => `<path d="M1200,120 Q${1200 + k * 160},170 ${1200 + k * 190},110" fill="none" stroke="${ROOM_COLORS.gold}" stroke-width="8"/>
    <rect x="${1200 + k * 190 - 7}" y="72" width="14" height="38" fill="#FFF8E6"/><ellipse cx="${1200 + k * 190}" cy="64" rx="7" ry="12" fill="#FFD36B"/>`).join('');
  return `<line x1="1200" y1="-400" x2="1200" y2="110" stroke="${ROOM_COLORS.goldDark}" stroke-width="6"/>${arms}<circle cx="1200" cy="126" r="22" fill="${ROOM_COLORS.gold}"/>
    <path d="M1180,146 L1200,190 L1220,146 Z" fill="${ROOM_COLORS.gold}"/>`;
}
