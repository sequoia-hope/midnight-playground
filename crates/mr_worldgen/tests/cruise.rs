//! WP 7.5: the Night City Cruise as one scene, as WP 3.9 holds Level 1
//! (`tests/level1.rs`, DECISIONS D472) and WP 7.1 Level 2 (D536). The loop's
//! only scenery module is City, ported in WP 3.8 with its loop variants
//! (D350), so the level builds from ported code alone
//! (`scenery_factory(None)`) and the whole build is held to the browser's
//! scene export: the scene's counts and its material kinds equal the world
//! golden's (`parity/golden/world/cruise.json`, WP 0.5) always, in CI and
//! in wasm; with the cache, the whole digest
//! (`parity/cache/<key>/scenes/cruise.digest.json`) equals ours, but for
//! the pixels of canvas textures, which WP 3.2's threshold gates hold
//! (City's in `tests/city.rs`, D354; the shared terrain and road pictures
//! in `tests/textures.rs`), and so do every node's visibility (what the
//! chunk cut-off hid) and the night parameters, which the digest leaves
//! out. The groups are held one by one in `tests/city.rs` (`city`),
//! `tests/road.rs` (the road and the sky) and `tests/terrain.rs`
//! (DECISIONS D630).
//!
//! The scene is taken as the export took it: built, the sky updated at the
//! start of the route around the export's focus, the updaters run once
//! (dt 0, the export's night factor and camera: the chunks the loop cuts
//! off beyond their distance are hidden as the export hid them) and their
//! edits applied to the objects, then assembled.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_scene::digest::{SceneDigest, digest};
use mr_scene::{BufferData, Scene, TextureSource};
use mr_worldgen::color::Color;
use mr_worldgen::object::SceneGraph;
use mr_worldgen::scenery::scenery_factory;
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::three_geom::{Quaternion, Vector3};
use mr_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs};
use serde_json::Value;

fn world_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/world/cruise.json"))
        .expect("world golden parses")
}

/// The export's night factor, camera and drawing buffer (`city-golden.mjs`).
fn city_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/city/cruise.json"))
        .expect("city golden parses")
}

/// The loop as the export's frame had it: built, the sky updated at the
/// start of the route around the export's focus, the updaters run once
/// (dt 0, the export's night factor and camera) and their edits applied to
/// the objects (the cut-off chunks' visibility and the ferris wheel's
/// rotor included), then assembled.
fn exported_state() -> Scene {
    let g = city_golden();
    let setup = LevelSetup {
        terrain: common::terrain_setup("cruise"),
        road: None,
    };
    let mut b = Build::new(
        World::new(common::level("cruise")),
        level_jobs(
            level_stages(setup),
            // City, the loop's only module, is ported: no recording, as the
            // client builds it.
            scenery_factory(None),
        ),
    );
    while !b.is_done() {
        b.step().expect("the cruise loop builds");
    }
    // The sky as the exported frame left it: at the start of the route
    // (dt 0), its dome and lights around the fly camera's focus.
    let road: Value = serde_json::from_str(common::road_text("cruise")).expect("road golden");
    let f = &road["sky"]["focus"];
    let focus = [0, 1, 2].map(|i| f[i].as_f64().expect("focus"));
    let w = &mut b.world;
    {
        let World { sky, graph, .. } = w;
        let sky = sky.as_mut().expect("a sky");
        let mut edits = Vec::new();
        sky.update(0.0, 0.0, Some(focus), &mut edits);
        sky.apply(graph, &edits);
    }
    assert!(w.graph.log.is_empty(), "the build logged {:?}", w.graph.log);
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
    let mut edits = Vec::new();
    for a in &mut w.animators {
        a.update(&u, &mut edits);
    }
    for e in edits {
        apply(&mut w.graph, e);
    }
    w.graph.finish().0
}

/// Writes what the digest sees of an edit (transforms, instances,
/// attributes) into the objects; materials and textures are not in it.
fn apply(graph: &mut SceneGraph, e: Edit) {
    match (e.target, e.change) {
        (Handle::Node(n), Change::InstanceMatrix { index, matrix }) => {
            let inst = graph.get_mut(n).instances.as_mut().expect("instanced");
            inst.matrices[index as usize * 16..][..16].copy_from_slice(&matrix);
        }
        (Handle::Node(n), Change::InstanceColor { index, rgb }) => {
            let inst = graph.get_mut(n).instances.as_mut().expect("instanced");
            inst.set_color_at(
                index as usize,
                Color::new(f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])),
            );
        }
        (
            Handle::Node(n),
            Change::Transform {
                position,
                quaternion,
                scale,
            },
        ) => {
            let o = graph.get_mut(n);
            o.position = Vector3::new(position[0], position[1], position[2]);
            o.quaternion = Quaternion {
                x: quaternion[0],
                y: quaternion[1],
                z: quaternion[2],
                w: quaternion[3],
            };
            o.scale = Vector3::new(scale[0], scale[1], scale[2]);
        }
        (Handle::Node(n), Change::Visible(v)) => graph.get_mut(n).visible = v,
        (
            Handle::Geometry(g),
            Change::Attribute {
                name,
                offset,
                values,
            },
        ) => {
            let a = graph
                .geometry_mut(g)
                .get_attribute_mut(name)
                .expect("the attribute");
            match &mut a.array {
                BufferData::F32(v) => v[offset..][..values.len()].copy_from_slice(&values),
                d => panic!("an edited buffer of {:?}", d.component()),
            }
        }
        _ => {}
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_digest() -> Option<SceneDigest> {
    let root = common::root();
    let key = mr_scene::cache::js_tree_key(&root).ok()?;
    let p = root.join(format!("parity/cache/{key}/scenes/cruise.digest.json"));
    let text = std::fs::read_to_string(p).ok()?;
    Some(serde_json::from_str(&text).expect("the export's digest parses"))
}

#[cfg(target_arch = "wasm32")]
fn cached_digest() -> Option<SceneDigest> {
    None
}

/// The cached export itself.
#[cfg(not(target_arch = "wasm32"))]
fn cached_scene() -> Option<Scene> {
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    mr_scene::read_file(dir.join("cruise.mrscene")).ok()
}

#[cfg(target_arch = "wasm32")]
fn cached_scene() -> Option<Scene> {
    None
}

#[test]
fn cruise_is_the_export() {
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
    println!("cruise: counts {counts}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The whole digest, with the cache.
    let Some(js) = cached_digest() else {
        println!("cruise: no cached export; counts and kinds only");
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

    // What the digest leaves out and the loop decides: which nodes the
    // chunk cut-off hid at the export's camera, and the night parameters
    // (which material, which property, its day and night values, in the
    // order registered).
    if let Some(js) = cached_scene() {
        let vis = |s: &Scene| -> Vec<String> {
            s.nodes
                .iter()
                .map(|n| format!("{:?} {} {}", n.ty, n.name, n.visible))
                .collect()
        };
        let (a, b) = (vis(&js), vis(&scene));
        let hidden = b.iter().filter(|l| l.ends_with(" false")).count();
        println!("cruise: {hidden} nodes hidden at the export's camera");
        if a != b {
            let k = a.iter().zip(&b).position(|(x, y)| x != y);
            problems.push(format!(
                "nodes and their visibility differ ({} and {}), first at {k:?}",
                a.len(),
                b.len()
            ));
        }
        let np = |s: &Scene| -> Vec<String> {
            s.night_params
                .iter()
                .map(|p| format!("{} {} {:?} {:?}", p.material, p.prop, p.day, p.night))
                .collect()
        };
        if np(&js) != np(&scene) {
            problems.push(format!(
                "the night parameters: ours {:?}, the JS {:?}",
                np(&scene),
                np(&js)
            ));
        }
    }
    println!(
        "cruise: {} meshes, {} drawables, {} materials, {} textures ({canvas} canvas textures' pixels left to WP 3.2's threshold gate)",
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
