//! The reader and writer agree, the digest does the arithmetic it documents,
//! and a header written the way the JS exporter writes it parses.

use mp_scene::digest::{compare, digest};
use mp_scene::*;
use serde_json::{Map, Value, json};

const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn node(name: &str, ty: NodeType, parent: Option<u32>) -> NodeDesc {
    NodeDesc {
        name: name.into(),
        ty,
        parent,
        children: vec![],
        matrix: IDENTITY,
        matrix_world: IDENTITY,
        visible: true,
        matrix_auto_update: true,
        frustum_culled: true,
        render_order: 0.0,
        cast_shadow: false,
        receive_shadow: false,
        layers: 1,
        user_data: Map::new(),
        mesh: None,
        materials: vec![],
        multi_material: None,
        instances: None,
        center: None,
        light: None,
    }
}

fn material(kind: MaterialKind, ty: &str, params: Value) -> MaterialDesc {
    MaterialDesc {
        kind,
        kind_opts: None,
        ty: ty.into(),
        name: String::new(),
        program_key: None,
        params: params.as_object().unwrap().clone(),
        uniforms: None,
        shader: None,
    }
}

/// A right triangle (area 2), an instanced copy of it twice, a 2×1 texture.
fn sample() -> Scene {
    let buffers = vec![
        Buffer {
            item_size: 3,
            normalized: false,
            data: BufferData::F32(vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0]),
        },
        Buffer {
            item_size: 1,
            normalized: false,
            data: BufferData::U16(vec![0, 1, 2]),
        },
        Buffer {
            item_size: 3,
            normalized: false,
            data: BufferData::F32(vec![0.5, 0.25, 1.0, 0.5, 0.25, 1.0, 0.5, 0.25, 1.0]),
        },
        Buffer {
            item_size: 16,
            normalized: false,
            data: BufferData::F32(
                [IDENTITY, {
                    let mut m = IDENTITY;
                    m[12] = 10.0;
                    m
                }]
                .iter()
                .flatten()
                .map(|&x| x as f32)
                .collect(),
            ),
        },
        Buffer {
            item_size: 4,
            normalized: false,
            data: BufferData::U8(vec![255, 0, 0, 255, 0, 255, 0, 128]),
        },
        Buffer {
            item_size: 1,
            normalized: false,
            data: BufferData::F64(vec![1.5]),
        },
    ];
    let mesh = MeshDesc {
        name: "tri".into(),
        attributes: vec![
            AttributeRef {
                name: "position".into(),
                accessor: 0,
                instanced: false,
                mesh_per_attribute: None,
            },
            AttributeRef {
                name: "aLane".into(),
                accessor: 2,
                instanced: false,
                mesh_per_attribute: None,
            },
        ],
        index: Some(1),
        groups: vec![GroupDesc {
            start: 0,
            count: f64::INFINITY,
            material_index: 0,
        }],
        draw_range: DrawRange {
            start: 0,
            count: None,
        },
        bounding_box: None,
        bounding_sphere: Some(vec![1.0, 1.0, 0.0, f64::INFINITY]),
    };
    let texture = TextureDesc {
        name: "t".into(),
        source: TextureSource::Canvas,
        url: None,
        width: 2,
        height: 1,
        channels: 4,
        pixels: 4,
        format: three::RGBA_FORMAT,
        ty: three::UNSIGNED_BYTE_TYPE,
        flip_y: true,
        color_space: "srgb".into(),
        premultiply_alpha: false,
        unpack_alignment: 4,
        generate_mipmaps: true,
        wrap_s: three::REPEAT_WRAPPING,
        wrap_t: three::REPEAT_WRAPPING,
        mag_filter: three::LINEAR_FILTER,
        min_filter: three::LINEAR_MIPMAP_LINEAR_FILTER,
        anisotropy: 8.0,
        offset: [0.0, 0.0],
        repeat: [1.0, 1.0],
        rotation: 0.0,
        center: [0.0, 0.0],
        matrix_auto_update: true,
        channel: 0,
    };
    let mut root = node("root", NodeType::Group, None);
    root.children = vec![1, 2];
    let mut a = node("a", NodeType::Mesh, Some(0));
    a.mesh = Some(0);
    a.materials = vec![0];
    a.multi_material = Some(false);
    let mut b = node("b", NodeType::InstancedMesh, Some(0));
    b.mesh = Some(0);
    b.materials = vec![1];
    b.multi_material = Some(false);
    b.instances = Some(0);
    b.matrix_world[13] = 5.0;
    Scene {
        meta: json!({ "name": "sample" }),
        buffers,
        meshes: vec![mesh],
        instances: vec![InstanceDesc {
            node: 2,
            count: 2,
            capacity: 2,
            matrices: 3,
            colors: None,
            bounding_sphere: None,
        }],
        materials: vec![
            material(
                MaterialKind::Asphalt,
                "MeshStandardMaterial",
                json!({ "color": { "color": [1, 1, 1] }, "map": { "texture": 0 }, "roughness": 0.86 }),
            ),
            material(
                MaterialKind::Basic,
                "MeshBasicMaterial",
                json!({ "opacity": 0.5 }),
            ),
        ],
        textures: vec![texture],
        nodes: vec![root, a, b],
        roots: vec![0],
        lights: vec![],
        night_params: vec![NightParam {
            material: 0,
            prop: "emissiveIntensity".into(),
            day: 0.0,
            night: 0.06,
        }],
        environment: None,
    }
}

#[test]
fn write_then_read_gives_the_same_scene() {
    let s = sample();
    let bytes = write(&s).unwrap();
    assert_eq!(&bytes[..8], MAGIC);
    let back = read(&bytes).unwrap();
    assert_eq!(back, s);
    // And again: the writer's layout is stable.
    assert_eq!(write(&back).unwrap(), bytes);
}

#[test]
fn buffers_are_8_aligned() {
    let bytes = write(&sample()).unwrap();
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let header: Value = serde_json::from_slice(&bytes[16..16 + json_len]).unwrap();
    for a in header["accessors"].as_array().unwrap() {
        assert_eq!(a["offset"].as_u64().unwrap() % 8, 0);
    }
    assert_eq!(
        (16 + json_len).div_ceil(8) * 8 + header["binary_length"].as_u64().unwrap() as usize,
        bytes.len()
    );
}

#[test]
fn the_digest_does_the_documented_arithmetic() {
    let d = digest(&sample());
    let m = &d.meshes[0];
    assert_eq!((m.vertices, m.indices), (3, 3));
    assert_eq!(m.bounds, Some(vec![0.0, 0.0, 0.0, 2.0, 2.0, 0.0]));
    assert_eq!(m.area, Some(2.0));
    assert_eq!(m.centroid, Some([2.0 / 3.0, 2.0 / 3.0, 0.0]));
    assert_eq!(m.attributes.len(), 2);
    assert_eq!(d.textures[0].sha256.len(), 64);
    assert_eq!(d.materials[0].textures, vec![0]);
    // The plain mesh: world = local. The instanced one: two copies 10 m
    // apart along x, all lifted 5 m by the node.
    assert_eq!(
        d.drawables[0].world_bounds,
        Some(vec![0.0, 0.0, 0.0, 2.0, 2.0, 0.0])
    );
    assert_eq!(
        d.drawables[1].world_bounds,
        Some(vec![0.0, 5.0, 0.0, 12.0, 7.0, 0.0])
    );
    assert_eq!(d.drawables[1].instances, Some(2));
    assert_eq!(d.drawables[0].textures, vec![0]);
    assert_eq!(d.kinds.get("Asphalt"), Some(&Value::from(1)));
    assert!(compare(&d, &digest(&read(&write(&sample()).unwrap()).unwrap())).is_empty());
}

#[test]
fn compare_names_the_difference() {
    let a = digest(&sample());
    let mut s = sample();
    if let BufferData::F32(v) = &mut s.buffers[0].data {
        v[3] = 3.0;
    }
    let diffs = compare(&a, &digest(&s));
    assert!(diffs.iter().any(|d| d.starts_with("mesh 0")), "{diffs:?}");
    assert!(
        diffs.iter().any(|d| d.starts_with("drawable 0")),
        "{diffs:?}"
    );
}

#[test]
fn bad_files_are_refused() {
    assert!(read(b"not a scene at all").is_err());
    let mut s = sample();
    s.nodes[1].mesh = Some(7);
    assert!(write(&s).is_err());
    let mut s = sample();
    s.textures[0].width = 3;
    assert!(write(&s).is_err());
    let mut bytes = write(&sample()).unwrap();
    bytes.pop();
    assert!(read(&bytes).is_err());
}

/// The header fields as the JS exporter writes them, with a tagged
/// non-finite number and the optional node fields left out.
#[test]
fn a_header_in_the_exporters_form_parses() {
    let header = json!({
        "format": "mrscene", "version": 1, "meta": { "name": "x" }, "binary_length": 36,
        "accessors": [ { "offset": 0, "count": 3, "item_size": 3, "component": "f32", "normalized": false } ],
        "meshes": [ { "name": "", "attributes": [ { "name": "position", "accessor": 0 } ], "index": null,
            "groups": [], "draw_range": { "start": 0, "count": null }, "bounding_box": null,
            "bounding_sphere": [0, 0, 0, { "num": "Infinity" }] } ],
        "instances": [],
        "materials": [ { "kind": "Points", "kind_opts": null, "type": "PointsMaterial", "name": "", "program_key": null,
            "params": { "size": 2, "sizeAttenuation": true }, "uniforms": null, "shader": null } ],
        "textures": [],
        "nodes": [ { "name": "p", "type": "Points", "parent": null, "children": [],
            "matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1], "matrix_world": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1],
            "visible": false, "matrix_auto_update": false, "frustum_culled": true, "render_order": 3,
            "cast_shadow": false, "receive_shadow": false, "layers": 1, "user_data": { "dynamic": true },
            "mesh": 0, "materials": [0], "multi_material": false } ],
        "roots": [0], "lights": [], "night_params": [], "environment": null
    });
    let json = serde_json::to_vec(&header).unwrap();
    let mut bytes = MAGIC.to_vec();
    bytes.extend(VERSION.to_le_bytes());
    bytes.extend((json.len() as u32).to_le_bytes());
    bytes.extend(&json);
    bytes.resize((16 + json.len()).div_ceil(8) * 8, b' ');
    for x in [1.0f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0] {
        bytes.extend(x.to_le_bytes());
    }
    let s = read(&bytes).unwrap();
    assert_eq!(
        s.meshes[0].bounding_sphere.as_ref().unwrap()[3],
        f64::INFINITY
    );
    assert!(!s.nodes[0].visible);
    assert_eq!(s.materials[0].kind, MaterialKind::Points);
    assert_eq!(s.materials[0].number("size"), Some(2.0));
    let d = digest(&s);
    assert_eq!(d.meshes[0].area, None, "points are not triangles");
    assert_eq!(d.meshes[0].bounds, Some(vec![1.0, 2.0, 3.0, 7.0, 8.0, 9.0]));
}
