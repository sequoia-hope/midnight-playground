import * as THREE from 'three';
import { glowTexture } from '../textures.js';

// Night lights that flicker individually — flares, camp fires, chaser
// bulbs — without a draw call or a CPU update each: one Points cloud or one
// InstancedMesh per kind, with a per-point phase attribute and a shared
// time uniform driving the flicker in the vertex shader.

export const glowTime = { value: 0 };

// Screen-facing glows. items: [{x, y, z, ph}]. `rate` scales the flicker
// speed, `depth` how far it dips (0 = steady), `blink` > 0 turns it into a
// hard on/off chase instead (chaser bulbs: ph picks the step).
export function flickerPoints(items, { size = 1, color = 0xffffff, rate = 1, depth = 0.35, blink = 0 } = {}) {
  const pos = new Float32Array(items.length * 3), ph = new Float32Array(items.length);
  items.forEach((p, i) => { pos[i * 3] = p.x; pos[i * 3 + 1] = p.y; pos[i * 3 + 2] = p.z; ph[i] = p.ph ?? i; });
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
  g.setAttribute('ph', new THREE.BufferAttribute(ph, 1));
  g.computeBoundingSphere();
  const m = new THREE.PointsMaterial({ size, map: glowTexture(), color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, sizeAttenuation: true });
  m.userData.kind = 'FlickerPoints'; // MaterialKind for the scene export
  m.userData.kindOpts = { rate, depth, blink };
  m.onBeforeCompile = (sh) => {
    sh.uniforms.uTime = glowTime;
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', `#include <common>
        attribute float ph;
        uniform float uTime;
        varying float vFl;`)
      .replace('#include <fog_vertex>', `#include <fog_vertex>
        ${blink > 0
          ? `vFl = step(0.5, fract(uTime * ${(blink).toFixed(3)} + ph * 0.3333));`
          : `float f_ = sin(uTime * ${(13 * rate).toFixed(3)} + ph * 6.3) * sin(uTime * ${(4.7 * rate).toFixed(3)} + ph * 2.1)
                 + 0.4 * sin(uTime * ${(29 * rate).toFixed(3)} + ph * 3.7);
             vFl = 1.0 - ${depth.toFixed(3)} + ${depth.toFixed(3)} * clamp(0.5 + 0.5 * f_, 0.0, 1.0);`}
        gl_PointSize *= 0.75 + 0.25 * vFl;`);
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying float vFl;')
      .replace('#include <alphatest_fragment>', 'diffuseColor.rgb *= vFl;\n#include <alphatest_fragment>');
  };
  m.customProgramCacheKey = () => `desert-flicker-${rate}-${depth}-${blink}`;
  const pts = new THREE.Points(g, m);
  pts.matrixAutoUpdate = false;
  return pts;
}

// Pools of light on the ground (flat additive quads). items:
// [{x, y, z, r, c: [r,g,b], fl (flicker depth, 0 = steady), ph}].
export function flickerPools(items) {
  const geo = new THREE.PlaneGeometry(1, 1);
  geo.rotateX(-Math.PI / 2);
  const mat = new THREE.MeshBasicMaterial({ map: glowTexture(), transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4 });
  const im = new THREE.InstancedMesh(geo, mat, items.length);
  const m4 = new THREE.Matrix4(), c = new THREE.Color();
  const ph = new Float32Array(items.length), fl = new Float32Array(items.length);
  items.forEach((p, i) => {
    m4.makeScale(p.r * 2, 1, p.r * 2).setPosition(p.x, p.y, p.z);
    im.setMatrixAt(i, m4);
    im.setColorAt(i, c.setRGB(p.c[0], p.c[1], p.c[2]));
    ph[i] = p.ph ?? 0; fl[i] = p.fl ?? 0;
  });
  geo.setAttribute('ph', new THREE.InstancedBufferAttribute(ph, 1));
  geo.setAttribute('fl', new THREE.InstancedBufferAttribute(fl, 1));
  mat.userData.kind = 'GroundPool'; // MaterialKind for the scene export
  mat.onBeforeCompile = (sh) => {
    sh.uniforms.uTime = glowTime;
    sh.vertexShader = sh.vertexShader
      .replace('#include <common>', `#include <common>
        attribute float ph;
        attribute float fl;
        uniform float uTime;
        varying float vFl;`)
      .replace('#include <fog_vertex>', `#include <fog_vertex>
        float f_ = sin(uTime * 13.0 + ph * 6.3) * sin(uTime * 4.7 + ph * 2.1) + 0.4 * sin(uTime * 29.0 + ph * 3.7);
        vFl = 1.0 - fl + fl * clamp(0.5 + 0.5 * f_, 0.0, 1.0);`);
    sh.fragmentShader = sh.fragmentShader
      .replace('#include <common>', '#include <common>\nvarying float vFl;')
      .replace('#include <alphatest_fragment>', 'diffuseColor.rgb *= vFl;\n#include <alphatest_fragment>');
  };
  mat.customProgramCacheKey = () => 'desert-pools';
  im.renderOrder = 2;
  im.computeBoundingSphere();
  im.matrixAutoUpdate = false;
  return { mesh: im, material: mat };
}
