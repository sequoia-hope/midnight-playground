//! A menu section of a level (D741): the level's build restricted to a
//! stretch. Seaside Raceway's terrain, road and sky (the cheapest build),
//! whole and as a section.

mod common;

use mr_scene::Scene;
use mr_track::Track;
use mr_worldgen::section::{self, Section};
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::world::{Build, level_jobs};

fn build(sec: Option<Section>) -> Scene {
    let mut w = common::world("seaside");
    w.section = sec;
    let setup = LevelSetup {
        terrain: common::terrain_setup("seaside"),
        road: None,
    };
    let mut b = Build::new(w, level_jobs(level_stages(setup), |_| None));
    while !b.is_done() {
        b.step().expect("builds");
    }
    section::finish(b).0.scene
}

fn vertices(s: &Scene) -> usize {
    s.meshes
        .iter()
        .filter_map(|m| m.attribute("position"))
        .map(|a| s.buffers[a.accessor as usize].count())
        .sum()
}

#[test]
fn a_section_is_the_level_near_its_stretch() {
    let track = Track::new(&common::level("seaside")).expect("track");
    let sec = Section {
        s0: track.start_s + 20.0,
        s1: track.start_s + 660.0,
        terrain_radius: 1000.0,
        scenery_radius: 300.0,
        scenery: true,
    };
    let whole = build(None);
    let part = build(Some(sec.clone()));
    // Less of everything, the same materials' kinds.
    assert!(
        vertices(&part) < vertices(&whole) * 3 / 4,
        "{} of {}",
        vertices(&part),
        vertices(&whole)
    );
    assert!(part.nodes.len() <= whole.nodes.len());
    for m in &part.materials {
        assert!(whole.materials.iter().any(|w| w.kind == m.kind));
    }
    // Every drawable left reaches the stretch: its origin's bounds (or its
    // vertices') within the radius, measured as the cut does it, coarsely:
    // some vertex within radius + 1 km.
    let pts = sec.points(&track);
    for n in part.nodes.iter().filter(|n| n.mesh.is_some()) {
        let m = &part.meshes[n.mesh.unwrap() as usize];
        let p = &part.buffers[m.attribute("position").unwrap().accessor as usize];
        let w = n.matrix_world;
        let near = (0..p.count()).step_by(7).any(|i| {
            let (x, y, z) = (
                p.data.get(i * 3),
                p.data.get(i * 3 + 1),
                p.data.get(i * 3 + 2),
            );
            let wx = w[0] * x + w[4] * y + w[8] * z + w[12];
            let wz = w[2] * x + w[6] * y + w[10] * z + w[14];
            pts.iter()
                .any(|q| (q[0] - wx).hypot_free(q[1] - wz) < sec.terrain_radius + 1000.0)
        });
        assert!(near, "{} is far from the stretch", n.name);
    }
}

trait HypotFree {
    fn hypot_free(self, o: f64) -> f64;
}

impl HypotFree for f64 {
    /// The distance without `hypot` (not exact everywhere, SPEC 4.2).
    fn hypot_free(self, o: f64) -> f64 {
        (self * self + o * o).sqrt()
    }
}
