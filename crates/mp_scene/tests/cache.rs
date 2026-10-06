//! The JS tree key and the scene cache paths: the key matches what
//! `tools/parity/lib/jstree.mjs` computes for a fixed tree (the values below
//! were produced by running it on the same files), changes with any input's
//! content, name or presence, ignores files outside the inputs, and a
//! missing input is an error.

use mp_scene::cache::{js_tree_key, scenes_dir, scenes_rel};
use std::path::{Path, PathBuf};

/// A small tree with every input kind: nested directories, a directory and
/// a file that sort next to each other (`sub` before `sub.js`), upper case
/// before lower case, an empty file and a binary one.
fn fixture(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("mp_scene_cache_{name}"));
    let _ = std::fs::remove_dir_all(&root);
    let files: &[(&str, &[u8])] = &[
        ("src/a.js", b"a"),
        ("src/B.js", b"B"),
        ("src/sub/x.js", b"x();\n"),
        ("src/sub.js", b""),
        ("vendor/three.js", b"three"),
        ("index.html", b"<!doctype html>"),
        ("tools/parity/kernel/mr_kernel.wasm", b"\0asm\x01\0\0\0"),
    ];
    for (rel, bytes) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    root
}

#[test]
fn the_key_matches_the_js_for_a_fixed_tree() {
    let root = fixture("js");
    assert_eq!(js_tree_key(&root).unwrap(), "de8799c6d1677ba0");
    std::fs::write(root.join("src/a.js"), b"b").unwrap();
    assert_eq!(js_tree_key(&root).unwrap(), "435976ee397a4950");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_key_follows_every_input_and_nothing_else() {
    let root = fixture("follow");
    let base = js_tree_key(&root).unwrap();
    assert_eq!(base.len(), 16);
    assert!(
        base.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    assert_eq!(js_tree_key(&root).unwrap(), base, "stable");

    // Outside the inputs: no change.
    std::fs::write(root.join("README.md"), b"x").unwrap();
    std::fs::create_dir_all(root.join("tools/parity/lib")).unwrap();
    std::fs::write(root.join("tools/parity/lib/jstree.mjs"), b"x").unwrap();
    assert_eq!(js_tree_key(&root).unwrap(), base);

    // Each of these is a new key, and undoing it restores the old one.
    type Edit = fn(&Path);
    let edits: &[(&str, Edit, Edit)] = &[
        (
            "content",
            |r| std::fs::write(r.join("vendor/three.js"), b"three!").unwrap(),
            |r| std::fs::write(r.join("vendor/three.js"), b"three").unwrap(),
        ),
        (
            "an empty file gains a byte",
            |r| std::fs::write(r.join("src/sub.js"), b"\0").unwrap(),
            |r| std::fs::write(r.join("src/sub.js"), b"").unwrap(),
        ),
        (
            "a new file",
            |r| std::fs::write(r.join("src/sub/y.js"), b"").unwrap(),
            |r| std::fs::remove_file(r.join("src/sub/y.js")).unwrap(),
        ),
        (
            "a rename",
            |r| std::fs::rename(r.join("src/a.js"), r.join("src/c.js")).unwrap(),
            |r| std::fs::rename(r.join("src/c.js"), r.join("src/a.js")).unwrap(),
        ),
        (
            "the kernel",
            |r| std::fs::write(r.join("tools/parity/kernel/mr_kernel.wasm"), b"").unwrap(),
            |r| {
                std::fs::write(
                    r.join("tools/parity/kernel/mr_kernel.wasm"),
                    b"\0asm\x01\0\0\0",
                )
                .unwrap()
            },
        ),
        (
            "the page",
            |r| std::fs::write(r.join("index.html"), b"<!DOCTYPE html>").unwrap(),
            |r| std::fs::write(r.join("index.html"), b"<!doctype html>").unwrap(),
        ),
    ];
    for (what, edit, undo) in edits {
        edit(&root);
        assert_ne!(js_tree_key(&root).unwrap(), base, "{what}");
        undo(&root);
        assert_eq!(js_tree_key(&root).unwrap(), base, "undoing {what}");
    }
    std::fs::remove_dir_all(&root).unwrap();
}

/// Moving bytes between two files changes the key: the separators keep
/// "ab" + "" apart from "a" + "b".
#[test]
fn moving_bytes_between_files_changes_the_key() {
    let root = fixture("boundary");
    std::fs::write(root.join("src/B.js"), b"").unwrap();
    std::fs::write(root.join("src/a.js"), b"aB").unwrap();
    let k1 = js_tree_key(&root).unwrap();
    std::fs::write(root.join("src/B.js"), b"B").unwrap();
    std::fs::write(root.join("src/a.js"), b"a").unwrap();
    // B.js sorts before a.js, so the bytes moved across the boundary.
    std::fs::write(root.join("src/B.js"), b"Ba").unwrap();
    std::fs::write(root.join("src/a.js"), b"").unwrap();
    let k2 = js_tree_key(&root).unwrap();
    assert_ne!(k1, k2);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_missing_input_is_an_error() {
    for missing in [
        "src",
        "vendor",
        "index.html",
        "tools/parity/kernel/mr_kernel.wasm",
    ] {
        let root = fixture("missing");
        let p = root.join(missing);
        if p.is_dir() {
            std::fs::remove_dir_all(&p).unwrap();
        } else {
            std::fs::remove_file(&p).unwrap();
        }
        assert!(js_tree_key(&root).is_err(), "{missing}");
        assert!(scenes_dir(&root).is_err(), "{missing}");
        std::fs::remove_dir_all(&root).unwrap();
    }
    assert!(js_tree_key(Path::new("/nonexistent/mp_scene/root")).is_err());
}

#[test]
fn the_scenes_directory_is_under_the_key() {
    assert_eq!(
        scenes_rel("0123abcd0123abcd"),
        "parity/cache/0123abcd0123abcd/scenes/"
    );
    let root = fixture("dir");
    let key = js_tree_key(&root).unwrap();
    assert_eq!(
        scenes_dir(&root).unwrap(),
        root.join("parity/cache").join(&key).join("scenes")
    );
    assert!(
        !root.join("parity").exists(),
        "computing the path creates nothing"
    );
    std::fs::remove_dir_all(&root).unwrap();
}

/// The repository's own tree has a key (every input is present).
#[test]
fn the_repository_has_a_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let key = js_tree_key(&root).unwrap();
    assert_eq!(key.len(), 16);
}
