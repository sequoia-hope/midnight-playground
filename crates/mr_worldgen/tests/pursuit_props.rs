//! WP 8.2's L3 gate: PursuitView.js's props (the sawhorse with its striped
//! canvas board, and a spike strip on Sierra's road at startS + 200 from
//! lat -4 to 4) against the browser's models export (its group
//! `pursuit-props`), through `parity/golden/car_model/props.json`
//! (`tools/parity/pursuit-props-golden.mjs`): every node's line as
//! tests/car_model.rs writes it, the materials each node uses, each material
//! in its canonical form, and the board's texture's 8×8 block means within
//! WP 3.2's threshold (with the cache, its mean absolute difference over
//! every pixel).

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mr_canvas::compare::{block_diff, block_means};
use mr_scene::digest::{SceneDigest, digest, sha256_hex};
use mr_scene::{BufferData, Scene, TextureSource};
use mr_track::Track;
use mr_worldgen::object::SceneGraph;
use mr_worldgen::pursuit_props::{PropKit, sawhorse_model, spike_strip};
use serde_json::{Map, Value, json};

const GOLDEN: &str = include_str!("../../../parity/golden/car_model/props.json");

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// Keys sorted, numbers as their f64 bits (as car-model-golden.mjs
/// `canon`).
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

/// One node's line (car-model-golden.mjs `nodeLine`).
fn node_line(s: &Scene, d: &SceneDigest, i: u32) -> String {
    let n = &s.nodes[i as usize];
    let (mut ml, mut groups, mut sphere) = ("-".to_string(), "-".to_string(), "-".to_string());
    if let Some(mesh) = n.mesh {
        let m = &d.meshes[mesh as usize];
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
        let md = &s.meshes[mesh as usize];
        if !md.groups.is_empty() {
            groups = md
                .groups
                .iter()
                .map(|g| format!("{}+{}:{}", g.start, g.count, g.material_index))
                .collect::<Vec<_>>()
                .join(",");
        }
        if let Some(bs) = &md.bounding_sphere {
            sphere = bs
                .iter()
                .map(|&x| bits(x + 0.0))
                .collect::<Vec<_>>()
                .join(",");
        }
    }
    let multi = match n.multi_material {
        None => "-".to_string(),
        Some(m) => u8::from(m).to_string(),
    };
    // The file writes -0 as 0 (JSON).
    let matrix: Vec<String> = n.matrix.iter().map(|&x| bits(x + 0.0)).collect();
    let msha = sha256_hex(matrix.join(",").as_bytes());
    let ud = serde_json::to_string(&canon(&Value::Object(n.user_data.clone()))).expect("json");
    format!(
        "{:?} {} {} {} {} {}{} {} {} {} {} {} {} {}",
        n.ty,
        n.name,
        ml,
        groups,
        sphere,
        u8::from(n.cast_shadow),
        u8::from(n.receive_shadow),
        u8::from(n.visible),
        n.render_order,
        u8::from(n.frustum_culled),
        u8::from(n.matrix_auto_update),
        &msha[..16],
        ud,
        multi
    )
}

/// The cached JS export and its digest, if there.
fn cached() -> Option<(Scene, SceneDigest)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mr_scene::cache::scenes_dir(&common::root()).ok()?;
    let scene = dir.join("models.mrscene");
    let dig = dir.join("models.digest.json");
    if !scene.exists() || !dig.exists() {
        return None;
    }
    let s = mr_scene::read_file(&scene).ok()?;
    let d: SceneDigest =
        serde_json::from_str(&std::fs::read_to_string(dig).ok()?).expect("a digest");
    Some((s, d))
}

fn rgba(s: &Scene, t: u32) -> Vec<u8> {
    match &s.buffers[s.textures[t as usize].pixels as usize].data {
        BufferData::U8(v) => v.clone(),
        other => panic!("pixels as {other:?}"),
    }
}

/// The 4-channel textures a material uses: (where, texture index).
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

/// The props as `exportModels` builds them: a root `models` whose group
/// `pursuit-props` holds the sawhorse (its root named `sawhorse`) and the
/// spike strip (`spike-strip`).
fn build() -> (Scene, u32) {
    let mut graph = SceneGraph::new();
    let mut kit = PropKit::default();
    let root = graph.group("models");
    let props = graph.group("pursuit-props");
    graph.add(root, props);
    let saw = sawhorse_model(&mut graph, &mut kit);
    graph.get_mut(saw.root).name = "sawhorse".into();
    graph.add(props, saw.root);
    let level = common::level("sierra");
    let track = Track::new(&level).expect("track");
    let spikes = spike_strip(
        &mut graph,
        &mut kit,
        &track,
        track.start_s + 200.0,
        -4.0,
        4.0,
    );
    graph.get_mut(spikes).name = "spike-strip".into();
    graph.add(props, spikes);
    graph.add_root(root);
    assert!(graph.log.is_empty(), "the build logged {:?}", graph.log);
    let (s, h) = graph.finish();
    let p = h.node(props).expect("the props group");
    (s, p)
}

/// One prop's node lines and the materials each node uses.
type PropView = (String, Vec<String>, Vec<Vec<i64>>);

/// The props of a group, and the materials in order of first use.
fn views(s: &Scene, d: &SceneDigest, group: u32) -> (Vec<PropView>, Vec<u32>) {
    fn walk(
        c: u32,
        s: &Scene,
        d: &SceneDigest,
        lines: &mut Vec<String>,
        mats: &mut Vec<Vec<i64>>,
        table: &mut Vec<u32>,
    ) {
        let n = &s.nodes[c as usize];
        lines.push(node_line(s, d, c));
        mats.push(
            n.materials
                .iter()
                .map(|&m| match table.iter().position(|&x| x == m) {
                    Some(k) => k as i64,
                    None => {
                        table.push(m);
                        table.len() as i64 - 1
                    }
                })
                .collect(),
        );
        for &k in &n.children {
            walk(k, s, d, lines, mats, table);
        }
    }
    let mut table: Vec<u32> = Vec::new();
    let mut out = Vec::new();
    for &c in &s.nodes[group as usize].children {
        let mut lines = Vec::new();
        let mut mats = Vec::new();
        walk(c, s, d, &mut lines, &mut mats, &mut table);
        out.push((s.nodes[c as usize].name.clone(), lines, mats));
    }
    (out, table)
}

/// The export's `pursuit-props` group.
fn js_group(s: &Scene) -> u32 {
    let root = &s.nodes[s.roots[0] as usize];
    *root
        .children
        .iter()
        .find(|&&i| s.nodes[i as usize].name == "pursuit-props")
        .expect("pursuit-props in the export")
}

#[test]
fn props_match_the_js_export() {
    let g: Value = serde_json::from_str(GOLDEN).expect("props golden parses");
    let (s, group) = build();
    let d = digest(&s);
    let (ours, table) = views(&s, &d, group);
    let js = cached();
    let js_views = js.as_ref().map(|(js, jd)| views(js, jd, js_group(js)));
    let want = g["props"].as_array().expect("props");
    assert_eq!(ours.len(), want.len(), "props built");
    let mut problems = Vec::new();
    for (k, ((name, lines, mats), w)) in ours.iter().zip(want).enumerate() {
        assert_eq!(Some(name.as_str()), w["name"].as_str());
        let wl: Vec<&str> = w["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .map(|l| l.as_str().expect("line"))
            .collect();
        if lines.len() != wl.len() {
            problems.push(format!(
                "{name}: {} nodes here, the JS {}",
                lines.len(),
                wl.len()
            ));
        }
        for (i, (o, j)) in lines.iter().zip(&wl).enumerate() {
            if o != j {
                problems.push(format!("{name} node {i}:\n  here {o}\n  JS   {j}"));
            }
        }
        if let Some((jv, _)) = &js_views {
            let jl: Vec<&str> = jv[k].1.iter().map(String::as_str).collect();
            assert_eq!(jl, wl, "the golden is the cache's");
        }
        let wm: Vec<Vec<i64>> = serde_json::from_value(w["mats"].clone()).expect("mats");
        if *mats != wm {
            problems.push(format!("{name}: materials used {mats:?}, the JS {wm:?}"));
        }
    }
    let wmats = g["materials"].as_array().expect("materials");
    assert_eq!(table.len(), wmats.len(), "materials");
    let mut mats_same = 0;
    for (k, (&m, w)) in table.iter().zip(wmats).enumerate() {
        let view = material_view(&s, &d, m);
        let sha = sha256_hex(
            serde_json::to_string(&canon(&view))
                .expect("json")
                .as_bytes(),
        );
        if Some(sha.as_str()) == w["sha256"].as_str() {
            mats_same += 1;
            continue;
        }
        let jsv = js
            .as_ref()
            .zip(js_views.as_ref())
            .map_or(Value::Null, |((js, jd), (_, jt))| {
                material_view(js, jd, jt[k])
            });
        problems.push(format!(
            "material {k}: differs\n  here {view}\n  JS   {jsv}"
        ));
    }
    let wt = g["textures"].as_array().expect("textures");
    let mut ours_t = Vec::new();
    for (k, &m) in table.iter().enumerate() {
        for (key, t) in material_textures(&s, m) {
            ours_t.push((k, key, t));
        }
    }
    assert_eq!(ours_t.len(), wt.len(), "textures");
    for ((k, key, t), w) in ours_t.iter().zip(wt) {
        let tex = &s.textures[*t as usize];
        let (tw, th) = (tex.width as usize, tex.height as usize);
        assert_eq!(w["key"].as_str(), Some(key.as_str()));
        assert_eq!(w["material"].as_u64(), Some(*k as u64));
        assert_eq!(
            (w["width"].as_u64(), w["height"].as_u64()),
            (Some(tw as u64), Some(th as u64))
        );
        let px = rgba(&s, *t);
        let blocks: Vec<[f64; 4]> = w["blocks"]
            .as_array()
            .expect("blocks")
            .iter()
            .map(|b| [0, 1, 2, 3].map(|c| b[c].as_f64().expect("mean")))
            .collect();
        let bd = block_diff(&block_means(tw, th, &px), &blocks);
        let mad = js
            .as_ref()
            .zip(js_views.as_ref())
            .map(|((js, _), (_, jt))| {
                let jtex = material_textures(js, jt[*k])
                    .into_iter()
                    .find(|(kk, _)| kk == key)
                    .expect("the JS texture")
                    .1;
                mr_canvas::compare::mean_abs_diff(&rgba(js, jtex), &px)
            });
        println!("texture of material {k} {key}: block means {bd:.2?}, mean abs diff {mad:.2?}");
        let gate = mad.unwrap_or(bd);
        if gate.iter().any(|&v| v >= LIMIT) {
            problems.push(format!("texture {key}: {gate:.2?} (limit {LIMIT})"));
        }
    }
    println!(
        "pursuit props: {} props, {mats_same} of {} materials identical",
        ours.len(),
        wmats.len()
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
