//! `PolyhedronGeometry` and its subclasses Tetrahedron, Octahedron,
//! Icosahedron and Dodecahedron (three.js r180).
//!
//! Non-indexed: each face of the base solid is subdivided `detail` times,
//! projected onto the sphere of `radius`, and given spherical UVs. At
//! detail 0 the normals are flat (`computeVertexNormals`), otherwise they
//! are the normalised positions.

use core::f64::consts::PI;

use mp_math::{js, kernel};

use super::attribute::BufferAttribute;
use super::geometry::BufferGeometry;
use super::math::{Vector2, Vector3};
use super::primitives::whole;

/// `new PolyhedronGeometry(vertices, indices, radius, detail)`.
pub fn polyhedron_geometry(
    vertices: &[f64],
    indices: &[usize],
    radius: f64,
    detail: f64,
) -> BufferGeometry {
    let detail = whole(detail, "PolyhedronGeometry detail");
    let mut p = Poly {
        vertices,
        vertex_buffer: Vec::new(),
        uv_buffer: Vec::new(),
    };

    // the subdivision creates the vertex buffer data
    p.subdivide(indices, detail as usize);
    // all vertices should lie on a conceptual sphere with a given radius
    p.apply_radius(radius);
    // finally, create the uv data
    p.generate_uvs();

    // build non-indexed geometry
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f64(&p.vertex_buffer, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(&p.vertex_buffer, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&p.uv_buffer, 2));
    if detail == 0.0 {
        g.compute_vertex_normals(); // flat normals
    } else {
        g.normalize_normals(); // smooth normals
    }
    g
}

struct Poly<'a> {
    vertices: &'a [f64],
    vertex_buffer: Vec<f64>,
    uv_buffer: Vec<f64>,
}

impl Poly<'_> {
    fn subdivide(&mut self, indices: &[usize], detail: usize) {
        // iterate over all faces and apply a subdivision with the given detail value
        let mut i = 0;
        while i < indices.len() {
            // get the vertices of the face
            let a = self.get_vertex_by_index(indices[i]);
            let b = self.get_vertex_by_index(indices[i + 1]);
            let c = self.get_vertex_by_index(indices[i + 2]);
            // perform subdivision
            self.subdivide_face(a, b, c, detail);
            i += 3;
        }
    }

    fn subdivide_face(&mut self, a: Vector3, b: Vector3, c: Vector3, detail: usize) {
        let cols = detail + 1;
        // we use this multidimensional array as a data structure for creating the subdivision
        let mut v: Vec<Vec<Vector3>> = Vec::with_capacity(cols + 1);
        // construct all of the vertices for this subdivision
        for i in 0..=cols {
            let mut row = Vec::new();
            let aj = a.lerp(c, i as f64 / cols as f64);
            let bj = b.lerp(c, i as f64 / cols as f64);
            let rows = cols - i;
            for j in 0..=rows {
                if j == 0 && i == cols {
                    row.push(aj);
                } else {
                    row.push(aj.lerp(bj, j as f64 / rows as f64));
                }
            }
            v.push(row);
        }
        // construct all of the faces
        for i in 0..cols {
            for j in 0..2 * (cols - i) - 1 {
                let k = j / 2;
                if j % 2 == 0 {
                    self.push_vertex(v[i][k + 1]);
                    self.push_vertex(v[i + 1][k]);
                    self.push_vertex(v[i][k]);
                } else {
                    self.push_vertex(v[i][k + 1]);
                    self.push_vertex(v[i + 1][k + 1]);
                    self.push_vertex(v[i + 1][k]);
                }
            }
        }
    }

    fn apply_radius(&mut self, radius: f64) {
        // iterate over the entire buffer and apply the radius to each vertex
        let mut i = 0;
        while i < self.vertex_buffer.len() {
            let vb = &mut self.vertex_buffer;
            let vertex = Vector3::new(vb[i], vb[i + 1], vb[i + 2])
                .normalize()
                .multiply_scalar(radius);
            vb[i] = vertex.x;
            vb[i + 1] = vertex.y;
            vb[i + 2] = vertex.z;
            i += 3;
        }
    }

    fn generate_uvs(&mut self) {
        let mut i = 0;
        while i < self.vertex_buffer.len() {
            let vb = &self.vertex_buffer;
            let vertex = Vector3::new(vb[i], vb[i + 1], vb[i + 2]);
            let u = azimuth(vertex) / 2.0 / PI + 0.5;
            let v = inclination(vertex) / PI + 0.5;
            self.uv_buffer.extend([u, 1.0 - v]);
            i += 3;
        }
        self.correct_uvs();
        self.correct_seam();
    }

    fn correct_seam(&mut self) {
        // handle case when face straddles the seam, see #3269
        let uv = &mut self.uv_buffer;
        let mut i = 0;
        while i < uv.len() {
            // uv data of a single face
            let x0 = uv[i];
            let x1 = uv[i + 2];
            let x2 = uv[i + 4];
            let max = js::max_n(&[x0, x1, x2]);
            let min = js::min_n(&[x0, x1, x2]);
            // 0.9 is somewhat arbitrary
            if max > 0.9 && min < 0.1 {
                if x0 < 0.2 {
                    uv[i] += 1.0;
                }
                if x1 < 0.2 {
                    uv[i + 2] += 1.0;
                }
                if x2 < 0.2 {
                    uv[i + 4] += 1.0;
                }
            }
            i += 6;
        }
    }

    fn push_vertex(&mut self, vertex: Vector3) {
        self.vertex_buffer.extend([vertex.x, vertex.y, vertex.z]);
    }

    fn get_vertex_by_index(&self, index: usize) -> Vector3 {
        let stride = index * 3;
        Vector3::new(
            self.vertices[stride],
            self.vertices[stride + 1],
            self.vertices[stride + 2],
        )
    }

    fn correct_uvs(&mut self) {
        let mut i = 0;
        let mut j = 0;
        while i < self.vertex_buffer.len() {
            let vb = &self.vertex_buffer;
            let a = Vector3::new(vb[i], vb[i + 1], vb[i + 2]);
            let b = Vector3::new(vb[i + 3], vb[i + 4], vb[i + 5]);
            let c = Vector3::new(vb[i + 6], vb[i + 7], vb[i + 8]);
            let ub = &self.uv_buffer;
            let uv_a = Vector2::new(ub[j], ub[j + 1]);
            let uv_b = Vector2::new(ub[j + 2], ub[j + 3]);
            let uv_c = Vector2::new(ub[j + 4], ub[j + 5]);
            let centroid = (a + b + c).divide_scalar(3.0);
            let azi = azimuth(centroid);
            self.correct_uv(uv_a, j, a, azi);
            self.correct_uv(uv_b, j + 2, b, azi);
            self.correct_uv(uv_c, j + 4, c, azi);
            i += 9;
            j += 6;
        }
    }

    fn correct_uv(&mut self, uv: Vector2, stride: usize, vector: Vector3, azimuth: f64) {
        if azimuth < 0.0 && uv.x == 1.0 {
            self.uv_buffer[stride] = uv.x - 1.0;
        }
        if vector.x == 0.0 && vector.z == 0.0 {
            self.uv_buffer[stride] = azimuth / 2.0 / PI + 0.5;
        }
    }
}

/// Angle around the y axis, counter-clockwise when looking from above.
fn azimuth(vector: Vector3) -> f64 {
    kernel::atan2(vector.z, -vector.x)
}

/// Angle above the XZ plane.
fn inclination(vector: Vector3) -> f64 {
    kernel::atan2(
        -vector.y,
        ((vector.x * vector.x) + (vector.z * vector.z)).sqrt(),
    )
}

/// `new TetrahedronGeometry(radius, detail)`; defaults `1, 0`.
pub fn tetrahedron_geometry(radius: f64, detail: f64) -> BufferGeometry {
    let vertices = [
        1.0, 1.0, 1.0, -1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, -1.0,
    ];
    let indices = [2, 1, 0, 0, 3, 2, 1, 3, 0, 2, 3, 1];
    polyhedron_geometry(&vertices, &indices, radius, detail)
}

/// `new OctahedronGeometry(radius, detail)`; defaults `1, 0`.
pub fn octahedron_geometry(radius: f64, detail: f64) -> BufferGeometry {
    let vertices = [
        1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0,
    ];
    let indices = [
        0, 2, 4, 0, 4, 3, 0, 3, 5, 0, 5, 2, 1, 2, 5, 1, 5, 3, 1, 3, 4, 1, 4, 2,
    ];
    polyhedron_geometry(&vertices, &indices, radius, detail)
}

/// `new IcosahedronGeometry(radius, detail)`; defaults `1, 0`.
pub fn icosahedron_geometry(radius: f64, detail: f64) -> BufferGeometry {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    #[rustfmt::skip]
    let vertices = [
        -1.0, t, 0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t, 0.0,
        0.0, -1.0, t, 0.0, 1.0, t, 0.0, -1.0, -t, 0.0, 1.0, -t,
        t, 0.0, -1.0, t, 0.0, 1.0, -t, 0.0, -1.0, -t, 0.0, 1.0,
    ];
    #[rustfmt::skip]
    let indices = [
        0, 11, 5, 0, 5, 1, 0, 1, 7, 0, 7, 10, 0, 10, 11,
        1, 5, 9, 5, 11, 4, 11, 10, 2, 10, 7, 6, 7, 1, 8,
        3, 9, 4, 3, 4, 2, 3, 2, 6, 3, 6, 8, 3, 8, 9,
        4, 9, 5, 2, 4, 11, 6, 2, 10, 8, 6, 7, 9, 8, 1,
    ];
    polyhedron_geometry(&vertices, &indices, radius, detail)
}

/// `new DodecahedronGeometry(radius, detail)`; defaults `1, 0`.
pub fn dodecahedron_geometry(radius: f64, detail: f64) -> BufferGeometry {
    let t = (1.0 + 5f64.sqrt()) / 2.0;
    let r = 1.0 / t;
    #[rustfmt::skip]
    let vertices = [
        // (±1, ±1, ±1)
        -1.0, -1.0, -1.0, -1.0, -1.0, 1.0,
        -1.0, 1.0, -1.0, -1.0, 1.0, 1.0,
        1.0, -1.0, -1.0, 1.0, -1.0, 1.0,
        1.0, 1.0, -1.0, 1.0, 1.0, 1.0,
        // (0, ±1/φ, ±φ)
        0.0, -r, -t, 0.0, -r, t,
        0.0, r, -t, 0.0, r, t,
        // (±1/φ, ±φ, 0)
        -r, -t, 0.0, -r, t, 0.0,
        r, -t, 0.0, r, t, 0.0,
        // (±φ, 0, ±1/φ)
        -t, 0.0, -r, t, 0.0, -r,
        -t, 0.0, r, t, 0.0, r,
    ];
    #[rustfmt::skip]
    let indices = [
        3, 11, 7, 3, 7, 15, 3, 15, 13,
        7, 19, 17, 7, 17, 6, 7, 6, 15,
        17, 4, 8, 17, 8, 10, 17, 10, 6,
        8, 0, 16, 8, 16, 2, 8, 2, 10,
        0, 12, 1, 0, 1, 18, 0, 18, 16,
        6, 10, 2, 6, 2, 13, 6, 13, 15,
        2, 16, 18, 2, 18, 3, 2, 3, 13,
        18, 1, 9, 18, 9, 11, 18, 11, 3,
        4, 14, 12, 4, 12, 0, 4, 0, 8,
        11, 9, 5, 11, 5, 19, 11, 19, 7,
        19, 5, 14, 19, 14, 4, 19, 4, 17,
        1, 12, 14, 1, 14, 5, 1, 5, 9,
    ];
    polyhedron_geometry(&vertices, &indices, radius, detail)
}
