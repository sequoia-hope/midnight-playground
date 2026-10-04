//! The helpers at the top of `Mountain.js`: the rendered-surface sampler,
//! `instanced`, `placed`, `bakeStatic`, `mergedMesh` and the triplanar
//! rock material.

use std::collections::BTreeMap;

use mr_math::{js, kernel};
use mr_scene::{BufferData, MaterialKind};
use serde_json::Value;

use crate::color::Color;
use crate::material::{Material, texture_value};
use crate::object::{GeoId, Layer, MaterialId, NodeId, SceneGraph};
use crate::terrain::Terrain;
use crate::textures::TextureCache;
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, Matrix4, Quaternion, Vector3, merge_geometries,
};

// ── Terrain surface as rendered ─────────────────────────────────────────

/// What [`SurfaceSampler::sample`] writes into its `out` object.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Surface {
    pub h: f64,
    pub gx: f64,
    pub gz: f64,
    pub slope: f64,
    pub step: f64,
}

/// `SurfaceSampler`: coarse terrain tiles interpolate between 16/32 m
/// samples, so the exact height function can sit metres above or below
/// what is drawn. Objects are placed on the rendered triangles instead.
pub struct SurfaceSampler<'a> {
    t: &'a Terrain,
    /// `'i,j'` → the tile's step.
    steps: BTreeMap<(i64, i64), f64>,
    /// The height cache, keyed as the JS keys it (the value is the height
    /// at the first point asked for with that key). Only looked up.
    cache: BTreeMap<u64, f64>,
}

impl<'a> SurfaceSampler<'a> {
    pub fn new(terrain: &'a Terrain) -> SurfaceSampler<'a> {
        let mut steps = BTreeMap::new();
        for t in terrain.tile_list() {
            steps.insert((t.i, t.j), t.step);
        }
        SurfaceSampler {
            t: terrain,
            steps,
            cache: BTreeMap::new(),
        }
    }

    /// `h(x, z)`: the terrain height through the cache.
    fn h(&mut self, x: f64, z: f64) -> f64 {
        let t = self.t;
        let key = js::round((x - t.min_x) / 4.0) * 65536.0 + js::round((z - t.min_z) / 4.0);
        // A Map key is compared as SameValueZero: -0 is 0.
        let key = (key + 0.0).to_bits();
        if let Some(&v) = self.cache.get(&key) {
            return v;
        }
        let v = t.height_at(x, z);
        self.cache.insert(key, v);
        v
    }

    /// `sample(x, z, out)`.
    pub fn sample(&mut self, x: f64, z: f64) -> Surface {
        let t = self.t;
        let ti = ((x - t.min_x) / 256.0).floor();
        let tj = ((z - t.min_z) / 256.0).floor();
        let step = self
            .steps
            .get(&(ti as i64, tj as i64))
            .copied()
            .unwrap_or(32.0);
        let x0 = t.min_x + ti * 256.0;
        let z0 = t.min_z + tj * 256.0;
        let gx = (x - x0) / step;
        let gz = (z - z0) / step;
        let ci = gx.floor();
        let cj = gz.floor();
        let fx = gx - ci;
        let fz = gz - cj;
        let ax = x0 + ci * step;
        let az = z0 + cj * step;
        let ha = self.h(ax, az);
        let hb = self.h(ax + step, az);
        let hc = self.h(ax, az + step);
        let hd = self.h(ax + step, az + step);
        // `(ci + cj) & 1` on the JS numbers.
        let odd = js::to_int32(ci + cj) & 1 != 0;
        let h = if odd {
            if fx + fz < 1.0 {
                ha + (hb - ha) * fx + (hc - ha) * fz
            } else {
                hd + (hc - hd) * (1.0 - fx) + (hb - hd) * (1.0 - fz)
            }
        } else if fz > fx {
            ha + (hd - hc) * fx + (hc - ha) * fz
        } else {
            ha + (hb - ha) * fx + (hd - hb) * fz
        };
        let gx = (hb - ha + hd - hc) * 0.5 / step;
        let gz = (hc - ha + hd - hb) * 0.5 / step;
        Surface {
            h,
            gx,
            gz,
            slope: kernel::hypot(gx, gz),
            step,
        }
    }
}

// ── Instancing and merging ──────────────────────────────────────────────

/// One instance: `{ x, y, z, sx, sy, sz, q?, rx?, ry?, rz?, col?, b? }`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Item {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub sx: f64,
    pub sy: f64,
    pub sz: f64,
    pub q: Option<Quaternion>,
    pub rx: f64,
    pub ry: f64,
    pub rz: f64,
    pub col: Option<u32>,
    pub b: Option<f64>,
}

impl Item {
    /// An instance at (x, y, z) with a scale and nothing else.
    pub fn at(x: f64, y: f64, z: f64, sx: f64, sy: f64, sz: f64) -> Item {
        Item {
            x,
            y,
            z,
            sx,
            sy,
            sz,
            ..Item::default()
        }
    }
}

/// `instanced(geo, mat, items, { cast = false, receive = true })`.
pub fn instanced(
    graph: &mut SceneGraph,
    geo: GeoId,
    mat: MaterialId,
    items: &[Item],
    cast: bool,
    receive: bool,
) -> NodeId {
    let im = graph.instanced_mesh(geo, mat, items.len() as u32);
    {
        let inst = graph.get_mut(im).instances.as_mut().expect("instanced");
        for (k, it) in items.iter().enumerate() {
            let q = match it.q {
                Some(q) => q,
                None => Quaternion::from_euler(&Euler::new(
                    js::or(it.rx, 0.0),
                    js::or(it.ry, 0.0),
                    js::or(it.rz, 0.0),
                )),
            };
            let m = Matrix4::compose(
                Vector3::new(it.x, it.y, it.z),
                q,
                Vector3::new(it.sx, it.sy, it.sz),
            );
            inst.set_matrix_at(k, &m);
            if let Some(col) = it.col {
                let mut c = Color::hex(col);
                c.multiply_scalar(it.b.unwrap_or(1.0));
                inst.set_color_at(k, c);
            }
        }
    }
    graph.compute_instance_bounding_sphere(im);
    let o = graph.get_mut(im);
    o.cast_shadow = cast;
    o.receive_shadow = receive;
    o.matrix_auto_update = false;
    o.update_matrix();
    im
}

/// `placed(geo, x, y, z, yaw = 0, sx = 1, sy = 1, sz = 1)`: a merged-
/// geometry piece at a pose (a copy of `geo`).
#[allow(clippy::too_many_arguments)]
pub fn placed_scaled(
    geo: &BufferGeometry,
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
    sx: f64,
    sy: f64,
    sz: f64,
) -> BufferGeometry {
    let mut g = geo.clone();
    let q = Quaternion::from_euler(&Euler::new(0.0, yaw, 0.0));
    let m = Matrix4::compose(Vector3::new(x, y, z), q, Vector3::new(sx, sy, sz));
    g.apply_matrix4(&m);
    g
}

/// `placed(geo, x, y, z, yaw)` at unit scale.
pub fn placed(geo: &BufferGeometry, x: f64, y: f64, z: f64, yaw: f64) -> BufferGeometry {
    placed_scaled(geo, x, y, z, yaw, 1.0, 1.0, 1.0)
}

/// Keeps only the attributes named, in their order.
fn keep_only(h: &mut BufferGeometry, keep: &[&str]) {
    let names: Vec<String> = h.attributes.iter().map(|(n, _)| n.clone()).collect();
    for a in names {
        if !keep.contains(&a.as_str()) {
            h.delete_attribute(&a);
        }
    }
}

/// A zero `uv` (`new Float32Array(count * 2)`).
fn zero_uv(h: &mut BufferGeometry) {
    let n = h.position().count();
    h.set_attribute(
        "uv",
        BufferAttribute::new(BufferData::F32(vec![0.0; n * 2]), 2, false),
    );
}

/// `mergedMesh(geos, mat, { cast = false, receive = true })`: `None` for
/// no pieces (the JS returns `null`).
pub fn merged_mesh(
    graph: &mut SceneGraph,
    geos: Vec<BufferGeometry>,
    mat: MaterialId,
    cast: bool,
    receive: bool,
) -> Option<NodeId> {
    if geos.is_empty() {
        return None;
    }
    let norm: Vec<BufferGeometry> = geos
        .into_iter()
        .map(|g| {
            let mut h = if g.index.is_some() {
                g.to_non_indexed()
            } else {
                g
            };
            keep_only(&mut h, &["position", "normal", "uv"]);
            if !h.has_attribute("uv") {
                zero_uv(&mut h);
            }
            h
        })
        .collect();
    let refs: Vec<&BufferGeometry> = norm.iter().collect();
    let merged = merge_geometries(&refs, false).expect("the pieces share their attributes");
    let geo = graph.add_geometry(merged);
    let m = graph.mesh(geo, mat);
    let o = graph.get_mut(m);
    o.cast_shadow = cast;
    o.receive_shadow = receive;
    o.matrix_auto_update = false;
    o.update_matrix();
    Some(m)
}

/// `root.updateMatrixWorld(true)` for a hierarchy not in the scene: each
/// node's world matrix, in traversal order (pre-order, children in order).
pub fn world_matrices(graph: &mut SceneGraph, root: NodeId) -> Vec<(NodeId, Matrix4)> {
    let mut out = Vec::new();
    let mut stack = vec![(root, None::<Matrix4>)];
    while let Some((id, parent)) = stack.pop() {
        let o = graph.get_mut(id);
        if o.matrix_auto_update {
            o.update_matrix();
        }
        let world = match parent {
            Some(p) => p.multiply(&o.matrix),
            None => o.matrix,
        };
        out.push((id, world));
        for &c in o.children.iter().rev() {
            stack.push((c, Some(world)));
        }
    }
    out
}

/// `bakeStatic(root)`: collapse a static prop hierarchy (a parked car)
/// into one mesh per material, so decoration doesn't cost a draw call per
/// part. Returns the new group (not added anywhere).
pub fn bake_static(graph: &mut SceneGraph, root: NodeId) -> NodeId {
    let nodes = world_matrices(graph, root);
    // `byMat`: a Map keyed by material, in the order first met.
    let mut by_mat: Vec<(MaterialId, Vec<BufferGeometry>)> = Vec::new();
    for (id, world) in nodes {
        let o = graph.get(id);
        let is_mesh = matches!(
            o.ty,
            mr_scene::NodeType::Mesh | mr_scene::NodeType::InstancedMesh
        );
        if !is_mesh || o.multi_material {
            continue;
        }
        let Some(geo) = o.geometry else { continue };
        let mat = o.materials[0];
        let mut g = graph.geometry(geo).clone();
        g.apply_matrix4(&world);
        match by_mat.iter_mut().find(|(m, _)| *m == mat) {
            Some((_, list)) => list.push(g),
            None => by_mat.push((mat, vec![g])),
        }
    }
    let out = graph.group("");
    for (mat, geos) in by_mat {
        let vertex_colors = graph.material(mat).get("vertexColors") == Some(&Value::Bool(true));
        let mut keep = vec!["position", "normal", "uv"];
        if vertex_colors {
            keep.push("color");
        }
        let norm: Vec<BufferGeometry> = geos
            .into_iter()
            .map(|g| {
                let mut h = if g.index.is_some() {
                    g.to_non_indexed()
                } else {
                    g
                };
                keep_only(&mut h, &keep);
                let n = h.position().count();
                if !h.has_attribute("uv") {
                    zero_uv(&mut h);
                }
                if !h.has_attribute("normal") {
                    h.compute_vertex_normals();
                }
                if vertex_colors && !h.has_attribute("color") {
                    h.set_attribute(
                        "color",
                        BufferAttribute::new(BufferData::F32(vec![1.0; n * 3]), 3, false),
                    );
                }
                h
            })
            .collect();
        let refs: Vec<&BufferGeometry> = norm.iter().collect();
        let merged = merge_geometries(&refs, false).expect("the parts share their attributes");
        let geo = graph.add_geometry(merged);
        let m = graph.mesh(geo, mat);
        let o = graph.get_mut(m);
        o.cast_shadow = true;
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        graph.add(out, m);
    }
    out
}

// ── Materials ───────────────────────────────────────────────────────────

/// `rockMaterial(vertexColors = true)`: triplanar strata for rocks,
/// instancing-aware (TerrainMesh's patch assumes no instance matrix). The
/// patch (kind `TriplanarRock`) samples `tRock` at the world position
/// projected on the three axes, weighted by the normal cubed.
pub fn rock_material(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    vertex_colors: bool,
) -> Material {
    let tex = graph.cached_texture(&textures.rock_texture(), Layer::Main, "");
    Material::standard()
        .set("color", 0xffffff)
        .set("roughness", 0.92)
        .set("metalness", 0.0)
        .set("vertexColors", vertex_colors)
        .kind(MaterialKind::TriplanarRock, None)
        .program_key(&format!(
            "mtn-rock{}",
            if vertex_colors { "-vc" } else { "" }
        ))
        .uniform("tRock", texture_value(tex))
        .uniform("clippingPlanes", Value::Null)
}
