//! The atlas: real geography for planning the open world
//! (`docs/vision/atlas.md`, vision ROADMAP M13).
//!
//! Three files under `assets/atlas/<world>/` make a world's atlas:
//!
//! - `terrain.bin`, a grid of heights and land cover over the whole box
//!   (written by `tools/atlas/build.py` from real elevation and land cover);
//! - `geo.json`, the projection, the road graph between junctions (with
//!   each road's class, name and route number), railways, parks and towns
//!   (the same script, from Overture Maps / OpenStreetMap);
//! - `plan.json`, written by hand by the owner and Claude: the regions,
//!   places, routes along real roads, events, and how the existing levels
//!   are remixed into the world.
//!
//! This crate reads them, resolves each route along the real roads
//! ([`graph`]), measures it over the real terrain ([`profile`]: length,
//! climb, crests, what it passes through), and writes what the planner page
//! (`tools/atlas.html`) draws. It is the place later corridor generation
//! reads real ground from.
//!
//! The local frame is the levels': x east, z south, metres from the world's
//! origin. Everything is a plain function of the files: no clock, no hash
//! maps, no platform maths (square roots only), so a plan measures the same
//! everywhere.

#![forbid(unsafe_code)]

pub mod geo;
pub mod graph;
pub mod plan;
pub mod profile;
pub mod report;
pub mod terrain;

pub use geo::{Geo, LatLon, Projection, V2};
pub use plan::Plan;
pub use terrain::{Cover, Terrain};

use std::path::{Path, PathBuf};

/// A world's atlas: its terrain, its geography and its plan.
pub struct Atlas {
    pub terrain: Terrain,
    pub geo: Geo,
    pub plan: Plan,
}

impl Atlas {
    /// Loads `terrain.bin`, `geo.json` and `plan.json` from a directory.
    pub fn load(dir: &Path) -> Result<Atlas, String> {
        let read = |n: &str| {
            let p = dir.join(n);
            std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
        };
        let terrain = Terrain::from_bytes(&read("terrain.bin")?)?;
        let geo = Geo::from_json(&read("geo.json")?)?;
        let plan = Plan::from_json(&read("plan.json")?)?;
        Ok(Atlas { terrain, geo, plan })
    }

    /// The repository's atlas for a world (`assets/atlas/<world>/`).
    pub fn dir(world: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/atlas")
            .join(world)
    }

    /// A plan place in the local frame.
    pub fn place(&self, id: &str) -> Result<V2, String> {
        let p = self
            .plan
            .places
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("no place `{id}` in the plan"))?;
        Ok(self.geo.projection.to_local(p.at))
    }
}
