// The hero: Looper's glass orb with the chrome S-fold inside. It is never faded in; light discovers it.
import * as THREE from "three";
import { BAR, BEATS, lerp, smooth } from "./story.js";

export const ORB_RADIUS = 1.15;

export function createOrb() {
  const group = new THREE.Group();
  const glass = new THREE.Mesh(
    new THREE.SphereGeometry(ORB_RADIUS, 128, 96),
    new THREE.MeshPhysicalMaterial({
      color: 0xffffff, roughness: 0.02, transmission: 1, thickness: 1.4, ior: 1.46,
      clearcoat: 1, clearcoatRoughness: 0.02, iridescence: 0.25, iridescenceIOR: 1.3,
      attenuationColor: new THREE.Color(0.86, 0.82, 1.0), attenuationDistance: 3.5, envMapIntensity: 1.2,
    }),
  );
  // The S-fold: a ribbon of chrome that turns back on itself, the loop inside the circle.
  const s = [];
  for (let i = 0; i <= 64; i += 1) {
    const k = i / 64 * 2 - 1;
    s.push(new THREE.Vector3(k * 0.72, Math.sin(k * Math.PI) * 0.38 - k * 0.1, Math.cos(k * Math.PI * 0.5) * 0.18));
  }
  const ribbon = new THREE.Mesh(
    new THREE.TubeGeometry(new THREE.CatmullRomCurve3(s), 256, 0.13, 48, false),
    new THREE.MeshPhysicalMaterial({ color: 0xeeeef6, metalness: 1, roughness: 0.1, clearcoat: 1, envMapIntensity: 1.4 }),
  );
  ribbon.scale.set(1, 1, 0.42);
  ribbon.rotation.z = -0.35;
  group.add(ribbon, glass);
  group.visible = false;
  return {
    group,
    update(t, scene) {
      const revealed = t >= BEATS.reveal - BAR * 0.5;
      group.visible = revealed;
      // The light sweep: the studio environment turns around the orb and brightens, like a product reveal.
      const light = smooth(BEATS.reveal - BAR * 0.5, BEATS.reveal + BAR * 1.5, t);
      scene.environmentIntensity = lerp(0.0, 0.62, light);
      scene.environmentRotation.set(0.2, lerp(-2.4, 0.4, light) + (t - BEATS.reveal) * 0.08, 0);
      group.rotation.set(0.12, Math.sin((t - BEATS.reveal) * 0.35) * 0.32, 0.04); // it sways; the S stays an S
    },
  };
}
