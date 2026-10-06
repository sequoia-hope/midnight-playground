//! A line-by-line port of the three.js r180 geometry the world builders use
//! (SPEC 5.2): the generators (Box, Cylinder, Cone, Plane, Circle, Sphere,
//! the polyhedra, Torus, Capsule, Lathe, Tube, Extrude with bevel, Shape),
//! `Shape`/`Path` and the curves, `ShapeUtils.triangulateShape` and earcut,
//! `CatmullRomCurve3`, `BufferGeometryUtils.mergeGeometries` and
//! `mergeVertices`, `BufferGeometry`'s transforms and normals, and the math
//! they need.
//!
//! Vertex order, index order, groups and UV layout are three's, value for
//! value: positions are computed in `f64` and rounded to `f32` where three
//! stores into a `Float32Array`, every inexact `Math` function goes through
//! `mp_math::kernel`, and the result matches a three.js dump taken with the
//! parity kernel bit for bit (`parity/golden/three_geom/`, written by
//! `tools/parity/three-geom.mjs`; DECISIONS D130 to D135).
//!
//! Generators are functions named after the class (`BoxGeometry` is
//! [`box_geometry`]) taking the constructor's arguments in order, every
//! default written out; three's defaults are given in each doc comment.
//!
//! Ported from three.js r180, which carries this licence:
//!
//! ```text
//! The MIT License
//!
//! Copyright © 2010-2025 three.js authors
//!
//! Permission is hereby granted, free of charge, to any person obtaining a copy
//! of this software and associated documentation files (the "Software"), to deal
//! in the Software without restriction, including without limitation the rights
//! to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
//! copies of the Software, and to permit persons to whom the Software is
//! furnished to do so, subject to the following conditions:
//!
//! The above copyright notice and this permission notice shall be included in
//! all copies or substantial portions of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
//! IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
//! FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
//! AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
//! LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
//! OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
//! THE SOFTWARE.
//! ```
//!
//! Earcut is mapbox/earcut v3.0.1 as vendored in three.js (ISC licence,
//! Copyright (c) 2024, Mapbox).

// The generators keep three's constructor signatures and index loops, so
// the port reads beside the JS (DECISIONS D52, D130).
#![allow(
    clippy::too_many_arguments,
    clippy::needless_range_loop,
    clippy::needless_late_init,
    clippy::explicit_counter_loop
)]

mod attribute;
mod curves;
mod earcut;
mod extrude;
mod geometry;
pub mod math;
mod polyhedron;
mod primitives;
mod tube;
mod utils;

pub use attribute::BufferAttribute;
pub use curves::{
    ARC_LENGTH_DIVISIONS, CatmullRomCurve3, CubicBezierCurve3, Curve2, Curve3, CurveType,
    EllipseCurve, FrenetFrames, LineCurve3, Path, QuadraticBezierCurve3, Shape, catmull_rom,
    cubic_bezier, quadratic_bezier, u_to_t_mapping,
};
pub use earcut::{earcut, is_clock_wise, shape_area, triangulate_shape};
pub use extrude::{ExtrudeOptions, extrude_geometry, shape_geometry, shape_geometry_array};
pub use geometry::{BufferGeometry, DrawRange, Group, array_needs_uint32};
pub use math::{Box3, Euler, EulerOrder, Matrix3, Matrix4, Quaternion, Sphere, Vector2, Vector3};
pub use polyhedron::{
    dodecahedron_geometry, icosahedron_geometry, octahedron_geometry, polyhedron_geometry,
    tetrahedron_geometry,
};
pub use primitives::{
    box_geometry, capsule_geometry, circle_geometry, cone_geometry, cylinder_geometry,
    lathe_geometry, plane_geometry, sphere_geometry, torus_geometry,
};
pub use tube::{tube_geometry, tube_geometry_with_frames};
pub use utils::{merge_attributes, merge_geometries, merge_vertices};
