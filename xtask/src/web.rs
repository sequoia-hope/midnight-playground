//! `cargo xtask web [--release] [--only webgpu|webgl2]`: build the web
//! client into `dist/next/` (SPEC 11): the WebGPU build and the WebGL2 one,
//! which the page picks between (SPEC 2, roadmap WP 2.7). `dist/` is
//! git-ignored; the registered dev server serves the repo root, so the build
//! is at `/dist/next/` there and on the tailnet https front. Nothing here
//! knows or writes a port.
//!
//! The build is made in `dist/next.staging/` and swapped in whole once it
//! is done, so `dist/next/` keeps serving the last complete build for the
//! minutes a build takes (a phone opening it meanwhile used to find a
//! directory listing, or one backend's new files beside the other's old).

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

/// One build of the client. Bevy picks its backend at compile time, so the
/// WebGL2 fallback is a second wasm file, chosen by the page (SPEC 2,
/// DECISIONS D391). The WebGL2 one is built with the `mp_webgl2` cfg in its
/// own target directory, so neither build's cache is thrown away by the
/// other's flags.
pub struct Backend {
    pub name: &'static str,
    /// wasm-bindgen's `--out-name`: `<out>.js` and `<out>_bg.wasm`.
    pub out_name: &'static str,
    /// Below `target/`; none for the default.
    pub target_dir: Option<&'static str>,
    pub cfg: Option<&'static str>,
}

pub const BACKENDS: [Backend; 2] = [
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
];

pub fn run(args: &[String]) -> Result {
    let mut release = false;
    let mut only: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--release" => release = true,
            "--only" => only = Some(it.next().ok_or("--only needs webgpu or webgl2")?.clone()),
            bad => {
                return Err(format!(
                    "unknown option `{bad}` (--release, --only webgpu|webgl2)"
                ));
            }
        }
    }
    if let Some(o) = &only
        && !BACKENDS.iter().any(|b| b.name == o)
    {
        return Err(format!("--only {o}: webgpu or webgl2"));
    }
    let root = root();
    let profile = if release { "web-release" } else { "dev" };

    check_bindgen_version()?;

    let live = root.join("dist").join("next");
    let out = root.join("dist").join("next.staging");
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(|e| format!("clearing {}: {e}", out.display()))?;
    }
    std::fs::create_dir_all(&out).map_err(|e| format!("creating {}: {e}", out.display()))?;
    // `--only` replaces one build's files and keeps the other's.
    if only.is_some() && live.is_dir() {
        copy_dir(&live, &out)?;
    }

    let mut built = Vec::new();
    for b in BACKENDS
        .iter()
        .filter(|b| only.as_deref().is_none_or(|o| o == b.name))
    {
        build_backend(b, release, profile, &out)?;
        built.push(b.name);
    }

    build_exhaust(release, &out)?;
    copy_dir(&root.join("crates/mp_game/web"), &out)?;
    copy_runtime_files(&root, &out)?;
    // Where the page finds the scene exports: the parity cache of this JS
    // tree, relative to dist/next/ (the registered server serves the repo
    // root, and every URL the client uses is relative; DECISIONS D102).
    let key = mp_scene::cache::js_tree_key(&root)
        .map_err(|e| format!("hashing the JS tree for the scene cache key: {e}"))?;
    let scenes_rel = mp_scene::cache::scenes_rel(&key);
    let backends = BACKENDS
        .iter()
        .filter(|b| out.join(format!("{}_bg.wasm", b.out_name)).exists())
        .map(|b| format!("\"{}\"", b.name))
        .collect::<Vec<_>>()
        .join(", ");
    std::fs::write(
        out.join("build.json"),
        format!(
            "{{\"profile\": \"{profile}\", \"wasm_opt\": {release}, \"backends\": [{backends}], \"scenes\": \"../../{scenes_rel}\"}}\n"
        ),
    )
    .map_err(|e| format!("writing build.json: {e}"))?;
    swap_in(&out, &live)?;
    if !root.join(&scenes_rel).is_dir() {
        println!(
            "web: no scene exports for this JS tree yet ({scenes_rel}); make them with\n\
             \x20    node tools/parity/scene-export.mjs"
        );
    }

    println!(
        "web: built {} ({profile}) into dist/next/\n\
         \x20    open /dist/next/ on the registered server (proj up midnight-racer),\n\
         \x20    or on phones at the project's tailnet web_url + dist/next/",
        built.join(" and ")
    );
    Ok(())
}

/// The repository files the page loads at run time, copied beside it, so
/// the build is whole wherever it is served: at `dist/next/` on the
/// registered server, or at the site root on GitHub Pages since the cutover
/// (D1112). Each is (source in the repository, path under `dist/next/`).
pub const RUNTIME_FILES: &[(&str, &str)] = &[
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
];

fn copy_runtime_files(root: &Path, out: &Path) -> Result {
    for (from, to) in RUNTIME_FILES {
        let (src, dest) = (root.join(from), out.join(to));
        if src.is_dir() {
            copy_dir(&src, &dest)?;
        } else {
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("creating {}: {e}", dir.display()))?;
            }
            std::fs::copy(&src, &dest).map_err(|e| format!("copying {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// Builds one backend's wasm and runs wasm-bindgen (and, for a release,
/// wasm-opt and gzip) into `out`.
fn build_backend(b: &Backend, release: bool, profile: &str, out: &Path) -> Result {
    let root = root();
    let target_root = match b.target_dir {
        Some(d) => root.join("target").join(d),
        None => root.join("target"),
    };
    let mut build = cargo();
    build.args([
        "build",
        "-p",
        "mp_game",
        "--lib",
        "--target",
        TARGET,
        "--profile",
        profile,
    ]);
    if b.target_dir.is_some() {
        build.arg("--target-dir").arg(&target_root);
    }
    if let Some(cfg) = b.cfg {
        add_cfg(&mut build, cfg);
    }
    println!("web: building the {} client", b.name);
    exec(&mut build)?;

    // The dev profile's output directory is called debug.
    let profile_dir = if release { "web-release" } else { "debug" };
    let wasm = target_root
        .join(TARGET)
        .join(profile_dir)
        .join("mp_game.wasm");
    exec(
        Command::new("wasm-bindgen")
            .args([
                "--target",
                "web",
                "--no-typescript",
                "--out-name",
                b.out_name,
                "--out-dir",
            ])
            .arg(out)
            .arg(&wasm),
    )?;

    let bg = format!("{}_bg.wasm", b.out_name);
    if release {
        // -Oz: size first. SPEC 11 allows size optimisation where a benchmark
        // shows no frame-time cost; revisit once there are frames to time.
        exec(
            Command::new("wasm-opt")
                .arg("-Oz")
                .args(WASM_FEATURES)
                .arg(out.join(&bg))
                .arg("-o")
                .arg(out.join(&bg)),
        )
        .map_err(|e| format!("{e}\n(install with: cargo install wasm-opt --locked)"))?;
        precompress(out, &[&bg, &format!("{}.js", b.out_name)])?;
    }
    Ok(())
}

/// The exhaust node's files (`mp_audio`'s web backend loads them beside the
/// page): the model as its own small wasm, with the C-ABI exports of its
/// `worklet` feature (kept out of the game's wasm), and the AudioWorklet
/// shim that runs it. Optimised even for a debug build (the `release`
/// profile): it runs per sample on the audio thread.
fn build_exhaust(release: bool, out: &Path) -> Result {
    let root = root();
    let profile = if release { "web-release" } else { "release" };
    let mut build = cargo();
    build.args([
        "build",
        "-p",
        "mp_exhaust",
        "--lib",
        "--features",
        "worklet",
        "--target",
        TARGET,
        "--profile",
        profile,
    ]);
    println!("web: building the exhaust worklet");
    exec(&mut build)?;
    let wasm = root
        .join("target")
        .join(TARGET)
        .join(profile)
        .join("mp_exhaust.wasm");
    let name = "mp_exhaust.wasm";
    let dest = out.join(name);
    std::fs::copy(&wasm, &dest).map_err(|e| format!("copying {}: {e}", wasm.display()))?;
    let js = "exhaust-worklet.js";
    let src = root.join("crates/mp_audio/web").join(js);
    std::fs::copy(&src, out.join(js)).map_err(|e| format!("copying {}: {e}", src.display()))?;
    if release {
        exec(
            Command::new("wasm-opt")
                .arg("-O3")
                .args(WASM_FEATURES)
                .arg(&dest)
                .arg("-o")
                .arg(&dest),
        )
        .map_err(|e| format!("{e}\n(install with: cargo install wasm-opt --locked)"))?;
        precompress(out, &[name, js])?;
    }
    Ok(())
}

/// Adds `--cfg <cfg>` to a cargo command's rustflags, keeping the ones the
/// caller set (CI sets `-D warnings`).
pub fn add_cfg(cmd: &mut Command, cfg: &str) {
    if let Ok(enc) = std::env::var("CARGO_ENCODED_RUSTFLAGS") {
        let sep = if enc.is_empty() { "" } else { "\x1f" };
        cmd.env(
            "CARGO_ENCODED_RUSTFLAGS",
            format!("{enc}{sep}--cfg\x1f{cfg}"),
        );
    } else {
        let flags = std::env::var("RUSTFLAGS").unwrap_or_default();
        cmd.env("RUSTFLAGS", format!("{flags} --cfg {cfg}").trim());
    }
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

/// Writes `<file>.br` and `<file>.gz` beside the given files, for a server
/// that sends precompressed files, so load times on phones are as they will
/// be (SPEC 6.6). `tools/serve.py` sends them for files under `dist/`:
/// brotli to browsers that take it (every current one, on https), gzip to
/// the rest (D677).
fn precompress(out: &Path, names: &[&str]) -> Result {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    for name in names {
        let src = out.join(name);
        let bytes = std::fs::read(&src).map_err(|e| format!("reading {}: {e}", src.display()))?;
        let mut gz = GzEncoder::new(Vec::new(), Compression::best());
        gz.write_all(&bytes).map_err(|e| e.to_string())?;
        let gz = gz.finish().map_err(|e| e.to_string())?;
        let dest = out.join(format!("{name}.gz"));
        std::fs::write(&dest, gz).map_err(|e| format!("writing {}: {e}", dest.display()))?;

        // Quality 11 with the largest standard window (16 MB): the wasm is
        // built once and downloaded many times.
        let mut br = Vec::new();
        let params = brotli::enc::BrotliEncoderParams {
            quality: 11,
            lgwin: 24,
            size_hint: bytes.len(),
            ..Default::default()
        };
        brotli::BrotliCompress(&mut &bytes[..], &mut br, &params).map_err(|e| e.to_string())?;
        let dest = out.join(format!("{name}.br"));
        std::fs::write(&dest, br).map_err(|e| format!("writing {}: {e}", dest.display()))?;
    }
    Ok(())
}

/// Puts the finished build in place: two renames, so `dist/next/` is
/// missing for a moment rather than incomplete for minutes.
fn swap_in(staging: &Path, live: &Path) -> Result {
    let old = live.with_extension("old");
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| format!("clearing {}: {e}", old.display()))?;
    }
    if live.exists() {
        std::fs::rename(live, &old).map_err(|e| format!("moving {} aside: {e}", live.display()))?;
    }
    std::fs::rename(staging, live)
        .map_err(|e| format!("moving {} into place: {e}", staging.display()))?;
    if old.exists() {
        std::fs::remove_dir_all(&old).map_err(|e| format!("removing {}: {e}", old.display()))?;
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
            std::fs::create_dir_all(&dest)
                .map_err(|e| format!("creating {}: {e}", dest.display()))?;
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
