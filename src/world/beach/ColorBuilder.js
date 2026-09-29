import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { Builder } from '../valley/Builder.js';

// A Builder that paints plain-coloured pieces with vertex colours and merges
// them into one "solid" bucket per channel, instead of one mesh per paint
// colour. Keys found in `palette` go to solid:<channel>; anything else
// (glass, emissive, textured, transparent) keeps its own bucket as usual.
// Set `channel` before drawing ('near' casts shadows, 'far' doesn't).

function strip(geo) {
  const g = geo.index ? geo.toNonIndexed() : geo.clone();
  for (const name of Object.keys(g.attributes)) if (!['position', 'normal', 'uv'].includes(name)) g.deleteAttribute(name);
  if (!g.attributes.normal) g.computeVertexNormals();
  if (!g.attributes.uv) g.setAttribute('uv', new THREE.Float32BufferAttribute(new Float32Array(g.attributes.position.count * 2), 2));
  g.morphAttributes = {};
  g.clearGroups();
  return g;
}

export class ColorBuilder extends Builder {
  constructor(palette) {
    super();
    this.palette = new Map(Object.entries(palette).map(([k, hex]) => [k, new THREE.Color(hex)]));
    this.channel = 'near';
  }

  add(key, geo, local) {
    const col = this.palette.get(key);
    const g = strip(geo);
    this._m.multiplyMatrices(this.frame, local || this._l.identity());
    g.applyMatrix4(this._m);
    let bucket = key;
    if (col) {
      const n = g.attributes.position.count;
      const c = new Float32Array(n * 3);
      for (let i = 0; i < n; i++) { c[i * 3] = col.r; c[i * 3 + 1] = col.g; c[i * 3 + 2] = col.b; }
      g.setAttribute('color', new THREE.BufferAttribute(c, 3));
      bucket = 'solid:' + this.channel;
    }
    if (!this.buckets.has(bucket)) this.buckets.set(bucket, []);
    this.buckets.get(bucket).push(g);
    return g;
  }

  build(materials, { castShadow = [], receiveShadow = true } = {}) {
    const meshes = [];
    const solid = materials.solid || new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.85 });
    for (const [key, geos] of this.buckets) {
      const isSolid = key.startsWith('solid:');
      const mat = isSolid ? solid : materials[key];
      if (!mat) { console.warn('Beach: no material for', key); continue; }
      const merged = mergeGeometries(geos, false);
      geos.forEach((g) => g.dispose());
      if (!merged) continue;
      merged.computeBoundingSphere();
      const mesh = new THREE.Mesh(merged, mat);
      mesh.name = 'beach:' + key;
      mesh.castShadow = isSolid ? key === 'solid:near' : castShadow === true || castShadow.includes(key);
      mesh.receiveShadow = receiveShadow;
      mesh.matrixAutoUpdate = false;
      mesh.updateMatrix();
      meshes.push(mesh);
    }
    this.buckets.clear();
    return meshes;
  }
}
