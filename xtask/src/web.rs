//! `cargo xtask web [--release]`: build the web client into `dist/next/`
//! (SPEC 11). `dist/` is git-ignored; the registered dev server serves the
//! repo root, so the build is at `/dist/next/` there and on the tailnet https
//! front. Nothing here knows or writes a port.

use crate::{Result, cargo, exec, output, root};
use std::path::Path;
use std::process::Command;

const TARGET: &str = "wasm32-unknown-unknown";

/// Features the wasm32 target enables by default in current stable Rust;
/// wasm-opt must be told, or it rejects the module.
const WASM_FEATURES: &[&str] = &[
    "--enable-bulk-memory",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    "--enable-reference-types",
    "--enable-multivalue",
];

pub fn run(args: &[String]) -> Result {
    let release = args.iter().any(|a| a == "--release");
    if let Some(bad) = args.iter().find(|a| *a != "--release") {
        return Err(format!("unknown option `{bad}`"));
    }
    let root = root();
    let profile = if release { "web-release" } else { "dev" };

    check_bindgen_version()?;

    let mut build = cargo();
    build.args([
        "build",
        "-p",
        "mr_game",
        "--lib",
        "--target",
        TARGET,
        "--profile",
        profile,
    ]);
    exec(&mut build)?;

    // The dev profile's output directory is called debug.
    let profile_dir = if release { "web-release" } else { "debug" };
    let wasm = root
        .join("target")
        .join(TARGET)
        .join(profile_dir)
        .join("mr_game.wasm");

    let out = root.join("dist").join("next");
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("clearing {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;

    exec(
        Command::new("wasm-bindgen")
            .args([
                "--target",
                "web",
                "--no-typescript",
                "--out-name",
                "mr_game",
                "--out-dir",
            ])
            .arg(&out)
            .arg(&wasm),
    )?;

    let bg = out.join("mr_game_bg.wasm");
    if release {
        // -Oz: size first. SPEC 11 allows size optimisation where a benchmark
        // shows no frame-time cost; revisit once there are frames to time.
        exec(
            Command::new("wasm-opt")
                .arg("-Oz")
                .args(WASM_FEATURES)
                .arg(&bg)
                .arg("-o")
                .arg(&bg),
        )
        .map_err(|e| format!("{e}\n(install with: cargo install wasm-opt --locked)"))?;
    }

    copy_dir(&root.join("crates/mr_game/web"), &out)?;
    // Where the page finds the scene exports: the parity cache of this JS
    // tree, relative to dist/next/ (the registered server serves the repo
    // root, and every URL the client uses is relative; DECISIONS D102).
    let key = mr_scene::cache::js_tree_key(&root)
        .map_err(|e| format!("hashing the JS tree for the scene cache key: {e}"))?;
    let scenes_rel = mr_scene::cache::scenes_rel(&key);
    std::fs::write(
        out.join("build.json"),
        format!(
            "{{\"profile\": \"{profile}\", \"wasm_opt\": {release}, \"scenes\": \"../../{scenes_rel}\"}}\n"
        ),
    )
    .map_err(|e| format!("writing build.json: {e}"))?;
    if !root.join(&scenes_rel).is_dir() {
        println!(
            "web: no scene exports for this JS tree yet ({scenes_rel}); make them with\n\
             \x20    node tools/parity/scene-export.mjs"
        );
    }
    if release {
        precompress(&out)?;
    }

    println!(
        "web: built {} ({profile}) into dist/next/\n\
         \x20    open /dist/next/ on the registered server (proj up midnight-racer),\n\
         \x20    or on phones at the project's tailnet web_url + dist/next/",
        bg.file_name().unwrap().to_string_lossy()
    );
    Ok(())
}

/// The wasm-bindgen CLI must be the same version as the crate in Cargo.lock,
/// or the glue it writes does not match the module.
fn check_bindgen_version() -> Result {
    let lock = std::fs::read_to_string(root().join("Cargo.lock"))
        .map_err(|e| format!("reading Cargo.lock: {e}"))?;
    let want = locked_version(&lock, "wasm-bindgen").ok_or("wasm-bindgen is not in Cargo.lock")?;
    let have = output(Command::new("wasm-bindgen").arg("--version")).map_err(|_| {
        format!("wasm-bindgen CLI not found; install: cargo install wasm-bindgen-cli --version {want} --locked")
    })?;
    let have = have.split_whitespace().nth(1).unwrap_or("").to_owned();
    if have != want {
        return Err(format!(
            "wasm-bindgen CLI is {have}, the crate is {want}; install: \
             cargo install wasm-bindgen-cli --version {want} --locked"
        ));
    }
    Ok(())
}

fn locked_version(lock: &str, name: &str) -> Option<String> {
    let needle = format!("name = \"{name}\"");
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == needle {
            let v = lines.next()?.trim();
            return Some(
                v.strip_prefix("version = \"")?
                    .strip_suffix('"')?
                    .to_owned(),
            );
        }
    }
    None
}

/// Writes `<file>.gz` beside the wasm and the JS glue, for a server that
/// sends precompressed files, so load times on phones are as they will be
/// (SPEC 6.6). `tools/serve.py` sends them for files under `dist/`.
fn precompress(out: &Path) -> Result {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    for name in ["mr_game_bg.wasm", "mr_game.js"] {
        let src = out.join(name);
        let bytes = std::fs::read(&src).map_err(|e| format!("reading {}: {e}", src.display()))?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&bytes).map_err(|e| e.to_string())?;
        let gz = gz.finish().map_err(|e| e.to_string())?;
        let dest = out.join(format!("{name}.gz"));
        std::fs::write(&dest, gz).map_err(|e| format!("writing {}: {e}", dest.display()))?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result {
    let entries =
        std::fs::read_dir(from).map_err(|e| format!("reading {}: {e}", from.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let dest = to.join(path.file_name().unwrap());
        if path.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_dir(&path, &dest)?;
        } else {
            std::fs::copy(&path, &dest).map_err(|e| format!("copying {}: {e}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reads_locked_version() {
        let lock = "[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.129\"\n";
        assert_eq!(
            super::locked_version(lock, "wasm-bindgen").as_deref(),
            Some("0.2.129")
        );
        assert_eq!(super::locked_version(lock, "js-sys"), None);
    }
}
