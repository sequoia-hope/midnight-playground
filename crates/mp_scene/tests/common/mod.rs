//! A small scene the format, digest and number tests share: an indexed
//! triangle drawn by a plain mesh and by an instanced mesh, a 2×1 texture
//! referenced from a material and a shader uniform, a light, and helpers to
//! rewrite a file's header.

#![allow(dead_code)]

use mp_scene::*;
use serde_json::{Map, Value, json};

pub const IDENTITY: [f64; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

pub fn node(name: &str, ty: NodeType, parent: Option<u32>) -> NodeDesc {
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

pub fn material(kind: MaterialKind, ty: &str, params: Value) -> MaterialDesc {
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

pub fn buffer(item_size: u32, data: BufferData) -> Buffer {
    Buffer {
        item_size,
        normalized: false,
        data,
    }
}

pub fn texture(width: u32, height: u32, pixels: u32) -> TextureDesc {
    TextureDesc {
        name: "t".into(),
        source: TextureSource::Canvas,
        url: None,
        width,
        height,
        channels: 4,
        pixels,
        format: three::RGBA_FORMAT,
        ty: three::UNSIGNED_BYTE_TYPE,
        flip_y: true,
        color_space: "srgb".into(),
        premultiply_alpha: false,
        unpack_alignment: 4,
        generate_mipmaps: true,
        wrap_s: three::REPEAT_WRAPPING,
        wrap_t: three::CLAMP_TO_EDGE_WRAPPING,
        mag_filter: three::LINEAR_FILTER,
        min_filter: three::LINEAR_MIPMAP_LINEAR_FILTER,
        anisotropy: 8.0,
        offset: [0.0, 0.0],
        repeat: [1.0, 1.0],
        rotation: 0.0,
        center: [0.0, 0.0],
        matrix_auto_update: true,
        channel: 0,
    }
}

pub fn mesh(position: u32, index: Option<u32>) -> MeshDesc {
    MeshDesc {
        name: "tri".into(),
        attributes: vec![AttributeRef {
            name: "position".into(),
            accessor: position,
            instanced: false,
            mesh_per_attribute: None,
        }],
        index,
        groups: vec![],
        draw_range: DrawRange {
            start: 0,
            count: None,
        },
        bounding_box: None,
        bounding_sphere: None,
    }
}

fn translate(x: f64, y: f64, z: f64) -> [f64; 16] {
    let mut m = IDENTITY;
    m[12] = x;
    m[13] = y;
    m[14] = z;
    m
}

/// Buffers: 0 positions (right triangle, legs 2), 1 index, 2 `aLane`,
/// 3 instance matrices (two, the second 10 m along x), 4 texture pixels,
/// 5 instance colours. Nodes: 0 root group, 1 mesh, 2 instanced mesh,
/// 3 directional light.
pub fn sample() -> Scene {
    let buffers = vec![
        buffer(
            3,
            BufferData::F32(vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, 0.0]),
        ),
        buffer(1, BufferData::U16(vec![0, 1, 2])),
        buffer(1, BufferData::F32(vec![0.25, 0.5, 0.75])),
        buffer(
            16,
            BufferData::F32(
                [IDENTITY, translate(10.0, 0.0, 0.0)]
                    .iter()
                    .flatten()
                    .map(|&x| x as f32)
                    .collect(),
            ),
        ),
        buffer(4, BufferData::U8(vec![255, 0, 0, 255, 0, 255, 0, 128])),
        buffer(3, BufferData::F32(vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0])),
    ];
    let mut m = mesh(0, Some(1));
    m.attributes.push(AttributeRef {
        name: "aLane".into(),
        accessor: 2,
        instanced: false,
        mesh_per_attribute: None,
    });
    let mut root = node("root", NodeType::Group, None);
    root.children = vec![1, 2, 3];
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
    let mut sun = node("sun", NodeType::DirectionalLight, Some(0));
    sun.light = Some(0);
    let mut shader = material(
        MaterialKind::SkyDome,
        "ShaderMaterial",
        json!({ "transparent": true }),
    );
    shader.uniforms = Some(
        json!({ "tStars": { "texture": 0 }, "uTime": 0.0 })
            .as_object()
            .unwrap()
            .clone(),
    );
    Scene {
        meta: json!({ "name": "sample", "level": "sierra" }),
        buffers,
        meshes: vec![m],
        instances: vec![InstanceDesc {
            node: 2,
            count: 2,
            capacity: 2,
            matrices: 3,
            colors: Some(5),
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
            shader,
        ],
        textures: vec![texture(2, 1, 4)],
        nodes: vec![root, a, b, sun],
        roots: vec![0],
        lights: vec![LightDesc {
            node: 3,
            ty: NodeType::DirectionalLight,
            color: [1.0, 0.9, 0.8],
            intensity: 2.5,
            cast_shadow: true,
            ground_color: None,
            distance: None,
            decay: None,
            angle: None,
            penumbra: None,
            target: Some([0.0, 0.0, 0.0]),
            shadow: None,
        }],
        night_params: vec![NightParam {
            material: 0,
            prop: "emissiveIntensity".into(),
            day: 0.0,
            night: 0.06,
        }],
        environment: None,
    }
}

/// The header JSON of a `.mrscene` file.
pub fn header(bytes: &[u8]) -> Value {
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    serde_json::from_slice(&bytes[16..16 + n]).unwrap()
}

/// The same file with its header rewritten by `f`: the binary section is
/// kept and the lengths and padding are redone, so only what `f` changed
/// differs.
pub fn rewrite(bytes: &[u8], f: impl FnOnce(&mut Value)) -> Vec<u8> {
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let bin = bytes[(16 + n).div_ceil(8) * 8..].to_vec();
    let mut h = header(bytes);
    f(&mut h);
    let json = serde_json::to_vec(&h).unwrap();
    let mut out = bytes[..12].to_vec();
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(&json);
    out.resize((16 + json.len()).div_ceil(8) * 8, b' ');
    out.extend(bin);
    out
}
