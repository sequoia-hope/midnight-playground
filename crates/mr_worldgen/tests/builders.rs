//! WP 3.3's gate: the builders against the JS builders run under Node with
//! the parity kernel (`parity/golden/builders/builders.json`, written by
//! `tools/parity/builders.mjs`): `Builder`, `PaintBuilder`, `ColorBuilder`,
//! `GeoBuilder`, `staticMesh`, `instanced`, `trs`, `yawOf`, Road.js's
//! `extrude` and `runs` over real level tracks, `THREE.Color`, the
//! built-in materials' parameters and World.build's progress labels. The
//! cases are made here exactly as the tool makes them, in the same order;
//! read the two side by side.
//!
//! The requirement is bit-identical: the same meshes with the same names,
//! flags, matrices and materials, the same attributes in the same order
//! with the same `f32` bits, the same index, bounds and instance arrays.
//! The golden is compiled in, so the test runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use core::f64::consts::PI;

use mr_math::kernel;
use mr_scene::{BufferData, NodeType};
use mr_track::{Frame, Track};
use mr_worldgen::builder::{BuildOpts, Builder, CastShadow};
use mr_worldgen::color::Color;
use mr_worldgen::geom::{GeoBuilder, PrismOpts, StaticOpts, instanced, static_mesh, trs, yaw_of};
use mr_worldgen::material::{Material, Param};
use mr_worldgen::object::{MaterialId, NodeId, SceneGraph};
use mr_worldgen::road::{ExtrudeColor, ExtrudeOpts, Lat, ProfilePoint, extrude, runs};
use mr_worldgen::three_geom::*;
use mr_worldgen::world::{
    Build, Job, Scenery, SceneryInfo, Stages, level_jobs, scenery_label, scenery_modules,
};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/builders/builders.json");
const HEAD: usize = 16;

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn hex64(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn type_name(d: &BufferData) -> &'static str {
    match d {
        BufferData::F32(_) => "f32",
        BufferData::F64(_) => "f64",
        BufferData::U8(_) => "u8",
        BufferData::U16(_) => "u16",
        BufferData::U32(_) => "u32",
        BufferData::I8(_) => "i8",
        BufferData::I16(_) => "i16",
        BufferData::I32(_) => "i32",
    }
}

fn head_of(d: &BufferData) -> Vec<Value> {
    (0..d.len().min(HEAD))
        .map(|i| match d {
            BufferData::F32(a) => Value::from(format!("{:08x}", a[i].to_bits())),
            BufferData::F64(a) => Value::from(hex64(a[i])),
            _ => Value::from(d.get(i)),
        })
        .collect()
}

/// A typed array against the tool's `attr()`.
fn check_array(
    what: &str,
    array: &BufferData,
    item_size: usize,
    normalized: bool,
    w: &Value,
    errors: &mut Vec<String>,
) {
    if item_size as u64 != w["itemSize"].as_u64().unwrap()
        || type_name(array) != w["type"].as_str().unwrap()
        || normalized != w["normalized"].as_bool().unwrap()
    {
        errors.push(format!(
            "{what}: item size, type or normalized differ ({item_size} {} vs {} {})",
            type_name(array),
            w["itemSize"],
            w["type"]
        ));
        return;
    }
    if array.len() as u64 != w["n"].as_u64().unwrap() {
        errors.push(format!("{what}: {} values, JS has {}", array.len(), w["n"]));
        return;
    }
    let head = head_of(array);
    let whead = w["head"].as_array().unwrap();
    if &head != whead {
        errors.push(format!(
            "{what}: first values differ\n  rust {head:?}\n  js   {whead:?}"
        ));
        return;
    }
    if format!("{:016x}", fnv1a64(&array.to_le_bytes())) != w["hash"].as_str().unwrap() {
        errors.push(format!("{what}: values differ past the first {HEAD}"));
    }
}

fn hexes(v: &[f64]) -> Vec<String> {
    v.iter().map(|&x| hex64(x)).collect()
}

fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
}

fn check_geo(name: &str, g: Option<&BufferGeometry>, want: &Value, errors: &mut Vec<String>) {
    let Some(g) = g else {
        if want.get("null").is_none() {
            errors.push(format!("{name}: Rust gave none, JS gave a geometry"));
        }
        return;
    };
    if want.get("null").is_some() {
        errors.push(format!("{name}: JS gave null"));
        return;
    }
    let attrs = want["attrs"].as_array().unwrap();
    let names: Vec<&str> = g.attributes.iter().map(|(n, _)| n.as_str()).collect();
    let want_names: Vec<&str> = attrs.iter().map(|a| a["name"].as_str().unwrap()).collect();
    if names != want_names {
        errors.push(format!(
            "{name}: attributes {names:?}, JS has {want_names:?}"
        ));
        return;
    }
    for ((an, a), w) in g.attributes.iter().zip(attrs) {
        check_array(
            &format!("{name} {an}"),
            &a.array,
            a.item_size,
            a.normalized,
            w,
            errors,
        );
    }
    match (&g.index, want["index"].is_null()) {
        (None, true) => {}
        (Some(_), true) => errors.push(format!("{name}: indexed, JS is not")),
        (None, false) => errors.push(format!("{name}: not indexed, JS is")),
        (Some(ix), false) => {
            let w = &want["index"];
            let vals: Vec<u32> = (0..ix.array.len())
                .map(|i| ix.array.get(i) as u32)
                .collect();
            let bytes: Vec<u8> = vals.iter().flat_map(|v| v.to_le_bytes()).collect();
            let head: Vec<u64> = vals.iter().take(HEAD).map(|&v| v as u64).collect();
            let whead: Vec<u64> = w["head"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap())
                .collect();
            if type_name(&ix.array) != w["type"].as_str().unwrap()
                || vals.len() as u64 != w["n"].as_u64().unwrap()
                || head != whead
                || format!("{:016x}", fnv1a64(&bytes)) != w["hash"].as_str().unwrap()
            {
                errors.push(format!("{name}: index differs"));
            }
        }
    }
    let groups: Vec<Value> = g
        .groups
        .iter()
        .map(|x| {
            Value::from(vec![
                x.start as u64,
                x.count as u64,
                x.material_index as u64,
            ])
        })
        .collect();
    if &groups != want["groups"].as_array().unwrap() {
        errors.push(format!("{name}: groups {groups:?}, JS {}", want["groups"]));
    }
    let bbox = g
        .bounding_box
        .map(|b| hexes(&[b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]));
    if bbox != strings(want.get("bbox")) {
        errors.push(format!(
            "{name}: bounding box {bbox:?}, JS {:?}",
            want.get("bbox")
        ));
    }
    let bs = g
        .bounding_sphere
        .map(|s| hexes(&[s.center.x, s.center.y, s.center.z, s.radius]));
    if bs != strings(want.get("bsphere")) {
        errors.push(format!(
            "{name}: bounding sphere {bs:?}, JS {:?}",
            want.get("bsphere")
        ));
    }
}

fn check_seq(name: &str, values: &[f64], want: &Value, errors: &mut Vec<String>) {
    if values.len() as u64 != want["n"].as_u64().unwrap() {
        errors.push(format!(
            "{name}: {} values, JS has {}",
            values.len(),
            want["n"]
        ));
        return;
    }
    for (i, h) in want["head"].as_array().unwrap().iter().enumerate() {
        if hex64(values[i]) != h.as_str().unwrap() {
            let w = f64::from_bits(u64::from_str_radix(h.as_str().unwrap(), 16).unwrap());
            errors.push(format!("{name}[{i}]: {} vs JS {w}", values[i]));
            return;
        }
    }
    let bytes: Vec<u8> = values
        .iter()
        .map(|&v| if v.is_nan() { f64::NAN } else { v })
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect();
    if format!("{:016x}", fnv1a64(&bytes)) != want["hash"].as_str().unwrap() {
        errors.push(format!("{name}: values differ past the first {HEAD}"));
    }
}

/// JSON values equal, numbers by their f64 bits, object keys in order.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x.as_f64().map(f64::to_bits) == y.as_f64().map(f64::to_bits)
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((kx, vx), (ky, vy))| kx == ky && same(vx, vy))
        }
        _ => a == b,
    }
}

fn check_material(name: &str, m: &Material, want: &Value, errors: &mut Vec<String>) {
    if m.desc.ty != want["type"].as_str().unwrap() {
        errors.push(format!("{name}: type {}, JS {}", m.desc.ty, want["type"]));
        return;
    }
    let got = Value::Object(m.desc.params.clone());
    if !same(&got, &want["params"]) {
        let w = want["params"].as_object().unwrap();
        let first = m
            .desc
            .params
            .iter()
            .zip(w)
            .find(|((k, v), (wk, wv))| k != wk || !same(v, wv));
        errors.push(format!(
            "{name}: parameters differ ({} vs {} keys), first: {first:?}",
            m.desc.params.len(),
            w.len()
        ));
    }
}

/// Meshes a builder made, against the tool's `meshes()`.
fn check_meshes(
    name: &str,
    graph: &SceneGraph,
    list: &[NodeId],
    mats: &[(&str, MaterialId)],
    want: &Value,
    errors: &mut Vec<String>,
) {
    let wm = want["meshes"].as_array().unwrap();
    if list.len() != wm.len() {
        errors.push(format!(
            "{name}: {} meshes, JS made {}",
            list.len(),
            wm.len()
        ));
        return;
    }
    let mut seen: Vec<MaterialId> = Vec::new();
    for (k, (&id, w)) in list.iter().zip(wm).enumerate() {
        let o = graph.get(id);
        let what = format!("{name} mesh {k} ({})", o.name);
        let ty = match o.ty {
            NodeType::InstancedMesh => "InstancedMesh",
            NodeType::Mesh => "Mesh",
            _ => "?",
        };
        if o.name != w["name"].as_str().unwrap()
            || ty != w["type"].as_str().unwrap()
            || o.cast_shadow != w["cast"].as_bool().unwrap()
            || o.receive_shadow != w["receive"].as_bool().unwrap()
            || o.matrix_auto_update != w["auto"].as_bool().unwrap()
        {
            errors.push(format!(
                "{what}: name, type or flags differ: {} {ty} cast {} receive {} auto {}, JS {}",
                o.name, o.cast_shadow, o.receive_shadow, o.matrix_auto_update, w
            ));
        }
        if Some(hexes(&o.matrix.elements)) != strings(w.get("matrix")) {
            errors.push(format!("{what}: matrix differs"));
        }
        let m = o.materials[0];
        let key = match mats.iter().find(|(_, x)| *x == m) {
            Some((k, _)) => k.to_string(),
            None => {
                if !seen.contains(&m) {
                    seen.push(m);
                }
                format!("new{}", seen.iter().position(|x| *x == m).unwrap())
            }
        };
        if key != w["material"].as_str().unwrap() {
            errors.push(format!("{what}: material {key}, JS {}", w["material"]));
        }
        let g = graph.geometry(o.geometry.unwrap());
        check_geo(&what, Some(g), &w["geo"], errors);
        if let Some(inst) = &o.instances {
            if u64::from(inst.count) != w["count"].as_u64().unwrap()
                || u64::from(inst.capacity()) != w["capacity"].as_u64().unwrap()
            {
                errors.push(format!("{what}: instance count or capacity differ"));
            }
            check_array(
                &format!("{what} instanceMatrix"),
                &BufferData::F32(inst.matrices.clone()),
                16,
                false,
                &w["matrices"],
                errors,
            );
            match (&inst.colors, w["colors"].is_null()) {
                (None, true) => {}
                (Some(c), false) => check_array(
                    &format!("{what} instanceColor"),
                    &BufferData::F32(c.clone()),
                    3,
                    false,
                    &w["colors"],
                    errors,
                ),
                _ => errors.push(format!("{what}: instance colours present on one side only")),
            }
            let bs = inst
                .bounding_sphere
                .map(|s| hexes(&[s.center.x, s.center.y, s.center.z, s.radius]));
            if bs != strings(w.get("bsphere")) {
                errors.push(format!(
                    "{what}: bounding sphere {bs:?}, JS {}",
                    w["bsphere"]
                ));
            }
        }
    }
    let wmats = want["materials"].as_array().unwrap();
    if seen.len() != wmats.len() {
        errors.push(format!(
            "{name}: {} new materials, JS {}",
            seen.len(),
            wmats.len()
        ));
    } else {
        for (m, w) in seen.iter().zip(wmats) {
            check_material(
                &format!("{name} {}", w["id"]),
                graph.material(*m),
                w,
                errors,
            );
        }
    }
    let warnings: Vec<&str> = want["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    if graph.log != warnings {
        errors.push(format!(
            "{name}: log {:?}, JS warned {warnings:?}",
            graph.log
        ));
    }
}

// ── The cases ───────────────────────────────────────────────────────────

enum Out {
    Geo(Option<BufferGeometry>),
    Seq(Vec<f64>),
    Strings(Vec<String>),
    Materials(Vec<Material>),
    Meshes(SceneGraph, Vec<NodeId>, Vec<(&'static str, MaterialId)>),
    Progress(Vec<(String, Vec<(String, f64)>)>),
}

struct Cases(Vec<(String, Out)>);

impl Cases {
    fn g(&mut self, name: &str, g: Option<BufferGeometry>) {
        self.0.push((name.to_string(), Out::Geo(g)));
    }
    fn s(&mut self, name: &str, v: Vec<f64>) {
        self.0.push((name.to_string(), Out::Seq(v)));
    }
    fn o(&mut self, name: &str, o: Out) {
        self.0.push((name.to_string(), o));
    }
}

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn rgb(c: Color) -> [f64; 3] {
    [c.r, c.g, c.b]
}

const HEXES: [u32; 12] = [
    0x000000, 0xffffff, 0x4a3a2c, 0xd9d0bc, 0x0a0b0c, 0x808080, 0x123456, 0xfedcba, 0x010203,
    0x0b0b0b, 0x8a6a4a, 0xff7f00,
];

fn color_cases(c: &mut Cases) {
    c.s(
        "color/hex",
        HEXES.iter().flat_map(|&h| rgb(Color::hex(h))).collect(),
    );
    c.s(
        "color/hex-frac",
        [255.7, f64::from(0x123456) + 0.9, -1.0, f64::from(0x1ffffff)]
            .iter()
            .flat_map(|&h| {
                let mut c = Color::default();
                c.set_hex(h);
                rgb(c)
            })
            .collect(),
    );
    let hsl: [[f64; 3]; 8] = [
        [0.0, 0.0, 0.5],
        [0.1, 0.6, 0.3],
        [0.55, 1.0, 0.7],
        [-0.2, 0.5, 0.5],
        [1.3, 0.4, 0.2],
        [0.9, 1.2, -0.1],
        [0.33, 0.25, 0.5],
        [0.66, 0.8, 0.9],
    ];
    c.s(
        "color/hsl",
        hsl.iter()
            .flat_map(|&[h, s, l]| {
                let mut c = Color::default();
                c.set_hsl(h, s, l);
                rgb(c)
            })
            .collect(),
    );
    c.s(
        "color/get-hsl",
        HEXES
            .iter()
            .flat_map(|&h| {
                let o = Color::hex(h).get_hsl();
                [o.h, o.s, o.l]
            })
            .collect(),
    );
    c.s(
        "color/offset-hsl",
        HEXES
            .iter()
            .flat_map(|&h| rgb(*Color::hex(h).offset_hsl(0.05, -0.1, 0.02)))
            .collect(),
    );
    c.s(
        "color/get-hex",
        HEXES
            .iter()
            .map(|&h| f64::from(Color::hex(h).multiply_scalar(0.7).get_hex()))
            .collect(),
    );
    c.s(
        "color/round-trip",
        HEXES
            .iter()
            .map(|&h| f64::from(Color::hex(h).get_hex()))
            .collect(),
    );
    let styles = [
        "#abc",
        "#a1b2c3",
        "rgb(10,20,30)",
        "rgb( 255 , 0 , 300 )",
        "rgb(10%,50%,100%)",
        "rgba(1,2,3,0.5)",
        "hsl(120,50%,25%)",
        "hsl(300.5, 20%, 75.5%)",
        "#12",
        "nonsense",
    ];
    c.s(
        "color/style",
        styles
            .iter()
            .flat_map(|s| rgb(*Color::hex(0x336699).set_style(s)))
            .collect(),
    );
    let a = Color::hex(0x4a3a2c);
    let b = Color::hex(0xd9d0bc);
    let mut ops = Vec::new();
    ops.extend(rgb(*a.clone().lerp(b, 0.3)));
    ops.extend(rgb(*Color::default().lerp_colors(a, b, 0.65)));
    ops.extend(rgb(*a.clone().lerp_hsl(b, 0.4)));
    ops.extend(rgb(*a.clone().multiply(b)));
    ops.extend(rgb(*a.clone().add(b).add_scalar(-0.1)));
    ops.extend(rgb(*Color::new(0.2, 0.5, 0.9).convert_srgb_to_linear()));
    ops.extend(rgb(*Color::new(0.2, 0.5, 0.9).convert_linear_to_srgb()));
    ops.extend(rgb(*Color::default().set_scalar(0.3)));
    ops.extend(rgb(*Color::default()
        .set_rgb(3.5, 2.4, 1.2)
        .multiply_scalar(0.25 + 0.6)));
    c.s("color/ops", ops);
    c.o(
        "color/hex-string",
        Out::Strings(
            HEXES
                .iter()
                .map(|&h| Color::hex(h).offset_hsl(0.02, 0.0, -0.05).get_hex_string())
                .collect(),
        ),
    );
}

fn material_cases(c: &mut Cases) {
    c.o(
        "material/defaults",
        Out::Materials(vec![
            Material::standard(),
            Material::physical(),
            Material::lambert(),
            Material::basic(),
            Material::line_basic(),
            Material::sprite(),
            Material::points(),
        ]),
    );
    const DOUBLE_SIDE: u32 = 2;
    const ADDITIVE_BLENDING: u32 = 2;
    c.o(
        "material/made",
        Out::Materials(vec![
            Material::standard()
                .set("color", 0x8a6a4a)
                .set("roughness", 0.9)
                .set("metalness", 0.1),
            Material::standard()
                .set("vertexColors", true)
                .set("roughness", 0.85),
            Material::standard()
                .set("color", "#a1b2c3")
                .set("emissive", 0xffaa33)
                .set("emissiveIntensity", 0.4)
                .set("side", DOUBLE_SIDE)
                .set("transparent", true)
                .set("opacity", 0.5)
                .set("depthWrite", false),
            Material::standard().set("size", 3).set("flatShading", true),
            Material::basic()
                .set("color", 0xffffff)
                .set("fog", false)
                .set("toneMapped", false),
            Material::lambert()
                .set("color", 0x335522)
                .set("flatShading", true)
                .set("alphaTest", 0.5),
            Material::points()
                .set("size", 2)
                .set("sizeAttenuation", false)
                .set("color", 0xffcc88)
                .set("transparent", true)
                .set("blending", ADDITIVE_BLENDING),
            Material::physical()
                .set("color", 0x112233)
                .set("clearcoat", 1)
                .set("clearcoatRoughness", 0.1)
                .set("reflectivity", 0.5)
                .set("sheenColor", 0x445566),
            Material::sprite()
                .set("color", Param::Color(Color::new(0.2, 0.4, 0.6)))
                .set("opacity", 0.8),
            Material::line_basic()
                .set("color", 0x00ff00)
                .set("linewidth", 2),
        ]),
    );
}

fn std_mat(g: &mut SceneGraph) -> MaterialId {
    g.add_material(Material::standard())
}

fn pieces_no_normal() -> BufferGeometry {
    // Non-indexed, position only, plus an attribute normalise() drops.
    let mut g = BufferGeometry::new();
    g.set_attribute(
        "position",
        BufferAttribute::from_f64(
            &[
                0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.5, 0.0,
                1.0, 0.0,
            ],
            3,
        ),
    );
    g.set_attribute("color", BufferAttribute::from_f64(&[0.5; 18], 3));
    g
}

fn cylinder(rt: f64, rb: f64, h: f64, radial: f64) -> BufferGeometry {
    cylinder_geometry(rt, rb, h, radial, 1.0, false, 0.0, 2.0 * PI)
}

fn draw_builder(b: &mut Builder) {
    b.box_("wall", 4.0, 3.0, 0.2, 1.0, 0.0, 2.0, 0.0, 0.0, 0.0);
    b.box_("wall", 2.0, 1.0, 1.0, -1.0, 0.5, 0.0, 0.3, 0.1, -0.2);
    b.set_frame(10.0, 2.0, -5.0, 0.7);
    b.cbox("roof", 5.0, 0.2, 4.0, 0.0, 3.0, 0.0, [0.2, 0.0, 0.0]);
    b.push_frame(1.0, 0.0, 1.0, -0.4);
    b.put_at("post", &cylinder(0.1, 0.12, 2.0, 6.0), 0.0, 1.0, 0.0);
    b.put(
        "post",
        &cylinder(0.1, 0.12, 2.0, 6.0),
        0.5,
        1.0,
        0.5,
        [0.1, 0.2, 0.3],
        [1.5, 0.5, 2.0],
    );
    b.beam("beam", v3(0.0, 0.0, 0.0), v3(1.0, 2.0, 3.0), 0.12);
    b.beam("beam", v3(0.0, 0.0, 0.0), v3(0.0, 3.0, 0.0), 0.3);
    b.beam("beam", v3(0.5, 2.0, 0.0), v3(0.5, 0.0, 0.0), 0.12);
    b.push_frame(0.0, 3.0, 0.0, 1.1);
    b.box_yaw("wall", 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 2.5);
    b.pop_frame();
    b.pop_frame();
    b.add("misc", &pieces_no_normal(), None);
    b.add(
        "misc",
        &torus_geometry(1.0, 0.2, 4.0, 8.0, 2.0 * PI),
        Some(&Matrix4::make_translation(0.0, 5.0, 0.0)),
    );
    b.add(
        "misc",
        &lathe_geometry(
            &[
                Vector2::new(0.0, 0.0),
                Vector2::new(1.0, 0.5),
                Vector2::new(0.5, 1.0),
            ],
            5.0,
            0.0,
            2.0 * PI,
        ),
        None,
    );
    b.set_frame(-3.0, 0.0, 4.0, 0.0);
    let curve = CatmullRomCurve3::new(
        vec![v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 0.0), v3(2.0, 0.0, 1.0)],
        false,
        CurveType::Centripetal,
        0.5,
    );
    b.put_at(
        "tube",
        &tube_geometry(&curve, 8.0, 0.1, 4.0, false),
        0.0,
        0.0,
        0.0,
    );
    b.cbox("wall", 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, [0.0; 3]);
}

fn keys(list: &[&str]) -> CastShadow {
    CastShadow::Keys(list.iter().map(|s| s.to_string()).collect())
}

fn builder_cases(c: &mut Cases) {
    {
        let mut b = Builder::new();
        draw_builder(&mut b);
        let mut g = SceneGraph::new();
        let m: Vec<(&str, MaterialId)> = ["wall", "roof", "post", "beam", "misc"]
            .into_iter()
            .map(|k| (k, std_mat(&mut g)))
            .collect();
        let opts = BuildOpts {
            cast_shadow: keys(&["wall", "post"]),
            receive_shadow: true,
        };
        let list = b.build(&mut g, &m, &opts);
        c.o("builder/build", Out::Meshes(g, list, m));
    }
    {
        let mut b = Builder::new();
        b.box_("a", 1.0, 2.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        b.box_yaw("b", 1.0, 2.0, 3.0, 5.0, 0.0, 0.0, 0.5);
        let mut g = SceneGraph::new();
        let m = vec![("a", std_mat(&mut g)), ("b", std_mat(&mut g))];
        let list = b.build(&mut g, &m, &BuildOpts::default());
        c.o("builder/build-default-opts", Out::Meshes(g, list, m));
    }
    {
        let mut b = Builder::new();
        draw_builder(&mut b);
        c.g("builder/merge-all", b.merge_all());
    }
    {
        let mut b = Builder::new_paint(&[
            ("paint", ("p", 0x8a6a4a)),
            ("trim", ("p", 0xeeeeee)),
            ("roof", ("r", 0x553322)),
        ]);
        b.box_("paint", 4.0, 3.0, 0.2, 1.0, 0.0, 2.0, 0.0, 0.0, 0.0);
        b.box_("glass", 1.0, 1.0, 0.05, 1.0, 1.0, 2.1, 0.0, 0.0, 0.0);
        b.set_frame(3.0, 0.0, 3.0, 0.25);
        b.box_("trim", 4.2, 0.2, 0.3, 0.0, 3.0, 0.0, 0.0, 0.0, 0.0);
        b.cbox("roof", 5.0, 0.2, 4.0, 0.0, 3.4, 0.0, [0.2, 0.0, 0.0]);
        b.beam("trim", v3(0.0, 0.0, 0.0), v3(0.3, 2.0, -0.4), 0.1);
        b.put(
            "glass",
            &plane_geometry(1.0, 2.0, 1.0, 1.0),
            0.0,
            1.0,
            0.0,
            [0.0, PI / 2.0, 0.0],
            [1.0; 3],
        );
        b.box_("paint", 2.0, 2.0, 2.0, -2.0, 0.0, 0.0, 0.0, 0.0, 0.3);
        let mut g = SceneGraph::new();
        let m: Vec<(&str, MaterialId)> = ["p", "r", "glass"]
            .into_iter()
            .map(|k| (k, std_mat(&mut g)))
            .collect();
        let opts = BuildOpts {
            cast_shadow: CastShadow::All,
            receive_shadow: false,
        };
        let list = b.build(&mut g, &m, &opts);
        c.o("paint/build", Out::Meshes(g, list, m));
    }
    {
        // As Valley's millSailsGeometry: one painted bucket, merged for instancing.
        let mut b = Builder::new_paint(&[("wood", ("a", 0x4a3a2c)), ("cloth", ("a", 0xd9d0bc))]);
        for k in 0..4 {
            let a = k as f64 * PI / 2.0;
            b.push_frame(0.0, 0.0, 0.0, 0.0);
            b.box_("wood", 0.2, 6.0, 0.1, 0.0, 0.0, 0.0, 0.0, 0.0, a);
            b.cbox("cloth", 1.2, 4.5, 0.02, 0.7, 3.0, 0.0, [0.0, 0.0, a]);
            b.pop_frame();
        }
        c.g("paint/merge-all", b.merge_all());
    }
    {
        let mut b = Builder::new_paint(&[("wood", ("a", 0x4a3a2c))]);
        b.box_("wood", 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        b.box_("bare", 1.0, 1.0, 1.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        c.g("paint/merge-all-mixed", b.merge_all());
    }
}

const PALETTE: [(&str, u32); 3] = [("wall", 0xf2e8d5), ("trim", 0x2f6f8f), ("roof", 0xb5543a)];

fn draw_color(b: &mut Builder) {
    b.box_yaw("wall", 6.0, 4.0, 5.0, 0.0, 0.0, 0.0, 0.1);
    b.box_yaw("trim", 6.2, 0.3, 5.2, 0.0, 4.0, 0.0, 0.1);
    b.box_yaw("glass", 1.2, 1.4, 0.05, 0.0, 1.2, 2.5, 0.1);
    b.set_channel("far");
    b.set_frame(20.0, 0.0, -10.0, 1.2);
    b.box_yaw("wall", 8.0, 6.0, 6.0, 0.0, 0.0, 0.0, 0.0);
    b.cbox("roof", 8.4, 0.3, 6.4, 0.0, 6.15, 0.0, [0.0; 3]);
    b.beam("trim", v3(-4.0, 6.0, -3.0), v3(4.0, 6.0, 3.0), 0.2);
    b.set_channel("near");
    b.set_frame(0.0, 0.0, 0.0, 0.0);
    b.put_at("sign", &plane_geometry(3.0, 1.0, 1.0, 1.0), 0.0, 5.0, 2.6);
    b.box_("roof", 6.4, 0.3, 5.4, 0.0, 4.3, 0.0, 0.1, 0.05, 0.0);
}

fn color_builder_cases(c: &mut Cases) {
    {
        let mut b = Builder::new_color(&PALETTE);
        draw_color(&mut b);
        let mut g = SceneGraph::new();
        let m = vec![("glass", std_mat(&mut g))];
        let opts = BuildOpts {
            cast_shadow: keys(&["glass"]),
            receive_shadow: true,
        };
        let list = b.build(&mut g, &m, &opts);
        c.o("color-builder/build", Out::Meshes(g, list, m));
    }
    {
        let mut b = Builder::new_color(&PALETTE);
        draw_color(&mut b);
        let mut g = SceneGraph::new();
        let m: Vec<(&str, MaterialId)> = ["solid", "glass", "sign"]
            .into_iter()
            .map(|k| (k, std_mat(&mut g)))
            .collect();
        let opts = BuildOpts {
            cast_shadow: CastShadow::All,
            receive_shadow: true,
        };
        let list = b.build(&mut g, &m, &opts);
        c.o("color-builder/build-solid", Out::Meshes(g, list, m));
    }
}

fn draw_geo(g: &mut GeoBuilder) {
    let uv4 = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    g.quad(
        [0.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [2.0, 3.0, 0.0],
        [0.0, 3.0, 0.0],
        Some(uv4),
        Some([0.5, 0.25, 0.125]),
        3.0,
    );
    g.quad(
        [1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [3.0, 2.0, 1.0],
        [1.0, 4.0, 2.0],
        None,
        None,
        0.0,
    );
    g.quad(
        [0.0, 0.0, 0.0],
        [1.0, 1.0, 1.0],
        [2.0, 2.0, 2.0],
        [3.0, 3.0, 3.0],
        None,
        None,
        1.0,
    );
    g.quad(
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [2.0, 0.0, 0.0],
        [1.0, 0.0, 1.0],
        None,
        None,
        0.0,
    );
    g.tri(
        [0.0, 5.0, 0.0],
        [1.0, 5.0, 1.0],
        [2.0, 5.0, 0.0],
        Some([0.9, 0.8, 0.7]),
        2.0,
    );
    g.tri([0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [1.0, 1.0, 1.0], None, 0.0);
    g.tri_uv(
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        None,
        4.0,
    );
    g.tri_uv(
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 1.0],
        [1.0, 1.0, 0.0],
        [0.0, 0.0],
        [0.0, 1.0],
        [1.0, 0.0],
        None,
        0.0,
    );
    g.prism(
        &[[0.0, 0.0], [10.0, 0.0], [10.0, 8.0], [0.0, 8.0]],
        0.0,
        30.0,
        &PrismOpts::default(),
    );
    g.prism(
        &[
            [20.0, 0.0],
            [20.0, 6.0],
            [27.0, 9.0],
            [31.0, 3.0],
            [25.0, -2.0],
        ],
        2.0,
        14.5,
        &PrismOpts {
            tile_w: Some(6.0),
            tile_h: Some(3.5),
            u_off: Some(0.25),
            v_off: Some(0.5),
            v_ref: Some(1.0),
            cell: Some(2.0),
            roof_cell: Some(7.0),
            roof_tile: Some(8.0),
            color: Some([0.3, 0.4, 0.5]),
            roof_color: Some([0.6, 0.6, 0.6]),
            ..PrismOpts::default()
        },
    );
    g.prism(
        &[[0.0, 20.0], [0.0, 26.0], [5.0, 26.0], [5.0, 20.0]],
        0.0,
        4.0,
        &PrismOpts {
            roof: Some(false),
            color: Some([0.1, 0.2, 0.3]),
            ..PrismOpts::default()
        },
    );
    g.prism(
        &[[40.0, 0.0], [44.0, 0.0], [42.0, 3.0]],
        -1.0,
        2.0,
        &PrismOpts {
            v_ref: Some(0.0),
            ..PrismOpts::default()
        },
    );
    g.box_(5.0, 0.0, 5.0, 4.0, 3.0, 2.0, 0.6, &PrismOpts::default());
    g.box_(
        -5.0,
        1.0,
        5.0,
        2.0,
        1.0,
        6.0,
        -2.2,
        &PrismOpts {
            bottom: true,
            color: Some([1.0, 0.5, 0.0]),
            cell: Some(5.0),
            ..PrismOpts::default()
        },
    );
    g.box_(
        0.0,
        0.0,
        -9.0,
        8.0,
        2.0,
        3.0,
        yaw_of(1.0, 2.0),
        &PrismOpts {
            tile_w: Some(2.0),
            roof: Some(false),
            bottom: true,
            ..PrismOpts::default()
        },
    );
    g.box_(
        3.0,
        0.0,
        -3.0,
        1.0,
        1.0,
        1.0,
        PI / 3.0,
        &PrismOpts {
            tile_h: Some(0.5),
            u_off: Some(0.1),
            ..PrismOpts::default()
        },
    );
}

fn trs_default(x: f64, y: f64, z: f64, yaw: f64) -> Matrix4 {
    trs(x, y, z, yaw, 1.0, 1.0, 1.0, 0.0, 0.0)
}

fn geom_cases(c: &mut Cases) {
    for (name, color, cell) in [
        ("geo/color-cell", true, true),
        ("geo/plain", false, false),
        ("geo/color", true, false),
    ] {
        let mut g = GeoBuilder::new(color, cell);
        draw_geo(&mut g);
        c.g(name, Some(g.build()));
    }
    c.g("geo/empty", Some(GeoBuilder::new(false, true).build()));

    let trs_args: [[f64; 9]; 5] = [
        [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0],
        [1.0, 2.0, 3.0, 0.5, 1.0, 1.0, 1.0, 0.0, 0.0],
        [1.0, 2.0, 3.0, -1.2, 2.0, 0.5, 3.0, 0.0, 0.0],
        [5.0, -1.0, 2.0, 0.3, 1.0, 1.0, 1.0, 0.2, -0.1],
        [0.0, 0.0, 0.0, PI, 1.0, 1.0, 1.0, PI / 2.0, 0.4],
    ];
    c.s(
        "trs",
        trs_args
            .iter()
            .flat_map(|a| trs(a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]).elements)
            .collect(),
    );
    c.s(
        "yaw-of",
        [
            [1.0, 0.0],
            [0.0, 1.0],
            [-1.0, 0.0],
            [0.0, -1.0],
            [3.0, 4.0],
            [-2.0, -0.5],
            [0.0, 0.0],
            [-0.0, -0.0],
        ]
        .iter()
        .map(|&[x, z]| yaw_of(x, z))
        .collect(),
    );

    {
        let mut g = SceneGraph::new();
        let geo = g.add_geometry(box_geometry(1.0, 2.0, 1.0, 1.0, 1.0, 1.0));
        let m = std_mat(&mut g);
        let ms = [
            trs_default(1.0, 0.0, 2.0, 0.3),
            trs(-4.0, 1.0, 0.0, 1.2, 2.0, 1.0, 0.5, 0.0, 0.0),
            trs(10.0, 0.0, -3.0, -2.0, 1.0, 3.0, 1.0, 0.1, 0.2),
        ];
        let im = instanced(&mut g, geo, m, &ms, true, false);
        let inst = g.get_mut(im).instances.as_mut().unwrap();
        inst.set_color_at(1, Color::hex(0x336699));
        inst.set_color_at(2, Color::new(0.25, 0.5, 2.0));
        c.o("instanced/some", Out::Meshes(g, vec![im], vec![("m", m)]));
    }
    {
        let mut g = SceneGraph::new();
        let geo = g.add_geometry(cylinder(0.1, 0.1, 1.0, 6.0));
        let m = std_mat(&mut g);
        let im = instanced(&mut g, geo, m, &[], false, true);
        c.o("instanced/none", Out::Meshes(g, vec![im], vec![("m", m)]));
    }
    {
        let mut g = SceneGraph::new();
        let geo = g.add_geometry(sphere_geometry(1.0, 6.0, 4.0, 0.0, 2.0 * PI, 0.0, PI));
        let m = std_mat(&mut g);
        let ms = [
            trs_default(1.0, 1.0, 1.0, 0.0),
            trs(1.0, 1.0, 1.0, 0.0, 2.0, 2.0, 2.0, 0.0, 0.0),
        ];
        let im = instanced(&mut g, geo, m, &ms, false, false);
        c.o(
            "instanced/same-centre",
            Out::Meshes(g, vec![im], vec![("m", m)]),
        );
    }
    {
        let mut g = SceneGraph::new();
        let m = std_mat(&mut g);
        let ga = g.add_geometry(box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let a = static_mesh(&mut g, ga, m, &StaticOpts::default());
        let gb = g.add_geometry(box_geometry(2.0, 1.0, 1.0, 1.0, 1.0, 1.0));
        let b = static_mesh(
            &mut g,
            gb,
            m,
            &StaticOpts {
                cast: true,
                receive: false,
                name: "city:block".into(),
            },
        );
        c.o("static-mesh", Out::Meshes(g, vec![a, b], vec![("m", m)]));
    }
}

fn track(id: &str) -> Track {
    Track::new(&mr_levels::level_by_id(id)).expect("track builds")
}

fn road_profile() -> Vec<ProfilePoint> {
    vec![
        ProfilePoint::new(Lat::f(|f, _| -f.hw)),
        ProfilePoint::new(0.0),
        ProfilePoint::new(Lat::f(|f, _| f.hw)),
    ]
}

fn wave(s: f64) -> bool {
    kernel::sin(s / 300.0) > 0.2
}

fn flat(r: &[[f64; 2]]) -> Vec<f64> {
    r.iter().flat_map(|x| x.iter().copied()).collect()
}

fn road_cases(c: &mut Cases) {
    let sierra = track("sierra");
    let coast = track("coast");
    let cruise = track("cruise");
    let desert = track("desert");
    let opts = |step: f64, v_scale: f64, color: Option<ExtrudeColor>| ExtrudeOpts {
        step,
        v_scale,
        color,
    };
    c.g(
        "extrude/sierra-surface",
        Some(extrude(
            &sierra,
            &[[0.0, 560.0]],
            &road_profile(),
            &opts(2.0, 4.0, None),
        )),
    );
    c.g(
        "extrude/coast-surface-end",
        Some(extrude(
            &coast,
            &[[coast.length - 333.3, coast.length]],
            &road_profile(),
            &opts(2.0, 4.0, None),
        )),
    );
    for side in [-1.0, 1.0] {
        let w = move |f: &Frame| if side < 0.0 { f.wall_l } else { f.wall_r };
        let mut prof = vec![
            ProfilePoint::new(Lat::f(move |f, _| side * f.hw))
                .dy(-0.01)
                .u(0.0),
            ProfilePoint::new(Lat::f(move |f, _| side * w(f)))
                .dy(-0.08)
                .u(0.5),
            ProfilePoint::new(Lat::f(move |f, _| side * (w(f) + 2.5)))
                .dy(-1.6)
                .u(1.0),
        ];
        if side < 0.0 {
            prof.reverse();
        }
        c.g(
            &format!("extrude/shoulder{side}"),
            Some(extrude(
                &sierra,
                &[[1000.0, 1560.0]],
                &prof,
                &opts(2.0, 6.0, Some(ExtrudeColor::Rgb([0.45, 0.42, 0.33]))),
            )),
        );
        let mut city = vec![
            ProfilePoint::new(Lat::f(move |f, _| side * f.hw))
                .dy(0.0)
                .u(0.0),
            ProfilePoint::new(Lat::f(move |f, _| side * (w(f) + 0.35)))
                .dy(0.0)
                .u(0.4),
            ProfilePoint::new(Lat::f(move |f, _| side * (w(f) + 0.35)))
                .dy(-1.6)
                .u(1.0),
        ];
        if side < 0.0 {
            city.reverse();
        }
        c.g(
            &format!("extrude/city{side}"),
            Some(extrude(
                &cruise,
                &[[cruise.length - 300.0, cruise.length + 260.0]],
                &city,
                &opts(2.0, 6.0, Some(ExtrudeColor::Rgb([0.62, 0.62, 0.6]))),
            )),
        );
    }
    c.s("runs/sierra", flat(&runs(wave, 2.0, 0.0, sierra.length)));
    c.s(
        "runs/step-range",
        flat(&runs(|s| kernel::cos(s / 77.0) < -0.3, 1.0, 500.5, 2100.0)),
    );
    let end = sierra.length - 50.0;
    c.s(
        "runs/open-end",
        flat(&runs(|s| s > end, 3.0, 0.0, sierra.length)),
    );
    {
        let side = 1.0;
        let lat = move |k: f64| {
            Lat::f(move |f: &Frame, _| {
                side * (f.hw
                    + (if side < 0.0 {
                        f.wall_l - f.hw
                    } else {
                        f.wall_r - f.hw
                    })
                    + k)
            })
        };
        let prof = vec![
            ProfilePoint::new(lat(0.15)).dy(0.48).u(0.0),
            ProfilePoint::new(lat(0.1)).dy(0.64).u(0.5),
            ProfilePoint::new(lat(0.15)).dy(0.8).u(1.0),
        ];
        let r = runs(wave, 2.0, 0.0, sierra.length);
        c.g(
            "extrude/guardrail",
            Some(extrude(&sierra, &r[..4], &prof, &opts(2.0, 4.0, None))),
        );
    }
    {
        let prof = vec![
            ProfilePoint::new(-6.0).dy(0.5).gap_after(),
            ProfilePoint::new(Lat::f(|f, _| -f.hw)).dy(Lat::f(|_, s| 0.1 * kernel::sin(s / 10.0))),
            ProfilePoint::new(Lat::f(|f, _| f.hw * 0.5))
                .abs()
                .dy(Lat::f(|f, _| f.y + 2.0))
                .u(0.75),
            ProfilePoint::new(7.5).dy(0.0),
        ];
        let color = ExtrudeColor::Fn(Box::new(|f, s, p| {
            [p as f64 / 4.0, f.hw / 10.0, s / 1000.0]
        }));
        c.g(
            "extrude/mixed",
            Some(extrude(
                &desert,
                &[
                    [100.0, 100.3],
                    [200.0, 263.7],
                    [263.7, 264.0],
                    [900.25, 1001.0],
                ],
                &prof,
                &opts(3.0, 2.4, Some(color)),
            )),
        );
    }
    c.g(
        "extrude/runout",
        Some(extrude(
            &sierra,
            &[[sierra.length - 20.0, sierra.length + 90.0]],
            &road_profile(),
            &opts(5.0, 8.0, None),
        )),
    );
    c.g(
        "extrude/none",
        Some(extrude(
            &sierra,
            &[[10.0, 10.2]],
            &road_profile(),
            &ExtrudeOpts::default(),
        )),
    );
}

/// A scenery module that only has a label, for the job list.
struct Stub(&'static str);

impl Scenery for Stub {
    fn name(&self) -> &str {
        self.0
    }
    fn label(&self) -> Option<&str> {
        scenery_label(self.0)
    }
    fn build(&mut self, _w: &mut mr_worldgen::world::World) -> Result<(), String> {
        Ok(())
    }
}

fn stub(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    Some(Box::new(Stub(info.name)))
}

fn progress_cases(c: &mut Cases) {
    let mut out = Vec::new();
    for level in mr_levels::levels() {
        if level.id == "seaside" {
            continue;
        }
        let id = level.id.to_string();
        let world = mr_worldgen::world::World::new(level);
        let build = Build::new(world, level_jobs(Stages::default(), stub));
        let mut steps = Vec::new();
        build
            .run(|l, f| steps.push((l.to_string(), f)))
            .expect("the build runs");
        out.push((id, steps));
    }
    c.o("world/progress", Out::Progress(out));
}

fn build() -> Cases {
    let mut c = Cases(Vec::new());
    color_cases(&mut c);
    material_cases(&mut c);
    builder_cases(&mut c);
    color_builder_cases(&mut c);
    geom_cases(&mut c);
    road_cases(&mut c);
    progress_cases(&mut c);
    c
}

#[test]
fn builders_match_the_js() {
    let golden: Value = serde_json::from_str(GOLDEN).expect("builders.json parses");
    let want = golden["cases"].as_object().unwrap();
    let cases = build();
    let names: Vec<&str> = cases.0.iter().map(|(n, _)| n.as_str()).collect();
    let want_names: Vec<&str> = want.keys().map(String::as_str).collect();
    assert_eq!(
        names, want_names,
        "the test's cases and the golden's differ"
    );
    let mut errors = Vec::new();
    for (name, out) in &cases.0 {
        let w = &want[name];
        match out {
            Out::Geo(g) => check_geo(name, g.as_ref(), w, &mut errors),
            Out::Seq(v) => check_seq(name, v, w, &mut errors),
            Out::Strings(v) => {
                let ws: Vec<&str> = w
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap())
                    .collect();
                if *v != ws {
                    errors.push(format!("{name}: {v:?}, JS {ws:?}"));
                }
            }
            Out::Materials(ms) => {
                let wm = w.as_array().unwrap();
                assert_eq!(ms.len(), wm.len(), "{name}");
                for (k, (m, wv)) in ms.iter().zip(wm).enumerate() {
                    check_material(&format!("{name}[{k}]"), m, wv, &mut errors);
                }
            }
            Out::Meshes(g, list, mats) => check_meshes(name, g, list, mats, w, &mut errors),
            Out::Progress(levels) => {
                let wl = w.as_object().unwrap();
                let ids: Vec<&str> = levels.iter().map(|(id, _)| id.as_str()).collect();
                let wids: Vec<&str> = wl.keys().map(String::as_str).collect();
                if ids != wids {
                    errors.push(format!("{name}: levels {ids:?}, JS {wids:?}"));
                    continue;
                }
                for (id, steps) in levels {
                    let got: Vec<(String, String)> =
                        steps.iter().map(|(l, f)| (l.clone(), hex64(*f))).collect();
                    let exp: Vec<(String, String)> = wl[id]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|s| {
                            (
                                s[0].as_str().unwrap().to_string(),
                                s[1].as_str().unwrap().to_string(),
                            )
                        })
                        .collect();
                    if got != exp {
                        errors.push(format!("{name} {id}: {got:?}\n  JS {exp:?}"));
                    }
                }
            }
        }
    }
    assert!(
        errors.is_empty(),
        "{} of {} cases differ from the JS:\n{}",
        errors.len(),
        cases.0.len(),
        errors.join("\n")
    );
}

// ── The world build, beyond the golden ──────────────────────────────────

#[test]
fn scenery_modules_follow_the_zones() {
    let sierra = mr_levels::level_by_id("sierra");
    let m = scenery_modules(&sierra);
    let names: Vec<(&str, usize, &str)> = m.iter().map(|s| (s.name, s.zone, s.key)).collect();
    assert_eq!(
        names,
        [
            ("Mountain", 0, "mountain"),
            ("Valley", 1, "valley"),
            ("City", 2, "city")
        ]
    );
    let streets = mr_levels::level_by_id("streets");
    let m = scenery_modules(&streets);
    assert_eq!(m.len(), 1);
    assert_eq!((m[0].name, m[0].zone, m[0].key), ("Streets", 0, "neon"));
}

/// A scenery module that builds something with each builder, animates it
/// and registers a night parameter: the path a ported module takes.
struct Farm {
    fail_plan: bool,
}

impl Scenery for Farm {
    fn name(&self) -> &str {
        "Farm"
    }
    fn label(&self) -> Option<&str> {
        None
    }
    fn plan(&mut self, _w: &mut mr_worldgen::world::World) -> Result<(), String> {
        if self.fail_plan {
            Err("no room".into())
        } else {
            Ok(())
        }
    }
    fn build(&mut self, w: &mut mr_worldgen::world::World) -> Result<(), String> {
        use mr_worldgen::world::{Change, Edit, Handle, UpdateCtx};
        let group = w.graph.group("farm");
        w.graph.add(w.root, group);
        let wood = w
            .graph
            .add_material(Material::standard().set("color", 0x8a6a4a));
        let lamp = w.graph.add_material(
            Material::standard()
                .set("emissive", 0xffcc88)
                .set("emissiveIntensity", 0.3),
        );
        w.add_night(Some(lamp), "emissiveIntensity", 0.3, 7.0);
        let unused = w.graph.add_material(Material::basic());
        w.add_night(Some(unused), "opacity", 0.0, 1.0);
        let mut b = Builder::new_paint(&[("paint", ("p", 0xd9d0bc))]);
        b.box_yaw("paint", 4.0, 3.0, 5.0, 0.0, 0.0, 0.0, 0.2);
        b.box_yaw("lamp", 0.3, 0.3, 0.3, 0.0, 3.0, 0.0, 0.0);
        for m in b.build(
            &mut w.graph,
            &[("p", wood), ("lamp", lamp)],
            &BuildOpts::default(),
        ) {
            w.graph.add(group, m);
        }
        let mut gb = GeoBuilder::new(true, false);
        gb.box_(10.0, 0.0, 0.0, 2.0, 2.0, 2.0, 0.0, &PrismOpts::default());
        let geo = w.graph.add_geometry(gb.build());
        let wheel = static_mesh(&mut w.graph, geo, wood, &StaticOpts::default());
        w.graph.add(group, wheel);
        let mut t = 0.0;
        w.add_animator(move |u: &UpdateCtx, out: &mut Vec<Edit>| {
            t += u.dt;
            out.push(Edit {
                target: Handle::Node(wheel),
                change: Change::Visible(t < 1.0),
            });
            out.push(Edit {
                target: Handle::Material(unused),
                change: Change::Number {
                    prop: "opacity",
                    value: t,
                },
            });
        });
        w.sim_data.runout = 123.0;
        Ok(())
    }
}

#[test]
fn a_world_builds_into_a_scene() {
    use mr_worldgen::world::{SceneRef, UpdateCtx, World};
    let level = mr_levels::level_by_id("sierra");
    let farm = |info: &SceneryInfo| -> Option<Box<dyn Scenery>> {
        match info.name {
            "Mountain" => None,
            "Valley" => Some(Box::new(Farm { fail_plan: true })),
            _ => Some(Box::new(Farm { fail_plan: false })),
        }
    };
    let jobs = level_jobs(Stages::default(), farm);
    let mut labels = Vec::new();
    let mut wb = Build::new(World::new(level), jobs)
        .run(|l, f| labels.push((l.to_string(), f)))
        .unwrap();
    // Mountain is missing, Valley's plan failed: one module, labelled by default.
    assert_eq!(
        labels.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
        [
            "Surveying the route",
            "Shaping the land",
            "Sculpting terrain",
            "Paving roads",
            "Building scenery",
            "Ready"
        ]
    );
    assert_eq!(labels[4].1, 0.72);
    assert_eq!(
        wb.log,
        ["Mountain scenery missing", "Farm.plan failed no room"]
    );
    let s = &wb.scene;
    let names: Vec<&str> = s.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(
        names,
        ["world:sierra", "farm", "valley:p", "valley:lamp", ""]
    );
    assert_eq!(s.roots, [0]);
    assert_eq!(s.nodes[1].parent, Some(0));
    assert_eq!(s.nodes[1].children, [2, 3, 4]);
    // Materials in the order the walk meets them; the unused one left out,
    // and its night parameter with it.
    assert_eq!(s.materials.len(), 2);
    assert_eq!(s.night_params.len(), 1);
    assert_eq!(s.night_params[0].material, 1);
    assert_eq!(s.night_params[0].night, 7.0);
    assert_eq!(s.meshes.len(), 3);
    assert_eq!(wb.sim_data.runout, 123.0);
    // The file format takes it.
    let bytes = mr_scene::write(s).unwrap();
    let back = mr_scene::read(&bytes).unwrap();
    assert_eq!(back.nodes.len(), s.nodes.len());

    let u = UpdateCtx {
        dt: 0.6,
        night: 0.0,
        camera: None,
        s: 0.0,
    };
    let e = wb.update(&u);
    // The edit of the unused material is dropped; the wheel is node 4.
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].target, SceneRef::Node(4));
    assert_eq!(e[0].change, mr_worldgen::world::Change::Visible(true));
    let e = wb.update(&u);
    assert_eq!(e[0].change, mr_worldgen::world::Change::Visible(false));
}

#[test]
fn parallel_parts_merge_in_order() {
    use mr_worldgen::world::{Change, Edit, Handle, Part, PartFn, UpdateCtx, World};
    let make = |k: u32| -> PartFn {
        Box::new(move |w: &World| {
            let mut p = Part::default();
            let g = p.graph.group(&format!("tile{k}"));
            p.graph.add_root(g);
            let mut gb = GeoBuilder::new(false, false);
            gb.box_(
                f64::from(k) * 10.0 + w.track().length * 0.0,
                0.0,
                0.0,
                1.0,
                1.0,
                1.0,
                0.0,
                &PrismOpts::default(),
            );
            let geo = p.graph.add_geometry(gb.build());
            let mat = p
                .graph
                .add_material(Material::lambert().set("color", 0x335522));
            let m = p.graph.mesh(geo, mat);
            p.graph.add(g, m);
            p.animators
                .push(Box::new(move |_: &UpdateCtx, out: &mut Vec<Edit>| {
                    out.push(Edit {
                        target: Handle::Node(m),
                        change: Change::Visible(false),
                    })
                }));
            p
        })
    };
    let jobs = vec![
        Job::serial("Surveying the route", 0.02, move |w| {
            w.track = Some(Track::new(&w.level)?);
            Ok(vec![Job::Parallel {
                label: "Sculpting terrain".into(),
                progress: 0.1,
                parent: None,
                parts: (0..7).map(make).collect(),
            }])
        }),
        Job::serial("Ready", 1.0, |_| Ok(Vec::new())),
    ];
    let mut wb = Build::new(World::new(mr_levels::level_by_id("desert")), jobs)
        .run(|_, _| {})
        .unwrap();
    let names: Vec<&str> = wb.scene.nodes.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names[0], "world:desert");
    for k in 0..7 {
        assert_eq!(names[1 + 2 * k], format!("tile{k}"));
    }
    assert_eq!(wb.scene.materials.len(), 7);
    let edits = wb.update(&UpdateCtx {
        dt: 0.0,
        night: 0.0,
        camera: None,
        s: 0.0,
    });
    let targets: Vec<_> = edits.iter().map(|e| e.target).collect();
    let want: Vec<_> = (0..7)
        .map(|k| mr_worldgen::world::SceneRef::Node(2 + 2 * k))
        .collect();
    assert_eq!(targets, want);
}
