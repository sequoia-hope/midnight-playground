//! `BufferAttribute` (three.js r180 `src/core/BufferAttribute.js`): a typed
//! array read and written in items of `item_size` components.
//!
//! The array is an [`mr_scene::BufferData`], so an attribute goes into a
//! scene as it is. Writes behave as stores into the JS typed array do: a
//! `Float32Array` rounds to `f32`, the integer arrays wrap (ToInt32,
//! ToUint32, and the 8- and 16-bit versions), and a `normalized`
//! attribute scales through three's `normalize` / `denormalize` first.

use mr_math::js;
use mr_scene::BufferData;

use super::math::{Matrix3, Matrix4, Vector3};

#[derive(Clone, Debug, PartialEq)]
pub struct BufferAttribute {
    pub array: BufferData,
    pub item_size: usize,
    pub normalized: bool,
}

impl BufferAttribute {
    /// `new BufferAttribute(array, itemSize, normalized)`.
    pub fn new(array: BufferData, item_size: usize, normalized: bool) -> Self {
        BufferAttribute {
            array,
            item_size,
            normalized,
        }
    }

    /// `new Float32BufferAttribute(values, itemSize)`: each value rounded to
    /// `f32`.
    pub fn from_f64(values: &[f64], item_size: usize) -> Self {
        let array = values.iter().map(|&v| v as f32).collect();
        BufferAttribute::new(BufferData::F32(array), item_size, false)
    }

    /// `new Float32BufferAttribute(values, itemSize)` from values already
    /// `f32`.
    pub fn from_f32(values: Vec<f32>, item_size: usize) -> Self {
        BufferAttribute::new(BufferData::F32(values), item_size, false)
    }

    /// `new Uint16BufferAttribute(values, 1)`: each value stored as a
    /// `Uint16Array` stores it (modulo 2^16).
    pub fn from_u16(values: &[u32], item_size: usize) -> Self {
        let array = values.iter().map(|&v| v as u16).collect();
        BufferAttribute::new(BufferData::U16(array), item_size, false)
    }

    /// `new Uint32BufferAttribute(values, 1)`.
    pub fn from_u32(values: Vec<u32>, item_size: usize) -> Self {
        BufferAttribute::new(BufferData::U32(values), item_size, false)
    }

    /// A zero-filled attribute of the same array type (`new
    /// array.constructor(len)`).
    pub fn zeroed_like(&self, len: usize) -> BufferData {
        zeroed(&self.array, len)
    }

    /// `count`: the number of items.
    pub fn count(&self) -> usize {
        self.array.len() / self.item_size
    }

    /// The `f32` values, for the usual `Float32Array` attribute.
    pub fn as_f32(&self) -> Option<&[f32]> {
        self.array.as_f32()
    }

    /// Element `k` of the array as stored (no denormalising): `array[k]`.
    pub fn raw(&self, k: usize) -> f64 {
        self.array.get(k)
    }

    /// `array[k] = v`, with the typed array's conversion.
    pub fn set_raw(&mut self, k: usize, v: f64) {
        store(&mut self.array, k, v);
    }

    pub fn get_component(&self, index: usize, component: usize) -> f64 {
        let v = self.array.get(index * self.item_size + component);
        if self.normalized {
            denormalize(v, &self.array)
        } else {
            v
        }
    }

    pub fn set_component(&mut self, index: usize, component: usize, value: f64) {
        let v = if self.normalized {
            normalize(value, &self.array)
        } else {
            value
        };
        store(&mut self.array, index * self.item_size + component, v);
    }

    pub fn get_x(&self, index: usize) -> f64 {
        self.get_component(index, 0)
    }

    pub fn get_y(&self, index: usize) -> f64 {
        self.get_component(index, 1)
    }

    pub fn get_z(&self, index: usize) -> f64 {
        self.get_component(index, 2)
    }

    pub fn get_w(&self, index: usize) -> f64 {
        self.get_component(index, 3)
    }

    pub fn set_x(&mut self, index: usize, x: f64) {
        self.set_component(index, 0, x);
    }

    pub fn set_y(&mut self, index: usize, y: f64) {
        self.set_component(index, 1, y);
    }

    pub fn set_z(&mut self, index: usize, z: f64) {
        self.set_component(index, 2, z);
    }

    pub fn set_w(&mut self, index: usize, w: f64) {
        self.set_component(index, 3, w);
    }

    pub fn set_xy(&mut self, index: usize, x: f64, y: f64) {
        self.set_component(index, 0, x);
        self.set_component(index, 1, y);
    }

    pub fn set_xyz(&mut self, index: usize, x: f64, y: f64, z: f64) {
        self.set_component(index, 0, x);
        self.set_component(index, 1, y);
        self.set_component(index, 2, z);
    }

    pub fn set_xyzw(&mut self, index: usize, x: f64, y: f64, z: f64, w: f64) {
        self.set_component(index, 0, x);
        self.set_component(index, 1, y);
        self.set_component(index, 2, z);
        self.set_component(index, 3, w);
    }

    /// `_vector.fromBufferAttribute(this, i)`.
    pub fn get_vector3(&self, index: usize) -> Vector3 {
        Vector3::new(self.get_x(index), self.get_y(index), self.get_z(index))
    }

    pub fn set_vector3(&mut self, index: usize, v: Vector3) {
        self.set_xyz(index, v.x, v.y, v.z);
    }

    pub fn apply_matrix3(&mut self, m: &Matrix3) {
        if self.item_size == 2 {
            for i in 0..self.count() {
                let v = super::math::Vector2::new(self.get_x(i), self.get_y(i)).apply_matrix3(m);
                self.set_xy(i, v.x, v.y);
            }
        } else if self.item_size == 3 {
            for i in 0..self.count() {
                let v = self.get_vector3(i).apply_matrix3(m);
                self.set_vector3(i, v);
            }
        }
    }

    pub fn apply_matrix4(&mut self, m: &Matrix4) {
        for i in 0..self.count() {
            let v = self.get_vector3(i).apply_matrix4(m);
            self.set_vector3(i, v);
        }
    }

    pub fn apply_normal_matrix(&mut self, m: &Matrix3) {
        for i in 0..self.count() {
            let v = self.get_vector3(i).apply_normal_matrix(m);
            self.set_vector3(i, v);
        }
    }

    pub fn transform_direction(&mut self, m: &Matrix4) {
        for i in 0..self.count() {
            let v = self.get_vector3(i).transform_direction(m);
            self.set_vector3(i, v);
        }
    }
}

/// A zero-filled array of the same type as `like`.
pub(crate) fn zeroed(like: &BufferData, len: usize) -> BufferData {
    match like {
        BufferData::F32(_) => BufferData::F32(vec![0.0; len]),
        BufferData::F64(_) => BufferData::F64(vec![0.0; len]),
        BufferData::U8(_) => BufferData::U8(vec![0; len]),
        BufferData::U16(_) => BufferData::U16(vec![0; len]),
        BufferData::U32(_) => BufferData::U32(vec![0; len]),
        BufferData::I8(_) => BufferData::I8(vec![0; len]),
        BufferData::I16(_) => BufferData::I16(vec![0; len]),
        BufferData::I32(_) => BufferData::I32(vec![0; len]),
    }
}

/// `array[k] = v` on a JS typed array: `Float32Array` rounds to `f32`, the
/// integer arrays convert as ToInt32/ToUint32 and keep the low bits.
pub(crate) fn store(array: &mut BufferData, k: usize, v: f64) {
    match array {
        BufferData::F32(a) => a[k] = v as f32,
        BufferData::F64(a) => a[k] = v,
        BufferData::U8(a) => a[k] = js::to_uint32(v) as u8,
        BufferData::U16(a) => a[k] = js::to_uint32(v) as u16,
        BufferData::U32(a) => a[k] = js::to_uint32(v),
        BufferData::I8(a) => a[k] = js::to_int32(v) as i8,
        BufferData::I16(a) => a[k] = js::to_int32(v) as i16,
        BufferData::I32(a) => a[k] = js::to_int32(v),
    }
}

/// three's `denormalize(value, array)`.
fn denormalize(value: f64, array: &BufferData) -> f64 {
    match array {
        BufferData::F32(_) | BufferData::F64(_) => value,
        BufferData::U32(_) => value / 4294967295.0,
        BufferData::U16(_) => value / 65535.0,
        BufferData::U8(_) => value / 255.0,
        BufferData::I32(_) => js::max(value / 2147483647.0, -1.0),
        BufferData::I16(_) => js::max(value / 32767.0, -1.0),
        BufferData::I8(_) => js::max(value / 127.0, -1.0),
    }
}

/// three's `normalize(value, array)`.
fn normalize(value: f64, array: &BufferData) -> f64 {
    match array {
        BufferData::F32(_) | BufferData::F64(_) => value,
        BufferData::U32(_) => js::round(value * 4294967295.0),
        BufferData::U16(_) => js::round(value * 65535.0),
        BufferData::U8(_) => js::round(value * 255.0),
        BufferData::I32(_) => js::round(value * 2147483647.0),
        BufferData::I16(_) => js::round(value * 32767.0),
        BufferData::I8(_) => js::round(value * 127.0),
    }
}
