//! `BufferGeometry` (three.js r180 `src/core/BufferGeometry.js`): named
//! attributes in insertion order, an optional index, draw groups, bounds.
//!
//! Attribute order matters: `mergeGeometries` and `toNonIndexed` follow it,
//! and `LatheGeometry` sets `uv` before `normal`. As with a JS object,
//! setting an attribute that exists keeps its place, and deleting one and
//! setting it again moves it to the end.

use mr_scene::{
    AttributeRef, Buffer, BufferData, DrawRange as SceneDrawRange, GroupDesc, MeshDesc,
};

use super::attribute::{BufferAttribute, zeroed};
use super::math::{Box3, Matrix3, Matrix4, Quaternion, Sphere, Vector3};

/// A draw group (`geometry.groups[i]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Group {
    pub start: usize,
    pub count: usize,
    pub material_index: usize,
}

/// `geometry.drawRange`; `count: None` is three's `Infinity`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawRange {
    pub start: usize,
    pub count: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BufferGeometry {
    pub index: Option<BufferAttribute>,
    pub attributes: Vec<(String, BufferAttribute)>,
    pub groups: Vec<Group>,
    pub bounding_box: Option<Box3>,
    pub bounding_sphere: Option<Sphere>,
    pub draw_range: DrawRange,
}

/// three's `arrayNeedsUint32`: any value at or above 65535 (the primitive
/// restart index) needs 32 bits.
pub fn array_needs_uint32(array: &[u32]) -> bool {
    array.iter().rev().any(|&v| v >= 65535)
}

impl BufferGeometry {
    pub fn new() -> Self {
        BufferGeometry::default()
    }

    /// `setIndex(array)`: a `Uint16BufferAttribute`, or a
    /// `Uint32BufferAttribute` when [`array_needs_uint32`].
    pub fn set_index(&mut self, index: &[u32]) {
        self.index = Some(if array_needs_uint32(index) {
            BufferAttribute::from_u32(index.to_vec(), 1)
        } else {
            BufferAttribute::from_u16(index, 1)
        });
    }

    /// `setIndex(attribute)`.
    pub fn set_index_attribute(&mut self, index: Option<BufferAttribute>) {
        self.index = index;
    }

    pub fn get_attribute(&self, name: &str) -> Option<&BufferAttribute> {
        self.attributes
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, a)| a)
    }

    pub fn get_attribute_mut(&mut self, name: &str) -> Option<&mut BufferAttribute> {
        self.attributes
            .iter_mut()
            .find(|(n, _)| n == name)
            .map(|(_, a)| a)
    }

    /// `setAttribute(name, attribute)`.
    pub fn set_attribute(&mut self, name: &str, attribute: BufferAttribute) {
        match self.attributes.iter_mut().find(|(n, _)| n == name) {
            Some((_, a)) => *a = attribute,
            None => self.attributes.push((name.to_string(), attribute)),
        }
    }

    /// `deleteAttribute(name)`.
    pub fn delete_attribute(&mut self, name: &str) {
        self.attributes.retain(|(n, _)| n != name);
    }

    pub fn has_attribute(&self, name: &str) -> bool {
        self.get_attribute(name).is_some()
    }

    /// The `position` attribute (every generator sets one).
    pub fn position(&self) -> &BufferAttribute {
        self.get_attribute("position")
            .expect("geometry has a position attribute")
    }

    pub fn add_group(&mut self, start: usize, count: usize, material_index: usize) {
        self.groups.push(Group {
            start,
            count,
            material_index,
        });
    }

    pub fn clear_groups(&mut self) {
        self.groups.clear();
    }

    pub fn set_draw_range(&mut self, start: usize, count: Option<usize>) {
        self.draw_range = DrawRange { start, count };
    }

    /// `applyMatrix4(matrix)`: positions by the matrix, normals by its
    /// normal matrix, tangents as directions; bounds recomputed if set.
    pub fn apply_matrix4(&mut self, matrix: &Matrix4) -> &mut Self {
        if let Some(position) = self.get_attribute_mut("position") {
            position.apply_matrix4(matrix);
        }
        if let Some(normal) = self.get_attribute_mut("normal") {
            let normal_matrix = Matrix3::get_normal_matrix(matrix);
            normal.apply_normal_matrix(&normal_matrix);
        }
        if let Some(tangent) = self.get_attribute_mut("tangent") {
            tangent.transform_direction(matrix);
        }
        if self.bounding_box.is_some() {
            self.compute_bounding_box();
        }
        if self.bounding_sphere.is_some() {
            self.compute_bounding_sphere();
        }
        self
    }

    pub fn apply_quaternion(&mut self, q: Quaternion) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_rotation_from_quaternion(q))
    }

    /// Rotate about the world x axis.
    pub fn rotate_x(&mut self, angle: f64) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_rotation_x(angle))
    }

    pub fn rotate_y(&mut self, angle: f64) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_rotation_y(angle))
    }

    pub fn rotate_z(&mut self, angle: f64) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_rotation_z(angle))
    }

    pub fn translate(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_translation(x, y, z))
    }

    pub fn scale(&mut self, x: f64, y: f64, z: f64) -> &mut Self {
        self.apply_matrix4(&Matrix4::make_scale(x, y, z))
    }

    /// `lookAt(vector)`: rotate so that +z faces `vector`, through an
    /// `Object3D` at the origin with `up` (0, 1, 0), as three does.
    pub fn look_at(&mut self, vector: Vector3) -> &mut Self {
        let position = Vector3::new(0.0, 0.0, 0.0);
        let mut m1 = Matrix4::IDENTITY;
        m1.look_at(vector, position, Vector3::new(0.0, 1.0, 0.0));
        let quaternion = Quaternion::from_rotation_matrix(&m1);
        let matrix = Matrix4::compose(position, quaternion, Vector3::splat(1.0));
        self.apply_matrix4(&matrix)
    }

    /// `center()`: translate the bounding box's centre to the origin.
    pub fn center(&mut self) -> &mut Self {
        self.compute_bounding_box();
        let offset = -self.bounding_box.unwrap_or_default().get_center();
        self.translate(offset.x, offset.y, offset.z)
    }

    pub fn compute_bounding_box(&mut self) {
        let mut bb = Box3::EMPTY;
        if let Some(position) = self.get_attribute("position") {
            for i in 0..position.count() {
                bb.expand_by_point(position.get_vector3(i));
            }
        }
        self.bounding_box = Some(bb);
    }

    pub fn compute_bounding_sphere(&mut self) {
        let mut sphere = self.bounding_sphere.unwrap_or_default();
        if let Some(position) = self.get_attribute("position") {
            // first, find the center of the bounding sphere
            let mut bb = Box3::EMPTY;
            for i in 0..position.count() {
                bb.expand_by_point(position.get_vector3(i));
            }
            let center = bb.get_center();
            // second, try to find a boundingSphere with a radius smaller than the
            // boundingSphere of the boundingBox: sqrt(3) smaller in the best case
            let mut max_radius_sq = 0.0;
            for i in 0..position.count() {
                let v = position.get_vector3(i);
                max_radius_sq = mr_math::js::max(max_radius_sq, center.distance_to_squared(v));
            }
            sphere.center = center;
            sphere.radius = f64::sqrt(max_radius_sq);
        }
        self.bounding_sphere = Some(sphere);
    }

    /// `computeVertexNormals()`: area-weighted face normals summed at each
    /// vertex of an indexed geometry, or each triangle's own normal for a
    /// non-indexed one, then normalised.
    pub fn compute_vertex_normals(&mut self) {
        let Some(position) = self.get_attribute("position").cloned() else {
            return;
        };
        let mut normal = match self.get_attribute("normal") {
            None => {
                BufferAttribute::new(BufferData::F32(vec![0.0; position.count() * 3]), 3, false)
            }
            Some(n) => {
                let mut n = n.clone();
                // reset existing normals to zero
                for i in 0..n.count() {
                    n.set_xyz(i, 0.0, 0.0, 0.0);
                }
                n
            }
        };
        if let Some(index) = &self.index {
            let mut i = 0;
            while i < index.count() {
                let va = index.get_x(i) as usize;
                let vb = index.get_x(i + 1) as usize;
                let vc = index.get_x(i + 2) as usize;
                let pa = position.get_vector3(va);
                let pb = position.get_vector3(vb);
                let pc = position.get_vector3(vc);
                let cb = (pc - pb).cross(pa - pb);
                let na = normal.get_vector3(va) + cb;
                let nb = normal.get_vector3(vb) + cb;
                let nc = normal.get_vector3(vc) + cb;
                normal.set_vector3(va, na);
                normal.set_vector3(vb, nb);
                normal.set_vector3(vc, nc);
                i += 3;
            }
        } else {
            // non-indexed elements (unconnected triangle soup)
            let mut i = 0;
            while i < position.count() {
                let pa = position.get_vector3(i);
                let pb = position.get_vector3(i + 1);
                let pc = position.get_vector3(i + 2);
                let cb = (pc - pb).cross(pa - pb);
                normal.set_vector3(i, cb);
                normal.set_vector3(i + 1, cb);
                normal.set_vector3(i + 2, cb);
                i += 3;
            }
        }
        self.set_attribute("normal", normal);
        self.normalize_normals();
    }

    pub fn normalize_normals(&mut self) {
        let normals = self
            .get_attribute_mut("normal")
            .expect("geometry has normals");
        for i in 0..normals.count() {
            let v = normals.get_vector3(i).normalize();
            normals.set_vector3(i, v);
        }
    }

    /// `toNonIndexed()`: every attribute expanded through the index. A
    /// geometry without an index comes back as it is (three warns and
    /// returns `this`).
    pub fn to_non_indexed(&self) -> BufferGeometry {
        let Some(index) = &self.index else {
            return self.clone();
        };
        let indices: Vec<usize> = (0..index.array.len())
            .map(|i| index.array.get(i) as usize)
            .collect();
        let mut geometry2 = BufferGeometry::new();
        for (name, attribute) in &self.attributes {
            let item_size = attribute.item_size;
            let mut array2 = zeroed(&attribute.array, indices.len() * item_size);
            let mut index2 = 0;
            for &ix in &indices {
                let mut k = ix * item_size;
                for _ in 0..item_size {
                    copy_element(&mut array2, index2, &attribute.array, k);
                    index2 += 1;
                    k += 1;
                }
            }
            geometry2.set_attribute(
                name,
                BufferAttribute::new(array2, item_size, attribute.normalized),
            );
        }
        for g in &self.groups {
            geometry2.add_group(g.start, g.count, g.material_index);
        }
        geometry2
    }

    /// The vertex count (`position.count`).
    pub fn vertex_count(&self) -> usize {
        self.get_attribute("position")
            .map_or(0, BufferAttribute::count)
    }

    /// This geometry as an `mr_scene` mesh: each attribute and the index
    /// become buffers appended to `buffers`, referenced from the
    /// [`MeshDesc`].
    pub fn to_mesh_desc(&self, name: &str, buffers: &mut Vec<Buffer>) -> MeshDesc {
        let mut push = |a: &BufferAttribute| {
            buffers.push(Buffer {
                item_size: a.item_size as u32,
                normalized: a.normalized,
                data: a.array.clone(),
            });
            (buffers.len() - 1) as u32
        };
        let attributes = self
            .attributes
            .iter()
            .map(|(n, a)| AttributeRef {
                name: n.clone(),
                accessor: push(a),
                instanced: false,
                mesh_per_attribute: None,
            })
            .collect();
        let index = self.index.as_ref().map(&mut push);
        MeshDesc {
            name: name.to_string(),
            attributes,
            index,
            groups: self
                .groups
                .iter()
                .map(|g| GroupDesc {
                    start: g.start as u32,
                    count: g.count as f64,
                    material_index: g.material_index as u32,
                })
                .collect(),
            draw_range: SceneDrawRange {
                start: self.draw_range.start as u32,
                count: self.draw_range.count.map(|c| c as u32),
            },
            bounding_box: self
                .bounding_box
                .map(|b| vec![b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]),
            bounding_sphere: self
                .bounding_sphere
                .map(|s| vec![s.center.x, s.center.y, s.center.z, s.radius]),
        }
    }

    /// Appends this geometry to a scene as a mesh; returns its index.
    pub fn add_to_scene(&self, scene: &mut mr_scene::Scene, name: &str) -> u32 {
        let mesh = self.to_mesh_desc(name, &mut scene.buffers);
        scene.meshes.push(mesh);
        (scene.meshes.len() - 1) as u32
    }
}

/// `dst[i] = src[k]` between typed arrays of the same type.
pub(crate) fn copy_element(dst: &mut BufferData, i: usize, src: &BufferData, k: usize) {
    match (dst, src) {
        (BufferData::F32(d), BufferData::F32(s)) => d[i] = s[k],
        (BufferData::F64(d), BufferData::F64(s)) => d[i] = s[k],
        (BufferData::U8(d), BufferData::U8(s)) => d[i] = s[k],
        (BufferData::U16(d), BufferData::U16(s)) => d[i] = s[k],
        (BufferData::U32(d), BufferData::U32(s)) => d[i] = s[k],
        (BufferData::I8(d), BufferData::I8(s)) => d[i] = s[k],
        (BufferData::I16(d), BufferData::I16(s)) => d[i] = s[k],
        (BufferData::I32(d), BufferData::I32(s)) => d[i] = s[k],
        (d, s) => {
            let v = s.get(k);
            super::attribute::store(d, i, v);
        }
    }
}
