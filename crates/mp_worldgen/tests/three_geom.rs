//! WP 3.1's gate (SPEC 5.2): every `three_geom` generator, curve,
//! transform and merge against three.js r180 run with the parity kernel
//! (`parity/golden/three_geom/three_geom.json`, written by
//! `tools/parity/three-geom.mjs`). The cases are built here exactly as that
//! tool builds them, in the same order; read the two side by side.
//!
//! The requirement is bit-identical: the same attributes in the same order,
//! the same array types and counts, the same `f32` bits (hashed), the same
//! index and groups. The golden is compiled in, so the test runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use core::f64::consts::PI;

use mp_math::{Mulberry32, js, kernel};
use mp_scene::BufferData;
use mp_worldgen::three_geom::*;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/three_geom/three_geom.json");
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

enum Out {
    Geo(Option<BufferGeometry>),
    Seq(Vec<f64>),
}

struct Cases(Vec<(String, Out)>);

impl Cases {
    fn g(&mut self, name: &str, g: BufferGeometry) {
        self.0.push((name.to_string(), Out::Geo(Some(g))));
    }
    fn opt(&mut self, name: &str, g: Option<BufferGeometry>) {
        self.0.push((name.to_string(), Out::Geo(g)));
    }
    fn s(&mut self, name: &str, v: Vec<f64>) {
        self.0.push((name.to_string(), Out::Seq(v)));
    }
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

/// The largest difference between two f32 arrays, for the report.
fn max_diff(a: &BufferData, head: &[Value]) -> Option<f64> {
    let BufferData::F32(a) = a else { return None };
    let mut m: f64 = 0.0;
    for (i, h) in head.iter().enumerate() {
        let bits = u32::from_str_radix(h.as_str()?, 16).ok()?;
        m = m.max((f64::from(a[i]) - f64::from(f32::from_bits(bits))).abs());
    }
    Some(m)
}

fn check_geo(name: &str, g: &Option<BufferGeometry>, want: &Value, errors: &mut Vec<String>) {
    let Some(g) = g else {
        if want.get("null").is_none() {
            errors.push(format!("{name}: Rust gave none, three gave a geometry"));
        }
        return;
    };
    if want.get("null").is_some() {
        errors.push(format!("{name}: three gave null"));
        return;
    }
    let attrs = want["attrs"].as_array().unwrap();
    let names: Vec<&str> = g.attributes.iter().map(|(n, _)| n.as_str()).collect();
    let want_names: Vec<&str> = attrs.iter().map(|a| a["name"].as_str().unwrap()).collect();
    if names != want_names {
        errors.push(format!(
            "{name}: attributes {names:?}, three has {want_names:?}"
        ));
        return;
    }
    for ((an, a), w) in g.attributes.iter().zip(attrs) {
        let what = format!("{name} {an}");
        if a.item_size as u64 != w["itemSize"].as_u64().unwrap()
            || type_name(&a.array) != w["type"].as_str().unwrap()
            || a.normalized != w["normalized"].as_bool().unwrap()
        {
            errors.push(format!("{what}: item size, type or normalized differ"));
            continue;
        }
        if a.array.len() as u64 != w["n"].as_u64().unwrap() {
            errors.push(format!(
                "{what}: {} values, three has {}",
                a.array.len(),
                w["n"]
            ));
            continue;
        }
        let head = head_of(&a.array);
        let whead = w["head"].as_array().unwrap();
        if &head != whead {
            errors.push(format!(
                "{what}: first values differ (max {:?})\n  rust  {head:?}\n  three {whead:?}",
                max_diff(&a.array, whead)
            ));
            continue;
        }
        let h = format!("{:016x}", fnv1a64(&a.array.to_le_bytes()));
        if h != w["hash"].as_str().unwrap() {
            errors.push(format!("{what}: values differ past the first {HEAD}"));
        }
    }
    match (&g.index, want["index"].is_null()) {
        (None, true) => {}
        (Some(_), true) => errors.push(format!("{name}: indexed, three is not")),
        (None, false) => errors.push(format!("{name}: not indexed, three is")),
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
            if type_name(&ix.array) != w["type"].as_str().unwrap() {
                errors.push(format!(
                    "{name}: index type {}, three {}",
                    type_name(&ix.array),
                    w["type"]
                ));
            } else if vals.len() as u64 != w["n"].as_u64().unwrap() {
                errors.push(format!(
                    "{name}: {} indices, three has {}",
                    vals.len(),
                    w["n"]
                ));
            } else if head != whead {
                errors.push(format!("{name}: first indices {head:?}, three {whead:?}"));
            } else if format!("{:016x}", fnv1a64(&bytes)) != w["hash"].as_str().unwrap() {
                errors.push(format!("{name}: indices differ past the first {HEAD}"));
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
        errors.push(format!(
            "{name}: groups {groups:?}, three {}",
            want["groups"]
        ));
    }
    let bbox = g.bounding_box.map(|b| {
        [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]
            .map(hex64)
            .to_vec()
    });
    let wbbox = want.get("bbox").map(|v| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    });
    if bbox != wbbox {
        errors.push(format!("{name}: bounding box {bbox:?}, three {wbbox:?}"));
    }
    let bs = g.bounding_sphere.map(|s| {
        [s.center.x, s.center.y, s.center.z, s.radius]
            .map(hex64)
            .to_vec()
    });
    let wbs = want.get("bsphere").map(|v| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    });
    if bs != wbs {
        errors.push(format!("{name}: bounding sphere {bs:?}, three {wbs:?}"));
    }
}

fn check_seq(name: &str, values: &[f64], want: &Value, errors: &mut Vec<String>) {
    let values: Vec<f64> = values
        .iter()
        .map(|&v| if v.is_nan() { f64::NAN } else { v })
        .collect();
    if values.len() as u64 != want["n"].as_u64().unwrap() {
        errors.push(format!(
            "{name}: {} values, three has {}",
            values.len(),
            want["n"]
        ));
        return;
    }
    let head = want["head"].as_array().unwrap();
    for (i, h) in head.iter().enumerate() {
        let w = f64::from_bits(u64::from_str_radix(h.as_str().unwrap(), 16).unwrap());
        if hex64(values[i]) != h.as_str().unwrap() {
            errors.push(format!(
                "{name}[{i}]: {} vs three {} (diff {:e})",
                values[i],
                w,
                values[i] - w
            ));
            return;
        }
    }
    let bytes: Vec<u8> = values
        .iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect();
    if format!("{:016x}", fnv1a64(&bytes)) != want["hash"].as_str().unwrap() {
        errors.push(format!("{name}: values differ past the first {HEAD}"));
    }
}

// ── Inputs shared with the tool ─────────────────────────────────────────

fn v2(x: f64, y: f64) -> Vector2 {
    Vector2::new(x, y)
}

fn v3(x: f64, y: f64, z: f64) -> Vector3 {
    Vector3::new(x, y, z)
}

fn bale_profile() -> Vec<Vector2> {
    let (r, l) = (0.78, 1.25);
    let mut prof = Vec::new();
    for k in 0..=4 {
        prof.push(v2((k as f64 / 4.0) * (r - 0.12), l / 2.0));
    }
    for k in 1..=2 {
        let a = (k as f64 / 2.0) * PI / 2.0;
        prof.push(v2(
            r - 0.12 + kernel::sin(a) * 0.12,
            l / 2.0 - 0.12 + kernel::cos(a) * 0.12,
        ));
    }
    let n0 = prof.len();
    for k in (0..n0).rev() {
        prof.push(v2(prof[k].x, -prof[k].y));
    }
    prof
}

fn random_profile() -> Vec<Vector2> {
    let mut r = Mulberry32::new(31);
    (0..12)
        .map(|k| {
            let x = 0.05 + r.next_f64() * 0.3;
            let y = k as f64 * 0.1 + r.next_f64() * 0.05;
            v2(x, y)
        })
        .collect()
}

fn default_lathe() -> Vec<Vector2> {
    vec![v2(0.0, -0.5), v2(0.5, 0.0), v2(0.0, 0.5)]
}

fn shape_of(pts: &[[f64; 2]]) -> Shape {
    Shape::from_points(&pts.iter().map(|p| v2(p[0], p[1])).collect::<Vec<_>>())
}

fn closed_shape(pts: &[[f64; 2]]) -> Shape {
    let mut s = Shape::new();
    for (i, p) in pts.iter().enumerate() {
        if i > 0 {
            s.line_to(p[0], p[1]);
        } else {
            s.move_to(p[0], p[1]);
        }
    }
    s.close_path();
    s
}

fn trapezoid() -> Shape {
    shape_of(&[[-0.3, 0.0], [0.3, 0.0], [0.2, 0.82], [-0.2, 0.82]])
}

fn roof() -> Shape {
    closed_shape(&[[-3.4, 0.0], [0.0, 2.1], [3.4, 0.0]])
}

fn hull() -> Shape {
    closed_shape(&[
        [-5.5, -1.6],
        [3.4, -1.8],
        [5.6, 0.0],
        [3.4, 1.8],
        [-5.5, 1.6],
    ])
}

fn surfboard() -> Shape {
    let (w, l) = (0.6, 2.2);
    let mut s = Shape::new();
    s.move_to(-w / 2.0, -l / 2.0);
    s.line_to(w / 2.0, -l / 2.0);
    s.quadratic_curve_to(w / 2.0, l * 0.2, 0.0, l / 2.0);
    s.quadratic_curve_to(-w / 2.0, l * 0.2, -w / 2.0, -l / 2.0);
    s
}

fn caliper() -> Shape {
    let rim_r = 0.3;
    let (ro, ri, a0, a1) = (rim_r * 0.9, rim_r * 0.6, PI * 0.04, PI * 0.4);
    let mut s = Shape::new();
    s.absarc(0.0, 0.0, ro, a0, a1, false);
    s.absarc(0.0, 0.0, ri, a1, a0, true);
    s
}

fn barn() -> Shape {
    let (w, h, r) = (3.0, 2.5, 2.0);
    closed_shape(&[
        [-w, 0.0],
        [w, 0.0],
        [w, h],
        [0.72 * w, h + 0.62 * r],
        [0.0, h + r],
        [-0.72 * w, h + 0.62 * r],
        [-w, h],
    ])
}

fn holed() -> Shape {
    let mut s = closed_shape(&[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]);
    let mut h = Path::new();
    h.absarc(0.2, 0.1, 0.4, 0.0, 2.0 * PI, true);
    s.holes.push(h);
    let mut h2 = Path::new();
    h2.move_to(-0.8, -0.8);
    h2.line_to(-0.8, -0.4);
    h2.line_to(-0.4, -0.4);
    h2.line_to(-0.4, -0.8);
    s.holes.push(h2);
    s
}

fn curvy() -> Shape {
    let mut s = Shape::new();
    s.move_to(0.0, 0.0);
    s.bezier_curve_to(0.5, -0.3, 1.2, 0.2, 1.4, 0.9);
    s.spline_thru(&[v2(1.1, 1.4), v2(0.6, 1.5), v2(0.2, 1.2)]);
    s.absellipse(-0.1, 0.6, 0.35, 0.6, PI / 2.0, 3.0 * PI / 2.0, false, 0.3);
    s.line_to(0.0, 0.0);
    s
}

fn triangle() -> Shape {
    shape_of(&[[0.0, 0.5], [-0.5, -0.5], [0.5, -0.5]])
}

fn opts(f: impl FnOnce(&mut ExtrudeOptions)) -> ExtrudeOptions {
    let mut o = ExtrudeOptions::THREE_DEFAULTS;
    f(&mut o);
    o
}

fn tri(mut contour: Vec<Vector2>, mut holes: Vec<Vec<Vector2>>) -> Vec<f64> {
    let faces = triangulate_shape(&mut contour, &mut holes);
    let mut out: Vec<f64> = faces.iter().flatten().map(|&i| i as f64).collect();
    out.push(contour.len() as f64);
    out.extend(holes.iter().map(|h| h.len() as f64));
    out
}

fn wobbly(n: usize, seed: u32, hole_r: Option<f64>) -> Vec<Vector2> {
    let mut r = Mulberry32::new(seed);
    (0..n)
        .map(|k| {
            let a = (k as f64 / n as f64) * 2.0 * PI;
            let rad = hole_r.unwrap_or(10.0) * (0.7 + r.next_f64() * 0.6);
            v2(kernel::cos(a) * rad, kernel::sin(a) * rad)
        })
        .collect()
}

fn coaster_base() -> Vec<Vector3> {
    let l = 300.0;
    [
        [-20.0, 1.5, l - 84.0],
        [-8.0, 3.0, l - 86.0],
        [-5.0, 9.0, l - 78.0],
        [-6.0, 13.0, l - 66.0],
        [-10.0, 13.5, l - 56.0],
        [-19.0, 9.0, l - 50.0],
        [-20.0, 4.0, l - 40.0],
        [-12.0, 6.0, l - 30.0],
        [-6.0, 10.0, l - 20.0],
        [-12.0, 12.0, l - 10.0],
        [-20.0, 7.0, l - 12.0],
        [-21.0, 3.0, l - 26.0],
        [-21.0, 2.0, l - 60.0],
        [-21.0, 1.6, l - 76.0],
    ]
    .iter()
    .map(|p| v3(p[0], p[1], p[2]))
    .collect()
}

/// Beach.js buildCoaster: the rails either side of a centripetal loop.
fn coaster_rails() -> (CatmullRomCurve3, Vec<Vector3>, Vec<Vector3>) {
    let curve = CatmullRomCurve3::new(coaster_base(), true, CurveType::Centripetal, 0.5);
    let n = 200;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for i in 0..n {
        let u = i as f64 / n as f64;
        let p = curve.get_point_at(u);
        let tg = curve.get_tangent_at(u);
        let side = v3(-tg.z, 0.0, tg.x).normalize();
        left.push(p.add_scaled_vector(side, 0.55));
        right.push(p.add_scaled_vector(side, -0.55));
    }
    (curve, left, right)
}

fn open_pts() -> Vec<Vector3> {
    vec![
        v3(0.0, 0.0, 0.0),
        v3(1.0, 2.0, 0.5),
        v3(3.0, 2.5, -1.0),
        v3(4.0, 0.0, -2.0),
        v3(6.0, 1.0, 0.0),
    ]
}

fn xyz(v: Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn xyzw(q: Quaternion) -> [f64; 4] {
    [q.x, q.y, q.z, q.w]
}

fn sphere(r: f64, w: f64, h: f64) -> BufferGeometry {
    sphere_geometry(r, w, h, 0.0, 2.0 * PI, 0.0, PI)
}

fn box1() -> BufferGeometry {
    box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0)
}

fn trio() -> Vec<BufferGeometry> {
    let mut b = box1();
    b.translate(2.0, 0.0, 0.0);
    vec![
        b,
        cylinder_geometry(0.5, 0.5, 1.0, 8.0, 1.0, false, 0.0, 2.0 * PI),
        sphere(1.0, 8.0, 6.0),
    ]
}

fn refs(v: &[BufferGeometry]) -> Vec<&BufferGeometry> {
    v.iter().collect()
}

// ── The cases, in the tool's order ──────────────────────────────────────

fn build() -> Cases {
    let mut c = Cases(Vec::new());
    let tau = 2.0 * PI;

    c.g("box/default", box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0));
    c.g("box/sized", box_geometry(2.0, 3.0, 4.0, 1.0, 1.0, 1.0));
    c.g("box/segs222", box_geometry(2.0, 2.0, 2.0, 2.0, 2.0, 2.0));
    c.g("box/odd", box_geometry(1.5, 0.25, 3.0, 2.7, 1.0, 3.2));
    c.g("box/negative", box_geometry(-1.0, 2.0, 0.5, 1.0, 2.0, 1.0));

    c.g("plane/default", plane_geometry(1.0, 1.0, 1.0, 1.0));
    c.g("plane/grid", plane_geometry(2.0, 3.0, 4.0, 5.0));
    c.g("plane/strip", plane_geometry(10.0, 1.0, 1.0, 3.0));
    c.g("plane/frac", plane_geometry(1.0, 1.0, 2.5, 1.5));

    c.g("circle/default", circle_geometry(1.0, 32.0, 0.0, tau));
    c.g("circle/hex", circle_geometry(2.0, 6.0, 0.0, tau));
    c.g("circle/quarter", circle_geometry(1.0, 3.0, 0.0, PI / 2.0));
    c.g("circle/min", circle_geometry(1.0, 2.0, 0.0, tau));
    c.g("circle/arc", circle_geometry(0.5, 16.0, 1.0, 4.0));

    c.g(
        "cylinder/default",
        cylinder_geometry(1.0, 1.0, 1.0, 32.0, 1.0, false, 0.0, tau),
    );
    c.g(
        "cylinder/post",
        cylinder_geometry(0.1, 0.2, 2.0, 8.0, 1.0, false, 0.0, tau),
    );
    c.g(
        "cylinder/open",
        cylinder_geometry(0.5, 0.5, 1.0, 6.0, 1.0, true, 0.0, tau),
    );
    c.g(
        "cylinder/topzero",
        cylinder_geometry(0.0, 1.0, 2.0, 12.0, 3.0, false, 0.0, tau),
    );
    c.g(
        "cylinder/bottomzero",
        cylinder_geometry(1.0, 0.0, 2.0, 5.0, 2.0, false, 0.0, tau),
    );
    c.g(
        "cylinder/arc",
        cylinder_geometry(1.0, 1.0, 1.0, 8.0, 2.0, false, 0.3, PI),
    );
    c.g(
        "cylinder/tri",
        cylinder_geometry(0.3, 0.3, 1.0, 3.0, 1.0, false, 0.0, tau),
    );
    c.g(
        "cylinder/frac",
        cylinder_geometry(0.4, 0.6, 1.5, 7.6, 2.2, false, 0.0, tau),
    );

    c.g(
        "cone/default",
        cone_geometry(1.0, 1.0, 32.0, 1.0, false, 0.0, tau),
    );
    c.g(
        "cone/six",
        cone_geometry(0.5, 2.0, 6.0, 1.0, false, 0.0, tau),
    );
    c.g(
        "cone/open",
        cone_geometry(1.0, 1.0, 4.0, 3.0, true, 0.0, tau),
    );
    c.g(
        "cone/half",
        cone_geometry(2.0, 1.0, 8.0, 1.0, false, 0.0, PI),
    );

    c.g(
        "sphere/default",
        sphere_geometry(1.0, 32.0, 16.0, 0.0, tau, 0.0, PI),
    );
    c.g(
        "sphere/low",
        sphere_geometry(1.0, 8.0, 6.0, 0.0, tau, 0.0, PI),
    );
    c.g(
        "sphere/min",
        sphere_geometry(2.0, 3.0, 2.0, 0.0, tau, 0.0, PI),
    );
    c.g(
        "sphere/halfphi",
        sphere_geometry(1.0, 12.0, 8.0, 0.0, PI, 0.0, PI),
    );
    c.g(
        "sphere/hemi",
        sphere_geometry(1.0, 10.0, 6.0, 0.0, tau, 0.0, PI / 2.0),
    );
    c.g(
        "sphere/band",
        sphere_geometry(1.0, 10.0, 6.0, 0.0, tau, PI / 4.0, PI / 2.0),
    );
    c.g(
        "sphere/frac",
        sphere_geometry(0.5, 7.5, 4.2, 0.0, tau, 0.0, PI),
    );

    c.g("icosahedron/0", icosahedron_geometry(1.0, 0.0));
    c.g("icosahedron/1", icosahedron_geometry(1.0, 1.0));
    c.g("icosahedron/2", icosahedron_geometry(2.0, 2.0));
    c.g("icosahedron/3", icosahedron_geometry(0.5, 3.0));
    c.g("octahedron/0", octahedron_geometry(0.28, 0.0));
    c.g("octahedron/1", octahedron_geometry(1.0, 1.0));
    c.g("dodecahedron/0", dodecahedron_geometry(1.0, 0.0));
    c.g("dodecahedron/rock", dodecahedron_geometry(0.62, 0.0));
    c.g("dodecahedron/1", dodecahedron_geometry(1.0, 1.0));
    c.g("tetrahedron/0", tetrahedron_geometry(1.0, 0.0));
    c.g("tetrahedron/2", tetrahedron_geometry(1.0, 2.0));

    c.g("torus/default", torus_geometry(1.0, 0.4, 12.0, 48.0, tau));
    c.g("torus/thin", torus_geometry(1.0, 0.2, 6.0, 12.0, tau));
    c.g("torus/arc", torus_geometry(2.0, 0.5, 3.0, 8.0, PI));
    c.g(
        "torus/quarter",
        torus_geometry(0.5, 0.1, 8.0, 24.0, PI / 2.0),
    );

    c.g("capsule/default", capsule_geometry(1.0, 1.0, 4.0, 8.0, 1.0));
    c.g(
        "capsule/streets",
        capsule_geometry(0.19, 0.85, 3.0, 6.0, 1.0),
    );
    c.g("capsule/coast", capsule_geometry(0.28, 2.3, 3.0, 8.0, 1.0));
    c.g("capsule/valley", capsule_geometry(0.4, 0.95, 3.0, 8.0, 1.0));
    c.g("capsule/flat", capsule_geometry(1.0, 0.0, 2.0, 5.0, 1.0));
    c.g("capsule/segs", capsule_geometry(0.5, 1.0, 1.0, 3.0, 3.0));
    c.g("capsule/frac", capsule_geometry(0.5, -1.0, 2.5, 4.9, 1.5));

    c.g(
        "lathe/default",
        lathe_geometry(&default_lathe(), 12.0, 0.0, tau),
    );
    c.g(
        "lathe/bale",
        lathe_geometry(&bale_profile(), 11.0, 0.0, tau),
    );
    c.g(
        "lathe/partial",
        lathe_geometry(
            &[v2(0.3, -0.1), v2(0.32, 0.0), v2(0.3, 0.1), v2(0.1, 0.12)],
            9.0,
            0.5,
            PI,
        ),
    );
    c.g(
        "lathe/clamped",
        lathe_geometry(&default_lathe(), 5.0, 0.0, 7.0),
    );
    c.g(
        "lathe/random",
        lathe_geometry(&random_profile(), 12.0, 0.0, tau),
    );

    // ── Shapes and extrusion
    let flat = ExtrudeOptions::flat;
    c.g(
        "extrude/default",
        extrude_geometry(
            &[shape_of(&[
                [0.5, 0.5],
                [-0.5, 0.5],
                [-0.5, -0.5],
                [0.5, -0.5],
            ])],
            &ExtrudeOptions::THREE_DEFAULTS,
        ),
    );
    c.g(
        "extrude/trapezoid",
        extrude_geometry(&[trapezoid()], &flat(2.0)),
    );
    c.g(
        "extrude/trapezoid-cw",
        extrude_geometry(
            &[shape_of(&[
                [-0.2, 0.82],
                [0.2, 0.82],
                [0.3, 0.0],
                [-0.3, 0.0],
            ])],
            &flat(2.0),
        ),
    );
    c.g("extrude/roof", extrude_geometry(&[roof()], &flat(9.6)));
    c.g("extrude/hull", extrude_geometry(&[hull()], &flat(1.7)));
    c.g("extrude/barn", extrude_geometry(&[barn()], &flat(6.0)));
    c.g(
        "extrude/surfboard",
        extrude_geometry(
            &[surfboard()],
            &opts(|o| {
                o.depth = 0.08;
                o.bevel_thickness = 0.15;
                o.bevel_size = 0.12;
                o.bevel_segments = 2.0;
                o.curve_segments = 8.0;
            }),
        ),
    );
    c.g("extrude/car", {
        let (width, bevel) = (0.3, 0.01);
        let depth = js::max(0.005, width - 2.0 * bevel);
        let mut g = extrude_geometry(
            &[closed_shape(&[
                [-1.0, 0.0],
                [1.0, 0.0],
                [0.9, 0.4],
                [-0.8, 0.45],
            ])],
            &ExtrudeOptions {
                depth,
                steps: 1.0,
                curve_segments: 1.0,
                bevel_enabled: bevel > 0.0,
                bevel_thickness: bevel,
                bevel_size: bevel,
                bevel_offset: -bevel,
                bevel_segments: 1.0,
            },
        );
        g.translate(0.0, 0.0, -depth / 2.0);
        g.rotate_y(-PI / 2.0);
        g.compute_vertex_normals();
        g
    });
    c.g("extrude/caliper", {
        let mut g = extrude_geometry(
            &[caliper()],
            &opts(|o| {
                o.depth = 0.07;
                o.bevel_enabled = false;
                o.curve_segments = 4.0;
            }),
        );
        g.translate(0.0, 0.0, -0.035);
        g.rotate_y(PI / 2.0);
        g.clear_groups();
        g.compute_vertex_normals();
        g
    });
    c.g(
        "extrude/steps",
        extrude_geometry(
            &[trapezoid()],
            &opts(|o| {
                o.depth = 2.0;
                o.steps = 3.0;
                o.bevel_enabled = false;
            }),
        ),
    );
    c.g(
        "extrude/holes",
        extrude_geometry(&[holed()], &opts(|o| o.curve_segments = 6.0)),
    );
    c.g(
        "extrude/curvy",
        extrude_geometry(
            &[curvy()],
            &opts(|o| {
                o.curve_segments = 6.0;
                o.depth = 0.5;
                o.bevel_thickness = 0.1;
                o.bevel_size = 0.05;
                o.bevel_segments = 2.0;
            }),
        ),
    );
    c.g(
        "extrude/multi",
        extrude_geometry(&[trapezoid(), roof()], &flat(1.5)),
    );

    c.g("shape/triangle", shape_geometry(&triangle(), 12.0));
    c.g("shape/holes", shape_geometry(&holed(), 5.0));
    c.g("shape/curvy", shape_geometry(&curvy(), 5.0));
    c.g("shape/closed", shape_geometry(&barn(), 12.0));
    c.g(
        "shape/array",
        shape_geometry_array(&[triangle(), holed()], 4.0),
    );

    c.s(
        "path/curvy-points",
        curvy()
            .get_points(7)
            .iter()
            .flat_map(|p| [p.x, p.y])
            .collect(),
    );
    c.s(
        "path/caliper-points",
        caliper()
            .get_points(4)
            .iter()
            .flat_map(|p| [p.x, p.y])
            .collect(),
    );
    c.s(
        "path/surfboard-spaced",
        surfboard()
            .get_spaced_points(10)
            .iter()
            .flat_map(|p| {
                let p = p.unwrap();
                [p.x, p.y]
            })
            .collect(),
    );
    c.s("path/curvy-length", {
        let s = curvy();
        let mut v = vec![s.get_length()];
        v.extend(s.get_curve_lengths());
        v
    });

    // ── triangulateShape
    c.s(
        "triangulate/square",
        tri(
            vec![v2(0.0, 0.0), v2(1.0, 0.0), v2(1.0, 1.0), v2(0.0, 1.0)],
            vec![],
        ),
    );
    c.s(
        "triangulate/star",
        tri(
            (0..10)
                .map(|k| {
                    let m = if k % 2 == 1 { 0.4 } else { 1.0 };
                    let a = k as f64 * PI / 5.0;
                    v2(kernel::cos(a) * m, kernel::sin(a) * m)
                })
                .collect(),
            vec![],
        ),
    );
    c.s("triangulate/wobbly-small", tri(wobbly(40, 3, None), vec![]));
    c.s(
        "triangulate/wobbly-hashed",
        tri(wobbly(120, 4, None), vec![]),
    );
    c.s(
        "triangulate/dup-end",
        tri(
            vec![
                v2(0.0, 0.0),
                v2(2.0, 0.0),
                v2(2.0, 1.0),
                v2(1.0, 2.0),
                v2(0.0, 1.0),
                v2(0.0, 0.0),
            ],
            vec![],
        ),
    );
    c.s(
        "triangulate/collinear",
        tri(
            [
                [0.0, 0.0],
                [1.0, 0.0],
                [2.0, 0.0],
                [3.0, 0.0],
                [3.0, 1.0],
                [2.0, 1.0],
                [2.0, 2.0],
                [1.0, 2.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ]
            .iter()
            .map(|p| v2(p[0], p[1]))
            .collect(),
            vec![],
        ),
    );
    let rev = |mut v: Vec<Vector2>| {
        v.reverse();
        v
    };
    c.s(
        "triangulate/holes",
        tri(
            wobbly(30, 5, None),
            vec![
                rev(wobbly(8, 6, Some(2.0))
                    .iter()
                    .map(|p| v2(p.x - 3.0, p.y))
                    .collect()),
                rev(wobbly(6, 7, Some(1.5))
                    .iter()
                    .map(|p| v2(p.x + 4.0, p.y + 1.0))
                    .collect()),
            ],
        ),
    );
    c.s(
        "triangulate/holes-hashed",
        tri(
            wobbly(100, 8, None),
            vec![
                rev(wobbly(12, 9, Some(2.0))),
                rev(wobbly(5, 10, Some(1.0))
                    .iter()
                    .map(|p| v2(p.x + 5.0, p.y))
                    .collect()),
            ],
        ),
    );
    c.s("triangulate/ellipse", {
        let pts = (0..12)
            .map(|i| {
                let a = (i as f64 / 12.0) * 2.0 * PI;
                let x = kernel::cos(a) * 0.4;
                let y = kernel::sin(a) * 0.25;
                let rot = 0.3;
                v2(
                    0.1 + x * kernel::cos(rot) - y * kernel::sin(rot),
                    0.2 + x * kernel::sin(rot) + y * kernel::cos(rot),
                )
            })
            .collect();
        tri(pts, vec![])
    });
    c.s(
        "triangulate/self-touch",
        tri(
            vec![
                v2(0.0, 0.0),
                v2(4.0, 0.0),
                v2(4.0, 4.0),
                v2(2.0, 2.0),
                v2(0.0, 4.0),
                v2(2.0, 2.5),
                v2(1.0, 1.0),
            ],
            vec![],
        ),
    );

    // ── Curves and Tube
    c.s("curve/coaster", {
        let (curve, _, _) = coaster_rails();
        let mut out = vec![curve.get_length()];
        for i in 0..=50 {
            let u = i as f64 / 50.0;
            out.extend(xyz(curve.get_point(u)));
            out.extend(xyz(curve.get_point_at(u)));
            out.extend(xyz(curve.get_tangent_at(u)));
        }
        out
    });
    c.s("curve/rails", {
        let (_, left, right) = coaster_rails();
        left.iter().chain(&right).flat_map(|&p| xyz(p)).collect()
    });
    c.s("curve/open", {
        let mut out = Vec::new();
        for (ty, tension) in [
            (CurveType::Centripetal, 0.5),
            (CurveType::Chordal, 0.5),
            (CurveType::CatmullRom, 0.5),
            (CurveType::CatmullRom, 0.2),
        ] {
            let c = CatmullRomCurve3::new(open_pts(), false, ty, tension);
            for i in 0..=20 {
                out.extend(xyz(c.get_point(i as f64 / 20.0)));
            }
            out.push(c.get_length());
            out.extend(xyz(c.get_point_at(0.37)));
            out.extend(xyz(c.get_tangent(1.0)));
            out.extend(xyz(c.get_tangent(0.0)));
        }
        let two = CatmullRomCurve3::new(
            vec![v3(0.0, 0.0, 0.0), v3(1.0, 1.0, 1.0)],
            false,
            CurveType::Centripetal,
            0.5,
        );
        for i in 0..=8 {
            out.extend(xyz(two.get_point(i as f64 / 8.0)));
        }
        out
    });
    c.s("curve/frenet", {
        let mut out = Vec::new();
        for closed in [false, true] {
            let c = CatmullRomCurve3::new(open_pts(), closed, CurveType::Centripetal, 0.5);
            let f = c.compute_frenet_frames(24, closed);
            for list in [&f.tangents, &f.normals, &f.binormals] {
                for &v in list {
                    out.extend(xyz(v));
                }
            }
        }
        out
    });
    let (_, left, right) = coaster_rails();
    c.g(
        "tube/coaster-left",
        tube_geometry(
            &CatmullRomCurve3::new(left, true, CurveType::Centripetal, 0.5),
            400.0,
            0.09,
            5.0,
            true,
        ),
    );
    c.g(
        "tube/coaster-right",
        tube_geometry(
            &CatmullRomCurve3::new(right, true, CurveType::Centripetal, 0.5),
            400.0,
            0.09,
            5.0,
            true,
        ),
    );
    c.g(
        "tube/open",
        tube_geometry(
            &CatmullRomCurve3::new(open_pts(), false, CurveType::CatmullRom, 0.5),
            20.0,
            0.2,
            6.0,
            false,
        ),
    );
    c.g(
        "tube/chordal",
        tube_geometry(
            &CatmullRomCurve3::new(open_pts(), false, CurveType::Chordal, 0.5),
            16.0,
            0.3,
            4.0,
            false,
        ),
    );
    c.g(
        "tube/line",
        tube_geometry(
            &LineCurve3 {
                v1: v3(0.0, 0.0, 0.0),
                v2: v3(1.0, 2.0, 3.0),
            },
            4.0,
            1.0,
            8.0,
            false,
        ),
    );
    c.g(
        "tube/quadratic",
        tube_geometry(
            &QuadraticBezierCurve3 {
                v0: v3(-1.0, -1.0, 0.0),
                v1: v3(-1.0, 1.0, 0.0),
                v2: v3(1.0, 1.0, 0.0),
            },
            64.0,
            1.0,
            8.0,
            false,
        ),
    );
    c.g(
        "tube/cubic",
        tube_geometry(
            &CubicBezierCurve3 {
                v0: v3(0.0, 0.0, 0.0),
                v1: v3(1.0, 3.0, 0.0),
                v2: v3(2.0, -1.0, 1.0),
                v3: v3(3.0, 0.0, 2.0),
            },
            12.0,
            0.5,
            3.0,
            false,
        ),
    );

    // ── Transforms and normals
    c.g("transform/chain", {
        let mut g = box_geometry(1.0, 2.0, 3.0, 2.0, 1.0, 1.0);
        g.rotate_x(0.3)
            .rotate_y(-1.2)
            .rotate_z(2.5)
            .translate(1.0, 2.0, 3.0)
            .scale(1.0, 2.0, 0.5);
        g
    });
    c.g("transform/matrix", {
        let q = Quaternion::from_euler(&Euler::new(0.4, -0.7, 1.1));
        let m = Matrix4::compose(v3(3.0, -1.0, 2.0), q, v3(1.5, 0.5, -2.0));
        let mut g = cylinder_geometry(0.5, 0.5, 2.0, 12.0, 1.0, false, 0.0, tau);
        g.apply_matrix4(&m);
        g
    });
    for (name, order) in [
        ("XYZ", EulerOrder::XYZ),
        ("YXZ", EulerOrder::YXZ),
        ("ZXY", EulerOrder::ZXY),
        ("ZYX", EulerOrder::ZYX),
        ("YZX", EulerOrder::YZX),
        ("XZY", EulerOrder::XZY),
    ] {
        let e = Euler::with_order(0.3, 0.5, -0.9, order);
        let mut g = sphere(1.0, 6.0, 4.0);
        g.apply_quaternion(Quaternion::from_euler(&e));
        c.g(&format!("transform/quaternion-{name}"), g);
        let mut g = sphere(1.0, 6.0, 4.0);
        g.apply_matrix4(&Matrix4::make_rotation_from_euler(&e));
        c.g(&format!("transform/euler-{name}"), g);
    }
    for (name, target) in [
        ("transform/lookat", v3(1.0, 2.0, 3.0)),
        ("transform/lookat-up", v3(0.0, 5.0, 0.0)),
        ("transform/lookat-origin", v3(0.0, 0.0, 0.0)),
    ] {
        let mut g = plane_geometry(2.0, 2.0, 1.0, 1.0);
        g.look_at(target);
        c.g(name, g);
    }
    c.g("transform/center", {
        let mut g = torus_geometry(1.0, 0.3, 6.0, 12.0, PI);
        g.center();
        g
    });
    c.g("transform/bounds", {
        let mut g = torus_geometry(1.0, 0.3, 6.0, 12.0, PI);
        g.compute_bounding_box();
        g.compute_bounding_sphere();
        g.rotate_x(0.7).translate(0.0, 3.0, 0.0);
        g
    });
    c.g("transform/normals-indexed", {
        let mut g = sphere(1.0, 8.0, 6.0);
        g.scale(1.0, 0.5, 2.0);
        g.compute_vertex_normals();
        g
    });
    c.g("transform/normals-added", {
        let mut g = torus_geometry(1.0, 0.4, 5.0, 7.0, tau);
        g.delete_attribute("normal");
        g.compute_vertex_normals();
        g
    });
    c.g("transform/nonindexed", {
        let mut g = box_geometry(1.0, 1.0, 1.0, 1.0, 2.0, 1.0).to_non_indexed();
        g.compute_vertex_normals();
        g
    });
    c.g(
        "transform/nonindexed-lathe",
        lathe_geometry(&bale_profile(), 11.0, 0.0, tau).to_non_indexed(),
    );
    c.s("math/matrices", {
        let mut out = Vec::new();
        let q = Quaternion::from_euler(&Euler::with_order(0.4, -0.7, 1.1, EulerOrder::YXZ));
        let m = Matrix4::compose(v3(3.0, -1.0, 2.0), q, v3(1.5, 0.5, -2.0));
        out.extend(m.elements);
        out.push(m.determinant());
        out.extend(m.invert().elements);
        let (p, qq, s) = m.decompose();
        out.extend(xyz(p));
        out.extend(xyzw(qq));
        out.extend(xyz(s));
        out.extend(Matrix3::get_normal_matrix(&m).elements);
        out.extend(Matrix4::make_rotation_axis(v3(1.0, 2.0, 2.0).normalize(), 0.9).elements);
        out.extend(
            Matrix4::make_basis(v3(1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), v3(0.0, -1.0, 0.0))
                .multiply(&m)
                .elements,
        );
        for (a, b) in [
            (v3(0.0, 1.0, 0.0), v3(1.0, 0.0, 0.0)),
            (v3(0.0, 1.0, 0.0), v3(0.0, -1.0, 0.0)),
            (v3(1.0, 0.0, 0.0), v3(-1.0, 0.0, 0.0)),
            (v3(0.0, 0.0, 1.0), v3(0.6, 0.0, 0.8)),
            (v3(0.0, 1.0, 0.0), v3(0.3, 0.4, -0.866).normalize()),
        ] {
            out.extend(xyzw(Quaternion::from_unit_vectors(a, b)));
        }
        let qa = Quaternion::from_axis_angle(v3(0.0, 1.0, 0.0), 0.8);
        let qb = Quaternion::from_euler(&Euler::with_order(0.2, 0.1, -0.3, EulerOrder::ZYX));
        out.extend(xyzw(qa.multiply(qb)));
        out.extend(xyzw(qa.premultiply(qb)));
        out.extend(xyzw(qa.slerp(qb, 0.35)));
        out.extend(xyz(
            v3(1.0, 2.0, 3.0).apply_axis_angle(v3(0.0, 0.0, 1.0), 0.5)
        ));
        out.extend(xyz(v3(1.0, 2.0, 3.0).apply_quaternion(qb)));
        out.extend(xyzw(Quaternion::from_rotation_matrix(
            &Matrix4::make_rotation_from_euler(&Euler::new(2.8, 0.1, -2.9)),
        )));
        out
    });

    // ── mergeGeometries and mergeVertices
    c.opt("merge/indexed", merge_geometries(&refs(&trio()), false));
    c.opt("merge/groups", merge_geometries(&refs(&trio()), true));
    c.opt(
        "merge/nonindexed",
        merge_geometries(
            &refs(&[box1().to_non_indexed(), icosahedron_geometry(1.0, 0.0)]),
            true,
        ),
    );
    c.opt(
        "merge/attribute-order",
        merge_geometries(
            &refs(&[lathe_geometry(&default_lathe(), 12.0, 0.0, tau), box1()]),
            false,
        ),
    );
    c.opt(
        "merge/mixed-index",
        merge_geometries(&refs(&[box1(), icosahedron_geometry(1.0, 0.0)]), false),
    );
    c.opt("merge/missing-attribute", {
        let mut b = box1();
        b.delete_attribute("uv");
        merge_geometries(&refs(&[box1(), b]), false)
    });
    c.opt("merge/uint32", {
        let parts: Vec<BufferGeometry> = (0..130)
            .map(|k| {
                let mut g = sphere(1.0, 32.0, 16.0);
                g.translate(k as f64 * 3.0, 0.0, 0.0);
                g
            })
            .collect();
        merge_geometries(&refs(&parts), false)
    });
    c.opt("merge/colors", {
        let parts: Vec<BufferGeometry> =
            [box1(), cone_geometry(0.5, 1.0, 6.0, 1.0, false, 0.0, tau)]
                .iter()
                .enumerate()
                .map(|(k, g)| {
                    let mut g = g.to_non_indexed();
                    g.delete_attribute("uv");
                    let n = g.vertex_count();
                    let col: Vec<f64> = (0..n * 3)
                        .map(|i| (i % 7) as f64 / 7.0 + k as f64 * 0.1)
                        .collect();
                    g.set_attribute("color", BufferAttribute::from_f64(&col, 3));
                    g
                })
                .collect();
        merge_geometries(&refs(&parts), false)
    });

    c.g("mergeVertices/icosahedron-2", {
        let mut g = icosahedron_geometry(1.0, 2.0);
        g.delete_attribute("normal");
        g.delete_attribute("uv");
        merge_vertices(&g, 1e-4)
    });
    c.g("mergeVertices/box-222", {
        let mut g = box_geometry(2.0, 2.0, 2.0, 2.0, 2.0, 2.0);
        g.delete_attribute("uv");
        g.delete_attribute("normal");
        merge_vertices(&g, 1e-4)
    });
    c.g(
        "mergeVertices/icosahedron-full",
        merge_vertices(&icosahedron_geometry(1.0, 1.0), 1e-4),
    );
    c.g(
        "mergeVertices/indexed",
        merge_vertices(&sphere(1.0, 8.0, 6.0), 1e-4),
    );
    c.g("mergeVertices/tolerance", {
        let mut g = torus_geometry(1.0, 0.4, 8.0, 12.0, tau);
        g.delete_attribute("uv");
        merge_vertices(&g.to_non_indexed(), 0.05)
    });
    c.g("mergeVertices/flora", {
        let mut g = icosahedron_geometry(1.0, 1.0);
        g.delete_attribute("normal");
        g.delete_attribute("uv");
        g.scale(1.0, 0.7, 1.0);
        g.translate(0.1, 0.2, 0.0);
        let mut g = merge_vertices(&g, 1e-4);
        g.compute_vertex_normals();
        g.to_non_indexed()
    });

    c
}

#[test]
fn three_geom_matches_three_js() {
    let golden: Value = serde_json::from_str(GOLDEN).expect("three_geom.json parses");
    let want = golden["cases"].as_object().unwrap();
    let cases = build();
    let mut errors = Vec::new();
    assert_eq!(
        cases.0.len(),
        want.len(),
        "the test builds {} cases, the golden has {}",
        cases.0.len(),
        want.len()
    );
    for (name, out) in &cases.0 {
        let Some(w) = want.get(name) else {
            errors.push(format!("{name}: not in the golden"));
            continue;
        };
        match out {
            Out::Geo(g) => check_geo(name, g, w, &mut errors),
            Out::Seq(v) => check_seq(name, v, w, &mut errors),
        }
    }
    assert!(
        errors.is_empty(),
        "{} of {} cases differ from three.js:\n{}",
        errors.len(),
        cases.0.len(),
        errors.join("\n")
    );
}

/// A geometry goes into an `mp_scene` mesh with its attributes, index and
/// groups as they are.
#[test]
fn geometry_maps_onto_a_scene_mesh() {
    let mut scene = mp_scene::Scene::default();
    let mut g = box_geometry(1.0, 2.0, 3.0, 1.0, 1.0, 1.0);
    g.compute_bounding_sphere();
    let k = g.add_to_scene(&mut scene, "box");
    let m = &scene.meshes[k as usize];
    let names: Vec<&str> = m.attributes.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, ["position", "normal", "uv"]);
    let pos = &scene.buffers[m.attribute("position").unwrap().accessor as usize];
    assert_eq!((pos.item_size, pos.count()), (3, 24));
    let ix = &scene.buffers[m.index.unwrap() as usize];
    assert_eq!(
        (ix.data.component(), ix.count()),
        (mp_scene::Component::U16, 36)
    );
    assert_eq!(m.groups.len(), 6);
    assert_eq!(m.groups[5].material_index, 5);
    assert!(m.bounding_sphere.is_some() && m.bounding_box.is_none());
}
