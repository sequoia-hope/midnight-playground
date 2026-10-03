//! `TubeGeometry` (three.js r180): a tube along a 3D curve, oriented by the
//! curve's Frenet frames.

use core::f64::consts::PI;

use mr_math::kernel;

use super::attribute::BufferAttribute;
use super::curves::{Curve3, FrenetFrames};
use super::geometry::BufferGeometry;
use super::primitives::whole;

/// `new TubeGeometry(path, tubularSegments, radius, radialSegments,
/// closed)`; three's defaults `(a QuadraticBezierCurve3), 64, 1, 8, false`.
/// Also returns the frames three keeps on the geometry (`tangents`,
/// `normals`, `binormals`).
pub fn tube_geometry_with_frames(
    path: &dyn Curve3,
    tubular_segments: f64,
    radius: f64,
    radial_segments: f64,
    closed: bool,
) -> (BufferGeometry, FrenetFrames) {
    let tubular_segments = whole(tubular_segments, "TubeGeometry tubularSegments");
    let radial_segments = whole(radial_segments, "TubeGeometry radialSegments");
    let ts = tubular_segments as usize;
    let rs = radial_segments as usize;

    // expose internals
    let frames = path.compute_frenet_frames(ts, closed);

    // buffer
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // create buffer data
    let mut generate_segment = |i: usize| {
        // we use getPointAt to sample evenly distributed points from the given path
        let p = path.get_point_at(i as f64 / tubular_segments);

        // retrieve corresponding normal and binormal
        let n = frames.normals[i];
        let b = frames.binormals[i];

        // generate normals and vertices for the current segment
        for j in 0..=rs {
            let v = j as f64 / radial_segments * PI * 2.0;
            let sin = kernel::sin(v);
            let cos = -kernel::cos(v);

            // normal
            let mut normal = super::math::Vector3::new(
                cos * n.x + sin * b.x,
                cos * n.y + sin * b.y,
                cos * n.z + sin * b.z,
            );
            normal = normal.normalize();
            normals.extend([normal.x, normal.y, normal.z]);

            // vertex
            let vertex = super::math::Vector3::new(
                p.x + radius * normal.x,
                p.y + radius * normal.y,
                p.z + radius * normal.z,
            );
            vertices.extend([vertex.x, vertex.y, vertex.z]);
        }
    };

    for i in 0..ts {
        generate_segment(i);
    }

    // if the geometry is not closed, generate the last row of vertices and normals
    // at the regular position on the given path
    //
    // if the geometry is closed, duplicate the first row of vertices and normals (uvs will differ)
    generate_segment(if closed { 0 } else { ts });

    // uvs are generated in a separate function.
    // this makes it easy compute correct values for closed geometries
    for i in 0..=ts {
        for j in 0..=rs {
            uvs.push(i as f64 / tubular_segments);
            uvs.push(j as f64 / radial_segments);
        }
    }

    // finally create faces
    let rs32 = rs as u32;
    for j in 1..=ts as u32 {
        for i in 1..=rs32 {
            let a = (rs32 + 1) * (j - 1) + (i - 1);
            let b = (rs32 + 1) * j + (i - 1);
            let c = (rs32 + 1) * j + i;
            let d = (rs32 + 1) * (j - 1) + i;
            // faces
            indices.extend([a, b, d]);
            indices.extend([b, c, d]);
        }
    }

    let mut g = BufferGeometry::new();
    g.set_index(&indices);
    g.set_attribute("position", BufferAttribute::from_f64(&vertices, 3));
    g.set_attribute("normal", BufferAttribute::from_f64(&normals, 3));
    g.set_attribute("uv", BufferAttribute::from_f64(&uvs, 2));
    (g, frames)
}

/// `new TubeGeometry(path, tubularSegments, radius, radialSegments,
/// closed)`.
pub fn tube_geometry(
    path: &dyn Curve3,
    tubular_segments: f64,
    radius: f64,
    radial_segments: f64,
    closed: bool,
) -> BufferGeometry {
    tube_geometry_with_frames(path, tubular_segments, radius, radial_segments, closed).0
}
