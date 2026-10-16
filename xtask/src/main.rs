//! `cargo xtask <command>`: build, parity and size tooling (SPEC 11).
//!
//! The build tooling is Playground Kit's `pk_xtask` (DECISIONS D1233):
//! this file is its configuration (the `WebBuild` below) plus this game's
//! JS-parity commands.
//!
//! Commands:
//!   check-deps        enforce the crate dependency rules (SPEC 3.2; deps.toml)
//!   web [--release]   build the web client into dist/next/
//!   prof              dist/prof/: the web build with function names kept, for CPU profiles
//!   size [--budget]   report the size of the built wasm (raw, gzip, brotli)
//!   imgdiff           compare two directories of screenshots (CIEDE2000)
//!   kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits
//!   parity shots      compare two sets of screenshots (CIEDE2000), write a report
//!   parity scene-check [files]  read exported scenes back and check their digests
//!   parity materials  the material test scenes, JS and Rust, compared (WP 2.3)
//!   parity stations   the screenshot stations, JS and Rust, compared (WP 2.5)

mod kernel;
mod materials;
mod parity_scene;
mod shots;
mod stations;

use pk_xtask::{Backend, Command, RuntimeFiles, SideWasm, WebBuild};
pub use pk_xtask::{Result, cargo, exec, output, root};
use std::path::Path;

/// The web build (SPEC 2, 11; roadmap WP 2.7): the WebGPU client and the
/// WebGL2 one (the `mp_webgl2` cfg, its own target directory; D391), the
/// two AudioWorklet wasms (D1110, D1151), the section builder's worker
/// wasm (D1215), the fonts, Seaside's survey and photo and the radio clips
/// copied in so the build is whole wherever it is served (D1112), and the
/// scene cache key in build.json (D102).
static WEB: WebBuild = WebBuild {
    crate_name: "mp_game",
    backends: &[
        Backend {
            name: "webgpu",
            out_name: "mp_game",
            target_dir: None,
            cfg: None,
        },
        Backend {
            name: "webgl2",
            out_name: "mp_game_webgl2",
            target_dir: Some("webgl2"),
            cfg: Some("mp_webgl2"),
        },
    ],
    page_dir: "crates/mp_game/web",
    dist_dir: "dist/next",
    runtime_files: RuntimeFiles {
        required: &[
            (
                "assets/fonts/rajdhani/Rajdhani-Medium.ttf",
                "assets/fonts/rajdhani/Rajdhani-Medium.ttf",
            ),
            (
                "assets/fonts/rajdhani/Rajdhani-SemiBold.ttf",
                "assets/fonts/rajdhani/Rajdhani-SemiBold.ttf",
            ),
            (
                "assets/fonts/rajdhani/Rajdhani-Bold.ttf",
                "assets/fonts/rajdhani/Rajdhani-Bold.ttf",
            ),
            (
                "assets/fonts/rajdhani/OFL.txt",
                "assets/fonts/rajdhani/OFL.txt",
            ),
            ("assets/seaside/survey.bin", "assets/seaside/survey.bin"),
            ("src/levels/seaside/photo.jpg", "assets/seaside/photo.jpg"),
            ("audio/radio", "audio/radio"),
        ],
        // The DJ clips (D1151) are recorded locally and not committed yet.
        optional: &[("audio/dj", "audio/dj")],
    },
    side_wasm: &[
        // The exhaust node's wasm and its AudioWorklet shim (D1110).
        SideWasm {
            krate: "mp_exhaust",
            features: &["worklet"],
            opt_level: "-O3",
            shim_js: Some("crates/mp_audio/web/exhaust-worklet.js"),
            what: "exhaust worklet",
        },
        // The radio node's: the Music Lab's port with the station player (D1151).
        SideWasm {
            krate: "mp_music",
            features: &["worklet"],
            opt_level: "-O3",
            shim_js: Some("crates/mp_audio/web/music-worklet.js"),
            what: "radio worklet",
        },
        // The open world's section builder for the page's worker (D1215).
        SideWasm {
            krate: "mp_builder",
            features: &["worker"],
            opt_level: "-O3",
            shim_js: Some("crates/mp_game/web/builder-worker.js"),
            what: "section builder's worker",
        },
    ],
    build_json_extra: Some(build_json_extra),
    wasm_opt: "116",
    size_budget_gzip: pk_xtask::size::DEFAULT_BUDGET_GZIP,
};

/// Where the page finds the scene exports: the parity cache of this JS
/// tree, relative to dist/next/ (the registered server serves the repo
/// root, and every URL the client uses is relative; DECISIONS D102).
fn build_json_extra(root: &Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    let rel = scenes_rel(root)?;
    let mut m = serde_json::Map::new();
    m.insert("scenes".into(), serde_json::json!(format!("../../{rel}")));
    Ok(m)
}

fn scenes_rel(root: &Path) -> Result<String> {
    let key = mp_scene::cache::js_tree_key(root)
        .map_err(|e| format!("hashing the JS tree for the scene cache key: {e}"))?;
    Ok(mp_scene::cache::scenes_rel(&key))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut commands = pk_xtask::web::commands(&WEB);
    commands.push(Command::new(
        "kernel",
        "kernel [--check]  build the math kernel's wasm for the JS oracle; check its bits",
        kernel::run,
    ));
    commands.push(Command::new(
        "parity",
        "parity shots --a <dir> --b <dir> [--label name]\n\
         \x20                  compare two sets of screenshots; report in parity/report/\n\
         \x20 parity scene-check [files]  read exported .mrscene files back and check their digests\n\
         \x20 parity materials [--only all|every|<names>] [--js-run a] [--rerun-js]\n\
         \x20                  the material test scenes rendered by the JS and the Rust client, compared\n\
         \x20 parity stations [--levels a,b] [--js-run a] [--rerun-js] [--label name]\n\
         \x20                 [--rust-run rust] [--base] [--gate level/name.png,...]\n\
         \x20                  the screenshot stations of the JS and the Rust client, compared;\n\
         \x20                  --base: terrain, road and sky only (WP 2.4's gate on Sierra)",
        |rest| match rest.first().map(String::as_str) {
            Some("shots") => shots::run(&rest[1..]),
            Some("scene-check") => parity_scene::run(&rest[1..]),
            Some("materials") => materials::run(&rest[1..]),
            Some("stations") => stations::run(&rest[1..]),
            _ => Err("which comparison? shots, scene-check, materials or stations".into()),
        },
    ));
    if let Err(e) = pk_xtask::main_with(args.iter().cloned(), &commands) {
        eprintln!("xtask {e}");
        std::process::exit(1);
    }
    if args.first().map(String::as_str) == Some("web")
        && let Ok(rel) = scenes_rel(&root())
        && !root().join(&rel).is_dir()
    {
        println!(
            "web: no scene exports for this JS tree yet ({rel}); make them with\n\
             \x20    node tools/parity/scene-export.mjs"
        );
    }
}
