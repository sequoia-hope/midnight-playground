//! Golden checks shared by the flora and Mountain tests: geometries,
//! number sequences and materials against the records the parity tools
//! write (`geo()`, `seq()`, `material()` of `tools/parity/builders.mjs`).
//! A copy of the helpers in `tests/builders.rs`, made public.

#![allow(dead_code)]

use mp_scene::BufferData;
use mp_worldgen::material::Material;
use mp_worldgen::three_geom::BufferGeometry;
use serde_json::Value;

pub const HEAD: usize = 16;

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn hex64(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

pub fn type_name(d: &BufferData) -> &'static str {
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

pub fn head_of(d: &BufferData) -> Vec<Value> {
    (0..d.len().min(HEAD))
        .map(|i| match d {
            BufferData::F32(a) => Value::from(format!("{:08x}", a[i].to_bits())),
            BufferData::F64(a) => Value::from(hex64(a[i])),
            _ => Value::from(d.get(i)),
        })
        .collect()
}

/// A typed array against the tool's `attr()`.
pub fn check_array(
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

pub fn hexes(v: &[f64]) -> Vec<String> {
    v.iter().map(|&x| hex64(x)).collect()
}

pub fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().map(|s| s.as_str().unwrap().to_string()).collect())
}

pub fn check_geo(name: &str, g: Option<&BufferGeometry>, want: &Value, errors: &mut Vec<String>) {
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

pub fn check_seq(name: &str, values: &[f64], want: &Value, errors: &mut Vec<String>) {
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
pub fn same(a: &Value, b: &Value) -> bool {
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

pub fn check_material(name: &str, m: &Material, want: &Value, errors: &mut Vec<String>) {
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
