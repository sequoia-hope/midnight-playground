//! `cargo xtask parity scene-check [files...]` (roadmap WP 0.5): reads each
//! `.mrscene` the JS exporter wrote (`tools/parity/scene-export.mjs`) with
//! `mp_scene`, recomputes its digest and compares it with the digest the
//! exporter took from the live three.js scene (`<name>.digest.json` beside
//! it). It also writes each scene back out and reads it again, and checks
//! that the digest file is the one the committed golden names
//! (`parity/golden/world/<level>.json`).
//!
//! Without arguments it checks every scene in
//! `parity/cache/<js-tree-key>/scenes/`.

use crate::{Result, root};
use mp_scene::digest::{SceneDigest, compare, digest, sha256_hex};
use sha2::{Digest as _, Sha256};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub fn run(args: &[String]) -> Result {
    let files: Vec<PathBuf> = if args.is_empty() {
        let dir = root()
            .join("parity/cache")
            .join(js_tree_key()?)
            .join("scenes");
        let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| {
                format!(
                    "{}: {e} (run `node tools/parity/scene-export.mjs` first)",
                    dir.display()
                )
            })?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "mrscene"))
            .collect();
        v.sort();
        v
    } else {
        args.iter().map(PathBuf::from).collect()
    };
    if files.is_empty() {
        return Err("no .mrscene files to check".into());
    }
    let mut failed = 0;
    for f in &files {
        match check(f) {
            Ok(line) => println!("ok    {line}"),
            Err(e) => {
                failed += 1;
                println!("FAIL  {}: {e}", f.display());
            }
        }
    }
    if failed == 0 {
        println!("scene-check: {} scene(s) match their digests", files.len());
        Ok(())
    } else {
        Err(format!("{failed} of {} scene(s) differ", files.len()))
    }
}

fn check(file: &Path) -> Result<String> {
    let t0 = Instant::now();
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    let scene = mp_scene::read(&bytes)?;
    let t_read = t0.elapsed();
    let mine = digest(&scene);
    let t_digest = t0.elapsed() - t_read;

    let digest_file = file.with_extension("digest.json");
    let text =
        std::fs::read(&digest_file).map_err(|e| format!("{}: {e}", digest_file.display()))?;
    let theirs: SceneDigest =
        serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", digest_file.display()))?;
    let diffs = compare(&theirs, &mine);
    if !diffs.is_empty() {
        let shown: Vec<&str> = diffs.iter().take(8).map(String::as_str).collect();
        return Err(format!(
            "{} difference(s) from the live digest:\n  {}",
            diffs.len(),
            shown.join("\n  ")
        ));
    }

    // Written back by mp_scene and read again: the same scene.
    let again = mp_scene::read(&mp_scene::write(&scene)?)?;
    if again != scene {
        return Err("writing the scene and reading it back changed it".into());
    }

    // The digest is the one the committed golden names.
    let name = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let golden = root()
        .join("parity/golden/world")
        .join(format!("{name}.json"));
    let mut note = String::new();
    if golden.exists() {
        let g: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&golden).map_err(|e| format!("{}: {e}", golden.display()))?,
        )
        .map_err(|e| format!("{}: {e}", golden.display()))?;
        let want = g
            .pointer("/scene/digest_sha256")
            .or_else(|| g.get("digest_sha256"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("{}: no digest_sha256", golden.display()))?;
        if want != sha256_hex(&text) {
            return Err(format!(
                "the digest differs from the golden {} (JS changed? rerun the exporter and commit)",
                golden.display()
            ));
        }
        note = ", = golden".into();
    }

    let kinds: Vec<String> = mine.kinds.iter().map(|(k, v)| format!("{k}×{v}")).collect();
    Ok(format!(
        "{name}: {:.1} MB, {} nodes, {} meshes, {} materials, {} textures, {} vertices; read {:.2}s, digest {:.2}s{note}\n      kinds: {}",
        bytes.len() as f64 / 1e6,
        mine.counts.nodes,
        mine.counts.meshes,
        mine.counts.materials,
        mine.counts.textures,
        mine.counts.vertices,
        t_read.as_secs_f64(),
        t_digest.as_secs_f64(),
        kinds.join(" ")
    ))
}

/// The JS tree key of `tools/parity/lib/jstree.mjs`: SHA-256 over the paths
/// and contents of the game's files, first 16 hex digits.
pub fn js_tree_key() -> Result<String> {
    const INPUTS: &[&str] = &[
        "src",
        "vendor",
        "index.html",
        "tools/parity/kernel/mr_kernel.wasm",
    ];
    fn walk(base: &Path, rel: &str, out: &mut Vec<String>) -> Result {
        let abs = base.join(rel);
        let meta = std::fs::metadata(&abs).map_err(|e| format!("{}: {e}", abs.display()))?;
        if meta.is_dir() {
            let mut names: Vec<String> = std::fs::read_dir(&abs)
                .map_err(|e| format!("{}: {e}", abs.display()))?
                .filter_map(|e| e.ok().and_then(|e| e.file_name().into_string().ok()))
                .collect();
            names.sort();
            for n in names {
                walk(base, &format!("{rel}/{n}"), out)?;
            }
        } else {
            out.push(rel.to_owned());
        }
        Ok(())
    }
    let base = root();
    let mut files = Vec::new();
    for rel in INPUTS {
        walk(&base, rel, &mut files)?;
    }
    let mut h = Sha256::new();
    for rel in &files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(std::fs::read(base.join(rel)).map_err(|e| format!("{rel}: {e}"))?);
        h.update([0]);
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex[..16].to_owned())
}
