//! Scene digests: a small description of a scene that two scenes can be
//! compared by (roadmap WP 0.5, SPEC 5.7). The JS exporter computes one from
//! the live three.js objects (`tools/parity/lib/scene-page.js`); [`digest`]
//! computes the same from a [`Scene`], with the same arithmetic in the same
//! order, so a file read back must give an identical digest.
//!
//! - Per mesh: vertex and index counts, local bounds, a SHA-256 of each
//!   attribute's bytes and of the index, and for meshes drawn as triangles
//!   the surface area and area-weighted centroid.
//! - Per texture: size, channels and a SHA-256 of the pixels.
//! - Per material: kind, three.js type and the textures it uses.
//! - Per drawable node: its mesh, kinds and textures, instance count and
//!   hashes, and world-space bounds over every vertex (of every instance).

use crate::file::layout;
use crate::num;
use crate::{Buffer, BufferData, MaterialKind, NodeType, Scene};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDigest {
    pub version: u32,
    pub name: Option<String>,
    pub counts: Counts,
    /// Materials per kind, in order of first use.
    pub kinds: Map<String, Value>,
    pub meshes: Vec<MeshDigest>,
    pub textures: Vec<TextureDigest>,
    pub materials: Vec<MaterialDigest>,
    pub drawables: Vec<DrawableDigest>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub nodes: u64,
    pub meshes: u64,
    pub materials: u64,
    pub textures: u64,
    pub instances: u64,
    pub lights: u64,
    pub drawables: u64,
    pub vertices: u64,
    pub indices: u64,
    pub binary_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshDigest {
    pub vertices: u64,
    pub indices: u64,
    /// Min then max of each position component (up to three).
    #[serde(with = "num::opt_seq")]
    pub bounds: Option<Vec<f64>>,
    /// SHA-256 (hex) of each attribute's bytes, by name.
    pub attributes: Map<String, Value>,
    pub index: Option<String>,
    pub area: Option<f64>,
    pub centroid: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureDigest {
    pub width: u32,
    pub height: u32,
    pub channels: u32,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDigest {
    pub kind: MaterialKind,
    #[serde(rename = "type")]
    pub ty: String,
    pub textures: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrawableDigest {
    pub node: u32,
    #[serde(rename = "type")]
    pub ty: NodeType,
    pub mesh: u32,
    pub kinds: Vec<MaterialKind>,
    pub textures: Vec<u32>,
    pub instances: Option<u32>,
    pub instance_matrices: Option<String>,
    pub instance_colors: Option<String>,
    #[serde(with = "num::opt_seq")]
    pub world_bounds: Option<Vec<f64>>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn buffer_sha(b: &Buffer) -> String {
    sha256_hex(&b.data.to_le_bytes())
}

/// The digest of a scene.
pub fn digest(scene: &Scene) -> SceneDigest {
    let used_as_mesh: Vec<bool> = {
        let mut u = vec![false; scene.meshes.len()];
        for n in &scene.nodes {
            if let (Some(m), true) = (n.mesh, n.ty.is_mesh()) {
                u[m as usize] = true;
            }
        }
        u
    };
    let meshes: Vec<MeshDigest> = scene
        .meshes
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let pos = m
                .attribute("position")
                .map(|a| &scene.buffers[a.accessor as usize]);
            let index = m.index.map(|ix| &scene.buffers[ix as usize]);
            let mut attributes = Map::new();
            for a in &m.attributes {
                attributes.insert(
                    a.name.clone(),
                    Value::String(buffer_sha(&scene.buffers[a.accessor as usize])),
                );
            }
            let (area, centroid) = match pos {
                Some(p) if used_as_mesh[i] && p.item_size == 3 => {
                    area_centroid(&p.data, index.map(|b| &b.data), p.count())
                }
                _ => (None, None),
            };
            MeshDigest {
                vertices: pos.map_or(0, |p| p.count() as u64),
                indices: index.map_or(0, |b| b.data.len() as u64),
                bounds: pos.and_then(local_bounds),
                attributes,
                index: index.map(buffer_sha),
                area,
                centroid,
            }
        })
        .collect();
    let textures = scene
        .textures
        .iter()
        .map(|t| TextureDigest {
            width: t.width,
            height: t.height,
            channels: t.channels,
            sha256: buffer_sha(&scene.buffers[t.pixels as usize]),
        })
        .collect();
    let materials: Vec<MaterialDigest> = scene
        .materials
        .iter()
        .map(|m| MaterialDigest {
            kind: m.kind,
            ty: m.ty.clone(),
            textures: m.textures(),
        })
        .collect();
    let mut drawables = Vec::new();
    for (i, n) in scene.nodes.iter().enumerate() {
        let Some(mesh) = n.mesh else { continue };
        let inst = n.instances.map(|k| &scene.instances[k as usize]);
        let mut textures = Vec::new();
        for &m in &n.materials {
            for &t in &materials[m as usize].textures {
                if !textures.contains(&t) {
                    textures.push(t);
                }
            }
        }
        let pos = scene.meshes[mesh as usize]
            .attribute("position")
            .map(|a| &scene.buffers[a.accessor as usize]);
        let im = inst.map(|k| &scene.buffers[k.matrices as usize]);
        drawables.push(DrawableDigest {
            node: i as u32,
            ty: n.ty,
            mesh,
            kinds: n
                .materials
                .iter()
                .map(|&m| scene.materials[m as usize].kind)
                .collect(),
            textures,
            instances: inst.map(|k| k.count),
            instance_matrices: im.map(buffer_sha),
            instance_colors: inst
                .and_then(|k| k.colors)
                .map(|c| buffer_sha(&scene.buffers[c as usize])),
            world_bounds: match pos {
                Some(p) if p.item_size == 3 => world_bounds(
                    &p.data,
                    p.count(),
                    &n.matrix_world,
                    im.map(|b| &b.data),
                    inst.map_or(0, |k| k.count as usize),
                ),
                _ => None,
            },
        });
    }
    let mut kinds = Map::new();
    for m in &scene.materials {
        let name = serde_json::to_value(m.kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let c = kinds.get(&name).and_then(Value::as_u64).unwrap_or(0);
        kinds.insert(name, Value::from(c + 1));
    }
    SceneDigest {
        version: 1,
        name: scene
            .meta
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned),
        counts: Counts {
            nodes: scene.nodes.len() as u64,
            meshes: scene.meshes.len() as u64,
            materials: scene.materials.len() as u64,
            textures: scene.textures.len() as u64,
            instances: scene.instances.len() as u64,
            lights: scene.lights.len() as u64,
            drawables: drawables.len() as u64,
            vertices: meshes.iter().map(|m| m.vertices).sum(),
            indices: meshes.iter().map(|m| m.indices).sum(),
            binary_bytes: layout(&scene.buffers).1,
        },
        kinds,
        meshes,
        textures,
        materials,
        drawables,
    }
}

fn local_bounds(p: &Buffer) -> Option<Vec<f64>> {
    let n = p.count();
    if n == 0 {
        return None;
    }
    let k = p.item_size as usize;
    let dims = k.min(3);
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for i in 0..n {
        for c in 0..dims {
            let v = p.data.get(i * k + c);
            if v < lo[c] {
                lo[c] = v;
            }
            if v > hi[c] {
                hi[c] = v;
            }
        }
    }
    Some(lo[..dims].iter().chain(&hi[..dims]).copied().collect())
}

/// Surface area and area-weighted centroid, as `areaCentroid` in the JS.
fn area_centroid(
    a: &BufferData,
    index: Option<&BufferData>,
    count: usize,
) -> (Option<f64>, Option<[f64; 3]>) {
    let n = index.map_or(count, BufferData::len);
    let at = |t: usize| -> usize { index.map_or(t, |ix| ix.get(t) as usize) * 3 };
    let (mut area, mut cx, mut cy, mut cz) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for t in 0..n / 3 {
        let (i0, i1, i2) = (at(t * 3), at(t * 3 + 1), at(t * 3 + 2));
        let (ax, ay, az) = (a.get(i0), a.get(i0 + 1), a.get(i0 + 2));
        let (ux, uy, uz) = (a.get(i1) - ax, a.get(i1 + 1) - ay, a.get(i1 + 2) - az);
        let (vx, vy, vz) = (a.get(i2) - ax, a.get(i2 + 1) - ay, a.get(i2 + 2) - az);
        let nx = uy * vz - uz * vy;
        let ny = uz * vx - ux * vz;
        let nz = ux * vy - uy * vx;
        let ar = 0.5 * (nx * nx + ny * ny + nz * nz).sqrt();
        area += ar;
        cx += ar * ((ax + a.get(i1) + a.get(i2)) / 3.0);
        cy += ar * ((ay + a.get(i1 + 1) + a.get(i2 + 1)) / 3.0);
        cz += ar * ((az + a.get(i1 + 2) + a.get(i2 + 2)) / 3.0);
    }
    let centroid = (area > 0.0).then(|| [cx / area, cy / area, cz / area]);
    (Some(area), centroid)
}

/// World-space bounds over every vertex, as `worldBounds` in the JS:
/// `e × (m ×) p`, affine, evaluated left to right.
fn world_bounds(
    a: &BufferData,
    n: usize,
    e: &[f64; 16],
    im: Option<&BufferData>,
    count: usize,
) -> Option<Vec<f64>> {
    if n == 0 {
        return None;
    }
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut put = |x: f64, y: f64, z: f64| {
        let w = [
            e[0] * x + e[4] * y + e[8] * z + e[12],
            e[1] * x + e[5] * y + e[9] * z + e[13],
            e[2] * x + e[6] * y + e[10] * z + e[14],
        ];
        for c in 0..3 {
            if w[c] < lo[c] {
                lo[c] = w[c];
            }
            if w[c] > hi[c] {
                hi[c] = w[c];
            }
        }
    };
    match im {
        Some(im) => {
            if count == 0 {
                return None;
            }
            for k in 0..count {
                let m = |j: usize| im.get(k * 16 + j);
                for i in 0..n {
                    let (x, y, z) = (a.get(i * 3), a.get(i * 3 + 1), a.get(i * 3 + 2));
                    put(
                        m(0) * x + m(4) * y + m(8) * z + m(12),
                        m(1) * x + m(5) * y + m(9) * z + m(13),
                        m(2) * x + m(6) * y + m(10) * z + m(14),
                    );
                }
            }
        }
        None => {
            for i in 0..n {
                put(a.get(i * 3), a.get(i * 3 + 1), a.get(i * 3 + 2));
            }
        }
    }
    Some(lo.iter().chain(&hi).copied().collect())
}

/// The differences between two digests, as readable lines (empty when they
/// are identical). Numbers must match exactly: both sides compute them with
/// the same IEEE operations in the same order.
pub fn compare(expected: &SceneDigest, actual: &SceneDigest) -> Vec<String> {
    let mut out = Vec::new();
    if expected.counts != actual.counts {
        out.push(format!(
            "counts: expected {:?}, got {:?}",
            expected.counts, actual.counts
        ));
    }
    if expected.kinds != actual.kinds {
        out.push(format!(
            "kinds: expected {:?}, got {:?}",
            expected.kinds, actual.kinds
        ));
    }
    list(&mut out, "mesh", &expected.meshes, &actual.meshes);
    list(&mut out, "texture", &expected.textures, &actual.textures);
    list(&mut out, "material", &expected.materials, &actual.materials);
    list(&mut out, "drawable", &expected.drawables, &actual.drawables);
    out
}

fn list<T: PartialEq + std::fmt::Debug>(out: &mut Vec<String>, what: &str, e: &[T], a: &[T]) {
    if e.len() != a.len() {
        out.push(format!("{what}s: expected {}, got {}", e.len(), a.len()));
    }
    for (i, (x, y)) in e.iter().zip(a).enumerate() {
        if x != y {
            out.push(format!("{what} {i}: expected {x:?}\n    got {y:?}"));
        }
    }
}
