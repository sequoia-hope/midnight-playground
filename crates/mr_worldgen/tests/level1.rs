//! WP 3.9: Level 1 (Sierra) as one scene. Every module is ported (terrain,
//! road, sky, Mountain, Valley, City), so the whole build is held to the
//! browser's scene export, not group by group: the scene's counts and its
//! material kinds equal the world golden's (`parity/golden/world/
//! sierra.json`, WP 0.5) always, in CI and in wasm; with the cache, the
//! whole digest (`parity/cache/<key>/scenes/sierra.digest.json`: every
//! mesh's counts, bounds, attribute hashes, area and centroid; every
//! drawable's kinds, textures, instances and world bounds; every material's
//! kind and textures; every texture's pixels) equals ours, but for the
//! pixels of canvas textures, which WP 3.2's threshold gate holds per group
//! (DECISIONS D472).
//!
//! The scene is taken as the export took it: built, the sky updated at the
//! start of the route around the export's focus, the updaters run once
//! (dt 0, the export's camera) and their edits written into the scene.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_scene::digest::{SceneDigest, digest};
use mr_scene::{BufferData, Scene, TextureSource};
use mr_worldgen::scenery::scenery_factory;
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::world::{
    Build, CameraView, Change, SceneEdit, SceneRef, UpdateCtx, World, WorldBuild, level_jobs,
};
use serde_json::Value;

fn world_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/world/sierra.json"))
        .expect("world golden parses")
}

/// The export's night factor, camera and drawing buffer (`city-golden.mjs`).
fn city_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/city/sierra.json"))
        .expect("city golden parses")
}

fn build() -> WorldBuild {
    let setup = LevelSetup {
        terrain: common::terrain_setup("sierra"),
        road: None,
    };
    let b = Build::new(
        World::new(common::level("sierra")),
        level_jobs(
            level_stages(setup),
            // Every module of Sierra is ported: no recording, as the client
            // builds it.
            scenery_factory(None),
        ),
    );
    let mut b = b;
    while !b.is_done() {
        b.step().expect("Sierra builds");
    }
    // The sky as the exported frame left it: at the start of the route
    // (dt 0), its dome and lights around the fly camera's focus.
    let road: Value = serde_json::from_str(common::road_text("sierra")).expect("road golden");
    let f = &road["sky"]["focus"];
    let focus = [0, 1, 2].map(|i| f[i].as_f64().expect("focus"));
    {
        let World { sky, graph, .. } = &mut b.world;
        let sky = sky.as_mut().expect("a sky");
        let mut edits = Vec::new();
        sky.update(0.0, 0.0, Some(focus), &mut edits);
        sky.apply(graph, &edits);
    }
    let wb = b.finish();
    assert!(wb.log.is_empty(), "the build logged {:?}", wb.log);
    wb
}

/// Writes what the digest sees of an edit (attributes, instances) into
/// the scene's buffers.
fn f32s(scene: &mut Scene, acc: u32) -> &mut Vec<f32> {
    match &mut scene.buffers[acc as usize].data {
        BufferData::F32(v) => v,
        d => panic!("an edited buffer of {:?}", d.component()),
    }
}

fn write(scene: &mut Scene, e: SceneEdit) {
    match (e.target, e.change) {
        (SceneRef::Node(n), Change::InstanceMatrix { index, matrix }) => {
            let i = scene.nodes[n as usize].instances.expect("instanced");
            let acc = scene.instances[i as usize].matrices;
            f32s(scene, acc)[index as usize * 16..][..16].copy_from_slice(&matrix);
        }
        (SceneRef::Node(n), Change::InstanceColor { index, rgb }) => {
            let i = scene.nodes[n as usize].instances.expect("instanced");
            let acc = scene.instances[i as usize].colors.expect("colours");
            f32s(scene, acc)[index as usize * 3..][..3].copy_from_slice(&rgb);
        }
        (SceneRef::Node(n), Change::InstanceCount(c)) => {
            let i = scene.nodes[n as usize].instances.expect("instanced");
            scene.instances[i as usize].count = c;
        }
        (
            SceneRef::Mesh(g),
            Change::Attribute {
                name,
                offset,
                values,
            },
        ) => {
            let acc = scene.meshes[g as usize]
                .attribute(name)
                .expect("the attribute")
                .accessor;
            f32s(scene, acc)[offset..][..values.len()].copy_from_slice(&values);
        }
        // Transforms, visibility, materials and textures' offsets are not in
        // the digest (a node's world bounds come from its matrix, which an
        // updater at dt 0 leaves where the build put it).
        _ => {}
    }
}

/// Sierra as the export's frame had it.
fn exported_state() -> Scene {
    let g = city_golden();
    let mut wb = build();
    let night = g["night"].as_f64().expect("night");
    let cam = &g["camera"];
    let u = UpdateCtx {
        dt: 0.0,
        night,
        camera: Some(CameraView {
            position: [0, 1, 2].map(|i| cam["position"][i].as_f64().expect("camera")),
            fov: cam["fov"].as_f64().expect("fov"),
            viewport_height: g["city"]["viewportHeight"].as_f64().expect("viewport"),
        }),
        s: 0.0,
    };
    let edits = wb.update(&u);
    let mut scene = wb.scene;
    for e in edits {
        write(&mut scene, e);
    }
    scene
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_digest() -> Option<SceneDigest> {
    let root = common::root();
    let key = mr_scene::cache::js_tree_key(&root).ok()?;
    let p = root.join(format!("parity/cache/{key}/scenes/sierra.digest.json"));
    let text = std::fs::read_to_string(p).ok()?;
    Some(serde_json::from_str(&text).expect("the export's digest parses"))
}

#[cfg(target_arch = "wasm32")]
fn cached_digest() -> Option<SceneDigest> {
    None
}

#[test]
fn sierra_is_the_export() {
    let scene = exported_state();
    let ours = digest(&scene);
    let g = world_golden();
    let want = &g["scene"];

    // Counts (the file's byte count is how the exporter laid out its
    // buffers, not what the scene holds) and kinds, always.
    let counts = serde_json::to_value(&ours.counts).expect("json");
    let mut problems = Vec::new();
    for (k, v) in want["counts"].as_object().expect("counts") {
        if k != "binary_bytes" && counts[k] != *v {
            problems.push(format!("{k}: ours {}, the JS {v}", counts[k]));
        }
    }
    let kinds = serde_json::to_value(&ours.kinds).expect("json");
    if kinds != want["kinds"] {
        problems.push(format!("kinds: ours {kinds}, the JS {}", want["kinds"]));
    }
    println!("sierra: counts {counts}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The whole digest, with the cache.
    let Some(js) = cached_digest() else {
        println!("sierra: no cached export; counts and kinds only");
        return;
    };
    let mut canvas = 0;
    for (i, (a, b)) in js.textures.iter().zip(&ours.textures).enumerate() {
        let t = &scene.textures[i];
        if a != b {
            if t.source == TextureSource::Canvas && (a.width, a.height) == (b.width, b.height) {
                canvas += 1;
            } else {
                problems.push(format!("texture {i} ({}): {a:?}, ours {b:?}", t.name));
            }
        }
    }
    let mut js_rest = js.clone();
    let mut ours_rest = ours.clone();
    js_rest.textures.clear();
    ours_rest.textures.clear();
    js_rest.counts.binary_bytes = 0;
    ours_rest.counts.binary_bytes = 0;
    js_rest.name = None;
    ours_rest.name = None;
    problems.extend(mr_scene::digest::compare(&js_rest, &ours_rest));
    println!(
        "sierra: {} meshes, {} drawables, {} materials, {} textures ({canvas} canvas textures' pixels left to WP 3.2's threshold gate)",
        ours.meshes.len(),
        ours.drawables.len(),
        ours.materials.len(),
        ours.textures.len()
    );
    assert!(
        problems.is_empty(),
        "{} differences:\n{}",
        problems.len(),
        problems
            .iter()
            .take(20)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
