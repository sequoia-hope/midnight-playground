//! `BufferGeometryUtils.mergeGeometries` and `mergeVertices` (three.js r180
//! `examples/jsm/utils/BufferGeometryUtils.js`). Morph attributes are not
//! ported; no geometry in the game has them.

use std::collections::BTreeMap;

use mp_math::{js, kernel};
use mp_scene::BufferData;

use super::attribute::{BufferAttribute, zeroed};
use super::geometry::{BufferGeometry, copy_element};

/// `mergeGeometries(geometries, useGroups)`: one geometry holding them all,
/// attributes in the first geometry's order, the indices offset. `None`
/// where three logs an error and returns `null`: geometries that are not
/// all indexed or all not, or whose attribute names or array types differ.
pub fn merge_geometries(
    geometries: &[&BufferGeometry],
    use_groups: bool,
) -> Option<BufferGeometry> {
    let first = geometries.first()?;
    let is_indexed = first.index.is_some();
    let attributes_used: Vec<&str> = first.attributes.iter().map(|(n, _)| n.as_str()).collect();

    // name → the attributes to merge, in the first geometry's order
    let mut attributes: Vec<(&str, Vec<&BufferAttribute>)> =
        attributes_used.iter().map(|&n| (n, Vec::new())).collect();
    let mut merged_geometry = BufferGeometry::new();
    let mut offset = 0;

    for geometry in geometries {
        let mut attributes_count = 0;

        // ensure that all geometries are indexed, or none
        if is_indexed != geometry.index.is_some() {
            return None;
        }

        // gather attributes, exit early if they're different
        for (name, attribute) in &geometry.attributes {
            let slot = attributes.iter_mut().find(|(n, _)| n == name)?;
            slot.1.push(attribute);
            attributes_count += 1;
        }

        // ensure geometries have the same number of attributes
        if attributes_count != attributes_used.len() {
            return None;
        }

        if use_groups {
            let count = if is_indexed {
                geometry.index.as_ref().unwrap().count()
            } else {
                geometry.get_attribute("position")?.count()
            };
            let material_index = merged_geometry.groups.len();
            merged_geometry.add_group(offset, count, material_index);
            offset += count;
        }
    }

    // merge indices
    if is_indexed {
        let mut index_offset = 0u32;
        let mut merged_index: Vec<u32> = Vec::new();
        for g in geometries {
            let index = g.index.as_ref().unwrap();
            for j in 0..index.count() {
                merged_index.push(index.get_x(j) as u32 + index_offset);
            }
            index_offset += g.position().count() as u32;
        }
        merged_geometry.set_index(&merged_index);
    }

    // merge attributes
    for (name, list) in &attributes {
        let merged_attribute = merge_attributes(list)?;
        merged_geometry.set_attribute(name, merged_attribute);
    }

    Some(merged_geometry)
}

/// `mergeAttributes(attributes)`: the arrays concatenated; `None` if their
/// types, item sizes or normalisation differ.
pub fn merge_attributes(attributes: &[&BufferAttribute]) -> Option<BufferAttribute> {
    let first = attributes.first()?;
    let item_size = first.item_size;
    let normalized = first.normalized;
    let mut array_length = 0;
    for attribute in attributes {
        if attribute.array.component() != first.array.component()
            || attribute.item_size != item_size
            || attribute.normalized != normalized
        {
            return None;
        }
        array_length += attribute.count() * item_size;
    }
    let mut array = zeroed(&first.array, array_length);
    let mut offset = 0;
    for attribute in attributes {
        let n = attribute.count() * item_size;
        for k in 0..n {
            copy_element(&mut array, offset + k, &attribute.array, k);
        }
        offset += n;
    }
    Some(BufferAttribute::new(array, item_size, normalized))
}

/// `mergeVertices(geometry, tolerance)` (three's default tolerance 1e-4):
/// vertices whose every attribute agrees after quantising to the tolerance
/// become one, and the result is indexed.
pub fn merge_vertices(geometry: &BufferGeometry, tolerance: f64) -> BufferGeometry {
    let tolerance = js::max(tolerance, f64::EPSILON);

    // Generate an index buffer if the geometry doesn't have one, or optimize it
    // if it's already available. The JS keys an object by the hash string;
    // the key here is the same list of integers.
    let mut hash_to_index: BTreeMap<Vec<i32>, u32> = BTreeMap::new();
    let indices = geometry.index.as_ref();
    let positions = geometry.position();
    let vertex_count = match indices {
        Some(ix) => ix.count(),
        None => positions.count(),
    };

    // next value for triangle indices
    let mut next_index = 0u32;

    // attributes and new attribute arrays
    let mut tmp_attributes: Vec<BufferAttribute> = geometry
        .attributes
        .iter()
        .map(|(_, attr)| {
            BufferAttribute::new(
                attr.zeroed_like(attr.count() * attr.item_size),
                attr.item_size,
                attr.normalized,
            )
        })
        .collect();
    let mut new_indices = Vec::new();

    // convert the error tolerance to an amount of decimal places to truncate to
    let half_tolerance = tolerance * 0.5;
    let exponent = kernel::log10(1.0 / tolerance);
    let hash_multiplier = kernel::pow(10.0, exponent);
    let hash_additive = half_tolerance * hash_multiplier;

    let mut hash = Vec::new();
    for i in 0..vertex_count {
        let index = match indices {
            Some(ix) => ix.get_x(i) as usize,
            None => i,
        };

        // Generate a hash for the vertex attributes at the current index 'i'
        hash.clear();
        for (_, attribute) in &geometry.attributes {
            for k in 0..attribute.item_size.min(4) {
                // double tilde truncates the decimal value
                hash.push(js::to_int32(
                    attribute.get_component(index, k) * hash_multiplier + hash_additive,
                ));
            }
        }

        // Add another reference to the vertex if it's already
        // used by another index
        if let Some(&ix) = hash_to_index.get(&hash) {
            new_indices.push(ix);
        } else {
            // copy data to the new index in the temporary attributes
            for (j, (_, attribute)) in geometry.attributes.iter().enumerate() {
                for k in 0..attribute.item_size.min(4) {
                    let v = attribute.get_component(index, k);
                    tmp_attributes[j].set_component(next_index as usize, k, v);
                }
            }
            hash_to_index.insert(hash.clone(), next_index);
            new_indices.push(next_index);
            next_index += 1;
        }
    }

    // generate result BufferGeometry
    let mut result = geometry.clone();
    for (j, (name, _)) in geometry.attributes.iter().enumerate() {
        let tmp = &tmp_attributes[j];
        let len = next_index as usize * tmp.item_size;
        let array = slice(&tmp.array, len);
        result.set_attribute(
            name,
            BufferAttribute::new(array, tmp.item_size, tmp.normalized),
        );
    }

    // indices
    result.set_index(&new_indices);
    result
}

/// `array.slice(0, len)`.
fn slice(array: &BufferData, len: usize) -> BufferData {
    match array {
        BufferData::F32(a) => BufferData::F32(a[..len].to_vec()),
        BufferData::F64(a) => BufferData::F64(a[..len].to_vec()),
        BufferData::U8(a) => BufferData::U8(a[..len].to_vec()),
        BufferData::U16(a) => BufferData::U16(a[..len].to_vec()),
        BufferData::U32(a) => BufferData::U32(a[..len].to_vec()),
        BufferData::I8(a) => BufferData::I8(a[..len].to_vec()),
        BufferData::I16(a) => BufferData::I16(a[..len].to_vec()),
        BufferData::I32(a) => BufferData::I32(a[..len].to_vec()),
    }
}
