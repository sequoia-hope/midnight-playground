//! WP 7.3's L3 gate: Desert Run against the browser's scene export (SPEC
//! 5.7; DECISIONS D550 on). Desert is the level's only scenery module, so
//! with it ported the level builds from ported code alone
//! (`scenery_factory(None)`), and the whole scene is held to the export,
//! as Level 1 is (D472):
//!
//! - `desert_is_the_export`: the scene's counts and material kinds equal
//!   the world golden's (`parity/golden/world/desert.json`) always, in CI
//!   and in wasm; with the cache, the whole `mr_scene` digest equals the
//!   export's entry by entry (every mesh, drawable, material and texture),
//!   but for the pixels of canvas textures, which the group test holds to
//!   WP 3.2's threshold;
//! - `desert_group` and `road_group`: the groups `desert` and `road`
//!   (whose asphalt and shoulders past the lake's start carry Desert's
//!   cracked mud) against `parity/golden/desert/desert.json`
//!   (`tools/parity/desert-golden.mjs`): one line per node (type, name,
//!   every attribute's and the index's SHA-256, flags, local matrix,
//!   instance count, matrices and colours), each drawable's material in a
//!   canonical form (keys sorted, numbers as bits; textures by sampler),
//!   every light, and the 8×8 block means of every texture a material uses
//!   within WP 3.2's threshold (3/255 per channel); the lettered canvases
//!   against Chrome's drawing with the bundled fonts
//!   (`parity/golden/desert/textures.json`, `tools/parity/desert-textures.mjs`),
//!   the others against the export. With the cache, a failing node is
//!   shown beside the JS one, a failing material as both views, and each
//!   texture's mean absolute difference over every pixel is checked, with
//!   sheets in `parity/report/desert/`.
//!
//! The scene is taken as the export took it: built, the sky updated at the
//! start of the route around the export's focus, the night parameters at
//! the export's night factor, the updaters run once (dt 0, s 0, the
//! export's camera) and their edits applied.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::sync::OnceLock;

use mr_canvas::compare::{block_diff, block_means};
use mr_scene::digest::{SceneDigest, digest, sha256_hex};
use mr_scene::{Scene, TextureSource};
use mr_worldgen::color::Color;
use mr_worldgen::material::{Param, num};
use mr_worldgen::object::SceneGraph;
use mr_worldgen::scenery::scenery_factory;
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::three_geom::{Quaternion, Vector3};
use mr_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs};
use serde_json::{Map, Value, json};

fn golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/desert/desert.json"))
        .expect("desert golden parses")
}

fn world_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/world/desert.json"))
        .expect("world golden parses")
}

/// The font-matched capture of Desert's lettered textures
/// (`tools/parity/desert-textures.mjs`).
fn lettered_golden() -> Vec<Value> {
    let v: Value =
        serde_json::from_str(include_str!("../../../parity/golden/desert/textures.json"))
            .expect("desert textures golden parses");
    v["levels"]["desert"].as_array().expect("a level").clone()
}

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Keys sorted, numbers as their f64 bits (as desert-golden.mjs `canon`).
fn canon(v: &Value) -> Value {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let mut m = Map::new();
            for k in keys {
                m.insert(k.clone(), canon(&o[k]));
            }
            Value::Object(m)
        }
        Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
        Value::Number(n) => Value::from(bits(n.as_f64().expect("a number") + 0.0)),
        _ => v.clone(),
    }
}

/// As tests/road.rs `material_view` (and road-plan.mjs `materialView`).
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
    let mut m = walk(
        &serde_json::to_value(&s.materials[m as usize]).expect("json"),
        s,
        d,
    );
    if let Some(Value::Object(o)) = m.get_mut("shader") {
        for k in ["vertex", "fragment"] {
            let text = o[k].as_str().expect("glsl").to_string();
            o.insert(k.into(), Value::from(sha256_hex(text.as_bytes())));
        }
    }
    m
}

/// What the gate compares of a group.
struct GroupView {
    lines: Vec<String>,
    mats: Vec<i64>,
    table: Vec<u32>,
    vertices: u64,
}

fn group_view(s: &Scene, d: &SceneDigest, name: &str) -> GroupView {
    let group = s
        .nodes
        .iter()
        .position(|n| n.name == name)
        .unwrap_or_else(|| panic!("a group {name}"));
    let mut v = GroupView {
        lines: Vec::new(),
        mats: Vec::new(),
        table: Vec::new(),
        vertices: 0,
    };
    fn walk(c: u32, s: &Scene, d: &SceneDigest, v: &mut GroupView) {
        let n = &s.nodes[c as usize];
        let mut ml = "-".to_string();
        let mut inst = "- - -".to_string();
        let mut mk = -1i64;
        if let Some(mesh) = n.mesh {
            let m = &d.meshes[mesh as usize];
            v.vertices += m.vertices;
            let attrs: Vec<String> = m
                .attributes
                .iter()
                .map(|(k, x)| format!("{k}={}", x.as_str().expect("sha")))
                .collect();
            ml = format!(
                "{} {} {} {}",
                m.vertices,
                m.indices,
                attrs.join(","),
                m.index.as_deref().unwrap_or("null")
            );
            let dr = d.drawables.iter().find(|x| x.node == c).expect("drawable");
            inst = format!(
                "{} {} {}",
                dr.instances.map_or("-".into(), |i| i.to_string()),
                dr.instance_matrices.as_deref().unwrap_or("-"),
                dr.instance_colors.as_deref().unwrap_or("-"),
            );
            let mat = n.materials[0];
            mk = match v.table.iter().position(|&x| x == mat) {
                Some(k) => k as i64,
                None => {
                    v.table.push(mat);
                    v.table.len() as i64 - 1
                }
            };
        }
        // The file writes -0 as 0 (JSON).
        let matrix: Vec<String> = n.matrix.iter().map(|&x| bits(x + 0.0)).collect();
        let msha = sha256_hex(matrix.join(",").as_bytes());
        v.lines.push(format!(
            "{:?} {} {} {}{} {} {} {} {} {} {}",
            n.ty,
            n.name,
            ml,
            u8::from(n.cast_shadow),
            u8::from(n.receive_shadow),
            u8::from(n.visible),
            n.render_order,
            u8::from(n.frustum_culled),
            u8::from(n.matrix_auto_update),
            &msha[..16],
            inst
        ));
        v.mats.push(mk);
        for &k in &n.children {
            walk(k, s, d, v);
        }
    }
    for &c in &s.nodes[group].children {
        walk(c, s, d, &mut v);
    }
    v
}

/// The textures a material uses: (where, texture index).
fn material_textures(s: &Scene, m: u32) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let md = &s.materials[m as usize];
    let lists: [(&str, Option<&Map<String, Value>>); 2] = [
        ("params", Some(&md.params)),
        ("uniforms", md.uniforms.as_ref()),
    ];
    for (where_, obj) in lists {
        let Some(obj) = obj else { continue };
        for (k, v) in obj {
            if let Value::Object(o) = v
                && o.len() == 1
                && let Some(t) = o.get("texture").and_then(Value::as_u64)
            {
                if s.textures[t as usize].channels != 4 {
                    continue;
                }
                out.push((format!("{where_}.{k}"), t as u32));
            }
        }
    }
    out
}

/// Sets a uniform if the material has it, else the parameter.
fn set_number(graph: &mut SceneGraph, m: mr_worldgen::object::MaterialId, prop: &str, v: f64) {
    let mat = graph.material_mut(m);
    if let Some(u) = &mut mat.desc.uniforms
        && let Some(slot) = u.get_mut(prop)
    {
        *slot = num(v);
        return;
    }
    mat.set_value(prop, Param::Num(v));
}

/// Applies the updaters' edits to the graph, as the JS objects took them.
/// A light's target follows its target node (`beam.target`), as three
/// reads it.
fn apply(graph: &mut SceneGraph, edits: Vec<Edit>, spot: Option<(u32, u32)>) {
    for e in edits {
        match (e.target, e.change) {
            (Handle::Material(m), Change::Number { prop, value }) => {
                set_number(graph, m, prop, value)
            }
            (Handle::Material(m), Change::Color { prop, rgb }) => {
                graph
                    .material_mut(m)
                    .set_value(prop, Param::Color(Color::new(rgb[0], rgb[1], rgb[2])));
            }
            (Handle::Node(n), Change::InstanceMatrix { index, matrix }) => {
                let inst = graph.get_mut(n).instances.as_mut().expect("instanced");
                inst.matrices[index as usize * 16..][..16].copy_from_slice(&matrix);
            }
            (Handle::Node(n), Change::Visible(v)) => graph.get_mut(n).visible = v,
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
                if let Some((beam, target)) = spot
                    && n.0 == target
                {
                    let l = graph
                        .get_mut(mr_worldgen::object::NodeId(beam))
                        .light
                        .as_mut()
                        .expect("a light");
                    l.target = Some(position);
                }
            }
            (
                Handle::Node(n),
                Change::Light {
                    color,
                    intensity,
                    ground_color,
                },
            ) => {
                let l = graph.get_mut(n).light.as_mut().expect("a light");
                l.color = color;
                l.intensity = intensity;
                if ground_color.is_some() {
                    l.ground_color = ground_color;
                }
            }
            (t, c) => panic!("an edit the gate does not apply: {t:?} {c:?}"),
        }
    }
}

/// Desert Run as the export had it (built once for every test here).
fn exported() -> &'static Scene {
    static SCENE: OnceLock<Scene> = OnceLock::new();
    SCENE.get_or_init(|| {
        let g = golden();
        let setup = LevelSetup {
            terrain: common::terrain_setup("desert"),
            road: None,
        };
        let mut b = Build::new(
            World::new(common::level("desert")),
            level_jobs(level_stages(setup), scenery_factory(None)),
        );
        while !b.is_done() {
            b.step().expect("Desert Run builds");
        }
        let w = &mut b.world;
        assert!(w.graph.log.is_empty(), "the build logged {:?}", w.graph.log);
        // The sky as the exported frame left it: at the start of the route
        // (dt 0), its dome and lights around the fly camera's focus.
        let road: Value = serde_json::from_str(common::road_text("desert")).expect("road golden");
        let f = &road["sky"]["focus"];
        let focus = [0, 1, 2].map(|i| f[i].as_f64().expect("focus"));
        {
            let World { sky, graph, .. } = &mut *w;
            let sky = sky.as_mut().expect("a sky");
            let mut edits = Vec::new();
            sky.update(0.0, 0.0, Some(focus), &mut edits);
            sky.apply(graph, &edits);
        }
        let night = g["night"].as_f64().expect("night");
        for n in w.graph.night.clone() {
            set_number(
                &mut w.graph,
                n.material,
                &n.prop,
                n.day + (n.night - n.day) * night,
            );
        }
        // The train's spot light and its target, for `apply`.
        let spot = w
            .graph
            .objects
            .iter()
            .position(|o| o.ty == mr_scene::NodeType::SpotLight)
            .map(|i| {
                let p = w.graph.objects[i].parent.expect("in the group");
                let sib = &w.graph.get(p).children;
                let k = sib.iter().position(|c| c.0 as usize == i).expect("a child");
                (i as u32, sib[k + 1].0)
            });
        let cam = &g["camera"];
        let ctx = UpdateCtx {
            dt: 0.0,
            night,
            camera: Some(CameraView {
                position: [0, 1, 2].map(|i| cam["position"][i].as_f64().expect("camera")),
                fov: cam["fov"].as_f64().expect("fov"),
                viewport_height: 800.0,
            }),
            s: 0.0,
        };
        let mut edits = Vec::new();
        for a in &mut w.animators {
            a.update(&ctx, &mut edits);
        }
        apply(&mut w.graph, edits, spot);
        w.graph.finish().0
    })
}

/// The cached JS export and its digest, if there.
fn cached() -> Option<(Scene, SceneDigest)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let scene = dir.join("desert.mrscene");
    let dig = dir.join("desert.digest.json");
    if !scene.exists() || !dig.exists() {
        return None;
    }
    let s = mr_scene::read_file(&scene).ok()?;
    let d: SceneDigest =
        serde_json::from_str(&std::fs::read_to_string(dig).ok()?).expect("a digest");
    Some((s, d))
}

fn js_export() -> Option<&'static (Scene, SceneDigest)> {
    static JS: OnceLock<Option<(Scene, SceneDigest)>> = OnceLock::new();
    JS.get_or_init(cached).as_ref()
}

fn pixels(s: &Scene, t: u32) -> (usize, usize, Vec<u8>) {
    let tex = &s.textures[t as usize];
    let buf = &s.buffers[tex.pixels as usize];
    (
        tex.width as usize,
        tex.height as usize,
        buf.data.to_le_bytes(),
    )
}

/// The canvas textures a material uses as both `map` and `emissiveMap`,
/// in order of first use, once per picture (as desert-textures.mjs reads
/// them).
fn lettered_textures(s: &Scene, mats: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    for &m in mats {
        let md = &s.materials[m as usize];
        let (Some(a), Some(b)) = (md.texture("map"), md.texture("emissiveMap")) else {
            continue;
        };
        let t = &s.textures[a as usize];
        if a == b
            && t.source == TextureSource::Canvas
            && !out
                .iter()
                .any(|&o| s.textures[o as usize].pixels == t.pixels)
        {
            out.push(a);
        }
    }
    out
}

/// The font-matched capture's RGBA, when the cache has it and it is the
/// one the golden describes.
fn captured_rgba(k: usize, sha: &str) -> Option<Vec<u8>> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let p = dir.parent()?.join(format!("desert/desert-canvas-{k}.rgba"));
    let b = std::fs::read(p).ok()?;
    (sha256_hex(&b) == sha).then_some(b)
}

/// JS, Rust and their difference side by side, in `parity/report/desert/`.
fn sheet(name: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let dir = common::root().join("parity/report/desert");
    std::fs::create_dir_all(&dir).expect("report dir");
    let (sw, sh, px) = mr_canvas::compare::sheet(w, h, js, rust);
    let path = dir.join(format!("{name}.png"));
    let f = std::fs::File::create(path).expect("sheet file");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(&px)
        .expect("png data");
}

fn check_group(name: &str) {
    let g = golden();
    let scene = exported();
    let d = digest(scene);
    let v = group_view(scene, &d, name);
    let gc = &g[name];
    let mut problems: Vec<String> = Vec::new();
    let js = js_export();
    let jv = js.map(|(s, d)| group_view(s, d, name));

    // Nodes.
    let want: Vec<&str> = gc["lines"]
        .as_array()
        .expect("lines")
        .iter()
        .map(|l| l.as_str().expect("line"))
        .collect();
    let ours: Vec<String> = v
        .lines
        .iter()
        .map(|l| sha256_hex(l.as_bytes())[..16].to_string())
        .collect();
    if ours.len() != want.len() {
        problems.push(format!("{} nodes, the JS has {}", ours.len(), want.len()));
    }
    let mut differ = 0;
    for (k, (o, w)) in ours.iter().zip(&want).enumerate() {
        if o != w {
            differ += 1;
            if differ <= 8 {
                let theirs = jv.as_ref().map_or("(no cache)".into(), |j| {
                    j.lines.get(k).cloned().unwrap_or_default()
                });
                problems.push(format!("node {k}:\n  rust {}\n  js   {theirs}", v.lines[k]));
            }
        }
    }
    if differ > 8 {
        problems.push(format!("… {differ} nodes differ in all"));
    }
    if sha256_hex(v.lines.join("\n").as_bytes()) != gc["sha256"].as_str().expect("sha")
        && differ == 0
    {
        problems.push("the node lines' hash differs".into());
    }
    if v.vertices != gc["vertices"].as_u64().expect("vertices") {
        problems.push(format!(
            "{} vertices, the JS has {}",
            v.vertices, gc["vertices"]
        ));
    }
    // Materials.
    let wmats: Vec<i64> = gc["mats"]
        .as_array()
        .expect("mats")
        .iter()
        .map(|x| x.as_i64().expect("index"))
        .collect();
    if v.mats != wmats {
        let first = v
            .mats
            .iter()
            .zip(&wmats)
            .position(|(a, b)| a != b)
            .unwrap_or(v.mats.len().min(wmats.len()));
        problems.push(format!(
            "the nodes' material indices differ from node {first}: {:?} vs {:?}",
            &v.mats[first.min(v.mats.len())..(first + 8).min(v.mats.len())],
            &wmats[first.min(wmats.len())..(first + 8).min(wmats.len())]
        ));
    }
    let wm = gc["materials"].as_array().expect("materials");
    let mut mat_differ = 0;
    for (k, &m) in v.table.iter().enumerate() {
        let view = canon(&material_view(scene, &d, m));
        let sha = sha256_hex(serde_json::to_string(&view).expect("json").as_bytes());
        let Some(w) = wm.get(k) else {
            problems.push(format!("material {k} has no JS counterpart"));
            continue;
        };
        if sha != w["sha256"].as_str().expect("sha") {
            mat_differ += 1;
            let detail = match (js, &jv) {
                (Some((s, dd)), Some(j)) if k < j.table.len() => {
                    let jview = canon(&material_view(s, dd, j.table[k]));
                    let (Value::Object(a), Value::Object(bb)) = (&view, &jview) else {
                        unreachable!()
                    };
                    let mut out = Vec::new();
                    for (key, x) in a {
                        if bb.get(key) != Some(x) {
                            out.push(format!(
                                "{key}: rust {x}\n     js {}",
                                bb.get(key).unwrap_or(&Value::Null)
                            ));
                        }
                    }
                    for key in bb.keys() {
                        if !a.contains_key(key) {
                            out.push(format!("{key}: missing in rust"));
                        }
                    }
                    out.join("\n    ")
                }
                _ => String::new(),
            };
            problems.push(format!(
                "material {k} ({}) differs\n    {detail}",
                w["kind"]
            ));
        }
    }
    // Textures: within WP 3.2's threshold. The lettered ones (`map` and
    // `emissiveMap` both) against Chrome's drawing with the bundled fonts:
    // the export drew its text with the machine's fonts, so against it they
    // are only reported. The rest against the export.
    let lettered = lettered_textures(scene, &v.table);
    let fc = if name == "desert" {
        lettered_golden()
    } else {
        Vec::new()
    };
    if name == "desert" && lettered.len() != fc.len() {
        problems.push(format!(
            "{} lettered canvas textures here, the font-matched capture {}",
            lettered.len(),
            fc.len()
        ));
    }
    let mut tk = 0;
    let mut worst = [0.0f64; 4];
    let wt = gc["textures"].as_array().expect("textures");
    for (k, &m) in v.table.iter().enumerate() {
        for (key, t) in material_textures(scene, m) {
            let Some(w) = wt.get(tk) else {
                problems.push(format!("texture {key} of material {k}: none in the JS"));
                continue;
            };
            tk += 1;
            assert_eq!(
                w["key"].as_str(),
                Some(key.as_str()),
                "{name}: texture order"
            );
            let (tw, th, px) = pixels(scene, t);
            if tw as u64 != w["width"].as_u64().expect("width")
                || th as u64 != w["height"].as_u64().expect("height")
            {
                problems.push(format!("texture {key} of material {k}: size {tw}×{th}"));
                continue;
            }
            let ours_b = block_means(tw, th, &px);
            let theirs: Vec<[f64; 4]> = w["blocks"]
                .as_array()
                .expect("blocks")
                .iter()
                .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().expect("mean")))
                .collect();
            let bd = block_diff(&ours_b, &theirs);
            let mut line =
                format!("texture {key} of material {k} ({tw}×{th}): block diff {bd:.2?}");
            let mut gate = bd;
            if let (Some((s, _)), Some(j)) = (js, &jv)
                && k < j.table.len()
            {
                let jt = material_textures(s, j.table[k]);
                if let Some(&(_, jt)) = jt.iter().find(|(kk, _)| *kk == key) {
                    let (_, _, jpx) = pixels(s, jt);
                    let mad = mr_canvas::compare::mean_abs_diff(&px, &jpx);
                    sheet(
                        &format!("{name}-m{k}-{}", key.replace('.', "-")),
                        tw,
                        th,
                        &jpx,
                        &px,
                    );
                    line += &format!(", mean abs diff {mad:.3?}");
                    gate = mad;
                }
            }
            let tex = &scene.textures[t as usize];
            let fmatch = lettered
                .iter()
                .position(|&l| scene.textures[l as usize].pixels == tex.pixels);
            if let Some(f) = fmatch
                && let Some(fw) = fc.get(f)
            {
                if fw["width"].as_u64() != Some(tw as u64)
                    || fw["height"].as_u64() != Some(th as u64)
                {
                    problems.push(format!(
                        "lettered {f}: {tw}×{th}, the capture {}×{}",
                        fw["width"], fw["height"]
                    ));
                    continue;
                }
                let fblocks: Vec<[f64; 4]> = fw["blocks"]
                    .as_array()
                    .expect("blocks")
                    .iter()
                    .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().expect("mean")))
                    .collect();
                let fbd = block_diff(&ours_b, &fblocks);
                let fmad = captured_rgba(f, fw["sha256"].as_str().expect("sha")).map(|jpx| {
                    sheet(&format!("lettered-{f}"), tw, th, &jpx, &px);
                    mr_canvas::compare::mean_abs_diff(&px, &jpx)
                });
                line += &format!(
                    " (held to the font-matched capture; against the export only reported): with the bundled fonts, block diff {fbd:.2?}, mean abs diff {}",
                    fmad.map_or("(no cache)".into(), |m| format!("{m:.3?}"))
                );
                gate = fmad.unwrap_or(fbd);
            }
            for c in 0..4 {
                worst[c] = worst[c].max(gate[c]);
            }
            let bad = gate.iter().any(|&x| x >= LIMIT);
            println!("{name}: {line}");
            if bad {
                problems.push(line);
            }
        }
    }
    if tk != wt.len() {
        problems.push(format!("{tk} textures, the JS has {}", wt.len()));
    }
    // The lights.
    if name == "desert" {
        let wl: Vec<&str> = g["lights"]
            .as_array()
            .expect("lights")
            .iter()
            .map(|x| x.as_str().expect("sha"))
            .collect();
        let ol: Vec<String> = scene
            .lights
            .iter()
            .map(|l| {
                let v = canon(&serde_json::to_value(l).expect("json"));
                sha256_hex(serde_json::to_string(&v).expect("json").as_bytes())[..16].to_string()
            })
            .collect();
        if ol != wl {
            let detail = js.map_or(String::new(), |(s, _)| {
                format!("\n  rust {:?}\n  js   {:?}", scene.lights, s.lights)
            });
            problems.push(format!("the lights differ{detail}"));
        }
    }
    println!(
        "{name}: {} nodes ({} differ), {} vertices, {} materials ({mat_differ} differ), {tk} textures, worst texture difference {worst:.2?}",
        v.lines.len(),
        differ,
        v.vertices,
        v.table.len()
    );
    assert!(problems.is_empty(), "{name}:\n  {}", problems.join("\n  "));
}

#[test]
fn desert_group() {
    check_group("desert");
}

#[test]
fn road_group() {
    check_group("road");
}

#[test]
fn desert_is_the_export() {
    let scene = exported();
    let ours = digest(scene);
    let g = world_golden();
    let want = &g["scene"];
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
    println!("desert: counts {counts}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The whole digest, with the cache.
    let Some((_, js)) = js_export() else {
        println!("desert: no cached export; counts and kinds only");
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
        "desert: {} meshes, {} drawables, {} materials, {} textures ({canvas} canvas textures' pixels left to the threshold gate)",
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
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
