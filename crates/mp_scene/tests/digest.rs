//! Scene digests: equal scenes give equal digests, any change to what the
//! digest documents (vertices, indices, attributes, pixels, materials,
//! instances, world matrices) changes it, what it leaves out (names,
//! non-texture parameters) does not, and its arithmetic holds on the edge
//! cases (unindexed, degenerate, non-triangle, 2D, empty, partly drawn
//! instances). Also the digest's own JSON form and `compare`'s messages.

mod common;

use common::*;
use mp_scene::digest::{SceneDigest, compare, digest, sha256_hex};
use mp_scene::*;
use serde_json::{Value, json};

fn f32s(s: &mut Scene, buf: usize) -> &mut Vec<f32> {
    match &mut s.buffers[buf].data {
        BufferData::F32(v) => v,
        _ => panic!("buffer {buf} is not f32"),
    }
}

#[test]
fn equal_scenes_give_equal_digests() {
    let a = digest(&sample());
    assert_eq!(a, digest(&sample()));
    assert_eq!(a, digest(&sample().clone()));
    assert!(compare(&a, &digest(&sample())).is_empty());
    // And through the file.
    let back = read(&write(&sample()).unwrap()).unwrap();
    assert_eq!(a, digest(&back));
}

#[test]
fn every_documented_input_changes_the_digest() {
    type Change = fn(&mut Scene);
    let cases: &[(&str, Change)] = &[
        ("a vertex", |s| f32s(s, 0)[4] = 0.5),
        ("a vertex by one ulp", |s| {
            let v = &mut f32s(s, 0)[3];
            *v = f32::from_bits(v.to_bits() + 1);
        }),
        ("the index", |s| {
            s.buffers[1].data = BufferData::U16(vec![0, 2, 1])
        }),
        ("the index type", |s| {
            s.buffers[1].data = BufferData::U32(vec![0, 1, 2])
        }),
        ("a custom attribute", |s| f32s(s, 2)[0] = 0.0),
        ("an attribute's name", |s| {
            s.meshes[0].attributes[1].name = "aSurf".into()
        }),
        ("a pixel", |s| {
            s.buffers[4].data = BufferData::U8(vec![255, 0, 0, 255, 0, 255, 0, 127])
        }),
        ("a texture's size", |s| {
            s.textures[0].width = 1;
            s.textures[0].height = 2;
        }),
        ("a material's kind", |s| {
            s.materials[1].kind = MaterialKind::Standard
        }),
        ("a material's three.js type", |s| {
            s.materials[1].ty = "MeshLambertMaterial".into()
        }),
        ("a material's texture", |s| {
            s.materials[1]
                .params
                .insert("map".into(), json!({ "texture": 0 }));
        }),
        ("a shader uniform's texture", |s| {
            s.materials[2].uniforms = None
        }),
        ("a node's material", |s| s.nodes[1].materials = vec![1]),
        ("a node's world matrix", |s| {
            s.nodes[1].matrix_world[14] = 1.0
        }),
        ("an instance matrix", |s| f32s(s, 3)[28] = 3.0),
        ("the instance count", |s| s.instances[0].count = 1),
        ("an instance colour", |s| f32s(s, 5)[0] = 0.5),
        ("the scene's name", |s| s.meta["name"] = json!("other")),
        ("a node's type", |s| s.nodes[2].ty = NodeType::Points),
        ("a light", |s| {
            s.lights.clear();
            s.nodes[3].light = None;
        }),
        ("an extra node", |s| {
            s.nodes.push(node("x", NodeType::Group, None))
        }),
    ];
    let base = digest(&sample());
    for (what, f) in cases {
        let mut s = sample();
        f(&mut s);
        let d = digest(&s);
        assert_ne!(d, base, "changing {what} left the digest alone");
        if *what != "the scene's name" {
            assert!(
                !compare(&base, &d).is_empty(),
                "compare missed a change of {what}"
            );
        }
    }
}

/// The digest describes geometry and bindings, not every property: these
/// do not change it, by design.
#[test]
fn what_the_digest_leaves_out_does_not_change_it() {
    type Change = fn(&mut Scene);
    let cases: &[(&str, Change)] = &[
        ("a node's name", |s| s.nodes[1].name = "renamed".into()),
        ("a node's local matrix", |s| s.nodes[1].matrix[12] = 7.0),
        ("visibility", |s| s.nodes[1].visible = false),
        ("a number parameter", |s| {
            s.materials[0].params["roughness"] = json!(0.1)
        }),
        ("a colour parameter", |s| {
            s.materials[0].params["color"] = json!({ "color": [0, 0, 0] })
        }),
        ("a night parameter", |s| s.night_params[0].night = 1.0),
        ("a sampler setting", |s| {
            s.textures[0].wrap_s = three::MIRRORED_REPEAT_WRAPPING
        }),
        ("meta other than the name", |s| {
            s.meta["level"] = json!("coast")
        }),
        ("a light's intensity", |s| s.lights[0].intensity = 9.0),
        ("the instance capacity", |s| s.instances[0].capacity = 9),
        ("a stated bounding sphere", |s| {
            s.meshes[0].bounding_sphere = Some(vec![0.0, 0.0, 0.0, f64::INFINITY])
        }),
    ];
    let base = digest(&sample());
    for (what, f) in cases {
        let mut s = sample();
        f(&mut s);
        assert_eq!(digest(&s), base, "{what} changed the digest");
    }
}

#[test]
fn winding_order_changes_the_index_hash_but_not_the_area_or_centroid() {
    let a = digest(&sample());
    let mut s = sample();
    s.buffers[1].data = BufferData::U16(vec![0, 2, 1]);
    let b = digest(&s);
    assert_ne!(a.meshes[0].index, b.meshes[0].index);
    assert_eq!(a.meshes[0].area, b.meshes[0].area);
    assert_eq!(a.meshes[0].centroid, b.meshes[0].centroid);
    assert_eq!(a.meshes[0].bounds, b.meshes[0].bounds);
}

#[test]
fn an_f64_copy_of_the_positions_has_the_same_bounds_but_a_different_hash() {
    let a = digest(&sample());
    let mut s = sample();
    let v: Vec<f64> = f32s(&mut s, 0).iter().map(|&x| f64::from(x)).collect();
    s.buffers[0].data = BufferData::F64(v);
    let b = digest(&s);
    assert_eq!(a.meshes[0].bounds, b.meshes[0].bounds);
    assert_eq!(a.meshes[0].area, b.meshes[0].area);
    assert_ne!(
        a.meshes[0].attributes["position"],
        b.meshes[0].attributes["position"]
    );
    assert_ne!(a.counts.binary_bytes, b.counts.binary_bytes);
}

#[test]
fn attribute_hashes_are_the_sha256_of_the_little_endian_bytes() {
    let d = digest(&sample());
    let mut bytes = Vec::new();
    for x in [0.25f32, 0.5, 0.75] {
        bytes.extend(x.to_le_bytes());
    }
    assert_eq!(d.meshes[0].attributes["aLane"], json!(sha256_hex(&bytes)));
    assert_eq!(
        d.meshes[0].index.as_deref(),
        Some(sha256_hex(&[0, 0, 1, 0, 2, 0]).as_str())
    );
    // The well-known empty-input hash.
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn an_unindexed_mesh_reads_its_vertices_as_consecutive_triangles() {
    let mut s = sample();
    s.meshes[0].index = None;
    // Two triangles: the sample's (area 2), and a 1×3 one at z = 4.
    f32s(&mut s, 0).extend([0.0, 0.0, 4.0, 1.0, 0.0, 4.0, 0.0, 3.0, 4.0]);
    f32s(&mut s, 2).extend([0.0, 0.0, 0.0]);
    let m = &digest(&s).meshes[0];
    assert_eq!((m.vertices, m.indices, m.index.as_ref()), (6, 0, None));
    assert_eq!(m.area, Some(3.5));
    // Centroids (2/3, 2/3, 0) weight 2 and (1/3, 1, 4) weight 1.5.
    let c = m.centroid.unwrap();
    let want = [
        (2.0 * (2.0 / 3.0) + 1.5 * (1.0 / 3.0)) / 3.5,
        (2.0 * (2.0 / 3.0) + 1.5) / 3.5,
        6.0 / 3.5,
    ];
    for k in 0..3 {
        assert!((c[k] - want[k]).abs() < 1e-12, "{c:?} vs {want:?}");
    }
    assert_eq!(m.bounds, Some(vec![0.0, 0.0, 0.0, 2.0, 3.0, 4.0]));
}

#[test]
fn degenerate_triangles_have_zero_area_and_no_centroid() {
    let mut s = sample();
    s.buffers[0].data = BufferData::F32(vec![1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0]);
    let m = &digest(&s).meshes[0];
    assert_eq!(m.area, Some(0.0));
    assert_eq!(m.centroid, None);
    // A trailing partial triangle is ignored.
    let mut s = sample();
    s.buffers[1].data = BufferData::U16(vec![0, 1, 2, 0]);
    assert_eq!(digest(&s).meshes[0].area, Some(2.0));
}

#[test]
fn only_meshes_drawn_as_triangles_get_an_area() {
    // Drawn only by points, lines or sprites, or by nothing.
    for ty in [
        Some(NodeType::Points),
        Some(NodeType::LineSegments),
        Some(NodeType::Sprite),
        None,
    ] {
        let mut s = sample();
        match ty {
            Some(ty) => {
                s.nodes[1].ty = ty;
                s.nodes[2].ty = ty;
            }
            None => {
                s.nodes[1].mesh = None;
                s.nodes[2].mesh = None;
            }
        }
        let m = &digest(&s).meshes[0];
        assert_eq!((m.area, m.centroid), (None, None), "{ty:?}");
        assert!(m.bounds.is_some());
    }
    // One triangle user is enough.
    let mut s = sample();
    s.nodes[1].ty = NodeType::Points;
    assert_eq!(digest(&s).meshes[0].area, Some(2.0));
}

#[test]
fn two_dimensional_positions_have_2d_bounds_and_no_area_or_world_bounds() {
    let mut s = sample();
    s.buffers[0] = buffer(2, BufferData::F32(vec![0.0, 0.0, 2.0, -1.0, 0.0, 2.0]));
    let d = digest(&s);
    assert_eq!(d.meshes[0].bounds, Some(vec![0.0, -1.0, 2.0, 2.0]));
    assert_eq!(d.meshes[0].area, None);
    assert_eq!(d.drawables[0].world_bounds, None);
}

#[test]
fn a_mesh_without_positions_or_vertices_has_no_bounds() {
    let mut s = sample();
    s.meshes[0].attributes.remove(0);
    let d = digest(&s);
    assert_eq!(
        (d.meshes[0].vertices, d.meshes[0].bounds.clone()),
        (0, None)
    );
    assert_eq!(d.drawables[0].world_bounds, None);

    let mut s = sample();
    s.buffers[0].data = BufferData::F32(vec![]);
    s.buffers[1].data = BufferData::U16(vec![]);
    let d = digest(&s);
    assert_eq!(d.meshes[0].bounds, None);
    assert_eq!(d.meshes[0].area, Some(0.0));
    assert_eq!(d.drawables[0].world_bounds, None);
}

#[test]
fn world_bounds_apply_the_node_then_each_drawn_instance() {
    let mut s = sample();
    // Scale the plain mesh by 2 and move it.
    let m = &mut s.nodes[1].matrix_world;
    m[0] = 2.0;
    m[5] = 2.0;
    m[10] = 2.0;
    m[12] = -1.0;
    m[14] = 3.0;
    let d = digest(&s);
    assert_eq!(
        d.drawables[0].world_bounds,
        Some(vec![-1.0, 0.0, 3.0, 3.0, 4.0, 3.0])
    );
    // Only the first `count` instances are drawn.
    let mut s = sample();
    s.instances[0].count = 1;
    assert_eq!(
        digest(&s).drawables[1].world_bounds,
        Some(vec![0.0, 5.0, 0.0, 2.0, 7.0, 0.0])
    );
    // None drawn: no bounds.
    s.instances[0].count = 0;
    let d = digest(&s);
    assert_eq!(d.drawables[1].world_bounds, None);
    assert_eq!(d.drawables[1].instances, Some(0));
}

#[test]
fn drawables_list_textures_once_in_order_of_first_use() {
    let mut s = sample();
    s.textures.push(texture(1, 2, 4));
    s.materials[2].uniforms.as_mut().unwrap()["tStars"] = json!({ "texture": 1 });
    s.materials[2]
        .params
        .insert("map".into(), json!({ "texture": 1 }));
    s.materials[2]
        .params
        .insert("alphaMap".into(), json!({ "texture": 0 }));
    s.nodes[1].materials = vec![2, 0, 2];
    s.nodes[1].multi_material = Some(true);
    let d = digest(&s);
    assert_eq!(d.materials[2].textures, vec![1, 0]);
    assert_eq!(d.drawables[0].textures, vec![1, 0]);
    assert_eq!(
        d.drawables[0].kinds,
        vec![
            MaterialKind::SkyDome,
            MaterialKind::Asphalt,
            MaterialKind::SkyDome
        ]
    );
}

#[test]
fn kinds_count_materials_in_order_of_first_use() {
    let mut s = sample();
    s.materials.push(material(
        MaterialKind::Basic,
        "MeshBasicMaterial",
        json!({}),
    ));
    s.materials.push(material(
        MaterialKind::Asphalt,
        "MeshStandardMaterial",
        json!({}),
    ));
    let d = digest(&s);
    let kinds: Vec<(&String, &Value)> = d.kinds.iter().collect();
    assert_eq!(
        kinds,
        [
            (&"Asphalt".to_string(), &json!(2)),
            (&"Basic".to_string(), &json!(2)),
            (&"SkyDome".to_string(), &json!(1)),
        ]
    );
}

#[test]
fn counts_add_up() {
    let d = digest(&sample());
    let c = &d.counts;
    assert_eq!(
        (
            c.nodes,
            c.meshes,
            c.materials,
            c.textures,
            c.instances,
            c.lights
        ),
        (4, 1, 3, 1, 1, 1)
    );
    assert_eq!((c.drawables, c.vertices, c.indices), (2, 3, 3));
    // Buffers at 0, 40, 48, 64, 192 and 200, the last 24 bytes long.
    assert_eq!(c.binary_bytes, 224);
    assert_eq!(d.name.as_deref(), Some("sample"));
    let mut s = sample();
    s.meta = json!(null);
    assert_eq!(digest(&s).name, None);
}

#[test]
fn the_empty_scene_has_an_empty_digest() {
    let d = digest(&Scene::default());
    assert_eq!(d.counts.nodes + d.counts.binary_bytes, 0);
    assert!(d.meshes.is_empty() && d.drawables.is_empty() && d.kinds.is_empty());
}

#[test]
fn compare_reports_length_and_count_differences() {
    let a = digest(&sample());
    let mut s = sample();
    s.meshes.push(mesh(0, None));
    let diffs = compare(&a, &digest(&s));
    assert!(diffs.iter().any(|d| d.starts_with("counts:")), "{diffs:?}");
    assert!(
        diffs.iter().any(|d| d == "meshs: expected 1, got 2"),
        "{diffs:?}"
    );
    // Symmetric in what it finds.
    assert_eq!(compare(&digest(&s), &a).len(), diffs.len());
    // A kinds-only change.
    let mut s = sample();
    s.materials[1].kind = MaterialKind::Lambert;
    let diffs = compare(&a, &digest(&s));
    assert!(diffs.iter().any(|d| d.starts_with("kinds:")), "{diffs:?}");
    assert!(
        diffs.iter().any(|d| d.starts_with("material 1")),
        "{diffs:?}"
    );
}

/// The digest's JSON form, as the JS side writes it: round-trips, keeps
/// non-finite bounds as tagged numbers, and refuses unknown fields.
#[test]
fn the_digest_round_trips_through_json() {
    let mut s = sample();
    // NaN positions never win a comparison, so the bounds stay ±Infinity.
    s.buffers[0].data = BufferData::F32(vec![f32::NAN; 9]);
    let d = digest(&s);
    assert_eq!(
        d.meshes[0].bounds.as_ref().unwrap()[0],
        f64::INFINITY,
        "{:?}",
        d.meshes[0].bounds
    );
    let v = serde_json::to_value(&d).unwrap();
    assert_eq!(v["meshes"][0]["bounds"][0], json!({ "num": "Infinity" }));
    assert_eq!(v["meshes"][0]["bounds"][3], json!({ "num": "-Infinity" }));
    let back: SceneDigest = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(back.meshes[0].bounds, d.meshes[0].bounds);
    let d = digest(&sample());
    let back: SceneDigest = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
    assert_eq!(back, d);
    let mut v = serde_json::to_value(&d).unwrap();
    v["meshes"][0]["extra"] = json!(1);
    assert!(serde_json::from_value::<SceneDigest>(v).is_err());
}
