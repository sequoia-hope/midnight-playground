//! The level's top-level scene groups (SPEC 8.6's panel toggles): what a
//! world build puts under its root (`world:<level>`: the terrain, the road,
//! each scenery module's group, the sea, ...) and the other drawn roots
//! (the sky dome). Read from the scene when the loader indexes it
//! (`animate::SceneIndex`), so it is there after the scene's CPU copy goes.
//!
//! The viewer hides a group by moving its entities to a render layer no
//! camera or light draws (`super::apply_groups`), so the animators' own
//! visibility edits stay as they are.

use mp_scene::{NodeType, Scene};

/// No group (a light, the scene's own root).
pub const NONE: u16 = u16::MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Groups {
    /// Each group's name, unique.
    pub names: Vec<String>,
    /// Per scene node, its group (or [`NONE`]).
    pub of_node: Vec<u16>,
}

fn drawn(t: NodeType) -> bool {
    matches!(
        t,
        NodeType::Mesh
            | NodeType::InstancedMesh
            | NodeType::Points
            | NodeType::Line
            | NodeType::LineSegments
            | NodeType::LineLoop
            | NodeType::Sprite
    )
}

/// The groups of a scene.
pub fn scene_groups(scene: &Scene) -> Groups {
    let n = scene.nodes.len();
    let mut g = Groups {
        names: Vec::new(),
        of_node: vec![NONE; n],
    };
    let mut tops: Vec<u32> = Vec::new();
    for &r in &scene.roots {
        let node = &scene.nodes[r as usize];
        if node.name.starts_with("world:") {
            tops.extend(node.children.iter().copied());
        } else {
            tops.push(r);
        }
    }
    for top in tops {
        // The subtree, and whether it draws anything.
        let mut stack = vec![top];
        let mut members = Vec::new();
        let mut kind = None;
        while let Some(i) = stack.pop() {
            let Some(node) = scene.nodes.get(i as usize) else {
                continue;
            };
            if drawn(node.ty) && kind.is_none() {
                kind = Some(
                    node.materials
                        .first()
                        .and_then(|m| scene.materials.get(*m as usize))
                        .map(|m| words(&format!("{:?}", m.kind)))
                        .unwrap_or_else(|| words(&format!("{:?}", node.ty))),
                );
            }
            members.push(i);
            stack.extend(node.children.iter().copied());
        }
        let Some(kind) = kind else { continue };
        if g.names.len() >= NONE as usize {
            continue;
        }
        let node = &scene.nodes[top as usize];
        // An unnamed group is named for what it draws (the sky dome, the
        // sea).
        let base = if node.name.is_empty() {
            kind
        } else {
            node.name.clone()
        };
        let mut name = base.clone();
        let mut k = 2;
        while g.names.contains(&name) {
            name = format!("{base} {k}");
            k += 1;
        }
        let id = g.names.len() as u16;
        g.names.push(name);
        for m in members {
            g.of_node[m as usize] = id;
        }
    }
    g
}

/// `SkyDome` → `sky dome`.
fn words(camel: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(c.to_lowercase());
    }
    out
}

impl Groups {
    pub fn group_of(&self, node: u32) -> Option<usize> {
        self.of_node
            .get(node as usize)
            .filter(|g| **g != NONE)
            .map(|g| *g as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_read_as_words() {
        assert_eq!(words("SkyDome"), "sky dome");
        assert_eq!(words("Sea"), "sea");
    }
}
