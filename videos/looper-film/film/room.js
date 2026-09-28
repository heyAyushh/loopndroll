// THE ROOM: a concrete loft at night. Practical light only: the monitor, the moon through the window
// grid, the phone, and, once it takes over, the lavender glass orb. Dawn replaces the moon at the end.
import * as THREE from "three";
import { RoundedBoxGeometry } from "three/addons/geometries/RoundedBoxGeometry.js";
import { RectAreaLightUniformsLib } from "three/addons/lights/RectAreaLightUniformsLib.js";
import { BAR, BEAT, BEATS, at, clamp, lerp, looperOn, prng, smooth } from "./story.js";
import { chairState, createDeveloper, phoneCarried } from "./character.js";
import { createCity, createClock, createLens, createNeighbour, createNote, createPhone, createRain, createScreen } from "./ui.js";

const COLD_SCREEN = new THREE.Color(0xc4ccff);
const LAVENDER = new THREE.Color(0xb9a4ff);
const MOON = new THREE.Color(0x7f8cff);
const DAWN = new THREE.Color(0xffb27a);

export const LAYOUT = {
  desk: new THREE.Vector3(-2.2, 0.75, -2.4),
  monitor: new THREE.Vector3(-2.2, 1.2, -2.62),
  orb: new THREE.Vector3(-1.5, 0.827, -2.33),
  phoneOnDesk: new THREE.Vector3(-2.82, 0.765, -2.28),
  note: new THREE.Vector3(-2.66, 1.03, -2.6),
  clock: new THREE.Vector3(-2.2, 2.3, -2.985),
  window: { x0: 0.6, x1: 4.6, y0: 0.9, y1: 3.8, z: -3.0 },
  neighbour: new THREE.Vector3(3.3, 3.1, -11.9),
  couch: new THREE.Vector3(2.55, 0, -2.5),
  mug: new THREE.Vector3(-1.62, 0.8, -2.52),
  keyboard: new THREE.Vector3(-2.2, 0.772, -2.24),
  mouse: new THREE.Vector3(-1.72, 0.775, -2.2),
};

function concreteTexture(seed, tint) {
  const size = 1024;
  const element = document.createElement("canvas");
  element.width = size; element.height = size;
  const g = element.getContext("2d");
  const random = prng(seed);
  g.fillStyle = tint; g.fillRect(0, 0, size, size);
  for (let i = 0; i < 26000; i += 1) {
    const v = random();
    g.fillStyle = v > 0.5 ? `rgba(255,255,255,${0.02 + random() * 0.04})` : `rgba(0,0,0,${0.03 + random() * 0.06})`;
    const r = 1 + random() * 5;
    g.fillRect(random() * size, random() * size, r, r);
  }
  g.filter = "blur(1.5px)"; g.drawImage(element, 0, 0); g.filter = "none";
  g.strokeStyle = "rgba(0,0,0,0.35)"; g.lineWidth = 3;
  g.beginPath(); g.moveTo(0, size / 2); g.lineTo(size, size / 2); g.moveTo(size / 2, 0); g.lineTo(size / 2, size); g.stroke();
  g.fillStyle = "rgba(0,0,0,0.45)";
  for (const [x, y] of [[size / 4, size / 4], [3 * size / 4, size / 4], [size / 4, 3 * size / 4], [3 * size / 4, 3 * size / 4]]) { g.beginPath(); g.arc(x, y, 7, 0, Math.PI * 2); g.fill(); }
  const texture = new THREE.CanvasTexture(element);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  texture.anisotropy = 8;
  return texture;
}

function canvasTexture(element) {
  const texture = new THREE.CanvasTexture(element);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 8;
  return texture;
}

function box(w, h, d, material, x, y, z, parent) {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), material);
  mesh.position.set(x, y, z); mesh.castShadow = true; mesh.receiveShadow = true;
  parent.add(mesh);
  return mesh;
}

export function createRoom(orbImage) {
  RectAreaLightUniformsLib.init();
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x020204);
  scene.fog = new THREE.FogExp2(0x05060c, 0.035);

  const screen = createScreen(orbImage);
  const phoneUi = createPhone(orbImage);
  const clockUi = createClock();
  const rainUi = createRain();
  const neighbourUi = createNeighbour();
  const lensUi = createLens(screen);
  const textures = {
    screen: canvasTexture(screen.canvas), phone: canvasTexture(phoneUi.canvas), clock: canvasTexture(clockUi.canvas),
    rain: canvasTexture(rainUi.canvas), neighbour: canvasTexture(neighbourUi.canvas), lens: canvasTexture(lensUi.canvas),
    note: canvasTexture(createNote().canvas), skyNight: canvasTexture(createCity(false, "sky").canvas), skyDawn: canvasTexture(createCity(true, "sky").canvas),
    cityNight: canvasTexture(createCity(false, "skyline").canvas), cityDawn: canvasTexture(createCity(true, "skyline").canvas),
  };

  // ---------- architecture ----------
  const wallTexture = concreteTexture(11, "#3a3a42");
  wallTexture.repeat.set(3, 1.4);
  const wall = new THREE.MeshStandardMaterial({ map: wallTexture, roughness: 0.92, color: 0x8a8a96 });
  const floorTexture = concreteTexture(12, "#2c2c33");
  floorTexture.repeat.set(4, 3);
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(14, 12), new THREE.MeshStandardMaterial({ map: floorTexture, roughness: 0.55, metalness: 0.05, color: 0x7a7a86 }));
  floor.rotation.x = -Math.PI / 2; floor.position.set(0.5, 0, 1); floor.receiveShadow = true;
  scene.add(floor);
  const W = LAYOUT.window;
  const backZ = W.z;
  box(6.0, 4.6, 0.25, wall, W.x0 - 3.0, 2.3, backZ - 0.12, scene);            // left of window
  box(2.0, 4.6, 0.25, wall, W.x1 + 1.0, 2.3, backZ - 0.12, scene);            // right of window
  box(W.x1 - W.x0, W.y0, 0.25, wall, (W.x0 + W.x1) / 2, W.y0 / 2, backZ - 0.12, scene); // sill
  box(W.x1 - W.x0, 4.6 - W.y1, 0.25, wall, (W.x0 + W.x1) / 2, (4.6 + W.y1) / 2, backZ - 0.12, scene); // lintel
  box(0.25, 4.6, 12, wall, -5.2, 2.3, 3, scene);                              // left wall
  box(0.25, 4.6, 12, wall, 5.7, 2.3, 3, scene);                               // right wall
  box(12, 0.2, 12, new THREE.MeshStandardMaterial({ color: 0x0c0c10, roughness: 1 }), 0.25, 4.7, 3, scene); // ceiling
  // The window grid: squares, twelve of them. The moon throws them across the floor.
  const steel = new THREE.MeshStandardMaterial({ color: 0x0b0b0e, roughness: 0.5, metalness: 0.6 });
  for (let i = 0; i <= 4; i += 1) box(0.07, W.y1 - W.y0, 0.14, steel, W.x0 + i * (W.x1 - W.x0) / 4, (W.y0 + W.y1) / 2, backZ, scene);
  for (let r = 0; r <= 3; r += 1) box(W.x1 - W.x0, 0.07, 0.14, steel, (W.x0 + W.x1) / 2, W.y0 + r * (W.y1 - W.y0) / 3, backZ, scene);
  const rainGlass = new THREE.Mesh(new THREE.PlaneGeometry(W.x1 - W.x0, W.y1 - W.y0), new THREE.MeshBasicMaterial({ map: textures.rain, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, opacity: 0.55 }));
  rainGlass.position.set((W.x0 + W.x1) / 2, (W.y0 + W.y1) / 2, backZ + 0.02);
  scene.add(rainGlass);

  // ---------- outside ----------
  // Sky, then the sun, then the skyline in front: the sun rises from behind the buildings.
  const layer = (map, z, transparent) => {
    map.wrapS = THREE.RepeatWrapping; map.repeat.x = 3;
    const mesh = new THREE.Mesh(new THREE.PlaneGeometry(240, 27), new THREE.MeshBasicMaterial({ map, transparent, opacity: 1, fog: false, depthWrite: !transparent }));
    mesh.position.set(40, 8.5, z);
    scene.add(mesh);
    return mesh;
  };
  layer(textures.skyNight, -34, false);
  const skyDawn = layer(textures.skyDawn, -33.9, true);
  const sun = new THREE.Mesh(new THREE.CircleGeometry(2.4, 64), new THREE.MeshBasicMaterial({ color: 0xfff1d6, transparent: true, opacity: 0, fog: false, toneMapped: false }));
  sun.position.set(22, 0, -33.5);
  scene.add(sun);
  const cityNight = layer(textures.cityNight, -33, true);
  const cityDawn = layer(textures.cityDawn, -32.9, true);
  // The building across the street, and the one window where someone else is stuck too.
  const facade = new THREE.Mesh(new THREE.PlaneGeometry(9, 9), new THREE.MeshStandardMaterial({ color: 0x101016, roughness: 1 }));
  facade.position.set(3.3, 3.5, -12.0);
  scene.add(facade);
  const neighbour = new THREE.Mesh(new THREE.PlaneGeometry(1.3, 1.3), new THREE.MeshBasicMaterial({ map: textures.neighbour, fog: false }));
  neighbour.position.copy(LAYOUT.neighbour);
  scene.add(neighbour);
  const darkPanes = new THREE.MeshBasicMaterial({ color: 0x07070c });
  const litPane = new THREE.MeshBasicMaterial({ color: 0x3a2f24 });
  for (let row = 0; row < 5; row += 1) for (let column = 0; column < 5; column += 1) {
    if (row === 2 && column === 2) continue;
    const pane = new THREE.Mesh(new THREE.PlaneGeometry(1.1, 1.1), (row * 7 + column) % 9 === 0 ? litPane : darkPanes);
    pane.position.set(3.3 + (column - 2) * 1.7, 3.1 + (row - 2) * 1.7, -11.95);
    scene.add(pane);
  }

  // ---------- desk ----------
  const deskWood = new THREE.MeshStandardMaterial({ color: 0x1a1714, roughness: 0.7 });
  const black = new THREE.MeshStandardMaterial({ color: 0x08080a, roughness: 0.45, metalness: 0.3 });
  box(1.9, 0.04, 0.72, deskWood, LAYOUT.desk.x, 0.73, LAYOUT.desk.z, scene);
  for (const [dx, dz] of [[-0.9, -0.32], [0.9, -0.32], [-0.9, 0.32], [0.9, 0.32]]) box(0.04, 0.72, 0.04, black, LAYOUT.desk.x + dx, 0.36, LAYOUT.desk.z + dz, scene);
  const M = LAYOUT.monitor;
  box(1.06, 0.48, 0.03, black, M.x, M.y, M.z - 0.02, scene);
  box(0.06, 0.34, 0.05, black, M.x, 0.92, M.z - 0.03, scene);
  box(0.3, 0.015, 0.2, black, M.x, 0.757, M.z + 0.02, scene);
  const screenMesh = new THREE.Mesh(new THREE.PlaneGeometry(1.0, 0.42), new THREE.MeshBasicMaterial({ map: textures.screen, toneMapped: false }));
  screenMesh.position.set(M.x, M.y, M.z + 0.0 + 0.001);
  scene.add(screenMesh);
  const keys = document.createElement("canvas");
  keys.width = 1024; keys.height = 320;
  const kg = keys.getContext("2d");
  kg.fillStyle = "#0b0b0e"; kg.fillRect(0, 0, 1024, 320);
  const rows = [15, 14, 13, 12];
  rows.forEach((count, row) => { const w = 1000 / 15; for (let i = 0; i < count; i += 1) { kg.fillStyle = "#1d1d23"; kg.beginPath(); kg.roundRect(12 + row * w * 0.3 + i * w, 12 + row * 62, w - 8, 54, 8); kg.fill(); } });
  kg.fillStyle = "#1d1d23"; kg.beginPath(); kg.roundRect(260, 262, 480, 50, 8); kg.fill();
  const keyTexture = canvasTexture(keys);
  const keyboard = box(0.44, 0.018, 0.14, [0, 0, new THREE.MeshStandardMaterial({ map: keyTexture, roughness: 0.55 }), 0, 0, 0].map((m) => m || new THREE.MeshStandardMaterial({ color: 0x101013, roughness: 0.6 })), LAYOUT.keyboard.x, LAYOUT.keyboard.y, LAYOUT.keyboard.z, scene);
  const mouse = new THREE.Mesh(new THREE.CapsuleGeometry(0.025, 0.04, 4, 10), new THREE.MeshStandardMaterial({ color: 0x1a1a1e, roughness: 0.4 }));
  mouse.rotation.x = Math.PI / 2; mouse.scale.set(1, 1, 0.5); mouse.position.copy(LAYOUT.mouse); mouse.castShadow = true;
  scene.add(mouse);
  const mug = new THREE.Mesh(new THREE.CylinderGeometry(0.042, 0.038, 0.1, 24), new THREE.MeshStandardMaterial({ color: 0xd9d4cc, roughness: 0.4 }));
  mug.position.copy(LAYOUT.mug); mug.castShadow = true;
  scene.add(mug);
  const note = new THREE.Mesh(new THREE.PlaneGeometry(0.08, 0.08), new THREE.MeshStandardMaterial({ map: textures.note, roughness: 0.9 }));
  note.position.copy(LAYOUT.note); note.rotation.z = 0.09;
  scene.add(note);
  const clock = new THREE.Mesh(new THREE.PlaneGeometry(0.72, 0.225), new THREE.MeshBasicMaterial({ map: textures.clock, toneMapped: false }));
  clock.position.copy(LAYOUT.clock);
  scene.add(clock);

  // The glass orb: a circle among squares, dark until Looper keeps the loop.
  const orbGlass = new THREE.Mesh(new THREE.SphereGeometry(0.075, 48, 32), new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 0.03, transmission: 1, thickness: 0.12, ior: 1.45, clearcoat: 1, envMapIntensity: 1 }));
  orbGlass.position.copy(LAYOUT.orb); orbGlass.castShadow = true;
  scene.add(orbGlass);
  const orbCore = new THREE.Mesh(new THREE.TorusKnotGeometry(0.025, 0.007, 90, 10, 2, 3), new THREE.MeshStandardMaterial({ color: 0x9a92b0, emissive: LAVENDER, emissiveIntensity: 0, roughness: 0.2, metalness: 0.8 }));
  orbCore.position.copy(LAYOUT.orb);
  scene.add(orbCore);
  const orbLight = new THREE.PointLight(LAVENDER, 0, 5, 2);
  orbLight.position.copy(LAYOUT.orb).add(new THREE.Vector3(0, 0.05, 0.05));
  scene.add(orbLight);

  // The phone, face up on the desk until they carry it.
  const phone = new THREE.Group();
  const phoneBody = new THREE.Mesh(new RoundedBoxGeometry(0.075, 0.009, 0.155, 2, 0.004), black);
  phoneBody.castShadow = true;
  phone.add(phoneBody);
  const phoneScreen = new THREE.Mesh(new THREE.PlaneGeometry(0.068, 0.148), new THREE.MeshBasicMaterial({ map: textures.phone, toneMapped: false, transparent: true, opacity: 0 }));
  phoneScreen.rotation.x = -Math.PI / 2; phoneScreen.position.y = 0.0048;
  phone.add(phoneScreen);
  scene.add(phone);
  const phoneLight = new THREE.PointLight(0xd8dcff, 0, 0.7, 2);
  scene.add(phoneLight);

  // ---------- chair, couch, set dressing ----------
  const chair = new THREE.Group();
  const fabric = new THREE.MeshStandardMaterial({ color: 0x141418, roughness: 0.9 });
  const seat = new THREE.Mesh(new RoundedBoxGeometry(0.5, 0.08, 0.48, 3, 0.03), fabric); seat.position.y = 0.45; seat.castShadow = true; chair.add(seat);
  const back = new THREE.Mesh(new RoundedBoxGeometry(0.48, 0.6, 0.06, 3, 0.03), fabric); back.position.set(0, 0.84, 0.25); back.rotation.x = -0.12; back.castShadow = true; chair.add(back);
  const stem = new THREE.Mesh(new THREE.CylinderGeometry(0.025, 0.025, 0.35, 12), black); stem.position.y = 0.24; chair.add(stem);
  for (let leg = 0; leg < 5; leg += 1) {
    const arm = new THREE.Mesh(new THREE.BoxGeometry(0.035, 0.03, 0.32), black);
    arm.position.set(Math.sin(leg * 1.2566) * 0.16, 0.05, Math.cos(leg * 1.2566) * 0.16); arm.rotation.y = leg * 1.2566; chair.add(arm);
  }
  scene.add(chair);
  const couchFabric = new THREE.MeshStandardMaterial({ color: 0x24232b, roughness: 0.95 });
  const C = LAYOUT.couch;
  const couchBase = new THREE.Mesh(new RoundedBoxGeometry(2.7, 0.42, 0.9, 3, 0.06), couchFabric); couchBase.position.set(C.x, 0.23, C.z + 0.05); couchBase.castShadow = couchBase.receiveShadow = true; scene.add(couchBase);
  const couchBack = new THREE.Mesh(new RoundedBoxGeometry(2.7, 0.5, 0.22, 3, 0.08), couchFabric); couchBack.position.set(C.x, 0.66, C.z - 0.33); couchBack.castShadow = couchBack.receiveShadow = true; scene.add(couchBack);
  for (const side of [-1, 1]) { const arm = new THREE.Mesh(new RoundedBoxGeometry(0.22, 0.62, 0.9, 3, 0.08), couchFabric); arm.position.set(C.x + side * 1.3, 0.33, C.z + 0.05); arm.castShadow = arm.receiveShadow = true; scene.add(arm); }
  const throwBlanket = new THREE.Mesh(new RoundedBoxGeometry(0.8, 0.05, 0.7, 2, 0.02), new THREE.MeshStandardMaterial({ color: 0x3a3048, roughness: 1 }));
  throwBlanket.position.set(C.x - 0.7, 0.47, C.z + 0.08); throwBlanket.rotation.y = 0.2; scene.add(throwBlanket);
  const random = prng(5);
  for (let shelf = 0; shelf < 4; shelf += 1) {
    box(0.35, 0.03, 1.6, black, -5.0, 0.4 + shelf * 0.55, 0.2, scene);
    let z = -0.55;
    while (z < 0.9) { const w = 0.03 + random() * 0.05; const h = 0.2 + random() * 0.18; box(0.22, h, w, new THREE.MeshStandardMaterial({ color: new THREE.Color().setHSL(0.6 + random() * 0.4, 0.15, 0.08 + random() * 0.1), roughness: 0.9 }), -4.95, 0.415 + shelf * 0.55 + h / 2, z, scene); z += w + 0.01; }
  }
  const pot = new THREE.Mesh(new THREE.CylinderGeometry(0.16, 0.12, 0.34, 20), new THREE.MeshStandardMaterial({ color: 0x1b1b1f, roughness: 0.8 }));
  pot.position.set(-0.55, 0.17, -2.55); pot.castShadow = true; scene.add(pot);
  for (let leaf = 0; leaf < 14; leaf += 1) {
    const blade = new THREE.Mesh(new THREE.SphereGeometry(0.16, 10, 8), new THREE.MeshStandardMaterial({ color: 0x14231a, roughness: 0.8 }));
    const angle = leaf * 2.4; const height = 0.45 + (leaf % 5) * 0.14;
    blade.scale.set(0.35, 1, 0.08); blade.position.set(-0.55 + Math.cos(angle) * 0.14, height, -2.55 + Math.sin(angle) * 0.14);
    blade.rotation.set(Math.sin(angle) * 0.6, angle, Math.cos(angle) * 0.6); blade.castShadow = true; scene.add(blade);
  }
  const cord = new THREE.Mesh(new THREE.CylinderGeometry(0.004, 0.004, 1.9, 6), black); cord.position.set(-2.2, 3.7, -2.1); scene.add(cord);
  const bulb = new THREE.Mesh(new THREE.SphereGeometry(0.06, 16, 12), new THREE.MeshStandardMaterial({ color: 0x2a2620, roughness: 0.3 })); bulb.position.set(-2.2, 2.72, -2.1); scene.add(bulb);
  const rug = new THREE.Mesh(new THREE.PlaneGeometry(3.4, 2.2), new THREE.MeshStandardMaterial({ color: 0x3a3642, roughness: 1 }));
  rug.rotation.x = -Math.PI / 2; rug.position.set(1.2, 0.004, -0.9); rug.receiveShadow = true; scene.add(rug);

  // ---------- the developer ----------
  const developer = createDeveloper(textures.lens);
  scene.add(developer.root);

  // ---------- light ----------
  const hemisphere = new THREE.HemisphereLight(0x2a2f55, 0x050508, 0.25);
  scene.add(hemisphere);
  const moon = new THREE.DirectionalLight(MOON, 1.4);
  moon.position.set(7, 9, -16); moon.target.position.set(1.2, 0, -0.5);
  moon.castShadow = true; moon.shadow.mapSize.set(4096, 4096);
  Object.assign(moon.shadow.camera, { left: -9, right: 9, top: 9, bottom: -9, near: 1, far: 40 });
  moon.shadow.bias = -0.0004; moon.shadow.normalBias = 0.02;
  scene.add(moon, moon.target);
  const screenArea = new THREE.RectAreaLight(COLD_SCREEN, 6, 1.0, 0.42);
  screenArea.position.set(M.x, M.y, M.z + 0.02); screenArea.lookAt(M.x, M.y, 2);
  scene.add(screenArea);
  const screenSpot = new THREE.SpotLight(COLD_SCREEN, 5, 6, 1.1, 1, 2);
  screenSpot.position.set(M.x, M.y + 0.05, M.z + 0.08); screenSpot.target.position.set(M.x, 0.9, -1.0);
  screenSpot.castShadow = true; screenSpot.shadow.mapSize.set(2048, 2048); screenSpot.shadow.bias = -0.0006;
  scene.add(screenSpot, screenSpot.target);
  const cityBounce = new THREE.PointLight(0xff9a5a, 0.8, 9, 2);
  cityBounce.position.set(2.6, 2.4, -4.5);
  scene.add(cityBounce);

  // Volumetric shaft from the window, with dust hanging in it.
  const shaftGeometry = new THREE.BufferGeometry();
  const shaftDirection = moon.target.position.clone().sub(moon.position).normalize();
  const corners = [[W.x0, W.y0], [W.x1, W.y0], [W.x1, W.y1], [W.x0, W.y1]].map(([x, y]) => new THREE.Vector3(x, y, backZ));
  const floorHit = (p) => { const k = -p.y / shaftDirection.y; return p.clone().addScaledVector(shaftDirection, k * 0.98); };
  const ends = corners.map(floorHit);
  const positions = [];
  const colors = [];
  const quad = (a, b, c, d, ca, cb) => { for (const [p, alpha] of [[a, ca], [b, ca], [c, cb], [a, ca], [c, cb], [d, cb]]) { positions.push(p.x, p.y, p.z); colors.push(alpha, alpha, alpha); } };
  for (let i = 0; i < 4; i += 1) quad(corners[i], corners[(i + 1) % 4], ends[(i + 1) % 4], ends[i], 1, 0.15);
  shaftGeometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  shaftGeometry.setAttribute("color", new THREE.Float32BufferAttribute(colors, 3));
  const shaftMaterial = new THREE.MeshBasicMaterial({ vertexColors: true, transparent: true, opacity: 0.05, blending: THREE.AdditiveBlending, depthWrite: false, side: THREE.DoubleSide, color: MOON, fog: false });
  const shaftAxis = corners.reduce((sum, c) => sum.add(c), new THREE.Vector3()).multiplyScalar(0.25);
  for (const shell of [1.0, 0.82, 0.64, 0.46]) {
    const layer = new THREE.Mesh(shaftGeometry, shaftMaterial);
    layer.position.copy(shaftAxis).multiplyScalar(1 - shell);
    layer.scale.setScalar(shell);
    scene.add(layer);
  }
  const dustCount = 420;
  const dustSeed = prng(19);
  const dustBase = Array.from({ length: dustCount }, () => ({ u: dustSeed(), v: dustSeed(), w: dustSeed(), phase: dustSeed() * 50 }));
  const dustGeometry = new THREE.BufferGeometry();
  dustGeometry.setAttribute("position", new THREE.Float32BufferAttribute(new Float32Array(dustCount * 3), 3));
  const dustMaterial = new THREE.PointsMaterial({ size: 0.012, color: 0xc8ccff, transparent: true, opacity: 0.6, blending: THREE.AdditiveBlending, depthWrite: false, sizeAttenuation: true });
  const dust = new THREE.Points(dustGeometry, dustMaterial);
  scene.add(dust);

  // Steam from the mug, cooling through the night.
  const steamCount = 40;
  const steamGeometry = new THREE.BufferGeometry();
  steamGeometry.setAttribute("position", new THREE.Float32BufferAttribute(new Float32Array(steamCount * 3), 3));
  const steam = new THREE.Points(steamGeometry, new THREE.PointsMaterial({ size: 0.03, color: 0xaaaacc, transparent: true, opacity: 0.25, depthWrite: false, blending: THREE.AdditiveBlending }));
  scene.add(steam);

  return {
    scene,
    developer,
    screen, screenMesh, phone, orbGlass, clock,
    update(t, sub) {
      screen.draw(t, sub); textures.screen.needsUpdate = true;
      lensUi.draw(t); textures.lens.needsUpdate = true;
      clockUi.draw(t); textures.clock.needsUpdate = true;
      phoneUi.draw(t); textures.phone.needsUpdate = true;
      rainUi.draw(t); textures.rain.needsUpdate = true;
      neighbourUi.draw(t); textures.neighbour.needsUpdate = true;

      const on = looperOn(t);
      const dawn = smooth(BEATS.dawn - BAR * 0.5, BEATS.dawn + BAR * 2.5, t);
      const stopped = !on && t >= BEATS.firstStop;
      const screenColor = COLD_SCREEN.clone().lerp(LAVENDER, on ? 0.45 + 0.2 * sub : 0);
      const screenPower = (on ? 1.1 + 0.9 * sub : stopped ? 0.75 : 1.0) * (1 - dawn * 0.5);
      screenArea.color.copy(screenColor); screenArea.intensity = 7 * screenPower;
      screenSpot.color.copy(screenColor); screenSpot.intensity = 3.2 * screenPower;

      // The moon crawls; in the time-lapse it sweeps, dragging the window's grid across the floor.
      const moonTravel = clamp(t / BEATS.dawn) * 1.5 + smooth(at(19), at(21), t) * 4 + smooth(at(38), at(39), t) * 2;
      moon.position.set(lerp(7, 9, dawn) - moonTravel, lerp(9, 2.6, dawn), -16);
      moon.color.copy(MOON).lerp(DAWN, dawn);
      moon.intensity = lerp(3.4, 6.5, dawn);
      hemisphere.intensity = lerp(0.16, 0.6, dawn);
      hemisphere.color.set(0x2a2f55).lerp(new THREE.Color(0x805a48), dawn);
      shaftMaterial.color.copy(MOON).lerp(DAWN, dawn);
      shaftMaterial.opacity = lerp(0.012, 0.035, dawn);
      cityBounce.intensity = 0.8 * (1 - dawn);
      cityDawn.material.opacity = dawn;
      skyDawn.material.opacity = dawn;
      cityNight.material.opacity = 1 - dawn;
      sun.material.opacity = smooth(BEATS.dawn - BAR, BEATS.dawn, t);
      sun.material.color.set(0xfff1d6).multiplyScalar(1.6);
      sun.position.y = lerp(1.5, 9.5, smooth(BEATS.dawn - BAR, at(44), t));
      rainGlass.material.opacity = 0.55 * (1 - smooth(BEATS.rainStops, BEATS.rainStops + BAR * 2, t));

      const orbPower = on ? 1.2 + 2.2 * sub : 0;
      orbCore.material.emissiveIntensity = orbPower * (1 - dawn * 0.5) + (on ? 0.3 : 0);
      orbCore.rotation.set(t * 0.3, t * (on ? 1.2 : 0.15), 0);
      orbLight.intensity = orbPower * 0.9 * (1 - dawn * 0.6);

      // Phone: buzzes on the desk; lit when it has something to say.
      const lit = [[at(13), at(14.5)], [BEATS.buzz, at(26)], [at(37), BEATS.asleepCouch], [BEATS.wake + 0.5, at(48)]].some(([a, b]) => t >= a && t < b);
      const buzzing = (t >= BEATS.buzz && t < BEATS.buzz + 0.6) || (t >= at(13) && t < at(13) + 0.3);
      phoneScreen.material.opacity = lit ? 1 : 0;
      if (phoneCarried(t)) {
        developer.root.updateMatrixWorld(true);
        developer.joints.handR.getWorldPosition(phone.position);
        // Held in the right hand, screen turned toward their face.
        const face = developer.joints.head.getWorldPosition(new THREE.Vector3()).add(new THREE.Vector3(0, 0.1, 0));
        phone.position.lerp(face, 0.06 / Math.max(0.06, phone.position.distanceTo(face))); // in front of the grip
        phone.lookAt(face); phone.rotateX(Math.PI / 2);
      } else {
        phone.position.copy(LAYOUT.phoneOnDesk); phone.quaternion.identity(); phone.rotation.y = 0.25;
        if (buzzing) { phone.position.x += Math.sin(t * 190) * 0.003; phone.rotation.y += Math.sin(t * 160) * 0.03; }
      }
      phoneLight.position.copy(phone.position).add(new THREE.Vector3(0, 0.12, 0.05));
      phoneLight.intensity = lit ? 0.18 : 0;

      const chairNow = chairState(t);
      chair.position.set(-2.2 + chairNow.x, 0, -1.55 + chairNow.z);
      chair.rotation.y = chairNow.yaw;
      developer.update(t, sub);

      const dustPositions = dustGeometry.attributes.position.array;
      dustBase.forEach((d, i) => {
        const a = corners[0].clone().lerp(corners[1], d.u).lerp(corners[3].clone().lerp(corners[2], d.u), d.v);
        const p = a.addScaledVector(shaftDirection, -a.y / shaftDirection.y * d.w * 0.95);
        dustPositions[i * 3] = p.x + Math.sin(t * 0.3 + d.phase) * 0.08;
        dustPositions[i * 3 + 1] = p.y + Math.sin(t * 0.2 + d.phase * 1.3) * 0.05 + sub * Math.sin(d.phase * 9) * 0.01;
        dustPositions[i * 3 + 2] = p.z + Math.cos(t * 0.25 + d.phase) * 0.08;
      });
      dustGeometry.attributes.position.needsUpdate = true;
      dustMaterial.color.copy(new THREE.Color(0xc8ccff)).lerp(DAWN, dawn);
      dustMaterial.opacity = 0.3 + 0.25 * dawn;

      const steamPositions = steamGeometry.attributes.position.array;
      const heat = 1 - smooth(at(15), at(19), t);
      for (let i = 0; i < steamCount; i += 1) {
        const life = ((t * 0.35 + i / steamCount) % 1);
        steamPositions[i * 3] = LAYOUT.mug.x + Math.sin(life * 9 + i) * 0.02 * life;
        steamPositions[i * 3 + 1] = LAYOUT.mug.y + 0.06 + life * 0.3;
        steamPositions[i * 3 + 2] = LAYOUT.mug.z + Math.cos(life * 7 + i) * 0.02 * life;
      }
      steamGeometry.attributes.position.needsUpdate = true;
      steam.material.opacity = 0.22 * heat;
    },
  };
}
