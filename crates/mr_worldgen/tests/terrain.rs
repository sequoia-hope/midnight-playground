//! WP 3.4's L2 gate: `Terrain.heightAt` against the JS (SPEC 5.4, 5.7;
//! DECISIONS D28). The world golden (`parity/golden/world/<level>.json`)
//! holds the SHA-256 of the 10,000 dumped points and of their heights; the
//! points are regenerated here as `tools/parity/lib/scene-page.js`
//! `terrainDump` makes them (6,000 along the road, 4,000 on a Halton
//! sequence over the terrain's bounds) and both hashes must match, so the
//! heights are bit-identical. When the cache holds the browser's dump
//! (`parity/cache/<key>/world/<level>/terrain.bin`), every point is also
//! compared, and the first difference is reported with its position.
//!
//! The terrain is built as `World.build` builds it, with the scenery's
//! plan replayed from `parity/golden/terrain/<level>.json`
//! (`tools/parity/terrain-plan.mjs`, DECISIONS D232); the flattens the JS
//! resolved itself are checked too.
//!
//! Also the terrain checks deferred from M1 (DECISIONS D56):
//! `test/unit/track.test.js` "the terrain never covers the road" for every
//! level and `test/unit/seaside.test.js` "inside the barriers the terrain is
//! the run-off the car drives on", with the same assertions.
//!
//! The goldens are compiled in, so the gate runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::{plan, plan_json, terrain};
use mr_track::Track;
use mr_worldgen::terrain::Terrain;
use sha2::{Digest, Sha256};

fn world_golden(id: &str) -> serde_json::Value {
    let text = match id {
        "sierra" => include_str!("../../../parity/golden/world/sierra.json"),
        "coast" => include_str!("../../../parity/golden/world/coast.json"),
        "streets" => include_str!("../../../parity/golden/world/streets.json"),
        "desert" => include_str!("../../../parity/golden/world/desert.json"),
        "seaside" => include_str!("../../../parity/golden/world/seaside.json"),
        "cruise" => include_str!("../../../parity/golden/world/cruise.json"),
        _ => panic!("no golden for {id}"),
    };
    serde_json::from_str(text).expect("world golden parses")
}

fn sha(values: &[f64]) -> String {
    let mut h = Sha256::new();
    for v in values {
        h.update(v.to_le_bytes());
    }
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// `terrainDump`'s points and the heights there.
fn dump(t: &Track, tr: &Terrain, total: usize, along: usize) -> (Vec<f64>, Vec<f64>) {
    let halton = |mut i: u64, b: u64| {
        let (mut f, mut r) = (1.0, 0.0);
        while i > 0 {
            f /= b as f64;
            r += f * (i % b) as f64;
            i /= b;
        }
        r
    };
    let mut xz = vec![0.0; total * 2];
    let mut h = vec![0.0; total];
    for i in 0..total {
        let (x, z);
        if i < along {
            let s = ((i as f64 + 0.5) / along as f64) * t.length;
            let g = (i as f64 * 0.6180339887498949) % 1.0;
            let p = t.point_at(s, -60.0 + 120.0 * g);
            x = p.x;
            z = p.z;
        } else {
            let k = (i - along + 1) as u64;
            x = tr.min_x + (tr.max_x - tr.min_x) * halton(k, 2);
            z = tr.min_z + (tr.max_z - tr.min_z) * halton(k, 3);
        }
        xz[i * 2] = x;
        xz[i * 2 + 1] = z;
        h[i] = tr.height_at(x, z);
    }
    (xz, h)
}

/// The browser's dump, when the cache has it for the current JS tree.
#[cfg(not(target_arch = "wasm32"))]
fn cached(id: &str) -> Option<(Vec<f64>, Vec<f64>)> {
    let root = common::root();
    let key = mr_scene::cache::js_tree_key(&root).ok()?;
    let bytes =
        std::fs::read(root.join(format!("parity/cache/{key}/world/{id}/terrain.bin"))).ok()?;
    let f: Vec<f64> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| f64::from_le_bytes(*c))
        .collect();
    Some((f[..20000].to_vec(), f[20000..].to_vec()))
}

#[cfg(target_arch = "wasm32")]
fn cached(_id: &str) -> Option<(Vec<f64>, Vec<f64>)> {
    None
}

fn heights(id: &str) {
    let p = plan(id);
    let (_, t, tr) = terrain(id, Some(&p));
    // The flattens the JS resolved (a height of null: the landform there).
    let pj = plan_json(id);
    for (k, f) in tr.flattens.iter().enumerate() {
        let want = pj["flattens"][k].get("yResolved").map(common::hex);
        assert_eq!(
            f.y_resolved.map(f64::to_bits),
            want.map(f64::to_bits),
            "{id}: flatten {k} resolved to {:?}, the JS to {want:?}",
            f.y_resolved
        );
    }
    let g = world_golden(id);
    let gt = &g["terrain"];
    let (total, along) = (
        gt["count"].as_u64().expect("count") as usize,
        gt["along"].as_u64().expect("along") as usize,
    );
    let bounds: Vec<f64> = gt["bounds"]
        .as_array()
        .expect("bounds")
        .iter()
        .map(|v| v.as_f64().expect("number"))
        .collect();
    assert_eq!(
        bounds,
        vec![tr.min_x, tr.min_z, tr.max_x, tr.max_z],
        "{id}: terrain bounds"
    );
    let (xz, h) = dump(&t, &tr, total, along);
    let cache = cached(id);
    if cache.is_none() {
        println!("{id}: no cached dump; checking the golden's hashes only");
    }
    if let Some((jxz, jh)) = cache {
        let first = (0..total).find(|&i| {
            xz[i * 2].to_bits() != jxz[i * 2].to_bits()
                || xz[i * 2 + 1].to_bits() != jxz[i * 2 + 1].to_bits()
        });
        assert!(
            first.is_none(),
            "{id}: point {} differs from the dump: ({}, {}) vs ({}, {})",
            first.unwrap_or(0),
            xz[first.unwrap_or(0) * 2],
            xz[first.unwrap_or(0) * 2 + 1],
            jxz[first.unwrap_or(0) * 2],
            jxz[first.unwrap_or(0) * 2 + 1]
        );
        let diffs: Vec<usize> = (0..total)
            .filter(|&i| h[i].to_bits() != jh[i].to_bits())
            .collect();
        let worst = (0..total).fold(0.0f64, |m, i| m.max((h[i] - jh[i]).abs()));
        if let Some(&i) = diffs.first() {
            panic!(
                "{id}: {} of {total} heights differ from the browser's (worst {worst:e} m); first at point {i} ({}, {}): {} vs {}",
                diffs.len(),
                xz[i * 2],
                xz[i * 2 + 1],
                h[i],
                jh[i]
            );
        }
        println!("{id}: every height equals the browser's dump");
    }
    let arrays = &gt["arrays"];
    assert_eq!(
        sha(&xz),
        arrays["xz"]["sha256"].as_str().expect("sha"),
        "{id}: the dump's points"
    );
    assert_eq!(
        sha(&h),
        arrays["height"]["sha256"].as_str().expect("sha"),
        "{id}: the heights at the dump's points"
    );
}

#[test]
fn heights_sierra() {
    heights("sierra");
}

#[test]
fn heights_coast() {
    heights("coast");
}

#[test]
fn heights_streets() {
    heights("streets");
}

#[test]
fn heights_desert() {
    heights("desert");
}

#[test]
fn heights_seaside() {
    heights("seaside");
}

#[test]
fn heights_cruise() {
    heights("cruise");
}

/// `test/unit/track.test.js`: "<level>: the terrain never covers the road"
/// (the terrain alone, without scenery, as the JS test builds it).
fn never_covers_the_road(id: &str) {
    let (_, t, tr) = terrain(id, None);
    let mut bad = Vec::new();
    let mut count = 0;
    let mut s = 0.0;
    while s < t.length {
        let f = t.frame(s);
        if !tr.is_elevated(&t, t.idx(s)) {
            for u in [-1.0, -0.66, -0.33, 0.0, 0.33, 0.66, 1.0] {
                let lat = u * f.hw;
                let g = tr.height_at(f.x + f.rx * lat, f.z + f.rz * lat);
                let road = f.y - lat * f.bank;
                if g > road + 0.03 {
                    count += 1;
                    if bad.len() < 5 {
                        bad.push(format!("s={s} lat={lat:.1} ground {:.2} m above", g - road));
                    }
                }
            }
        }
        s += 3.0;
    }
    assert!(
        bad.is_empty(),
        "{id}: {count} samples where the ground is above the road: {bad:?}"
    );
}

#[test]
fn sierra_terrain_never_covers_the_road() {
    never_covers_the_road("sierra");
}

#[test]
fn coast_terrain_never_covers_the_road() {
    never_covers_the_road("coast");
}

#[test]
fn streets_terrain_never_covers_the_road() {
    never_covers_the_road("streets");
}

#[test]
fn desert_terrain_never_covers_the_road() {
    never_covers_the_road("desert");
}

#[test]
fn seaside_terrain_never_covers_the_road() {
    never_covers_the_road("seaside");
}

#[test]
fn cruise_terrain_never_covers_the_road() {
    never_covers_the_road("cruise");
}

/// `test/unit/seaside.test.js`: "inside the barriers the terrain is the
/// run-off the car drives on".
#[test]
fn inside_the_barriers_the_terrain_is_the_run_off() {
    let (_, t, tr) = terrain("seaside", None);
    let (mut worst, mut at) = (0.0f64, String::new());
    let mut s = 0;
    while s < t.n {
        let f = t.frame(s as f64);
        for side in [-1.0, 1.0] {
            let w = if side < 0.0 { f.wall_l } else { f.wall_r };
            let mut lat = f.hw + 3.0;
            while lat < (w - 2.0).min(f.hw + 12.0) {
                let p = t.point_at(s as f64, side * lat);
                // What the car drives on there: physics projects onto the nearest
                // bit of centreline (inside a tight corner that can be another
                // part of it).
                let q = t.project(p.x, p.z, s as f64);
                let d = t.surface_y(q.s, q.lat) - tr.height_at(p.x, p.z);
                if d.abs() > worst.abs() {
                    worst = d;
                    at = format!(
                        "s {s} lat {} (projects to s {:.0} lat {:.1})",
                        side * lat,
                        q.s,
                        q.lat
                    );
                }
                lat += 3.0;
            }
        }
        s += 13;
    }
    // Within half a metre everywhere (a car would visibly float or sink).
    assert!(
        worst.abs() < 0.5,
        "terrain vs run-off surface: {worst:.2} m at {at}"
    );
}

// ── L3 through the job list ─────────────────────────────────────────────

/// A level built through `level_jobs` with the terrain's stages (the
/// scenery modules missing, their plan replayed), and the progress it
/// reported.
fn level_build(id: &str) -> (Vec<(String, f64)>, mr_worldgen::world::WorldBuild) {
    use mr_worldgen::terrain_mesh::{TerrainSetup, seaside_ground_color, terrain_stages};
    use mr_worldgen::world::{Build, Stages, World, level_jobs};
    let setup = TerrainSetup {
        plan: Some(plan(id)),
        ground_color: (id == "seaside").then(|| seaside_ground_color(common::survey())),
        ..TerrainSetup::default()
    };
    let (terrain, fields, terrain_meshes) = terrain_stages(setup);
    let stages = Stages {
        terrain: Some(terrain),
        fields: Some(fields),
        terrain_meshes: Some(terrain_meshes),
        ..Stages::default()
    };
    let build = Build::new(World::new(common::level(id)), level_jobs(stages, |_| None));
    let mut progress = Vec::new();
    let wb = build
        .run(|label, f| progress.push((label.to_string(), f)))
        .expect("the level builds");
    (progress, wb)
}

/// The line per terrain mesh that `tools/parity/terrain-plan.mjs` hashes.
fn mesh_lines(scene: &mr_scene::Scene) -> (u64, u64, String) {
    let d = mr_scene::digest::digest(scene);
    let g = scene
        .nodes
        .iter()
        .find(|n| n.name == "terrain")
        .expect("a terrain group");
    let mut lines = Vec::new();
    let mut vertices = 0;
    for &c in &g.children {
        let m = &d.meshes[scene.nodes[c as usize].mesh.expect("a mesh") as usize];
        vertices += m.vertices;
        let attrs: Vec<String> = m
            .attributes
            .iter()
            .map(|(k, v)| format!("{k}={}", v.as_str().expect("sha")))
            .collect();
        lines.push(format!(
            "{} {} {} {}",
            m.vertices,
            m.indices,
            attrs.join(","),
            m.index.as_deref().unwrap_or("null")
        ));
    }
    let mut h = Sha256::new();
    h.update(lines.join("\n").as_bytes());
    let sha = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    (g.children.len() as u64, vertices, sha)
}

fn meshes(id: &str) {
    let (progress, wb) = level_build(id);
    // "Sculpting terrain" at 0.1, then after every 24 tiles 0.1 + f × 0.55,
    // before "Paving roads" (World.build's labels).
    let sculpt: Vec<f64> = progress
        .iter()
        .filter(|(l, _)| l == "Sculpting terrain")
        .map(|&(_, f)| f)
        .collect();
    assert_eq!(sculpt[0], 0.1, "{id}: {progress:?}");
    assert!(
        sculpt.windows(2).all(|w| w[0] <= w[1]) && *sculpt.last().expect("one") <= 0.65,
        "{id}: {sculpt:?}"
    );
    let at = |l: &str| progress.iter().position(|(x, _)| x == l);
    assert!(
        at("Sculpting terrain") < at("Paving roads"),
        "{id}: {progress:?}"
    );
    // The terrain group is the root's first child, as world.root.add puts it.
    let s = &wb.scene;
    let root = &s.nodes[s.roots[0] as usize];
    assert_eq!(root.name, format!("world:{id}"));
    assert_eq!(s.nodes[root.children[0] as usize].name, "terrain");
    let (count, vertices, sha) = mesh_lines(s);
    let want = &plan_json(id)["meshes"];
    if want.is_null() {
        println!("{id}: no terrain mesh digest recorded ({count} meshes, {vertices} vertices)");
        return;
    }
    assert_eq!(
        (count, vertices),
        (
            want["count"].as_u64().expect("count"),
            want["vertices"].as_u64().expect("vertices")
        ),
        "{id}: terrain meshes and vertices"
    );
    assert_eq!(
        sha,
        want["sha256"].as_str().expect("sha"),
        "{id}: the terrain meshes differ from the JS export's (tests/terrain_mesh.rs localises it with the cache)"
    );
}

#[test]
fn meshes_sierra() {
    meshes("sierra");
}

#[test]
fn meshes_coast() {
    meshes("coast");
}

#[test]
fn meshes_streets() {
    meshes("streets");
}

#[test]
fn meshes_desert() {
    meshes("desert");
}

#[test]
fn meshes_seaside() {
    meshes("seaside");
}

#[test]
fn meshes_cruise() {
    meshes("cruise");
}
