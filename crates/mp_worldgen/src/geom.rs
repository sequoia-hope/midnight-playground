//! Port of `src/world/city/geom.js` (roadmap WP 3.3): `GeoBuilder`,
//! `staticMesh`, `instanced`, `trs`, `yawOf`.
//!
//! Accumulates flat-shaded quads into one non-indexed BufferGeometry. The
//! city emits thousands of boxes, so everything of one material is gathered
//! here and handed to the GPU as a single mesh per spatial chunk.
//!
//! The JS accumulates plain arrays of doubles and stores them into
//! `Float32Array`s in `build`; so does the port.

// The shape methods keep the JS argument lists (DECISIONS D130, D190).
#![allow(clippy::too_many_arguments)]

use mp_math::kernel;

use crate::object::{GeoId, MaterialId, NodeId, SceneGraph};
use crate::three_geom::{
    BufferAttribute, BufferGeometry, Euler, EulerOrder, Matrix4, Quaternion, Vector3,
};

pub type P3 = [f64; 3];
pub type P2 = [f64; 2];

/// `prism`'s and `box`'s options object. `None` is a property the JS
/// object leaves out (its default applies).
#[derive(Clone, Debug, Default)]
pub struct PrismOpts {
    pub tile_w: Option<f64>,
    pub tile_h: Option<f64>,
    pub u_off: Option<f64>,
    pub v_off: Option<f64>,
    pub v_ref: Option<f64>,
    pub cell: Option<f64>,
    pub roof: Option<bool>,
    pub roof_cell: Option<f64>,
    pub roof_tile: Option<f64>,
    pub color: Option<P3>,
    pub roof_color: Option<P3>,
    /// `box` only: also a bottom face.
    pub bottom: bool,
}

/// Flat-shaded triangles with optional per-vertex colour and atlas cell.
#[derive(Clone, Debug, Default)]
pub struct GeoBuilder {
    pub pos: Vec<f64>,
    pub nor: Vec<f64>,
    pub uv: Vec<f64>,
    pub col: Option<Vec<f64>>,
    pub cell: Option<Vec<f64>>,
}

/// The face normal of (a, b, c) as `_n.crossVectors(_ab, _ac)`.
fn cross(a: &P3, b: &P3, c: &P3) -> Vector3 {
    let ab = Vector3::new(b[0] - a[0], b[1] - a[1], b[2] - a[2]);
    let ac = Vector3::new(c[0] - a[0], c[1] - a[1], c[2] - a[2]);
    Vector3::cross_vectors(ab, ac)
}

impl GeoBuilder {
    /// `new GeoBuilder({ color, cell })`.
    pub fn new(color: bool, cell: bool) -> GeoBuilder {
        GeoBuilder {
            pos: Vec::new(),
            nor: Vec::new(),
            uv: Vec::new(),
            col: color.then(Vec::new),
            cell: cell.then(Vec::new),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pos.is_empty()
    }

    fn vertex(&mut self, v: &P3, n: Vector3, uv: P2, c3: Option<P3>, cell: f64) {
        self.pos.extend_from_slice(v);
        self.nor.extend_from_slice(&[n.x, n.y, n.z]);
        self.uv.extend_from_slice(&uv);
        if let Some(col) = &mut self.col {
            let c = c3.unwrap_or([1.0, 1.0, 1.0]);
            col.extend_from_slice(&c);
        }
        if let Some(cells) = &mut self.cell {
            cells.push(cell);
        }
    }

    /// a,b,c,d: [x,y,z] counter-clockwise seen from the front face.
    /// uvs: [[u,v] x4]. c3: optional rgb for all four corners. cell: atlas cell.
    pub fn quad(
        &mut self,
        a: P3,
        b: P3,
        c: P3,
        d: P3,
        uvs: Option<[P2; 4]>,
        c3: Option<P3>,
        cell: f64,
    ) {
        let mut n = cross(&a, &b, &c);
        if n.length_sq() < 1e-12 {
            n = cross(&a, &b, &d);
        }
        if n.length_sq() < 1e-12 {
            // a and b coincide: take the normal from the other triangle (a, c, d).
            n = cross(&a, &c, &d);
        }
        // A zero normal lights to NaN, which bloom then smears across the screen.
        if n.length_sq() < 1e-12 {
            n = Vector3::new(0.0, 1.0, 0.0);
        }
        let n = n.normalize();
        let v = [a, b, c, a, c, d];
        let u = uvs.map(|uvs| [uvs[0], uvs[1], uvs[2], uvs[0], uvs[2], uvs[3]]);
        for k in 0..6 {
            let uv = u.map_or([0.0, 0.0], |u| u[k]);
            self.vertex(&v[k], n, uv, c3, cell);
        }
    }

    pub fn tri(&mut self, a: P3, b: P3, c: P3, c3: Option<P3>, cell: f64) {
        let mut n = cross(&a, &b, &c);
        if n.length_sq() < 1e-12 {
            n = Vector3::new(0.0, 1.0, 0.0);
        }
        let n = n.normalize();
        for v in [a, b, c] {
            self.vertex(&v, n, [0.0, 0.0], c3, cell);
        }
    }

    /// Vertical walls around a closed footprint (xz corners, any winding) plus
    /// an optional roof. Wall UVs are in texture tiles: u = metres / tileW
    /// along the wall, v = (y - vRef) / tileH.
    pub fn prism(&mut self, corners: &[P2], y0: f64, y1: f64, o: &PrismOpts) {
        let tile_w = o.tile_w.unwrap_or(20.0);
        let tile_h = o.tile_h.unwrap_or(40.0);
        let u_off = o.u_off.unwrap_or(0.0);
        let v_off = o.v_off.unwrap_or(0.0);
        let v_ref = o.v_ref.unwrap_or(y0);
        let cell = o.cell.unwrap_or(0.0);
        let roof = o.roof.unwrap_or(true);
        let roof_cell = o.roof_cell.unwrap_or(6.0);
        let roof_tile = o.roof_tile.unwrap_or(16.0);
        let n = corners.len();
        // Ensure clockwise-in-xz order (negative signed area) → outward normals.
        let mut area = 0.0;
        for i in 0..n {
            let (p, q) = (corners[i], corners[(i + 1) % n]);
            area += p[0] * q[1] - q[0] * p[1];
        }
        let c: Vec<P2> = if area > 0.0 {
            corners.iter().rev().copied().collect()
        } else {
            corners.to_vec()
        };
        let mut u = u_off;
        for i in 0..n {
            let (p, q) = (c[i], c[(i + 1) % n]);
            let len = kernel::hypot(q[0] - p[0], q[1] - p[1]);
            let u1 = u + len / tile_w;
            let v0 = (y0 - v_ref) / tile_h + v_off;
            let v1 = (y1 - v_ref) / tile_h + v_off;
            self.quad(
                [p[0], y0, p[1]],
                [q[0], y0, q[1]],
                [q[0], y1, q[1]],
                [p[0], y1, p[1]],
                Some([[u, v0], [u1, v0], [u1, v1], [u, v1]]),
                o.color,
                cell,
            );
            u = u1;
        }
        if roof {
            let uvr: Vec<P2> = c
                .iter()
                .map(|p| [p[0] / roof_tile, p[1] / roof_tile])
                .collect();
            let top: Vec<P3> = c.iter().map(|p| [p[0], y1, p[1]]).collect();
            for i in 1..n.saturating_sub(1) {
                // Roof: fan, wound so the normal points up.
                self.tri_uv(
                    top[0],
                    top[i + 1],
                    top[i],
                    uvr[0],
                    uvr[i + 1],
                    uvr[i],
                    o.roof_color.or(o.color),
                    roof_cell,
                );
            }
        }
    }

    pub fn tri_uv(
        &mut self,
        a: P3,
        mut b: P3,
        mut c: P3,
        ua: P2,
        mut ub: P2,
        mut uc: P2,
        c3: Option<P3>,
        cell: f64,
    ) {
        let mut n = cross(&a, &b, &c);
        if n.length_sq() < 1e-12 {
            n = Vector3::new(0.0, 1.0, 0.0);
        }
        n = n.normalize();
        if n.y < 0.0 && n.y.abs() > 0.9 {
            // Guarantee roofs face up regardless of footprint winding.
            std::mem::swap(&mut b, &mut c);
            std::mem::swap(&mut ub, &mut uc);
            n = -n;
        }
        let v = [a, b, c];
        let u = [ua, ub, uc];
        for k in 0..3 {
            self.vertex(&v[k], n, u[k], c3, cell);
        }
    }

    /// Axis-free box: centre (x,y,z) bottom-centred at y, size (sx, sy, sz)
    /// with sx along direction yaw (radians, measured like atan2(dz, dx)).
    pub fn box_(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        sx: f64,
        sy: f64,
        sz: f64,
        yaw: f64,
        o: &PrismOpts,
    ) {
        let c = kernel::cos(yaw);
        let s = kernel::sin(yaw);
        let hx = sx / 2.0;
        let hz = sz / 2.0;
        let pts: Vec<P2> = [[-hx, -hz], [hx, -hz], [hx, hz], [-hx, hz]]
            .iter()
            .map(|&[a, b]| [x + a * c - b * s, z + a * s + b * c])
            .collect();
        // `{ tileW: o.tileW ?? 4, tileH: o.tileH ?? 4, ...o }`
        let po = PrismOpts {
            tile_w: Some(o.tile_w.unwrap_or(4.0)),
            tile_h: Some(o.tile_h.unwrap_or(4.0)),
            ..o.clone()
        };
        self.prism(&pts, y, y + sy, &po);
        if o.bottom {
            let bot: Vec<P3> = pts.iter().map(|p| [p[0], y, p[1]]).collect();
            self.quad(
                bot[0],
                bot[1],
                bot[2],
                bot[3],
                Some([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]),
                o.color,
                o.cell.unwrap_or(0.0),
            );
        }
    }

    /// The geometry: `Float32` position, normal, uv, then colour and cell
    /// if kept; bounding sphere computed.
    pub fn build(&self) -> BufferGeometry {
        let mut g = BufferGeometry::new();
        g.set_attribute("position", BufferAttribute::from_f64(&self.pos, 3));
        g.set_attribute("normal", BufferAttribute::from_f64(&self.nor, 3));
        g.set_attribute("uv", BufferAttribute::from_f64(&self.uv, 2));
        if let Some(col) = &self.col {
            g.set_attribute("color", BufferAttribute::from_f64(col, 3));
        }
        if let Some(cell) = &self.cell {
            g.set_attribute("cell", BufferAttribute::from_f64(cell, 1));
        }
        g.compute_bounding_sphere();
        g
    }
}

/// `staticMesh`'s options: `{ cast = false, receive = true, name = '' }`.
#[derive(Clone, Debug)]
pub struct StaticOpts {
    pub cast: bool,
    pub receive: bool,
    pub name: String,
}

impl Default for StaticOpts {
    fn default() -> Self {
        StaticOpts {
            cast: false,
            receive: true,
            name: String::new(),
        }
    }
}

/// A static mesh that never moves.
pub fn static_mesh(graph: &mut SceneGraph, geo: GeoId, mat: MaterialId, o: &StaticOpts) -> NodeId {
    let m = graph.mesh(geo, mat);
    let obj = graph.get_mut(m);
    obj.cast_shadow = o.cast;
    obj.receive_shadow = o.receive;
    obj.matrix_auto_update = false;
    obj.update_matrix();
    if !o.name.is_empty() {
        obj.name = o.name.clone();
    }
    m
}

/// Instanced mesh from a list of matrices (`{ cast = false, receive = false }`).
pub fn instanced(
    graph: &mut SceneGraph,
    geo: GeoId,
    mat: MaterialId,
    matrices: &[Matrix4],
    cast: bool,
    receive: bool,
) -> NodeId {
    let im = graph.instanced_mesh(geo, mat, matrices.len().max(1) as u32);
    {
        let o = graph.get_mut(im);
        let inst = o.instances.as_mut().expect("instanced");
        for (i, m) in matrices.iter().enumerate() {
            inst.set_matrix_at(i, m);
        }
        inst.count = matrices.len() as u32;
        o.cast_shadow = cast;
        o.receive_shadow = receive;
        o.matrix_auto_update = false;
        o.update_matrix();
    }
    graph.compute_instance_bounding_sphere(im);
    graph.compute_instance_bounding_box(im);
    im
}

/// `trs(x, y, z, yaw = 0, sx = 1, sy = 1, sz = 1, pitch = 0, roll = 0)`:
/// Euler (pitch, yaw, roll) in YXZ order.
pub fn trs(
    x: f64,
    y: f64,
    z: f64,
    yaw: f64,
    sx: f64,
    sy: f64,
    sz: f64,
    pitch: f64,
    roll: f64,
) -> Matrix4 {
    let q = Quaternion::from_euler(&Euler::with_order(pitch, yaw, roll, EulerOrder::YXZ));
    Matrix4::compose(Vector3::new(x, y, z), q, Vector3::new(sx, sy, sz))
}

/// three.js yaw (rotation about +Y) that turns local +X onto direction (dx, dz).
pub fn yaw_of(dx: f64, dz: f64) -> f64 {
    -kernel::atan2(dz, dx)
}
