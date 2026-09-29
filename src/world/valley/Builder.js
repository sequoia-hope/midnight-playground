import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';

// Collects primitive pieces into per-material buckets, then merges each
// bucket into a single static mesh. Farm buildings are made of hundreds of
// boxes; this keeps them to one draw call per material for the whole valley.

const UNIT_BOX = new THREE.BoxGeometry(1, 1, 1);
const KEEP = ['position', 'normal', 'uv'];

function normalise(geo) {
  let g = geo.index ? geo.toNonIndexed() : geo.clone();
  for (const name of Object.keys(g.attributes)) if (!KEEP.includes(name)) g.deleteAttribute(name);
  if (!g.attributes.normal) g.computeVertexNormals();
  if (!g.attributes.uv) g.setAttribute('uv', new THREE.Float32BufferAttribute(new Float32Array(g.attributes.position.count * 2), 2));
  g.morphAttributes = {};
  g.clearGroups();
  return g;
}

export class Builder {
  constructor() {
    this.buckets = new Map();
    this.frame = new THREE.Matrix4();
    this._m = new THREE.Matrix4();
    this._l = new THREE.Matrix4();
    this._q = new THREE.Quaternion();
    this._e = new THREE.Euler();
    this._v = new THREE.Vector3();
    this._s = new THREE.Vector3();
  }

  // Place subsequent pieces relative to (x,y,z) rotated by yaw about Y.
  setFrame(x, y, z, yaw = 0) {
    this._e.set(0, yaw, 0);
    this._q.setFromEuler(this._e);
    this.frame.compose(this._v.set(x, y, z), this._q, this._s.set(1, 1, 1));
    return this;
  }

  pushFrame(x, y, z, yaw = 0) {
    this._stack = this._stack || [];
    this._stack.push(this.frame.clone());
    this._e.set(0, yaw, 0);
    this._q.setFromEuler(this._e);
    this._l.compose(this._v.set(x, y, z), this._q, this._s.set(1, 1, 1));
    this.frame.multiply(this._l);
    return this;
  }

  popFrame() {
    this.frame.copy(this._stack.pop());
    return this;
  }

  add(key, geo, local) {
    const g = normalise(geo);
    this._m.multiplyMatrices(this.frame, local || this._l.identity());
    g.applyMatrix4(this._m);
    if (!this.buckets.has(key)) this.buckets.set(key, []);
    this.buckets.get(key).push(g);
    return g;
  }

  // Generic transformed primitive.
  put(key, geo, x, y, z, rx = 0, ry = 0, rz = 0, sx = 1, sy = 1, sz = 1) {
    this._e.set(rx, ry, rz);
    this._q.setFromEuler(this._e);
    this._l.compose(this._v.set(x, y, z), this._q, this._s.set(sx, sy, sz));
    return this.add(key, geo, this._l);
  }

  // Box whose base sits at y (not centred).
  box(key, w, h, d, x, y, z, ry = 0, rx = 0, rz = 0) {
    this._e.set(rx, ry, rz);
    this._q.setFromEuler(this._e);
    // Offset so the pivot is at the bottom centre of the box.
    const off = new THREE.Vector3(0, h / 2, 0).applyQuaternion(this._q);
    this._l.compose(this._v.set(x + off.x, y + off.y, z + off.z), this._q, this._s.set(w, h, d));
    return this.add(key, UNIT_BOX, this._l);
  }

  // Box centred at (x,y,z).
  cbox(key, w, h, d, x, y, z, rx = 0, ry = 0, rz = 0) {
    return this.put(key, UNIT_BOX, x, y, z, rx, ry, rz, w, h, d);
  }

  // Thin beam between two local points.
  beam(key, a, b, t = 0.12) {
    const dir = new THREE.Vector3().subVectors(b, a);
    const len = dir.length();
    dir.normalize();
    const q = new THREE.Quaternion().setFromUnitVectors(new THREE.Vector3(0, 1, 0), dir);
    const mid = new THREE.Vector3().addVectors(a, b).multiplyScalar(0.5);
    this._l.compose(mid, q, this._s.set(t, len, t));
    return this.add(key, UNIT_BOX, this._l);
  }

  // Build meshes: one per bucket. materials: {key: Material}.
  build(materials, { castShadow = [], receiveShadow = true } = {}) {
    const meshes = [];
    for (const [key, geos] of this.buckets) {
      const mat = materials[key];
      if (!mat) { console.warn('Valley: no material for', key); continue; }
      const merged = mergeGeometries(geos, false);
      geos.forEach((g) => g.dispose());
      if (!merged) continue;
      merged.computeBoundingSphere();
      const mesh = new THREE.Mesh(merged, mat);
      mesh.name = 'valley:' + key;
      mesh.castShadow = castShadow === true || castShadow.includes(key);
      mesh.receiveShadow = receiveShadow;
      mesh.matrixAutoUpdate = false;
      mesh.updateMatrix();
      meshes.push(mesh);
    }
    this.buckets.clear();
    return meshes;
  }

  // Merge everything into one geometry (for instanced parts).
  mergeAll() {
    const all = [];
    for (const geos of this.buckets.values()) all.push(...geos);
    const merged = mergeGeometries(all, false);
    all.forEach((g) => g.dispose());
    this.buckets.clear();
    return merged;
  }
}

// A Builder that paints plain-coloured pieces with vertex colours and files
// them under a shared bucket, so the many flat paint colours of the valley's
// buildings cost one draw call per *material class* instead of one per
// colour. groups: {key: [bucket, hexColour]}; keys not listed keep their own
// bucket (textured, emissive or transparent materials).
export class PaintBuilder extends Builder {
  constructor(groups) {
    super();
    this.groups = new Map(Object.entries(groups).map(([k, [bucket, hex]]) => [k, [bucket, new THREE.Color(hex)]]));
  }

  add(key, geo, local) {
    const grp = this.groups.get(key);
    if (!grp) return super.add(key, geo, local);
    const g = normalise(geo);
    this._m.multiplyMatrices(this.frame, local || this._l.identity());
    g.applyMatrix4(this._m);
    const [bucket, col] = grp;
    const n = g.attributes.position.count;
    const c = new Float32Array(n * 3);
    for (let i = 0; i < n; i++) { c[i * 3] = col.r; c[i * 3 + 1] = col.g; c[i * 3 + 2] = col.b; }
    g.setAttribute('color', new THREE.BufferAttribute(c, 3));
    if (!this.buckets.has(bucket)) this.buckets.set(bucket, []);
    this.buckets.get(bucket).push(g);
    return g;
  }
}
