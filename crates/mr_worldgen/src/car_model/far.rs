//! The far LOD of `CarModel.js` (`:2304-2368`).
//!
//! Traffic a hundred metres off is a few dozen pixels wide, yet each car
//! was ~15 draw calls (every material bucket, two per wheel), and a
//! freeway keeps 30-40 of them in view. The far version keeps the paint and
//! every light (so brake lights, headlights and sirens still switch per
//! car) and bakes the rest (trim, glass, plates, chrome, parked wheels)
//! into one mesh coloured per vertex and shared by every car of the kind: 4
//! draw calls for most traffic. `handle.setFar(true)` swaps it in.

use super::{CarKit, Far, VehicleModel};
use crate::material::Material;
use crate::object::{NodeId, SceneGraph};
use crate::three_geom::{BufferAttribute, BufferGeometry, Group, Matrix4, merge_geometries};

/// `FAR_OWN`: what the far model leaves to the car's own meshes.
const FAR_OWN: [&str; 8] = [
    "paint",
    "stripe",
    "head",
    "tail",
    "accent",
    "lightRed",
    "lightBlue",
    "sirenGlow",
];

/// `root.updateMatrixWorld(true)` for a root with no parent: each node's
/// world matrix (`parent.matrixWorld × matrix`).
fn world_of(graph: &SceneGraph, root: NodeId, id: NodeId) -> Matrix4 {
    let mut chain = vec![id];
    let mut at = id;
    while at != root {
        at = graph.get(at).parent.expect("under the root");
        chain.push(at);
    }
    let mut world: Option<Matrix4> = None;
    for &n in chain.iter().rev() {
        let o = graph.get(n);
        let m = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        world = Some(match world {
            Some(p) => Matrix4::multiply_matrices(&p, &m),
            None => m,
        });
    }
    world.expect("a node")
}

fn is_mesh(graph: &SceneGraph, o: NodeId) -> bool {
    graph.get(o).ty.is_mesh()
}

/// `farGeometry(handle)`.
fn far_geometry(graph: &SceneGraph, h: &VehicleModel) -> BufferGeometry {
    let to_root = world_of(graph, h.root, h.root).invert();
    let mut parts: Vec<BufferGeometry> = Vec::new();
    let mut add = |o: NodeId| {
        let node = graph.get(o);
        let mats = &node.materials;
        let g = graph.geometry(node.geometry.expect("a mesh"));
        let g = if g.index.is_some() {
            g.to_non_indexed()
        } else {
            g.clone()
        };
        let n = g.position().count();
        let mut col = vec![0f32; n * 3];
        let groups: Vec<Group> = if node.multi_material && !g.groups.is_empty() {
            g.groups.clone()
        } else {
            vec![Group {
                start: 0,
                count: n,
                material_index: 0,
            }]
        };
        for gr in groups {
            let c = graph
                .material(mats[gr.material_index])
                .color("color")
                .expect("a material with a colour");
            for i in gr.start..n.min(gr.start + gr.count) {
                col[i * 3] = c.r as f32;
                col[i * 3 + 1] = c.g as f32;
                col[i * 3 + 2] = c.b as f32;
            }
        }
        let mut out = BufferGeometry::new();
        out.set_attribute("position", g.position().clone());
        out.set_attribute(
            "normal",
            g.get_attribute("normal").expect("normals").clone(),
        );
        out.set_attribute("color", BufferAttribute::from_f32(col, 3));
        let m = Matrix4::multiply_matrices(&to_root, &world_of(graph, h.root, o));
        out.apply_matrix4(&m);
        parts.push(out);
    };
    for &o in &graph.get(h.body).children {
        if is_mesh(graph, o) && !FAR_OWN.contains(&graph.get(o).name.as_str()) {
            add(o);
        }
    }
    for &w in &h.wheels {
        // `w.traverse`: the wheel and everything under it, depth first.
        let mut stack = vec![w];
        while let Some(o) = stack.pop() {
            if is_mesh(graph, o) {
                add(o);
            }
            stack.extend(graph.get(o).children.iter().rev());
        }
    }
    let refs: Vec<&BufferGeometry> = parts.iter().collect();
    let mut geo = merge_geometries(&refs, false).expect("the far parts merge");
    geo.compute_bounding_sphere();
    geo
}

/// `addFarLod(handle, key)`.
pub(super) fn add_far_lod(
    kit: &mut CarKit,
    graph: &mut SceneGraph,
    h: &mut VehicleModel,
    key: &str,
) {
    let geo = match kit.far.iter().find(|(k, _)| k == key) {
        Some(&(_, g)) => g,
        None => {
            let g = graph.add_geometry(far_geometry(graph, h));
            kit.far.push((key.to_string(), g));
            g
        }
    };
    let mat = *kit.far_material.get_or_insert_with(|| {
        graph.add_material(
            Material::standard()
                .set("vertexColors", true)
                .set("metalness", 0.3)
                .set("roughness", 0.5),
        )
    });
    let far = graph.mesh(geo, mat);
    {
        let o = graph.get_mut(far);
        o.name = "far".into();
        o.visible = false;
    }
    // Everything the far mesh stands in for: the baked body buckets, and
    // the wheels on their pivots.
    let mut near: Vec<NodeId> = graph
        .get(h.body)
        .children
        .iter()
        .copied()
        .filter(|&o| is_mesh(graph, o) && !FAR_OWN.contains(&graph.get(o).name.as_str()))
        .collect();
    near.extend(
        graph
            .get(h.root)
            .children
            .iter()
            .copied()
            .filter(|&o| o != h.body),
    );
    graph.add(h.body, far);
    h.far = Some(Far { mesh: far, near });
}
