//! WP 3.5's L3 gate: the road, sky and sea against the JS scene exports
//! (SPEC 5.7; DECISIONS D26, D274). Each level is built through
//! `level_jobs` with every ported stage (`stages::level_stages`: the
//! terrain with its recorded plan, the road plan the scenery would
//! register, the road, the sky, the sea), and compared with
//! `parity/golden/road/<level>.json`, which `tools/parity/road-plan.mjs`
//! writes from the browser's exports:
//!
//! - the road: per child of the group `road`, its type, vertex and index
//!   counts, the SHA-256 of every attribute (`aLane` included) and of the
//!   index, its shadow flags, its instance count and matrices, hashed
//!   together; which material each child draws with; and every material
//!   parameter by parameter, uniforms included, its textures by sampler
//!   (and pixels, but for canvas textures: WP 3.2's threshold gate);
//! - `track.sideL`/`sideR` against the world golden;
//! - the sky at the export's point of the route and focus: the dome's
//!   mesh, its material (uniforms, the GLSL by hash), the four roots'
//!   nodes, both lights, the fog, exposure and night factor;
//! - the sea (Coast): its node, mesh, material and wave normal map;
//! - the number of textures (terrain, road and sky share one
//!   `terrainDetailTexture`, D271).
//!
//! What the scenery does to the road after it is built is replayed here,
//! as the scenery is not ported yet: Streets makes its route's asphalt
//! wetter (`Streets.js` build), and Desert lays its lake bed material on
//! the asphalt and shoulders past the lake's start (`buildLakebed`); those
//! meshes are checked to be the ones the JS swapped, and their material is
//! Desert's. The night parameters and the dew updater are applied at the
//! export's night factor, as the exported game had them.
//!
//! The golden is compiled in, so the gate runs in wasm too. With the cache
//! (`node tools/parity/scene-export.mjs --base`), a failing road is
//! localised per mesh and attribute.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use common::{hex, level};
use mp_math::smoothstep;
use mp_scene::digest::{SceneDigest, digest, sha256_hex};
use mp_scene::{Scene, TextureSource};
use mp_track::FenceGap;
use mp_worldgen::color::Color;
use mp_worldgen::material::{Param, num};
use mp_worldgen::object::{MaterialId, NodeId, SceneGraph};
use mp_worldgen::road::MarkGap;
use mp_worldgen::scenery::plan_only_factory;
use mp_worldgen::stages::{LevelSetup, RoadPlan, level_stages};
use mp_worldgen::world::{Build, Change, World, level_jobs};
use serde_json::{Value, json};

fn golden(id: &str) -> Value {
    let text = match id {
        "sierra" => include_str!("../../../parity/golden/road/sierra.json"),
        "coast" => include_str!("../../../parity/golden/road/coast.json"),
        "streets" => include_str!("../../../parity/golden/road/streets.json"),
        "desert" => include_str!("../../../parity/golden/road/desert.json"),
        "seaside" => include_str!("../../../parity/golden/road/seaside.json"),
        "cruise" => include_str!("../../../parity/golden/road/cruise.json"),
        _ => panic!("no golden for {id}"),
    };
    serde_json::from_str(text).expect("road golden parses")
}

fn world_golden(id: &str) -> Value {
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

/// The recorded plan.
fn road_plan(g: &Value) -> RoadPlan {
    RoadPlan {
        fence_gaps: g["fenceGaps"]
            .as_array()
            .expect("fenceGaps")
            .iter()
            .map(|f| FenceGap {
                s0: hex(&f["s0"]),
                s1: hex(&f["s1"]),
                side: hex(&f["side"]),
            })
            .collect(),
        no_marks: g["noMarks"]
            .as_array()
            .expect("noMarks")
            .iter()
            .map(|f| MarkGap {
                s0: hex(&f["s0"]),
                s1: hex(&f["s1"]),
            })
            .collect(),
        runout: hex(&g["runout"]),
    }
}

/// Every number as an f64 (1 and 1.0 are the same number in the JS).
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), norm(x))).collect()),
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        Value::Number(n) => Value::from(n.as_f64().expect("a number")),
        _ => v.clone(),
    }
}

/// A material with each texture reference replaced by what the texture is
/// (as `road-plan.mjs` writes it), and a ShaderMaterial's GLSL by hash.
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
    if let Some(sh) = m.get_mut("shader")
        && let Value::Object(o) = sh
    {
        for k in ["vertex", "fragment"] {
            let text = o[k].as_str().expect("glsl").to_string();
            o.insert(k.into(), Value::from(sha256_hex(text.as_bytes())));
        }
    }
    norm(&m)
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

fn node_view(s: &Scene, n: u32) -> Value {
    let n = &s.nodes[n as usize];
    json!({
        "name": n.name, "type": n.ty, "matrix": n.matrix.to_vec(), "visible": n.visible,
        "matrix_auto_update": n.matrix_auto_update, "frustum_culled": n.frustum_culled,
        "render_order": n.render_order, "cast_shadow": n.cast_shadow,
        "receive_shadow": n.receive_shadow, "layers": n.layers,
    })
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

/// A level built with every ported stage, its sky updated as the export's
/// game had it, the scenery's edits to the road replayed; the scene
/// without the sea (the base export's content) and with it, and which
/// road children Desert's lake bed covers.
struct Built {
    base: Scene,
    full: Scene,
    side_l: Vec<u8>,
    side_r: Vec<u8>,
    /// Road children (by position in the group) Desert's lake bed covers.
    lake: Vec<usize>,
    night: f64,
}

fn build(id: &str, g: &Value) -> Built {
    // The scenery registers the plan: the ported modules' own plan(), the
    // others replayed from the recordings (DECISIONS D330); their builds
    // are skipped, so the scene holds what the base export holds.
    let setup = LevelSetup {
        terrain: common::terrain_setup(id),
        road: None,
    };
    let mut b = Build::new(
        World::new(level(id)),
        level_jobs(
            level_stages(setup),
            plan_only_factory(Some(common::recording(id))),
        ),
    );
    let mut labels = Vec::new();
    while let Some((label, _)) = b.progress() {
        let label = label.to_string();
        labels.push(label.clone());
        b.step().expect("the level builds");
        if label == "Shaping the land" {
            // What the road is told equals the recording, bit for bit.
            let want = road_plan(g);
            let t = b.world.track();
            let gaps = |v: &[FenceGap]| {
                v.iter()
                    .map(|f| [f.s0, f.s1, f.side].map(f64::to_bits))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                gaps(&t.fence_gaps),
                gaps(&want.fence_gaps),
                "{id}: fenceGaps"
            );
            let marks = |v: &[MarkGap]| {
                v.iter()
                    .map(|f| [f.s0, f.s1].map(f64::to_bits))
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                marks(&b.world.no_marks),
                marks(&want.no_marks),
                "{id}: noMarks"
            );
            assert_eq!(t.runout.to_bits(), want.runout.to_bits(), "{id}: runout");
        }
    }
    // Road and sky in "Paving roads", after the terrain; the sea after.
    let at = |l: &str| labels.iter().position(|x| x == l);
    assert!(at("Sculpting terrain") < at("Paving roads"), "{labels:?}");
    let w = &mut b.world;
    let night = g["sky"]["night"].as_f64().expect("night");

    // The sky as the exported frame left it: at the start of the route
    // (dt 0: ?freeze=1), the lights placed around the fly camera's focus.
    let f = &g["sky"]["focus"];
    let focus = [0, 1, 2].map(|i| f[i].as_f64().expect("focus"));
    {
        let World { sky, graph, .. } = &mut *w;
        let sky = sky.as_mut().expect("a sky");
        let mut edits = Vec::new();
        let frame = sky.update(0.0, 0.0, Some(focus), &mut edits);
        sky.apply(graph, &edits);
        assert_eq!(frame.night, night, "{id}: the night factor");
    }
    // The night parameters and the dew updater at that night factor.
    for n in w.graph.night.clone() {
        set_number(
            &mut w.graph,
            n.material,
            &n.prop,
            n.day + (n.night - n.day) * night,
        );
    }
    let road = w.road.as_ref().expect("a road");
    let mut edits = Vec::new();
    road.set_night(smoothstep(0.55, 1.0, night) * 0.85, &mut edits);
    for e in edits {
        if let (mp_worldgen::world::Handle::Material(m), Change::Number { prop, value }) =
            (e.target, e.change)
        {
            set_number(&mut w.graph, m, prop, value);
        }
    }
    // Streets' build: "Route road: a little wetter than a dry street."
    if id == "streets"
        && let Some(rm) = road.material("asphalt2")
    {
        let m = w.graph.material_mut(rm);
        m.set_value("roughness", Param::Num(0.62));
        m.set_value("metalness", Param::Num(0.05));
        m.set_value("color", Param::Color(Color::new(0.72, 0.72, 0.72)));
    }
    // Desert's buildLakebed: which asphalt and shoulder meshes it covers.
    let mut lake = Vec::new();
    if id == "desert" {
        let t = w.track.as_ref().expect("track");
        let lake_from = t.zones[2].s0 + 120.0;
        let asphalt = road.asphalt_materials();
        let shoulder = road.material("shoulder");
        let children: Vec<NodeId> = w.graph.get(road.group).children.clone();
        for (k, &c) in children.iter().enumerate() {
            let o = w.graph.get(c);
            let m = o.materials[0];
            if !(asphalt.contains(&m) || Some(m) == shoulder) {
                continue;
            }
            let mut geo = w.graph.geometry(o.geometry.expect("geometry")).clone();
            geo.compute_bounding_sphere();
            let ctr = geo.bounding_sphere.expect("sphere").center;
            let hint = t.nearest(ctr.x, ctr.z, 400.0);
            if hint < 0 {
                continue;
            }
            let p = t.project_window(ctr.x, ctr.z, hint as f64, 30);
            if p.s > lake_from {
                lake.push(k);
            }
        }
    }
    let side_l = road.side_l.clone();
    let side_r = road.side_r.clone();
    let (full, _) = w.graph.finish();
    let mut graph = w.graph.clone();
    if let Some(sea) = &w.sea {
        graph.detach(sea.mesh);
    }
    let (base, _) = graph.finish();
    Built {
        base,
        full,
        side_l,
        side_r,
        lake,
        night,
    }
}

/// Desert's lake bed material (`buildLakebed`): a cracked-mud canvas
/// repeated (4.4, 2.5), roughness 0.95.
fn is_lake_bed(v: &Value) -> bool {
    norm(&v["params"]["map"]["texture"]["repeat"]) == norm(&json!([4.4, 2.5]))
        && norm(&v["params"]["roughness"]) == norm(&json!(0.95))
}

/// The road's lines (as road-plan.mjs makes them), each child's material
/// and the materials in order of first use.
fn road_lines(s: &Scene, d: &SceneDigest) -> (Vec<String>, Vec<u32>, u64) {
    let g = s
        .nodes
        .iter()
        .find(|n| n.name == "road")
        .expect("a road group");
    let mut lines = Vec::new();
    let mut mats = Vec::new();
    let mut vertices = 0;
    for &c in &g.children {
        let n = &s.nodes[c as usize];
        let mesh = n.mesh.expect("a mesh");
        vertices += d.meshes[mesh as usize].vertices;
        let dr = d.drawables.iter().find(|x| x.node == c).expect("drawable");
        lines.push(format!(
            "{:?} {} {}{} {} {}",
            n.ty,
            mesh_line(d, mesh),
            u8::from(n.cast_shadow),
            u8::from(n.receive_shadow),
            dr.instances.map_or("-".into(), |i| i.to_string()),
            dr.instance_matrices.as_deref().unwrap_or("-"),
        ));
        mats.push(n.materials[0]);
    }
    (lines, mats, vertices)
}

fn check(id: &str) {
    let g = golden(id);
    let b = build(id, &g);
    let mut problems = Vec::new();

    // track.sideL / sideR.
    let wg = world_golden(id);
    for (name, arr) in [("sideL", &b.side_l), ("sideR", &b.side_r)] {
        if sha256_hex(arr) != wg["track"]["arrays"][name]["sha256"].as_str().expect("sha") {
            problems.push(format!("track.{name} differs from the world golden"));
        }
    }

    // The road.
    let d = digest(&b.base);
    let (lines, mats, vertices) = road_lines(&b.base, &d);
    let want = &g["road"];
    if lines.len() as u64 != want["count"].as_u64().expect("count")
        || vertices != want["vertices"].as_u64().expect("vertices")
    {
        problems.push(format!(
            "road: {} meshes, {vertices} vertices; the JS {} and {}",
            lines.len(),
            want["count"],
            want["vertices"]
        ));
    }
    let same_lines =
        sha256_hex(lines.join("\n").as_bytes()) == want["sha256"].as_str().expect("sha");
    if !same_lines {
        problems.push("road: the meshes differ from the JS export's".into());
        problems.extend(localise(id, &b.base, &d));
    }
    // Materials: each child's, by the JS table.
    let table = want["table"].as_array().expect("table");
    let js_mats: Vec<usize> = want["materials"]
        .as_array()
        .expect("materials")
        .iter()
        .map(|v| v.as_u64().expect("index") as usize)
        .collect();
    let mut seen: Vec<(Option<u32>, usize)> = Vec::new();
    for (k, &m) in mats.iter().enumerate() {
        let Some(&jm) = js_mats.get(k) else { break };
        if b.lake.contains(&k) {
            if !is_lake_bed(&table[jm]) {
                problems.push(format!(
                    "road child {k}: under the lake bed, the JS material is not it"
                ));
            }
            continue;
        }
        match seen.iter().find(|(x, _)| *x == Some(m)) {
            Some(&(_, j)) if j != jm => {
                problems.push(format!("road child {k}: material {jm} in the JS, {j} here"));
            }
            Some(_) => {}
            None => {
                seen.push((Some(m), jm));
                let mut out = Vec::new();
                diff(
                    &format!("material {jm}"),
                    &material_view(&b.base, &d, m),
                    &table[jm],
                    &mut out,
                );
                problems.extend(out);
            }
        }
    }
    if mats.len() != js_mats.len() {
        problems.push(format!(
            "road: {} children, the JS {}",
            mats.len(),
            js_mats.len()
        ));
    }
    let js_lake = js_mats.iter().filter(|&&j| is_lake_bed(&table[j])).count();
    if b.lake.len() != js_lake {
        problems.push(format!(
            "{} meshes under the lake bed here, {js_lake} in the JS",
            b.lake.len()
        ));
    }

    // Textures: terrain, road and sky share the detail texture.
    let tw = g["textures"].as_u64().expect("textures") as usize;
    // Desert's lake bed brings a texture of its own; Seaside's terrain
    // drapes the aerial photo and its loose-ground mask, which this build
    // leaves out (the photo is decoded by the client, D234).
    let lake_tex = usize::from(id == "desert");
    let photo_tex = if id == "seaside" { 2 } else { 0 };
    if b.base.textures.len() + lake_tex + photo_tex != tw {
        problems.push(format!(
            "{} textures, the JS export {tw}{}",
            b.base.textures.len(),
            if lake_tex == 1 {
                " (one is Desert's lake bed)"
            } else {
                ""
            }
        ));
    }

    // The sky.
    let sky = &g["sky"];
    let roots = &b.base.roots;
    let dome = &b.base.nodes[roots[1] as usize];
    if mesh_line(&d, dome.mesh.expect("dome mesh")) != sky["mesh"].as_str().expect("mesh") {
        problems.push("sky: the dome's mesh differs".into());
    }
    diff(
        "sky material",
        &material_view(&b.base, &d, dome.materials[0]),
        &sky["material"],
        &mut problems,
    );
    for (k, &r) in roots[1..].iter().enumerate() {
        diff(
            &format!("sky node {k}"),
            &node_view(&b.base, r),
            &sky["nodes"][k],
            &mut problems,
        );
    }
    for (k, l) in b.base.lights.iter().enumerate() {
        let mut v = serde_json::to_value(l).expect("json");
        v.as_object_mut().expect("object").remove("node");
        diff(&format!("light {k}"), &v, &sky["lights"][k], &mut problems);
    }
    let (frame, _) = {
        // The frame's fog and exposure (the renderer's state at export).
        let l = level(id);
        let t = mp_track::Track::new(&l).expect("track");
        let p = mp_worldgen::sky::SkyParams::new(&l, &t);
        (p.frame_at(0.0, None), ())
    };
    diff(
        "fog",
        &json!({ "color": [frame.fog_color.r, frame.fog_color.g, frame.fog_color.b], "density": frame.fog_density }),
        &json!({ "color": sky["fog"]["color"], "density": sky["fog"]["density"] }),
        &mut problems,
    );
    diff(
        "exposure",
        &json!(frame.exposure),
        &sky["exposure"],
        &mut problems,
    );
    diff("night", &json!(b.night), &sky["night"], &mut problems);

    // The sea.
    let sea = &g["sea"];
    let fd = digest(&b.full);
    let root = &b.full.nodes[b.full.roots[0] as usize];
    let ours = root.children.iter().position(|&c| {
        b.full.nodes[c as usize]
            .materials
            .first()
            .is_some_and(|&m| b.full.materials[m as usize].kind == mp_scene::MaterialKind::Sea)
    });
    match (ours, sea.is_null()) {
        (None, true) => {}
        (Some(_), true) => problems.push("a sea the JS level does not have".into()),
        (None, false) => problems.push("no sea".into()),
        (Some(k), false) => {
            if Some(k as u64) != sea["child"].as_u64() {
                problems.push(format!(
                    "sea: child {k} of the root, the JS {}",
                    sea["child"]
                ));
            }
            let c = root.children[k];
            let n = &b.full.nodes[c as usize];
            diff(
                "sea node",
                &node_view(&b.full, c),
                &sea["node"],
                &mut problems,
            );
            if mesh_line(&fd, n.mesh.expect("mesh")) != sea["mesh"].as_str().expect("mesh") {
                problems.push(format!(
                    "sea: the mesh differs: {} vs the JS {}",
                    mesh_line(&fd, n.mesh.expect("mesh")),
                    sea["mesh"]
                ));
            }
            diff(
                "sea material",
                &material_view(&b.full, &fd, n.materials[0]),
                &sea["material"],
                &mut problems,
            );
            let nm = b.full.materials[n.materials[0] as usize]
                .texture("normalMap")
                .expect("normalMap");
            if fd.textures[nm as usize].sha256 != sea["normal_map_sha256"].as_str().expect("sha") {
                problems.push("sea: the wave normal map's pixels differ".into());
            }
        }
    }

    println!(
        "{id}: road {} meshes ({vertices} vertices){}, {} materials{}; sky; {}",
        lines.len(),
        if same_lines { " identical" } else { "" },
        table.len(),
        if b.lake.is_empty() {
            String::new()
        } else {
            format!(" ({} under Desert's lake bed)", b.lake.len())
        },
        if sea.is_null() {
            "no sea".to_string()
        } else {
            "sea".to_string()
        }
    );
    assert!(
        problems.is_empty(),
        "{id}: {} problem(s):\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// With the cache: which road meshes differ from the export's, and how.
#[cfg(not(target_arch = "wasm32"))]
fn localise(id: &str, s: &Scene, d: &SceneDigest) -> Vec<String> {
    let r = common::root();
    let Ok(dir) = mp_scene::cache::scenes_dir(&r) else {
        return vec!["(no cache to localise it)".into()];
    };
    let (scene, dig) = (
        dir.join(format!("{id}.base.mrscene")),
        dir.join(format!("{id}.base.digest.json")),
    );
    if !scene.exists() || !dig.exists() {
        return vec!["(no base export in the cache to localise it)".into()];
    }
    let js = mp_scene::read_file(&scene).expect("scene reads");
    let jd: SceneDigest =
        serde_json::from_slice(&std::fs::read(&dig).expect("digest")).expect("digest parses");
    let (ours, _, _) = road_lines(s, d);
    let (theirs, _, _) = road_lines(&js, &jd);
    let og = s.nodes.iter().find(|n| n.name == "road").expect("road");
    let jg = js.nodes.iter().find(|n| n.name == "road").expect("road");
    let mut out = Vec::new();
    for (k, (a, b)) in ours.iter().zip(&theirs).enumerate() {
        if a == b {
            continue;
        }
        let (on, jn) = (
            &s.nodes[og.children[k] as usize],
            &js.nodes[jg.children[k] as usize],
        );
        let (om, jm) = (on.mesh.expect("mesh"), jn.mesh.expect("mesh"));
        let (od, jdm) = (&d.meshes[om as usize], &jd.meshes[jm as usize]);
        let mut what = vec![format!(
            "{:?} {}/{} vs {:?} {}/{}",
            on.ty, od.vertices, od.indices, jn.ty, jdm.vertices, jdm.indices
        )];
        for attr in &js.meshes[jm as usize].attributes {
            let Some(ra) = s.meshes[om as usize].attribute(&attr.name) else {
                what.push(format!("no {}", attr.name));
                continue;
            };
            if od.attributes.get(&attr.name) != jdm.attributes.get(&attr.name) {
                let (x, y) = (
                    &s.buffers[ra.accessor as usize].data,
                    &js.buffers[attr.accessor as usize].data,
                );
                let mut n = 0;
                let mut worst = 0.0f64;
                let mut first = None;
                for i in 0..x.len().min(y.len()) {
                    if x.get(i).to_bits() != y.get(i).to_bits() {
                        n += 1;
                        worst = worst.max((x.get(i) - y.get(i)).abs());
                        first.get_or_insert(i);
                    }
                }
                what.push(format!(
                    "{}: {n} values differ (lengths {} and {}), worst {worst:e}, first at {first:?}",
                    attr.name,
                    x.len(),
                    y.len()
                ));
            }
        }
        if od.index != jdm.index {
            what.push("index differs".into());
        }
        out.push(format!("road child {k}: {}", what.join("; ")));
        if out.len() > 12 {
            out.push("...".into());
            break;
        }
    }
    if ours.len() != theirs.len() {
        out.push(format!(
            "{} road children, the JS {}",
            ours.len(),
            theirs.len()
        ));
    }
    out
}

#[cfg(target_arch = "wasm32")]
fn localise(_id: &str, _s: &Scene, _d: &SceneDigest) -> Vec<String> {
    Vec::new()
}

#[test]
fn road_sierra() {
    check("sierra");
}

#[test]
fn road_coast() {
    check("coast");
}

#[test]
fn road_streets() {
    check("streets");
}

#[test]
fn road_desert() {
    check("desert");
}

#[test]
fn road_seaside() {
    check("seaside");
}

#[test]
fn road_cruise() {
    check("cruise");
}

// ── The time of day along the route ─────────────────────────────────────

/// `Sky.update(0, s, focus)` at 41 points from the start to the finish,
/// the focus on the road there: every value it sets (the dome's uniforms,
/// both lights and their placing, fog, exposure, the dome's position), in
/// the order `road-plan.mjs` hashes them, must be bit-identical.
fn sky_route(id: &str) {
    use mp_worldgen::sky::Sky;
    use mp_worldgen::textures::TextureCache;
    let g = golden(id);
    let want = &g["skyRoute"];
    let n = want["samples"].as_u64().expect("samples") as usize;
    let l = level(id);
    let t = mp_track::Track::new(&l).expect("track");
    let mut graph = SceneGraph::new();
    let mut textures = TextureCache::new();
    let mut sky = Sky::new(&mut graph, &mut textures, &l, &t);
    let mut values: Vec<f64> = Vec::new();
    for i in 0..n {
        let s = (i as f64 / (n - 1) as f64) * t.length;
        let f = t.frame(s);
        let mut edits = Vec::new();
        let k = sky.update(0.0, s, Some([f.x, f.y, f.z]), &mut edits);
        sky.apply(&mut graph, &edits);
        let u = graph
            .material(sky.material)
            .desc
            .uniforms
            .clone()
            .expect("uniforms");
        let c = |name: &str| -> Vec<f64> {
            let v = &u[name];
            let a = v.get("color").or_else(|| v.get("vec")).unwrap_or(v);
            match a {
                Value::Array(a) => a.iter().map(|x| x.as_f64().expect("number")).collect(),
                _ => vec![a.as_f64().expect("number")],
            }
        };
        values.extend([s, sky.night]);
        for name in [
            "uZenith",
            "uHorizon",
            "uGround",
            "uSunColor",
            "uSunDir",
            "uMoonDir",
            "uNight",
            "uCloud",
            "uHaze",
        ] {
            values.extend(c(name));
        }
        let sun = graph.get(sky.sun);
        let sl = sun.light.as_ref().expect("light");
        values.extend(sl.color);
        values.push(sl.intensity);
        values.extend([sun.position.x, sun.position.y, sun.position.z]);
        let tg = graph.get(sky.target).position;
        values.extend([tg.x, tg.y, tg.z]);
        let hl = graph.get(sky.hemi).light.as_ref().expect("light");
        values.extend(hl.color);
        values.extend(hl.ground_color.expect("ground"));
        values.push(hl.intensity);
        values.extend([
            k.fog_color.r,
            k.fog_color.g,
            k.fog_color.b,
            k.fog_density,
            k.exposure,
        ]);
        let dp = graph.get(sky.dome).position;
        values.extend([dp.x, dp.y, dp.z]);
        values.push(k.key.sun_el);
        // What the client reads from the frame is what the scene shows.
        assert_eq!(
            sl.color,
            [k.light_color.r, k.light_color.g, k.light_color.b]
        );
    }
    assert_eq!(
        values.len() as u64,
        want["values"].as_u64().expect("values")
    );
    let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    assert_eq!(
        sha256_hex(&bytes),
        want["sha256"].as_str().expect("sha"),
        "{id}: the sky along the route differs from the JS"
    );
}

#[test]
fn sky_route_sierra() {
    sky_route("sierra");
}

#[test]
fn sky_route_coast() {
    sky_route("coast");
}

#[test]
fn sky_route_streets() {
    sky_route("streets");
}

#[test]
fn sky_route_desert() {
    sky_route("desert");
}

#[test]
fn sky_route_seaside() {
    sky_route("seaside");
}

#[test]
fn sky_route_cruise() {
    sky_route("cruise");
}
