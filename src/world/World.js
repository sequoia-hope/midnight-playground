import * as THREE from 'three';
import { smoothstep } from '../util/math.js';
import { Track } from '../track/Track.js';
import { Terrain } from './Terrain.js';
import { buildTerrainMeshes } from './TerrainMesh.js';
import { Road } from './Road.js';
import { Sky } from './Sky.js';
import { Sea } from './Sea.js';

// Builds one level's world in stages, reporting progress so the loading
// screen can show something while terrain tiles are generated. Everything
// it creates hangs off `root` (plus the sky's lights and dome), so switching
// levels can tear it all down and free the GPU memory.

export class World {
  constructor(scene, renderer, level) {
    this.realScene = scene;
    this.renderer = renderer;
    this.level = level;
    this.root = new THREE.Group();
    this.root.name = 'world:' + level.id;
    scene.add(this.root);
    // Scenery modules add their groups to world.scene.
    this.scene = this.root;
    this.updaters = [];   // per-frame hooks from scenery (windmills, blinking lights)
    this.nightMaterials = []; // {material, prop, day, night} scaled by night factor
  }

  async build(progress = () => {}) {
    const step = async (label, frac) => { progress(label, frac); await new Promise((r) => setTimeout(r, 0)); };
    await step('Surveying the route', 0.02);
    this.track = new Track(this.level);

    await step('Shaping the land', 0.06);
    this.terrain = new Terrain(this.track, this.level);
    this.scenery = await this.loadScenery();
    // Scenery may carve creeks or flatten building pads before heights are
    // final. A broken scenery module logs and is skipped rather than
    // stopping the level from loading.
    this.scenery = this.scenery.filter((s) => {
      try { s.plan?.(this); return true; } catch (e) { console.error(`${s.constructor.name}.plan failed`, e); return false; }
    });
    this.terrain.buildFields();
    this.terrain.resolveFlattens();

    await step('Sculpting terrain', 0.1);
    const { group: terrainGroup, material: terrainMat } = await buildTerrainMeshes(this.terrain, (f) => progress('Sculpting terrain', 0.1 + f * 0.55));
    this.terrainMaterial = terrainMat;
    this.root.add(terrainGroup);

    await step('Paving roads', 0.67);
    this.road = new Road(this.track, this.terrain);
    this.root.add(this.road.build());
    this.addNight(this.road.materials.markings, 'emissiveIntensity', 0.0, 0.06);
    this.addNight(this.road.materials.chevron, 'emissiveIntensity', 0.12, 0.9);
    // Dew on the tarmac after dark: lamps and headlights glint off it.
    this.updaters.push((dt, n) => this.road.setNight(smoothstep(0.55, 1.0, n) * 0.85));

    this.sky = new Sky(this.realScene, this.renderer, this.track, this.level.sky, this.level.sunAzimuth);
    if (this.level.sea) {
      await step('Filling the sea', 0.7);
      this.sea = new Sea(this, this.level.sea.y);
    }

    let k = 0;
    for (const s of this.scenery) {
      await step(s.label || 'Building scenery', 0.72 + (k++ / this.scenery.length) * 0.26);
      try { await s.build(this); } catch (e) { console.error(`${s.constructor.name}.build failed`, e); }
    }
    await step('Ready', 1);
    return this;
  }

  async loadScenery() {
    const out = [];
    const zones = this.level.zones;
    const names = [...new Set(zones.map((z) => z.scenery).filter(Boolean))];
    for (const name of names) {
      try {
        const mod = await import(`./${name}.js`);
        const zone = zones.findIndex((z) => z.scenery === name);
        out.push(new mod.default({ zone, key: zones[zone].key, level: this.level }));
      } catch (e) {
        console.warn(`${name} scenery missing`, e);
      }
    }
    return out;
  }

  zoneIndex(key) { return this.level.zones.findIndex((z) => z.key === key); }

  // Register a material property that should follow nightfall.
  addNight(material, prop, day, night) {
    if (material) this.nightMaterials.push({ material, prop, day, night });
  }

  update(dt, s, focus, camera) {
    const k = this.sky.update(dt, s, focus);
    const n = this.sky.night;
    for (const m of this.nightMaterials) m.material[m.prop] = m.day + (m.night - m.day) * n;
    for (const u of this.updaters) u(dt, n, camera, s);
    return k;
  }

  // Remove everything this world added and free GPU resources.
  dispose() {
    const seen = new Set();
    const free = (o) => {
      if (o.geometry && !seen.has(o.geometry)) { seen.add(o.geometry); o.geometry.dispose(); }
      const mats = Array.isArray(o.material) ? o.material : o.material ? [o.material] : [];
      for (const m of mats) {
        if (seen.has(m)) continue;
        seen.add(m);
        for (const v of Object.values(m)) if (v && v.isTexture && !seen.has(v)) { seen.add(v); v.dispose(); }
        m.dispose();
      }
      if (o.isInstancedMesh) o.dispose();
    };
    this.root.traverse(free);
    this.realScene.remove(this.root);
    if (this.sky) {
      this.sky.dome.traverse(free);
      this.realScene.remove(this.sky.dome, this.sky.sun, this.sky.sun.target, this.sky.hemi);
      this.sky.sun.dispose?.();
      this.sky.sun.shadow?.map?.dispose();
    }
    this.updaters.length = 0;
    this.nightMaterials.length = 0;
  }
}
