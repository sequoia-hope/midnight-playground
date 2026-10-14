//! Streaming the open world (docs/vision/peninsula.md, stage 4b; D1213):
//! the loop planned once, then built in sections that partition it, the
//! base the horizon under them.

mod common;

use std::sync::Arc;

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
    // No clock in wasm under Node (std's Instant panics there).
    #[cfg(not(target_arch = "wasm32"))]
    let t0 = std::time::Instant::now();
    #[cfg(not(target_arch = "wasm32"))]
    let secs = || t0.elapsed().as_secs_f64();
    #[cfg(target_arch = "wasm32")]
    let secs = || 0.0;
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
        secs(),
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

/// A section packed and read back (the worker's hand-over, D1215) is the
/// same scene, and its animators are data that write the same edits.
#[test]
fn a_packed_section_reads_back_with_its_animators() {
    use mp_worldgen::world::UpdateCtx;
    let p = planned();
    // The coast's first section: the surf, the lighthouse's and the
    // boats' animators.
    let k = p.section_at(p.track.start_s);
    let mut b = p.build(p.section(k));
    while !b.is_done() {
        b.step().expect("a section's job");
    }
    let (mut wb, stats) = section::finish(b);
    assert!(!wb.animators.is_empty(), "the coast animates");
    assert_eq!(wb.animators.len(), wb.descs.len(), "every animator is data");
    let bytes = section::pack(&wb, stats).expect("packs");
    let (mut back, stats2) = section::unpack(&bytes).expect("reads back");
    assert_eq!((stats.kept, stats.cut), (stats2.kept, stats2.cut));
    assert_eq!(back.scene, wb.scene);
    assert_eq!(back.handles, wb.handles);
    assert_eq!(back.animators.len(), wb.animators.len());
    assert!(
        !back.log.iter().any(|l| l.contains("lost")),
        "{:?}",
        back.log
    );
    let u = UpdateCtx {
        dt: 1.0 / 60.0,
        night: 0.7,
        camera: None,
        s: 100.0,
    };
    for _ in 0..3 {
        let (mut a, mut c) = (Vec::new(), Vec::new());
        for x in wb.animators.iter_mut() {
            x.update(&u, &mut a);
        }
        for x in back.animators.iter_mut() {
            x.update(&u, &mut c);
        }
        assert_eq!(a.len(), c.len());
        for (e, f) in a.iter().zip(&c) {
            assert_eq!(format!("{e:?}"), format!("{f:?}"));
        }
    }
}

/// Stage 4d: the branches are sections of their own road, the ring
/// crosses a junction, each road's end is closed unless it is open onto
/// another road, and the crop land is fenced.
#[test]
fn branches_stream_as_their_own_sections_and_the_road_ends_are_closed() {
    let p = planned();
    let t = p.track.clone();
    let rs = p.road_sections();
    assert_eq!(rs.len(), 3, "the loop and its two branches");
    let (tf, tn) = rs[1];
    let (sf, sn) = rs[2];
    assert!(tn >= 7 && sn >= 5, "{rs:?}");
    assert_eq!(p.sections(), rs[0].1 + tn + sn);
    assert_eq!(p.road_of(tf + 2), (1, 2));
    assert_eq!(p.section(tf + 2).road, 1);

    // The ring at Tunitas's mouth holds the branch's first two sections;
    // on the branch's first section it holds the parent's round the
    // mouth; at its far end, Skyline's round where it meets it.
    let tun = &t.branches[0];
    let mouth = p.section_of(0, tun.s);
    let r = p.ring(mouth, 1, 2);
    assert!(r.contains(&tf) && r.contains(&(tf + 1)), "{r:?}");
    let r = p.ring(tf, 1, 2);
    assert!(r.contains(&mouth) && r.contains(&(tf + 1)), "{r:?}");
    let far = p.section_of(0, tun.end.expect("meets Skyline"));
    let r = p.ring(tf + tn - 1, 1, 2);
    assert!(r.contains(&far), "{r:?}");
    // A place on the branch is the branch's section.
    let f = tun.track.frame(2500.0);
    assert_eq!(p.section_under(f.x, f.z), Some(tf + 2));

    // The branch's section builds its own road and nothing of the loop's,
    // and the mouth's two sections share nothing.
    let b2 = built(&p, p.section(tf + 2));
    let ds = section::drawables(&b2);
    let sec = p.section(tf + 2);
    assert!(
        ds.iter().filter(|(n, _)| n == "road").count() >= 2,
        "the branch's road"
    );
    for (name, c) in &ds {
        assert!(
            sec.owns(&t, c[0], c[2]),
            "{name} at ({:.0}, {:.0})",
            c[0],
            c[2]
        );
    }
    let m = built(&p, p.section(mouth));
    let b0 = built(&p, p.section(tf));
    let key = |c: &[f64; 3]| [c[0].round() as i64, c[2].round() as i64];
    let mk: Vec<(String, [i64; 2])> = section::drawables(&m)
        .into_iter()
        .map(|(n, c)| (n, key(&c)))
        .collect();
    for (n, c) in section::drawables(&b0) {
        assert!(
            !mk.contains(&(n.clone(), key(&c))),
            "{n} at {:?} is in both",
            key(&c)
        );
    }
    assert!(
        section::drawables(&b0).iter().any(|(n, _)| n == "road"),
        "the branch's first section has its mouth"
    );

    // Stage Road ends at Pescadero: its last section bars the road.
    // Tunitas meets Skyline: no bar.
    let last = built(&p, p.section(sf + sn - 1));
    assert!(
        section::drawables(&last).iter().any(|(n, _)| n == "closed"),
        "Stage Road's end is closed"
    );
    let tl = built(&p, p.section(tf + tn - 1));
    assert!(
        !section::drawables(&tl).iter().any(|(n, _)| n == "closed"),
        "Tunitas runs onto Skyline"
    );

    // Crop land by a road is fenced: the first section whose verge is crop.
    let cover = p.level.cover.as_ref().expect("cover");
    let mut crop_s = None;
    let mut s = 0.0;
    while s < t.length && crop_s.is_none() {
        let f = t.frame(s);
        let off = f.hw + f.margin + 2.2;
        for side in [-1.0, 1.0] {
            if mp_track::Cover::of(cover(f.x + f.rx * off * side, f.z + f.rz * off * side))
                == mp_track::Cover::Crop
            {
                crop_s = Some(s);
            }
        }
        s += 50.0;
    }
    let crop_s = crop_s.expect("crop beside the loop");
    let c = built(&p, p.section(p.section_of(0, crop_s)));
    let posts = c
        .graph
        .log
        .iter()
        .find_map(|l| {
            l.strip_prefix("Land: ")
                .and_then(|l| l.split(", ").find(|w| w.ends_with("fence posts")))
                .and_then(|w| w.split(' ').next())
                .and_then(|n| n.parse::<usize>().ok())
        })
        .expect("the land's log");
    assert!(posts > 10, "{posts} posts at s {crop_s:.0}");
}
