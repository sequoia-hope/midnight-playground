//! WP 3.6's L3 gate: the Sierra Pass scenery (`Mountain.js`, zone 0 of
//! Sierra) against the JS scene export (SPEC 5.7; DECISIONS D26, D312).
//! Sierra is built through `level_jobs` with every ported stage and the
//! scenery factory (Mountain's own `plan()` and `build()`; the other
//! modules' plans replayed, their builds skipped), the night parameters
//! applied at the export's night factor and the updaters run once as the
//! frozen export ran them (dt 0, the camera where the export's was). The
//! group `mountain` is then compared with `parity/golden/mountain/
//! sierra.json`, which `tools/parity/mountain-scene.mjs` writes from the
//! cached export:
//!
//! - per child, in order: its node (type, matrix, flags, render order, a
//!   sprite's centre), its mesh (vertex and index counts, the SHA-256 of
//!   every attribute and of the index), its instances (count, SHA-256 of
//!   the matrices and colours, the bounding sphere) and its material,
//!   parameter by parameter with its uniforms and textures' samplers;
//! - every canvas texture against WP 3.2's threshold: the 8×8 block means
//!   from the golden always, the mean absolute difference per channel
//!   against the export's pixels with the cache (side-by-side sheets in
//!   `parity/report/mountain/`).
//!
//! The parked pickup and sedan are CarModel's (WP 4.1): the export's two
//! baked car groups are listed and skipped (D311).
//!
//! The golden is compiled in, so the gate runs in CI and in wasm.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_canvas::compare::{block_diff, block_means};
use mr_scene::digest::{SceneDigest, digest};
use mr_scene::{BufferData, Scene, TextureSource};
use mr_worldgen::material::{Param, num};
use mr_worldgen::object::{MaterialId, SceneGraph};
use mr_worldgen::scenery::{PlanOnly, scenery_factory};
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::three_geom::{Quaternion, Vector3};
use mr_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs};
use serde_json::{Value, json};

const GOLDEN: &str = include_str!("../../../parity/golden/mountain/sierra.json");
const TEXTURES: &str = include_str!("../../../parity/golden/mountain/textures.json");

/// WP 3.2's gate: mean absolute difference under 3 levels per channel.
const LIMIT: f64 = 3.0;

/// Every number as an f64 (1 and 1.0 are the same number in the JS).
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), norm(x))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Number(n) => Value::from(n.as_f64().expect("a number")),
        _ => v.clone(),
    }
}

/// Where two descriptions differ, key by key.
fn diff(what: &str, ours: &Value, js: &Value, out: &mut Vec<String>) {
    let (a, b) = (norm(ours), norm(js));
    if a == b {
        return;
    }
    match (&a, &b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in y {
                match x.get(k) {
                    Some(w) => diff(&format!("{what}.{k}"), w, v, out),
                    None => out.push(format!("{what}.{k}: missing, the JS has {v}")),
                }
            }
            for k in x.keys().filter(|k| !y.contains_key(*k)) {
                out.push(format!("{what}.{k}: not in the JS"));
            }
        }
        _ => out.push(format!("{what}: {a} vs the JS {b}")),
    }
}

/// A material with each texture reference replaced by what the texture is
/// (as the tool writes it).
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
                    let sha = if tex.source == TextureSource::Canvas {
                        Value::Null
                    } else {
                        Value::from(td.sha256.clone())
                    };
                    let item_size = s.buffers[tex.pixels as usize].item_size;
                    return json!({ "texture": desc, "sha256": sha, "channels": td.channels, "item_size": item_size });
                }
                Value::Object(o.iter().map(|(k, x)| (k.clone(), walk(x, s, d))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(|x| walk(x, s, d)).collect()),
            _ => v.clone(),
        }
    }
    norm(&walk(
        &serde_json::to_value(&s.materials[m as usize]).expect("json"),
        s,
        d,
    ))
}

/// The golden's material table, each entry written in full.
fn table(g: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for e in g["table"].as_array().expect("table") {
        let Some(k) = e.get("like").and_then(Value::as_u64) else {
            out.push(e.clone());
            continue;
        };
        let mut m = out[k as usize].clone();
        for (key, v) in e.as_object().expect("entry") {
            match key.as_str() {
                "like" => {}
                "params" => {
                    for (pk, pv) in v.as_object().expect("params") {
                        m["params"][pk] = pv.clone();
                    }
                }
                _ => m[key] = v.clone(),
            }
        }
        out.push(m);
    }
    out
}

fn node_view(s: &Scene, n: u32) -> Value {
    let n = &s.nodes[n as usize];
    norm(&json!({
        "name": n.name, "type": n.ty, "matrix": n.matrix.to_vec(), "visible": n.visible,
        "matrix_auto_update": n.matrix_auto_update, "frustum_culled": n.frustum_culled,
        "render_order": n.render_order, "cast_shadow": n.cast_shadow,
        "receive_shadow": n.receive_shadow, "layers": n.layers, "center": n.center,
    }))
}

fn mesh_line(d: &SceneDigest, mesh: u32) -> String {
    let m = &d.meshes[mesh as usize];
    let attrs: Vec<String> = m
        .attributes
        .iter()
        .map(|(k, v)| format!("{k}={}", v.as_str().expect("sha")))
        .collect();
    format!(
        "{} {} {} {}",
        m.vertices,
        m.indices,
        attrs.join(","),
        m.index.as_deref().unwrap_or("null")
    )
}

/// Sets a uniform if the material has it, else the parameter.
fn set_number(graph: &mut SceneGraph, m: MaterialId, prop: &str, v: f64) {
    let mat = graph.material_mut(m);
    if let Some(u) = &mut mat.desc.uniforms
        && let Some(slot) = u.get_mut(prop)
    {
        *slot = num(v);
        return;
    }
    mat.set_value(prop, Param::Num(v));
}

/// An updater's edits applied to the graph.
fn apply(graph: &mut SceneGraph, edits: Vec<Edit>) {
    for e in edits {
        match (e.target, e.change) {
            (Handle::Material(m), Change::Number { prop, value }) => {
                set_number(graph, m, prop, value)
            }
            (Handle::Texture(t), Change::TextureOffset(o)) => graph.texture_mut(t).desc.offset = o,
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
                o.quaternion =
                    Quaternion::new(quaternion[0], quaternion[1], quaternion[2], quaternion[3]);
                o.scale = Vector3::new(scale[0], scale[1], scale[2]);
            }
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
                    .expect("attribute");
                for (k, v) in values.into_iter().enumerate() {
                    a.set_raw(offset + k, v as f64);
                }
            }
            (t, c) => panic!("an edit the test does not apply: {t:?} {c:?}"),
        }
    }
}

/// Sierra with Mountain built, as the export had it.
fn build(g: &Value) -> (Scene, Vec<String>) {
    let setup = LevelSetup {
        terrain: common::terrain_setup("sierra"),
        road: None,
    };
    let factory = scenery_factory(Some(common::recording("sierra")));
    let mut b = Build::new(
        World::new(common::level("sierra")),
        level_jobs(level_stages(setup), move |info| {
            let s = factory(info)?;
            Some(if info.name == "Mountain" {
                s
            } else {
                Box::new(PlanOnly(s))
            })
        }),
    );
    let mut labels = Vec::new();
    while let Some((label, _)) = b.progress() {
        labels.push(label.to_string());
        b.step().expect("the level builds");
    }
    assert!(
        labels.iter().any(|l| l == "Mountain scenery"),
        "the job list shows Mountain's label: {labels:?}"
    );
    let w = &mut b.world;
    let night = g["night"].as_f64().expect("night");
    for n in w.graph.night.clone() {
        set_number(
            &mut w.graph,
            n.material,
            &n.prop,
            n.day + (n.night - n.day) * night,
        );
    }
    let c = &g["camera"];
    let u = UpdateCtx {
        dt: 0.0,
        night,
        camera: Some(CameraView {
            position: [0, 1, 2].map(|i| c[i].as_f64().expect("camera")),
            fov: 62.0,
            viewport_height: 1080.0,
        }),
        s: 0.0,
    };
    let mut animators = std::mem::take(&mut w.animators);
    for a in &mut animators {
        let mut edits = Vec::new();
        a.update(&u, &mut edits);
        apply(&mut w.graph, edits);
    }
    let log = w.graph.log.clone();
    let (scene, _) = w.graph.finish();
    (scene, log)
}

/// The canvas textures a list of materials uses, in order of first use,
/// once per picture.
fn canvas_textures(s: &Scene, mats: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for &m in mats {
        let v = serde_json::to_value(&s.materials[m as usize]).expect("json");
        let mut refs = Vec::new();
        fn walk(v: &Value, refs: &mut Vec<u32>) {
            match v {
                Value::Object(o) => {
                    if o.len() == 1
                        && let Some(t) = o.get("texture").and_then(Value::as_u64)
                    {
                        refs.push(t as u32);
                        return;
                    }
                    o.values().for_each(|x| walk(x, refs));
                }
                Value::Array(a) => a.iter().for_each(|x| walk(x, refs)),
                _ => {}
            }
        }
        walk(&v["params"], &mut refs);
        walk(&v["uniforms"], &mut refs);
        for t in refs {
            let tex = &s.textures[t as usize];
            if tex.source == TextureSource::Canvas
                && !out
                    .iter()
                    .any(|&o| s.textures[o as usize].pixels == tex.pixels)
            {
                out.push(t);
            }
        }
    }
    out
}

fn blocks_of(w: &Value) -> Vec<[f64; 4]> {
    w["blocks"]
        .as_array()
        .expect("blocks")
        .iter()
        .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().expect("mean")))
        .collect()
}

/// The canvas textures a material uses as both `map` and `emissiveMap`,
/// in order of first use, once per picture.
fn lettered_textures(s: &Scene, mats: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for &m in mats {
        let p = &s.materials[m as usize].params;
        let tex = |k: &str| {
            p.get(k)
                .and_then(|v| v.get("texture"))
                .and_then(Value::as_u64)
        };
        let (Some(a), Some(b)) = (tex("map"), tex("emissiveMap")) else {
            continue;
        };
        let t = &s.textures[a as usize];
        if a == b
            && t.source == TextureSource::Canvas
            && !out
                .iter()
                .any(|&o| s.textures[o as usize].pixels == t.pixels)
        {
            out.push(a as u32);
        }
    }
    out
}

/// The font-matched capture's RGBA, when the cache has it and it is the
/// one the golden describes.
#[cfg(not(target_arch = "wasm32"))]
fn cached_rgba(k: usize, sha: &str) -> Option<Vec<u8>> {
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let p = dir.parent()?.join(format!("mountain/canvas-{k}.rgba"));
    let b = std::fs::read(p).ok()?;
    (mr_scene::digest::sha256_hex(&b) == sha).then_some(b)
}

#[cfg(target_arch = "wasm32")]
fn cached_rgba(_k: usize, _sha: &str) -> Option<Vec<u8>> {
    None
}

fn rgba(s: &Scene, t: u32) -> Vec<u8> {
    match &s.buffers[s.textures[t as usize].pixels as usize].data {
        BufferData::U8(v) => v.clone(),
        other => panic!("pixels as {other:?}"),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn cached_export() -> Option<Scene> {
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let p = dir.join("sierra.mrscene");
    p.exists()
        .then(|| mr_scene::read_file(&p).expect("the export reads"))
}

#[cfg(target_arch = "wasm32")]
fn cached_export() -> Option<Scene> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn write_sheet(name: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    let dir = common::root().join("parity/report/mountain");
    std::fs::create_dir_all(&dir).expect("report dir");
    let (sw, sh, px) = mr_canvas::compare::sheet(w, h, js, rust);
    let f = std::fs::File::create(dir.join(format!("{name}.png"))).expect("png");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(&px)
        .expect("png data");
}

#[cfg(target_arch = "wasm32")]
fn write_sheet(_name: &str, _w: usize, _h: usize, _js: &[u8], _rust: &[u8]) {}

#[test]
fn mountain_matches_the_js_export() {
    let g: Value = serde_json::from_str(GOLDEN).expect("golden parses");
    let (s, log) = build(&g);
    assert!(log.is_empty(), "the build logged {log:?}");
    let d = digest(&s);
    let mut problems = Vec::new();

    let root = &s.nodes[s.roots[0] as usize];
    let (k, &gi) = root
        .children
        .iter()
        .enumerate()
        .find(|&(_, &c)| s.nodes[c as usize].name == "mountain")
        .expect("a group mountain");
    assert_eq!(
        k as u64,
        g["group"]["child"].as_u64().expect("child"),
        "the group's place among the root's children"
    );
    diff(
        "group",
        &node_view(&s, gi),
        &g["group"]["node"],
        &mut problems,
    );

    let want = g["children"].as_array().expect("children");
    let ours = &s.nodes[gi as usize].children;
    let jt = table(&g);
    let mut pairs: Vec<(usize, u32)> = Vec::new();
    let mut skipped = Vec::new();
    let mut it = ours.iter();
    let mut matched = 0;
    for (j, w) in want.iter().enumerate() {
        // The parked cars (CarModel, WP 4.1): baked groups the port leaves
        // out until it has the car models.
        if w["node"]["type"] == "Group" && w.get("children").is_some() {
            skipped.push(j);
            continue;
        }
        let Some(&c) = it.next() else {
            problems.push(format!("child {j}: missing here"));
            continue;
        };
        let what = format!("child {j}");
        let before = problems.len();
        diff(&what, &node_view(&s, c), &w["node"], &mut problems);
        let n = &s.nodes[c as usize];
        if let Some(mesh) = n.mesh {
            if Some(mesh_line(&d, mesh).as_str()) != w["mesh"].as_str() {
                problems.push(format!(
                    "{what}: mesh {} vs the JS {}",
                    mesh_line(&d, mesh),
                    w["mesh"]
                ));
            }
            let dr = d.drawables.iter().find(|x| x.node == c).expect("drawable");
            if let Some(wi) = w.get("instances") {
                let bs = n
                    .instances
                    .and_then(|i| s.instances[i as usize].bounding_sphere.clone());
                let ours = json!({
                    "count": dr.instances, "matrices": dr.instance_matrices,
                    "colors": dr.instance_colors, "bounding_sphere": bs,
                });
                diff(&format!("{what} instances"), &ours, wi, &mut problems);
            } else if dr.instances.is_some() {
                problems.push(format!("{what}: instanced here, not in the JS"));
            }
            let jm = w["material"].as_u64().expect("material") as usize;
            match pairs.iter().find(|(x, _)| *x == jm) {
                Some(&(_, m)) if m != n.materials[0] => problems.push(format!(
                    "{what}: the JS uses its material {jm} again, here another"
                )),
                Some(_) => {}
                None => {
                    pairs.push((jm, n.materials[0]));
                    diff(
                        &format!("{what} material {jm}"),
                        &material_view(&s, &d, n.materials[0]),
                        &jt[jm],
                        &mut problems,
                    );
                }
            }
        } else if w.get("mesh").is_some() {
            problems.push(format!("{what}: no mesh here"));
        }
        if problems.len() == before {
            matched += 1;
        }
    }
    let extra = it.count();
    if extra > 0 {
        problems.push(format!("{extra} children here past the JS's"));
    }

    // Canvas textures within WP 3.2's threshold.
    pairs.sort_by_key(|&(j, _)| j);
    let mats: Vec<u32> = pairs.iter().map(|&(_, m)| m).collect();
    let ct = canvas_textures(&s, &mats);
    let gc = g["canvas"].as_array().expect("canvas");
    let js = cached_export();
    let js_ct = js.as_ref().map(|js| {
        let jroot = &js.nodes[js.roots[0] as usize];
        let jg = jroot
            .children
            .iter()
            .find(|&&c| js.nodes[c as usize].name == "mountain")
            .copied()
            .expect("mountain in the export");
        let mut order = Vec::new();
        fn walk(js: &Scene, n: u32, order: &mut Vec<u32>) {
            let node = &js.nodes[n as usize];
            if node.mesh.is_some() && !order.contains(&node.materials[0]) {
                order.push(node.materials[0]);
            }
            for &c in &node.children {
                walk(js, c, order);
            }
        }
        for &c in &js.nodes[jg as usize].children {
            walk(js, c, &mut order);
        }
        canvas_textures(js, &order)
    });
    if ct.len() != gc.len() {
        problems.push(format!(
            "{} canvas textures here, the JS {}",
            ct.len(),
            gc.len()
        ));
    }
    // The lettered ones (`map` and `emissiveMap` both: the sign atlas,
    // the snow poles' bands, the banners, the neon and the pole sign) are
    // held to Chrome's drawing with the bundled fonts (mountain-
    // textures.mjs); the export drew its text with the machine's fonts, so
    // against it they are only reported.
    let lettered = lettered_textures(&s, &mats);
    let fm: Value = serde_json::from_str(TEXTURES).expect("textures golden parses");
    let fc = fm["canvas"].as_array().expect("canvas");
    if lettered.len() != fc.len() {
        problems.push(format!(
            "{} lettered canvas textures here, the font-matched capture {}",
            lettered.len(),
            fc.len()
        ));
    }
    let mut rows = Vec::new();
    for (k, (&t, w)) in ct.iter().zip(gc).enumerate() {
        let tex = &s.textures[t as usize];
        let (tw, th) = (tex.width as usize, tex.height as usize);
        if w["width"].as_u64() != Some(tw as u64) || w["height"].as_u64() != Some(th as u64) {
            problems.push(format!(
                "canvas {k}: {tw}×{th}, the JS {}×{}",
                w["width"], w["height"]
            ));
            continue;
        }
        let px = rgba(&s, t);
        let bd = block_diff(&block_means(tw, th, &px), &blocks_of(w));
        let mad = js_ct.as_ref().and_then(|jc| {
            let jt = *jc.get(k)?;
            let jpx = rgba(js.as_ref().expect("export"), jt);
            write_sheet(&format!("canvas-{k}"), tw, th, &jpx, &px);
            Some(mr_canvas::compare::mean_abs_diff(&jpx, &px))
        });
        // Against the font-matched capture, for a lettered texture.
        let fmatch = lettered
            .iter()
            .position(|&l| s.textures[l as usize].pixels == tex.pixels);
        let mut gate = (
            mad.unwrap_or(bd),
            if mad.is_some() {
                "mean abs diff"
            } else {
                "block means differ by"
            },
        );
        let mut fonts = String::new();
        if let Some(f) = fmatch {
            let fw = &fc[f];
            let fbd = block_diff(&block_means(tw, th, &px), &blocks_of(fw));
            let fmad = cached_rgba(f, fw["sha256"].as_str().expect("sha")).map(|jpx| {
                write_sheet(&format!("lettered-{f}"), tw, th, &jpx, &px);
                mr_canvas::compare::mean_abs_diff(&jpx, &px)
            });
            gate = (
                fmad.unwrap_or(fbd),
                if fmad.is_some() {
                    "mean abs diff (bundled fonts)"
                } else {
                    "block means (bundled fonts) differ by"
                },
            );
            fonts = format!(
                "; with the bundled fonts: block means {fbd:.2?}, mean abs diff {}",
                fmad.map_or("(no cache)".into(), |m| format!("{m:.2?}"))
            );
        }
        if gate.0.iter().any(|&v| v >= LIMIT) {
            problems.push(format!(
                "canvas {k} ({tw}×{th}): {} {:.2?} (limit {LIMIT})",
                gate.1, gate.0
            ));
        }
        rows.push(format!(
            "canvas {k} {tw}×{th}: against the export: block means {bd:.2?}, mean abs diff {}{fonts}",
            mad.map_or("(no cache)".into(), |m| format!("{m:.2?}"))
        ));
    }
    println!(
        "mountain: {matched} of {} children identical ({} left out: the parked cars, WP 4.1); {} materials",
        want.len(),
        skipped.len(),
        pairs.len()
    );
    for r in &rows {
        println!("  {r}");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
