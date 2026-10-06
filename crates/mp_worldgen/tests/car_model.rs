//! WP 4.1's gates: `CarModel.js` against the browser's models export
//! (SPEC 5.7, L3 per kind; DECISIONS D29, D410 on), and every kind's
//! dimensions against `mp_sim::dims` (SPEC 4.3).
//!
//! The models are built in the export's order into one graph (so the
//! shared materials and geometry are shared as the JS module shares them),
//! each under a group named as the exporter names it, and compared with
//! `parity/golden/car_model/models.json` (`tools/parity/car-model-golden.mjs`):
//! per model one line per node of the vehicle's tree (type, name, every
//! attribute's and the index's SHA-256, draw groups, bounding sphere,
//! flags, local matrix, userData), which materials each node uses, each
//! material in a canonical form (keys sorted, numbers as bits; textures by
//! sampler), and the 8×8 block means of every texture a material uses,
//! within WP 3.2's threshold. The golden is compiled in, so the gate runs
//! in CI and wasm; with the cache (`node tools/parity/scene-export.mjs`), a
//! failing node is shown beside the JS one, a failing material as both
//! views, and each texture's mean absolute difference over every pixel is
//! checked.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use mp_canvas::compare::{block_diff, block_means};
use mp_scene::digest::{SceneDigest, digest, sha256_hex};
use mp_scene::{BufferData, Scene, TextureSource};
use mp_worldgen::car_model::{
    BuildOpts, Lod, SirenMode, VEHICLE_KINDS, VehicleModel, apply_edits, build_vehicle,
    siren_levels,
};
use mp_worldgen::object::SceneGraph;
use mp_worldgen::textures::TextureCache;
use mp_worldgen::world::{Change, Handle};
use serde_json::{Map, Value, json};

const GOLDEN: &str = include_str!("../../../parity/golden/car_model/models.json");
const WORLD_DATA: &str = include_str!("../../../parity/golden/sim/world-data.json");

/// WP 3.2's threshold, per channel.
const LIMIT: f64 = 3.0;

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("car model golden parses")
}

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

/// What the gate compares of one model.
struct ModelView {
    name: String,
    lines: Vec<String>,
    mats: Vec<Vec<i64>>,
    vertices: u64,
}

/// Every `car:` child of the root, as car-model-golden.mjs `models` walks
/// them; `table` collects the materials in order of first use.
fn model_views(s: &Scene, d: &SceneDigest, table: &mut Vec<u32>) -> Vec<ModelView> {
    let root = &s.nodes[s.roots[0] as usize];
    let mut out = Vec::new();
    for &gi in &root.children {
        let g = &s.nodes[gi as usize];
        if !g.name.starts_with("car:") {
            continue;
        }
        let mut v = ModelView {
            name: g.name.clone(),
            lines: Vec::new(),
            mats: Vec::new(),
            vertices: 0,
        };
        fn walk(c: u32, s: &Scene, d: &SceneDigest, v: &mut ModelView, table: &mut Vec<u32>) {
            let n = &s.nodes[c as usize];
            v.lines.push(node_line(s, d, c));
            let mk = n
                .materials
                .iter()
                .map(|&m| match table.iter().position(|&x| x == m) {
                    Some(k) => k as i64,
                    None => {
                        table.push(m);
                        table.len() as i64 - 1
                    }
                })
                .collect();
            v.mats.push(mk);
            if let Some(mesh) = n.mesh {
                v.vertices += d.meshes[mesh as usize].vertices;
            }
            for &k in &n.children {
                walk(k, s, d, v, table);
            }
        }
        for &c in &g.children {
            walk(c, s, d, &mut v, table);
        }
        out.push(v);
    }
    out
}

/// A model's options as the exporter recorded them.
fn opts_of(o: &Value) -> BuildOpts {
    BuildOpts {
        lod: match o["lod"].as_str() {
            Some("high") => Some(Lod::High),
            Some("low") => Some(Lod::Low),
            _ => None,
        },
        far: o["far"].as_bool().unwrap_or(false),
        seed: o["seed"].as_u64().unwrap_or(0) as u32,
        police_livery: o["livery"].as_str() == Some("police"),
        ..BuildOpts::default()
    }
}

/// The models of the golden, built in its order into one graph under a
/// root `models`, as `exportModels` builds them.
fn build_models(g: &Value) -> (SceneGraph, Vec<VehicleModel>) {
    let mut graph = SceneGraph::new();
    let mut tex = TextureCache::new();
    let root = graph.group("models");
    let mut handles = Vec::new();
    for m in g["models"].as_array().expect("models") {
        let h = build_vehicle(
            &mut graph,
            &mut tex,
            m["kind"].as_str().expect("kind"),
            &opts_of(&m["opts"]),
        )
        .expect("a known kind");
        let grp = graph.group(m["name"].as_str().expect("name"));
        graph.add(grp, h.root);
        graph.add(root, grp);
        handles.push(h);
    }
    graph.add_root(root);
    (graph, handles)
}

/// The cached JS export and its digest, if there.
fn cached() -> Option<(Scene, SceneDigest)> {
    if cfg!(target_arch = "wasm32") {
        return None;
    }
    let dir = mp_scene::cache::scenes_dir(&common::root()).ok()?;
    let scene = dir.join("models.mrscene");
    let dig = dir.join("models.digest.json");
    if !scene.exists() || !dig.exists() {
        return None;
    }
    let s = mp_scene::read_file(&scene).ok()?;
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

#[cfg(not(target_arch = "wasm32"))]
fn write_sheet(name: &str, w: usize, h: usize, js: &[u8], rust: &[u8]) {
    let dir = common::root().join("parity/report/car_model");
    std::fs::create_dir_all(&dir).expect("report dir");
    let (sw, sh, px) = mp_canvas::compare::sheet(w, h, js, rust);
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
fn models_match_the_js_export() {
    let g = golden();
    let (graph, _) = build_models(&g);
    assert!(graph.log.is_empty(), "the build logged {:?}", graph.log);
    let (s, _) = graph.finish();
    let d = digest(&s);
    let mut table = Vec::new();
    let views = model_views(&s, &d, &mut table);
    let js = cached();
    let mut js_table = Vec::new();
    let js_views = js
        .as_ref()
        .map(|(js, jd)| model_views(js, jd, &mut js_table));
    let want = g["models"].as_array().expect("models");
    assert_eq!(views.len(), want.len(), "models built");
    let mut problems = Vec::new();
    let mut rows = Vec::new();
    for (k, (v, w)) in views.iter().zip(want).enumerate() {
        let name = w["name"].as_str().expect("name");
        assert_eq!(v.name, name);
        let wl: Vec<&str> = w["lines"]
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
        let before = problems.len();
        if v.lines.len() != wl.len() {
            problems.push(format!(
                "{name}: {} nodes here, the JS {}",
                v.lines.len(),
                wl.len()
            ));
        }
        let mut same = 0;
        for (i, (o, j)) in ours.iter().zip(&wl).enumerate() {
            if o == j {
                same += 1;
                continue;
            }
            let jsl = js_views
                .as_ref()
                .map_or("(no cache)".to_string(), |jv| jv[k].lines[i].clone());
            problems.push(format!(
                "{name} node {i}:\n  here {}\n  JS   {jsl}",
                v.lines[i]
            ));
        }
        let wm: Vec<Vec<i64>> = w["mats"]
            .as_array()
            .expect("mats")
            .iter()
            .map(|m| {
                m.as_array()
                    .expect("list")
                    .iter()
                    .map(|x| x.as_i64().expect("index"))
                    .collect()
            })
            .collect();
        if v.mats != wm {
            problems.push(format!(
                "{name}: materials used {:?}, the JS {:?}",
                v.mats, wm
            ));
        }
        if v.vertices != w["vertices"].as_u64().expect("vertices") {
            problems.push(format!(
                "{name}: {} vertices, the JS {}",
                v.vertices, w["vertices"]
            ));
        }
        rows.push(format!(
            "{name}: {same} of {} nodes identical, {} vertices{}",
            wl.len(),
            v.vertices,
            if problems.len() == before {
                ""
            } else {
                " — FAILS"
            }
        ));
    }

    // Materials, in order of first use.
    let wmats = g["materials"].as_array().expect("materials");
    if table.len() != wmats.len() {
        problems.push(format!(
            "{} materials here, the JS {}",
            table.len(),
            wmats.len()
        ));
    }
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
            .map_or(Value::Null, |(js, jd)| material_view(js, jd, js_table[k]));
        problems.push(format!(
            "material {k} ({}): differs\n  here {view}\n  JS   {jsv}",
            w["kind"]
        ));
    }

    // Textures within WP 3.2's threshold.
    let wt = g["textures"].as_array().expect("textures");
    let mut ours_t = Vec::new();
    for (k, &m) in table.iter().enumerate() {
        for (key, t) in material_textures(&s, m) {
            ours_t.push((k, key, t));
        }
    }
    if ours_t.len() != wt.len() {
        problems.push(format!(
            "{} textures here, the JS {}",
            ours_t.len(),
            wt.len()
        ));
    }
    for (i, ((k, key, t), w)) in ours_t.iter().zip(wt).enumerate() {
        let tex = &s.textures[*t as usize];
        let (tw, th) = (tex.width as usize, tex.height as usize);
        if w["material"].as_u64() != Some(*k as u64)
            || w["key"].as_str() != Some(key.as_str())
            || w["width"].as_u64() != Some(tw as u64)
            || w["height"].as_u64() != Some(th as u64)
        {
            problems.push(format!(
                "texture {i}: material {k} {key} {tw}×{th}, the JS {w}"
            ));
            continue;
        }
        let px = rgba(&s, *t);
        let blocks: Vec<[f64; 4]> = w["blocks"]
            .as_array()
            .expect("blocks")
            .iter()
            .map(|b| [0, 1, 2, 3].map(|c| b[c].as_f64().expect("mean")))
            .collect();
        let bd = block_diff(&block_means(tw, th, &px), &blocks);
        let mad = js.as_ref().map(|(js, _)| {
            let jt = material_textures(js, js_table[*k])
                .into_iter()
                .find(|(kk, _)| kk == key)
                .expect("the JS texture")
                .1;
            let jpx = rgba(js, jt);
            write_sheet(&format!("texture-{i}"), tw, th, &jpx, &px);
            mp_canvas::compare::mean_abs_diff(&jpx, &px)
        });
        let gate = mad.unwrap_or(bd);
        if gate.iter().any(|&v| v >= LIMIT) {
            problems.push(format!(
                "texture {i} ({tw}×{th}): {} {gate:.2?} (limit {LIMIT})",
                if mad.is_some() {
                    "mean abs diff"
                } else {
                    "block means differ by"
                }
            ));
        }
        rows.push(format!(
            "texture {i} (material {k} {key}, {tw}×{th}): block means {bd:.2?}, mean abs diff {}",
            mad.map_or("(no cache)".into(), |m| format!("{m:.2?}"))
        ));
    }
    println!(
        "car models: {} models, {mats_same} of {} materials identical",
        views.len(),
        wmats.len()
    );
    for r in &rows {
        println!("  {r}");
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// SPEC 4.3: every kind's dimensions, at both detail levels, equal
/// `mp_sim::dims` (and the world-data dump the table was checked
/// against).
#[test]
fn dims_equal_mr_sim() {
    let wd: Value = serde_json::from_str(WORLD_DATA).expect("world data parses");
    let mut graph = SceneGraph::new();
    let mut tex = TextureCache::new();
    for kind in VEHICLE_KINDS {
        let sim = mp_sim::dims::dims(kind).expect("mp_sim has the kind");
        for lod in [Lod::High, Lod::Low] {
            let h = build_vehicle(
                &mut graph,
                &mut tex,
                kind,
                &BuildOpts {
                    lod: Some(lod),
                    far: lod == Lod::Low,
                    ..BuildOpts::default()
                },
            )
            .expect("a known kind");
            let d = h.dims;
            assert_eq!(
                (
                    d.length,
                    d.width,
                    d.height,
                    d.wheel_radius,
                    d.wheel_base,
                    Some(d.track)
                ),
                (
                    sim.length,
                    sim.width,
                    sim.height,
                    sim.wheel_radius,
                    sim.wheel_base,
                    sim.track
                ),
                "{kind} {lod:?}"
            );
            for (key, js) in wd["kinds"][kind].as_object().expect("dumped kind") {
                let f = |k: &str| js[k].as_f64().expect("a dimension");
                assert_eq!(
                    [
                        d.length,
                        d.width,
                        d.height,
                        d.wheel_radius,
                        d.wheel_base,
                        d.track
                    ],
                    [
                        f("length"),
                        f("width"),
                        f("height"),
                        f("wheelRadius"),
                        f("wheelBase"),
                        f("track")
                    ],
                    "{kind} against the dump's {key}"
                );
            }
        }
    }
    assert!(build_vehicle(&mut graph, &mut tex, "hovercraft", &BuildOpts::default()).is_none());
}

/// The light setters as `buildVehicle`'s handle has them.
#[test]
fn light_setters() {
    let mut graph = SceneGraph::new();
    let mut tex = TextureCache::new();
    let number = |e: &mp_worldgen::world::Edit| match e.change {
        Change::Number { value, .. } => value,
        _ => panic!("a number"),
    };
    let mut car =
        build_vehicle(&mut graph, &mut tex, "electric", &BuildOpts::default()).expect("electric");
    let e = car.set_headlights(1.0);
    assert_eq!(e.len(), 2);
    assert_eq!(number(&e[0]), 0.3 + 1.0 * 2.7);
    assert_eq!(number(&e[1]), 0.3 + 0.9);
    let e = car.set_brake(2.0);
    assert_eq!(number(&e[0]), 4.0);
    let e = car.set_reverse(true);
    assert_eq!(
        (e[0].target, number(&e[0])),
        (Handle::Material(car.rev), 2.5)
    );
    let e = car.set_boost(0.5);
    assert_eq!(number(&e[0]), 1.4 + 5.0 * 0.5);
    apply_edits(&mut graph, &e);
    assert_eq!(
        graph
            .material(car.accent.expect("accent"))
            .number("emissiveIntensity"),
        Some(3.9)
    );
    assert!(car.set_siren(SirenMode::Flash, 0.0).is_empty(), "no siren");
    assert!(car.set_far(true).is_empty(), "no far model");

    let mut cop = build_vehicle(
        &mut graph,
        &mut tex,
        "police",
        &BuildOpts {
            far: true,
            ..BuildOpts::default()
        },
    )
    .expect("police");
    assert_eq!(siren_levels(SirenMode::Flash, 0.0), (1.0, 0.0));
    assert_eq!(siren_levels(SirenMode::Flash, 0.45), (0.0, 1.0));
    assert_eq!(siren_levels(SirenMode::Flash, 0.06), (0.0, 0.0));
    assert_eq!(siren_levels(SirenMode::Steady, 3.0), (0.55, 0.55));
    assert_eq!(siren_levels(SirenMode::Disabled, 3.0), (0.3, 0.0));
    let e = cop.set_siren(SirenMode::Steady, 0.0);
    apply_edits(&mut graph, &e);
    let siren = cop.siren.expect("a siren");
    assert!(graph.get(siren.glow).visible);
    assert_eq!(
        graph.material(siren.red).number("emissiveIntensity"),
        Some(0.55 * 6.0)
    );
    let u = graph
        .material(siren.glow_material)
        .desc
        .uniforms
        .clone()
        .expect("uniforms");
    assert_eq!(
        u["uBlue"],
        json!({ "color": [0.1 * 0.55, 0.35 * 0.55, 3.2 * 0.55] })
    );
    assert_eq!(cop.siren_color(), (0.55, 0.55));
    let e = cop.set_far(true);
    let far = cop.far.clone().expect("a far model");
    assert_eq!(e.len(), 1 + far.near.len());
    apply_edits(&mut graph, &e);
    assert!(graph.get(far.mesh).visible && cop.is_far());
    assert!(far.near.iter().all(|&n| !graph.get(n).visible));
    assert!(cop.set_far(true).is_empty(), "already far");
}
