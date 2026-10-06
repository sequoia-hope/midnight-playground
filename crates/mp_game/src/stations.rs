//! Screenshot stations (DECISIONS D17) from the Rust client: natively,
//! `--stations <stations.json> --out <dir>` flies the debug camera to each
//! station `tools/parity/shots.mjs` listed for a level (same names, same
//! fly-camera parameters), lets the sky, the environment map and the
//! pipelines settle, saves `<dir>/<name>.png`, and exits after the last.
//! `cargo xtask parity stations` runs it and compares with the JS shots.

use crate::CameraState;
use crate::options::FlyParams;
use crate::render::pmrem::{ENV_DONE, EnvRequest};
use crate::status::{PIPELINES_WAITING, Status};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Resource)]
struct StationRun {
    stations: Vec<(String, FlyParams)>,
    index: usize,
    frames: u32,
    quiet: u32,
    capturing: bool,
    out: std::path::PathBuf,
}

static CAPTURED: AtomicBool = AtomicBool::new(false);

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

pub fn plugin(app: &mut App, path: &str, out: Option<String>) {
    let stations = match read_stations(path) {
        Ok(s) => s,
        Err(e) => {
            error!("{e}");
            Vec::new()
        }
    };
    app.insert_resource(StationRun {
        stations,
        index: 0,
        frames: 0,
        quiet: 0,
        capturing: false,
        out: out.unwrap_or_else(|| "stations-rust".into()).into(),
    })
    .add_systems(Update, run.after(crate::fly_system));
}

fn run(
    mut commands: Commands,
    mut sr: ResMut<StationRun>,
    mut cs: ResMut<CameraState>,
    status: Res<Status>,
    env: Res<EnvRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    if !status.ready {
        if status.state == "failed" {
            exit.write(AppExit::error());
        }
        return;
    }
    let sr = &mut *sr;
    if sr.capturing {
        if CAPTURED.load(Ordering::Relaxed) {
            sr.capturing = false;
            sr.index += 1;
            sr.frames = 0;
            sr.quiet = 0;
        }
        return;
    }
    let Some((name, params)) = sr.stations.get(sr.index).cloned() else {
        info!("stations: done");
        exit.write(AppExit::Success);
        return;
    };
    if sr.frames == 0 {
        cs.fly = Some(params);
    }
    sr.frames += 1;
    let settled = PIPELINES_WAITING.load(Ordering::Relaxed) == 0
        && ENV_DONE.load(Ordering::Relaxed) == env.generation;
    if settled && sr.frames > 3 {
        sr.quiet += 1;
    } else {
        sr.quiet = 0;
    }
    if sr.quiet >= 3 {
        let _ = std::fs::create_dir_all(&sr.out);
        CAPTURED.store(false, Ordering::Relaxed);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(sr.out.join(format!("{name}.png"))))
            .observe(|_: On<ScreenshotCaptured>| CAPTURED.store(true, Ordering::Relaxed));
        sr.capturing = true;
    }
}
