//! Shared by the terrain tests: the levels (Seaside with its survey), the
//! recorded scenery plans (`parity/golden/terrain/<level>.json`, written by
//! `tools/parity/terrain-plan.mjs`) and a level's terrain built as
//! `World.build` builds it.

#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use mr_levels::{SeasideData, level_by_id, seaside};
use mr_track::{Level, Track};
use mr_worldgen::terrain::{Carve, DesertRail, Flatten, Terrain, TerrainOpts, TerrainPlan};
use mr_worldgen::terrain_mesh::seaside_ground_color;
use serde_json::Value;

pub const LEVELS: [&str; 6] = ["sierra", "coast", "streets", "desert", "seaside", "cruise"];

pub const SURVEY: &[u8] = include_bytes!("../../../../assets/seaside/survey.bin");

pub fn survey() -> Arc<SeasideData> {
    static DATA: OnceLock<Arc<SeasideData>> = OnceLock::new();
    DATA.get_or_init(|| Arc::new(SeasideData::parse(SURVEY).expect("survey.bin parses")))
        .clone()
}

/// A level ready to build (Seaside prepared).
pub fn level(id: &str) -> Level {
    let mut l = level_by_id(id);
    if id == "seaside" {
        seaside::prepare(&mut l, survey());
    }
    l
}

/// The plan golden of a level.
pub fn plan_json(id: &str) -> Value {
    let text = match id {
        "sierra" => include_str!("../../../../parity/golden/terrain/sierra.json"),
        "coast" => include_str!("../../../../parity/golden/terrain/coast.json"),
        "streets" => include_str!("../../../../parity/golden/terrain/streets.json"),
        "desert" => include_str!("../../../../parity/golden/terrain/desert.json"),
        "seaside" => include_str!("../../../../parity/golden/terrain/seaside.json"),
        "cruise" => include_str!("../../../../parity/golden/terrain/cruise.json"),
        _ => panic!("no plan for {id}"),
    };
    serde_json::from_str(text).expect("plan golden parses")
}

pub fn hex(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().expect("hex bits"), 16).expect("hex"))
}

fn opt_hex(v: &Value) -> Option<f64> {
    if v.is_null() { None } else { Some(hex(v)) }
}

/// The recorded plan.
pub fn plan(id: &str) -> TerrainPlan {
    let j = plan_json(id);
    let flattens = j["flattens"]
        .as_array()
        .expect("flattens")
        .iter()
        .map(|f| Flatten {
            x: hex(&f["x"]),
            z: hex(&f["z"]),
            r: hex(&f["r"]),
            falloff: hex(&f["falloff"]),
            y: opt_hex(&f["y"]),
            y_resolved: f.get("yResolved").map(hex),
        })
        .collect();
    let carves = j["carves"]
        .as_array()
        .expect("carves")
        .iter()
        .map(|c| {
            let points: Vec<[f64; 2]> = c["points"]
                .as_array()
                .expect("points")
                .iter()
                .map(|p| [hex(&p[0]), hex(&p[1])])
                .collect();
            Carve {
                points,
                width: hex(&c["width"]),
                depth: hex(&c["depth"]),
                under_road: c["underRoad"].as_bool().expect("underRoad"),
                min_x: 0.0,
                max_x: 0.0,
                min_z: 0.0,
                max_z: 0.0,
            }
        })
        .collect();
    let r = &j["desertRail"];
    let desert_rail = (!r.is_null()).then(|| DesertRail {
        lat: hex(&r["lat"]),
        half: hex(&r["half"]),
        drop: hex(&r["drop"]),
        s0: hex(&r["s0"]),
        s1: hex(&r["s1"]),
    });
    TerrainPlan {
        flattens,
        carves,
        desert_rail,
    }
}

/// A level's track and terrain as `World.build` makes them: `new Terrain`,
/// the scenery's plan (`plan`), `buildFields`, `resolveFlattens`.
pub fn terrain(id: &str, plan: Option<&TerrainPlan>) -> (Level, Track, Terrain) {
    let l = level(id);
    let t = Track::new(&l).expect("track");
    let mut tr = Terrain::new(&t, &l, &TerrainOpts::default()).expect("terrain");
    if id == "seaside" {
        tr.ground_color = Some(seaside_ground_color(survey()));
    }
    if let Some(p) = plan {
        p.apply(&mut tr);
    }
    tr.build_fields(&t);
    tr.resolve_flattens();
    (l, t, tr)
}

/// The repo root.
pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
