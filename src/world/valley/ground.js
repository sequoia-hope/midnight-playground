import { TERRAIN_TILE } from '../Terrain.js';

// Height of the terrain *as rendered*. Tiles away from the road are meshed
// at 16–32 m, so the true height function can be metres off the triangles
// the player actually sees; things planted on the ground use this instead.
export function makeGround(terrain) {
  const tiles = terrain.tileList();
  const grid = new Map(tiles.map((t) => [t.i + ',' + t.j, t]));
  const cache = new Map();
  const H = (x, z) => {
    const k = x + ',' + z;
    let h = cache.get(k);
    if (h === undefined) {
      h = terrain.heightAt(x, z);
      if (cache.size > 200000) cache.clear();
      cache.set(k, h);
    }
    return h;
  };
  function height(x, z) {
    const i = Math.floor((x - terrain.minX) / TERRAIN_TILE);
    const j = Math.floor((z - terrain.minZ) / TERRAIN_TILE);
    const tile = grid.get(i + ',' + j);
    if (!tile) return terrain.heightAt(x, z);
    const st = tile.step;
    const ci = Math.floor((x - tile.x0) / st), cj = Math.floor((z - tile.z0) / st);
    const x0 = tile.x0 + ci * st, z0 = tile.z0 + cj * st;
    const fx = (x - x0) / st, fz = (z - z0) / st;
    const a = H(x0, z0), b = H(x0 + st, z0), c = H(x0, z0 + st), d = H(x0 + st, z0 + st);
    if ((ci + cj) & 1) {
      if (fx + fz <= 1) return a + fx * (b - a) + fz * (c - a);
      return d + (1 - fx) * (c - d) + (1 - fz) * (b - d);
    }
    if (fz >= fx) return a + fx * (d - c) + fz * (c - a);
    return a + fx * (b - a) + fz * (d - b);
  }
  function stepAt(x, z) {
    const i = Math.floor((x - terrain.minX) / TERRAIN_TILE);
    const j = Math.floor((z - terrain.minZ) / TERRAIN_TILE);
    return grid.get(i + ',' + j)?.step ?? 32;
  }
  return { height, stepAt, tiles };
}
