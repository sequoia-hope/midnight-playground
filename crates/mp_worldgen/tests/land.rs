//! The Land kit plants the open world by its land cover
//! (docs/vision/peninsula.md, stage 4a; D1212): on Skyline, which the
//! atlas has as forest nearly the whole way, the verge and the hillsides
//! fill with conifers; nothing stands on the road, under the sea or past
//! the kit's reach; and a cell plants the same whatever else is built.

mod common;

use mp_worldgen::land::{CELL, Land, REACH};
use mp_worldgen::section::Section;
use mp_worldgen::world::{Scenery, SceneryInfo};

fn build(s0: f64, s1: f64) -> (mp_worldgen::world::World, Land) {
    let (level, t, tr) = common::terrain("peninsula", None);
    let mut w = common::world("peninsula");
    w.section = Some(Section {
        s0,
        s1,
        terrain_radius: 600.0,
        scenery_radius: 600.0,
        scenery: true,
        ..Section::default()
    });
    w.track = Some(t);
    w.terrain = Some(tr);
    let info = SceneryInfo {
        name: "Land",
        zone: level
            .zones
            .iter()
            .position(|z| z.scenery == "Land")
            .unwrap(),
        key: "skyline",
    };
    let mut land = Land::new(&info);
    land.build(&mut w).expect("the land builds");
    (w, land)
}

#[test]
fn skyline_is_planted_with_conifers_clear_of_the_road() {
    let (w, land) = build(17_000.0, 17_600.0);
    let c = land.counts;
    eprintln!("land: {c:?}");
    assert!(c.cells >= 2 && c.cells <= 16, "{c:?}");
    assert!(c.conifers > 300, "{c:?}");
    assert!(c.far_conifers > 100, "{c:?}");
    assert!(c.conifers > c.oaks * 5, "{c:?}");
    // Every instance stands clear of the road, above the sea, within reach.
    let t = w.track.as_ref().unwrap();
    let tr = w.terrain.as_ref().unwrap();
    let root = mp_worldgen::land::group_of(&w.graph, w.root).expect("the land group");
    let mut n = 0;
    for &m in &w.graph.get(root).children {
        let o = w.graph.get(m);
        let inst = o.instances.as_ref().expect("instanced");
        for k in 0..inst.count as usize {
            let mat = inst.get_matrix_at(k);
            let (x, y, z) = (mat.elements[12], mat.elements[13], mat.elements[14]);
            let f = tr.far(x, z);
            assert!(f.d <= REACH, "an instance {:.0} m from the road", f.d);
            let hw = f64::from(t.hw[t.idx(f.s)]);
            assert!(f.d > hw + 1.5, "an instance on the road: {:.1} m out", f.d);
            assert!(y > 1.0, "an instance under the sea at y {y:.1}");
            n += 1;
        }
    }
    assert_eq!(n, c.conifers + c.far_conifers + c.oaks + c.scrub);
}

#[test]
fn a_cell_plants_the_same_whatever_section_asks_for_it() {
    // Two sections that both reach one cell: that cell's instances agree.
    let (a, _) = build(17_000.0, 17_300.0);
    let (b, _) = build(17_300.0, 17_600.0);
    let items = |w: &mp_worldgen::world::World| {
        let root = mp_worldgen::land::group_of(&w.graph, w.root).unwrap();
        let mut v: Vec<(i64, i64, usize, Vec<[f64; 3]>)> = Vec::new();
        for &m in &w.graph.get(root).children {
            let o = w.graph.get(m);
            let inst = o.instances.as_ref().unwrap();
            let mat = inst.get_matrix_at(0);
            let (ci, cj) = (
                (mat.elements[12] / CELL).floor() as i64,
                (mat.elements[14] / CELL).floor() as i64,
            );
            let pts = (0..inst.count as usize)
                .map(|k| {
                    let m = inst.get_matrix_at(k);
                    [m.elements[12], m.elements[13], m.elements[14]]
                })
                .collect();
            v.push((ci, cj, o.geometry.map_or(0, |g| g.0 as usize), pts));
        }
        v
    };
    let (ia, ib) = (items(&a), items(&b));
    let shared: Vec<_> = ia
        .iter()
        .filter(|x| ib.iter().any(|y| (y.0, y.1, y.2) == (x.0, x.1, x.2)))
        .collect();
    assert!(!shared.is_empty(), "the two sections share a cell");
    for x in shared {
        let y = ib
            .iter()
            .find(|y| (y.0, y.1, y.2) == (x.0, x.1, x.2))
            .unwrap();
        assert_eq!(x.3, y.3, "cell ({}, {}) template {}", x.0, x.1, x.2);
    }
}
