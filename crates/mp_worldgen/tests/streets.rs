//! WP 7.2's L3 gate: Downtown Streets against the browser's scene export
//! (SPEC 5.7; DECISIONS D610 on). Streets is the level's only scenery
//! module, so with it ported the level builds from ported code alone
//! (`scenery_factory(None)`), and the scene is held to the export as Level
//! 2 is (D531, D536):
//!
//! - `streets_group` and `road_group`: the groups `streets` and `road`
//!   (whose `asphalt2` Streets wets) against
//!   `parity/golden/streets/streets.json` (`tools/parity/streets-golden.mjs`):
//!   one line per node (type, name, every attribute's and the index's
//!   SHA-256, flags, local matrix, instance count, matrices and colours),
//!   each drawable's material in a canonical form (keys sorted, numbers as
//!   bits; textures by sampler), and every texture a material uses: canvas
//!   pictures within WP 3.2's threshold (3/255 per channel) of Chrome's
//!   drawing with the bundled fonts (`parity/golden/streets/textures.json`,
//!   `tools/parity/streets-textures.mjs`, D535's method), the others bit for
//!   bit. The goldens are compiled in, so the gate runs in CI and wasm;
//!   with the cache a failing node is shown beside the JS one, a failing
//!   material as both views, and each texture's mean absolute difference
//!   over every pixel is checked, with sheets in `parity/report/streets/`.
//! - `streets_is_the_export`: the scene's counts and material kinds equal
//!   the world golden's (`parity/golden/world/streets.json`) always; with
//!   the cache, the whole `mp_scene` digest equals the export's entry by
//!   entry but for the pixels of canvas textures, which the group gate
//!   holds.
//! - `streets_world_data`: the runout Streets' `plan()` sets (0) and no
//!   opposite carriageway, against `mp_levels::world` and the WP 0.4 dump.
//!
//! The scene is taken as the export took it (`?freeze=1`): built, the sky
//! updated at the start of the route around the export's focus, the night
//! parameters at the export's night factor, every updater run once (dt 0,
//! s 0, the export's camera) and its edits applied.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::sync::OnceLock;

use mp_canvas::compare::{block_diff, block_means};
use mp_scene::digest::{SceneDigest, digest, sha256_hex};
use mp_scene::{BufferData, Scene, TextureSource};
use mp_worldgen::color::Color;
use mp_worldgen::material::{Param, color_value, num};
use mp_worldgen::object::{MaterialId, SceneGraph};
use mp_worldgen::scenery::scenery_factory;
use mp_worldgen::stages::{LevelSetup, level_stages};
use mp_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs};
use serde_json::{Map, Value, json};

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("../../../parity/golden/streets/streets.json"))
            .expect("streets golden parses")
    })
}

/// The font-matched capture of the groups' canvas textures
/// (`tools/parity/streets-textures.mjs`).
fn captured() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("../../../parity/golden/streets/textures.json"))
            .expect("streets textures golden parses")
    })
}

fn world_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/world/streets.json"))
        .expect("world golden parses")
}

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Keys sorted, numbers as their f64 bits (as coast-golden.mjs `canon`).
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

/// As tests/city.rs `material_view` (and road-plan.mjs `materialView`).
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

/// What the gate compares of one group.
struct GroupView {
    lines: Vec<String>,
    mats: Vec<i64>,
    table: Vec<u32>,
    vertices: u64,
}

fn group_view(s: &Scene, d: &SceneDigest, name: &str) -> Option<GroupView> {
    let group = s.nodes.iter().position(|n| n.name == name)?;
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
    Some(v)
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

fn set_color(graph: &mut SceneGraph, m: MaterialId, prop: &str, rgb: [f64; 3]) {
    let c = Color::new(rgb[0], rgb[1], rgb[2]);
    let mat = graph.material_mut(m);
    if let Some(u) = &mut mat.desc.uniforms
        && let Some(slot) = u.get_mut(prop)
    {
        *slot = color_value(c);
        return;
    }
    mat.set_value(prop, Param::Color(c));
}

/// Applies the animators' edits to the graph, as the JS objects took them.
fn apply(graph: &mut SceneGraph, edits: Vec<Edit>) {
    for e in edits {
        match (e.target, e.change) {
            (Handle::Material(m), Change::Number { prop, value }) => {
                set_number(graph, m, prop, value)
            }
            (Handle::Material(m), Change::Color { prop, rgb }) => set_color(graph, m, prop, rgb),
            (Handle::Node(n), Change::InstanceColor { index, rgb }) => {
                let inst = graph.get_mut(n).instances.as_mut().expect("instanced");
                inst.set_color_at(
                    index as usize,
                    Color::new(f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])),
                );
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
                o.position =
                    mp_worldgen::three_geom::Vector3::new(position[0], position[1], position[2]);
                o.quaternion = mp_worldgen::three_geom::Quaternion {
                    x: quaternion[0],
                    y: quaternion[1],
                    z: quaternion[2],
                    w: quaternion[3],
                };
                o.scale = mp_worldgen::three_geom::Vector3::new(scale[0], scale[1], scale[2]);
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
                    .expect("the attribute");
                match &mut a.array {
                    BufferData::F32(v) => v[offset..][..values.len()].copy_from_slice(&values),
                    d => panic!("an edited buffer of {:?}", d.component()),
                }
            }
            (Handle::Texture(t), Change::TextureOffset(o)) => {
                graph.texture_mut(t).desc.offset = o;
            }
            (t, c) => panic!("an edit the gate does not apply: {t:?} {c:?}"),
        }
    }
}

struct Built {
    scene: Scene,
    digest: SceneDigest,
    world: mp_levels::world::WorldData,
    track: mp_track::Track,
}

/// Downtown Streets built as `World.build` builds it and taken as its
/// exported frame was drawn.
fn built() -> &'static Built {
    static B: OnceLock<Built> = OnceLock::new();
    B.get_or_init(|| {
        let g = golden();
        let setup = LevelSetup {
            terrain: common::terrain_setup("streets"),
            road: None,
        };
        let mut b = Build::new(
            World::new(common::level("streets")),
            // Every module of Level 3 is ported: no recording, as the client
            // builds it.
            level_jobs(level_stages(setup), scenery_factory(None)),
        );
        while !b.is_done() {
            b.step().expect("the level builds");
        }
        let w = &mut b.world;
        assert!(
            w.graph.log.is_empty(),
            "streets: the build logged {:?}",
            w.graph.log
        );
        // The sky as the exported frame left it: at the start of the route
        // (dt 0), its dome and lights around the fly camera's focus.
        let road: Value = serde_json::from_str(common::road_text("streets")).expect("road golden");
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
        apply(&mut w.graph, edits);
        let (scene, _) = w.graph.finish();
        let digest = digest(&scene);
        Built {
            scene,
            digest,
            world: w.sim_data.clone(),
            track: w.track().clone(),
        }
    })
}

/// The cached JS export and its digest, if there.
fn cached() -> Option<&'static (Scene, SceneDigest)> {
    static C: OnceLock<Option<(Scene, SceneDigest)>> = OnceLock::new();
    C.get_or_init(|| {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let dir = mp_scene::cache::scenes_dir(&common::root()).ok()?;
        let scene = dir.join("streets.mrscene");
        let dig = dir.join("streets.digest.json");
        if !scene.exists() || !dig.exists() {
            return None;
        }
        let s = mp_scene::read_file(&scene).ok()?;
        let d: SceneDigest =
            serde_json::from_str(&std::fs::read_to_string(dig).ok()?).expect("a digest");
        Some((s, d))
    })
    .as_ref()
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

/// The font-matched capture's RGBA of a picture, when the cache has it and
/// it is the one the golden describes.
fn captured_rgba(k: usize, sha: &str) -> Option<Vec<u8>> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mp_scene::cache::scenes_dir(&common::root()).ok()?;
    let p = dir.parent()?.join(format!("streets/canvas-{k}.rgba"));
    let b = std::fs::read(p).ok()?;
    (sha256_hex(&b) == sha).then_some(b)
}

/// The mean absolute difference of the premultiplied colour, and the share
/// of pixels whose alpha falls on the other side of `alpha_test` (0 for
/// none).
fn premultiplied(a: &[u8], b: &[u8], alpha_test: f64) -> ([f64; 3], f64) {
    let n = a.len() / 4;
    let mut s = [0.0; 3];
    let mut cross = 0usize;
    let cut = alpha_test * 255.0;
    for i in 0..n {
        let (aa, ba) = (f64::from(a[i * 4 + 3]), f64::from(b[i * 4 + 3]));
        for c in 0..3 {
            let x = f64::from(a[i * 4 + c]) * aa / 255.0;
            let y = f64::from(b[i * 4 + c]) * ba / 255.0;
            s[c] += (x - y).abs();
        }
        if alpha_test > 0.0 && ((aa >= cut) != (ba >= cut)) {
            cross += 1;
        }
    }
    (s.map(|v| v / n as f64), cross as f64 / n as f64)
}

/// JS, Rust and their difference side by side, in `parity/report/streets/`.
fn sheet(name: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    let dir = common::root().join("parity/report/streets");
    std::fs::create_dir_all(&dir).expect("report dir");
    let (sw, sh, px) = mp_canvas::compare::sheet(w, h, js, rust);
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

fn check(group: &str) {
    let g = &golden()["groups"][group];
    let b = built();
    let v = group_view(&b.scene, &b.digest, group).expect("the group was built");
    let mut problems: Vec<String> = Vec::new();
    let js = cached();
    let jv = js.and_then(|(s, d)| group_view(s, d, group));

    // Nodes.
    let want: Vec<&str> = g["lines"]
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
                let theirs = jv
                    .as_ref()
                    .map_or("(no cache)".into(), |j| j.lines[k].clone());
                problems.push(format!("node {k}:\n  rust {}\n  js   {theirs}", v.lines[k]));
            }
        }
    }
    if differ > 8 {
        problems.push(format!("… {differ} nodes differ in all"));
    }
    if sha256_hex(v.lines.join("\n").as_bytes()) != g["sha256"].as_str().expect("sha")
        && differ == 0
    {
        problems.push("the node lines' hash differs".into());
    }
    if v.vertices != g["vertices"].as_u64().expect("vertices") {
        problems.push(format!(
            "{} vertices, the JS has {}",
            v.vertices, g["vertices"]
        ));
    }
    // Materials.
    let wmats: Vec<i64> = g["mats"]
        .as_array()
        .expect("mats")
        .iter()
        .map(|x| x.as_i64().expect("index"))
        .collect();
    if v.mats != wmats {
        problems.push(format!(
            "the nodes' material indices differ: {:?} vs {wmats:?}",
            v.mats
        ));
    }
    let wm = g["materials"].as_array().expect("materials");
    for (k, &m) in v.table.iter().enumerate() {
        let view = canon(&material_view(&b.scene, &b.digest, m));
        let sha = sha256_hex(serde_json::to_string(&view).expect("json").as_bytes());
        let Some(w) = wm.get(k) else {
            problems.push(format!("material {k} has no JS counterpart"));
            continue;
        };
        if sha != w["sha256"].as_str().expect("sha") {
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
                            out.push(format!("{key}: only in the JS"));
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
    // Textures: canvas pictures within WP 3.2's threshold of Chrome's
    // drawing with the bundled fonts; the export (drawn with the machine's
    // fonts) is only reported.
    let cap = &captured()["groups"][group];
    let pics = captured()["pictures"].as_array().expect("pictures");
    let mut tk = 0;
    let wt = g["textures"].as_array().expect("textures");
    for (k, &m) in v.table.iter().enumerate() {
        for (key, t) in material_textures(&b.scene, m) {
            let Some(w) = wt.get(tk) else {
                problems.push(format!("texture {key} of material {k}: none in the JS"));
                continue;
            };
            let ck = cap.get(tk).cloned().unwrap_or(Value::Null);
            tk += 1;
            assert_eq!(w["key"].as_str(), Some(key.as_str()), "texture order");
            let (tw, th, px) = pixels(&b.scene, t);
            if tw as u64 != w["width"].as_u64().unwrap()
                || th as u64 != w["height"].as_u64().unwrap()
            {
                problems.push(format!("texture {key} of material {k}: size {tw}×{th}"));
                continue;
            }
            let ours = block_means(tw, th, &px);
            let theirs: Vec<[f64; 4]> = w["blocks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().unwrap()))
                .collect();
            let bd = block_diff(&ours, &theirs);
            let mut line = format!(
                "{group}: texture {key} of material {k} ({tw}×{th}): against the export: block diff {bd:.2?}"
            );
            let mut gate = bd;
            if let (Some((s, _)), Some(j)) = (js, &jv) {
                let jt = material_textures(s, j.table[k]);
                if let Some(&(_, jt)) = jt.iter().find(|(kk, _)| *kk == key) {
                    let (_, _, jpx) = pixels(s, jt);
                    let mad = mp_canvas::compare::mean_abs_diff(&px, &jpx);
                    line += &format!(", mean abs diff {mad:.3?}");
                    gate = mad;
                }
            }
            // A canvas picture: held to the font-matched capture.
            if let Some(p) = ck["picture"].as_u64() {
                let pw = &pics[p as usize];
                if pw["width"].as_u64() != Some(tw as u64)
                    || pw["height"].as_u64() != Some(th as u64)
                {
                    problems.push(format!(
                        "{line}: the capture is {}×{}",
                        pw["width"], pw["height"]
                    ));
                    continue;
                }
                let fblocks: Vec<[f64; 4]> = pw["blocks"]
                    .as_array()
                    .expect("blocks")
                    .iter()
                    .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().expect("mean")))
                    .collect();
                let fbd = block_diff(&ours, &fblocks);
                let alpha_test = b.scene.materials[m as usize]
                    .params
                    .get("alphaTest")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);

                let mut extra = String::new();
                let fmad =
                    captured_rgba(p as usize, pw["sha256"].as_str().expect("sha")).map(|jpx| {
                        sheet(
                            &format!("{group}-m{k}-{}", key.replace('.', "-")),
                            tw,
                            th,
                            &jpx,
                            &px,
                        );
                        let mad = mp_canvas::compare::mean_abs_diff(&px, &jpx);
                        if std::env::var_os("STREETS_DUMP").is_some() {
                            let dir = common::root().join("parity/report/streets");
                            let _ = std::fs::write(dir.join(format!("{group}-m{k}-rust.rgba")), &px);
                            let _ = std::fs::write(dir.join(format!("{group}-m{k}-js.rgba")), &jpx);
                        }
                        if mad.iter().any(|&x| x >= LIMIT) {
                            let (pm, cross) = premultiplied(&px, &jpx, alpha_test);
                            extra = format!(
                                " (premultiplied RGB {pm:.2?}; {:.2} % of pixels on the other side of alphaTest {alpha_test})",
                                cross * 100.0
                            );
                        }
                        mad
                    });
                line += &format!(
                    "; with the bundled fonts (held): block diff {fbd:.2?}, mean abs diff {}{extra}",
                    fmad.map_or("(no cache)".into(), |m| format!("{m:.3?}"))
                );
                gate = fmad.unwrap_or(fbd);
            } else if b.scene.textures[t as usize].source == TextureSource::Canvas {
                problems.push(format!("{line}: a canvas picture the capture lacks"));
            }
            let limit = LIMIT;
            let bad = gate.iter().any(|&x| x >= limit);
            println!("{line}");
            if bad {
                problems.push(line);
            }
        }
    }
    if tk != wt.len() {
        problems.push(format!("{tk} textures, the JS has {}", wt.len()));
    }
    println!(
        "{group}: {} nodes ({} differ), {} vertices, {} materials",
        v.lines.len(),
        differ,
        v.vertices,
        v.table.len()
    );
    assert!(problems.is_empty(), "{group}:\n  {}", problems.join("\n  "));
}

#[test]
fn streets_group() {
    check("streets");
}

#[test]
fn road_group() {
    check("road");
}

#[test]
fn streets_is_the_export() {
    let b = built();
    let scene = &b.scene;
    let ours = &b.digest;
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
    println!("streets: counts {counts}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The whole digest, with the cache.
    let Some((_, js)) = cached() else {
        println!("streets: no cached export; counts and kinds only");
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
    problems.extend(mp_scene::digest::compare(&js_rest, &ours_rest));
    println!(
        "streets: {} meshes, {} drawables, {} materials, {} textures ({canvas} canvas textures' pixels left to the threshold gate)",
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

/// The world data Level 3's build gives the simulation (SPEC 4.3): the
/// runout Streets' `plan()` sets, and no opposite carriageway, against
/// `mp_levels::world`.
#[test]
fn streets_world_data() {
    let b = built();
    let wd = mp_levels::world::world_data("streets");
    assert_eq!(
        b.track.runout.to_bits(),
        wd.runout.to_bits(),
        "track.runout"
    );
    assert_eq!(b.world.runout.to_bits(), wd.runout.to_bits(), "sim runout");
    assert_eq!(
        b.world.opposite_carriageway, wd.opposite_carriageway,
        "oppositeCarriageway"
    );
}
