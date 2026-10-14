//! Streaming the open world (docs/vision/peninsula.md, stage 4b; D1213):
//! the loop planned once, then built in sections that partition it, the
//! base the horizon under them.

mod common;

use std::sync::Arc;
use std::time::Instant;

use mp_worldgen::section::{self, Section};
use mp_worldgen::stages::LevelSetup;
use mp_worldgen::stream::{Planned, SECTION_LEN};
use mp_worldgen::terrain_mesh::TerrainSetup;
use mp_worldgen::world::World;

fn planned() -> Planned {
    let setup = LevelSetup {
        terrain: TerrainSetup {
            plan: None,
            ..TerrainSetup::default()
        },
        road: None,
    };
    Planned::new(
        common::level("peninsula"),
        None,
        setup,
        Arc::new(mp_worldgen::scenery::ported),
        Arc::new(|_| Vec::new()),
    )
    .expect("the loop plans")
}

/// Runs a section's build and returns its world, cut.
fn built(p: &Planned, sec: Section) -> World {
    let t0 = Instant::now();
    let mut b = p.build(sec.clone());
    while !b.is_done() {
        b.step().expect("a section's job");
    }
    let mut w = b.world;
    let stats = section::cut(&mut w);
    eprintln!(
        "section {:.0}..{:.0}{}: {:.2} s, kept {} cut {}",
        sec.s0,
        sec.s1,
        if sec.base { " (base)" } else { "" },
        t0.elapsed().as_secs_f64(),
        stats.kept,
        stats.cut
    );
    w
}

#[test]
fn sections_partition_the_loop_and_the_base_is_the_horizon() {
    let p = planned();
    let t = p.track.clone();
    assert!(p.sections() >= 34);
    let k0 = p.section_at(t.start_s);
    // Three sections round the start, and the base.
    let secs: Vec<Section> = (k0 - 1..=k0 + 1).map(|k| p.section(k)).collect();
    let worlds: Vec<World> = secs.iter().map(|s| built(&p, s.clone())).collect();
    let base = built(&p, p.base_section());

    // Everything a section keeps belongs to its stretch, and nothing is
    // in two sections.
    let mut seen: Vec<(usize, String, [i64; 3])> = Vec::new();
    for (si, (sec, w)) in secs.iter().zip(&worlds).enumerate() {
        let ds = section::drawables(w);
        assert!(ds.len() > 20, "section {}: {} drawables", sec.s0, ds.len());
        for (name, c) in &ds {
            assert!(
                sec.owns(&t, c[0], c[2]),
                "section {}: {name} at ({:.0}, {:.0}) is not its own",
                sec.s0,
                c[0],
                c[2]
            );
            let key = [
                c[0].round() as i64,
                c[1].round() as i64,
                c[2].round() as i64,
            ];
            assert!(
                !seen
                    .iter()
                    .any(|(j, n, k)| *j != si && n == name && *k == key),
                "'{name}' at {key:?} (s {:?}) is in two sections (this one {}..{})",
                Section::s_of(&t, c[0], c[2]),
                sec.s0,
                sec.s1
            );
            seen.push((si, name.clone(), key));
        }
        // Each has its stretch of road: a surface every chunk.
        assert!(w.road.is_some(), "section {}: no road", sec.s0);
        let roads = ds.iter().filter(|(n, _)| n == "road").count();
        assert!(roads >= 2, "section {}: {roads} road drawables", sec.s0);
    }
    // The base: coarse tiles, the sky and the sea; no road, no scenery.
    assert!(base.road.is_none());
    assert!(base.sky.is_some() && base.sea.is_some());
    let ds = section::drawables(&base);
    assert!(ds.len() > 100, "base: {} drawables", ds.len());
    assert!(
        !ds.iter()
            .any(|(n, _)| n == "road" || n == "land" || n == "coast"),
        "the base draws no road or scenery"
    );

    // Along every stretch the near tiles are there (a section's tiles
    // are 256 m, merged a few to a mesh; one is within 400 m of every
    // point of the road), and the base has the coarse ones.
    let tiles = |w: &World| -> Vec<[f64; 2]> {
        section::drawables(w)
            .iter()
            .filter(|(n, _)| n == "terrain")
            .map(|(_, c)| [c[0], c[2]])
            .collect()
    };
    assert!(
        tiles(&base).len() > 50,
        "the base has {} tile meshes",
        tiles(&base).len()
    );
    for sec in &secs {
        let near: Vec<[f64; 2]> = worlds.iter().flat_map(tiles).collect();
        let mut s = sec.s0 + 100.0;
        while s < sec.s1 {
            let f = t.frame(t.wrap(s));
            let d = near
                .iter()
                .map(|c| ((c[0] - f.x).powi(2) + (c[1] - f.z).powi(2)).sqrt())
                .fold(f64::INFINITY, f64::min);
            assert!(d < 400.0, "no near tile within {d:.0} m of s {s:.0}");
            s += 200.0;
        }
    }
    let _ = SECTION_LEN;
}
