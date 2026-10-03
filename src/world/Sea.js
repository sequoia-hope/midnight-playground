import * as THREE from 'three';
import { clamp, smoothstep, mulberry32 } from '../util/math.js';
import { terrainDetailTexture } from './textures.js';

// Open water for levels with a sea. One large plane at sea level, tinted per
// vertex by depth (turquoise shallows and white-ish surf where the ground
// meets the water, deep blue offshore) and alpha-faded in the shallows so
// sand shows through. Reflections come from the sky environment map; a
// scrolling normal map gives it ripples.

function waveNormals() {
  const S = 256;
  const c = document.createElement('canvas');
  c.width = c.height = S;
  const g = c.getContext('2d');
  const rng = mulberry32(51);
  const waves = [];
  // Integer frequencies keep it tileable; most energy runs along a wind
  // direction, with shorter choppy waves in a wider spread.
  for (let k = 0; k < 40; k++) {
    const f = 2 + k * 0.35 + rng() * 2;
    const ang = 0.5 + (rng() - 0.5) * (k < 12 ? 1.0 : 2.6);
    const fx = Math.round(Math.cos(ang) * f), fy = Math.round(Math.sin(ang) * f) || 1;
    waves.push({ fx, fy, ph: rng() * 6.28, a: 1.4 / Math.pow(Math.hypot(fx, fy), 1.1) });
  }
  const hgt = (x, y) => { let h = 0; for (const w of waves) h += w.a * Math.sin(((w.fx * x + w.fy * y) / S) * 6.2832 + w.ph); return h; };
  const img = g.createImageData(S, S);
  for (let y = 0; y < S; y++) for (let x = 0; x < S; x++) {
    const dx = hgt(x + 1, y) - hgt(x - 1, y), dy = hgt(x, y + 1) - hgt(x, y - 1);
    const i = (y * S + x) * 4;
    img.data[i] = 128 + dx * 22; img.data[i + 1] = 128 + dy * 22; img.data[i + 2] = 255; img.data[i + 3] = 255;
  }
  g.putImageData(img, 0, 0);
  const t = new THREE.CanvasTexture(c);
  t.wrapS = t.wrapT = THREE.RepeatWrapping;
  t.colorSpace = THREE.NoColorSpace;
  return t;
}

export class Sea {
  constructor(world, seaY = 0) {
    const T = world.terrain;
    this.y = seaY;
    const step = 20;
    const x0 = T.minX, z0 = T.minZ;
    const nx = Math.ceil((T.maxX - T.minX) / step) + 1, nz = Math.ceil((T.maxZ - T.minZ) / step) + 1;
    const H = new Float32Array(nx * nz);
    const F = {};
    for (let j = 0; j < nz; j++) for (let i = 0; i < nx; i++) {
      const x = x0 + i * step, z = z0 + j * step;
      T.far(x, z, F);
      // Exact heights near the road; far away, only which side matters.
      H[j * nx + i] = F.d < 700 ? T.heightAt(x, z) : T.seaSide(F) > 0.5 ? seaY - 30 : seaY + 50;
    }
    const pos = new Float32Array(nx * nz * 3);
    const col = new Float32Array(nx * nz * 4);
    const deep = new THREE.Color(0x0d3350), mid = new THREE.Color(0x14607a), shallow = new THREE.Color(0x3fa7a4), surf = new THREE.Color(0xcfe7e6);
    const c = new THREE.Color();
    for (let j = 0; j < nz; j++) for (let i = 0; i < nx; i++) {
      const k = j * nx + i;
      pos[k * 3] = x0 + i * step; pos[k * 3 + 1] = seaY; pos[k * 3 + 2] = z0 + j * step;
      const depth = seaY - H[k];
      c.copy(deep).lerp(mid, 1 - smoothstep(6, 25, depth));
      c.lerp(shallow, 1 - smoothstep(0.8, 6, depth));
      c.lerp(surf, (1 - smoothstep(-0.4, 0.9, depth)) * 0.45);
      col[k * 4] = c.r; col[k * 4 + 1] = c.g; col[k * 4 + 2] = c.b;
      col[k * 4 + 3] = clamp(0.55 + smoothstep(0.2, 5, depth) * 0.42, 0, 0.97);
    }
    const idx = [];
    for (let j = 0; j < nz - 1; j++) for (let i = 0; i < nx - 1; i++) {
      const a = j * nx + i, b = a + 1, d = a + nx, e = d + 1;
      // Skip quads that are entirely dry land.
      if (Math.min(H[a], H[b], H[d], H[e]) > seaY + 0.5) continue;
      idx.push(a, d, b, b, d, e);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.BufferAttribute(col, 4));
    g.setAttribute('normal', new THREE.BufferAttribute(new Float32Array(nx * nz * 3).map((_, q) => (q % 3 === 1 ? 1 : 0)), 3));
    const uv = new Float32Array(nx * nz * 2);
    for (let k = 0; k < nx * nz; k++) { uv[k * 2] = pos[k * 3] / 60; uv[k * 2 + 1] = pos[k * 3 + 2] / 60; }
    g.setAttribute('uv', new THREE.BufferAttribute(uv, 2));
    // Water depth per vertex drives the surf band in the shader.
    const depthAttr = new Float32Array(nx * nz);
    for (let k = 0; k < nx * nz; k++) depthAttr[k] = seaY - H[k];
    g.setAttribute('aDepth', new THREE.BufferAttribute(depthAttr, 1));
    g.setIndex(idx.length > 65535 ? new THREE.Uint32BufferAttribute(idx, 1) : new THREE.Uint16BufferAttribute(idx, 1));
    g.computeBoundingSphere();
    this.normalMap = waveNormals();
    this.material = new THREE.MeshStandardMaterial({
      vertexColors: true, transparent: true, roughness: 0.12, metalness: 0.0,
      normalMap: this.normalMap, normalScale: new THREE.Vector2(0.5, 0.5), envMapIntensity: 1.3,
    });
    this.uniforms = { uTime: { value: 0 }, uOff2: { value: new THREE.Vector2() }, tFoam: { value: terrainDetailTexture() } };
    this.patchMaterial(this.material);
    this.mesh = new THREE.Mesh(g, this.material);
    this.mesh.receiveShadow = true;
    this.mesh.renderOrder = 1;
    world.scene.add(this.mesh);
    world.updaters.push((dt) => {
      this.normalMap.offset.x += dt * 0.012;
      this.normalMap.offset.y += dt * 0.007;
      this.uniforms.uTime.value += dt;
      this.uniforms.uOff2.value.x -= dt * 0.021;
      this.uniforms.uOff2.value.y += dt * 0.016;
    });
  }

  // Two crossing scales of ripples (the second counter-scrolling, so the
  // pattern never visibly slides as one sheet), ripples calmed with distance
  // so the far sea doesn't sparkle into noise; surf lines rolling up the
  // beach where the water is shallow; fresnel opacity (see-through looking
  // down, mirror-like toward the horizon); and a sharp sun/moon glitter on
  // the ripple facets.
  patchMaterial(m) {
    const u = this.uniforms;
    m.userData.kind = 'Sea'; // MaterialKind for the scene export
    m.onBeforeCompile = (shader) => {
      Object.assign(shader.uniforms, u);
      shader.vertexShader = shader.vertexShader
        .replace('#include <common>', '#include <common>\nattribute float aDepth;\nvarying float vDepth;\nvarying vec3 vSeaPos;')
        .replace('#include <fog_vertex>', '#include <fog_vertex>\nvDepth = aDepth;\nvSeaPos = (modelMatrix * vec4(transformed, 1.0)).xyz;');
      shader.fragmentShader = shader.fragmentShader
        .replace('#include <common>', '#include <common>\nuniform float uTime;\nuniform vec2 uOff2;\nuniform sampler2D tFoam;\nvarying float vDepth;\nvarying vec3 vSeaPos;')
        .replace('#include <color_fragment>', `#include <color_fragment>
          float seaDist = length(vSeaPos - cameraPosition);
          vec4 fn = texture2D(tFoam, vSeaPos.xz / 9.0 + vec2(uTime * 0.012, uTime * 0.004));
          float surfZone = 1.0 - smoothstep(0.1, 2.4, vDepth);
          float roll = sin(vDepth * 4.2 - uTime * 1.3 + fn.a * 5.0) * 0.5 + 0.5;
          float foam = smoothstep(0.55, 0.9, fn.g * 0.55 + roll * 0.45 + surfZone * 0.35) * surfZone;
          foam = max(foam, (1.0 - smoothstep(0.0, 0.35, vDepth)) * (0.45 + 0.4 * fn.g));
          foam = clamp(foam, 0.0, 1.0);
          diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.82, 0.88, 0.9), foam * 0.85);
          diffuseColor.a = max(diffuseColor.a, foam * 0.92);
        `)
        .replace('#include <normal_fragment_maps>', `
          vec3 n1 = texture2D(normalMap, vNormalMapUv).xyz * 2.0 - 1.0;
          vec3 n2 = texture2D(normalMap, vNormalMapUv * 3.3 + uOff2).xyz * 2.0 - 1.0;
          vec3 mapN = vec3(n1.xy + n2.xy * 0.6, 1.0);
          mapN.xy *= normalScale * mix(1.0, 0.3, smoothstep(30.0, 900.0, seaDist)) * (1.0 - foam * 0.7);
          normal = normalize(tbn * mapN);
        `)
        .replace('#include <roughnessmap_fragment>', '#include <roughnessmap_fragment>\nroughnessFactor = mix(roughnessFactor, 0.85, foam);')
        .replace('#include <opaque_fragment>', `
          vec3 seaV = normalize(vViewPosition);
          float seaF = 1.0 - clamp(dot(seaV, normal), 0.0, 1.0);
          seaF *= seaF; seaF *= seaF;
          diffuseColor.a = mix(diffuseColor.a, 1.0, seaF * 0.8);
          #if NUM_DIR_LIGHTS > 0
            float gl = clamp(dot(reflect(-seaV, normal), directionalLights[0].direction), 0.0, 1.0);
            gl *= gl; gl *= gl; gl *= gl; gl *= gl; gl *= gl; gl *= gl; gl *= gl; gl *= gl; // ^256
            outgoingLight += min(directionalLights[0].color, vec3(4.0)) * gl * 3.0 * (1.0 - foam) * smoothstep(1500.0, 200.0, seaDist);
          #endif
          #include <opaque_fragment>
        `);
    };
  }
}
