import * as THREE from 'three';
import { clamp, smoothstep } from '../util/math.js';
import { ROAD_TYPES, ROAD_KEYS } from '../track/roadTypes.js';
import { asphaltTexture, gravelTexture, concreteTexture, checkerTexture, chevronTexture, terrainDetailTexture } from './textures.js';
import { TerrainColorizer } from './TerrainMesh.js';

// Road surface, shoulders, markings and barriers, all extruded along the
// track centreline from cross-section profiles.

const ROAD_STEP = 2;

// Generic extrusion. profile: [{lat(f,i), dy(f,i), u}] across the section.
// ranges: [[s0, s1], ...]. Produces one BufferGeometry.
export function extrude(track, ranges, profile, { step = ROAD_STEP, vScale = 8, flat = false, color = null } = {}) {
  const pos = [], uv = [], idx = [], col = [];
  const f = {};
  const P = profile.length;
  for (const [s0, s1] of ranges) {
    if (s1 - s0 < 0.5) continue;
    const base = pos.length / 3;
    let rows = 0;
    for (let s = s0; ; s += step) {
      const ss = Math.min(s, s1);
      track.frame(ss, f);
      for (let p = 0; p < P; p++) {
        const pr = profile[p];
        const lat = typeof pr.lat === 'function' ? pr.lat(f, ss) : pr.lat;
        const dy = typeof pr.dy === 'function' ? pr.dy(f, ss) : pr.dy || 0;
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        const y = (pr.abs ? 0 : f.y - lat * f.bank) + dy;
        pos.push(x, y, z);
        uv.push(pr.u !== undefined ? pr.u : lat / 4, ss / vScale);
        if (color) { const c = typeof color === 'function' ? color(f, ss, p) : color; col.push(c[0], c[1], c[2]); }
      }
      rows++;
      if (ss >= s1) break;
    }
    for (let r = 0; r < rows - 1; r++) {
      for (let p = 0; p < P - 1; p++) {
        if (profile[p].gapAfter) continue;
        const a = base + r * P + p, b = a + 1, c = a + P, d = c + 1;
        idx.push(a, b, c, b, d, c);
      }
    }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('uv', new THREE.Float32BufferAttribute(uv, 2));
  if (color) g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  g.setIndex(idx);
  g.computeVertexNormals();
  return g;
}

// Split [0, len] into chunks for frustum culling.
function chunks(s0, s1, size = 560) {
  const out = [];
  for (let s = s0; s < s1; s += size) out.push([s, Math.min(s1, s + size)]);
  return out;
}

// Split runs into chunks and bundle neighbouring pieces so each mesh covers
// about `size` metres of road (fewer draw calls, still frustum-cullable).
function groupRuns(ranges, size) {
  const pieces = ranges.flatMap((r) => chunks(r[0], r[1], size));
  const groups = [];
  let cur = [], start = null;
  for (const p of pieces) {
    if (start === null) start = p[0];
    if (p[1] - start > size && cur.length) { groups.push(cur); cur = []; start = p[0]; }
    cur.push(p);
  }
  if (cur.length) groups.push(cur);
  return groups;
}

// Runs where pred(s) holds, sampled every `step` metres.
export function runs(track, pred, step = 2, s0 = 0, s1 = track.length) {
  const out = [];
  let start = null;
  for (let s = s0; s <= s1; s += step) {
    const ok = pred(s);
    if (ok && start === null) start = s;
    if (!ok && start !== null) { out.push([start, s]); start = null; }
  }
  if (start !== null) out.push([start, s1]);
  return out;
}

// Lane layout across the section for a marking scheme: lanes start at
// `origin` (lateral metres) and repeat every `width`. The asphalt shader uses
// it to lay tyre-worn wheel paths and an oil stripe down each lane.
function laneLayout(marks, hw) {
  if (marks === 'freeway' && hw > 9.0) { const l = -hw + 1.2, r = hw - 2.0; return [l, (r - l) / 4]; }
  if (marks === 'boulevard') return [0, (hw - 0.45) / 2];
  if (marks === 'avenue' || marks === 'guide') return [0, hw / 2];
  return [0, hw - 0.3];
}

// Shared GLSL: cheap lattice hash (no sin, fine on phones).
const HASH_GLSL = /* glsl */`
float mrHash(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
`;

// Asphalt wear, layered over the tiling texture in the road's own frame
// (vRoad = lateral m, distance along the road m; vLane = origin, lane width,
// half width): darker polished wheel paths and an oil stripe down each lane,
// big slow tonal drift so the texture never visibly repeats, the odd
// rectangular repair patch, dusty edges, and at night a damp sheen (lower
// roughness in the wheel paths and dips) so lamps and headlights glint.
function patchAsphalt(material, uniforms) {
  material.userData.kind = 'Asphalt'; // MaterialKind for the scene export
  material.onBeforeCompile = (shader) => {
    shader.uniforms.tDetail = uniforms.tDetail;
    shader.uniforms.uWet = uniforms.uWet;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nattribute vec3 aLane;\nvarying vec3 vLane;\nvarying vec2 vRoad;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvLane = aLane;\nvRoad = uv * 4.0;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform sampler2D tDetail;\nuniform float uWet;\nvarying vec3 vLane;\nvarying vec2 vRoad;' + HASH_GLSL)
      .replace('#include <map_fragment>', `#include <map_fragment>
        float lat = vRoad.x, along = vRoad.y;
        float lp = (lat - vLane.x) / vLane.y;
        float dc = (fract(lp) - 0.5) * vLane.y;             // metres from the lane centre
        float wq = (abs(dc) - 0.85) / 0.32;
        float wheel = exp(-wq * wq);
        float oil = exp(-(dc * dc) / 0.1225);
        vec4 big = texture2D(tDetail, vec2(lat * 0.021, along * 0.0045));
        vec4 mid = texture2D(tDetail, vec2(lat * 0.09, along * 0.03) + 0.4);
        // Repair patches: whole cells of a coarse grid, a few percent of them.
        vec2 pc = vec2((lat + 40.0) / 2.6, along / 7.0);
        vec2 pf = fract(pc);
        float patchA = step(mrHash(floor(pc)), 0.045) * step(0.12, pf.x) * step(pf.x, 0.88) * step(0.1, pf.y) * step(pf.y, 0.9);
        float edge = smoothstep(vLane.z - 0.9, vLane.z, abs(lat));
        float tone = (0.86 + 0.28 * big.a) * (0.93 + 0.14 * mid.r);
        diffuseColor.rgb *= tone * (1.0 - wheel * 0.1 - oil * 0.12) * mix(1.0, 0.7, patchA);
        diffuseColor.rgb = mix(diffuseColor.rgb, diffuseColor.rgb * vec3(1.25, 1.2, 1.12), edge * 0.6);
        float damp = uWet * (0.55 + 0.45 * smoothstep(0.35, 0.7, big.r)) * (1.0 - edge * 0.7);
        diffuseColor.rgb *= 1.0 - damp * 0.25;
        float mrRough = -wheel * 0.08 - patchA * 0.06 - damp * 0.36 * (0.7 + 0.3 * wheel);
      `)
      .replace('#include <roughnessmap_fragment>', '#include <roughnessmap_fragment>\nroughnessFactor = clamp(roughnessFactor + mrRough, 0.25, 1.0);');
  };
}

// Paint wear: markings are thin quads carrying (side, s) in uv; fade the
// paint toward asphalt in blotches and where tyres cross.
function patchMarkings(material, uniforms) {
  material.userData.kind = 'Markings'; // MaterialKind for the scene export
  material.onBeforeCompile = (shader) => {
    shader.uniforms.tDetail = uniforms.tDetail;
    shader.vertexShader = shader.vertexShader
      .replace('#include <common>', '#include <common>\nvarying vec2 vMark;')
      .replace('#include <uv_vertex>', '#include <uv_vertex>\nvMark = uv;');
    shader.fragmentShader = shader.fragmentShader
      .replace('#include <common>', '#include <common>\nuniform sampler2D tDetail;\nvarying vec2 vMark;')
      .replace('#include <color_fragment>', `#include <color_fragment>
        vec4 w1 = texture2D(tDetail, vec2(vMark.x * 0.12, vMark.y * 0.35));
        vec4 w2 = texture2D(tDetail, vec2(vMark.x * 0.5, vMark.y * 0.05) + 0.3);
        float wear = smoothstep(0.42, 0.85, w1.g * 0.6 + w2.a * 0.6) * 0.7;
        diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.05, 0.05, 0.052), wear);
      `);
  };
}

export class Road {
  constructor(track, terrain) {
    this.track = track;
    this.terrain = terrain;
    this.group = new THREE.Group();
    this.group.name = 'road';
    this.materials = {};
    this.uniforms = { tDetail: { value: terrainDetailTexture() }, uWet: { value: 0 } };
  }

  // Night makes the tarmac look damp (see patchAsphalt).
  setNight(n) { this.uniforms.uWet.value = n; }

  build() {
    this.classifySides();
    this.buildSurface();
    this.buildMarkings();
    this.buildBarriers();
    this.buildLines();
    this.buildChevrons();
    this.buildViaduct();
    return this.group;
  }

  // For each metre and side decide what sits at the edge: a rock face, a
  // guardrail, a fence or a concrete barrier. Physics and scenery read it.
  classifySides() {
    const t = this.track, T = this.terrain;
    const n = t.n;
    const L = new Uint8Array(n), R = new Uint8Array(n);
    // 0 none, 1 rock wall, 2 guardrail, 3 fence, 4 jersey barrier, 5 kerb + railing
    const edge = (i) => ROAD_TYPES[ROAD_KEYS[t.roadType[i]]].edge;
    const EDGE_CODE = { fence: 3, jersey: 4, rail: 5, curb: 6, none: 0, circuit: 0 };
    const tmpL = new Float32Array(n), tmpR = new Float32Array(n);
    for (let i = 0; i < n; i += 2) {
      if (edge(i) === 'terrain') {
        for (const side of [-1, 1]) {
          const hw = t.hw[i] + t.margin[i];
          const probe = side * (hw + 5);
          const x = t.px[i] + t.rx[i] * probe, zz = t.pz[i] + t.rz[i] * probe;
          const th = T.heightAt(x, zz);
          const rh = t.surfaceY(i, side * hw);
          const v = th - rh; // positive = ground rises (wall)
          (side < 0 ? tmpL : tmpR)[i] = v;
        }
      }
    }
    // Smooth the decision so barriers come in sensible runs.
    const decide = (tmp, out) => {
      for (let i = 0; i < n; i++) {
        const e = edge(i);
        if (e !== 'terrain') { out[i] = EDGE_CODE[e]; continue; }
        let acc = 0, cnt = 0;
        for (let k = -16; k <= 16; k += 2) {
          const j = clamp((i & ~1) + k, 0, n - 1);
          acc += tmp[j]; cnt++;
        }
        out[i] = acc / cnt > 1.6 ? 1 : 2;
      }
    };
    decide(tmpL, L);
    decide(tmpR, R);
    // Drop stubs: short rock-wall gaps inside guardrail runs become guardrail,
    // and short guardrail pieces between rock walls disappear.
    const despeckle = (arr) => {
      for (const [val, other, minLen] of [[1, 2, 30], [2, 1, 40]]) {
        let i = 0;
        while (i < n) {
          if (edge(i) !== 'terrain' || arr[i] !== val) { i++; continue; }
          let j = i;
          while (j < n && arr[j] === val && edge(j) === 'terrain') j++;
          const before = i > 0 ? arr[i - 1] : other, after = j < n ? arr[j] : other;
          if (j - i < minLen && before === other && after === other) arr.fill(other, i, j);
          i = j;
        }
      }
    };
    despeckle(L);
    despeckle(R);
    // Hairpin outsides always get a guardrail.
    for (let i = 0; i < n; i++) {
      const k = t.kSmooth[i];
      if (edge(i) === 'terrain' && Math.abs(k) > 0.022) {
        if (k > 0) L[i] = 2; else R[i] = 2;
      }
    }
    this.sideL = L;
    this.sideR = R;
    t.sideL = L;
    t.sideR = R;
  }

  mat(key, make) {
    if (!this.materials[key]) this.materials[key] = make();
    return this.materials[key];
  }

  buildSurface() {
    const t = this.track;
    const zoneRanges = t.zones.map((z) => [z.s0, z.s1]);
    const tones = [0, 1, 2];
    const typeAt = (s) => ROAD_TYPES[ROAD_KEYS[t.roadType[t.idx(s)]]];
    const asphaltMats = tones.map((tone) => this.mat('asphalt' + tone, () => {
      const m = new THREE.MeshStandardMaterial({ map: asphaltTexture(tone), roughness: 0.86, metalness: 0.0, color: 0xffffff });
      m.map.repeat.set(1, 1);
      patchAsphalt(m, this.uniforms);
      return m;
    }));
    const shoulderMat = this.mat('shoulder', () => {
      const m = new THREE.MeshStandardMaterial({ map: gravelTexture(), roughness: 1, vertexColors: true });
      // Toward the outer edge the gravel texture flattens to its average so
      // the verge meets the terrain (whose colour the outer vertices carry)
      // without a hard seam.
      m.userData.kind = 'Shoulder'; // MaterialKind for the scene export
      m.onBeforeCompile = (shader) => {
        shader.fragmentShader = shader.fragmentShader.replace('#include <map_fragment>', `
          vec4 sampledDiffuseColor = texture2D(map, vMapUv);
          sampledDiffuseColor.rgb = mix(sampledDiffuseColor.rgb, vec3(0.22, 0.2, 0.17), smoothstep(0.55, 0.95, vMapUv.x) * 0.8);
          diffuseColor *= sampledDiffuseColor;
        `);
      };
      return m;
    });
    const colorizer = new TerrainColorizer(this.terrain);
    const tcol = [0, 0, 0];
    const f = {};

    t.zones.forEach((z, zi) => {
      const s0 = Math.max(0, z.s0 - 1), s1 = Math.min(t.length, z.s1 + 1);
      for (const r of chunks(s0, s1)) {
        // Asphalt UVs in metres / 4 both ways, so the aggregate isn't
        // stretched along the road; the shader recovers metres from them.
        const surf = extrude(t, [r], [
          { lat: (f) => -f.hw },
          { lat: 0 },
          { lat: (f) => f.hw },
        ], { vScale: 4 });
        const uv = surf.getAttribute('uv');
        const lane = new Float32Array(uv.count * 3);
        for (let k = 0; k < uv.count; k++) {
          const s = uv.getY(k) * 4;
          t.frame(Math.min(s, t.length), f);
          const [o, w] = laneLayout(ROAD_TYPES[ROAD_KEYS[t.roadType[t.idx(s)]]].marks, f.hw);
          lane[k * 3] = o; lane[k * 3 + 1] = w; lane[k * 3 + 2] = f.hw;
        }
        surf.setAttribute('aLane', new THREE.BufferAttribute(lane, 3));
        const mesh = new THREE.Mesh(surf, asphaltMats[typeAt((r[0] + r[1]) / 2).tone]);
        mesh.receiveShadow = true;
        this.group.add(mesh);

        // Shoulders + skirt (sloped into the terrain; vertical fascia in the city).
        // Profiles run left→right so faces point up/outward.
        const edgeType = typeAt((r[0] + r[1]) / 2).edge;
        // Kerbed streets: the scenery lays the kerbs and pavements; circuits
        // their kerbs and run-off.
        if (edgeType === 'curb' || edgeType === 'circuit') continue;
        const city = edgeType === 'jersey';
        const TINT = {
          mountain: [0.55, 0.52, 0.48], coast: [0.6, 0.55, 0.47], valley: [0.45, 0.42, 0.33], beach: [0.66, 0.64, 0.6],
          canyon: [0.62, 0.45, 0.33], desert: [0.66, 0.56, 0.42], playa: [0.78, 0.76, 0.7],
        };
        const tint = TINT[z.landform] || [0.62, 0.62, 0.6];
        for (const side of [-1, 1]) {
          const W = (f) => (side < 0 ? f.wallL : f.wallR);
          const prof = city
            ? [
              { lat: (f) => side * f.hw, dy: 0, u: 0 },
              { lat: (f) => side * (W(f) + 0.35), dy: 0, u: 0.4 },
              { lat: (f) => side * (W(f) + 0.35), dy: -1.6, u: 1 },
            ]
            : [
              { lat: (f) => side * f.hw, dy: -0.01, u: 0 },
              { lat: (f) => side * W(f), dy: -0.08, u: 0.5 },
              { lat: (f) => side * (W(f) + 2.5), dy: -1.6, u: 1 },
            ];
          if (side < 0) prof.reverse();
          const g = extrude(t, [r], prof, { vScale: 6, color: tint });
          if (!city) {
            // Outer edge takes the terrain's own colour (scaled so gravel
            // map × vertex colour lands on what the terrain shader shows).
            const gp = g.getAttribute('position'), gu = g.getAttribute('uv'), gc = g.getAttribute('color');
            for (let k = 0; k < gu.count; k++) {
              if (gu.getX(k) < 0.99) continue;
              const x = gp.getX(k), zz = gp.getZ(k);
              const y = this.terrain.heightAt(x, zz);
              const sl = this.terrain.slopeAt(x, zz, 3);
              colorizer.color(x, y, zz, 1 / Math.sqrt(1 + sl * sl), tcol);
              const q = 1.25 / 0.29;
              gc.setXYZ(k, tcol[0] * q, tcol[1] * q, tcol[2] * q);
            }
          }
          const m = new THREE.Mesh(g, shoulderMat);
          m.receiveShadow = true;
          this.group.add(m);
        }
      }
    });
  }

  buildMarkings() {
    const t = this.track;
    const white = [0.92, 0.92, 0.9], yellow = [0.95, 0.72, 0.12];
    const pos = [], col = [], idx = [], uvs = [];
    const f = {};
    const noMarks = t.noMarks || [];
    const linesAt = (s) => {
      t.frame(s, f);
      const hw = f.hw;
      const marks = ROAD_TYPES[ROAD_KEYS[t.roadType[t.idx(s)]]].marks;
      const L = [];
      // Scenery can blank the paint across junctions (crossings, plazas).
      if (noMarks.some((g) => s > g.s0 && s < g.s1)) return { L, f: { ...f } };
      if (marks === 'double') {
        L.push({ k: 'c1', lat: -0.14, w: 0.12, c: yellow }, { k: 'c2', lat: 0.14, w: 0.12, c: yellow });
        L.push({ k: 'el', lat: -(hw - 0.3), w: 0.15, c: white }, { k: 'er', lat: hw - 0.3, w: 0.15, c: white });
      } else if (marks === 'boulevard') {
        // Boulevard: two lanes each way, double yellow in the middle.
        L.push({ k: 'c1', lat: -0.14, w: 0.12, c: yellow }, { k: 'c2', lat: 0.14, w: 0.12, c: yellow });
        const lane = (hw - 0.45) / 2;
        L.push({ k: 'bl', lat: -lane, w: 0.13, c: white, dash: [3, 9] }, { k: 'br', lat: lane, w: 0.13, c: white, dash: [3, 9] });
        L.push({ k: 'el', lat: -(hw - 0.3), w: 0.15, c: white }, { k: 'er', lat: hw - 0.3, w: 0.15, c: white });
      } else if (marks === 'avenue') {
        // Kerbed city street: double yellow and lane lines; the kerbs are the edges.
        L.push({ k: 'c1', lat: -0.14, w: 0.12, c: yellow }, { k: 'c2', lat: 0.14, w: 0.12, c: yellow });
        const lane = hw / 2;
        L.push({ k: 'bl', lat: -lane, w: 0.13, c: white, dash: [3, 9] }, { k: 'br', lat: lane, w: 0.13, c: white, dash: [3, 9] });
      } else if (marks === 'dashed') {
        L.push({ k: 'c', lat: 0, w: 0.13, c: yellow, dash: [4, 11] });
        L.push({ k: 'el', lat: -(hw - 0.3), w: 0.13, c: white }, { k: 'er', lat: hw - 0.3, w: 0.13, c: white });
      } else if (marks === 'guide') {
        // Dry lake course: one dark guide line down the middle, no edges.
        L.push({ k: 'g', lat: 0, w: 0.3, c: [0.09, 0.08, 0.08] });
      } else if (marks === 'freeway' && hw > 9.0) {
        const left = -hw + 1.2, right = hw - 2.0;
        const lw = (right - left) / 4;
        L.push({ k: 'fl', lat: left, w: 0.15, c: yellow }, { k: 'fr', lat: right, w: 0.18, c: white });
        for (let q = 1; q < 4; q++) L.push({ k: 'f' + q, lat: left + lw * q, w: 0.14, c: white, dash: [3, 12] });
      } else {
        L.push({ k: 'el', lat: -(hw - 0.3), w: 0.15, c: white }, { k: 'er', lat: hw - 0.3, w: 0.15, c: white });
      }
      return { L, f: { ...f } };
    };
    let prev = linesAt(0);
    for (let s = 1; s <= t.length; s += 1) {
      const cur = linesAt(s);
      for (const a of prev.L) {
        const b = cur.L.find((x) => x.k === a.k);
        if (!b) continue;
        if (a.dash) {
          const ph = (s - 0.5) % a.dash[1];
          if (ph > a.dash[0]) continue;
        }
        const base = pos.length / 3;
        for (const [fr, ln, ss] of [[prev.f, a, s - 1], [cur.f, b, s]]) {
          for (const side of [-1, 1]) {
            const lat = ln.lat + side * ln.w * 0.5;
            pos.push(fr.x + fr.rx * lat, fr.y - lat * fr.bank + 0.018, fr.z + fr.rz * lat);
            col.push(...ln.c);
            uvs.push(lat * 8, ss);
          }
        }
        idx.push(base, base + 1, base + 2, base + 1, base + 3, base + 2);
      }
      prev = cur;
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
    g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(uvs, 2));
    g.setIndex(idx);
    g.computeVertexNormals();
    const m = new THREE.MeshStandardMaterial({
      vertexColors: true, roughness: 0.55, metalness: 0,
      emissive: 0xffffff, emissiveIntensity: 0.0,
      polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2,
    });
    patchMarkings(m, this.uniforms);
    this.materials.markings = m;
    const mesh = new THREE.Mesh(g, m);
    mesh.receiveShadow = true;
    this.group.add(mesh);
  }

  buildLines() {
    // Start and finish lines — checkered bands across the road.
    const t = this.track;
    const tex = checkerTexture(10);
    const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.6, polygonOffset: true, polygonOffsetFactor: -3, polygonOffsetUnits: -3 });
    // A circuit starts and finishes on the same line.
    for (const s of t.loop ? (t.laps ? [t.startS] : []) : [t.startS, t.finishS]) {
      const g = extrude(t, [[s - 1.2, s + 1.2]], [
        { lat: (f) => -f.hw, dy: 0.022, u: 0 },
        { lat: (f) => f.hw, dy: 0.022, u: 1 },
      ], { step: 2.4, vScale: 2.4 });
      // u runs across the road (checker columns), v along it (4 rows).
      const uv = g.getAttribute('uv');
      const v0 = uv.getY(0);
      for (let k = 0; k < uv.count; k++) uv.setY(k, uv.getY(k) - v0);
      this.group.add(new THREE.Mesh(g, mat));
    }
  }

  buildBarriers() {
    const t = this.track;
    const steel = this.mat('steel', () => new THREE.MeshStandardMaterial({ color: 0xa9adb3, metalness: 0.45, roughness: 0.55, side: THREE.DoubleSide }));
    const post = this.mat('post', () => new THREE.MeshStandardMaterial({ color: 0x6d6f73, metalness: 0.5, roughness: 0.5 }));
    const wood = this.mat('wood', () => new THREE.MeshStandardMaterial({ color: 0x7a6248, roughness: 0.95 }));
    const concrete = this.mat('concrete', () => new THREE.MeshStandardMaterial({ map: concreteTexture(), roughness: 0.9, color: 0xd8d5ce }));

    const sideArr = (side) => (side < 0 ? this.sideL : this.sideR);

    for (const side of [-1, 1]) {
      const arr = sideArr(side);
      // Guardrail W-beam: a vertical band.
      const gr = runs(t, (s) => arr[t.idx(s)] === 2);
      for (const r of groupRuns(gr, 560)) {
        const prof = [
          { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.15), dy: 0.48, u: 0 },
          { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.1), dy: 0.64, u: 0.5 },
          { lat: (f) => side * (f.hw + (side < 0 ? f.wallL - f.hw : f.wallR - f.hw) + 0.15), dy: 0.8, u: 1 },
        ];
        const g = extrude(t, r, prof, { step: 2, vScale: 4 });
        const m = new THREE.Mesh(g, steel);
        m.castShadow = true;
        this.group.add(m);
      }
      this.postInstances(gr, side, 4, post, [0.12, 0.85, 0.12], 0.25);

      // Wooden fence: two rails + posts.
      const gaps = t.fenceGaps.filter((g) => g.side === side);
      const fr = runs(t, (s) => arr[t.idx(s)] === 3 && !gaps.some((g) => s > g.s0 && s < g.s1), 1);
      for (const r of groupRuns(fr, 560)) {
        for (const h of [0.55, 1.0]) {
          const prof = [
            { lat: (f) => side * ((side < 0 ? f.wallL : f.wallR) + 0.25), dy: h - 0.07, u: 0 },
            { lat: (f) => side * ((side < 0 ? f.wallL : f.wallR) + 0.25), dy: h + 0.07, u: 1 },
          ];
          const g = extrude(t, r, side < 0 ? prof : prof.slice().reverse(), { step: 3, vScale: 3 });
          const m = new THREE.Mesh(g, wood);
          m.material.side = THREE.DoubleSide;
          m.castShadow = true;
          this.group.add(m);
        }
      }
      this.postInstances(fr, side, 3, wood, [0.14, 1.2, 0.14], 0.3);

      // Jersey barrier: extruded profile.
      const jr = runs(t, (s) => arr[t.idx(s)] === 4);
      for (const r of groupRuns(jr, 560)) {
        const w = (f) => (side < 0 ? f.wallL : f.wallR);
        const prof = [
          { lat: (f) => side * (w(f) - 0.02), dy: 0, u: 0 },
          { lat: (f) => side * (w(f) + 0.02), dy: 0.25, u: 0.2 },
          { lat: (f) => side * (w(f) + 0.2), dy: 0.36, u: 0.35 },
          { lat: (f) => side * (w(f) + 0.28), dy: 1.0, u: 0.7 },
          { lat: (f) => side * (w(f) + 0.42), dy: 1.0, u: 0.8 },
          { lat: (f) => side * (w(f) + 0.62), dy: 0.25, u: 0.9 },
          { lat: (f) => side * (w(f) + 0.62), dy: -1.8, u: 1 },
        ];
        const g = extrude(t, r, side > 0 ? prof : prof.slice().reverse(), { step: 4, vScale: 3 });
        const m = new THREE.Mesh(g, concrete);
        m.castShadow = true;
        m.receiveShadow = true;
        this.group.add(m);
      }

      // Kerb + pipe railing (seafront promenade).
      const rr = runs(t, (s) => arr[t.idx(s)] === 5 && !gaps.some((g) => s > g.s0 && s < g.s1), 1);
      const paint = this.mat('railPaint', () => new THREE.MeshStandardMaterial({ color: 0xdfe6e8, metalness: 0.4, roughness: 0.45, side: THREE.DoubleSide }));
      for (const r of groupRuns(rr, 560)) {
        const w = (f) => (side < 0 ? f.wallL : f.wallR);
        const kerb = [
          { lat: (f) => side * (w(f) - 0.05), dy: 0, u: 0 },
          { lat: (f) => side * (w(f) - 0.05), dy: 0.22, u: 0.3 },
          { lat: (f) => side * (w(f) + 0.35), dy: 0.22, u: 0.7 },
          { lat: (f) => side * (w(f) + 0.35), dy: -1.2, u: 1 },
        ];
        const gk = extrude(t, r, side > 0 ? kerb : kerb.slice().reverse(), { step: 4, vScale: 3 });
        const mk = new THREE.Mesh(gk, concrete);
        mk.receiveShadow = true;
        this.group.add(mk);
        for (const h of [0.62, 1.08]) {
          const bar = [
            { lat: (f) => side * (w(f) + 0.15), dy: h - 0.03, u: 0 },
            { lat: (f) => side * (w(f) + 0.15), dy: h + 0.03, u: 1 },
          ];
          const gb = extrude(t, r, bar, { step: 3, vScale: 3 });
          const mb = new THREE.Mesh(gb, paint);
          mb.castShadow = true;
          this.group.add(mb);
        }
      }
      this.postInstances(rr, side, 2.5, paint, [0.06, 1.1, 0.06], 0.15);
    }
  }

  postInstances(ranges, side, spacing, material, size, extra) {
    const t = this.track;
    const list = [];
    const f = {};
    for (const [s0, s1] of ranges) for (let s = s0; s <= s1; s += spacing) list.push(s);
    if (!list.length) return;
    const geo = new THREE.BoxGeometry(size[0], size[1], size[2]);
    geo.translate(0, size[1] / 2, 0);
    const im = new THREE.InstancedMesh(geo, material, list.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler();
    list.forEach((s, k) => {
      t.frame(s, f);
      const lat = side * ((side < 0 ? f.wallL : f.wallR) + extra);
      const x = f.x + f.rx * lat, z = f.z + f.rz * lat, y = f.y - lat * f.bank - 0.1;
      e.set(0, -Math.atan2(f.fz, f.fx), 0);
      q.setFromEuler(e);
      m4.compose(new THREE.Vector3(x, y, z), q, new THREE.Vector3(1, 1, 1));
      im.setMatrixAt(k, m4);
    });
    im.castShadow = true;
    im.receiveShadow = true;
    im.computeBoundingSphere();
    this.group.add(im);
  }

  buildChevrons() {
    const t = this.track;
    const tex = chevronTexture();
    const mat = new THREE.MeshStandardMaterial({ map: tex, roughness: 0.5, emissive: 0xffffff, emissiveMap: tex, emissiveIntensity: 0.12, side: THREE.DoubleSide });
    this.materials.chevron = mat;
    const items = [];
    const f = {};
    let last = -100;
    for (let s = 0; s < t.length; s += 1) {
      const k = t.kSmooth[t.idx(s)];
      if (['freeway', 'boulevard', 'street', 'playa', 'circuit', 'circuitWide'].includes(ROAD_KEYS[t.roadType[t.idx(s)]])) continue;
      if (Math.abs(k) > 0.03 && s - last > 11) {
        items.push({ s, side: k > 0 ? -1 : 1, dir: k > 0 ? 1 : -1 });
        last = s;
      }
    }
    if (!items.length) return;
    const plate = new THREE.PlaneGeometry(0.75, 0.75);
    plate.translate(0, 1.35, 0);
    const pole = new THREE.CylinderGeometry(0.04, 0.04, 1.1, 5);
    pole.translate(0, 0.55, 0);
    const imPlate = new THREE.InstancedMesh(plate, mat, items.length);
    const imPole = new THREE.InstancedMesh(pole, this.mat('post', () => new THREE.MeshStandardMaterial({ color: 0x777777 })), items.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), v = new THREE.Vector3(), sc = new THREE.Vector3();
    items.forEach((it, k) => {
      t.frame(it.s, f);
      const lat = it.side * ((it.side < 0 ? f.wallL : f.wallR) + 0.9);
      v.set(f.x + f.rx * lat, f.y - lat * f.bank, f.z + f.rz * lat);
      // Face back down the road toward approaching drivers.
      const yaw = Math.atan2(-f.fx, -f.fz);
      e.set(0, yaw, 0);
      q.setFromEuler(e);
      sc.set(it.dir, 1, 1);
      m4.compose(v, q, sc);
      imPlate.setMatrixAt(k, m4);
      sc.set(1, 1, 1);
      m4.compose(v, q, sc);
      imPole.setMatrixAt(k, m4);
    });
    imPlate.computeBoundingSphere();
    imPole.computeBoundingSphere();
    this.group.add(imPlate, imPole);
  }

  buildViaduct() {
    // Elevated freeway: piers and a deck underside where the road is well
    // above the city ground.
    const t = this.track, T = this.terrain;
    const cityY = T.cityY;
    const elevated = (s) => T.isElevated(t.idx(s));
    // Cable-stayed main spans have no piers; the harbour scenery builds those.
    const spans = t.tags.filter((g) => g.tag === 'bridge');
    const concrete = this.mat('concrete', () => new THREE.MeshStandardMaterial({ map: concreteTexture(), roughness: 0.9 }));
    const ranges = runs(t, elevated, 2);
    for (const r of groupRuns(ranges, 560)) {
      const g = extrude(t, r, [
        { lat: (f) => f.wallR + 0.62, dy: -1.8 },
        { lat: (f) => -(f.wallL + 0.62), dy: -1.8 },
      ], { step: 4 });
      this.group.add(new THREE.Mesh(g, concrete));
    }
    const piers = [];
    for (const [s0, s1] of ranges) for (let s = s0 + 15; s < s1 - 5; s += 32) {
      if (!spans.some((g) => s > g.s0 - 10 && s < g.s1 + 10)) piers.push(s);
    }
    if (!piers.length) return;
    const f = {};
    const colGeo = new THREE.CylinderGeometry(1.1, 1.3, 1, 10);
    colGeo.translate(0, 0.5, 0);
    const capGeo = new THREE.BoxGeometry(2.2, 1.2, 1);
    const imCol = new THREE.InstancedMesh(colGeo, concrete, piers.length * 2);
    const imCap = new THREE.InstancedMesh(capGeo, concrete, piers.length);
    const m4 = new THREE.Matrix4(), q = new THREE.Quaternion(), e = new THREE.Euler(), v = new THREE.Vector3(), sc = new THREE.Vector3();
    piers.forEach((s, k) => {
      t.frame(s, f);
      const yaw = -Math.atan2(f.fz, f.fx);
      e.set(0, yaw, 0); q.setFromEuler(e);
      const top = f.y - 2.8;
      for (const [j, lat] of [[0, -f.hw * 0.55], [1, f.hw * 0.55]]) {
        const x = f.x + f.rx * lat, z = f.z + f.rz * lat;
        const g = T.heightAt(x, z) - 0.5;
        v.set(x, g, z); sc.set(1, Math.max(0.5, top - g), 1);
        m4.compose(v, q, sc);
        imCol.setMatrixAt(k * 2 + j, m4);
      }
      // Crossbeam: local z maps onto the road's right vector, so stretch z.
      v.set(f.x, top + 0.4, f.z); sc.set(1, 1, f.hw * 2 + 2);
      m4.compose(v, q, sc);
      imCap.setMatrixAt(k, m4);
    });
    imCol.castShadow = imCap.castShadow = true;
    imCol.computeBoundingSphere();
    imCap.computeBoundingSphere();
    this.group.add(imCol, imCap);
  }
}
