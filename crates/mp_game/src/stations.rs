//! Screenshot stations (DECISIONS D17) from the Rust client: natively,
//! `--stations <stations.json> --out <dir>` flies the debug camera to each
//! station `tools/parity/shots.mjs` listed for a level (same names, same
//! fly-camera parameters), lets the sky, the environment map and the
//! pipelines settle, saves `<dir>/<name>.png`, and exits after the last
//! (Playground Kit's `pk_shell::stations`, D1234). `cargo xtask parity
//! stations` runs it and compares with the JS shots.

use crate::CameraState;
use crate::options::FlyParams;
use crate::render::pmrem::{ENV_DONE, EnvRequest};
use bevy::prelude::*;
use serde_json::Value;
use std::sync::atomic::Ordering;

/// Reads `stations.json` (as `tools/parity/shots.mjs` writes it).
pub fn read_stations(path: &str) -> Result<Vec<(String, FlyParams)>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    let f = |s: &Value, k: &str| s[k].as_f64().unwrap_or(0.0);
    Ok(v["stations"]
        .as_array()
        .ok_or_else(|| format!("{path}: no stations"))?
        .iter()
        .map(|s| {
            (
                s["name"].as_str().unwrap_or("station").to_string(),
                FlyParams {
                    s: f(s, "s"),
                    h: f(s, "h"),
                    back: f(s, "back"),
                    lat: f(s, "lat"),
                    speed: 0.0,
                    yaw: f(s, "yaw"),
                    pitch: f(s, "pitch"),
                },
            )
        })
        .collect())
}

/// The fly camera to the station.
fn apply(world: &mut World, p: &FlyParams) {
    world.resource_mut::<CameraState>().fly = Some(*p);
}

/// The environment map is built for the camera's time of day.
fn settled(world: &World) -> bool {
    ENV_DONE.load(Ordering::Relaxed) == world.resource::<EnvRequest>().generation
}

pub fn plugin(app: &mut App, path: &str, out: Option<String>) {
    let stations = match read_stations(path) {
        Ok(s) => s,
        Err(e) => {
            error!("{e}");
            Vec::new()
        }
    };
    app.insert_resource(pk_shell::stations::Stations::new(
        stations,
        out.unwrap_or_else(|| "stations-rust".into()).into(),
        apply,
        settled,
    ))
    .add_systems(
        Update,
        pk_shell::stations::run::<FlyParams>.after(crate::fly_system),
    );
}
