//! Where the exported scenes live: `parity/cache/<key>/scenes/`, with `<key>`
//! the hash of the JS tree that `tools/parity/lib/jstree.mjs` computes
//! (parity/README.md). The client finds a level's export with it natively,
//! and `cargo xtask web` writes it into the web build (DECISIONS D102).

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// What a capture depends on (`jstree.mjs` `INPUTS`), in its order.
const INPUTS: &[&str] = &[
    "src",
    "vendor",
    "index.html",
    "tools/parity/kernel/mr_kernel.wasm",
];

fn walk(root: &Path, rel: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    let abs = root.join(rel);
    if std::fs::metadata(&abs)?.is_dir() {
        let mut names: Vec<String> = std::fs::read_dir(&abs)?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_, _>>()?;
        // JS's default sort compares UTF-16 code units; for the ASCII names
        // in the tree that is byte order.
        names.sort();
        for name in names {
            walk(root, &format!("{rel}/{name}"), out)?;
        }
    } else {
        out.push(rel.to_owned());
    }
    Ok(())
}

/// The JS tree key (`jsTreeKey()`): SHA-256 over each input file's
/// repo-relative path, a NUL, its bytes and a NUL, in walk order; the first
/// 16 hex digits.
pub fn js_tree_key(root: &Path) -> std::io::Result<String> {
    let mut files = Vec::new();
    for rel in INPUTS {
        walk(root, rel, &mut files)?;
    }
    let mut h = Sha256::new();
    for rel in &files {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(std::fs::read(root.join(rel))?);
        h.update([0]);
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok(hex[..16].to_owned())
}

/// `parity/cache/<key>/scenes/` relative to the repo root.
pub fn scenes_rel(key: &str) -> String {
    format!("parity/cache/{key}/scenes/")
}

/// The scene exports' directory for the JS tree at `root`.
pub fn scenes_dir(root: &Path) -> std::io::Result<PathBuf> {
    Ok(root.join(scenes_rel(&js_tree_key(root)?)))
}
