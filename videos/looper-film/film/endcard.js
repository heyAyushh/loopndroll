// The title world: the blinking square that opens the film, and rounds into the Looper orb at the end.
import * as THREE from "three";
import { BAR, BEAT, BEATS, at, easeInOut, easeOut, smooth } from "./story.js";

const SANS = '-apple-system, "SF Pro Display", "Helvetica Neue", sans-serif';
const MONO = '"SF Mono", Menlo, monospace';
const LAVENDER = new THREE.Color(0xb9a4ff);
const WHITE = new THREE.Color(0xf4f2ff);

function roundedSquare(size, radius, segments = 128) {
  const half = size / 2;
  const r = Math.max(0.0001, Math.min(radius, half));
  const points = [];
  const corners = [[half - r, half - r, 0], [-(half - r), half - r, Math.PI / 2], [-(half - r), -(half - r), Math.PI], [half - r, -(half - r), Math.PI * 1.5]];
  for (const [cx, cy, start] of corners) for (let i = 0; i <= segments / 4; i += 1) {
    const a = start + (i / (segments / 4)) * Math.PI / 2;
    points.push(new THREE.Vector3(cx + Math.cos(a) * r, cy + Math.sin(a) * r, 0));
  }
  return new THREE.CatmullRomCurve3(points, true, "centripetal");
}

function textPlane(lines, width, height) {
  const scale = 2;
  const element = document.createElement("canvas");
  element.width = width * scale; element.height = height * scale;
  const g = element.getContext("2d");
  g.scale(scale, scale);
  g.textAlign = "center"; g.textBaseline = "middle";
  for (const { text, font, color, y, spacing = 0 } of lines) {
    g.font = font; g.fillStyle = color;
    if ("letterSpacing" in g) g.letterSpacing = `${spacing}px`;
    g.fillText(text, width / 2, y);
  }
  const texture = new THREE.CanvasTexture(element);
  texture.colorSpace = THREE.SRGBColorSpace;
  const material = new THREE.MeshBasicMaterial({ map: texture, transparent: true, opacity: 0, depthWrite: false, toneMapped: false });
  return new THREE.Mesh(new THREE.PlaneGeometry(width / 270, height / 270), material);
}

export function createTitles(orbImage) {
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x000000);
  const camera = new THREE.PerspectiveCamera(30, 1920 / 1080, 0.1, 100);
  camera.position.set(0, 0, 10);

  const frameMaterial = new THREE.MeshBasicMaterial({ color: WHITE.clone(), toneMapped: false });
  const frame = new THREE.Mesh(new THREE.BufferGeometry(), frameMaterial);
  scene.add(frame);
  let cachedRadius = -1;
  const setShape = (size, radius, thickness) => {
    const key = `${size.toFixed(3)}:${radius.toFixed(3)}:${thickness.toFixed(3)}`;
    if (key === cachedRadius) return;
    cachedRadius = key;
    frame.geometry.dispose();
    frame.geometry = new THREE.TubeGeometry(roundedSquare(size, radius), 160, thickness, 8, true);
  };
  const cursorFill = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.MeshBasicMaterial({ color: WHITE.clone(), toneMapped: false, transparent: true }));
  scene.add(cursorFill);

  const orbTexture = new THREE.Texture(orbImage);
  orbTexture.colorSpace = THREE.SRGBColorSpace; orbTexture.needsUpdate = true;
  const orb = new THREE.Mesh(new THREE.PlaneGeometry(1.5, 1.5), new THREE.MeshBasicMaterial({ map: orbTexture, transparent: true, opacity: 0, depthWrite: false }));
  orb.position.set(0, 1.1, -0.02);
  scene.add(orb);
  const halo = document.createElement("canvas");
  halo.width = halo.height = 512;
  const hg = halo.getContext("2d");
  const gradient = hg.createRadialGradient(256, 256, 0, 256, 256, 256);
  gradient.addColorStop(0, "rgba(185,164,255,0.55)"); gradient.addColorStop(0.45, "rgba(185,164,255,0.16)"); gradient.addColorStop(1, "rgba(185,164,255,0)");
  hg.fillStyle = gradient; hg.fillRect(0, 0, 512, 512);
  const glow = new THREE.Mesh(new THREE.PlaneGeometry(4.4, 4.4), new THREE.MeshBasicMaterial({ map: new THREE.CanvasTexture(halo), transparent: true, opacity: 0, blending: THREE.AdditiveBlending, depthWrite: false, toneMapped: false }));
  glow.position.set(0, 1.1, -0.05);
  scene.add(glow);

  const wordmark = textPlane([{ text: "LOOPER", font: `700 150px ${SANS}`, color: "#f4f2ff", y: 90, spacing: 18 }], 1400, 180);
  wordmark.position.set(0, -0.5, 0);
  const tagline = textPlane([{ text: "Leave the desk. Keep the loop.", font: `500 64px ${SANS}`, color: "#b9a4ff", y: 50 }], 1600, 100);
  tagline.position.set(0, -1.12, 0);
  const subline = textPlane([
    { text: "Keeps your coding agents moving until the work is actually done.", font: `400 34px ${SANS}`, color: "#d8d6e4", y: 30 },
    { text: "CODEX · CLAUDE CODE · CURSOR · ZED · DEVIN · GROK BUILD", font: `500 24px ${MONO}`, color: "#9c9ab0", y: 92, spacing: 3 },
  ], 1600, 120);
  subline.position.set(0, -1.62, 0);
  const url = textPlane([{ text: "looper.fyi", font: `600 44px ${SANS}`, color: "#f4f2ff", y: 36 }], 600, 72);
  url.position.set(0, -2.1, 0);
  scene.add(wordmark, tagline, subline, url);

  return {
    scene,
    camera,
    update(t, sub) {
      const opening = t < BEATS.endCard;
      // The cursor: a blinking square on the half-beat, the Stop before we know it.
      const blink = Math.floor(t / (BEAT / 2)) % 2 === 0 ? 1 : 0;
      if (opening) {
        setShape(0.36, 0, 0.012);
        frame.position.set(0, 0, 0);
        frameMaterial.color.copy(WHITE).multiplyScalar(blink ? 1.6 : 0.25);
        cursorFill.visible = true;
        cursorFill.scale.setScalar(0.36); cursorFill.material.opacity = blink * 0.9;
        // Slow drift, then an accelerating push into the square: it match-cuts to the lens.
        const push = Math.pow(smooth(at(0.9), at(1.5), t), 2.2);
        camera.position.set(0, 0, 10 - smooth(0, at(1.5), t) * 1.2 - push * 7.4);
        [orb, glow, wordmark, tagline, subline, url].forEach((mesh) => { mesh.material.opacity = 0; });
        return;
      }
      const k = t - BEATS.endCard;
      const round = easeInOut(smooth(BAR * 0.25, BAR * 0.75, k));
      const grow = easeOut(smooth(BAR * 0.25, BAR * 1.0, k));
      const size = 0.36 + grow * 1.44;
      setShape(size, round * size / 2, 0.012 + grow * 0.01);
      frame.position.set(0, grow * 1.1, 0);
      frameMaterial.color.copy(WHITE).lerp(LAVENDER, round).multiplyScalar(k < BAR * 0.25 ? (blink ? 1.6 : 0.3) : 1.3 + sub * 1.5);
      cursorFill.visible = k < BAR * 0.25;
      cursorFill.scale.setScalar(0.36); cursorFill.material.opacity = blink * 0.9;
      const reveal = (startBars, lengthBars = 0.35) => smooth(BAR * startBars, BAR * (startBars + lengthBars), k);
      orb.material.opacity = reveal(0.8, 0.5);
      orb.rotation.z = -k * 0.05;
      glow.material.opacity = reveal(0.8, 0.6) * (0.6 + 0.4 * sub);
      wordmark.material.opacity = reveal(1.4);
      wordmark.position.y = -0.5 - (1 - reveal(1.4)) * 0.15;
      tagline.material.opacity = reveal(1.9);
      subline.material.opacity = reveal(2.4);
      url.material.opacity = reveal(2.9);
      camera.position.set(0, -0.35, 10.5 - k * 0.12);
      camera.lookAt(0, -0.35, 0);
    },
  };
}
