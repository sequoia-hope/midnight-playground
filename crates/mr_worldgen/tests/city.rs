//! WP 3.8's L3 gate: everything `City.js` and `city/*` contribute to
//! Sierra (zone 2) and the cruise loop, against the browser's scene exports
//! (SPEC 5.7; DECISIONS D350 on), and the world data City gives the
//! simulation (SPEC 4.3).
//!
//! Each level is built through `level_jobs` with every ported stage and the
//! scenery factory (City ported and built, the other modules' plans
//! replayed, D330); City's animators are run once as the exported frame ran
//! them (`?freeze=1`: dt 0, the export's night factor and camera), after the
//! night parameters. The group `city` is then compared with
//! `parity/golden/city/<level>.json` (`tools/parity/city-golden.mjs`): one
//! line per node (type, name, every attribute's and the index's SHA-256,
//! flags, local matrix, instance count, matrices and colours), each
//! drawable's material in a canonical form (keys sorted, numbers as bits;
//! textures by sampler), and the 8×8 block means of every texture a
//! material uses, within WP 3.2's threshold (3/255 per channel). The golden
//! is compiled in, so the gate runs in CI and wasm; with the cache
//! (`node tools/parity/scene-export.mjs`), a failing node is shown in full
//! beside the JS one, a failing material as both views, and each texture's
//! mean absolute difference over every pixel is checked.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_canvas::compare::{block_diff, block_means};
use mr_levels::world::world_data;
use mr_scene::digest::{SceneDigest, digest, sha256_hex};
use mr_scene::{Scene, TextureSource};
use mr_worldgen::color::Color;
use mr_worldgen::material::{Param, num};
use mr_worldgen::object::SceneGraph;
use mr_worldgen::scenery::scenery_factory;
use mr_worldgen::stages::{LevelSetup, level_stages};
use mr_worldgen::world::{Build, CameraView, Change, Edit, Handle, UpdateCtx, World, level_jobs};
use serde_json::{Map, Value, json};

fn golden(id: &str) -> Value {
    let text = match id {
        "sierra" => include_str!("../../../parity/golden/city/sierra.json"),
        "cruise" => include_str!("../../../parity/golden/city/cruise.json"),
        _ => panic!("no city golden for {id}"),
    };
    serde_json::from_str(text).expect("city golden parses")
}

fn sim_golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/sim/world-data.json"))
        .expect("world-data golden parses")
}

/// The block-mean bound for lettering in another face (D354).
const TEXT_BLOCKS: f64 = 12.0;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Keys sorted, numbers as their f64 bits (as city-golden.mjs `canon`).
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

/// What the gate compares of a scene's `city` group.
struct CityView {
    lines: Vec<String>,
    mats: Vec<i64>,
    table: Vec<u32>,
    vertices: u64,
}

fn city_view(s: &Scene, d: &SceneDigest) -> CityView {
    let group = s
        .nodes
        .iter()
        .position(|n| n.name == "city")
        .expect("a city group");
    let mut v = CityView {
        lines: Vec::new(),
        mats: Vec::new(),
        table: Vec::new(),
        vertices: 0,
    };
    fn walk(c: u32, s: &Scene, d: &SceneDigest, v: &mut CityView) {
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

/// Applies City's edits to the graph, as the JS objects took them.
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
            (Handle::Node(n), Change::InstanceColor { index, rgb }) => {
                let inst = graph.get_mut(n).instances.as_mut().expect("instanced");
                inst.set_color_at(
                    index as usize,
                    Color::new(f64::from(rgb[0]), f64::from(rgb[1]), f64::from(rgb[2])),
                );
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
                    mr_worldgen::three_geom::Vector3::new(position[0], position[1], position[2]);
                o.quaternion = mr_worldgen::three_geom::Quaternion {
                    x: quaternion[0],
                    y: quaternion[1],
                    z: quaternion[2],
                    w: quaternion[3],
                };
                o.scale = mr_worldgen::three_geom::Vector3::new(scale[0], scale[1], scale[2]);
            }
            (t, c) => panic!("City made an edit the gate does not apply: {t:?} {c:?}"),
        }
    }
}

struct Built {
    scene: Scene,
    world: mr_levels::world::WorldData,
    runout: f64,
    track: mr_track::Track,
}

/// The level built as `World.build` builds it, City's animators run once as
/// the exported frame ran them.
fn build(id: &str, g: &Value) -> Built {
    let setup = LevelSetup {
        terrain: common::terrain_setup(id),
        road: None,
    };
    let mut b = Build::new(
        World::new(common::level(id)),
        level_jobs(
            level_stages(setup),
            scenery_factory(Some(common::recording(id))),
        ),
    );
    let mut city_anims = None;
    while let Some((label, _)) = b.progress() {
        let city = label == "Building the city";
        let before = b.world.animators.len();
        b.step().expect("the level builds");
        if city {
            city_anims = Some(before..b.world.animators.len());
        }
    }
    assert!(
        b.world.graph.log.is_empty(),
        "{id}: the build logged {:?}",
        b.world.graph.log
    );
    let range = city_anims.expect("City was built");
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
    let cam = &g["camera"];
    let position = [0, 1, 2].map(|i| cam["position"][i].as_f64().expect("camera"));
    let ctx = UpdateCtx {
        dt: 0.0,
        night,
        camera: Some(CameraView {
            position,
            fov: cam["fov"].as_f64().expect("fov"),
            viewport_height: g["city"]["viewportHeight"].as_f64().expect("viewport"),
        }),
        s: 0.0,
    };
    let mut edits = Vec::new();
    for a in &mut w.animators[range] {
        a.update(&ctx, &mut edits);
    }
    apply(&mut w.graph, edits);
    let (scene, _) = w.graph.finish();
    let t = w.track.clone().expect("track");
    Built {
        scene,
        world: w.sim_data.clone(),
        runout: t.runout,
        track: t,
    }
}

/// The cached JS export and its digest, if there.
fn cached(id: &str) -> Option<(Scene, SceneDigest)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let scene = dir.join(format!("{id}.mrscene"));
    let dig = dir.join(format!("{id}.digest.json"));
    if !scene.exists() || !dig.exists() {
        return None;
    }
    let s = mr_scene::read_file(&scene).ok()?;
    let d: SceneDigest =
        serde_json::from_str(&std::fs::read_to_string(dig).ok()?).expect("a digest");
    Some((s, d))
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

/// JS, Rust and their difference side by side, in `parity/report/city/`.
fn sheet(id: &str, k: usize, key: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    let dir = common::root().join("parity/report/city");
    std::fs::create_dir_all(&dir).expect("report dir");
    let (sw, sh, px) = mr_canvas::compare::sheet(w, h, js, rust);
    let path = dir.join(format!("{id}-m{k}-{}.png", key.replace('.', "-")));
    let f = std::fs::File::create(path).expect("sheet file");
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header()
        .expect("png header")
        .write_image_data(&px)
        .expect("png data");
}

fn check(id: &str) {
    let g = golden(id);
    let b = build(id, &g);
    let d = digest(&b.scene);
    let v = city_view(&b.scene, &d);
    let gc = &g["city"];
    let mut problems: Vec<String> = Vec::new();
    let js = cached(id);
    let jv = js.as_ref().map(|(s, d)| city_view(s, d));

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
            if differ <= 6 {
                let theirs = jv
                    .as_ref()
                    .map_or("(no cache)".into(), |j| j.lines[k].clone());
                problems.push(format!("node {k}:\n  rust {}\n  js   {theirs}", v.lines[k]));
            }
        }
    }
    if differ > 6 {
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
        problems.push(format!(
            "the nodes' material indices differ: {:?} vs {wmats:?}",
            v.mats
        ));
    }
    let wm = gc["materials"].as_array().expect("materials");
    for (k, &m) in v.table.iter().enumerate() {
        let view = canon(&material_view(&b.scene, &d, m));
        let sha = sha256_hex(serde_json::to_string(&view).expect("json").as_bytes());
        let Some(w) = wm.get(k) else {
            problems.push(format!("material {k} has no JS counterpart"));
            continue;
        };
        if sha != w["sha256"].as_str().expect("sha") {
            let detail = match (&js, &jv) {
                (Some((s, dd)), Some(j)) => {
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
    // Textures: within WP 3.2's threshold.
    let mut tk = 0;
    let wt = gc["textures"].as_array().expect("textures");
    for (k, &m) in v.table.iter().enumerate() {
        for (key, t) in material_textures(&b.scene, m) {
            let Some(w) = wt.get(tk) else {
                problems.push(format!("texture {key} of material {k}: none in the JS"));
                continue;
            };
            tk += 1;
            assert_eq!(w["key"].as_str(), Some(key.as_str()), "{id}: texture order");
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
            let mut line =
                format!("texture {key} of material {k} ({tw}×{th}): block diff {bd:.2?}");
            // Lettering (banners, the sign and ad atlases: one canvas for both
            // `map` and `emissiveMap`, 1024 px wide or more) is drawn in the
            // fonts of the machine that exported the scene, which the export
            // does not pin (D354): those are held to the layout only (block
            // means within 12 levels), every other texture to WP 3.2's 3.
            let md = &b.scene.materials[m as usize];
            let text = tw >= 1024
                && md.texture("map").is_some()
                && md.texture("map") == md.texture("emissiveMap");
            let limit = if text { TEXT_BLOCKS } else { 3.0 };
            let mut bad = bd.iter().any(|&x| x >= limit);
            if let (Some((s, _)), Some(j)) = (&js, &jv) {
                let jt = material_textures(s, j.table[k]);
                if let Some(&(_, jt)) = jt.iter().find(|(kk, _)| *kk == key) {
                    let (_, _, jpx) = pixels(s, jt);
                    let mad = mr_canvas::compare::mean_abs_diff(&px, &jpx);
                    sheet(id, k, &key, tw, th, &jpx, &px);
                    line += &format!(", mean abs diff {mad:.3?}");
                    if !text {
                        bad = mad.iter().any(|&x| x >= 3.0);
                    }
                }
            }
            if text {
                line += " (lettering)";
            }
            println!("{id}: {line}");
            if bad {
                problems.push(line);
            }
        }
    }
    if tk != wt.len() {
        problems.push(format!("{tk} textures, the JS has {}", wt.len()));
    }
    println!(
        "{id}: city {} nodes ({} differ), {} vertices, {} materials",
        v.lines.len(),
        differ,
        v.vertices,
        v.table.len()
    );

    // The world data City gives the simulation (SPEC 4.3).
    let wd = world_data(id);
    assert_eq!(
        b.runout.to_bits(),
        wd.runout.to_bits(),
        "{id}: track.runout"
    );
    assert_eq!(
        b.world.runout.to_bits(),
        wd.runout.to_bits(),
        "{id}: sim runout"
    );
    assert_eq!(
        b.world.opposite_carriageway, wd.opposite_carriageway,
        "{id}: oppositeCarriageway"
    );
    let sg = sim_golden();
    let opp = b
        .world
        .opposite_carriageway
        .as_ref()
        .expect("a carriageway");
    for p in sg["levels"][id]["oppositeCarriageway"]["ySamples"]
        .as_array()
        .expect("ySamples")
    {
        let s = p[0].as_f64().unwrap();
        let y = p[1].as_f64().unwrap();
        assert_eq!(opp.y(&b.track, s).to_bits(), y.to_bits(), "{id}: oppY({s})");
    }

    assert!(problems.is_empty(), "{id}:\n  {}", problems.join("\n  "));
}

#[test]
fn city_sierra() {
    check("sierra");
}

#[test]
fn city_cruise() {
    check("cruise");
}
