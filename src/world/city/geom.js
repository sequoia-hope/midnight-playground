import * as THREE from 'three';

// Accumulates flat-shaded quads into one non-indexed BufferGeometry. The
// city emits thousands of boxes, so everything of one material is gathered
// here and handed to the GPU as a single mesh per spatial chunk.

const _ab = new THREE.Vector3(), _ac = new THREE.Vector3(), _n = new THREE.Vector3();

export class GeoBuilder {
  constructor({ color = false, cell = false } = {}) {
    this.pos = [];
    this.nor = [];
    this.uv = [];
    this.col = color ? [] : null;
    this.cell = cell ? [] : null;
  }

  get empty() { return this.pos.length === 0; }

  // a,b,c,d: [x,y,z] counter-clockwise seen from the front face.
  // uvs: [[u,v] x4]. c3: optional rgb for all four corners. cell: atlas cell.
  quad(a, b, c, d, uvs = null, c3 = null, cell = 0) {
    _ab.set(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    _ac.set(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
    _n.crossVectors(_ab, _ac);
    if (_n.lengthSq() < 1e-12) {
      _ac.set(d[0] - a[0], d[1] - a[1], d[2] - a[2]);
      _n.crossVectors(_ab, _ac);
    }
    if (_n.lengthSq() < 1e-12) {
      // a and b coincide: take the normal from the other triangle (a, c, d).
      _ab.set(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
      _ac.set(d[0] - a[0], d[1] - a[1], d[2] - a[2]);
      _n.crossVectors(_ab, _ac);
    }
    // A zero normal lights to NaN, which bloom then smears across the screen.
    if (_n.lengthSq() < 1e-12) _n.set(0, 1, 0);
    _n.normalize();
    const V = [a, b, c, a, c, d];
    const U = uvs ? [uvs[0], uvs[1], uvs[2], uvs[0], uvs[2], uvs[3]] : null;
    for (let k = 0; k < 6; k++) {
      this.pos.push(V[k][0], V[k][1], V[k][2]);
      this.nor.push(_n.x, _n.y, _n.z);
      if (U) this.uv.push(U[k][0], U[k][1]); else this.uv.push(0, 0);
      if (this.col) { const c = c3 || [1, 1, 1]; this.col.push(c[0], c[1], c[2]); }
      if (this.cell) this.cell.push(cell);
    }
  }

  tri(a, b, c, c3 = null, cell = 0) {
    _ab.set(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    _ac.set(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
    _n.crossVectors(_ab, _ac);
    if (_n.lengthSq() < 1e-12) _n.set(0, 1, 0);
    _n.normalize();
    for (const v of [a, b, c]) {
      this.pos.push(v[0], v[1], v[2]);
      this.nor.push(_n.x, _n.y, _n.z);
      this.uv.push(0, 0);
      if (this.col) { const q = c3 || [1, 1, 1]; this.col.push(q[0], q[1], q[2]); }
      if (this.cell) this.cell.push(cell);
    }
  }

  // Vertical walls around a closed footprint (xz corners, any winding) plus
  // an optional roof. Wall UVs are in texture tiles: u = metres / tileW
  // along the wall, v = (y - vRef) / tileH.
  prism(corners, y0, y1, o = {}) {
    const {
      tileW = 20, tileH = 40, uOff = 0, vOff = 0, vRef = y0, cell = 0,
      roof = true, roofCell = 6, roofTile = 16, color = null, roofColor = null,
    } = o;
    const n = corners.length;
    // Ensure clockwise-in-xz order (negative signed area) → outward normals.
    let area = 0;
    for (let i = 0; i < n; i++) {
      const p = corners[i], q = corners[(i + 1) % n];
      area += p[0] * q[1] - q[0] * p[1];
    }
    const C = area > 0 ? corners.slice().reverse() : corners;
    let u = uOff;
    for (let i = 0; i < n; i++) {
      const p = C[i], q = C[(i + 1) % n];
      const len = Math.hypot(q[0] - p[0], q[1] - p[1]);
      const u1 = u + len / tileW;
      const v0 = (y0 - vRef) / tileH + vOff, v1 = (y1 - vRef) / tileH + vOff;
      this.quad([p[0], y0, p[1]], [q[0], y0, q[1]], [q[0], y1, q[1]], [p[0], y1, p[1]],
        [[u, v0], [u1, v0], [u1, v1], [u, v1]], color, cell);
      u = u1;
    }
    if (roof) {
      const uvr = C.map((p) => [p[0] / roofTile, p[1] / roofTile]);
      const top = C.map((p) => [p[0], y1, p[1]]);
      for (let i = 1; i < n - 1; i++) {
        // Roof: fan, wound so the normal points up.
        this.triUV(top[0], top[i + 1], top[i], uvr[0], uvr[i + 1], uvr[i], roofColor || color, roofCell);
      }
    }
  }

  triUV(a, b, c, ua, ub, uc, c3 = null, cell = 0) {
    _ab.set(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    _ac.set(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
    _n.crossVectors(_ab, _ac);
    if (_n.lengthSq() < 1e-12) _n.set(0, 1, 0);
    _n.normalize();
    if (_n.y < 0 && Math.abs(_n.y) > 0.9) {
      // Guarantee roofs face up regardless of footprint winding.
      [b, c] = [c, b];
      [ub, uc] = [uc, ub];
      _n.negate();
    }
    const V = [a, b, c], U = [ua, ub, uc];
    for (let k = 0; k < 3; k++) {
      this.pos.push(V[k][0], V[k][1], V[k][2]);
      this.nor.push(_n.x, _n.y, _n.z);
      this.uv.push(U[k][0], U[k][1]);
      if (this.col) { const q = c3 || [1, 1, 1]; this.col.push(q[0], q[1], q[2]); }
      if (this.cell) this.cell.push(cell);
    }
  }

  // Axis-free box: centre (x,y,z) bottom-centred at y, size (sx, sy, sz)
  // with sx along direction yaw (radians, measured like atan2(dz, dx)).
  box(x, y, z, sx, sy, sz, yaw = 0, o = {}) {
    const c = Math.cos(yaw), s = Math.sin(yaw);
    const hx = sx / 2, hz = sz / 2;
    const pts = [[-hx, -hz], [hx, -hz], [hx, hz], [-hx, hz]].map(([a, b]) => [x + a * c - b * s, z + a * s + b * c]);
    this.prism(pts, y, y + sy, { tileW: o.tileW ?? 4, tileH: o.tileH ?? 4, ...o });
    if (o.bottom) {
      const bot = pts.map((p) => [p[0], y, p[1]]);
      this.quad(bot[0], bot[1], bot[2], bot[3], [[0, 0], [1, 0], [1, 1], [0, 1]], o.color || null, o.cell || 0);
    }
  }

  build() {
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.Float32BufferAttribute(this.pos, 3));
    g.setAttribute('normal', new THREE.Float32BufferAttribute(this.nor, 3));
    g.setAttribute('uv', new THREE.Float32BufferAttribute(this.uv, 2));
    if (this.col) g.setAttribute('color', new THREE.Float32BufferAttribute(this.col, 3));
    if (this.cell) g.setAttribute('cell', new THREE.Float32BufferAttribute(this.cell, 1));
    g.computeBoundingSphere();
    return g;
  }
}

// A static mesh that never moves.
export function staticMesh(geo, mat, { cast = false, receive = true, name = '' } = {}) {
  const m = new THREE.Mesh(geo, mat);
  m.castShadow = cast;
  m.receiveShadow = receive;
  m.matrixAutoUpdate = false;
  m.updateMatrix();
  if (name) m.name = name;
  return m;
}

// Instanced mesh from a list of matrices.
export function instanced(geo, mat, matrices, { cast = false, receive = false } = {}) {
  const im = new THREE.InstancedMesh(geo, mat, Math.max(1, matrices.length));
  matrices.forEach((m, i) => im.setMatrixAt(i, m));
  im.count = matrices.length;
  im.castShadow = cast;
  im.receiveShadow = receive;
  im.matrixAutoUpdate = false;
  im.updateMatrix();
  im.computeBoundingSphere();
  im.computeBoundingBox?.();
  return im;
}

const _m = new THREE.Matrix4(), _q = new THREE.Quaternion(), _e = new THREE.Euler(), _p = new THREE.Vector3(), _s = new THREE.Vector3();
export function trs(x, y, z, yaw = 0, sx = 1, sy = 1, sz = 1, pitch = 0, roll = 0) {
  _e.set(pitch, yaw, roll, 'YXZ');
  _q.setFromEuler(_e);
  _p.set(x, y, z);
  _s.set(sx, sy, sz);
  return new THREE.Matrix4().compose(_p, _q, _s);
}

// three.js yaw (rotation about +Y) that turns local +X onto direction (dx, dz).
export const yawOf = (dx, dz) => -Math.atan2(dz, dx);
