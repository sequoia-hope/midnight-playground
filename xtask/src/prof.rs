//! `cargo xtask prof [--only webgpu|webgl2]`: the profiling copy of the web
//! build, `dist/prof/`, whose wasm keeps its function names.
//!
//! `cargo xtask web --release` strips the names (`wasm-opt -Oz`), so a CPU
//! profile of `dist/next/` shows `wasm-function[1234]`. This copies
//! `dist/next/` to `dist/prof/` and re-runs wasm-bindgen with `--keep-debug`
//! and wasm-opt with `-g` on the release wasm already in `target/` (no
//! rebuild), so the profile names every function, and the Bevy system it
//! runs in. `tools/parity/rust-prof.mjs --dist prof` profiles it. The copy
//! is about twice the size and is never served to players; `dist/` stays
//! out of git.

use crate::web::{BACKENDS, Backend, TARGET, WASM_FEATURES, copy_dir};
use crate::{Result, exec, root};
use std::path::Path;
use std::process::Command;

pub fn run(args: &[String]) -> Result {
    let mut only: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--only" => only = Some(it.next().ok_or("--only needs webgpu or webgl2")?.clone()),
            bad => return Err(format!("unknown option `{bad}` (--only webgpu|webgl2)")),
        }
    }
    if let Some(o) = &only
        && !BACKENDS.iter().any(|b| b.name == o)
    {
        return Err(format!("--only {o}: webgpu or webgl2"));
    }
    let root = root();
    let live = root.join("dist").join("next");
    if !live.join("build.json").exists() {
        return Err("dist/next/ is not built: run `cargo xtask web --release` first".into());
    }
    let out = root.join("dist").join("prof");
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("clearing {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;
    copy_dir(&live, &out)?;
    for b in BACKENDS
        .iter()
        .filter(|b| only.as_deref().is_none_or(|o| o == b.name))
    {
        names_kept(b, &out)?;
    }
    println!("prof: dist/prof/ built; profile it with tools/parity/rust-prof.mjs --dist prof");
    Ok(())
}

/// wasm-bindgen and wasm-opt of one backend's release wasm, names kept.
fn names_kept(b: &Backend, out: &Path) -> Result {
    let root = root();
    let target_root = match b.target_dir {
        Some(d) => root.join("target").join(d),
        None => root.join("target"),
    };
    let wasm = target_root
        .join(TARGET)
        .join("web-release")
        .join("mp_game.wasm");
    if !wasm.exists() {
        return Err(format!(
            "{} is not built: run `cargo xtask web --release` first",
            wasm.display()
        ));
    }
    println!("prof: {} with names kept", b.name);
    exec(
        Command::new("wasm-bindgen")
            .args([
                "--target",
                "web",
                "--no-typescript",
                "--keep-debug",
                "--out-name",
                b.out_name,
                "--out-dir",
            ])
            .arg(out)
            .arg(&wasm),
    )?;
    let bg = format!("{}_bg.wasm", b.out_name);
    exec(
        Command::new("wasm-opt")
            .args(["-Oz", "-g"])
            .args(WASM_FEATURES)
            .arg(out.join(&bg))
            .arg("-o")
            .arg(out.join(&bg)),
    )
    .map_err(|e| format!("{e}\n(install with: cargo install wasm-opt --locked)"))?;
    // The server would send the stale, stripped, precompressed copies.
    for ext in ["gz", "br"] {
        for name in [&bg, &format!("{}.js", b.out_name)] {
            let _ = std::fs::remove_file(out.join(format!("{name}.{ext}")));
        }
    }
    Ok(())
}
