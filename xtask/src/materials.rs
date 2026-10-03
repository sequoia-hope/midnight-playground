//! `cargo xtask parity materials [--only <names>] [--js-run a] [--rerun-js]`:
//! the material test scenes on both sides and their comparison, in one
//! command (roadmap WP 2.3's gate, run as WP 2.5 asks).
//!
//! 1. The JS renders: `node tools/parity/materials.mjs --run <js-run>`, unless
//!    `parity/cache/<key>/materials/<js-run>/` is already there (or with
//!    `--rerun-js`). The Rust side reads the levels' scene exports too, so
//!    `node tools/parity/scene-export.mjs` runs first if they are missing.
//! 2. The Rust renders: the native client, `--materials <names> --out
//!    parity/cache/<key>/materials/rust/` (`all` by default: the fixed scenes
//!    and the kinds WP 2.3 ports; `every` adds the other kinds drawn as
//!    their plain stand-ins).
//! 3. `cargo xtask parity shots --a <rust> --b <js> --label materials`: the
//!    CIEDE2000 metric with SPEC 12's limits, the report in
//!    `parity/report/shots-materials/`. Fails if a scene is over a limit.

use crate::{Result, cargo, exec, root, shots};
use std::process::Command;

pub fn run(args: &[String]) -> Result {
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let only = opt("--only").unwrap_or_else(|| "all".into());
    let js_run = opt("--js-run").unwrap_or_else(|| "a".into());
    let rerun_js = args.iter().any(|a| a == "--rerun-js");
    let root = root();
    let key =
        mr_scene::cache::js_tree_key(&root).map_err(|e| format!("hashing the JS tree: {e}"))?;
    let cache = root.join("parity/cache").join(&key);

    let scenes = cache.join("scenes");
    if ["sierra", "coast", "streets", "desert", "models"]
        .iter()
        .any(|l| !scenes.join(format!("{l}.mrscene")).exists())
    {
        println!("materials: exporting the scenes (node tools/parity/scene-export.mjs)");
        exec(Command::new("node").arg("tools/parity/scene-export.mjs"))?;
    }
    let js = cache.join("materials").join(&js_run);
    if rerun_js || !js.join("fixed").is_dir() {
        println!(
            "materials: rendering the JS side (node tools/parity/materials.mjs --run {js_run})"
        );
        exec(Command::new("node").args(["tools/parity/materials.mjs", "--run", &js_run]))?;
    }

    let rust = cache.join("materials/rust");
    let _ = std::fs::remove_dir_all(&rust);
    println!("materials: rendering the Rust side ({only})");
    exec(cargo().args(["build", "-p", "mr_game"]))?;
    let exe = root.join("target/debug/midnight-racer");
    exec(Command::new(exe).args([
        "--materials",
        &only,
        "--out",
        rust.to_str().ok_or("cache path is not UTF-8")?,
    ]))?;

    shots::run(&[
        "--a".into(),
        rust.to_string_lossy().into_owned(),
        "--b".into(),
        js.to_string_lossy().into_owned(),
        "--label".into(),
        "materials".into(),
    ])?;
    let summary = std::fs::read_to_string(root.join("parity/report/shots-materials/summary.json"))
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&summary).map_err(|e| e.to_string())?;
    let failing = v["failing"].as_u64().unwrap_or(0);
    for s in v["perStation"].as_array().into_iter().flatten() {
        println!(
            "  {:<40} mean {:6.3}  block95 {:6.3}",
            s["station"].as_str().unwrap_or(""),
            s["mean"].as_f64().unwrap_or(0.0),
            s["block95"].as_f64().unwrap_or(0.0)
        );
    }
    if failing > 0 {
        return Err(format!(
            "{failing} scene(s) over the limits (mean {}, block95 {})",
            shots::MEAN_LIMIT,
            shots::BLOCK_LIMIT
        ));
    }
    Ok(())
}
