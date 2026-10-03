//! Shared by the terrain tests: the levels (Seaside with its survey), the
//! recorded scenery plans (`parity/golden/terrain/<level>.json`, written by
//! `tools/parity/terrain-plan.mjs`) and a level's terrain built as
//! `World.build` builds it, with the scenery's plans run by the ported
//! modules where there are any and replayed from the recordings otherwise
//! (`mr_worldgen::scenery`, DECISIONS D330).

#![allow(dead_code)]

use std::sync::{Arc, OnceLock};

use mr_levels::{SeasideData, level_by_id, seaside};
use mr_track::{Level, Track};
use mr_worldgen::scenery::{PlanRecording, scenery_factory};
use mr_worldgen::terrain::{Carve, DesertRail, Flatten, Terrain, TerrainOpts, TerrainPlan};
use mr_worldgen::terrain_mesh::{TerrainSetup, seaside_ground_color, terrain_stages};
use mr_worldgen::world::{Build, Stages, World, level_jobs};
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

/// The road plan golden's text (`parity/golden/road/<level>.json`).
pub fn road_text(id: &str) -> &'static str {
    match id {
        "sierra" => include_str!("../../../../parity/golden/road/sierra.json"),
        "coast" => include_str!("../../../../parity/golden/road/coast.json"),
        "streets" => include_str!("../../../../parity/golden/road/streets.json"),
        "desert" => include_str!("../../../../parity/golden/road/desert.json"),
        "seaside" => include_str!("../../../../parity/golden/road/seaside.json"),
        "cruise" => include_str!("../../../../parity/golden/road/cruise.json"),
        _ => panic!("no road golden for {id}"),
    }
}

fn terrain_text(id: &str) -> &'static str {
    match id {
        "sierra" => include_str!("../../../../parity/golden/terrain/sierra.json"),
        "coast" => include_str!("../../../../parity/golden/terrain/coast.json"),
        "streets" => include_str!("../../../../parity/golden/terrain/streets.json"),
        "desert" => include_str!("../../../../parity/golden/terrain/desert.json"),
        "seaside" => include_str!("../../../../parity/golden/terrain/seaside.json"),
        "cruise" => include_str!("../../../../parity/golden/terrain/cruise.json"),
        _ => panic!("no plan for {id}"),
    }
}

fn split_text(id: &str) -> &'static str {
    match id {
        "sierra" => include_str!("../../../../parity/golden/scenery-plan/sierra.json"),
        "coast" => include_str!("../../../../parity/golden/scenery-plan/coast.json"),
        "streets" => include_str!("../../../../parity/golden/scenery-plan/streets.json"),
        "desert" => include_str!("../../../../parity/golden/scenery-plan/desert.json"),
        "seaside" => include_str!("../../../../parity/golden/scenery-plan/seaside.json"),
        "cruise" => include_str!("../../../../parity/golden/scenery-plan/cruise.json"),
        _ => panic!("no scenery plan for {id}"),
    }
}

/// A level's recorded plan stage, split by module.
pub fn recording(id: &str) -> Arc<PlanRecording> {
    Arc::new(
        PlanRecording::parse(terrain_text(id), road_text(id), split_text(id))
            .expect("the plan recordings parse and agree"),
    )
}

/// The terrain setup for a build whose scenery registers the plan
/// (`scenery_factory`): no recorded plan of its own.
pub fn terrain_setup(id: &str) -> TerrainSetup {
    TerrainSetup {
        plan: None,
        ground_color: (id == "seaside").then(|| seaside_ground_color(survey())),
        ..TerrainSetup::default()
    }
}

/// A level's world after "Shaping the land", as `World.build` leaves it:
/// the track, the terrain with every scenery module's plan registered in
/// order (the ported modules' own `plan()`, the others replayed), fields
/// built and flattens resolved.
pub fn planned(id: &str) -> World {
    let (terrain, fields, _) = terrain_stages(terrain_setup(id));
    let stages = Stages {
        terrain: Some(terrain),
        fields: Some(fields),
        ..Stages::default()
    };
    let mut b = Build::new(
        World::new(level(id)),
        level_jobs(stages, scenery_factory(Some(recording(id)))),
    );
    for _ in 0..2 {
        b.step().expect("the plan stage runs");
    }
    b.world
}

/// `terrain(id, Some(&plan(id)))`, with the plan registered by the scenery.
pub fn planned_terrain(id: &str) -> (Level, Track, Terrain) {
    let w = planned(id);
    (
        w.level,
        w.track.expect("surveyed"),
        w.terrain.expect("terrain made"),
    )
}

/// The repo root.
pub fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
