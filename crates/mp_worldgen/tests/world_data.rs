//! WP 3.9: the world data a level build gives the simulation (SPEC 4.3:
//! the runout, the opposite carriageway) equals the JS world's, dumped in
//! WP 0.4 (`parity/golden/sim/world-data.json`), on every level, and equals
//! what the simulation computes without a build (`mp_levels::world`,
//! DECISIONS D471).
//!
//! Each level runs `World.build`'s plan stage with its scenery (the ported
//! modules' own `plan()`, the others' stand-ins), which settles the runout,
//! then each module's `build()` where that is cheap: the stand-ins. A
//! ported module builds after the road, so the levels with one stop after
//! the plans: City's `build()`, which sets Sierra's and the cruise loop's
//! carriageway, is held to the same in `tests/city.rs`, and Level 2's
//! (Harbor's) in `tests/coast.rs`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_levels::world::{level_world_data, world_data};
use mp_worldgen::scenery::{plan_only_factory, scenery_factory};
use mp_worldgen::terrain_mesh::terrain_stages;
use mp_worldgen::world::{Build, Stages, level_jobs};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/sim/world-data.json");

#[test]
fn world_data_equals_the_js_world() {
    let g: Value = serde_json::from_str(GOLDEN).expect("world data golden parses");
    for id in common::LEVELS {
        let want = &g["levels"][id];
        // A ported module's build needs the road; its levels stop after the
        // plans.
        let city = id == "sierra" || id == "cruise" || id == "coast";
        let (terrain, fields, _) = terrain_stages(common::terrain_setup(id));
        let stages = Stages {
            terrain: Some(terrain),
            fields: Some(fields),
            ..Stages::default()
        };
        let rec = Some(common::recording(id));
        let jobs = if city {
            level_jobs(stages, plan_only_factory(rec))
        } else {
            level_jobs(stages, scenery_factory(rec))
        };
        let mut b = Build::new(common::world(id), jobs);
        while !b.is_done() {
            b.step().expect("the level builds");
        }
        let w = &b.world;
        assert!(w.graph.log.is_empty(), "{id}: {:?}", w.graph.log);
        let t = w.track();
        let runout = want["track"]["runout"].as_f64().expect("runout");
        assert_eq!(t.runout.to_bits(), runout.to_bits(), "{id}: track.runout");
        assert_eq!(
            w.sim_data.runout.to_bits(),
            runout.to_bits(),
            "{id}: the simulation's runout"
        );
        match want["track"]["roadEnd"].as_f64() {
            Some(e) => assert_eq!(t.road_end().to_bits(), e.to_bits(), "{id}: roadEnd"),
            None => assert!(t.road_end().is_infinite(), "{id}: roadEnd"),
        }

        // What the simulation computes from the level and a fresh Track.
        let fresh = mp_track::Track::new(&w.level).expect("track");
        let sim = level_world_data(&w.level, &fresh);
        assert_eq!(
            sim.runout.to_bits(),
            runout.to_bits(),
            "{id}: mp_levels runout"
        );
        if id != "seaside" {
            assert_eq!(world_data(id), sim, "{id}: world_data by id");
        }

        let oc = &want["oppositeCarriageway"];
        if city {
            assert!(!oc.is_null(), "{id}: City or Harbor sets a carriageway");
            continue;
        }
        assert_eq!(
            w.sim_data.opposite_carriageway, sim.opposite_carriageway,
            "{id}"
        );
        match &w.sim_data.opposite_carriageway {
            None => assert!(oc.is_null(), "{id}: the JS has an opposite carriageway"),
            Some(o) => {
                let f = |k: &str| oc[k].as_f64().expect("a number");
                assert_eq!(o.s0.to_bits(), f("s0").to_bits(), "{id}: s0");
                assert_eq!(o.s1.to_bits(), f("s1").to_bits(), "{id}: s1");
                assert_eq!(o.dir.to_bits(), f("dir").to_bits(), "{id}: dir");
                let lanes: Vec<f64> = oc["lanes"]
                    .as_array()
                    .expect("lanes")
                    .iter()
                    .map(|v| v.as_f64().expect("a lane"))
                    .collect();
                assert_eq!(o.lanes, lanes, "{id}: lanes");
                for p in oc["ySamples"].as_array().expect("ySamples") {
                    let (s, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
                    assert_eq!(o.y(t, s).to_bits(), y.to_bits(), "{id}: oppY at s {s}");
                }
            }
        }
    }
}
