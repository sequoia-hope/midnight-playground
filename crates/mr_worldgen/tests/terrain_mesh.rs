//! WP 3.4's L3 gate: the terrain meshes against the terrain of the JS
//! scene export (SPEC 5.7; DECISIONS D26). Each level's terrain is built as
//! `World.build` builds it (the scenery's plan replayed, D232), its meshes
//! go into an `mr_scene::Scene` through the object tree, and `mr_scene`'s
//! digest of each is compared with the JS digest of the export
//! (`parity/cache/<key>/scenes/<level>[.base].digest.json`, the meshes
//! under the group `terrain`): vertex and index counts, bounds, the SHA-256
//! of every attribute (`position`, `normal`, `color`, `uv`, `aSurf`) and of
//! the index, area and centroid. The material is compared parameter by
//! parameter, its textures by their pixels' digest.
//!
//! The requirement is identical vertex buffers. Where a mesh differs, the
//! test reports per attribute how many values differ and by how much (from
//! the scene file's buffers) before it fails.
//!
//! Needs the scene exports in the cache (`node tools/parity/scene-export.mjs
//! [--base]`); without them it says so and passes. Native only: the files
//! are large.

#![cfg(not(target_arch = "wasm32"))]

mod common;

use std::path::PathBuf;

use common::{planned_terrain, root};
use mr_scene::digest::{SceneDigest, digest};
use mr_scene::{BufferData, Scene};
use mr_worldgen::object::SceneGraph;
use mr_worldgen::terrain_mesh::{GroundPhoto, build_terrain_meshes};
use mr_worldgen::textures::{Texture, TextureCache};
use serde_json::Value;

/// The cached export of a level (the base export if there is one) and its
/// digest.
fn export(id: &str) -> Option<(PathBuf, PathBuf)> {
    let r = root();
    let dir = mr_scene::cache::scenes_dir(&r).ok()?;
    for name in [format!("{id}.base"), id.to_string()] {
        let scene = dir.join(format!("{name}.mrscene"));
        let dig = dir.join(format!("{name}.digest.json"));
        if scene.exists() && dig.exists() {
            return Some((scene, dig));
        }
    }
    None
}

/// The meshes under the group `terrain`, in order (scene mesh indices).
fn terrain_meshes(s: &Scene) -> (u32, Vec<u32>) {
    let ti = s
        .nodes
        .iter()
        .position(|n| n.name == "terrain")
        .expect("a terrain group");
    let meshes = s.nodes[ti]
        .children
        .iter()
        .map(|&c| s.nodes[c as usize].mesh.expect("a terrain mesh"))
        .collect();
    (ti as u32, meshes)
}

/// A material's parameters and uniforms with each texture reference
/// replaced by what the texture is (its description without the buffer
/// index, and its pixels' digest), so two scenes' materials compare.
fn material_view(s: &Scene, d: &SceneDigest, m: u32) -> Value {
    fn walk(v: &Value, s: &Scene, d: &SceneDigest) -> Value {
        match v {
            Value::Object(o) => {
                if o.len() == 1
                    && let Some(t) = o.get("texture").and_then(Value::as_u64)
                {
                    let tex = &s.textures[t as usize];
                    let mut desc = walk(&serde_json::to_value(tex).expect("json"), s, d);
                    desc["pixels"] = Value::Null;
                    let td = &d.textures[t as usize];
                    // A canvas texture's pixels are held to WP 3.2's
                    // threshold (tests/textures.rs), not to identity; the
                    // others must be identical.
                    let sha = if tex.source == mr_scene::TextureSource::Canvas {
                        Value::Null
                    } else {
                        Value::from(td.sha256.clone())
                    };
                    let item_size = s.buffers[tex.pixels as usize].item_size;
                    return serde_json::json!({ "texture": desc, "sha256": sha, "channels": td.channels, "item_size": item_size });
                }
                Value::Object(o.iter().map(|(k, x)| (k.clone(), walk(x, s, d))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, s, d)).collect()),
            // 1 and 1.0 are the same number in the JS.
            Value::Number(n) => Value::from(n.as_f64().expect("a number")),
            _ => v.clone(),
        }
    }
    let mut m = serde_json::to_value(&s.materials[m as usize]).expect("json");
    m = walk(&m, s, d);
    m
}

/// How two buffers differ: (values differing, largest difference).
fn buffer_diff(a: &BufferData, b: &BufferData) -> (usize, f64) {
    if a.len() != b.len() {
        return (a.len().max(b.len()), f64::INFINITY);
    }
    let mut n = 0;
    let mut worst = 0.0f64;
    for i in 0..a.len() {
        let (x, y) = (a.get(i), b.get(i));
        if x.to_bits() != y.to_bits() {
            n += 1;
            worst = worst.max((x - y).abs());
        }
    }
    (n, worst)
}

fn check(id: &str) {
    let Some((scene_path, digest_path)) = export(id) else {
        println!(
            "{id}: no scene export in the cache; run node tools/parity/scene-export.mjs --base"
        );
        return;
    };
    let js_digest: SceneDigest =
        serde_json::from_slice(&std::fs::read(&digest_path).expect("digest"))
            .expect("digest parses");
    let js = mr_scene::read_file(&scene_path).expect("scene reads");
    let (js_group, js_meshes) = terrain_meshes(&js);

    // The Rust terrain.
    let (_, _, tr) = planned_terrain(id);
    let photo = (id == "seaside").then(|| {
        // The photo decoded as Chrome decoded it (the JPEG decoder is the
        // client's; DECISIONS D234).
        let mat = js.nodes[js.nodes[js_group as usize].children[0] as usize].materials[0];
        let t = js.materials[mat as usize]
            .uniforms
            .as_ref()
            .expect("uniforms")["tPhoto"]["texture"]
            .as_u64()
            .expect("tPhoto") as usize;
        let td = &js.textures[t];
        let BufferData::U8(rgba) = &js.buffers[td.pixels as usize].data else {
            panic!("photo pixels are bytes")
        };
        let tex = Texture {
            width: td.width,
            height: td.height,
            rgba: rgba.clone(),
            source: mr_scene::TextureSource::Image,
            repeat: false,
            srgb: true,
            anisotropy: 8.0,
        };
        GroundPhoto::seaside(&common::survey(), tex, td.url.as_deref().expect("url"))
    });
    let mut graph = SceneGraph::new();
    let mut textures = TextureCache::new();
    let built = build_terrain_meshes(&mut graph, &mut textures, &tr, photo.as_ref());
    graph.add_root(built.group);
    let (rs, _) = graph.finish();
    let rd = digest(&rs);
    let (_, rs_meshes) = terrain_meshes(&rs);

    let mut problems = Vec::new();
    if rs_meshes.len() != js_meshes.len() {
        problems.push(format!(
            "{} meshes, the JS has {}",
            rs_meshes.len(),
            js_meshes.len()
        ));
    }
    let mut same = 0;
    let mut vertices = 0;
    for (k, (&rm, &jm)) in rs_meshes.iter().zip(&js_meshes).enumerate() {
        let (a, b) = (&rd.meshes[rm as usize], &js_digest.meshes[jm as usize]);
        vertices += b.vertices;
        if a == b {
            same += 1;
            continue;
        }
        let mut what = Vec::new();
        if a.vertices != b.vertices || a.indices != b.indices {
            what.push(format!(
                "counts {}/{} vs {}/{}",
                a.vertices, a.indices, b.vertices, b.indices
            ));
        }
        let (rmesh, jmesh) = (&rs.meshes[rm as usize], &js.meshes[jm as usize]);
        for attr in &jmesh.attributes {
            let Some(ra) = rmesh.attribute(&attr.name) else {
                what.push(format!("no {}", attr.name));
                continue;
            };
            if a.attributes.get(&attr.name) != b.attributes.get(&attr.name) {
                let (n, worst) = buffer_diff(
                    &rs.buffers[ra.accessor as usize].data,
                    &js.buffers[attr.accessor as usize].data,
                );
                what.push(format!("{}: {n} values differ, worst {worst:e}", attr.name));
            }
        }
        if a.index != b.index {
            let (n, _) = buffer_diff(
                &rs.buffers[rmesh.index.expect("index") as usize].data,
                &js.buffers[jmesh.index.expect("index") as usize].data,
            );
            what.push(format!("index: {n} values differ"));
        }
        if a.bounds != b.bounds {
            what.push(format!("bounds {:?} vs {:?}", a.bounds, b.bounds));
        }
        if a.area != b.area || a.centroid != b.centroid {
            what.push(format!(
                "area {:?} vs {:?}, centroid {:?} vs {:?}",
                a.area, b.area, a.centroid, b.centroid
            ));
        }
        problems.push(format!("mesh {k}: {}", what.join("; ")));
    }
    // The material, and the node flags.
    let rmat = rs.nodes[rs
        .nodes
        .iter()
        .position(|n| n.mesh == Some(rs_meshes[0]))
        .expect("node")]
    .materials[0];
    let jnode = &js.nodes[js.nodes[js_group as usize].children[0] as usize];
    let (mv, jv) = (
        material_view(&rs, &rd, rmat),
        material_view(&js, &js_digest, jnode.materials[0]),
    );
    if mv != jv {
        for key in [
            "kind",
            "kind_opts",
            "type",
            "name",
            "program_key",
            "params",
            "uniforms",
        ] {
            if mv.get(key) != jv.get(key) {
                if let (Some(Value::Object(a)), Some(Value::Object(b))) = (mv.get(key), jv.get(key))
                {
                    for (k, v) in b {
                        if a.get(k) != Some(v) {
                            problems.push(format!(
                                "material {key}.{k}: {} vs {}",
                                a.get(k).map_or("missing".into(), |x| x.to_string()),
                                v
                            ));
                        }
                    }
                    for k in a.keys().filter(|k| !b.contains_key(*k)) {
                        problems.push(format!("material {key}.{k}: not in the JS"));
                    }
                } else {
                    problems.push(format!(
                        "material {key}: {:?} vs {:?}",
                        mv.get(key),
                        jv.get(key)
                    ));
                }
            }
        }
    }
    for (rn, jn) in rs.nodes[rs
        .nodes
        .iter()
        .position(|n| n.name == "terrain")
        .expect("group")]
    .children
    .iter()
    .zip(&js.nodes[js_group as usize].children)
    {
        let (a, b) = (&rs.nodes[*rn as usize], &js.nodes[*jn as usize]);
        if (
            a.ty,
            &a.matrix,
            a.matrix_auto_update,
            a.receive_shadow,
            a.cast_shadow,
            &a.name,
        ) != (
            b.ty,
            &b.matrix,
            b.matrix_auto_update,
            b.receive_shadow,
            b.cast_shadow,
            &b.name,
        ) {
            problems.push(format!("node {rn}: flags differ from the JS node {jn}"));
            break;
        }
    }
    println!(
        "{id}: {same} of {} terrain meshes identical to the JS ({vertices} vertices)",
        js_meshes.len()
    );
    assert!(
        problems.is_empty(),
        "{id}: {} problem(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

#[test]
fn terrain_mesh_sierra() {
    check("sierra");
}

#[test]
fn terrain_mesh_coast() {
    check("coast");
}

#[test]
fn terrain_mesh_streets() {
    check("streets");
}

#[test]
fn terrain_mesh_desert() {
    check("desert");
}

#[test]
fn terrain_mesh_seaside() {
    check("seaside");
}

#[test]
fn terrain_mesh_cruise() {
    check("cruise");
}
