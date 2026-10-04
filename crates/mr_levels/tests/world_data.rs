//! The world data the simulation takes from the JS scenery (SPEC 4.3),
//! computed by the scenery's rules from each level and its Track (WP 3.9,
//! DECISIONS D471), against the dump of the live JS world
//! (`parity/golden/sim/world-data.json`, WP 0.4): runout, roadEnd, the
//! opposite carriageway's range, lanes and direction, and its height
//! through the ported `oppY` at the dumped samples.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_levels::world::{level_world_data, world_data};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/sim/world-data.json");

#[test]
fn world_data_matches_the_js_world() {
    let g: Value = serde_json::from_str(GOLDEN).unwrap();
    for (level, t) in common::tracks() {
        let id = level.id;
        let want = &g["levels"][id];
        let w = level_world_data(level, t);
        assert_eq!(world_data(id), w, "{id}: world_data by id");
        assert_eq!(
            want["track"]["runout"].as_f64().unwrap(),
            w.runout,
            "{id}: runout"
        );
        let mut t = t.clone();
        t.runout = w.runout;
        match want["track"]["roadEnd"].as_f64() {
            Some(e) => assert_eq!(e, t.road_end(), "{id}: roadEnd"),
            None => assert!(t.road_end().is_infinite(), "{id}: roadEnd"),
        }
        let oc = &want["oppositeCarriageway"];
        match &w.opposite_carriageway {
            None => assert!(oc.is_null(), "{id}: has an opposite carriageway in JS"),
            Some(o) => {
                assert_eq!(oc["s0"].as_f64().unwrap(), o.s0, "{id}: s0");
                assert_eq!(oc["s1"].as_f64().unwrap(), o.s1, "{id}: s1");
                assert_eq!(oc["dir"].as_f64().unwrap(), o.dir, "{id}: dir");
                let lanes: Vec<f64> = oc["lanes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_f64().unwrap())
                    .collect();
                assert_eq!(lanes, o.lanes, "{id}: lanes");
                for p in oc["ySamples"].as_array().unwrap() {
                    let (s, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
                    assert_eq!(o.y(&t, s).to_bits(), y.to_bits(), "{id}: oppY at s {s}");
                }
            }
        }
    }
}
