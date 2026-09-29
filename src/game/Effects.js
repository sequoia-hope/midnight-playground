import * as THREE from 'three';
import { clamp, lerp } from '../util/math.js';
import { smokeTexture, glowTexture } from '../world/textures.js';

// Tyre smoke, sparks, skid marks, nitro flames and fake headlight pools.

const pVert = /* glsl */`
attribute float aSize;
attribute float aAlpha;
attribute vec3 aColor;
uniform float uScale;
varying float vAlpha;
varying vec3 vColor;
#include <fog_pars_vertex>
void main() {
  vec4 mvPosition = modelViewMatrix * vec4(position, 1.0);
  gl_Position = projectionMatrix * mvPosition;
  gl_PointSize = aSize * uScale / max(0.1, -mvPosition.z);
  vAlpha = aAlpha;
  vColor = aColor;
  #include <fog_vertex>
}`;
const pFrag = /* glsl */`
uniform sampler2D uTex;
varying float vAlpha;
varying vec3 vColor;
#include <fog_pars_fragment>
void main() {
  vec4 t = texture2D(uTex, gl_PointCoord);
  gl_FragColor = vec4(vColor * t.rgb, t.a * vAlpha);
  #include <fog_fragment>
}`;

class Particles {
  constructor(max, texture, additive) {
    this.max = max;
    this.pos = new Float32Array(max * 3);
    this.vel = new Float32Array(max * 3);
    this.col = new Float32Array(max * 3);
    this.size = new Float32Array(max);
    this.alpha = new Float32Array(max);
    this.life = new Float32Array(max);
    this.age = new Float32Array(max);
    this.grow = new Float32Array(max);
    this.a0 = new Float32Array(max);
    this.drag = new Float32Array(max);
    this.grav = new Float32Array(max);
    this.next = 0;
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aColor', new THREE.BufferAttribute(this.col, 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aSize', new THREE.BufferAttribute(this.size, 1).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('aAlpha', new THREE.BufferAttribute(this.alpha, 1).setUsage(THREE.DynamicDrawUsage));
    this.uniforms = THREE.UniformsUtils.merge([THREE.UniformsLib.fog, { uScale: { value: 400 }, uTex: { value: texture } }]);
    this.uniforms.uTex.value = texture;
    const m = new THREE.ShaderMaterial({
      vertexShader: pVert, fragmentShader: pFrag, uniforms: this.uniforms,
      transparent: true, depthWrite: false, fog: true,
      blending: additive ? THREE.AdditiveBlending : THREE.NormalBlending,
    });
    this.points = new THREE.Points(g, m);
    this.points.frustumCulled = false;
    this.geo = g;
  }

  emit(x, y, z, vx, vy, vz, life, size, grow, r, g, b, alpha, drag = 1, grav = 0) {
    const i = this.next;
    this.next = (this.next + 1) % this.max;
    this.pos[i * 3] = x; this.pos[i * 3 + 1] = y; this.pos[i * 3 + 2] = z;
    this.vel[i * 3] = vx; this.vel[i * 3 + 1] = vy; this.vel[i * 3 + 2] = vz;
    this.col[i * 3] = r; this.col[i * 3 + 1] = g; this.col[i * 3 + 2] = b;
    this.size[i] = size; this.grow[i] = grow; this.life[i] = life; this.age[i] = 0;
    this.a0[i] = alpha; this.alpha[i] = alpha; this.drag[i] = drag; this.grav[i] = grav;
  }

  update(dt) {
    for (let i = 0; i < this.max; i++) {
      if (this.age[i] >= this.life[i]) { this.alpha[i] = 0; continue; }
      this.age[i] += dt;
      const k = Math.exp(-this.drag[i] * dt);
      this.vel[i * 3] *= k; this.vel[i * 3 + 1] = this.vel[i * 3 + 1] * k - this.grav[i] * dt; this.vel[i * 3 + 2] *= k;
      this.pos[i * 3] += this.vel[i * 3] * dt;
      this.pos[i * 3 + 1] += this.vel[i * 3 + 1] * dt;
      this.pos[i * 3 + 2] += this.vel[i * 3 + 2] * dt;
      this.size[i] += this.grow[i] * dt;
      const f = this.age[i] / this.life[i];
      this.alpha[i] = this.a0[i] * (f < 0.1 ? f / 0.1 : 1 - (f - 0.1) / 0.9);
    }
    for (const a of ['position', 'aColor', 'aSize', 'aAlpha']) this.geo.attributes[a].needsUpdate = true;
  }
}

class SkidMarks {
  constructor(scene, max = 2400) {
    this.max = max;
    this.pos = new Float32Array(max * 4 * 3);
    this.alpha = new Float32Array(max * 4);
    const idx = new Uint32Array(max * 6);
    for (let i = 0; i < max; i++) {
      const b = i * 4;
      idx.set([b, b + 1, b + 2, b + 1, b + 3, b + 2], i * 6);
    }
    const g = new THREE.BufferGeometry();
    g.setAttribute('position', new THREE.BufferAttribute(this.pos, 3).setUsage(THREE.DynamicDrawUsage));
    g.setAttribute('alpha', new THREE.BufferAttribute(this.alpha, 1).setUsage(THREE.DynamicDrawUsage));
    g.setIndex(new THREE.BufferAttribute(idx, 1));
    const m = new THREE.ShaderMaterial({
      transparent: true, depthWrite: false, polygonOffset: true, polygonOffsetFactor: -4, polygonOffsetUnits: -4,
      vertexShader: 'attribute float alpha; varying float vA; void main(){ vA = alpha; gl_Position = projectionMatrix * modelViewMatrix * vec4(position,1.0);} ',
      fragmentShader: 'varying float vA; void main(){ gl_FragColor = vec4(0.02,0.02,0.02, vA); }',
    });
    this.mesh = new THREE.Mesh(g, m);
    this.mesh.frustumCulled = false;
    this.geo = g;
    this.next = 0;
    this.dirty = false;
    scene.add(this.mesh);
  }

  add(a, b, width, alpha) {
    // a, b: {x,y,z}; build a quad across the direction of travel.
    const dx = b.x - a.x, dz = b.z - a.z;
    const l = Math.hypot(dx, dz);
    if (l < 0.05 || l > 4) return;
    const nx = -dz / l * width * 0.5, nz = dx / l * width * 0.5;
    const i = this.next;
    this.next = (this.next + 1) % this.max;
    const p = this.pos, o = i * 12;
    p[o] = a.x + nx; p[o + 1] = a.y + 0.03; p[o + 2] = a.z + nz;
    p[o + 3] = a.x - nx; p[o + 4] = a.y + 0.03; p[o + 5] = a.z - nz;
    p[o + 6] = b.x + nx; p[o + 7] = b.y + 0.03; p[o + 8] = b.z + nz;
    p[o + 9] = b.x - nx; p[o + 10] = b.y + 0.03; p[o + 11] = b.z - nz;
    this.alpha.fill(alpha, i * 4, i * 4 + 4);
    this.dirty = true;
  }

  flush() {
    if (!this.dirty) return;
    this.geo.attributes.position.needsUpdate = true;
    this.geo.attributes.alpha.needsUpdate = true;
    this.dirty = false;
  }
}

export class Effects {
  constructor(scene, renderer, camera) {
    this.scene = scene;
    this.renderer = renderer;
    this.camera = camera;
    this.smoke = new Particles(700, smokeTexture(), false);
    this.sparks = new Particles(500, glowTexture(), true);
    scene.add(this.smoke.points, this.sparks.points);
    this.skids = new SkidMarks(scene);
    this.cars = [];
    this.flameMat = new THREE.MeshBasicMaterial({ color: 0x66aaff, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false });
    this.flameCore = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.9, blending: THREE.AdditiveBlending, depthWrite: false });
    this.flameGeo = new THREE.ConeGeometry(0.13, 1, 10, 1, true);
    this.flameGeo.rotateX(-Math.PI / 2); // point down −Z (backwards)
    this.flameGeo.translate(0, 0, -0.5);
    const poolTex = glowTexture();
    this.poolGeo = new THREE.PlaneGeometry(1, 1);
    this.poolGeo.rotateX(-Math.PI / 2);
    this.poolMat = new THREE.MeshBasicMaterial({ map: poolTex, color: 0xfff1d0, transparent: true, opacity: 0.0, blending: THREE.AdditiveBlending, depthWrite: false, polygonOffset: true, polygonOffsetFactor: -6, polygonOffsetUnits: -6 });
    this.tailPoolMat = new THREE.MeshBasicMaterial({ map: poolTex, color: 0xff2020, transparent: true, opacity: 0.0, blending: THREE.AdditiveBlending, depthWrite: false, polygonOffset: true, polygonOffsetFactor: -6, polygonOffsetUnits: -6 });
    this.tmpA = new THREE.Vector3();
  }

  resize(heightPx, fovDeg) {
    const s = heightPx / (2 * Math.tan((fovDeg * Math.PI) / 360));
    this.smoke.uniforms.uScale.value = s;
    this.sparks.uniforms.uScale.value = s;
  }

  // Register a car for flames, wheel tracking and headlight pools.
  addCar(vehicle, { player = false } = {}) {
    const m = vehicle.model;
    const entry = { v: vehicle, player, flames: [], lastL: null, lastR: null };
    for (const e of m.exhausts || []) {
      const outer = new THREE.Mesh(this.flameGeo, this.flameMat);
      const core = new THREE.Mesh(this.flameGeo, this.flameCore);
      core.scale.set(0.45, 0.45, 0.6);
      outer.add(core);
      outer.position.copy(e);
      outer.visible = false;
      m.body.add(outer);
      entry.flames.push(outer);
    }
    const pool = new THREE.Mesh(this.poolGeo, this.poolMat);
    pool.scale.set(player ? 9 : 7, 1, player ? 16 : 12);
    pool.frustumCulled = false;
    this.scene.add(pool);
    entry.pool = pool;
    this.cars.push(entry);
    return entry;
  }

  removeCar(vehicle) {
    const i = this.cars.findIndex((c) => c.v === vehicle);
    if (i >= 0) { this.scene.remove(this.cars[i].pool); this.cars.splice(i, 1); }
  }

  wheelWorld(v, side, out) {
    // Rear wheel contact point in world space.
    const d = v.model.dims;
    const fx = Math.cos(v.yaw), fz = Math.sin(v.yaw);
    const rx = -fz, rz = fx;
    const back = -(d.wheelBase || 2.6) / 2, across = (d.track || 1.6) / 2 * side;
    out.x = v.x + fx * back + rx * across;
    out.z = v.z + fz * back + rz * across;
    out.y = v.y;
    return out;
  }

  sparksAt(x, y, z, n, vx = 0, vz = 0) {
    for (let i = 0; i < n; i++) {
      this.sparks.emit(x, y, z,
        vx * 0.6 + (Math.random() - 0.5) * 9, 2 + Math.random() * 5, vz * 0.6 + (Math.random() - 0.5) * 9,
        0.3 + Math.random() * 0.5, 0.18 + Math.random() * 0.12, -0.2, 1, 0.65 + Math.random() * 0.3, 0.25, 1, 1.5, 14);
    }
  }

  smokeAt(x, y, z, amount, vx = 0, vz = 0, night = 0) {
    const shade = lerp(0.85, 0.35, night);
    this.smoke.emit(x, y + 0.3, z, vx * 0.3 + (Math.random() - 0.5) * 1.5, 0.6 + Math.random(), vz * 0.3 + (Math.random() - 0.5) * 1.5,
      1.2 + Math.random() * 1.0, 0.8 + amount * 0.6, 2.4, shade, shade, shade, 0.07 + amount * 0.13, 1.2, -0.3);
  }

  update(dt, night, extras = new Map()) {
    const w = { x: 0, y: 0, z: 0 };
    for (const c of this.cars) {
      const v = c.v;
      const ex = extras.get(v) || {};
      const visible = v.model.root.visible;
      // Nitro flames.
      const nit = ex.nitro && visible;
      for (const f of c.flames) {
        f.visible = nit;
        if (nit) {
          const fl = 0.7 + Math.random() * 0.6;
          f.scale.set(1, 1, fl * (c.player ? 1.3 : 1));
        }
      }
      // Headlight pool on the road ahead at night.
      c.pool.visible = visible && night > 0.2;
      if (c.pool.visible) {
        const fx = Math.cos(v.yaw + (v.visualYaw || 0)), fz = Math.sin(v.yaw + (v.visualYaw || 0));
        const ahead = c.player ? 10 : 7.5;
        c.pool.position.set(v.x + fx * ahead, v.y + 0.06, v.z + fz * ahead);
        c.pool.rotation.set(0, -Math.atan2(fz, fx) + Math.PI / 2, 0);
        c.pool.material.opacity = 0.35 * night;
      }
      // Skids and smoke from the rear wheels.
      const skid = ex.skid || 0;
      if (skid > 0.3 && v.onGround && visible) {
        const L = this.wheelWorld(v, -1, { x: 0, y: 0, z: 0 });
        const R = this.wheelWorld(v, 1, { x: 0, y: 0, z: 0 });
        if (c.lastL) this.skids.add(c.lastL, L, 0.26, clamp(skid * 0.6, 0, 0.55));
        if (c.lastR) this.skids.add(c.lastR, R, 0.26, clamp(skid * 0.6, 0, 0.55));
        c.lastL = L; c.lastR = R;
        if (Math.random() < skid * 0.55) {
          const src = Math.random() < 0.5 ? L : R;
          this.smokeAt(src.x, src.y, src.z, skid, v.vx, v.vz, night);
        }
      } else { c.lastL = c.lastR = null; }
      // Exhaust smoke puffs at launch.
      if (ex.launch && visible) {
        this.wheelWorld(v, 0, w);
        this.smokeAt(w.x, w.y, w.z, 0.2, 0, 0, night);
      }
    }
    this.smoke.update(dt);
    this.sparks.update(dt);
    this.skids.flush();
    this.poolMat.opacity = 0.3 * night;
  }
}
