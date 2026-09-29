import * as THREE from 'three';
import { mergeGeometries, mergeVertices } from 'three/addons/utils/BufferGeometryUtils.js';
import { clamp, lerp, smoothstep, mulberry32 } from '../../util/math.js';

// Vegetation and rock geometry shared by the Sierra Pass and Old Mill
// Valley scenery. Everything here is a small template for InstancedMesh:
// shading comes from baked vertex colours (ambient occlusion, strata, tip
// highlights) and hand-set normals rather than extra triangles, so the
// instances stay cheap enough for a phone. Instance colour multiplies the
// vertex colour, which is how each copy gets its own tone.

// ── 3-D value noise (rock displacement) ─────────────────────────────
export function makeNoise3D(seed = 1) {
  const rng = mulberry32(seed);
  const P = new Uint8Array(512), R = new Float32Array(256);
  for (let i = 0; i < 256; i++) { P[i] = i; R[i] = rng() * 2 - 1; }
  for (let i = 255; i > 0; i--) { const j = Math.floor(rng() * (i + 1)); const t = P[i]; P[i] = P[j]; P[j] = t; }
  for (let i = 0; i < 256; i++) P[i + 256] = P[i];
  const h = (x, y, z) => R[P[P[P[x & 255] + (y & 255)] + (z & 255)]];
  const f = (t) => t * t * (3 - 2 * t);
  return (x, y, z) => {
    const xi = Math.floor(x), yi = Math.floor(y), zi = Math.floor(z);
    const u = f(x - xi), v = f(y - yi), w = f(z - zi);
    const a = lerp(lerp(h(xi, yi, zi), h(xi + 1, yi, zi), u), lerp(h(xi, yi + 1, zi), h(xi + 1, yi + 1, zi), u), v);
    const b = lerp(lerp(h(xi, yi, zi + 1), h(xi + 1, yi, zi + 1), u), lerp(h(xi, yi + 1, zi + 1), h(xi + 1, yi + 1, zi + 1), u), v);
    return lerp(a, b, w);
  };
}

function setColors(geo, fn) {
  const pos = geo.getAttribute('position'), nrm = geo.getAttribute('normal');
  const a = new Float32Array(pos.count * 3);
  const c = [0, 0, 0];
  for (let i = 0; i < pos.count; i++) {
    fn(pos.getX(i), pos.getY(i), pos.getZ(i), nrm ? nrm.getY(i) : 0, c, i);
    a[i * 3] = c[0]; a[i * 3 + 1] = c[1]; a[i * 3 + 2] = c[2];
  }
  geo.setAttribute('color', new THREE.BufferAttribute(a, 3));
  return geo;
}

// ── Rocks ─────────────────────────────────────────────────────────
// A fractured boulder: a noise-displaced sphere with a few random cleavage
// planes shaved off, so it has broad flat faces and softened edges instead
// of the lumpy-potato look. Welded, so shading is smooth; vertex colours
// carry sedimentary banding, dark crevices and lichen on upward faces.
// `crag` makes it blockier (more, deeper cuts) for outcrops set in a slope.
export function rockGeometry(seed, detail = 2, { squash = 0.74, crag = false, lichen = 1, tint = [0.62, 0.6, 0.57] } = {}) {
  let g = new THREE.IcosahedronGeometry(1, detail);
  g.deleteAttribute('normal');
  g.deleteAttribute('uv');
  g = mergeVertices(g);
  const n3 = makeNoise3D(seed);
  const rng = mulberry32(seed * 7 + 3);
  const planes = [];
  const nPl = crag ? 7 : 5;
  for (let k = 0; k < nPl; k++) {
    const th = rng() * Math.PI * 2, ph = Math.acos(rng() * 1.6 - 0.6); // bias away from the base
    const nx = Math.sin(ph) * Math.cos(th), ny = Math.cos(ph), nz = Math.sin(ph) * Math.sin(th);
    planes.push([nx, ny, nz, lerp(crag ? 0.5 : 0.62, crag ? 0.78 : 0.9, rng())]);
  }
  // The top is usually a flatter bedding plane.
  planes.push([0, 1, 0, crag ? 0.55 : 0.7]);
  const pos = g.getAttribute('position');
  const disp = new Float32Array(pos.count);
  for (let i = 0; i < pos.count; i++) {
    let x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    const d = n3(x * 1.6 + 11, y * 1.6, z * 1.6) * 0.22 + n3(x * 4.1, y * 4.1 + 7, z * 4.1) * 0.07;
    let r = 1 + d;
    x *= r; y *= r; z *= r;
    for (const [nx, ny, nz, off] of planes) {
      const s = x * nx + y * ny + z * nz;
      if (s > off) { const k = (s - off) * 0.9; x -= nx * k; y -= ny * k; z -= nz * k; }
    }
    y *= squash;
    if (y < -0.32) y = -0.32 - (y + 0.32) * 0.15;
    disp[i] = Math.hypot(x, y / squash, z);
    pos.setXYZ(i, x, y, z);
  }
  g.computeVertexNormals();
  const band = seed * 0.37;
  setColors(g, (x, y, z, ny, c, i) => {
    const strata = 0.88 + 0.12 * Math.sin(y * 11 + n3(x * 2, y * 2 + 3, z * 2) * 3 + band);
    const ao = clamp(0.55 + (disp[i] - 0.72) * 1.4, 0.55, 1.05) * (y < -0.2 ? 0.78 : 1);
    let r = tint[0] * strata * ao, gg = tint[1] * strata * ao, b = tint[2] * strata * ao;
    // Lichen and moss on faces that catch rain, in noisy patches.
    const l = smoothstep(0.35, 0.75, ny) * smoothstep(0.0, 0.35, n3(x * 3 + 40, y * 3, z * 3)) * lichen;
    r = lerp(r, 0.5, l * 0.55); gg = lerp(gg, 0.56, l * 0.55); b = lerp(b, 0.34, l * 0.55);
    // A few rusty-orange lichen spots.
    const o = smoothstep(0.42, 0.55, n3(x * 6 - 9, y * 6, z * 6)) * 0.5 * lichen;
    r = lerp(r, 0.72, o); gg = lerp(gg, 0.5, o); b = lerp(b, 0.3, o);
    c[0] = r; c[1] = gg; c[2] = b;
  });
  return g;
}

// ── Conifers ──────────────────────────────────────────────────────
// Each tier is a drooping skirt of branches: a jagged cone from the tier's
// apex out to branch tips that hang down, closed underneath by a shallow
// inverted cone so the tree has a dark, solid underside. Normals are bent
// outward from the trunk (a rounded "volume" normal) so the whole tree
// shades like a soft mass rather than a stack of flat facets.
function coniferParts(spec, rng) {
  const { tiers, H, R, trunkH, M, underside = true, taper = 1, droop = 0.25, lean = 0 } = spec;
  const pos = [], nrm = [], col = [], idx = [];
  const push = (x, y, z, nx, ny, nz, r, g, b) => {
    pos.push(x, y, z); const l = Math.hypot(nx, ny, nz) || 1; nrm.push(nx / l, ny / l, nz / l); col.push(r, g, b);
    return pos.length / 3 - 1;
  };
  const crownY0 = trunkH;
  for (let k = 0; k < tiers; k++) {
    const f = k / Math.max(1, tiers - 1);                  // 0 bottom → 1 top
    const y0 = crownY0 + (H - crownY0) * Math.pow(k / tiers, 0.92);
    const apex = crownY0 + (H - crownY0) * Math.min(1, Math.pow((k + 1.6) / tiers, 0.92));
    const top = k === tiers - 1 ? H : apex;
    const rad = R * Math.pow(1 - f * 0.86, taper) * lerp(0.92, 1.08, rng());
    const lightTop = lerp(0.95, 1.15, f), dark = lerp(0.38, 0.55, f);
    const base = [0.045, 0.088, 0.048];
    const a = push(lean * f, top, 0, 0, 1, 0, base[0] * lightTop * 1.15, base[1] * lightTop * 1.1, base[2] * lightTop);
    const rot = rng() * Math.PI * 2;
    const rim = [];
    for (let j = 0; j < M; j++) {
      const ang = rot + (j / M) * Math.PI * 2 + (rng() - 0.5) * 0.35;
      const long = j % 2 ? lerp(0.62, 0.8, rng()) : lerp(0.95, 1.12, rng());
      const rr = rad * long;
      const x = Math.cos(ang) * rr + lean * f, z = Math.sin(ang) * rr;
      const y = y0 - droop * rad * long + (j % 2 ? rad * 0.12 : 0);
      // Tips catch light, notches between branches are shaded.
      const t = j % 2 ? 0.72 : 1.08;
      rim.push(push(x, y, z, Math.cos(ang), 0.55, Math.sin(ang), base[0] * t * lightTop, base[1] * t * lightTop, base[2] * t * lightTop));
    }
    for (let j = 0; j < M; j++) idx.push(a, rim[(j + 1) % M], rim[j]);
    if (underside) {
      const u = push(lean * f, y0 + rad * 0.12, 0, 0, -1, 0, base[0] * dark * 0.6, base[1] * dark * 0.6, base[2] * dark * 0.6);
      for (let j = 0; j < M; j++) idx.push(u, rim[j], rim[(j + 1) % M]);
    }
  }
  const crown = new THREE.BufferGeometry();
  crown.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  crown.setAttribute('normal', new THREE.Float32BufferAttribute(nrm, 3));
  crown.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  crown.setIndex(idx);
  const trunk = new THREE.CylinderGeometry(0.1 * R / 1.8, 0.22 * R / 1.8, trunkH + 1.2, spec.trunkSides ?? 5, 1, true);
  trunk.deleteAttribute('uv');
  trunk.translate(0, (trunkH + 1.2) / 2 - 0.4, 0);
  setColors(trunk, (x, y, z, ny, c) => { const v = 0.8 + y * 0.05; c[0] = 0.08 * v; c[1] = 0.055 * v; c[2] = 0.035 * v; });
  // The far version has no trunk: at that range it is never seen.
  if (spec.noTrunk) return crown.toNonIndexed();
  return mergeGeometries([trunk.toNonIndexed(), crown.toNonIndexed()], false);
}

// kind: 'spruce' (narrow, dense), 'fir' (broad, open, taller trunk).
// lod: 0 detailed (verge), 1 mid (the forest behind), 2 far (hillsides).
export function coniferGeometry(kind = 'spruce', lod = 0, seed = 1) {
  const rng = mulberry32(seed);
  const spruce = kind === 'spruce';
  const spec = {
    H: spruce ? 8.2 : 9.0, R: spruce ? 1.75 : 2.3, trunkH: spruce ? 0.9 : 2.2,
    tiers: [spruce ? 7 : 5, 3, 2][lod], M: [8, 6, 5][lod],
    underside: lod === 0, taper: spruce ? 1.0 : 0.8, droop: spruce ? 0.3 : 0.18,
    trunkSides: lod === 0 ? 5 : 3,
  };
  if (lod === 2) { spec.trunkH = 0.8; spec.underside = false; spec.noTrunk = true; }
  return coniferParts(spec, rng);
}

// ── Broadleaf canopies ────────────────────────────────────────────
// A cluster of welded, noise-displaced lobes. Normals come from the whole
// cluster's centre (not each lobe's), so light wraps over it like a real
// crown; vertex colours darken the underside and the inner creases.
// lod 1 keeps only the main lobe and two side lobes (distant trees).
export function canopyGeometry(kind = 'shade', seed = 3, lod = 0) {
  const rng = mulberry32(seed);
  const n3 = makeNoise3D(seed + 11);
  const lobes = [];
  if (kind === 'orchard') {
    lobes.push([0, 0, 0, 1]);
    for (let k = 0; k < 5; k++) { const a = (k / 5) * Math.PI * 2 + rng(); lobes.push([Math.cos(a) * 0.55, 0.05 + rng() * 0.2, Math.sin(a) * 0.55, 0.55]); }
  } else if (kind === 'poplar') {
    for (let k = 0; k < 5; k++) lobes.push([(rng() - 0.5) * 0.25, -0.75 + k * 0.38, (rng() - 0.5) * 0.25, 0.62 - Math.abs(k - 1.6) * 0.09]);
  } else if (kind === 'willow') {
    lobes.push([0, 0.1, 0, 0.95]);
    for (let k = 0; k < 6; k++) { const a = (k / 6) * Math.PI * 2 + rng(); lobes.push([Math.cos(a) * 0.7, -0.25, Math.sin(a) * 0.7, 0.6]); }
  } else {
    lobes.push([0, 0, 0, 0.9]);
    for (let k = 0; k < 6; k++) {
      const a = (k / 6) * Math.PI * 2 + rng() * 0.6;
      lobes.push([Math.cos(a) * 0.62, (rng() - 0.3) * 0.5, Math.sin(a) * 0.62, lerp(0.45, 0.62, rng())]);
    }
    lobes.push([0.1, 0.55, -0.05, 0.55]);
  }
  if (lod > 0 && kind !== 'poplar') lobes.splice(3);
  const parts = lobes.map(([ox, oy, oz, r], k) => {
    let g = new THREE.IcosahedronGeometry(r, k === 0 && kind !== 'poplar' && lod === 0 ? 1 : 0);
    g.deleteAttribute('normal'); g.deleteAttribute('uv');
    g = mergeVertices(g);
    const p = g.getAttribute('position');
    for (let i = 0; i < p.count; i++) {
      const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
      const d = 1 + 0.2 * n3(x * 3 + ox * 5, y * 3 + k, z * 3);
      p.setXYZ(i, x * d + ox, y * d + oy, z * d + oz);
    }
    return g;
  });
  const g = mergeGeometries(parts, false);
  const p = g.getAttribute('position');
  const nrm = new Float32Array(p.count * 3);
  const col = new Float32Array(p.count * 3);
  const sy = kind === 'poplar' ? 0.35 : 1;
  for (let i = 0; i < p.count; i++) {
    const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
    const l = Math.hypot(x, (y + 0.1) * sy, z) || 1;
    const nx = x / l, ny = (y + 0.1) * sy / l + 0.15, nz = z / l;
    const ll = Math.hypot(nx, ny, nz);
    nrm[i * 3] = nx / ll; nrm[i * 3 + 1] = ny / ll; nrm[i * 3 + 2] = nz / ll;
    // Occlusion: inner points (close to the centre) and the underside are
    // darker; the sunny top a little yellower.
    const rad = Math.hypot(x, y * sy, z);
    const ao = clamp(0.45 + rad * 0.55, 0.45, 1.05) * lerp(0.62, 1.08, smoothstep(-0.8, 0.6, y * sy));
    const v = 0.9 + 0.2 * n3(x * 5, y * 5, z * 5 + 3);
    col[i * 3] = ao * v * (1 + smoothstep(0.2, 0.9, y * sy) * 0.12);
    col[i * 3 + 1] = ao * v;
    col[i * 3 + 2] = ao * v * 0.85;
  }
  g.setAttribute('normal', new THREE.BufferAttribute(nrm, 3));
  g.setAttribute('color', new THREE.BufferAttribute(col, 3));
  return g;
}

// ── Ground cover ──────────────────────────────────────────────────
// Grass tussock: a fan of bent blades (one triangle each, double-sided),
// dark at the root and sun-bleached at the tips.
export function grassClumpGeometry(blades = 9, seed = 5) {
  const rng = mulberry32(seed);
  const pos = [], col = [], nrm = [];
  for (let k = 0; k < blades; k++) {
    const a = (k / blades) * Math.PI * 2 + rng() * 0.5;
    const r0 = rng() * 0.2;
    const h = lerp(0.3, 0.8, rng());
    const lean = lerp(0.08, 0.3, rng());
    const w = lerp(0.035, 0.065, rng());
    const cx = Math.cos(a), cz = Math.sin(a);
    const bx = cx * r0, bz = cz * r0;
    const px = -cz * w, pz = cx * w;
    pos.push(bx - px, 0, bz - pz, bx + px, 0, bz + pz, bx + cx * lean, h, bz + cz * lean);
    for (let j = 0; j < 3; j++) nrm.push(cx * 0.4, 0.9, cz * 0.4);
    const tip = lerp(0.9, 1.25, rng());
    col.push(0.05, 0.06, 0.025, 0.05, 0.06, 0.025, 0.3 * tip, 0.28 * tip, 0.13 * tip);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('normal', new THREE.Float32BufferAttribute(nrm, 3));
  g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  return g;
}

// Wildflower heads: small upturned stars on short stems at varied heights.
// White vertex colour, so the instance colour is the flower's colour.
export function flowerGeometry(n = 5, seed = 9) {
  const rng = mulberry32(seed);
  const pos = [], col = [], nrm = [];
  for (let k = 0; k < n; k++) {
    const x = (rng() - 0.5) * 0.7, z = (rng() - 0.5) * 0.7, y = lerp(0.3, 0.6, rng());
    const r = lerp(0.05, 0.08, rng());
    const a0 = rng() * Math.PI;
    for (let j = 0; j < 3; j++) {
      const a = a0 + (j / 3) * Math.PI * 2;
      pos.push(x, y + 0.02, z, x + Math.cos(a) * r, y, z + Math.sin(a) * r, x + Math.cos(a + 1.2) * r, y, z + Math.sin(a + 1.2) * r);
      for (let q = 0; q < 3; q++) { nrm.push(0, 1, 0); col.push(1, 1, 1); }
    }
    // Stem.
    pos.push(x - 0.012, 0, z, x + 0.012, 0, z, x, y, z);
    for (let q = 0; q < 3; q++) { nrm.push(0, 0.5, 0.8); col.push(0.05, 0.09, 0.03); }
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.setAttribute('normal', new THREE.Float32BufferAttribute(nrm, 3));
  g.setAttribute('color', new THREE.Float32BufferAttribute(col, 3));
  return g;
}

// Low shrub (manzanita/sage on the pass, hedge filler in the valley).
export function shrubGeometry(seed = 21, detail = 1) {
  const n3 = makeNoise3D(seed);
  let g = new THREE.IcosahedronGeometry(1, detail);
  g.deleteAttribute('normal'); g.deleteAttribute('uv');
  g = mergeVertices(g);
  const p = g.getAttribute('position');
  const nrm = new Float32Array(p.count * 3), col = new Float32Array(p.count * 3);
  for (let i = 0; i < p.count; i++) {
    const x = p.getX(i), y = p.getY(i), z = p.getZ(i);
    const d = 1 + 0.28 * n3(x * 2.2, y * 2.2, z * 2.2);
    const yy = Math.max(y * 0.62 * d, -0.1) + 0.1;
    p.setXYZ(i, x * d, yy, z * d);
    const l = Math.hypot(x, y + 0.3, z);
    nrm[i * 3] = x / l; nrm[i * 3 + 1] = (y + 0.3) / l; nrm[i * 3 + 2] = z / l;
    const ao = lerp(0.5, 1.1, smoothstep(-0.2, 0.7, y)) * (0.9 + 0.2 * n3(x * 6, y * 6, z * 6));
    col[i * 3] = ao; col[i * 3 + 1] = ao; col[i * 3 + 2] = ao * 0.9;
  }
  g.setAttribute('normal', new THREE.BufferAttribute(nrm, 3));
  g.setAttribute('color', new THREE.BufferAttribute(col, 3));
  return g;
}

// Foliage material: vertex colours carry the shading, double-sided for
// blades and skirt undersides.
export function foliageMaterial(o = {}) {
  return new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.92, metalness: 0, side: THREE.DoubleSide, ...o });
}
