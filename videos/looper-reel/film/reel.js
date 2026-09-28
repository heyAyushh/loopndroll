// The Reel's world: a drawing breathing on a plane, and 42k particles made of its light that carry each
// scene into the next. A Stop freezes everything mid-flight. Looper turns the particles into a loop.
import * as THREE from "three";
import { BAR, BEAT, FREEZES, SCENES, at, clamp, easeInOut, easeOut, lerp, prng, smooth } from "./story.js";

export const WIDTH = 1080, HEIGHT = 1920;
const PLANE_W = 9, PLANE_H = 16;
export const CAMERA_Z = (PLANE_H / 2) / Math.tan(THREE.MathUtils.degToRad(15));
const ORB_CENTER = new THREE.Vector3(0, 0.6, 8); // moves up for the end card so the name sits clear below it
const RING_RADIUS = 3.0;
const RING_TILT = 1.15;

// Story time: the clock pauses inside every freeze.
export function storyTime(t) {
  let frozen = 0;
  for (const [a, b] of FREEZES) frozen += clamp(t - a, 0, b - a);
  return t - frozen;
}
export const isFrozen = (t) => FREEZES.some(([a, b]) => t >= a && t < b);

const imageShader = {
  vertexShader: `varying vec2 vUv; void main(){ vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
  fragmentShader: `
    uniform sampler2D tPlate; uniform vec2 uCrop; uniform float uZoom; uniform vec2 uFocus;
    uniform float uOpacity, uFatigue, uGlitch, uTime, uFrozen;
    varying vec2 vUv;
    float hash(vec2 p){ p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
    vec3 sampleCrop(vec2 s){
      vec2 u = uFocus + (s - uFocus) / uZoom;          // screen -> crop, zoomed about the focus
      if (u.x < 0.0 || u.x > 1.0 || u.y < 0.0 || u.y > 1.0) return vec3(0.0);
      return texture2D(tPlate, vec2(uCrop.x + u.x * uCrop.y, 1.0 - u.y)).rgb;
    }
    void main(){
      vec2 s = vec2(vUv.x, 1.0 - vUv.y);
      float frame = floor(uTime * 12.0);
      // Fatigue: the image boils and smears, like eyes that have been open too long.
      s += (vec2(hash(s * 40.0 + frame), hash(s * 40.0 + frame + 7.0)) - 0.5) * uFatigue * 0.004;
      // Glitch at a Stop: slices tear sideways and the channels split.
      float row = floor(s.y * 48.0);
      float tear = step(0.72, hash(vec2(row, floor(uTime * 24.0)))) * (hash(vec2(row * 3.1, frame)) - 0.5) * uGlitch * 0.12;
      s.x += tear;
      vec3 c = vec3(sampleCrop(s + vec2(0.006 * uGlitch, 0.0)).r, sampleCrop(s).g, sampleCrop(s - vec2(0.006 * uGlitch, 0.0)).b);
      if (uFatigue > 0.01) {
        vec3 smear = (sampleCrop(s + vec2(0.003, 0.002) * uFatigue) + sampleCrop(s - vec2(0.003, 0.002) * uFatigue)) * 0.5;
        c = mix(c, smear * 0.9, uFatigue * 0.5);
      }
      float grey = dot(c, vec3(0.299, 0.587, 0.114));
      c = mix(c, vec3(grey) * 0.8, uFrozen * 0.55);    // a frozen world loses its colour
      c += (hash(s * vec2(1080.0, 1920.0) + frame) - 0.5) * 0.05;
      gl_FragColor = vec4(c * uOpacity, 1.0);
    }`,
};

const pointShader = {
  vertexShader: `
    attribute vec3 tint; attribute float alpha; attribute float size;
    varying vec3 vTint; varying float vAlpha;
    void main(){ vTint = tint; vAlpha = alpha; vec4 view = modelViewMatrix * vec4(position, 1.0);
      gl_PointSize = size * ${(HEIGHT / (2 * Math.tan(Math.PI / 12))).toFixed(1)} / -view.z; gl_Position = projectionMatrix * view; }`,
  fragmentShader: `
    varying vec3 vTint; varying float vAlpha;
    void main(){ float d = length(gl_PointCoord - 0.5); float g = smoothstep(0.5, 0.0, d); gl_FragColor = vec4(vTint * vAlpha * g, vAlpha * g); }`,
};

const LAVENDER = new THREE.Color(0.7, 0.6, 1.0);
const WARM = new THREE.Color(1.0, 0.8, 0.62);

export function createReel({ plates, crops, points, count }) {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x020203);
  const textures = {};
  for (const [name, image] of Object.entries(plates)) {
    const texture = new THREE.Texture(image);
    texture.colorSpace = THREE.SRGBColorSpace; texture.needsUpdate = true; texture.anisotropy = 8;
    textures[name] = texture;
  }
  const imageMaterial = new THREE.ShaderMaterial({
    uniforms: { tPlate: { value: null }, uCrop: { value: new THREE.Vector2() }, uZoom: { value: 1 }, uFocus: { value: new THREE.Vector2(0.5, 0.5) }, uOpacity: { value: 1 }, uFatigue: { value: 0 }, uGlitch: { value: 0 }, uTime: { value: 0 }, uFrozen: { value: 0 } },
    ...imageShader, depthWrite: false,
  });
  const plane = new THREE.Mesh(new THREE.PlaneGeometry(PLANE_W, PLANE_H), imageMaterial);
  scene.add(plane);

  // Screens drawn into the drawings (monitor, phone) sit on the plane as their own textured quads.
  const screenCanvas = document.createElement("canvas");
  screenCanvas.width = 1024; screenCanvas.height = 512;
  const screenTexture = new THREE.CanvasTexture(screenCanvas);
  const screenMesh = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.MeshBasicMaterial({ map: screenTexture, transparent: true, depthWrite: false, toneMapped: false, side: THREE.DoubleSide }));
  screenMesh.position.z = 0.01;
  scene.add(screenMesh);

  // Particles.
  const random = prng(11);
  const seeds = Array.from({ length: count }, () => ({ delay: random(), lift: 2 + random() * 9, swirl: (random() - 0.5) * 2, twinkle: random() * 6.28, fall: 0.4 + random(), keep: random() }));
  const positions = new Float32Array(count * 3), tints = new Float32Array(count * 3), alphas = new Float32Array(count), sizes = new Float32Array(count);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
  geometry.setAttribute("tint", new THREE.BufferAttribute(tints, 3));
  geometry.setAttribute("alpha", new THREE.BufferAttribute(alphas, 1));
  geometry.setAttribute("size", new THREE.BufferAttribute(sizes, 1));
  const cloud = new THREE.Points(geometry, new THREE.ShaderMaterial({ ...pointShader, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending }));
  cloud.frustumCulled = false;
  scene.add(cloud);

  // The orb (Looper's mark: glass with a chrome S-fold), lit only when Looper arrives.
  const orb = new THREE.Group();
  orb.add(new THREE.Mesh(new THREE.SphereGeometry(1.45, 96, 72), new THREE.MeshPhysicalMaterial({ color: 0xffffff, roughness: 0.02, transmission: 1, thickness: 1.6, ior: 1.46, clearcoat: 1, iridescence: 0.25, envMapIntensity: 1.1 })));
  const fold = [];
  for (let i = 0; i <= 64; i += 1) { const k = i / 64 * 2 - 1; fold.push(new THREE.Vector3(k * 0.9, Math.sin(k * Math.PI) * 0.48 - k * 0.12, Math.cos(k * Math.PI * 0.5) * 0.22)); }
  const ribbon = new THREE.Mesh(new THREE.TubeGeometry(new THREE.CatmullRomCurve3(fold), 220, 0.16, 40, false), new THREE.MeshPhysicalMaterial({ color: 0xeeeef6, metalness: 1, roughness: 0.1, clearcoat: 1 }));
  ribbon.scale.set(1, 1, 0.42); ribbon.rotation.z = -0.35;
  orb.add(ribbon);
  orb.position.copy(ORB_CENTER);
  orb.visible = false;
  scene.add(orb);

  // Where each scene's points sit on screen (world units), given its zoom at story-time `tau`.
  const sceneState = (index, tau) => {
    const [name, startBar, endBar, options] = SCENES[index];
    const k = clamp((tau - storyTime(at(startBar))) / Math.max(0.001, storyTime(at(endBar)) - storyTime(at(startBar))));
    const [z0, z1] = options.zoom ?? [1, 1];
    return { name, options, zoom: lerp(z0, z1, easeInOut(k)), focus: options.focus ?? [0.5, 0.5], k };
  };
  const toWorld = (u, v, state, out) => {
    const sx = state.focus[0] + (u - state.focus[0]) * state.zoom;
    const sy = state.focus[1] + (v - state.focus[1]) * state.zoom;
    return out.set((sx - 0.5) * PLANE_W, (0.5 - sy) * PLANE_H, 0);
  };
  const ringPoint = (i, t, out) => {
    const theta = (i / count) * Math.PI * 2 + t * 1.2 + seeds[i].swirl * 0.03;
    const r = RING_RADIUS + seeds[i].swirl * 0.18;
    out.set(Math.cos(theta) * r, (seeds[i].keep - 0.5) * 0.12, Math.sin(theta) * r);
    const c = Math.cos(RING_TILT), s = Math.sin(RING_TILT);
    return out.set(out.x, out.y * c - out.z * s, out.y * s + out.z * c).add(ORB_CENTER);
  };
  // A scene's point i in world space (images: its sampled point; ring: on the loop; dark: nowhere).
  const a = new THREE.Vector3(), b = new THREE.Vector3(), p = new THREE.Vector3();
  // Prepared once per frame: a function placing point i of scene `index` as seen at story-time `tau`.
  const placer = (index, tau, t) => {
    const state = sceneState(index, tau);
    if (state.name === "ring") return (i, out) => ringPoint(i, t, out);
    if (state.name === "dark") return (i, out) => out.set(ORB_CENTER.x + seeds[i].swirl * 0.2, ORB_CENTER.y, ORB_CENTER.z);
    const set = points[state.name];
    return (i, out) => toWorld(set[i * 5], set[i * 5 + 1], state, out);
  };
  const colourOf = ({ name, options }, i, out) => {
    if (options.ink === "lavender") return out.copy(LAVENDER).multiplyScalar(0.8 + seeds[i].keep * 0.4);
    if (options.ink === "warm" && points[name]) { const set = points[name]; return out.setRGB(set[i * 5 + 2], set[i * 5 + 3], set[i * 5 + 4]).lerp(WARM, 0.4); }
    if (!points[name]) return out.copy(LAVENDER);
    const set = points[name];
    return out.setRGB(set[i * 5 + 2], set[i * 5 + 3], set[i * 5 + 4]);
  };

  const colourA = new THREE.Color(), colourB = new THREE.Color();
  const screen = screenCanvas.getContext("2d");
  return {
    scene,
    orb,
    screen: { canvas: screenCanvas, context: screen, texture: screenTexture, mesh: screenMesh },
    toWorld,
    sceneAt(t) { let index = SCENES.findIndex(([, s, e]) => t >= at(s) && t < at(e)); return index < 0 ? SCENES.length - 1 : index; },
    sceneState,
    update(t, sub) {
      const tau = storyTime(t);
      const index = this.sceneAt(t);
      const [name, startBar, , options] = SCENES[index];
      const state = sceneState(index, tau);
      const morphStart = storyTime(at(startBar));
      const morph = clamp((tau - morphStart) / options.morph);          // 0..1 while particles travel
      const frozen = isFrozen(t);
      ORB_CENTER.y = options.end ? lerp(0.6, 2.9, easeInOut(smooth(at(17), at(17.6), t))) : 0.6;
      orb.position.copy(ORB_CENTER);

      // ---------- the drawing ----------
      const hasImage = !!points[name] && name !== "dark";
      if (hasImage) {
        imageMaterial.uniforms.tPlate.value = textures[crops[name].plate];
        imageMaterial.uniforms.uCrop.value.set(crops[name].x, crops[name].w);
      }
      imageMaterial.uniforms.uZoom.value = state.zoom;
      imageMaterial.uniforms.uFocus.value.set(state.focus[0], state.focus[1]);
      // The drawing resolves as its particles land, and dissolves as they leave for the next scene.
      const next = SCENES[index + 1];
      const leaving = next ? smooth(at(next[1]) - 0.08, at(next[1]), t) : 0; // real time: a freeze must not hide the drawing
      imageMaterial.uniforms.uOpacity.value = hasImage ? smooth(0.45, 1.0, morph) * (1 - leaving) * (options.dust ? 1 - smooth(at(7.3), at(8), t) * 0.8 : 1) : 0;
      imageMaterial.uniforms.uFatigue.value = options.fatigue ?? 0;
      const lastFreeze = FREEZES.find(([s, e]) => t >= s && t < e);
      imageMaterial.uniforms.uGlitch.value = lastFreeze ? Math.max(0, 1 - (t - lastFreeze[0]) / 0.18) : 0;
      imageMaterial.uniforms.uFrozen.value = frozen ? 1 : 0;
      imageMaterial.uniforms.uTime.value = frozen ? lastFreeze[0] : t;
      plane.visible = hasImage;
      const shake = options.shake ? Math.max(0, 1 - (t - at(startBar)) * 3) : 0;
      plane.position.set(Math.sin(t * 90) * shake * 0.12, Math.cos(t * 70) * shake * 0.12, 0);

      // ---------- the particles ----------
      const previous = Math.max(0, index - 1);
      const burst = options.from === "burst";
      const placeHere = placer(index, tau, t);
      const placeBefore = placer(previous, storyTime(at(startBar)), t);
      const inkHere = sceneState(index, 0), inkBefore = sceneState(previous, 0);
      for (let i = 0; i < count; i += 1) {
        const seed = seeds[i];
        const local = clamp((morph - seed.delay * 0.35) / 0.65);
        const e = easeInOut(local);
        placeHere(i, b);
        if (burst) a.set(seed.swirl * 30, (seed.delay - 0.5) * 50, CAMERA_Z - 2 - seed.lift);
        else placeBefore(i, a);
        p.lerpVectors(a, b, e);
        // The flight: an arc toward the lens and a sideways swirl, strongest mid-journey.
        const arc = Math.sin(Math.PI * e);
        p.z += arc * seed.lift * (name === "ring" ? 0.3 : 1);
        p.x += arc * seed.swirl * 2.2; p.y += arc * (seed.delay - 0.5) * 2.5;
        if (options.dust) { const fall = smooth(at(7.2), at(8), t); p.y -= fall * fall * seed.fall * 9; }
        positions[i * 3] = p.x; positions[i * 3 + 1] = p.y; positions[i * 3 + 2] = p.z;
        colourOf(inkBefore, i, colourA); colourOf(inkHere, i, colourB);
        colourA.lerp(colourB, e);
        tints[i * 3] = colourA.r; tints[i * 3 + 1] = colourA.g; tints[i * 3 + 2] = colourA.b;
        // In flight they are the image; at rest they are its faint sparkle.
        const flying = 1 - smooth(0.8, 1.0, local) + (local < 0.02 && !burst ? 0.9 : 0);
        const rest = name === "ring" ? 0.3 + 0.2 * sub : 0.1 + 0.07 * Math.sin(t * 3 + seed.twinkle);
        const fatigueDropout = options.fatigue && seed.keep < options.fatigue * 0.6 ? 0.25 : 1;
        alphas[i] = clamp(Math.max(flying * 0.85, rest)) * fatigueDropout * (name === "dark" ? 0.2 * (1 - morph) : 1);
        sizes[i] = name === "ring" ? 0.07 + seed.keep * 0.05 : 0.045 + seed.keep * 0.04;
      }
      for (const key of ["position", "tint", "alpha", "size"]) geometry.attributes[key].needsUpdate = true;

      // ---------- the orb ----------
      const orbOn = name === "ring" || (name === "dark");
      orb.visible = orbOn;
      if (orbOn) {
        const since = t - at(startBar);
        scene.environmentIntensity = name === "dark" ? 0.12 * smooth(0, BAR / 2, since) : lerp(0.2, 0.75, smooth(0, 0.6, since));
        scene.environmentRotation.set(0.2, since * 0.6 - 1.2, 0);
        orb.rotation.set(0.12, Math.sin(t * 0.8) * 0.3, 0.04);
        orb.scale.setScalar(options.end ? 0.85 : 1);
      }
      return { name, options, state, morph, frozen, tau };
    },
  };
}
