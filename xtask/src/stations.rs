//! `cargo xtask parity stations [--levels sierra,coast,...] [--js-run a]
//! [--rerun-js] [--label name]`: the screenshot stations (DECISIONS D17) on
//! both sides and their comparison, in one command (roadmap WP 2.5).
//!
//! 1. The JS shots: `node tools/parity/shots.mjs --run <js-run> --only
//!    <levels>`, for the levels whose `parity/cache/<key>/shots/<js-run>/
//!    <level>/stations.json` is missing (or all with `--rerun-js`).
//! 2. The Rust shots: the native client per level, `--stations <that
//!    stations.json> --out parity/cache/<key>/shots/rust/<level>/ --query
//!    freeze=1` (the scenery frozen as the JS shots are).
//! 3. `cargo xtask parity shots --a <rust> --b <js>` (CIEDE2000, SPEC 12's
//!    limits), the report in `parity/report/shots-<label>/`. It reports; it
//!    does not fail on stations over the limits, since most kinds are still
//!    stand-ins (WP 2.4 and M3 onwards).

use crate::{Result, cargo, exec, root, shots};
use std::process::Command;

const LEVELS: [&str; 6] = ["sierra", "coast", "streets", "desert", "seaside", "cruise"];

pub fn run(args: &[String]) -> Result {
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let levels: Vec<String> = opt("--levels")
        .map(|l| l.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|| LEVELS.iter().map(|l| l.to_string()).collect());
    let js_run = opt("--js-run").unwrap_or_else(|| "a".into());
    let label = opt("--label").unwrap_or_else(|| "stations".into());
    let rerun_js = args.iter().any(|a| a == "--rerun-js");
    let root = root();
    let key =
        mr_scene::cache::js_tree_key(&root).map_err(|e| format!("hashing the JS tree: {e}"))?;
    let cache = root.join("parity/cache").join(&key);
    let js = cache.join("shots").join(&js_run);
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
        println!("stations: the JS shots of {only} (node tools/parity/shots.mjs)");
        exec(Command::new("node").args([
            "tools/parity/shots.mjs",
            "--run",
            &js_run,
            "--only",
            &only,
        ]))?;
    }
    exec(cargo().args(["build", "-p", "mr_game"]))?;
    let exe = root.join("target/debug/midnight-racer");
    let rust = cache.join("shots/rust");
    let _ = std::fs::remove_dir_all(&rust);
    for level in &levels {
        let out = rust.join(level);
        println!("stations: {level}, Rust");
        let stations = js.join(level).join("stations.json");
        exec(Command::new(&exe).args([
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
        ]))?;
    }
    shots::run(&[
        "--a".into(),
        rust.to_string_lossy().into_owned(),
        "--b".into(),
        js.to_string_lossy().into_owned(),
        "--label".into(),
        label,
    ])
}
