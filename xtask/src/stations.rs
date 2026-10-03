//! `cargo xtask parity stations [--levels sierra,coast,...] [--js-run a]
//! [--rerun-js] [--label name] [--rust-run rust] [--base] [--gate a,b,...]`:
//! the screenshot stations (DECISIONS D17) on both sides and their
//! comparison, in one command (roadmap WP 2.5).
//!
//! 1. The JS shots: `node tools/parity/shots.mjs --run <js-run> --only
//!    <levels>`, for the levels whose `parity/cache/<key>/shots/<js-run>/
//!    <level>/stations.json` is missing (or all with `--rerun-js`).
//! 2. The Rust shots: the native client per level, `--stations <that
//!    stations.json> --out parity/cache/<key>/shots/<rust-run>/<level>/
//!    --query freeze=1` (the scenery frozen as the JS shots are).
//! 3. `cargo xtask parity shots --a <rust> --b <js>` (CIEDE2000, SPEC 12's
//!    limits), the report in `parity/report/shots-<label>/`. It reports; it
//!    does not fail on stations over the limits, since most kinds are still
//!    stand-ins (M3 onwards), except for the stations named by `--gate`.
//!
//! `--base` (roadmap WP 2.4's gate): the terrain, road and sky only. The JS
//! side is `tools/parity/base-shots.mjs` (the game with everything else
//! hidden) into `shots/<js-run>.base/`, the Rust side draws the base export
//! (`scenes/<level>.base.mrscene`, made by `node tools/parity/scene-export.mjs
//! --base` if missing) into `shots/<rust-run>.base/`, and the gate defaults
//! to [`BASE_GATE`] on Sierra.

use crate::{Result, cargo, exec, root, shots};
use std::process::Command;

const LEVELS: [&str; 6] = ["sierra", "coast", "streets", "desert", "seaside", "cruise"];

/// WP 2.4's gate: five Sierra stations of the base export, chosen before
/// any were compared (DECISIONS D292): the attract view and four along the
/// route, chase and high views, from the pass in daylight to the city at
/// night.
pub const BASE_GATE: [&str; 5] = [
    "attract",
    "02000-chase",
    "04500-high",
    "07000-chase",
    "09500-high",
];

pub fn run(args: &[String]) -> Result {
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let base = args.iter().any(|a| a == "--base");
    let levels: Vec<String> = opt("--levels")
        .map(|l| l.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|| {
            if base {
                vec!["sierra".into()]
            } else {
                LEVELS.iter().map(|l| l.to_string()).collect()
            }
        });
    let suffix = if base { ".base" } else { "" };
    let js_run = opt("--js-run").unwrap_or_else(|| "a".into());
    let rust_run = opt("--rust-run").unwrap_or_else(|| "rust".into());
    let label = opt("--label").unwrap_or_else(|| {
        if base {
            "stations-base".into()
        } else {
            "stations".into()
        }
    });
    let gate: Vec<String> = match opt("--gate") {
        Some(g) => g.split(',').map(str::to_owned).collect(),
        None if base && levels.iter().any(|l| l == "sierra") => BASE_GATE
            .iter()
            .map(|s| format!("sierra/{s}.png"))
            .collect(),
        None => Vec::new(),
    };
    let rerun_js = args.iter().any(|a| a == "--rerun-js");
    let root = root();
    let key =
        mr_scene::cache::js_tree_key(&root).map_err(|e| format!("hashing the JS tree: {e}"))?;
    let cache = root.join("parity/cache").join(&key);
    let js = cache.join("shots").join(format!("{js_run}{suffix}"));
    let missing: Vec<&String> = levels
        .iter()
        .filter(|l| rerun_js || !js.join(l).join("stations.json").exists())
        .collect();
    if !missing.is_empty() {
        let only = missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",");
        let tool = if base {
            "tools/parity/base-shots.mjs"
        } else {
            "tools/parity/shots.mjs"
        };
        println!("stations: the JS shots of {only} (node {tool})");
        exec(Command::new("node").args([tool, "--run", &js_run, "--only", &only]))?;
    }
    let scenes = cache.join("scenes");
    if base
        && levels
            .iter()
            .any(|l| !scenes.join(format!("{l}.base.mrscene")).exists())
    {
        println!("stations: exporting the base scenes (node tools/parity/scene-export.mjs --base)");
        exec(Command::new("node").args([
            "tools/parity/scene-export.mjs",
            "--base",
            "--levels",
            &levels.join(","),
        ]))?;
    }
    exec(cargo().args(["build", "-p", "mr_game"]))?;
    let exe = root.join("target/debug/midnight-racer");
    let rust = cache.join("shots").join(format!("{rust_run}{suffix}"));
    let _ = std::fs::remove_dir_all(&rust);
    for level in &levels {
        let out = rust.join(level);
        println!("stations: {level}, Rust");
        let stations = js.join(level).join("stations.json");
        let mut cmd = Command::new(&exe);
        cmd.args([
            "--level",
            level,
            "--query",
            "freeze=1",
            "--size",
            "1280x800",
            "--stations",
            stations.to_str().ok_or("cache path is not UTF-8")?,
            "--out",
            out.to_str().ok_or("cache path is not UTF-8")?,
        ]);
        if base {
            let scene = scenes.join(format!("{level}.base.mrscene"));
            cmd.args(["--scene", scene.to_str().ok_or("cache path is not UTF-8")?]);
        }
        exec(&mut cmd)?;
    }
    shots::run(&[
        "--a".into(),
        rust.to_string_lossy().into_owned(),
        "--b".into(),
        js.to_string_lossy().into_owned(),
        "--label".into(),
        label.clone(),
    ])?;
    if gate.is_empty() {
        return Ok(());
    }
    let summary = std::fs::read_to_string(
        root.join("parity/report")
            .join(format!("shots-{label}"))
            .join("summary.json"),
    )
    .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&summary).map_err(|e| e.to_string())?;
    let mut failing = 0;
    println!("stations: the gate");
    for name in &gate {
        let row = v["perStation"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|s| s["station"].as_str() == Some(name.as_str()))
            .ok_or_else(|| format!("gate station {name} was not compared"))?;
        let (mean, block) = (
            row["mean"].as_f64().unwrap_or(f64::INFINITY),
            row["block95"].as_f64().unwrap_or(f64::INFINITY),
        );
        let pass = mean < shots::MEAN_LIMIT && block < shots::BLOCK_LIMIT;
        if !pass {
            failing += 1;
        }
        println!(
            "  {name:<32} mean {mean:6.3}  block95 {block:6.3}  {}",
            if pass { "pass" } else { "OVER" }
        );
    }
    if failing > 0 {
        return Err(format!(
            "{failing} gate station(s) over the limits (mean {}, block95 {})",
            shots::MEAN_LIMIT,
            shots::BLOCK_LIMIT
        ));
    }
    Ok(())
}
