//! Port of `src/world/harbor/build.js` (roadmap WP 7.1): geometry helpers
//! for the harbour: a batcher that merges everything of one material into a
//! few spatially-chunked meshes, plus 3D beams, tapered boxes and swept
//! ribbons along arbitrary frame lists.

// The JS signatures stay (DECISIONS D130).
#![allow(clippy::too_many_arguments)]

use mr_math::kernel;
use serde_json::Value;

use crate::color::Color;
use crate::geom::{GeoBuilder, P2, P3, StaticOpts, static_mesh};
use crate::object::{MaterialId, NodeId, SceneGraph};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Matrix4, Quaternion, Vector3, merge_geometries,
};

const KEEP: [&str; 4] = ["position", "normal", "uv", "color"];

/// sRGB hex → linear [r, g, b] for vertex colours.
pub fn col(hex: u32) -> P3 {
    let c = Color::hex(hex);
    [c.r, c.g, c.b]
}

/// One bucket of the batcher: `{ mat, cast, receive, geos }`.
struct Bucket {
    key: (MaterialId, u64, u64, bool),
    mat: MaterialId,
    cast: bool,
    receive: bool,
    geos: Vec<BufferGeometry>,
}

/// `Batch.add`'s options: `{ cast = false, receive = true, chunk = 700 }`.
#[derive(Clone, Copy, Debug)]
pub struct AddOpts {
    pub cast: bool,
    pub receive: bool,
    pub chunk: f64,
}

impl Default for AddOpts {
    fn default() -> Self {
        AddOpts {
            cast: false,
            receive: true,
            chunk: 700.0,
        }
    }
}

/// `new Batch(group)`: merges everything of one material into a few
/// spatially-chunked meshes, in the order the buckets were first used (the
/// JS `Map`).
pub struct Batch {
    pub group: NodeId,
    map: Vec<Bucket>,
}

/// A `Math.floor` result as a map key part (`-0` is `0` in the key string).
fn key_part(v: f64) -> u64 {
    if v.is_nan() {
        f64::NAN.to_bits()
    } else {
        (v + 0.0).to_bits()
    }
}

impl Batch {
    pub fn new(group: NodeId) -> Batch {
        Batch {
            group,
            map: Vec::new(),
        }
    }

    /// `add(geo, mat, opts)`.
    pub fn add(&mut self, graph: &SceneGraph, geo: BufferGeometry, mat: MaterialId, o: AddOpts) {
        let mut g = if geo.index.is_some() {
            geo.to_non_indexed()
        } else {
            geo
        };
        let names: Vec<String> = g.attributes.iter().map(|(n, _)| n.clone()).collect();
        for k in names {
            if !KEEP.contains(&k.as_str()) {
                g.delete_attribute(&k);
            }
        }
        if !g.has_attribute("normal") {
            g.compute_vertex_normals();
        }
        let n = g.position().count();
        if n == 0 {
            return;
        }
        if !g.has_attribute("uv") {
            g.set_attribute("uv", BufferAttribute::from_f32(vec![0.0; n * 2], 2));
        }
        let vertex_colors = graph.material(mat).get("vertexColors") == Some(&Value::Bool(true));
        if vertex_colors {
            if !g.has_attribute("color") {
                g.set_attribute("color", BufferAttribute::from_f32(vec![1.0; n * 3], 3));
            }
        } else if g.has_attribute("color") {
            g.delete_attribute("color");
        }
        g.compute_bounding_sphere();
        let c = g.bounding_sphere.expect("a bounding sphere").center;
        let key = (
            mat,
            key_part((c.x / o.chunk).floor()),
            key_part((c.z / o.chunk).floor()),
            o.cast,
        );
        match self.map.iter_mut().find(|b| b.key == key) {
            Some(b) => b.geos.push(g),
            None => self.map.push(Bucket {
                key,
                mat,
                cast: o.cast,
                receive: o.receive,
                geos: vec![g],
            }),
        }
    }

    /// `flush()`: one static mesh per bucket, added to the group.
    pub fn flush(&mut self, graph: &mut SceneGraph) {
        for b in std::mem::take(&mut self.map) {
            let mut geo = if b.geos.len() == 1 {
                b.geos.into_iter().next().expect("one")
            } else {
                let refs: Vec<&BufferGeometry> = b.geos.iter().collect();
                merge_geometries(&refs, false).expect("the pieces merge")
            };
            geo.compute_bounding_sphere();
            let g = graph.add_geometry(geo);
            let m = static_mesh(
                graph,
                g,
                b.mat,
                &StaticOpts {
                    cast: b.cast,
                    receive: b.receive,
                    ..StaticOpts::default()
                },
            );
            graph.add(self.group, m);
        }
    }
}

/// Set a flat vertex colour on a geometry.
pub fn tint(mut geo: BufferGeometry, rgb: P3) -> BufferGeometry {
    let n = geo.position().count();
    let mut a = Vec::with_capacity(n * 3);
    for _ in 0..n {
        a.extend_from_slice(&rgb);
    }
    geo.set_attribute("color", BufferAttribute::from_f64(&a, 3));
    geo
}

const UVS: [P2; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Quad whose normal points along `out` regardless of vertex order
/// (`quadOut(geo, p0, p1, p2, p3, out, color = null, uvs = null)`).
pub fn quad_out(
    geo: &mut GeoBuilder,
    p0: P3,
    p1: P3,
    p2: P3,
    p3: P3,
    out: P3,
    color: Option<P3>,
    uvs: Option<[P2; 4]>,
) {
    let a = Vector3::new(p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]);
    let b = Vector3::new(p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]);
    let n = Vector3::cross_vectors(a, b);
    let u = uvs.unwrap_or(UVS);
    if n.x * out[0] + n.y * out[1] + n.z * out[2] < 0.0 {
        geo.quad(p0, p3, p2, p1, Some([u[0], u[3], u[2], u[1]]), color, 0.0);
    } else {
        geo.quad(p0, p1, p2, p3, Some(u), color, 0.0);
    }
}

/// Rectangular beam between two 3D points (w across, h in the "up-ish"
/// axis): `beam(geo, A, B, w, h, color = null, caps = true)`.
pub fn beam(geo: &mut GeoBuilder, a: P3, b: P3, w: f64, h: f64, color: Option<P3>, caps: bool) {
    let d = Vector3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let l = d.length();
    if l < 1e-4 {
        return;
    }
    let d = d.divide_scalar(l);
    let refv = if d.y.abs() < 0.9 {
        Vector3::new(0.0, 1.0, 0.0)
    } else {
        Vector3::new(1.0, 0.0, 0.0)
    };
    let u = Vector3::cross_vectors(d, refv).normalize();
    let v = Vector3::cross_vectors(u, d).normalize();
    let p = |base: P3, su: f64, sv: f64| -> P3 {
        [
            base[0] + u.x * su * w / 2.0 + v.x * sv * h / 2.0,
            base[1] + u.y * su * w / 2.0 + v.y * sv * h / 2.0,
            base[2] + u.z * su * w / 2.0 + v.z * sv * h / 2.0,
        ]
    };
    for (su, sv) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        let out = [
            u.x * su + v.x * sv,
            u.y * su + v.y * sv,
            u.z * su + v.z * sv,
        ];
        // Two corners of this face at each end.
        let c1 = if su != 0.0 { (su, -1.0) } else { (-1.0, sv) };
        let c2 = if su != 0.0 { (su, 1.0) } else { (1.0, sv) };
        let uvs = [[0.0, 0.0], [l / 4.0, 0.0], [l / 4.0, 1.0], [0.0, 1.0]];
        quad_out(
            geo,
            p(a, c1.0, c1.1),
            p(b, c1.0, c1.1),
            p(b, c2.0, c2.1),
            p(a, c2.0, c2.1),
            out,
            color,
            Some(uvs),
        );
    }
    if caps {
        quad_out(
            geo,
            p(a, -1.0, -1.0),
            p(a, 1.0, -1.0),
            p(a, 1.0, 1.0),
            p(a, -1.0, 1.0),
            [-d.x, -d.y, -d.z],
            color,
            None,
        );
        quad_out(
            geo,
            p(b, -1.0, -1.0),
            p(b, 1.0, -1.0),
            p(b, 1.0, 1.0),
            p(b, -1.0, 1.0),
            [d.x, d.y, d.z],
            color,
            None,
        );
    }
}

/// Box centred on (cx, cz), bottom y0 to top y1, tapering from a0×w0 to
/// a1×w1 (a along the unit direction (ax, az), w across it):
/// `frustum(geo, cx, cz, y0, y1, ax, az, a0, w0, a1, w1, color = null,
/// tile = 8)`.
pub fn frustum(
    geo: &mut GeoBuilder,
    cx: f64,
    cz: f64,
    y0: f64,
    y1: f64,
    ax: f64,
    az: f64,
    a0: f64,
    w0: f64,
    a1: f64,
    w1: f64,
    color: Option<P3>,
    tile: f64,
) {
    let rx = -az;
    let rz = ax;
    let corner = |a: f64, w: f64, p: f64, q: f64| -> P2 {
        [
            cx + ax * p * a / 2.0 + rx * q * w / 2.0,
            cz + az * p * a / 2.0 + rz * q * w / 2.0,
        ]
    };
    const RING: [(f64, f64); 4] = [(1.0, 1.0), (1.0, -1.0), (-1.0, -1.0), (-1.0, 1.0)];
    for i in 0..4 {
        let (p0, q0) = RING[i];
        let (p1, q1) = RING[(i + 1) % 4];
        let b0 = corner(a0, w0, p0, q0);
        let b1 = corner(a0, w0, p1, q1);
        let t0 = corner(a1, w1, p0, q0);
        let t1 = corner(a1, w1, p1, q1);
        let mx = (p0 + p1) / 2.0;
        let mq = (q0 + q1) / 2.0;
        let out = [ax * mx + rx * mq, 0.0, az * mx + rz * mq];
        let len = kernel::hypot(b1[0] - b0[0], b1[1] - b0[1]);
        quad_out(
            geo,
            [b0[0], y0, b0[1]],
            [b1[0], y0, b1[1]],
            [t1[0], y1, t1[1]],
            [t0[0], y1, t0[1]],
            out,
            color,
            Some([
                [0.0, y0 / tile],
                [len / tile, y0 / tile],
                [len / tile, y1 / tile],
                [0.0, y1 / tile],
            ]),
        );
    }
    let t: Vec<P3> = RING
        .iter()
        .map(|&(p, q)| {
            let c = corner(a1, w1, p, q);
            [c[0], y1, c[1]]
        })
        .collect();
    quad_out(geo, t[0], t[1], t[2], t[3], [0.0, 1.0, 0.0], color, None);
}

/// A frame of a ribbon's path: `{ x, z, fx, fz, y }`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RFrame {
    pub x: f64,
    pub z: f64,
    pub fx: f64,
    pub fz: f64,
    pub y: f64,
}

/// A ribbon profile point's height: `y(fr, o)`.
pub type RHeight<'a> = Box<dyn Fn(&RFrame, f64) -> f64 + 'a>;

/// One point of a ribbon's cross-section: `{ o, y(fr, o) }`.
pub struct RProf<'a> {
    pub o: f64,
    pub y: RHeight<'a>,
}

pub fn rprof<'a>(o: f64, y: impl Fn(&RFrame, f64) -> f64 + 'a) -> RProf<'a> {
    RProf { o, y: Box::new(y) }
}

/// Swept cross-section along a list of frames. profile: [{o, y(fr, o)}] in
/// order of increasing offset (to the frame's right) for upward-facing
/// surfaces (`ribbon(frames, profile, { uS = 4, vS = 8 })`).
pub fn ribbon(frames: &[RFrame], profile: &[RProf], u_s: f64, v_s: f64) -> BufferGeometry {
    let mut pos: Vec<f64> = Vec::new();
    let mut uv: Vec<f64> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let pn = profile.len();
    let mut dist = 0.0;
    for (r, fr) in frames.iter().enumerate() {
        if r > 0 {
            dist += kernel::hypot(fr.x - frames[r - 1].x, fr.z - frames[r - 1].z);
        }
        let rx = -fr.fz;
        let rz = fr.fx;
        for pr in profile {
            let y = (pr.y)(fr, pr.o);
            pos.extend_from_slice(&[fr.x + rx * pr.o, y, fr.z + rz * pr.o]);
            uv.extend_from_slice(&[pr.o / u_s, dist / v_s]);
        }
    }
    for r in 0..frames.len().saturating_sub(1) {
        for p in 0..pn.saturating_sub(1) {
            let i0 = (r * pn + p) as u32;
            let i1 = i0 + 1;
            let i2 = i0 + pn as u32;
            let i3 = i2 + 1;
            idx.extend_from_slice(&[i0, i1, i2, i1, i3, i2]);
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&pos, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uv, 2));
    g.set_index(&idx);
    g.compute_vertex_normals();
    g
}

/// Matrix for a unit cylinder (height along +Y, centred) spanning A→B
/// (`spanMatrix(A, B, r = 1)`).
pub fn span_matrix(a: P3, b: P3, r: f64) -> Matrix4 {
    let d = Vector3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let l = d.length();
    let q = Quaternion::from_unit_vectors(Vector3::new(0.0, 1.0, 0.0), d.divide_scalar(l));
    Matrix4::compose(
        Vector3::new(
            (a[0] + b[0]) / 2.0,
            (a[1] + b[1]) / 2.0,
            (a[2] + b[2]) / 2.0,
        ),
        q,
        Vector3::new(r, l, r),
    )
}
