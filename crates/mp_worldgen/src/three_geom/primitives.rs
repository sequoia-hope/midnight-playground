//! The parametric generators of three.js r180 (`src/geometries/`): Box,
//! Plane, Circle, Cylinder, Cone, Sphere, Torus, Capsule and Lathe.
//!
//! Each function takes the constructor's arguments in its order, with every
//! default written out by the caller (DECISIONS D130). Counts are `f64`, as
//! in the JS: where the JS floors a count (`Math.floor(widthSegments)`), so
//! does the port; where it uses the count as given, the port requires a
//! whole number (see [`whole`]).

use core::f64::consts::PI;

use mp_math::{js, kernel};

use super::attribute::BufferAttribute;
use super::geometry::BufferGeometry;
use super::math::{Vector2, Vector3, clamp};

/// A count the JS uses without flooring. A fractional count would make the
/// JS loops and index formulas disagree in ways the port does not follow,
/// and the world code never passes one; so it is refused.
pub(crate) fn whole(n: f64, what: &str) -> f64 {
    assert!(
        n.fract() == 0.0,
        "three_geom: {what} must be a whole number, got {n} (DECISIONS D130)"
    );
    n
}

/// The usual end of a generator: index, then `position`, `normal`, `uv` as
/// `Float32BufferAttribute`s.
fn finish(indices: &[u32], vertices: &[f64], normals: &[f64], uvs: &[f64]) -> BufferGeometry {
    let mut g = BufferGeometry::new();
    g.set_index(indices);
    g.set_attribute("position", BufferAttribute::from_f64(vertices, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(normals, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(uvs, 2));
    g
}

/// Writes component `axis` (0 = x, 1 = y, 2 = z), as `vector[u] = ...`.
fn set_axis(v: &mut Vector3, axis: usize, value: f64) {
    match axis {
        0 => v.x = value,
        1 => v.y = value,
        _ => v.z = value,
    }
}

// ── BoxGeometry ─────────────────────────────────────────────────────────

/// `new BoxGeometry(width, height, depth, widthSegments, heightSegments,
/// depthSegments)`; three's defaults are `1, 1, 1, 1, 1, 1`.
///
/// Six faces in the order +x, -x, +y, -y, +z, -z, each its own group
/// (material index 0 to 5); builders rely on this order for UV regions.
pub fn box_geometry(
    width: f64,
    height: f64,
    depth: f64,
    width_segments: f64,
    height_segments: f64,
    depth_segments: f64,
) -> BufferGeometry {
    // segments
    let width_segments = width_segments.floor();
    let height_segments = height_segments.floor();
    let depth_segments = depth_segments.floor();

    let mut st = BoxState::default();

    // build each side of the box geometry
    st.build_plane(
        2,
        1,
        0,
        -1.0,
        -1.0,
        depth,
        height,
        width,
        depth_segments,
        height_segments,
        0,
    ); // px
    st.build_plane(
        2,
        1,
        0,
        1.0,
        -1.0,
        depth,
        height,
        -width,
        depth_segments,
        height_segments,
        1,
    ); // nx
    st.build_plane(
        0,
        2,
        1,
        1.0,
        1.0,
        width,
        depth,
        height,
        width_segments,
        depth_segments,
        2,
    ); // py
    st.build_plane(
        0,
        2,
        1,
        1.0,
        -1.0,
        width,
        depth,
        -height,
        width_segments,
        depth_segments,
        3,
    ); // ny
    st.build_plane(
        0,
        1,
        2,
        1.0,
        -1.0,
        width,
        height,
        depth,
        width_segments,
        height_segments,
        4,
    ); // pz
    st.build_plane(
        0,
        1,
        2,
        -1.0,
        -1.0,
        width,
        height,
        -depth,
        width_segments,
        height_segments,
        5,
    ); // nz

    // build geometry
    let mut g = finish(&st.indices, &st.vertices, &st.normals, &st.uvs);
    g.groups = st.groups;
    g
}

#[derive(Default)]
struct BoxState {
    indices: Vec<u32>,
    vertices: Vec<f64>,
    normals: Vec<f64>,
    uvs: Vec<f64>,
    groups: Vec<super::geometry::Group>,
    number_of_vertices: u32,
    group_start: usize,
}

impl BoxState {
    #[allow(clippy::too_many_arguments)]
    fn build_plane(
        &mut self,
        u: usize,
        v: usize,
        w: usize,
        udir: f64,
        vdir: f64,
        width: f64,
        height: f64,
        depth: f64,
        grid_x: f64,
        grid_y: f64,
        material_index: usize,
    ) {
        let segment_width = width / grid_x;
        let segment_height = height / grid_y;
        let width_half = width / 2.0;
        let height_half = height / 2.0;
        let depth_half = depth / 2.0;
        let grid_x1 = grid_x + 1.0;
        let grid_y1 = grid_y + 1.0;
        let mut vertex_counter = 0;
        let mut group_count = 0;
        let mut vector = Vector3::default();

        // generate vertices, normals and uvs
        let mut iy = 0.0;
        while iy < grid_y1 {
            let y = iy * segment_height - height_half;
            let mut ix = 0.0;
            while ix < grid_x1 {
                let x = ix * segment_width - width_half;
                // set values to correct vector component
                set_axis(&mut vector, u, x * udir);
                set_axis(&mut vector, v, y * vdir);
                set_axis(&mut vector, w, depth_half);
                // now apply vector to vertex buffer
                self.vertices.extend([vector.x, vector.y, vector.z]);
                // set values to correct vector component
                set_axis(&mut vector, u, 0.0);
                set_axis(&mut vector, v, 0.0);
                set_axis(&mut vector, w, if depth > 0.0 { 1.0 } else { -1.0 });
                // now apply vector to normal buffer
                self.normals.extend([vector.x, vector.y, vector.z]);
                // uvs
                self.uvs.push(ix / grid_x);
                self.uvs.push(1.0 - (iy / grid_y));
                // counters
                vertex_counter += 1;
                ix += 1.0;
            }
            iy += 1.0;
        }

        // indices
        // 1. you need three indices to draw a single face
        // 2. a single segment consists of two faces
        // 3. so we need to generate six (2*3) indices per segment
        let gx1 = grid_x1 as u32;
        let mut iy = 0;
        while (iy as f64) < grid_y {
            let mut ix = 0;
            while (ix as f64) < grid_x {
                let n = self.number_of_vertices;
                let a = n + ix + gx1 * iy;
                let b = n + ix + gx1 * (iy + 1);
                let c = n + (ix + 1) + gx1 * (iy + 1);
                let d = n + (ix + 1) + gx1 * iy;
                // faces
                self.indices.extend([a, b, d]);
                self.indices.extend([b, c, d]);
                // increase counter
                group_count += 6;
                ix += 1;
            }
            iy += 1;
        }

        // add a group to the geometry. this will ensure multi material support
        self.groups.push(super::geometry::Group {
            start: self.group_start,
            count: group_count,
            material_index,
        });
        // calculate new start value for groups
        self.group_start += group_count;
        // update total number of vertices
        self.number_of_vertices += vertex_counter;
    }
}

// ── PlaneGeometry ───────────────────────────────────────────────────────

/// `new PlaneGeometry(width, height, widthSegments, heightSegments)`;
/// defaults `1, 1, 1, 1`. In the XY plane, facing +z.
pub fn plane_geometry(
    width: f64,
    height: f64,
    width_segments: f64,
    height_segments: f64,
) -> BufferGeometry {
    let width_half = width / 2.0;
    let height_half = height / 2.0;
    let grid_x = width_segments.floor();
    let grid_y = height_segments.floor();
    let grid_x1 = grid_x + 1.0;
    let grid_y1 = grid_y + 1.0;
    let segment_width = width / grid_x;
    let segment_height = height / grid_y;

    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    let mut iy = 0.0;
    while iy < grid_y1 {
        let y = iy * segment_height - height_half;
        let mut ix = 0.0;
        while ix < grid_x1 {
            let x = ix * segment_width - width_half;
            vertices.extend([x, -y, 0.0]);
            normals.extend([0.0, 0.0, 1.0]);
            uvs.push(ix / grid_x);
            uvs.push(1.0 - (iy / grid_y));
            ix += 1.0;
        }
        iy += 1.0;
    }

    let gx1 = grid_x1 as u32;
    let mut iy = 0u32;
    while (iy as f64) < grid_y {
        let mut ix = 0u32;
        while (ix as f64) < grid_x {
            let a = ix + gx1 * iy;
            let b = ix + gx1 * (iy + 1);
            let c = (ix + 1) + gx1 * (iy + 1);
            let d = (ix + 1) + gx1 * iy;
            indices.extend([a, b, d]);
            indices.extend([b, c, d]);
            ix += 1;
        }
        iy += 1;
    }

    finish(&indices, &vertices, &normals, &uvs)
}

// ── CircleGeometry ──────────────────────────────────────────────────────

/// `new CircleGeometry(radius, segments, thetaStart, thetaLength)`;
/// defaults `1, 32, 0, 2π`. A fan in the XY plane facing +z; at least 3
/// segments.
pub fn circle_geometry(
    radius: f64,
    segments: f64,
    theta_start: f64,
    theta_length: f64,
) -> BufferGeometry {
    let segments = whole(js::max(3.0, segments), "CircleGeometry segments");

    let mut indices = Vec::new();
    let mut vertices = vec![0.0, 0.0, 0.0];
    let mut normals = vec![0.0, 0.0, 1.0];
    let mut uvs = vec![0.5, 0.5];
    let mut vertex = Vector3::default();

    let n = segments as u32;
    let mut i = 3;
    for s in 0..=n {
        let segment = theta_start + s as f64 / segments * theta_length;
        vertex.x = radius * kernel::cos(segment);
        vertex.y = radius * kernel::sin(segment);
        vertices.extend([vertex.x, vertex.y, vertex.z]);
        normals.extend([0.0, 0.0, 1.0]);
        let uv = Vector2::new(
            (vertices[i] / radius + 1.0) / 2.0,
            (vertices[i + 1] / radius + 1.0) / 2.0,
        );
        uvs.extend([uv.x, uv.y]);
        i += 3;
    }

    for i in 1..=n {
        indices.extend([i, i + 1, 0]);
    }

    finish(&indices, &vertices, &normals, &uvs)
}

// ── CylinderGeometry and ConeGeometry ───────────────────────────────────

/// `new CylinderGeometry(radiusTop, radiusBottom, height, radialSegments,
/// heightSegments, openEnded, thetaStart, thetaLength)`; defaults `1, 1, 1,
/// 32, 1, false, 0, 2π`. Along y; groups: torso 0, top cap 1, bottom cap 2.
#[allow(clippy::too_many_arguments)]
pub fn cylinder_geometry(
    radius_top: f64,
    radius_bottom: f64,
    height: f64,
    radial_segments: f64,
    height_segments: f64,
    open_ended: bool,
    theta_start: f64,
    theta_length: f64,
) -> BufferGeometry {
    let radial_segments = radial_segments.floor();
    let height_segments = height_segments.floor();

    let mut c = Cyl {
        radius_top,
        radius_bottom,
        height,
        radial_segments,
        height_segments,
        theta_start,
        theta_length,
        half_height: height / 2.0,
        indices: Vec::new(),
        vertices: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        index: 0,
        index_array: Vec::new(),
        group_start: 0,
        groups: Vec::new(),
    };

    c.generate_torso();
    if !open_ended {
        if radius_top > 0.0 {
            c.generate_cap(true);
        }
        if radius_bottom > 0.0 {
            c.generate_cap(false);
        }
    }

    let mut g = finish(&c.indices, &c.vertices, &c.normals, &c.uvs);
    g.groups = c.groups;
    g
}

/// `new ConeGeometry(radius, height, radialSegments, heightSegments,
/// openEnded, thetaStart, thetaLength)`: a cylinder with top radius 0.
/// Defaults `1, 1, 32, 1, false, 0, 2π`.
pub fn cone_geometry(
    radius: f64,
    height: f64,
    radial_segments: f64,
    height_segments: f64,
    open_ended: bool,
    theta_start: f64,
    theta_length: f64,
) -> BufferGeometry {
    cylinder_geometry(
        0.0,
        radius,
        height,
        radial_segments,
        height_segments,
        open_ended,
        theta_start,
        theta_length,
    )
}

struct Cyl {
    radius_top: f64,
    radius_bottom: f64,
    height: f64,
    radial_segments: f64,
    height_segments: f64,
    theta_start: f64,
    theta_length: f64,
    half_height: f64,
    indices: Vec<u32>,
    vertices: Vec<f64>,
    normals: Vec<f64>,
    uvs: Vec<f64>,
    index: u32,
    index_array: Vec<Vec<u32>>,
    group_start: usize,
    groups: Vec<super::geometry::Group>,
}

impl Cyl {
    fn generate_torso(&mut self) {
        let mut group_count = 0;
        // this will be used to calculate the normal
        let slope = (self.radius_bottom - self.radius_top) / self.height;
        let rs = self.radial_segments as i64;
        let hs = self.height_segments as i64;

        // generate vertices, normals and uvs
        for y in 0..=hs {
            let mut index_row = Vec::new();
            let v = y as f64 / self.height_segments;
            // calculate the radius of the current row
            let radius = v * (self.radius_bottom - self.radius_top) + self.radius_top;
            for x in 0..=rs {
                let u = x as f64 / self.radial_segments;
                let theta = u * self.theta_length + self.theta_start;
                let sin_theta = kernel::sin(theta);
                let cos_theta = kernel::cos(theta);
                // vertex
                let vertex = Vector3::new(
                    radius * sin_theta,
                    -v * self.height + self.half_height,
                    radius * cos_theta,
                );
                self.vertices.extend([vertex.x, vertex.y, vertex.z]);
                // normal
                let normal = Vector3::new(sin_theta, slope, cos_theta).normalize();
                self.normals.extend([normal.x, normal.y, normal.z]);
                // uv
                self.uvs.extend([u, 1.0 - v]);
                // save index of vertex in respective row
                index_row.push(self.index);
                self.index += 1;
            }
            // now save vertices of the row in our index array
            self.index_array.push(index_row);
        }

        // generate indices
        for x in 0..rs.max(0) as usize {
            for y in 0..hs.max(0) as usize {
                // we use the index array to access the correct indices
                let a = self.index_array[y][x];
                let b = self.index_array[y + 1][x];
                let c = self.index_array[y + 1][x + 1];
                let d = self.index_array[y][x + 1];
                // faces
                if self.radius_top > 0.0 || y != 0 {
                    self.indices.extend([a, b, d]);
                    group_count += 3;
                }
                if self.radius_bottom > 0.0 || y as i64 != hs - 1 {
                    self.indices.extend([b, c, d]);
                    group_count += 3;
                }
            }
        }

        // add a group to the geometry. this will ensure multi material support
        self.groups.push(super::geometry::Group {
            start: self.group_start,
            count: group_count,
            material_index: 0,
        });
        // calculate new start value for groups
        self.group_start += group_count;
    }

    fn generate_cap(&mut self, top: bool) {
        // save the index of the first center vertex
        let center_index_start = self.index;
        let mut group_count = 0;
        let radius = if top {
            self.radius_top
        } else {
            self.radius_bottom
        };
        let sign = if top { 1.0 } else { -1.0 };
        let rs = self.radial_segments as i64;

        // first we generate the center vertex data of the cap.
        // because the geometry needs one set of uvs per face,
        // we must generate a center vertex per face/segment
        for _x in 1..=rs {
            self.vertices.extend([0.0, self.half_height * sign, 0.0]);
            self.normals.extend([0.0, sign, 0.0]);
            self.uvs.extend([0.5, 0.5]);
            self.index += 1;
        }

        // save the index of the last center vertex
        let center_index_end = self.index;

        // now we generate the surrounding vertices, normals and uvs
        for x in 0..=rs {
            let u = x as f64 / self.radial_segments;
            let theta = u * self.theta_length + self.theta_start;
            let cos_theta = kernel::cos(theta);
            let sin_theta = kernel::sin(theta);
            let vertex = Vector3::new(
                radius * sin_theta,
                self.half_height * sign,
                radius * cos_theta,
            );
            self.vertices.extend([vertex.x, vertex.y, vertex.z]);
            self.normals.extend([0.0, sign, 0.0]);
            let uv = Vector2::new((cos_theta * 0.5) + 0.5, (sin_theta * 0.5 * sign) + 0.5);
            self.uvs.extend([uv.x, uv.y]);
            self.index += 1;
        }

        // generate indices
        for x in 0..rs.max(0) as u32 {
            let c = center_index_start + x;
            let i = center_index_end + x;
            if top {
                // face top
                self.indices.extend([i, i + 1, c]);
            } else {
                // face bottom
                self.indices.extend([i + 1, i, c]);
            }
            group_count += 3;
        }

        self.groups.push(super::geometry::Group {
            start: self.group_start,
            count: group_count,
            material_index: if top { 1 } else { 2 },
        });
        self.group_start += group_count;
    }
}

// ── SphereGeometry ──────────────────────────────────────────────────────

/// `new SphereGeometry(radius, widthSegments, heightSegments, phiStart,
/// phiLength, thetaStart, thetaLength)`; defaults `1, 32, 16, 0, 2π, 0, π`.
pub fn sphere_geometry(
    radius: f64,
    width_segments: f64,
    height_segments: f64,
    phi_start: f64,
    phi_length: f64,
    theta_start: f64,
    theta_length: f64,
) -> BufferGeometry {
    let width_segments = js::max(3.0, width_segments.floor());
    let height_segments = js::max(2.0, height_segments.floor());
    let theta_end = js::min(theta_start + theta_length, PI);

    let mut index = 0u32;
    let mut grid: Vec<Vec<u32>> = Vec::new();
    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let ws = width_segments as usize;
    let hs = height_segments as usize;

    // generate vertices, normals and uvs
    for iy in 0..=hs {
        let mut vertices_row = Vec::new();
        let v = iy as f64 / height_segments;
        // special case for the poles
        let mut u_offset = 0.0;
        if iy == 0 && theta_start == 0.0 {
            u_offset = 0.5 / width_segments;
        } else if iy == hs && theta_end == PI {
            u_offset = -0.5 / width_segments;
        }
        for ix in 0..=ws {
            let u = ix as f64 / width_segments;
            // vertex
            let vertex = Vector3::new(
                -radius
                    * kernel::cos(phi_start + u * phi_length)
                    * kernel::sin(theta_start + v * theta_length),
                radius * kernel::cos(theta_start + v * theta_length),
                radius
                    * kernel::sin(phi_start + u * phi_length)
                    * kernel::sin(theta_start + v * theta_length),
            );
            vertices.extend([vertex.x, vertex.y, vertex.z]);
            // normal
            let normal = vertex.normalize();
            normals.extend([normal.x, normal.y, normal.z]);
            // uv
            uvs.extend([u + u_offset, 1.0 - v]);
            vertices_row.push(index);
            index += 1;
        }
        grid.push(vertices_row);
    }

    // indices
    for iy in 0..hs {
        for ix in 0..ws {
            let a = grid[iy][ix + 1];
            let b = grid[iy][ix];
            let c = grid[iy + 1][ix];
            let d = grid[iy + 1][ix + 1];
            if iy != 0 || theta_start > 0.0 {
                indices.extend([a, b, d]);
            }
            if iy != hs - 1 || theta_end < PI {
                indices.extend([b, c, d]);
            }
        }
    }

    finish(&indices, &vertices, &normals, &uvs)
}

// ── TorusGeometry ───────────────────────────────────────────────────────

/// `new TorusGeometry(radius, tube, radialSegments, tubularSegments, arc)`;
/// defaults `1, 0.4, 12, 48, 2π`. In the XY plane.
pub fn torus_geometry(
    radius: f64,
    tube: f64,
    radial_segments: f64,
    tubular_segments: f64,
    arc: f64,
) -> BufferGeometry {
    let radial_segments = radial_segments.floor();
    let tubular_segments = tubular_segments.floor();
    let rs = radial_segments as u32;
    let ts = tubular_segments as u32;

    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    // generate vertices, normals and uvs
    for j in 0..=rs {
        for i in 0..=ts {
            let u = i as f64 / tubular_segments * arc;
            let v = j as f64 / radial_segments * PI * 2.0;
            // vertex
            let vertex = Vector3::new(
                (radius + tube * kernel::cos(v)) * kernel::cos(u),
                (radius + tube * kernel::cos(v)) * kernel::sin(u),
                tube * kernel::sin(v),
            );
            vertices.extend([vertex.x, vertex.y, vertex.z]);
            // normal
            let center = Vector3::new(radius * kernel::cos(u), radius * kernel::sin(u), 0.0);
            let normal = (vertex - center).normalize();
            normals.extend([normal.x, normal.y, normal.z]);
            // uv
            uvs.push(i as f64 / tubular_segments);
            uvs.push(j as f64 / radial_segments);
        }
    }

    // generate indices
    for j in 1..=rs {
        for i in 1..=ts {
            // indices
            let a = (ts + 1) * j + i - 1;
            let b = (ts + 1) * (j - 1) + i - 1;
            let c = (ts + 1) * (j - 1) + i;
            let d = (ts + 1) * j + i;
            // faces
            indices.extend([a, b, d]);
            indices.extend([b, c, d]);
        }
    }

    finish(&indices, &vertices, &normals, &uvs)
}

// ── CapsuleGeometry ─────────────────────────────────────────────────────

/// `new CapsuleGeometry(radius, height, capSegments, radialSegments,
/// heightSegments)`; defaults `1, 1, 4, 8, 1`. Along y; `height` is the
/// straight middle part.
pub fn capsule_geometry(
    radius: f64,
    height: f64,
    cap_segments: f64,
    radial_segments: f64,
    height_segments: f64,
) -> BufferGeometry {
    let height = js::max(0.0, height);
    let cap_segments = js::max(1.0, cap_segments.floor());
    let radial_segments = js::max(3.0, radial_segments.floor());
    let height_segments = js::max(1.0, height_segments.floor());

    // buffers
    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    // helper variables
    let half_height = height / 2.0;
    let cap_arc_length = (PI / 2.0) * radius;
    let cylinder_part_length = height;
    let total_arc_length = 2.0 * cap_arc_length + cylinder_part_length;
    let num_vertical_segments = cap_segments * 2.0 + height_segments;
    let vertices_per_row = radial_segments as u32 + 1;
    let nvs = num_vertical_segments as u32;
    let rs = radial_segments as u32;

    // generate vertices, normals, and uvs
    for iy in 0..=nvs {
        let fy = iy as f64;
        let current_arc_length;
        let profile_y;
        let profile_radius;
        let normal_y_component;

        if fy <= cap_segments {
            // bottom cap
            let segment_progress = fy / cap_segments;
            let angle = (segment_progress * PI) / 2.0;
            profile_y = -half_height - radius * kernel::cos(angle);
            profile_radius = radius * kernel::sin(angle);
            normal_y_component = -radius * kernel::cos(angle);
            current_arc_length = segment_progress * cap_arc_length;
        } else if fy <= cap_segments + height_segments {
            // middle section
            let segment_progress = (fy - cap_segments) / height_segments;
            profile_y = -half_height + segment_progress * height;
            profile_radius = radius;
            normal_y_component = 0.0;
            current_arc_length = cap_arc_length + segment_progress * cylinder_part_length;
        } else {
            // top cap
            let segment_progress = (fy - cap_segments - height_segments) / cap_segments;
            let angle = (segment_progress * PI) / 2.0;
            profile_y = half_height + radius * kernel::sin(angle);
            profile_radius = radius * kernel::cos(angle);
            normal_y_component = radius * kernel::sin(angle);
            current_arc_length =
                cap_arc_length + cylinder_part_length + segment_progress * cap_arc_length;
        }

        let v = js::max(0.0, js::min(1.0, current_arc_length / total_arc_length));

        // special case for the poles
        let mut u_offset = 0.0;
        if iy == 0 {
            u_offset = 0.5 / radial_segments;
        } else if iy == nvs {
            u_offset = -0.5 / radial_segments;
        }

        for ix in 0..=rs {
            let u = ix as f64 / radial_segments;
            let theta = u * PI * 2.0;
            let sin_theta = kernel::sin(theta);
            let cos_theta = kernel::cos(theta);
            // vertex
            let vertex = Vector3::new(
                -profile_radius * cos_theta,
                profile_y,
                profile_radius * sin_theta,
            );
            vertices.extend([vertex.x, vertex.y, vertex.z]);
            // normal
            let normal = Vector3::new(
                -profile_radius * cos_theta,
                normal_y_component,
                profile_radius * sin_theta,
            )
            .normalize();
            normals.extend([normal.x, normal.y, normal.z]);
            // uv
            uvs.extend([u + u_offset, v]);
        }

        if iy > 0 {
            let prev_index_row = (iy - 1) * vertices_per_row;
            for ix in 0..rs {
                let i1 = prev_index_row + ix;
                let i2 = prev_index_row + ix + 1;
                let i3 = iy * vertices_per_row + ix;
                let i4 = iy * vertices_per_row + ix + 1;
                indices.extend([i1, i2, i3]);
                indices.extend([i2, i4, i3]);
            }
        }
    }

    finish(&indices, &vertices, &normals, &uvs)
}

// ── LatheGeometry ───────────────────────────────────────────────────────

/// `new LatheGeometry(points, segments, phiStart, phiLength)`; defaults
/// `[(0, -0.5), (0.5, 0), (0, 0.5)], 12, 0, 2π`. The profile is in (x, y)
/// = (radius, height). Attributes come in the order `position`, `uv`,
/// `normal`, as in three.
pub fn lathe_geometry(
    points: &[Vector2],
    segments: f64,
    phi_start: f64,
    phi_length: f64,
) -> BufferGeometry {
    let segments = segments.floor();
    // clamp phiLength so it's in range of [ 0, 2PI ]
    let phi_length = clamp(phi_length, 0.0, PI * 2.0);

    // buffers
    let mut indices = Vec::new();
    let mut vertices = Vec::new();
    let mut uvs = Vec::new();
    let mut init_normals: Vec<f64> = Vec::new();
    let mut normals = Vec::new();

    // helper variables
    let inverse_segments = 1.0 / segments;
    let mut normal = Vector3::default();
    let mut cur_normal;
    let mut prev_normal = Vector3::default();
    let n = points.len();

    // pre-compute normals for initial "meridian"
    for j in 0..n {
        if j == 0 {
            // special handling for 1st vertex on path
            let dx = points[j + 1].x - points[j].x;
            let dy = points[j + 1].y - points[j].y;
            normal.x = dy * 1.0;
            normal.y = -dx;
            normal.z = dy * 0.0;
            prev_normal = normal;
            normal = normal.normalize();
            init_normals.extend([normal.x, normal.y, normal.z]);
        } else if j == n - 1 {
            // special handling for last Vertex on path
            init_normals.extend([prev_normal.x, prev_normal.y, prev_normal.z]);
        } else {
            // default handling for all vertices in between
            let dx = points[j + 1].x - points[j].x;
            let dy = points[j + 1].y - points[j].y;
            normal.x = dy * 1.0;
            normal.y = -dx;
            normal.z = dy * 0.0;
            cur_normal = normal;
            normal.x += prev_normal.x;
            normal.y += prev_normal.y;
            normal.z += prev_normal.z;
            normal = normal.normalize();
            init_normals.extend([normal.x, normal.y, normal.z]);
            prev_normal = cur_normal;
        }
    }

    // generate vertices, uvs and normals
    let segs = segments as i64;
    for i in 0..=segs {
        let phi = phi_start + i as f64 * inverse_segments * phi_length;
        let sin = kernel::sin(phi);
        let cos = kernel::cos(phi);
        for j in 0..n {
            // vertex
            let vertex = Vector3::new(points[j].x * sin, points[j].y, points[j].x * cos);
            vertices.extend([vertex.x, vertex.y, vertex.z]);
            // uv
            let uv = Vector2::new(i as f64 / segments, j as f64 / (n - 1) as f64);
            uvs.extend([uv.x, uv.y]);
            // normal
            let x = init_normals[3 * j] * sin;
            let y = init_normals[3 * j + 1];
            let z = init_normals[3 * j] * cos;
            normals.extend([x, y, z]);
        }
    }

    // indices
    let np = n as u32;
    for i in 0..segs.max(0) as u32 {
        for j in 0..np - 1 {
            let base = j + i * np;
            let a = base;
            let b = base + np;
            let c = base + np + 1;
            let d = base + 1;
            // faces
            indices.extend([a, b, d]);
            indices.extend([c, d, b]);
        }
    }

    // build geometry
    let mut g = BufferGeometry::new();
    g.set_index(&indices);
    g.set_attribute("position", BufferAttribute::from_f64(&vertices, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uvs, 2));
    g.set_attribute("normal", BufferAttribute::from_f64(&normals, 3));
    g
}
