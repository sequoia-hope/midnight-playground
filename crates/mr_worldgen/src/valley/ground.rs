//! Port of `src/world/valley/ground.js`: the height of the terrain *as
//! rendered* (roadmap WP 3.7).
//!
//! Tiles away from the road are meshed at 16–32 m, so the true height
//! function can be metres off the triangles the player actually sees;
//! things planted on the ground use this instead.
//!
//! The JS memoises `terrain.heightAt` at the lattice points in a `Map`
//! (cleared past 200,000 entries); the heights are a pure function, so the
//! port reads them directly and the values are the same.

use mr_math::js;

use crate::terrain::{TERRAIN_TILE, Terrain, Tile};

/// `makeGround(terrain)`: `{ height, stepAt, tiles }`.
pub struct Ground<'a> {
    terrain: &'a Terrain,
    pub tiles: Vec<Tile>,
    /// The tile grid (`tileList` is complete: `nx × nz` in order).
    nx: i64,
    nz: i64,
}

impl<'a> Ground<'a> {
    pub fn new(terrain: &'a Terrain) -> Ground<'a> {
        let tiles = terrain.tile_list();
        let nx = tiles.iter().map(|t| t.i + 1).max().unwrap_or(0);
        let nz = tiles.iter().map(|t| t.j + 1).max().unwrap_or(0);
        Ground {
            terrain,
            tiles,
            nx,
            nz,
        }
    }

    /// `grid.get(i + ',' + j)`.
    fn tile(&self, i: f64, j: f64) -> Option<&Tile> {
        if !(i >= 0.0 && j >= 0.0 && i < self.nx as f64 && j < self.nz as f64) {
            return None;
        }
        let t = &self.tiles[(j as i64 * self.nx + i as i64) as usize];
        debug_assert!(t.i == i as i64 && t.j == j as i64);
        Some(t)
    }

    fn h(&self, x: f64, z: f64) -> f64 {
        self.terrain.height_at(x, z)
    }

    /// `height(x, z)`: the triangle of the tile's mesh under (x, z).
    pub fn height(&self, x: f64, z: f64) -> f64 {
        let i = ((x - self.terrain.min_x) / TERRAIN_TILE).floor();
        let j = ((z - self.terrain.min_z) / TERRAIN_TILE).floor();
        let Some(tile) = self.tile(i, j) else {
            return self.terrain.height_at(x, z);
        };
        let st = tile.step;
        let ci = ((x - tile.x0) / st).floor();
        let cj = ((z - tile.z0) / st).floor();
        let x0 = tile.x0 + ci * st;
        let z0 = tile.z0 + cj * st;
        let fx = (x - x0) / st;
        let fz = (z - z0) / st;
        let a = self.h(x0, z0);
        let b = self.h(x0 + st, z0);
        let c = self.h(x0, z0 + st);
        let d = self.h(x0 + st, z0 + st);
        if js::to_int32(ci + cj) & 1 != 0 {
            if fx + fz <= 1.0 {
                return a + fx * (b - a) + fz * (c - a);
            }
            return d + (1.0 - fx) * (c - d) + (1.0 - fz) * (b - d);
        }
        if fz >= fx {
            return a + fx * (d - c) + fz * (c - a);
        }
        a + fx * (b - a) + fz * (d - b)
    }

    /// `stepAt(x, z)`: the mesh step there (32 off the tiles).
    pub fn step_at(&self, x: f64, z: f64) -> f64 {
        let i = ((x - self.terrain.min_x) / TERRAIN_TILE).floor();
        let j = ((z - self.terrain.min_z) / TERRAIN_TILE).floor();
        self.tile(i, j).map_or(32.0, |t| t.step)
    }
}
