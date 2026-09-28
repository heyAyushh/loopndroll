// The pencil: turns a composed page into graphite (or cyanotype, or watercolour) on paper.
//   - draw-on / un-draw: strongest strokes appear first, shading after; reversal is the Stop
//   - boil: the line is re-drawn on twos (12 fps), like hand animation
//   - fatigue: smudge, jitter, eraser ghosts — the drawing gets worse every time they type "y"
//   - ink: the lavender layer, printed like a risograph (halftone + misregistration) on the kick
import * as THREE from "three";

export const WIDTH = 1920;
export const HEIGHT = 1080;

const vertexShader = `varying vec2 vUv; void main(){ vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }`;

const fragmentShader = `
precision highp float;
uniform sampler2D tPage, tInk, tPaper;
uniform float uReveal, uBoil, uFatigue, uTime, uRiso, uFlash, uFade, uMode, uInkGlow, uShake;
uniform vec3 uPaper, uInkColor, uFlashColor;
varying vec2 vUv;

float hash(vec2 p){ p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p){ vec2 i = floor(p), f = fract(p); f = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), f.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), f.x), f.y); }
float fbm(vec2 p){ float v = 0.0, a = 0.5; for (int i = 0; i < 4; i++){ v += a * noise(p); p *= 2.03; a *= 0.5; } return v; }
mat2 rot(float a){ float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }

void main(){
  vec2 aspect = vec2(${WIDTH}.0 / ${HEIGHT}.0, 1.0);
  float boilFrame = floor(uTime * 12.0);
  vec2 uv = vUv;
  // Camera shake (the jolt awake).
  uv += (vec2(noise(vec2(uTime * 40.0, 1.0)), noise(vec2(1.0, uTime * 40.0))) - 0.5) * uShake * 0.02;
  // Boil: every twelfth of a second the whole drawing is re-traced, never quite the same.
  vec2 boil = (vec2(noise(uv * 14.0 + boilFrame * 7.13), noise(uv * 14.0 + 31.7 + boilFrame * 3.71)) - 0.5) * (0.0012 + uFatigue * 0.004) * uBoil;
  vec3 page = texture2D(tPage, uv + boil).rgb;
  // Fatigue: graphite smudged by a tired hand; darker, softer, with eraser ghosts.
  if (uFatigue > 0.001) {
    vec3 smear = vec3(0.0);
    for (int i = 0; i < 6; i++) { float o = float(i) - 2.5; smear += texture2D(tPage, uv + boil + vec2(o * 0.0016, o * 0.0009) * uFatigue).rgb; }
    smear /= 6.0;
    page = mix(page, smear * smear * 1.15, uFatigue * 0.55);
    float ghost = smoothstep(0.62, 0.8, fbm(uv * aspect * 3.0 + boilFrame * 0.01)) * uFatigue;
    page = mix(page, uPaper * 0.92, ghost * 0.45);
  }
  float luminance = dot(page, vec3(0.299, 0.587, 0.114));
  float paperLuminance = dot(uPaper, vec3(0.299, 0.587, 0.114));
  // How much "mark" is here: graphite = darker than paper; cyanotype = lighter than the blue.
  float mark = uMode > 0.5 && uMode < 1.5 ? clamp((luminance - paperLuminance) * 2.6, 0.0, 1.0) : clamp((paperLuminance - luminance) * 1.6, 0.0, 1.0);
  // Stroke order: hatch-shaped noise (long thin strokes at an angle) decides when each mark lands.
  vec2 strokeSpace = rot(0.62) * (uv * aspect) * vec2(260.0, 9.0);
  float hatch = noise(strokeSpace) * 0.7 + noise(uv * aspect * 5.0) * 0.3;
  if (uMode > 1.5) hatch = fbm(uv * aspect * 3.5); // watercolour blooms outward in soft blobs
  float threshold = mix(hatch, 1.0 - mark, 0.62);
  float reveal = uReveal * 1.12;
  float visible = smoothstep(threshold - 0.05, threshold + 0.05, reveal);
  vec3 color = mix(uPaper, page, visible);
  if (uMode > 1.5) { // a wet edge where watercolour is still spreading
    float edge = smoothstep(0.0, 0.05, reveal - threshold) * (1.0 - smoothstep(0.05, 0.14, reveal - threshold));
    color *= 1.0 - edge * 0.18 * (1.0 - step(0.999, uReveal));
  }
  // Paper tooth: graphite catches only on the peaks of the paper.
  float tooth = texture2D(tPaper, uv * vec2(2.0, 1.125)).r;
  color *= mix(1.0, 0.86 + tooth * 0.22, 0.6);
  // The lavender ink layer, printed like a risograph: halftone dots and a misregistered second pass.
  vec4 ink = texture2D(tInk, uv);
  if (ink.a > 0.001) {
    vec2 cell = rot(0.785) * (uv * aspect) * 150.0;
    // Riso: dots that never quite merge, and ink that lays down unevenly on the drum.
    float unevenInk = 0.72 + 0.28 * fbm(uv * aspect * 9.0 + boilFrame * 0.37);
    float dotRadius = sqrt(ink.a) * 0.5 * unevenInk;
    float halftone = 1.0 - smoothstep(dotRadius - 0.06, dotRadius + 0.06, length(fract(cell) - 0.5));
    float coverage = mix(ink.a, halftone, uRiso);
    float inkAlpha = clamp(coverage * 1.05, 0.0, 1.0);
    color = mix(color, color * uInkColor * 1.15, inkAlpha * 0.55);
    color = mix(color, uInkColor, inkAlpha * 0.6);
    vec4 shifted = texture2D(tInk, uv + vec2(0.006, -0.004) * uRiso);
    color = mix(color, vec3(1.0, 0.42, 0.7), shifted.a * uRiso * 0.5 * (1.0 - inkAlpha));
  }
  color += uInkColor * texture2D(tInk, uv, 3.0).a * uInkGlow * 0.35;
  // Grain of the print.
  color += (hash(uv * vec2(${WIDTH}.0, ${HEIGHT}.0) + boilFrame) - 0.5) * 0.035;
  float vignette = smoothstep(0.95, 0.35, length((vUv - 0.5) * vec2(1.1, 1.0)));
  color *= mix(0.82, 1.0, vignette);
  color = mix(color, uFlashColor, uFlash);
  color *= 1.0 - uFade;
  gl_FragColor = vec4(color, 1.0);
}`;

function paperTexture() {
  const element = document.createElement("canvas");
  element.width = 1024; element.height = 1024;
  const g = element.getContext("2d");
  const image = g.createImageData(1024, 1024);
  let seed = 7;
  const random = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);
  for (let i = 0; i < image.data.length; i += 4) { const v = 150 + random() * 105; image.data[i] = image.data[i + 1] = image.data[i + 2] = v; image.data[i + 3] = 255; }
  g.putImageData(image, 0, 0);
  g.filter = "blur(0.7px)"; g.drawImage(element, 0, 0); g.filter = "none";
  const texture = new THREE.CanvasTexture(element);
  texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
  return texture;
}

export function createPencil(renderer, pageCanvas, inkCanvas) {
  const page = new THREE.CanvasTexture(pageCanvas);
  page.colorSpace = THREE.NoColorSpace; page.minFilter = THREE.LinearFilter; page.generateMipmaps = false;
  const ink = new THREE.CanvasTexture(inkCanvas);
  ink.colorSpace = THREE.NoColorSpace; ink.minFilter = THREE.LinearMipmapLinearFilter; ink.generateMipmaps = true;
  const uniforms = {
    tPage: { value: page }, tInk: { value: ink }, tPaper: { value: paperTexture() },
    uReveal: { value: 1 }, uBoil: { value: 1 }, uFatigue: { value: 0 }, uTime: { value: 0 }, uRiso: { value: 0 },
    uFlash: { value: 0 }, uFade: { value: 0 }, uMode: { value: 0 }, uInkGlow: { value: 0 }, uShake: { value: 0 },
    uPaper: { value: new THREE.Color(0.93, 0.91, 0.86) }, uInkColor: { value: new THREE.Color(0.62, 0.52, 0.98) }, uFlashColor: { value: new THREE.Color(1, 1, 1) },
  };
  const material = new THREE.ShaderMaterial({ uniforms, vertexShader, fragmentShader, depthTest: false, depthWrite: false });
  const scene = new THREE.Scene();
  scene.add(new THREE.Mesh(new THREE.PlaneGeometry(2, 2), material));
  const camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  return {
    render(look) {
      page.needsUpdate = true; ink.needsUpdate = true;
      for (const [key, value] of Object.entries(look)) {
        const uniform = uniforms[`u${key[0].toUpperCase()}${key.slice(1)}`];
        if (!uniform) continue;
        if (uniform.value instanceof THREE.Color) uniform.value.set(value); else uniform.value = value;
      }
      renderer.render(scene, camera);
    },
  };
}
