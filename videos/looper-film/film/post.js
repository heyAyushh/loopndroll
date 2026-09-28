// The lens and the print: depth of field, bloom, then grade, grain, vignette, fringe, flashes and letterbox.
import * as THREE from "three";
import { EffectComposer } from "three/addons/postprocessing/EffectComposer.js";
import { RenderPass } from "three/addons/postprocessing/RenderPass.js";
import { BokehPass } from "three/addons/postprocessing/BokehPass.js";
import { UnrealBloomPass } from "three/addons/postprocessing/UnrealBloomPass.js";
import { OutputPass } from "three/addons/postprocessing/OutputPass.js";
import { ShaderPass } from "three/addons/postprocessing/ShaderPass.js";

export const WIDTH = 1920;
export const HEIGHT = 1080;
const LETTERBOX = (HEIGHT - WIDTH / 2.39) / 2 / HEIGHT;

const FinishShader = {
  uniforms: {
    tDiffuse: { value: null }, uTime: { value: 0 }, uFlash: { value: 0 }, uFlashColor: { value: new THREE.Color(1, 1, 1) },
    uFade: { value: 0 }, uGrain: { value: 0.06 }, uVignette: { value: 0.45 }, uFringe: { value: 0.0015 }, uLetterbox: { value: LETTERBOX },
    uLift: { value: new THREE.Color(0.012, 0.01, 0.022) },
  },
  vertexShader: `varying vec2 vUv; void main(){ vUv = uv; gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0); }`,
  fragmentShader: `uniform sampler2D tDiffuse; uniform float uTime, uFlash, uFade, uGrain, uVignette, uFringe, uLetterbox; uniform vec3 uFlashColor, uLift; varying vec2 vUv;
    float hash(vec2 p){ p = fract(p * vec2(443.897, 441.423)); p += dot(p, p.yx + 19.19); return fract((p.x + p.y) * p.x); }
    void main(){
      vec2 centered = vUv - 0.5;
      float edge = dot(centered, centered);
      vec2 offset = centered * uFringe * (1.0 + edge * 6.0);
      vec3 color = vec3(texture2D(tDiffuse, vUv + offset).r, texture2D(tDiffuse, vUv).g, texture2D(tDiffuse, vUv - offset).b);
      color = uLift + color * (1.0 - uLift);
      color *= 1.0 - uVignette * smoothstep(0.08, 0.55, edge);
      float grain = hash(vUv * vec2(1920.0, 1080.0) + fract(uTime * 13.7) * 100.0) - 0.5;
      color += grain * uGrain * (0.6 + 0.4 * (1.0 - dot(color, vec3(0.333))));
      color = mix(color, uFlashColor, uFlash);
      color *= 1.0 - uFade;
      if (vUv.y < uLetterbox || vUv.y > 1.0 - uLetterbox) color = vec3(0.0);
      gl_FragColor = vec4(color, 1.0);
    }`,
};

export function createPost(renderer) {
  const composer = new EffectComposer(renderer);
  composer.setSize(WIDTH, HEIGHT);
  const placeholderScene = new THREE.Scene();
  const placeholderCamera = new THREE.PerspectiveCamera();
  const renderPass = new RenderPass(placeholderScene, placeholderCamera);
  const bokeh = new BokehPass(placeholderScene, placeholderCamera, { focus: 3, aperture: 0.002, maxblur: 0.01 });
  const bloom = new UnrealBloomPass(new THREE.Vector2(WIDTH, HEIGHT), 0.6, 0.55, 0.72);
  const output = new OutputPass();
  const finish = new ShaderPass(FinishShader);
  composer.addPass(renderPass);
  composer.addPass(bokeh);
  composer.addPass(bloom);
  composer.addPass(output);
  composer.addPass(finish);
  return {
    render(scene, camera, look) {
      renderPass.scene = scene; renderPass.camera = camera;
      bokeh.scene = scene; bokeh.camera = camera;
      bokeh.enabled = look.aperture > 0;
      bokeh.uniforms.focus.value = look.focus;
      bokeh.uniforms.aperture.value = look.aperture;
      bokeh.uniforms.maxblur.value = look.maxBlur ?? 0.012;
      bokeh.uniforms.nearClip.value = camera.near; bokeh.uniforms.farClip.value = camera.far;
      bloom.strength = look.bloom; bloom.radius = look.bloomRadius ?? 0.55; bloom.threshold = look.bloomThreshold ?? 0.72;
      const u = finish.uniforms;
      u.uTime.value = look.time; u.uFlash.value = look.flash ?? 0; u.uFade.value = look.fade ?? 0;
      u.uFlashColor.value.set(look.flashColor ?? 0xffffff);
      u.uGrain.value = look.grain ?? 0.06; u.uVignette.value = look.vignette ?? 0.45;
      composer.render();
    },
  };
}
