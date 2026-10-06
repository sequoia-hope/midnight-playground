//! The `.mrscene` reader on damaged and hand-edited files: every truncation,
//! appended bytes, flipped bytes, wrong magic or version, bad lengths and
//! every dangling reference is an error and never a panic. Also the round
//! trip of every component type, non-finite numbers, JSON order and the
//! file-system wrappers.

mod common;

use common::*;
use mp_scene::*;
use serde_json::{Value, json};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn read_no_panic(bytes: &[u8]) -> Result<Scene, String> {
    catch_unwind(AssertUnwindSafe(|| read(bytes)))
        .unwrap_or_else(|_| panic!("read panicked on {} bytes", bytes.len()))
}

#[test]
fn every_truncation_of_a_file_is_refused() {
    let bytes = write(&sample()).unwrap();
    for n in 0..bytes.len() {
        assert!(read_no_panic(&bytes[..n]).is_err(), "prefix of {n} bytes");
    }
}

#[test]
fn bytes_after_the_binary_section_are_refused() {
    let mut bytes = write(&sample()).unwrap();
    bytes.push(0);
    let e = read(&bytes).unwrap_err();
    assert!(e.contains("binary section"), "{e}");
}

#[test]
fn flipping_any_single_byte_never_panics() {
    let bytes = write(&sample()).unwrap();
    for i in 0..bytes.len() {
        for mask in [0x01u8, 0x80, 0xff] {
            let mut b = bytes.clone();
            b[i] ^= mask;
            let _ = read_no_panic(&b);
        }
    }
}

#[test]
fn damage_to_the_fixed_prefix_is_refused() {
    let bytes = write(&sample()).unwrap();
    // Magic.
    let mut b = bytes.clone();
    b[7] = b'!';
    assert!(read(&b).unwrap_err().contains("magic"));
    // Version in the prefix.
    for v in [0u32, 2, u32::MAX] {
        let mut b = bytes.clone();
        b[8..12].copy_from_slice(&v.to_le_bytes());
        let e = read(&b).unwrap_err();
        assert!(e.contains(&format!("version {v}")), "{e}");
    }
    // Header length past the end, and absurdly large.
    for n in [bytes.len() as u32, u32::MAX] {
        let mut b = bytes.clone();
        b[12..16].copy_from_slice(&n.to_le_bytes());
        assert!(read(&b).unwrap_err().contains("inside the header"));
    }
    // Header length one short: the JSON is cut.
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
    let mut b = bytes.clone();
    b[12..16].copy_from_slice(&(n - 1).to_le_bytes());
    assert!(read(&b).is_err());
}

#[test]
fn a_header_that_disagrees_with_the_prefix_is_refused() {
    let bytes = write(&sample()).unwrap();
    type Edit = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Edit)> = vec![
        ("version", Box::new(|h| h["version"] = json!(2))),
        ("format", Box::new(|h| h["format"] = json!("gltf"))),
        ("unknown field", Box::new(|h| h["extra"] = json!(1))),
        (
            "unknown accessor field",
            Box::new(|h| h["accessors"][0]["stride"] = json!(12)),
        ),
        (
            "binary length",
            Box::new(|h| {
                let n = h["binary_length"].as_u64().unwrap();
                h["binary_length"] = json!(n + 8);
            }),
        ),
        (
            "missing field",
            Box::new(|h| {
                h.as_object_mut().unwrap().remove("roots");
            }),
        ),
        (
            "unknown component",
            Box::new(|h| h["accessors"][0]["component"] = json!("f16")),
        ),
        (
            "unknown material kind",
            Box::new(|h| h["materials"][0]["kind"] = json!("Chrome")),
        ),
        (
            "unknown node type",
            Box::new(|h| h["nodes"][0]["type"] = json!("Bone")),
        ),
        (
            "misaligned accessor",
            Box::new(|h| h["accessors"][1]["offset"] = json!(4)),
        ),
        (
            "accessor past the end",
            Box::new(|h| h["accessors"][5]["count"] = json!(1000)),
        ),
        (
            "item size zero",
            Box::new(|h| h["accessors"][1]["item_size"] = json!(0)),
        ),
        (
            "matrix of 15",
            Box::new(|h| {
                h["nodes"][0]["matrix"].as_array_mut().unwrap().pop();
            }),
        ),
        (
            "matrix with null",
            Box::new(|h| h["nodes"][0]["matrix"][3] = Value::Null),
        ),
        (
            "bad tagged number",
            Box::new(|h| h["nodes"][0]["matrix"][3] = json!({ "num": "inf" })),
        ),
        (
            "tagged number with another key",
            Box::new(|h| h["nodes"][0]["matrix"][3] = json!({ "num": "NaN", "x": 1 })),
        ),
        (
            "string for a number",
            Box::new(|h| h["nodes"][0]["matrix"][3] = json!("1")),
        ),
    ];
    for (what, f) in cases {
        assert!(read_no_panic(&rewrite(&bytes, f)).is_err(), "{what}");
    }
    // The rewrite itself keeps a good file good.
    assert_eq!(read(&rewrite(&bytes, |_| {})).unwrap(), sample());
}

/// Each reference `validate` checks, broken one at a time: `write` refuses
/// the scene, and `read` refuses a file that carries it.
#[test]
fn every_dangling_reference_is_refused_by_both_reader_and_writer() {
    type Break = fn(&mut Scene);
    let cases: &[(&str, Break)] = &[
        ("attribute accessor", |s| {
            s.meshes[0].attributes[1].accessor = 99
        }),
        ("mesh index", |s| s.meshes[0].index = Some(99)),
        ("instance matrices", |s| s.instances[0].matrices = 99),
        ("instance colours", |s| s.instances[0].colors = Some(99)),
        ("instance node", |s| s.instances[0].node = 99),
        ("texture pixels", |s| s.textures[0].pixels = 99),
        ("texture size", |s| s.textures[0].height = 2),
        ("texture channels", |s| s.textures[0].channels = 1),
        ("material texture", |s| {
            s.materials[0].params["map"] = json!({ "texture": 5 })
        }),
        ("uniform texture", |s| {
            s.materials[2].uniforms.as_mut().unwrap()["tStars"] = json!({ "texture": 1 })
        }),
        ("node mesh", |s| s.nodes[1].mesh = Some(1)),
        ("node material", |s| s.nodes[1].materials = vec![3]),
        ("node instances", |s| s.nodes[2].instances = Some(1)),
        ("node light", |s| s.nodes[3].light = Some(1)),
        ("node parent", |s| s.nodes[1].parent = Some(4)),
        ("node child", |s| s.nodes[0].children.push(4)),
        ("root", |s| s.roots.push(4)),
        ("night param", |s| s.night_params[0].material = 3),
        ("item size", |s| s.buffers[0].item_size = 4),
        ("item size zero", |s| s.buffers[1].item_size = 0),
    ];
    let good = write(&sample()).unwrap();
    for (what, f) in cases {
        let mut s = sample();
        f(&mut s);
        assert!(write(&s).is_err(), "write accepted a broken {what}");
        // The same damage in a file: the good file's header with the broken
        // scene's tables. (A file cannot hold an item size that does not
        // divide its values, since the accessor counts elements.)
        if *what == "item size" {
            continue;
        }
        let t = tables(&s);
        let broken = rewrite(&good, |h| {
            for key in t.as_object().unwrap().keys() {
                h[key] = t[key].clone();
            }
            if s.buffers[1].item_size == 0 {
                h["accessors"][1]["item_size"] = json!(0);
            }
        });
        assert!(
            read_no_panic(&broken).is_err(),
            "read accepted a broken {what}"
        );
    }
}

/// The scene's tables as the header writes them (everything but buffers).
fn tables(s: &Scene) -> Value {
    json!({
        "meshes": serde_json::to_value(&s.meshes).unwrap(),
        "instances": serde_json::to_value(&s.instances).unwrap(),
        "materials": serde_json::to_value(&s.materials).unwrap(),
        "textures": serde_json::to_value(&s.textures).unwrap(),
        "nodes": serde_json::to_value(&s.nodes).unwrap(),
        "roots": serde_json::to_value(&s.roots).unwrap(),
        "night_params": serde_json::to_value(&s.night_params).unwrap(),
    })
}

#[test]
fn every_component_type_round_trips_bit_for_bit() {
    let buffers = vec![
        buffer(
            1,
            BufferData::F32(vec![
                -0.0,
                f32::MIN_POSITIVE / 2.0,
                f32::MAX,
                f32::NAN,
                f32::INFINITY,
            ]),
        ),
        buffer(
            1,
            BufferData::F64(vec![-0.0, 5e-324, f64::MAX, f64::NAN, f64::NEG_INFINITY]),
        ),
        buffer(1, BufferData::U8(vec![0, 1, 255])),
        buffer(1, BufferData::U16(vec![0, 1, u16::MAX])),
        buffer(1, BufferData::U32(vec![0, 1, u32::MAX])),
        buffer(1, BufferData::I8(vec![i8::MIN, -1, i8::MAX])),
        buffer(1, BufferData::I16(vec![i16::MIN, -1, i16::MAX])),
        buffer(1, BufferData::I32(vec![i32::MIN, -1, i32::MAX])),
        // Empty buffers take no room but keep their slot.
        buffer(2, BufferData::F64(vec![])),
    ];
    let s = Scene {
        buffers,
        ..Scene::default()
    };
    let bytes = write(&s).unwrap();
    let back = read(&bytes).unwrap();
    assert_eq!(back.buffers.len(), s.buffers.len());
    for (a, b) in s.buffers.iter().zip(&back.buffers) {
        assert_eq!(a.data.component(), b.data.component());
        assert_eq!(a.item_size, b.item_size);
        assert_eq!(a.data.to_le_bytes(), b.data.to_le_bytes());
    }
    // Offsets: each buffer at the next multiple of 8 after the last.
    let h = header(&bytes);
    let offsets: Vec<u64> = h["accessors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["offset"].as_u64().unwrap())
        .collect();
    assert_eq!(offsets, [0, 24, 64, 72, 80, 96, 104, 112, 128]);
    assert_eq!(h["binary_length"], json!(128));
}

#[test]
fn the_empty_scene_round_trips() {
    let s = Scene::default();
    let bytes = write(&s).unwrap();
    assert_eq!(bytes.len() % 8, 0, "header padded, no binary section");
    assert_eq!(read(&bytes).unwrap(), s);
}

#[test]
fn non_finite_numbers_survive_as_tagged_values() {
    let mut s = sample();
    s.nodes[1].matrix[12] = f64::INFINITY;
    s.nodes[1].matrix_world[13] = f64::NEG_INFINITY;
    s.meshes[0].bounding_sphere = Some(vec![0.0, -0.0, 1e-310, f64::INFINITY]);
    s.meshes[0].bounding_box = Some(vec![f64::NAN; 6]);
    s.meshes[0].groups = vec![GroupDesc {
        start: 3,
        count: f64::INFINITY,
        material_index: 0,
    }];
    let bytes = write(&s).unwrap();
    let h = header(&bytes);
    assert_eq!(h["nodes"][1]["matrix"][12], json!({ "num": "Infinity" }));
    assert_eq!(
        h["nodes"][1]["matrix_world"][13],
        json!({ "num": "-Infinity" })
    );
    assert_eq!(h["meshes"][0]["bounding_box"][0], json!({ "num": "NaN" }));
    assert_eq!(
        h["meshes"][0]["groups"][0]["count"],
        json!({ "num": "Infinity" })
    );
    let back = read(&bytes).unwrap();
    assert_eq!(back.nodes[1].matrix[12], f64::INFINITY);
    assert_eq!(back.nodes[1].matrix_world[13], f64::NEG_INFINITY);
    let sphere = back.meshes[0].bounding_sphere.as_ref().unwrap();
    assert!(sphere[1] == 0.0 && sphere[1].is_sign_negative(), "-0 kept");
    assert_eq!(sphere[2], 1e-310, "subnormal kept");
    assert!(
        back.meshes[0]
            .bounding_box
            .as_ref()
            .unwrap()
            .iter()
            .all(|x| x.is_nan())
    );
    assert_eq!(back.meshes[0].groups[0].count, f64::INFINITY);
}

#[test]
fn awkward_finite_numbers_survive_exactly() {
    let values = [
        0.1 + 0.2,
        1.0 / 3.0,
        f64::MAX,
        f64::MIN_POSITIVE,
        5e-324,
        -123456789.12345679,
        f64::EPSILON,
        9007199254740993.0,
    ];
    let mut s = sample();
    for (i, &v) in values.iter().enumerate() {
        s.nodes[1].matrix[i] = v;
    }
    s.lights[0].intensity = 0.1 + 0.7;
    let back = read(&write(&s).unwrap()).unwrap();
    for (i, &v) in values.iter().enumerate() {
        assert_eq!(back.nodes[1].matrix[i].to_bits(), v.to_bits(), "{v}");
    }
    assert_eq!(back.lights[0].intensity, 0.1 + 0.7);
}

#[test]
fn json_objects_keep_their_insertion_order() {
    let mut s = sample();
    s.meta = json!({ "z": 1, "a": { "y": [1, 2], "b": null }, "m": "x" });
    s.materials[1].params = json!({ "zeta": 1, "alpha": 2, "mid": { "texture": 0 } })
        .as_object()
        .unwrap()
        .clone();
    let back = read(&write(&s).unwrap()).unwrap();
    let keys: Vec<&String> = back.meta.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["z", "a", "m"]);
    let keys: Vec<&String> = back.materials[1].params.keys().collect();
    assert_eq!(keys, ["zeta", "alpha", "mid"]);
    assert_eq!(back.materials[1].textures(), vec![0]);
}

#[test]
fn optional_node_fields_are_left_out_when_absent() {
    let h = header(&write(&sample()).unwrap());
    let root = h["nodes"][0].as_object().unwrap();
    for key in [
        "mesh",
        "materials",
        "multi_material",
        "instances",
        "center",
        "light",
    ] {
        assert!(!root.contains_key(key), "{key} written for a group");
    }
    let attr = h["meshes"][0]["attributes"][0].as_object().unwrap();
    assert!(!attr.contains_key("instanced"));
    assert!(!attr.contains_key("mesh_per_attribute"));
}

#[test]
fn a_texture_may_share_its_pixels_with_another() {
    let mut s = sample();
    s.textures.push(texture(1, 2, 4));
    let back = read(&write(&s).unwrap()).unwrap();
    assert_eq!(back.textures[1].pixels, back.textures[0].pixels);
    assert_eq!(back.buffers.len(), s.buffers.len());
}

#[test]
fn files_on_disk_round_trip_and_errors_name_the_path() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("mp_scene_format");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sample.mrscene");
    write_file(&path, &sample()).unwrap();
    assert_eq!(read_file(&path).unwrap(), sample());
    std::fs::write(&path, b"MRSCENE\0garbage").unwrap();
    let e = read_file(&path).unwrap_err();
    assert!(e.contains("sample.mrscene"), "{e}");
    let missing = dir.join("missing.mrscene");
    let e = read_file(&missing).unwrap_err();
    assert!(e.contains("missing.mrscene"), "{e}");
    let mut bad = sample();
    bad.roots = vec![9];
    assert!(write_file(&path, &bad).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A corrupt accessor count big enough to overflow the byte length must be
/// an error, not an arithmetic overflow.
#[test]
fn an_accessor_count_that_overflows_is_refused() {
    let bytes = write(&sample()).unwrap();
    for count in [u64::MAX, u64::MAX / 2, 1 << 62] {
        let b = rewrite(&bytes, |h| h["accessors"][0]["count"] = json!(count));
        assert!(read_no_panic(&b).is_err(), "count {count}");
    }
    let b = rewrite(&bytes, |h| {
        h["accessors"][0]["offset"] = json!(u64::MAX - 7)
    });
    assert!(read_no_panic(&b).is_err());
}

/// What the writer writes, the reader reads: a non-finite plain `f64` field
/// (here `render_order`), which JSON could only write as `null`, is refused
/// by the writer as the reader would refuse it.
#[test]
fn the_writer_never_writes_a_file_the_reader_refuses() {
    let mut s = sample();
    s.nodes[1].render_order = f64::INFINITY;
    if let Ok(bytes) = write(&s) {
        assert!(read(&bytes).is_ok(), "{:?}", read(&bytes).err());
    }
    let e = write(&s).unwrap_err();
    assert!(e.contains("finite"), "{e}");
    s.nodes[1].render_order = 2.0;
    assert!(read(&write(&s).unwrap()).is_ok());
}

/// A vertex index past the end of the positions, or an instance count past
/// the matrices buffer, is refused (the digest would index out of bounds).
#[test]
fn a_scene_the_reader_accepts_can_be_digested() {
    let bytes = write(&sample()).unwrap();
    // The writer refuses both scenes outright.
    let mut s = sample();
    s.buffers[1].data = BufferData::U16(vec![0, 1, 3]);
    assert!(write(&s).unwrap_err().contains("vertices"));
    let mut t = sample();
    t.instances[0].count = 3;
    assert!(write(&t).unwrap_err().contains("matrices"));
    // The same damage in a file: the last index (buffer 1, at byte 40 of
    // the binary section) made 3, and the instance count made 3.
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = (16 + n).div_ceil(8) * 8;
    let mut bad_index = bytes.clone();
    bad_index[bin + 44..bin + 46].copy_from_slice(&3u16.to_le_bytes());
    let bad_count = rewrite(&bytes, |h| h["instances"][0]["count"] = json!(3));
    for (what, b) in [("index", bad_index), ("count", bad_count)] {
        let r = read_no_panic(&b);
        assert!(r.is_err(), "{what}");
        if let Ok(back) = r {
            let r = catch_unwind(AssertUnwindSafe(|| mp_scene::digest::digest(&back)));
            assert!(r.is_ok(), "digest panicked on a scene read accepted");
        }
    }
}
