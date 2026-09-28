// THE MACHINE: the agent's world. Lights racing tracks through square pylons, halted at square gates.
// At the click every square rounds into a circle and four tracks bend into one loop.
import * as THREE from "three";
import { BAR, BEAT, BEATS, at, clamp, easeInOut, lerp, prng, smooth } from "./story.js";

const TRACK_X = [-3, -1, 1, 3];
const TRACK_Y = 0.3;
const AGENT_SPEED = 14;           // m/s along the straight track
export const RING_RADIUS = 8;
const RING_SPEED = (2 * Math.PI * RING_RADIUS) / BAR; // one lap per bar
const GATE_SIZE = 1.3;
const LAVENDER = new THREE.Color(0xb9a4ff);
const WHITE = new THREE.Color(0xf2efff);
const GREEN = new THREE.Color(0x7fe0a4);
const COLD = new THREE.Color(0xa9b4e8);

// The lead agent only moves while the groove plays; its stops are the story's stops.
const LEAD_WINDOWS = [[0, 8], [11, 12], [12.5, 13], [13.5, 14], [14.5, 15], [27.0, 27.25]];
const LEAD_STOPS = [at(8), at(12), at(13), at(14), at(15), at(27.25)];
// Each gate lifts just after the "y" that releases it; the last one never lifts: Looper replaces it.
const LEAD_RELEASES = [BEATS.firstYes, ...BEATS.yeses, BEATS.habitYes];
const leadRunSeconds = (t) => LEAD_WINDOWS.reduce((sum, [a, b]) => sum + clamp(t - at(a), 0, at(b) - at(a)), 0);
const LEAD_GATES = LEAD_STOPS.map((stop) => AGENT_SPEED * leadRunSeconds(stop) + 0.6);
// Where every agent is halted when the Looper click lands (the cluster the ring forms around).
const HALT = [LEAD_GATES[5], LEAD_GATES[5] - 4, LEAD_GATES[5] + 6, LEAD_GATES[5] - 9];
const RING_CENTER = new THREE.Vector3(0, TRACK_Y, -HALT[0]);

export const morphAmount = (t) => easeInOut(smooth(BEATS.click, BEATS.click + BAR * 0.9, t));

function agentStraightS(index, t) {
  if (index === 0) return AGENT_SPEED * leadRunSeconds(t);
  return HALT[index] - 0.6;
}

// Arc-length along the ring since the click (with a quick spin-up, and a blur at the end).
function ringTravel(t) {
  if (t < BEATS.click) return 0;
  const k = t - BEATS.click;
  const spinUp = k - (1 - Math.exp(-k * 3)) / 3;
  const blur = t > at(39) ? Math.pow((t - at(39)) / BAR, 3) * RING_SPEED * 3 : 0;
  return spinUp * RING_SPEED + blur;
}

function straightPoint(index, s) { return new THREE.Vector3(TRACK_X[index], TRACK_Y, -s); }
function ringPoint(index, arc) {
  const angle = index * Math.PI / 2 + arc / RING_RADIUS;
  return new THREE.Vector3(RING_CENTER.x + Math.sin(angle) * RING_RADIUS, TRACK_Y + 1.2, RING_CENTER.z + Math.cos(angle) * RING_RADIUS);
}

// A point on track `index` at offset `u` (meters) from its halt position, blended by the morph.
function trackPoint(index, u, morph) {
  const line = straightPoint(index, HALT[index] + u);
  if (morph <= 0) return line;
  return line.lerp(ringPoint(index, u), morph);
}

export function agentPosition(index, t) {
  const morph = morphAmount(t);
  if (morph <= 0) return straightPoint(index, agentStraightS(index, t));
  return trackPoint(index, agentStraightS(index, Math.min(t, BEATS.click)) - HALT[index] + ringTravel(t), morph);
}

function roundedSquare(size, radius, segments = 96) {
  const half = size / 2;
  const r = Math.min(radius, half);
  const points = [];
  const corners = [[half - r, half - r, 0], [-(half - r), half - r, Math.PI / 2], [-(half - r), -(half - r), Math.PI], [half - r, -(half - r), Math.PI * 1.5]];
  const perCorner = segments / 4;
  for (const [cx, cy, start] of corners) {
    for (let i = 0; i <= perCorner; i += 1) {
      const a = start + (i / perCorner) * Math.PI / 2;
      points.push(new THREE.Vector3(cx + Math.cos(a) * r, cy + Math.sin(a) * r, 0));
    }
  }
  return new THREE.CatmullRomCurve3(points, true, "centripetal");
}

function glowMaterial(color, intensity = 1) {
  return new THREE.MeshBasicMaterial({ color: color.clone().multiplyScalar(intensity), toneMapped: false, fog: true });
}

export function createMachine() {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x010103);
  scene.fog = new THREE.FogExp2(0x020208, 0.028);

  // Floor: a field of squares.
  const floorMaterial = new THREE.ShaderMaterial({
    uniforms: { uColor: { value: COLD.clone() }, uPulse: { value: 0 }, uFogDensity: { value: 0.028 } },
    transparent: false,
    vertexShader: `varying vec3 vWorld; void main(){ vec4 w = modelMatrix * vec4(position,1.0); vWorld = w.xyz; gl_Position = projectionMatrix * viewMatrix * w; }`,
    fragmentShader: `uniform vec3 uColor; uniform float uPulse; uniform float uFogDensity; varying vec3 vWorld;
      void main(){ vec2 g = abs(fract(vWorld.xz / 2.0 - 0.5) - 0.5) / fwidth(vWorld.xz / 2.0);
        float line = 1.0 - min(min(g.x, g.y), 1.0);
        float d = length(vWorld - cameraPosition);
        float fog = exp(-pow(uFogDensity * d, 2.0));
        vec3 c = uColor * line * (0.18 + 0.5 * uPulse) * fog;
        gl_FragColor = vec4(c, 1.0); }`,
  });
  const floor = new THREE.Mesh(new THREE.PlaneGeometry(1200, 1200), floorMaterial);
  floor.rotation.x = -Math.PI / 2; floor.position.set(0, 0, -300);
  scene.add(floor);

  // Pylons: square columns flanking the tracks, edge-lit.
  const edge = document.createElement("canvas");
  edge.width = edge.height = 128;
  const eg = edge.getContext("2d");
  eg.fillStyle = "#050508"; eg.fillRect(0, 0, 128, 128); eg.strokeStyle = "#ffffff"; eg.lineWidth = 6; eg.strokeRect(3, 3, 122, 122);
  const edgeTexture = new THREE.CanvasTexture(edge);
  const pylonMaterial = new THREE.MeshBasicMaterial({ map: edgeTexture, color: COLD.clone().multiplyScalar(0.55), toneMapped: false });
  const pylonCount = 260;
  const pylons = new THREE.InstancedMesh(new THREE.BoxGeometry(0.5, 1, 0.5), pylonMaterial, pylonCount);
  const random = prng(33);
  const dummy = new THREE.Object3D();
  const pylonBase = [];
  for (let i = 0; i < pylonCount; i += 1) {
    const side = i % 2 === 0 ? -1 : 1;
    const lane = Math.floor(random() * 3);
    const x = side * (5 + lane * 3.5 + random() * 1.5);
    const z = -((i / 2) * 3.2) + 20;
    const h = 2 + random() * 9;
    pylonBase.push({ x, z, h });
    dummy.position.set(x, h / 2, z); dummy.scale.set(1, h, 1); dummy.updateMatrix();
    pylons.setMatrixAt(i, dummy.matrix);
  }
  scene.add(pylons);

  // Tracks: rebuilt while they bend; cached when straight.
  const trackMaterial = glowMaterial(COLD, 0.9);
  const tracks = TRACK_X.map(() => { const mesh = new THREE.Mesh(new THREE.BufferGeometry(), trackMaterial); scene.add(mesh); return mesh; });
  const longTracks = TRACK_X.map((x) => {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(0.05, 0.05, 800), trackMaterial);
    mesh.position.set(x, TRACK_Y, -380);
    scene.add(mesh);
    return mesh;
  });
  let cachedMorph = -1;
  const RING_SPAN = 2 * Math.PI * RING_RADIUS;
  function rebuildTracks(morph) {
    if (Math.abs(morph - cachedMorph) < 1e-4) return;
    cachedMorph = morph;
    tracks.forEach((mesh, index) => {
      const points = [];
      for (let i = 0; i <= 160; i += 1) points.push(trackPoint(index, -RING_SPAN * 0.5 + (i / 160) * RING_SPAN, morph));
      mesh.geometry.dispose();
      mesh.geometry = new THREE.TubeGeometry(new THREE.CatmullRomCurve3(points), 320, 0.04 + morph * 0.04, 6, false);
    });
  }

  // Gates: squares that halt each agent. The lead track has one per stop; far gates form the cage.
  const gateMaterial = glowMaterial(WHITE, 1.4);
  const gates = [];
  const addGate = (track, s, isLead) => {
    const mesh = new THREE.Mesh(new THREE.BufferGeometry(), gateMaterial.clone());
    scene.add(mesh);
    gates.push({ mesh, track, s, isLead, radius: -1 });
  };
  LEAD_GATES.forEach((s) => addGate(0, s, true));
  [1, 2, 3].forEach((track) => addGate(track, HALT[track], true));
  for (let track = 0; track < 4; track += 1) for (let k = 1; k <= 16; k += 1) addGate(track, HALT[track] + k * 11 + track * 2.5, false);
  function gateGeometry(gate, radius) {
    if (Math.abs(radius - gate.radius) < 1e-3) return;
    gate.radius = radius;
    gate.mesh.geometry.dispose();
    gate.mesh.geometry = new THREE.TubeGeometry(roundedSquare(GATE_SIZE, radius), 128, 0.035 + radius * 0.02, 6, true);
  }
  gates.forEach((gate) => gateGeometry(gate, 0));

  // Agents: a bright head and a trail of glyph cubes.
  const TRAIL = 70;
  const agents = TRACK_X.map((_, index) => {
    const head = new THREE.Mesh(new THREE.SphereGeometry(0.16, 20, 14), glowMaterial(WHITE, 3));
    const light = new THREE.PointLight(0xc9c0ff, 3, 8, 2);
    head.add(light);
    scene.add(head);
    const trail = new THREE.InstancedMesh(new THREE.BoxGeometry(0.07, 0.07, 0.07), new THREE.MeshBasicMaterial({ color: 0xffffff, toneMapped: false }), TRAIL);
    trail.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(TRAIL * 3), 3);
    scene.add(trail);
    return { head, light, trail, index };
  });

  // The orb rises at the center of the loop; light beams turn with the kick.
  const orb = new THREE.Group();
  const orbGlass = new THREE.Mesh(new THREE.SphereGeometry(2.4, 64, 48), new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 0.02, transmission: 1, thickness: 2, ior: 1.4, clearcoat: 1 }));
  const orbCore = new THREE.Mesh(new THREE.TorusKnotGeometry(0.9, 0.18, 220, 20, 2, 3), new THREE.MeshStandardMaterial({ color: 0x9d95b8, emissive: LAVENDER, emissiveIntensity: 2, metalness: 0.9, roughness: 0.2 }));
  const orbLight = new THREE.PointLight(LAVENDER, 0, 40, 1.6);
  orb.add(orbGlass, orbCore, orbLight);
  orb.position.copy(RING_CENTER).add(new THREE.Vector3(0, 2.2, 0));
  scene.add(orb);
  const beams = new THREE.Group();
  for (let i = 0; i < 12; i += 1) {
    const beam = new THREE.Mesh(new THREE.BoxGeometry(0.05, 0.05, 60), new THREE.MeshBasicMaterial({ color: LAVENDER.clone().multiplyScalar(1.6), transparent: true, opacity: 0.5, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false }));
    beam.geometry.translate(0, 0, 30);
    beam.rotation.set(-0.35 - (i % 3) * 0.25, (i / 12) * Math.PI * 2, 0);
    beams.add(beam);
  }
  orb.add(beams);
  const ambient = new THREE.AmbientLight(0x404060, 0.4);
  scene.add(ambient);

  // Check pulses: a green ring expands where an agent passes a round gate on the loop.
  const pulses = Array.from({ length: 4 }, () => {
    const mesh = new THREE.Mesh(new THREE.TorusGeometry(1, 0.03, 8, 64), new THREE.MeshBasicMaterial({ color: GREEN.clone().multiplyScalar(2), transparent: true, opacity: 0, toneMapped: false, depthWrite: false }));
    scene.add(mesh);
    return mesh;
  });

  return {
    scene,
    agentPosition,
    ringCenter: RING_CENTER,
    orbPosition: orb.position,
    leadGate: (k) => straightPoint(0, LEAD_GATES[k]),
    update(t, sub) {
      const morph = morphAmount(t);
      const on = t >= BEATS.click;
      // The crane climbs out of the fog so the cage can be read from above.
      const fogDensity = lerp(0.028, 0.009, smooth(at(16), at(18.5), t) * (1 - smooth(at(19), at(19.1), t)));
      scene.fog.density = fogDensity; floorMaterial.uniforms.uFogDensity.value = fogDensity;
      rebuildTracks(morph);
      longTracks.forEach((mesh) => { mesh.visible = morph < 0.4; });
      trackMaterial.color.copy(COLD).lerp(LAVENDER, morph).multiplyScalar(0.9 + (on ? sub * 1.5 : 0));
      floorMaterial.uniforms.uColor.value.copy(COLD).lerp(LAVENDER, morph);
      floorMaterial.uniforms.uPulse.value = on ? sub : 0;
      pylonMaterial.color.copy(COLD).lerp(LAVENDER, morph).multiplyScalar(0.5 + (on ? sub * 0.8 : 0));

      // Gates: lead gates lift once "y" is typed; every gate rounds into a circle at the click.
      gates.forEach((gate) => {
        gateGeometry(gate, morph * GATE_SIZE / 2);
        let position = straightPoint(gate.track, gate.s);
        position.y = TRACK_Y + GATE_SIZE / 2 - 0.1;
        const leadIndex = gate.track === 0 && gate.isLead ? LEAD_GATES.indexOf(gate.s) : -1;
        if (leadIndex >= 0 && leadIndex < LEAD_RELEASES.length) position.y += smooth(LEAD_RELEASES[leadIndex] + 0.15, LEAD_RELEASES[leadIndex] + 0.5, t) * 3;
        const hit = leadIndex >= 0 && t >= LEAD_STOPS[leadIndex] ? Math.exp(-(t - LEAD_STOPS[leadIndex]) * 3) : 0;
        const blink = !on && gate.isLead && t > at(8) ? (Math.floor(t / (BEAT / 2)) % 2 ? 1 : 0.55) : 1;
        if (morph > 0) {
          // The four halting gates become the loop's checkpoints; the cage gates fade out.
          const ringSpot = ringPoint(gate.track, gate.isLead ? RING_RADIUS * Math.PI / 4 : 0);
          position = position.lerp(ringSpot, gate.isLead ? morph : 0);
          gate.mesh.visible = gate.isLead ? (gate.track !== 0 || gate.s === LEAD_GATES[5]) : morph < 0.98;
          const tangentAngle = gate.track * Math.PI / 2 + Math.PI / 4;
          gate.mesh.rotation.y = gate.isLead ? lerp(0, tangentAngle + Math.PI / 2, morph) : 0;
        } else {
          gate.mesh.visible = true; gate.mesh.rotation.y = 0;
        }
        gate.mesh.position.copy(position);
        const fade = gate.isLead ? 1 : 1 - morph;
        gate.mesh.material.color.copy(WHITE).lerp(LAVENDER, morph).multiplyScalar((0.75 + hit * 4) * blink * fade + (on ? sub * 1.5 : 0));
      });

      // Agents and their trails; trails pile up and freeze when an agent is stopped.
      agents.forEach((agent) => {
        const head = agentPosition(agent.index, t);
        agent.head.position.copy(head);
        const moving = on || (agent.index === 0 && LEAD_WINDOWS.some(([a, b]) => t >= at(a) && t < at(b)));
        const brightness = moving ? 1.7 : 1.1 + (Math.floor(t / (BEAT / 2)) % 2) * 0.6;
        agent.head.material.color.copy(WHITE).lerp(LAVENDER, morph * 0.4).multiplyScalar(brightness + (on ? sub * 2 : 0));
        agent.light.intensity = moving ? 2.2 + sub * 3 : 1.0;
        const spacing = moving ? 0.22 + (t > at(39) ? (t - at(39)) * 0.6 : 0) : 0.035;
        for (let k = 0; k < TRAIL; k += 1) {
          const back = agent.index === 0 || on
            ? (on ? trackPoint(agent.index, agentStraightS(agent.index, BEATS.click) - HALT[agent.index] + ringTravel(t) - k * spacing, morph) : straightPoint(agent.index, agentStraightS(agent.index, t) - k * spacing))
            : straightPoint(agent.index, HALT[agent.index] - 0.6 - k * spacing);
          const jitter = moving ? 0 : 0.05;
          dummy.position.set(back.x + Math.sin(k * 12.9) * jitter, back.y + Math.cos(k * 7.1) * jitter * 1.5, back.z);
          dummy.rotation.set(k + t * (moving ? 3 : 0), k * 0.7, 0);
          const scale = 1 - k / TRAIL;
          dummy.scale.setScalar(scale * (moving ? 1 : 1.4));
          dummy.updateMatrix();
          agent.trail.setMatrixAt(k, dummy.matrix);
          const color = (on ? LAVENDER : COLD).clone().lerp(WHITE, scale * 0.6).multiplyScalar(0.6 + scale * 1.8);
          agent.trail.instanceColor.setXYZ(k, color.r, color.g, color.b);
        }
        agent.trail.instanceMatrix.needsUpdate = true;
        agent.trail.instanceColor.needsUpdate = true;
      });

      // The orb rises out of the floor as the loop closes.
      const rise = smooth(BEATS.click, BEATS.click + BAR, t);
      orb.visible = rise > 0;
      orb.position.y = lerp(-3, 2.6, easeInOut(rise));
      orbCore.rotation.set(t * 0.4, t * 0.9, 0);
      orbCore.material.emissiveIntensity = 1.5 + sub * 4;
      orbLight.intensity = rise * (60 + sub * 120);
      beams.rotation.y = t * 0.35;
      beams.children.forEach((beam, i) => { beam.material.opacity = rise * (0.25 + 0.5 * sub) * (0.6 + 0.4 * Math.sin(i * 1.7 + t)); });

      // A check passes each time an agent flies through its round gate on the loop.
      const circumference = 2 * Math.PI * RING_RADIUS;
      const firstPass = RING_RADIUS * Math.PI / 4 + 0.6;
      pulses.forEach((pulse, agentIndex) => {
        const travel = ringTravel(t) - firstPass;
        const since = ((travel % circumference) + circumference) % circumference / RING_SPEED;
        pulse.visible = on && morph > 0.95 && travel > 0 && t < at(40) && since < 0.8;
        if (!pulse.visible) return;
        pulse.position.copy(ringPoint(agentIndex, RING_RADIUS * Math.PI / 4));
        pulse.rotation.y = agentIndex * Math.PI / 2 + Math.PI / 4 + Math.PI / 2;
        pulse.scale.setScalar(0.7 + since * 3);
        pulse.material.opacity = (1 - since / 0.8) * 0.9;
      });
    },
  };
}
