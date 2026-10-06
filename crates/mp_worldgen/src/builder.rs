//! Port of `src/world/valley/Builder.js`: `Builder` and `PaintBuilder`
//! (roadmap WP 3.3). `beach/ColorBuilder.js` is in [`crate::color_builder`].
//!
//! Collects primitive pieces into per-material buckets, then merges each
//! bucket into a single static mesh. Farm buildings are made of hundreds of
//! boxes; this keeps them to one draw call per material for the whole valley.
//!
//! The JS has three classes, `PaintBuilder` and `ColorBuilder` extending
//! `Builder` and overriding `add`, which every shape method (`box`, `put`,
//! `beam`, ...) calls. Here one [`Builder`] carries its [`Paint`] mode and
//! `add` dispatches on it, so a shape drawn on a paint builder paints as in
//! the JS (DECISIONS D192).

// The shape methods keep the JS argument lists (DECISIONS D130, D190).
#![allow(clippy::too_many_arguments)]

use std::sync::OnceLock;

use crate::color::Color;
use crate::object::{MaterialId, NodeId, SceneGraph};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, Matrix4, Quaternion, Vector3, box_geometry,
    merge_geometries,
};

/// `UNIT_BOX`: `new THREE.BoxGeometry(1, 1, 1)`.
fn unit_box() -> &'static BufferGeometry {
    static UNIT_BOX: OnceLock<BufferGeometry> = OnceLock::new();
    UNIT_BOX.get_or_init(|| box_geometry(1.0, 1.0, 1.0, 1.0, 1.0, 1.0))
}

const KEEP: [&str; 3] = ["position", "normal", "uv"];

/// `normalise(geo)` (and ColorBuilder's identical `strip`): non-indexed,
/// only position, normal and uv (normals computed and uvs zeroed where
/// missing), no groups.
pub fn normalise(geo: &BufferGeometry) -> BufferGeometry {
    let mut g = if geo.index.is_some() {
        geo.to_non_indexed()
    } else {
        geo.clone()
    };
    let names: Vec<String> = g.attributes.iter().map(|(n, _)| n.clone()).collect();
    for name in names {
        if !KEEP.contains(&name.as_str()) {
            g.delete_attribute(&name);
        }
    }
    if !g.has_attribute("normal") {
        g.compute_vertex_normals();
    }
    if !g.has_attribute("uv") {
        let n = g.position().count();
        g.set_attribute("uv", BufferAttribute::from_f32(vec![0.0; n * 2], 2));
    }
    g.clear_groups();
    g
}

/// What `add` does with a piece (the JS subclass).
#[derive(Clone, Debug)]
pub enum Paint {
    /// `Builder`: every key its own bucket.
    None,
    /// `PaintBuilder(groups)`: `{key: [bucket, hexColour]}`; a listed key
    /// is painted with vertex colours and filed under its bucket.
    Groups(Vec<(String, (String, Color))>),
    /// `ColorBuilder(palette)`: `{key: hex}`; a listed key is painted and
    /// filed under `solid:<channel>`.
    Palette {
        palette: Vec<(String, Color)>,
        channel: String,
    },
}

/// `build`'s options: `{ castShadow = [], receiveShadow = true }`.
#[derive(Clone, Debug)]
pub struct BuildOpts {
    pub cast_shadow: CastShadow,
    pub receive_shadow: bool,
}

/// `castShadow`: `true` or a list of keys.
#[derive(Clone, Debug)]
pub enum CastShadow {
    All,
    Keys(Vec<String>),
}

impl Default for BuildOpts {
    fn default() -> Self {
        BuildOpts {
            cast_shadow: CastShadow::Keys(Vec::new()),
            receive_shadow: true,
        }
    }
}

impl CastShadow {
    /// `castShadow === true || castShadow.includes(key)`.
    pub fn includes(&self, key: &str) -> bool {
        match self {
            CastShadow::All => true,
            CastShadow::Keys(k) => k.iter().any(|x| x == key),
        }
    }
}

/// The buckets and the placement frame.
#[derive(Clone, Debug)]
pub struct Builder {
    /// Bucket → pieces, in the order the buckets were first used.
    pub buckets: Vec<(String, Vec<BufferGeometry>)>,
    pub frame: Matrix4,
    stack: Vec<Matrix4>,
    pub paint: Paint,
}

impl Default for Builder {
    fn default() -> Self {
        Builder::new()
    }
}

/// `new Euler(x, y, z)` → quaternion (the scratch `_e` stays in XYZ order).
fn quat(x: f64, y: f64, z: f64) -> Quaternion {
    Quaternion::from_euler(&Euler::new(x, y, z))
}

impl Builder {
    /// `new Builder()`.
    pub fn new() -> Builder {
        Builder {
            buckets: Vec::new(),
            frame: Matrix4::IDENTITY,
            stack: Vec::new(),
            paint: Paint::None,
        }
    }

    /// `new PaintBuilder(groups)`: `groups` is `[(key, (bucket, hex))]` in
    /// the JS object's order.
    pub fn new_paint(groups: &[(&str, (&str, u32))]) -> Builder {
        let mut b = Builder::new();
        b.paint = Paint::Groups(
            groups
                .iter()
                .map(|(k, (bucket, hex))| (k.to_string(), (bucket.to_string(), Color::hex(*hex))))
                .collect(),
        );
        b
    }

    pub(crate) fn bucket(&mut self, key: &str) -> &mut Vec<BufferGeometry> {
        let i = match self.buckets.iter().position(|(k, _)| k == key) {
            Some(i) => i,
            None => {
                self.buckets.push((key.to_string(), Vec::new()));
                self.buckets.len() - 1
            }
        };
        &mut self.buckets[i].1
    }

    /// Place subsequent pieces relative to (x,y,z) rotated by yaw about Y.
    pub fn set_frame(&mut self, x: f64, y: f64, z: f64, yaw: f64) -> &mut Self {
        self.frame = Matrix4::compose(
            Vector3::new(x, y, z),
            quat(0.0, yaw, 0.0),
            Vector3::splat(1.0),
        );
        self
    }

    pub fn push_frame(&mut self, x: f64, y: f64, z: f64, yaw: f64) -> &mut Self {
        self.stack.push(self.frame);
        let l = Matrix4::compose(
            Vector3::new(x, y, z),
            quat(0.0, yaw, 0.0),
            Vector3::splat(1.0),
        );
        self.frame = self.frame.multiply(&l);
        self
    }

    pub fn pop_frame(&mut self) -> &mut Self {
        self.frame = self.stack.pop().expect("popFrame without pushFrame");
        self
    }

    /// `add(key, geo, local)`: the piece normalised, placed by
    /// `frame × local`, and filed (painted first if the builder paints this
    /// key). The JS returns the placed geometry; no caller uses it.
    pub fn add(&mut self, key: &str, geo: &BufferGeometry, local: Option<&Matrix4>) {
        let m = Matrix4::multiply_matrices(&self.frame, local.unwrap_or(&Matrix4::IDENTITY));
        match &self.paint {
            Paint::None => {
                let mut g = normalise(geo);
                g.apply_matrix4(&m);
                self.bucket(key).push(g);
            }
            Paint::Groups(groups) => {
                let Some((_, (bucket, col))) = groups.iter().find(|(k, _)| k == key) else {
                    let mut g = normalise(geo);
                    g.apply_matrix4(&m);
                    self.bucket(key).push(g);
                    return;
                };
                let (bucket, col) = (bucket.clone(), *col);
                let mut g = normalise(geo);
                g.apply_matrix4(&m);
                paint(&mut g, col);
                self.bucket(&bucket).push(g);
            }
            Paint::Palette { .. } => self.color_add(key, geo, &m),
        }
    }

    /// Generic transformed primitive.
    pub fn put(
        &mut self,
        key: &str,
        geo: &BufferGeometry,
        x: f64,
        y: f64,
        z: f64,
        r: [f64; 3],
        s: [f64; 3],
    ) {
        let l = Matrix4::compose(
            Vector3::new(x, y, z),
            quat(r[0], r[1], r[2]),
            Vector3::new(s[0], s[1], s[2]),
        );
        self.add(key, geo, Some(&l));
    }

    /// `put(key, geo, x, y, z)` with no rotation and unit scale.
    pub fn put_at(&mut self, key: &str, geo: &BufferGeometry, x: f64, y: f64, z: f64) {
        self.put(key, geo, x, y, z, [0.0; 3], [1.0; 3]);
    }

    /// Box whose base sits at y (not centred). Note the JS argument order
    /// of the rotation: `ry, rx, rz`.
    pub fn box_(
        &mut self,
        key: &str,
        w: f64,
        h: f64,
        d: f64,
        x: f64,
        y: f64,
        z: f64,
        ry: f64,
        rx: f64,
        rz: f64,
    ) {
        let q = quat(rx, ry, rz);
        // Offset so the pivot is at the bottom centre of the box.
        let off = Vector3::new(0.0, h / 2.0, 0.0).apply_quaternion(q);
        let l = Matrix4::compose(
            Vector3::new(x + off.x, y + off.y, z + off.z),
            q,
            Vector3::new(w, h, d),
        );
        self.add(key, unit_box(), Some(&l));
    }

    /// `box(key, w, h, d, x, y, z, ry = 0)`.
    pub fn box_yaw(&mut self, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64, ry: f64) {
        self.box_(key, w, h, d, x, y, z, ry, 0.0, 0.0);
    }

    /// Box centred at (x,y,z).
    pub fn cbox(&mut self, key: &str, w: f64, h: f64, d: f64, x: f64, y: f64, z: f64, r: [f64; 3]) {
        self.put(key, unit_box(), x, y, z, r, [w, h, d]);
    }

    /// Thin beam between two local points (`t` defaults to 0.12 in the JS).
    pub fn beam(&mut self, key: &str, a: Vector3, b: Vector3, t: f64) {
        let dir = b - a;
        let len = dir.length();
        let dir = dir.normalize();
        let q = Quaternion::from_unit_vectors(Vector3::new(0.0, 1.0, 0.0), dir);
        let mid = (a + b).multiply_scalar(0.5);
        let l = Matrix4::compose(mid, q, Vector3::new(t, len, t));
        self.add(key, unit_box(), Some(&l));
    }

    /// Build meshes: one per bucket, named `valley:<key>`, static (matrix
    /// updated once, never again), not yet in the tree. `materials` is the
    /// JS `{key: Material}` object.
    pub fn build(
        &mut self,
        graph: &mut SceneGraph,
        materials: &[(&str, MaterialId)],
        opts: &BuildOpts,
    ) -> Vec<NodeId> {
        if let Paint::Palette { .. } = self.paint {
            return self.color_build(graph, materials, opts);
        }
        let mut meshes = Vec::new();
        for (key, geos) in std::mem::take(&mut self.buckets) {
            let Some(mat) = lookup(materials, &key) else {
                graph.log.push(format!("Valley: no material for {key}"));
                continue;
            };
            let refs: Vec<&BufferGeometry> = geos.iter().collect();
            let Some(mut merged) = merge_geometries(&refs, false) else {
                continue;
            };
            merged.compute_bounding_sphere();
            let geo = graph.add_geometry(merged);
            let mesh = graph.mesh(geo, mat);
            let o = graph.get_mut(mesh);
            o.name = format!("valley:{key}");
            o.cast_shadow = opts.cast_shadow.includes(&key);
            o.receive_shadow = opts.receive_shadow;
            o.matrix_auto_update = false;
            o.update_matrix();
            meshes.push(mesh);
        }
        meshes
    }

    /// Merge everything into one geometry (for instanced parts); `None`
    /// where `mergeGeometries` fails (pieces with different attributes).
    pub fn merge_all(&mut self) -> Option<BufferGeometry> {
        let buckets = std::mem::take(&mut self.buckets);
        let all: Vec<&BufferGeometry> = buckets.iter().flat_map(|(_, g)| g.iter()).collect();
        merge_geometries(&all, false)
    }
}

/// `materials[key]`.
pub(crate) fn lookup(materials: &[(&str, MaterialId)], key: &str) -> Option<MaterialId> {
    materials.iter().find(|(k, _)| *k == key).map(|&(_, m)| m)
}

/// The vertex colour attribute: `col` at every vertex, as a `Float32Array`.
pub(crate) fn paint(g: &mut BufferGeometry, col: Color) {
    let n = g.position().count();
    let mut c = vec![0.0f32; n * 3];
    for i in 0..n {
        c[i * 3] = col.r as f32;
        c[i * 3 + 1] = col.g as f32;
        c[i * 3 + 2] = col.b as f32;
    }
    g.set_attribute("color", BufferAttribute::from_f32(c, 3));
}
