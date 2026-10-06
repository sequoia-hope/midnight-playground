//! The `.mrscene` reader and writer (FORMAT.md).

use crate::{
    Buffer, BufferData, Component, Environment, InstanceDesc, LightDesc, MaterialDesc, MeshDesc,
    NightParam, NodeDesc, Scene, TextureDesc,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// The first eight bytes of every file.
pub const MAGIC: &[u8; 8] = b"MRSCENE\0";
/// The format version this crate reads and writes.
pub const VERSION: u32 = 1;

/// Where a buffer lives in the binary section.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AccessorDesc {
    /// Byte offset from the start of the binary section; a multiple of 8.
    offset: u64,
    /// Elements (each `item_size` components).
    count: u64,
    item_size: u32,
    component: Component,
    normalized: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: String,
    version: u32,
    meta: Value,
    binary_length: u64,
    accessors: Vec<AccessorDesc>,
    meshes: Vec<MeshDesc>,
    instances: Vec<InstanceDesc>,
    materials: Vec<MaterialDesc>,
    textures: Vec<TextureDesc>,
    nodes: Vec<NodeDesc>,
    roots: Vec<u32>,
    lights: Vec<LightDesc>,
    night_params: Vec<NightParam>,
    environment: Option<Environment>,
}

/// Rounds up to a multiple of 8.
fn align8(n: u64) -> u64 {
    n.div_ceil(8) * 8
}

/// Byte offset of each buffer in the binary section, and its total length:
/// buffers in order, each at the next multiple of 8 (as the JS exporter lays
/// them out).
pub(crate) fn layout(buffers: &[Buffer]) -> (Vec<u64>, u64) {
    let mut at = 0u64;
    let mut offsets = Vec::with_capacity(buffers.len());
    for b in buffers {
        let off = align8(at);
        offsets.push(off);
        at = off + (b.data.len() * b.data.component().size()) as u64;
    }
    (offsets, at)
}

/// Parses a scene from the bytes of a `.mrscene` file.
pub fn read(bytes: &[u8]) -> Result<Scene, String> {
    if bytes.len() < 16 || &bytes[..8] != MAGIC {
        return Err("not a .mrscene file (bad magic)".into());
    }
    let version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());
    if version != VERSION {
        return Err(format!(
            "format version {version}, this reader knows {VERSION}"
        ));
    }
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let json_end = 16 + json_len;
    if bytes.len() < json_end {
        return Err("file ends inside the header".into());
    }
    let header: Header =
        serde_json::from_slice(&bytes[16..json_end]).map_err(|e| format!("header: {e}"))?;
    if header.format != "mrscene" || header.version != VERSION {
        return Err(format!(
            "header says {} version {}",
            header.format, header.version
        ));
    }
    let bin_start = align8(json_end as u64) as usize;
    let bin = bytes
        .get(bin_start..)
        .ok_or("file ends before the binary section")?;
    if bin.len() as u64 != header.binary_length {
        return Err(format!(
            "binary section is {} bytes, header says {}",
            bin.len(),
            header.binary_length
        ));
    }
    let mut buffers = Vec::with_capacity(header.accessors.len());
    for (i, a) in header.accessors.iter().enumerate() {
        if a.offset % 8 != 0 {
            return Err(format!(
                "accessor {i}: offset {} is not 8-aligned",
                a.offset
            ));
        }
        let len = a.count * u64::from(a.item_size) * a.component.size() as u64;
        let range = a.offset as usize..(a.offset + len) as usize;
        let slice = bin
            .get(range)
            .ok_or_else(|| format!("accessor {i}: past the end of the binary section"))?;
        buffers.push(Buffer {
            item_size: a.item_size,
            normalized: a.normalized,
            data: BufferData::from_le_bytes(a.component, slice),
        });
    }
    let scene = Scene {
        meta: header.meta,
        buffers,
        meshes: header.meshes,
        instances: header.instances,
        materials: header.materials,
        textures: header.textures,
        nodes: header.nodes,
        roots: header.roots,
        lights: header.lights,
        night_params: header.night_params,
        environment: header.environment,
    };
    validate(&scene)?;
    Ok(scene)
}

pub fn read_file(path: impl AsRef<Path>) -> Result<Scene, String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    read(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// The bytes of a `.mrscene` file for `scene`.
pub fn write(scene: &Scene) -> Result<Vec<u8>, String> {
    validate(scene)?;
    let (offsets, bin_len) = layout(&scene.buffers);
    let header = Header {
        format: "mrscene".into(),
        version: VERSION,
        meta: scene.meta.clone(),
        binary_length: bin_len,
        accessors: scene
            .buffers
            .iter()
            .zip(&offsets)
            .map(|(b, &offset)| AccessorDesc {
                offset,
                count: b.count() as u64,
                item_size: b.item_size,
                component: b.data.component(),
                normalized: b.normalized,
            })
            .collect(),
        meshes: scene.meshes.clone(),
        instances: scene.instances.clone(),
        materials: scene.materials.clone(),
        textures: scene.textures.clone(),
        nodes: scene.nodes.clone(),
        roots: scene.roots.clone(),
        lights: scene.lights.clone(),
        night_params: scene.night_params.clone(),
        environment: scene.environment.clone(),
    };
    let json = serde_json::to_vec(&header).map_err(|e| format!("header: {e}"))?;
    let json_len = u32::try_from(json.len()).map_err(|_| "header over 4 GB")?;
    let bin_start = align8(16 + json.len() as u64) as usize;
    let mut out = Vec::with_capacity(bin_start + bin_len as usize);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&json_len.to_le_bytes());
    out.extend_from_slice(&json);
    out.resize(bin_start, b' ');
    for (b, &off) in scene.buffers.iter().zip(&offsets) {
        out.resize(bin_start + off as usize, 0);
        out.extend_from_slice(&b.data.to_le_bytes());
    }
    debug_assert_eq!(out.len(), bin_start + bin_len as usize);
    Ok(out)
}

pub fn write_file(path: impl AsRef<Path>, scene: &Scene) -> Result<(), String> {
    let path = path.as_ref();
    std::fs::write(path, write(scene)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// Every index points at something that exists and every buffer fits its
/// item size.
fn validate(s: &Scene) -> Result<(), String> {
    let nb = s.buffers.len() as u32;
    let buf = |i: u32, what: &str| -> Result<(), String> {
        if i < nb {
            Ok(())
        } else {
            Err(format!("{what}: buffer {i} does not exist"))
        }
    };
    for (i, b) in s.buffers.iter().enumerate() {
        if b.item_size == 0 || b.data.len() % b.item_size as usize != 0 {
            return Err(format!(
                "buffer {i}: {} values do not fit item size {}",
                b.data.len(),
                b.item_size
            ));
        }
    }
    for (i, m) in s.meshes.iter().enumerate() {
        for a in &m.attributes {
            buf(a.accessor, &format!("mesh {i} attribute {}", a.name))?;
        }
        if let Some(ix) = m.index {
            buf(ix, &format!("mesh {i} index"))?;
        }
    }
    for (i, inst) in s.instances.iter().enumerate() {
        buf(inst.matrices, &format!("instances {i} matrices"))?;
        if let Some(c) = inst.colors {
            buf(c, &format!("instances {i} colours"))?;
        }
        if inst.node as usize >= s.nodes.len() {
            return Err(format!("instances {i}: node {} does not exist", inst.node));
        }
    }
    for (i, t) in s.textures.iter().enumerate() {
        buf(t.pixels, &format!("texture {i} pixels"))?;
        let b = &s.buffers[t.pixels as usize];
        if b.count() != (t.width as usize) * (t.height as usize) || b.item_size != t.channels {
            return Err(format!(
                "texture {i}: {}×{}×{} does not match its buffer",
                t.width, t.height, t.channels
            ));
        }
    }
    let nm = s.materials.len() as u32;
    for (i, m) in s.materials.iter().enumerate() {
        if let Some(t) = m
            .textures()
            .into_iter()
            .find(|&t| t as usize >= s.textures.len())
        {
            return Err(format!("material {i}: texture {t} does not exist"));
        }
    }
    for (i, n) in s.nodes.iter().enumerate() {
        let ok = n.mesh.is_none_or(|m| (m as usize) < s.meshes.len())
            && n.materials.iter().all(|&m| m < nm)
            && n.instances.is_none_or(|k| (k as usize) < s.instances.len())
            && n.light.is_none_or(|k| (k as usize) < s.lights.len())
            && n.parent.is_none_or(|p| (p as usize) < s.nodes.len())
            && n.children.iter().all(|&c| (c as usize) < s.nodes.len());
        if !ok {
            return Err(format!("node {i}: an index points past its table"));
        }
    }
    if let Some(r) = s.roots.iter().find(|&&r| r as usize >= s.nodes.len()) {
        return Err(format!("root {r} does not exist"));
    }
    if let Some(p) = s.night_params.iter().find(|p| p.material >= nm) {
        return Err(format!(
            "night param: material {} does not exist",
            p.material
        ));
    }
    Ok(())
}
