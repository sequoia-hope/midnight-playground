//! WP 7.4's L3 gate: Seaside Raceway against the browser's scene export
//! (SPEC 5.7; DECISIONS D590 on). Raceway is the level's only scenery
//! module, so with it ported the level builds from ported code alone
//! (`scenery_factory(None)`), and the whole scene is held to the export, as
//! Level 1 and Desert Run are (D472, D551):
//!
//! - `raceway_group`: the group `raceway` against
//!   `parity/golden/seaside/seaside.json` (`tools/parity/raceway-golden.mjs`):
//!   one line per node (type, name, every attribute's and the index's
//!   SHA-256, flags, local matrix, instance count, matrices and colours),
//!   each drawable's material in a canonical form (keys sorted, numbers as
//!   bits; textures by sampler), and every texture a material uses: canvas
//!   pictures within WP 3.2's threshold (3/255 per channel) of Chrome's
//!   drawing with the bundled fonts (`parity/golden/seaside/textures.json`,
//!   `tools/parity/raceway-textures.mjs`), the export only reported. The
//!   golden is compiled in, so the gate runs in CI and wasm; with the cache
//!   a failing node is shown beside the JS one, a failing material as both
//!   views, and each texture's mean absolute difference over every pixel is
//!   checked, with sheets in `parity/report/seaside/`.
//! - `seaside_is_the_export`: the night parameters, the scene's counts and
//!   material kinds equal the world golden's (`parity/golden/world/
//!   seaside.json`) always; every texture that is not a canvas (the
//!   terrain's data texture, the loose-ground mask made from the survey and
//!   the aerial photo) byte for byte; with the cache, the whole `mp_scene`
//!   digest equals the export's entry by entry, but for the pixels of
//!   canvas textures, which `raceway_group` and the shared texture gates
//!   hold.
//!
//! The photo is decoded by the caller (D234): with the cache the test takes
//! Chrome's decode from the export, as `tests/terrain_mesh.rs` does; without
//! it a blank picture of the photo's size stands in, and the photo's bytes
//! are not checked.
//!
//! The scene is taken as the export took it (`?freeze=1`, the menu: no race,
//! so the start lights are dark): built, the sky updated at the start of the
//! route around the export's focus, the night parameters at the export's
//! night factor, the updaters run once (dt 0, s 0, the export's camera) and
//! their edits applied.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::sync::OnceLock;

use mp_canvas::compare::{block_diff, block_means};
use mp_scene::digest::{SceneDigest, digest, sha256_hex};
use mp_scene::{BufferData, Scene, TextureSource};
use mp_worldgen::color::Color;
use mp_worldgen::material::{Param, num};
use mp_worldgen::object::{MaterialId, SceneGraph};
use mp_worldgen::scenery::scenery_factory;
use mp_worldgen::stages::{LevelSetup, level_stages};
use mp_worldgen::terrain_mesh::GroundPhoto;
use mp_worldgen::textures::Texture;
use mp_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, level_jobs};
use serde_json::{Map, Value, json};

fn golden() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("../../../parity/golden/seaside/seaside.json"))
            .expect("seaside golden parses")
    })
}

fn world_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/world/seaside.json"))
        .expect("world golden parses")
}

/// The font-matched capture of the group's canvas textures
/// (`tools/parity/raceway-textures.mjs`).
fn captured() -> &'static Value {
    static G: OnceLock<Value> = OnceLock::new();
    G.get_or_init(|| {
        serde_json::from_str(include_str!("../../../parity/golden/seaside/textures.json"))
            .expect("seaside textures golden parses")
    })
}

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Keys sorted, numbers as their f64 bits (as raceway-golden.mjs `canon`).
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

/// Applies the updaters' edits to the graph, as the JS objects took them.
fn apply(graph: &mut SceneGraph, edits: Vec<Edit>) {
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
            (t, c) => panic!("an edit the gate does not apply: {t:?} {c:?}"),
        }
    }
}

/// The cached JS export and its digest, if there.
fn cached() -> Option<&'static (Scene, SceneDigest)> {
    static C: OnceLock<Option<(Scene, SceneDigest)>> = OnceLock::new();
    C.get_or_init(|| {
        if cfg!(target_arch = "wasm32") {
            return None;
        }
        let dir = mp_scene::cache::scenes_dir(&common::root()).ok()?;
        let scene = dir.join("seaside.mrscene");
        let dig = dir.join("seaside.digest.json");
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

/// The photo's URL in the scene (`level.groundPhoto.url`, relative to the
/// page).
const PHOTO_URL: &str = "src/levels/seaside/photo.jpg";

/// The draped photo as the caller hands it in (D234): Chrome's decode from
/// the export when cached, else a blank picture of its size.
fn photo() -> GroundPhoto {
    let survey = common::survey();
    let (w, h, rgba, url) = match cached() {
        Some((js, _)) => {
            let t = js
                .textures
                .iter()
                .find(|t| t.source == TextureSource::Image)
                .expect("the export has the photo");
            let BufferData::U8(rgba) = &js.buffers[t.pixels as usize].data else {
                panic!("photo pixels are bytes")
            };
            (
                t.width,
                t.height,
                rgba.clone(),
                t.url.clone().expect("a url"),
            )
        }
        None => {
            let (w, h) = (survey.photo.w as u32, survey.photo.h as u32);
            (w, h, vec![0; (w * h * 4) as usize], PHOTO_URL.to_string())
        }
    };
    let tex = Texture {
        width: w,
        height: h,
        rgba,
        source: TextureSource::Image,
        repeat: false,
        srgb: true,
        anisotropy: 8.0,
    };
    GroundPhoto::seaside(&survey, tex, &url)
}

/// Seaside Raceway as the export had it (built once for every test here).
fn exported() -> &'static (Scene, SceneDigest) {
    static SCENE: OnceLock<(Scene, SceneDigest)> = OnceLock::new();
    SCENE.get_or_init(|| {
        let g = golden();
        let mut terrain = common::terrain_setup("seaside");
        terrain.photo = Some(photo());
        let setup = LevelSetup {
            terrain,
            road: None,
        };
        let mut b = Build::new(
            common::world("seaside"),
            level_jobs(level_stages(setup), scenery_factory(None)),
        );
        while !b.is_done() {
            b.step().expect("Seaside Raceway builds");
        }
        let w = &mut b.world;
        assert!(w.graph.log.is_empty(), "the build logged {:?}", w.graph.log);
        assert!(w.on_countdown.is_some(), "Raceway sets world.onCountdown");
        // The sky as the exported frame left it: at the start of the route
        // (dt 0), its dome and lights around the fly camera's focus.
        let road: Value = serde_json::from_str(common::road_text("seaside")).expect("road golden");
        let f = &road["sky"]["focus"];
        let focus = [0, 1, 2].map(|i| f[i].as_f64().expect("focus"));
        {
            let sky = w.sky.as_mut().expect("a sky");
            let mut edits = Vec::new();
            sky.update(0.0, 0.0, Some(focus), &mut edits);
            sky.apply(&mut w.graph, &edits);
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
        let scene = w.graph.finish().0;
        let d = digest(&scene);
        (scene, d)
    })
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
    let p = dir.parent()?.join(format!("seaside/canvas-{k}.rgba"));
    let b = std::fs::read(p).ok()?;
    (sha256_hex(&b) == sha).then_some(b)
}

/// JS, Rust and their difference side by side, in `parity/report/seaside/`.
fn sheet(name: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    let dir = common::root().join("parity/report/seaside");
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

#[test]
fn raceway_group() {
    let group = "raceway";
    let g = &golden()[group];
    let (scene, d) = exported();
    let v = group_view(scene, d, group).expect("the group was built");
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
    let wm = g["materials"].as_array().expect("materials");
    let mut mat_differ = 0;
    for (k, &m) in v.table.iter().enumerate() {
        let view = canon(&material_view(scene, d, m));
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
    let mut worst = [0.0f64; 4];
    let wt = g["textures"].as_array().expect("textures");
    for (k, &m) in v.table.iter().enumerate() {
        for (key, t) in material_textures(scene, m) {
            let Some(w) = wt.get(tk) else {
                problems.push(format!("texture {key} of material {k}: none in the JS"));
                continue;
            };
            let ck = cap.get(tk).cloned().unwrap_or(Value::Null);
            tk += 1;
            assert_eq!(w["key"].as_str(), Some(key.as_str()), "texture order");
            let (tw, th, px) = pixels(scene, t);
            if tw as u64 != w["width"].as_u64().expect("width")
                || th as u64 != w["height"].as_u64().expect("height")
            {
                problems.push(format!("texture {key} of material {k}: size {tw}×{th}"));
                continue;
            }
            let ours = block_means(tw, th, &px);
            let theirs: Vec<[f64; 4]> = w["blocks"]
                .as_array()
                .expect("blocks")
                .iter()
                .map(|b| [0, 1, 2, 3].map(|i| b[i].as_f64().expect("mean")))
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
                    sheet(
                        &format!("export-m{k}-{}", key.replace('.', "-")),
                        tw,
                        th,
                        &jpx,
                        &px,
                    );
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
                let fmad =
                    captured_rgba(p as usize, pw["sha256"].as_str().expect("sha")).map(|jpx| {
                        sheet(
                            &format!("{group}-m{k}-{}", key.replace('.', "-")),
                            tw,
                            th,
                            &jpx,
                            &px,
                        );
                        if std::env::var_os("SEASIDE_DUMP").is_some() {
                            let dir = common::root().join("parity/report/seaside");
                            let _ =
                                std::fs::write(dir.join(format!("{group}-m{k}-rust.rgba")), &px);
                            let _ = std::fs::write(dir.join(format!("{group}-m{k}-js.rgba")), &jpx);
                        }
                        mp_canvas::compare::mean_abs_diff(&px, &jpx)
                    });
                line += &format!(
                    "; with the bundled fonts (held): block diff {fbd:.2?}, mean abs diff {}",
                    fmad.map_or("(no cache)".into(), |m| format!("{m:.3?}"))
                );
                gate = fmad.unwrap_or(fbd);
            } else if scene.textures[t as usize].source == TextureSource::Canvas {
                problems.push(format!("{line}: a canvas picture the capture lacks"));
            }
            for c in 0..4 {
                worst[c] = worst[c].max(gate[c]);
            }
            let bad = gate.iter().any(|&x| x >= LIMIT);
            println!("{line}");
            if bad {
                problems.push(line);
            }
        }
    }
    if tk != wt.len() {
        problems.push(format!("{tk} textures, the JS has {}", wt.len()));
    }
    // The lights (the sky's two).
    let wl: Vec<&str> = golden()["lights"]
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
    println!(
        "{group}: {} nodes ({differ} differ), {} vertices, {} materials ({mat_differ} differ), {tk} textures, worst texture difference {worst:.2?}",
        v.lines.len(),
        v.vertices,
        v.table.len()
    );
    assert!(problems.is_empty(), "{group}:\n  {}", problems.join("\n  "));
}

#[test]
fn seaside_is_the_export() {
    let (scene, ours) = exported();
    let g = world_golden();
    let want = &g["scene"];
    let mut problems = Vec::new();

    // The night parameters: which material, which property, its day and
    // night values, in the order registered.
    let ours_np: Vec<String> = scene
        .night_params
        .iter()
        .map(|p| format!("{} {} {:?} {:?}", p.material, p.prop, p.day, p.night))
        .collect();
    let js_np: Vec<String> = golden()["night_params"]
        .as_array()
        .expect("night params")
        .iter()
        .map(|p| {
            let n = |k: &str| p[k].as_f64().expect("a number");
            format!(
                "{} {} {:?} {:?}",
                p["material"],
                p["prop"].as_str().expect("a property"),
                n("day"),
                n("night")
            )
        })
        .collect();
    if ours_np != js_np {
        problems.push(format!(
            "night parameters: ours {ours_np:?}, the JS {js_np:?}"
        ));
    }

    let counts = serde_json::to_value(&ours.counts).expect("json");
    for (k, v) in want["counts"].as_object().expect("counts") {
        if k != "binary_bytes" && counts[k] != *v {
            problems.push(format!("{k}: ours {}, the JS {v}", counts[k]));
        }
    }
    let kinds = serde_json::to_value(&ours.kinds).expect("json");
    if kinds != want["kinds"] {
        problems.push(format!("kinds: ours {kinds}, the JS {}", want["kinds"]));
    }

    // Every texture that is not a canvas, byte for byte: the terrain's data
    // texture, the aerial photo (only with the cache, where it is Chrome's
    // decode) and the loose-ground mask made from the survey.
    let js = cached();
    for w in golden()["data"].as_array().expect("data textures") {
        let i = w["texture"].as_u64().expect("index") as usize;
        let Some(t) = scene.textures.get(i) else {
            problems.push(format!("no texture {i}"));
            continue;
        };
        let src = serde_json::to_value(t.source).expect("json");
        if src != w["source"]
            || u64::from(t.width) != w["width"].as_u64().expect("width")
            || u64::from(t.height) != w["height"].as_u64().expect("height")
            || u64::from(ours.textures[i].channels) != w["channels"].as_u64().expect("channels")
        {
            problems.push(format!(
                "texture {i}: {src} {}×{}×{}, the JS {w}",
                t.width, t.height, ours.textures[i].channels
            ));
            continue;
        }
        if t.source == TextureSource::Image && js.is_none() {
            println!("seaside: texture {i} (the photo): no cache, its bytes are the caller's");
            continue;
        }
        let sha = &ours.textures[i].sha256;
        if Some(sha.as_str()) != w["sha256"].as_str() {
            problems.push(format!(
                "texture {i} ({src}): sha {sha}, the JS {}",
                w["sha256"]
            ));
        } else {
            println!(
                "seaside: texture {i} ({src} {}×{}): byte-identical",
                t.width, t.height
            );
        }
    }
    println!("seaside: counts {counts}");
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    // The whole digest, with the cache.
    let Some((_, js)) = js else {
        println!("seaside: no cached export; counts, kinds and data textures only");
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
        "seaside: {} meshes, {} drawables, {} materials, {} textures ({canvas} canvas textures' pixels left to the threshold gates)",
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
