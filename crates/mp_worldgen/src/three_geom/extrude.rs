//! `ExtrudeGeometry` and `ShapeGeometry` (three.js r180).
//!
//! `ExtrudeGeometry` is ported without `extrudePath` and with only the
//! default `WorldUVGenerator`: the world code extrudes straight along +z
//! and never passes either (DECISIONS D133).

use mp_math::{js, kernel};

use core::f64::consts::PI;

use super::attribute::BufferAttribute;
use super::curves::Shape;
use super::earcut::{is_clock_wise, triangulate_shape};
use super::geometry::BufferGeometry;
use super::math::Vector2;
use super::primitives::whole;

/// `ExtrudeGeometry`'s options, every one explicit. three's defaults are
/// [`ExtrudeOptions::THREE_DEFAULTS`]; note that an omitted `bevelSize` is
/// `bevelThickness - 0.1`, which [`ExtrudeOptions::new`] reproduces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtrudeOptions {
    pub curve_segments: f64,
    pub steps: f64,
    pub depth: f64,
    pub bevel_enabled: bool,
    pub bevel_thickness: f64,
    pub bevel_size: f64,
    pub bevel_offset: f64,
    pub bevel_segments: f64,
}

impl ExtrudeOptions {
    /// `{}`: curveSegments 12, steps 1, depth 1, bevel on, thickness 0.2,
    /// size 0.1 (thickness - 0.1), offset 0, 3 segments.
    pub const THREE_DEFAULTS: ExtrudeOptions = ExtrudeOptions {
        curve_segments: 12.0,
        steps: 1.0,
        depth: 1.0,
        bevel_enabled: true,
        bevel_thickness: 0.2,
        bevel_size: 0.2 - 0.1,
        bevel_offset: 0.0,
        bevel_segments: 3.0,
    };

    /// `{ depth, bevelEnabled: false }`, the most common call in the world
    /// code (curveSegments 12, steps 1).
    pub const fn flat(depth: f64) -> Self {
        ExtrudeOptions {
            depth,
            bevel_enabled: false,
            ..ExtrudeOptions::THREE_DEFAULTS
        }
    }
}

/// `new ExtrudeGeometry(shapes, options)`: non-indexed, `position` and `uv`
/// then computed normals; per shape a group 0 (the lids) and a group 1 (the
/// sides).
pub fn extrude_geometry(shapes: &[Shape], options: &ExtrudeOptions) -> BufferGeometry {
    let mut vertices_array: Vec<f64> = Vec::new();
    let mut uv_array: Vec<f64> = Vec::new();
    let mut groups = Vec::new();

    for shape in shapes {
        add_shape(
            shape,
            options,
            &mut vertices_array,
            &mut uv_array,
            &mut groups,
        );
    }

    let mut g = BufferGeometry::new();
    g.groups = groups;
    g.set_attribute("position", BufferAttribute::from_f64(&vertices_array, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uv_array, 2));
    g.compute_vertex_normals();
    g
}

fn add_shape(
    shape: &Shape,
    options: &ExtrudeOptions,
    vertices_array: &mut Vec<f64>,
    uv_array: &mut Vec<f64>,
    groups: &mut Vec<super::geometry::Group>,
) {
    let mut placeholder: Vec<f64> = Vec::new();

    let curve_segments = whole(options.curve_segments, "ExtrudeGeometry curveSegments");
    let steps = whole(options.steps, "ExtrudeGeometry steps");
    let depth = options.depth;
    let bevel_enabled = options.bevel_enabled;
    let mut bevel_thickness = options.bevel_thickness;
    let mut bevel_size = options.bevel_size;
    let mut bevel_offset = options.bevel_offset;
    let mut bevel_segments = whole(options.bevel_segments, "ExtrudeGeometry bevelSegments");

    // safeguards if bevels are not enabled
    if !bevel_enabled {
        bevel_segments = 0.0;
        bevel_thickness = 0.0;
        bevel_size = 0.0;
        bevel_offset = 0.0;
    }

    // Variables initialization
    let (mut vertices, mut holes) = shape.extract_points(curve_segments as usize);
    let reverse = !is_clock_wise(&vertices);
    if reverse {
        vertices.reverse();
        // Maybe we should also check if holes are in the opposite direction, just to be safe ...
        for ahole in holes.iter_mut() {
            if is_clock_wise(ahole) {
                ahole.reverse();
            }
        }
    }

    merge_overlapping_points(&mut vertices);
    for h in holes.iter_mut() {
        merge_overlapping_points(h);
    }

    let num_holes = holes.len();

    // vertices has all points but contour has only points of circumference
    let mut contour = vertices;
    let mut vertices = contour.clone();
    for ahole in &holes {
        vertices.extend_from_slice(ahole);
    }

    let vlen = vertices.len();

    let mut contour_movements = Vec::with_capacity(contour.len());
    {
        let il = contour.len();
        let mut j = il.wrapping_sub(1);
        let mut k = 1;
        for i in 0..il {
            if j == il {
                j = 0;
            }
            if k == il {
                k = 0;
            }
            //  (j)---(i)---(k)
            contour_movements.push(get_bevel_vec(contour[i], contour[j], contour[k]));
            j = j.wrapping_add(1);
            k += 1;
        }
    }

    let mut holes_movements = Vec::with_capacity(num_holes);
    let mut vertices_movements = contour_movements.clone();
    for ahole in &holes {
        let il = ahole.len();
        let mut one_hole_movements = Vec::with_capacity(il);
        let mut j = il.wrapping_sub(1);
        let mut k = 1;
        for i in 0..il {
            if j == il {
                j = 0;
            }
            if k == il {
                k = 0;
            }
            //  (j)---(i)---(k)
            one_hole_movements.push(get_bevel_vec(ahole[i], ahole[j], ahole[k]));
            j = j.wrapping_add(1);
            k += 1;
        }
        vertices_movements.extend_from_slice(&one_hole_movements);
        holes_movements.push(one_hole_movements);
    }

    let v = |placeholder: &mut Vec<f64>, x: f64, y: f64, z: f64| {
        placeholder.push(x);
        placeholder.push(y);
        placeholder.push(z);
    };

    let faces;
    if bevel_segments == 0.0 {
        faces = triangulate_shape(&mut contour, &mut holes);
    } else {
        let mut contracted_contour_vertices = Vec::new();
        let mut expanded_hole_vertices = Vec::new();

        // Loop bevelSegments, 1 for the front, 1 for the back
        for b in 0..bevel_segments as usize {
            let t = b as f64 / bevel_segments;
            let z = bevel_thickness * kernel::cos(t * PI / 2.0);
            let bs = bevel_size * kernel::sin(t * PI / 2.0) + bevel_offset;

            // contract shape
            for i in 0..contour.len() {
                let vert = scale_pt2(contour[i], contour_movements[i], bs);
                v(&mut placeholder, vert.x, vert.y, -z);
                if t == 0.0 {
                    contracted_contour_vertices.push(vert);
                }
            }

            // expand holes
            for h in 0..num_holes {
                let ahole = &holes[h];
                let one_hole_movements = &holes_movements[h];
                let mut one_hole_vertices = Vec::new();
                for i in 0..ahole.len() {
                    let vert = scale_pt2(ahole[i], one_hole_movements[i], bs);
                    v(&mut placeholder, vert.x, vert.y, -z);
                    if t == 0.0 {
                        one_hole_vertices.push(vert);
                    }
                }
                if t == 0.0 {
                    expanded_hole_vertices.push(one_hole_vertices);
                }
            }
        }

        faces = triangulate_shape(
            &mut contracted_contour_vertices,
            &mut expanded_hole_vertices,
        );
    }

    let flen = faces.len();
    let bs = bevel_size + bevel_offset;

    // Back facing vertices
    for i in 0..vlen {
        let vert = if bevel_enabled {
            scale_pt2(vertices[i], vertices_movements[i], bs)
        } else {
            vertices[i]
        };
        v(&mut placeholder, vert.x, vert.y, 0.0);
    }

    // Add stepped vertices...
    // Including front facing vertices
    for s in 1..=steps as usize {
        for i in 0..vlen {
            let vert = if bevel_enabled {
                scale_pt2(vertices[i], vertices_movements[i], bs)
            } else {
                vertices[i]
            };
            v(&mut placeholder, vert.x, vert.y, depth / steps * s as f64);
        }
    }

    // Add bevel segments planes
    for b in (0..bevel_segments as usize).rev() {
        let t = b as f64 / bevel_segments;
        let z = bevel_thickness * kernel::cos(t * PI / 2.0);
        let bs = bevel_size * kernel::sin(t * PI / 2.0) + bevel_offset;

        // contract shape
        for i in 0..contour.len() {
            let vert = scale_pt2(contour[i], contour_movements[i], bs);
            v(&mut placeholder, vert.x, vert.y, depth + z);
        }

        // expand holes
        for h in 0..holes.len() {
            let ahole = &holes[h];
            let one_hole_movements = &holes_movements[h];
            for i in 0..ahole.len() {
                let vert = scale_pt2(ahole[i], one_hole_movements[i], bs);
                v(&mut placeholder, vert.x, vert.y, depth + z);
            }
        }
    }

    let mut w = Writer {
        placeholder: &placeholder,
        vertices_array,
        uv_array,
    };

    /* Faces */

    // Top and bottom faces
    {
        let start = w.vertices_array.len() / 3;
        if bevel_enabled {
            let mut layer = 0; // steps + 1
            let mut offset = vlen * layer;
            // Bottom faces
            for face in faces.iter().take(flen) {
                w.f3(face[2] + offset, face[1] + offset, face[0] + offset);
            }
            layer = steps as usize + bevel_segments as usize * 2;
            offset = vlen * layer;
            // Top faces
            for face in faces.iter().take(flen) {
                w.f3(face[0] + offset, face[1] + offset, face[2] + offset);
            }
        } else {
            // Bottom faces
            for face in faces.iter().take(flen) {
                w.f3(face[2], face[1], face[0]);
            }
            // Top faces
            let top = vlen * steps as usize;
            for face in faces.iter().take(flen) {
                w.f3(face[0] + top, face[1] + top, face[2] + top);
            }
        }
        groups.push(super::geometry::Group {
            start,
            count: w.vertices_array.len() / 3 - start,
            material_index: 0,
        });
    }

    // Create faces for the z-sides of the shape
    {
        let start = w.vertices_array.len() / 3;
        let sl = steps as usize + bevel_segments as usize * 2;
        let mut layeroffset = 0;
        w.sidewalls(contour.len(), layeroffset, vlen, sl);
        layeroffset += contour.len();
        for ahole in &holes {
            w.sidewalls(ahole.len(), layeroffset, vlen, sl);
            layeroffset += ahole.len();
        }
        groups.push(super::geometry::Group {
            start,
            count: w.vertices_array.len() / 3 - start,
            material_index: 1,
        });
    }
}

struct Writer<'a> {
    placeholder: &'a [f64],
    vertices_array: &'a mut Vec<f64>,
    uv_array: &'a mut Vec<f64>,
}

impl Writer<'_> {
    fn sidewalls(&mut self, contour_len: usize, layeroffset: usize, vlen: usize, sl: usize) {
        let mut i = contour_len as i64;
        loop {
            i -= 1;
            if i < 0 {
                break;
            }
            let j = i as usize;
            let k = if i - 1 < 0 {
                contour_len - 1
            } else {
                (i - 1) as usize
            };
            for s in 0..sl {
                let slen1 = vlen * s;
                let slen2 = vlen * (s + 1);
                let a = layeroffset + j + slen1;
                let b = layeroffset + k + slen1;
                let c = layeroffset + k + slen2;
                let d = layeroffset + j + slen2;
                self.f4(a, b, c, d);
            }
        }
    }

    fn f3(&mut self, a: usize, b: usize, c: usize) {
        self.add_vertex(a);
        self.add_vertex(b);
        self.add_vertex(c);
        let next_index = self.vertices_array.len() / 3;
        let uvs = generate_top_uv(
            self.vertices_array,
            next_index - 3,
            next_index - 2,
            next_index - 1,
        );
        self.add_uv(uvs[0]);
        self.add_uv(uvs[1]);
        self.add_uv(uvs[2]);
    }

    fn f4(&mut self, a: usize, b: usize, c: usize, d: usize) {
        self.add_vertex(a);
        self.add_vertex(b);
        self.add_vertex(d);
        self.add_vertex(b);
        self.add_vertex(c);
        self.add_vertex(d);
        let next_index = self.vertices_array.len() / 3;
        let uvs = generate_side_wall_uv(
            self.vertices_array,
            next_index - 6,
            next_index - 3,
            next_index - 2,
            next_index - 1,
        );
        self.add_uv(uvs[0]);
        self.add_uv(uvs[1]);
        self.add_uv(uvs[3]);
        self.add_uv(uvs[1]);
        self.add_uv(uvs[2]);
        self.add_uv(uvs[3]);
    }

    fn add_vertex(&mut self, index: usize) {
        self.vertices_array.push(self.placeholder[index * 3]);
        self.vertices_array.push(self.placeholder[index * 3 + 1]);
        self.vertices_array.push(self.placeholder[index * 3 + 2]);
    }

    fn add_uv(&mut self, v: Vector2) {
        self.uv_array.push(v.x);
        self.uv_array.push(v.y);
    }
}

/// Removes consecutive points closer than a threshold scaled to the
/// coordinates (the last point is compared with the first, and the first
/// removed if they overlap).
fn merge_overlapping_points(points: &mut Vec<Vector2>) {
    const THRESHOLD: f64 = 1e-10;
    const THRESHOLD_SQ: f64 = THRESHOLD * THRESHOLD;
    if points.is_empty() {
        return;
    }
    let mut prev_pos = points[0];
    let mut i = 1;
    while i <= points.len() {
        let current_index = i % points.len();
        let current_pos = points[current_index];
        let dx = current_pos.x - prev_pos.x;
        let dy = current_pos.y - prev_pos.y;
        let dist_sq = dx * dx + dy * dy;
        let scaling_factor_sqrt = js::max_n(&[
            current_pos.x.abs(),
            current_pos.y.abs(),
            prev_pos.x.abs(),
            prev_pos.y.abs(),
        ]);
        let threshold_sq_scaled = THRESHOLD_SQ * scaling_factor_sqrt * scaling_factor_sqrt;
        if dist_sq <= threshold_sq_scaled {
            points.remove(current_index);
            // `i--; continue;` then the loop's `i++`
            if points.is_empty() {
                return;
            }
            continue;
        }
        prev_pos = current_pos;
        i += 1;
    }
}

fn scale_pt2(pt: Vector2, vec: Vector2, size: f64) -> Vector2 {
    pt.add_scaled_vector(vec, size)
}

// Find directions for point movement
fn get_bevel_vec(in_pt: Vector2, in_prev: Vector2, in_next: Vector2) -> Vector2 {
    // computes for inPt the corresponding point inPt' on a new contour
    //   shifted by 1 unit (length of normalized vector) to the left
    // if we walk along contour clockwise, this new contour is outside the old one
    //
    // inPt' is the intersection of the two lines parallel to the two
    //  adjacent edges of inPt at a distance of 1 unit on the left side.

    let v_trans_x;
    let v_trans_y;
    let shrink_by; // resulting translation vector for inPt

    // good reading for geometry algorithms (here: line-line intersection)
    // http://geomalgorithms.com/a05-_intersect-1.html

    let v_prev_x = in_pt.x - in_prev.x;
    let v_prev_y = in_pt.y - in_prev.y;
    let v_next_x = in_next.x - in_pt.x;
    let v_next_y = in_next.y - in_pt.y;

    let v_prev_lensq = v_prev_x * v_prev_x + v_prev_y * v_prev_y;

    // check for collinear edges
    let collinear0 = v_prev_x * v_next_y - v_prev_y * v_next_x;

    if collinear0.abs() > f64::EPSILON {
        // not collinear

        // length of vectors for normalizing
        let v_prev_len = v_prev_lensq.sqrt();
        let v_next_len = (v_next_x * v_next_x + v_next_y * v_next_y).sqrt();

        // shift adjacent points by unit vectors to the left
        let pt_prev_shift_x = in_prev.x - v_prev_y / v_prev_len;
        let pt_prev_shift_y = in_prev.y + v_prev_x / v_prev_len;

        let pt_next_shift_x = in_next.x - v_next_y / v_next_len;
        let pt_next_shift_y = in_next.y + v_next_x / v_next_len;

        // scaling factor for v_prev to intersection point
        let sf = ((pt_next_shift_x - pt_prev_shift_x) * v_next_y
            - (pt_next_shift_y - pt_prev_shift_y) * v_next_x)
            / (v_prev_x * v_next_y - v_prev_y * v_next_x);

        // vector from inPt to intersection point
        v_trans_x = pt_prev_shift_x + v_prev_x * sf - in_pt.x;
        v_trans_y = pt_prev_shift_y + v_prev_y * sf - in_pt.y;

        // Don't normalize!, otherwise sharp corners become ugly
        //  but prevent crazy spikes
        let v_trans_lensq = v_trans_x * v_trans_x + v_trans_y * v_trans_y;
        if v_trans_lensq <= 2.0 {
            return Vector2::new(v_trans_x, v_trans_y);
        } else {
            shrink_by = (v_trans_lensq / 2.0).sqrt();
        }
    } else {
        // handle special case of collinear edges

        let mut direction_eq = false; // assumes: opposite

        if v_prev_x > f64::EPSILON {
            if v_next_x > f64::EPSILON {
                direction_eq = true;
            }
        } else if v_prev_x < -f64::EPSILON {
            if v_next_x < -f64::EPSILON {
                direction_eq = true;
            }
        } else if js::sign(v_prev_y) == js::sign(v_next_y) {
            direction_eq = true;
        }

        if direction_eq {
            v_trans_x = -v_prev_y;
            v_trans_y = v_prev_x;
            shrink_by = v_prev_lensq.sqrt();
        } else {
            v_trans_x = v_prev_x;
            v_trans_y = v_prev_y;
            shrink_by = (v_prev_lensq / 2.0).sqrt();
        }
    }

    Vector2::new(v_trans_x / shrink_by, v_trans_y / shrink_by)
}

/// `WorldUVGenerator.generateTopUV`.
fn generate_top_uv(
    vertices: &[f64],
    index_a: usize,
    index_b: usize,
    index_c: usize,
) -> [Vector2; 3] {
    let a_x = vertices[index_a * 3];
    let a_y = vertices[index_a * 3 + 1];
    let b_x = vertices[index_b * 3];
    let b_y = vertices[index_b * 3 + 1];
    let c_x = vertices[index_c * 3];
    let c_y = vertices[index_c * 3 + 1];
    [
        Vector2::new(a_x, a_y),
        Vector2::new(b_x, b_y),
        Vector2::new(c_x, c_y),
    ]
}

/// `WorldUVGenerator.generateSideWallUV`.
fn generate_side_wall_uv(
    vertices: &[f64],
    index_a: usize,
    index_b: usize,
    index_c: usize,
    index_d: usize,
) -> [Vector2; 4] {
    let a_x = vertices[index_a * 3];
    let a_y = vertices[index_a * 3 + 1];
    let a_z = vertices[index_a * 3 + 2];
    let b_x = vertices[index_b * 3];
    let b_y = vertices[index_b * 3 + 1];
    let b_z = vertices[index_b * 3 + 2];
    let c_x = vertices[index_c * 3];
    let c_y = vertices[index_c * 3 + 1];
    let c_z = vertices[index_c * 3 + 2];
    let d_x = vertices[index_d * 3];
    let d_y = vertices[index_d * 3 + 1];
    let d_z = vertices[index_d * 3 + 2];

    if (a_y - b_y).abs() < (a_x - b_x).abs() {
        [
            Vector2::new(a_x, 1.0 - a_z),
            Vector2::new(b_x, 1.0 - b_z),
            Vector2::new(c_x, 1.0 - c_z),
            Vector2::new(d_x, 1.0 - d_z),
        ]
    } else {
        [
            Vector2::new(a_y, 1.0 - a_z),
            Vector2::new(b_y, 1.0 - b_z),
            Vector2::new(c_y, 1.0 - c_z),
            Vector2::new(d_y, 1.0 - d_z),
        ]
    }
}

// ── ShapeGeometry ───────────────────────────────────────────────────────

/// `new ShapeGeometry(shape, curveSegments)`: one shape, no groups. three's
/// default `curveSegments` is 12.
pub fn shape_geometry(shape: &Shape, curve_segments: f64) -> BufferGeometry {
    shape_geometry_impl(std::slice::from_ref(shape), curve_segments, false)
}

/// `new ShapeGeometry([shapes], curveSegments)`: a group per shape.
pub fn shape_geometry_array(shapes: &[Shape], curve_segments: f64) -> BufferGeometry {
    shape_geometry_impl(shapes, curve_segments, true)
}

fn shape_geometry_impl(shapes: &[Shape], curve_segments: f64, array: bool) -> BufferGeometry {
    let curve_segments = whole(curve_segments, "ShapeGeometry curveSegments");
    let mut indices: Vec<u32> = Vec::new();
    let mut vertices: Vec<f64> = Vec::new();
    let mut normals: Vec<f64> = Vec::new();
    let mut uvs: Vec<f64> = Vec::new();
    let mut g = BufferGeometry::new();

    let mut group_start = 0;
    let mut group_count = 0;

    for (i, shape) in shapes.iter().enumerate() {
        let index_offset = (vertices.len() / 3) as u32;
        let (mut shape_vertices, mut shape_holes) = shape.extract_points(curve_segments as usize);

        // check direction of vertices
        if !is_clock_wise(&shape_vertices) {
            shape_vertices.reverse();
        }
        for shape_hole in shape_holes.iter_mut() {
            if is_clock_wise(shape_hole) {
                shape_hole.reverse();
            }
        }

        let faces = triangulate_shape(&mut shape_vertices, &mut shape_holes);

        // join vertices of inner and outer paths to a single array
        for shape_hole in &shape_holes {
            shape_vertices.extend_from_slice(shape_hole);
        }

        // vertices, normals, uvs
        for vertex in &shape_vertices {
            vertices.extend([vertex.x, vertex.y, 0.0]);
            normals.extend([0.0, 0.0, 1.0]);
            uvs.extend([vertex.x, vertex.y]); // world uvs
        }

        // indices
        for face in &faces {
            let a = face[0] as u32 + index_offset;
            let b = face[1] as u32 + index_offset;
            let c = face[2] as u32 + index_offset;
            indices.extend([a, b, c]);
            group_count += 3;
        }

        if array {
            g.add_group(group_start, group_count, i);
            group_start += group_count;
            group_count = 0;
        }
    }

    g.set_index(&indices);
    g.set_attribute("position", BufferAttribute::from_f64(&vertices, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(&normals, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uvs, 2));
    g
}
