//! Pipeline warm-up (SPEC 6.3, roadmap WP 2.6). On the web, Bevy creates a
//! render pipeline synchronously the first time a material and mesh layout
//! is drawn, which costs tens to hundreds of milliseconds a pipeline: a
//! hitch whenever the camera first sees a combination. So while the loading
//! screen is still up, the client draws one degenerate triangle with every
//! material × mesh-layout combination the scene uses, off screen, in the
//! main pass and (for shadow casters) the shadow pass, until every pipeline
//! has compiled; then the stand-ins are despawned and the scene is shown.
//!
//! Combinations are the pipeline's inputs: the material's [`ThreeKey`]
//! (shader defs, blending, culling, depth state), the mesh's vertex
//! attributes and topology, and whether it casts a shadow. Materials with
//! the same key share one pipeline, so one stand-in covers them all.

use crate::render::ThreeMaterial;
use crate::render::material::ThreeKey;
use crate::status::Status;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{
    Mesh, MeshVertexAttribute, MeshVertexAttributeId, PrimitiveTopology, VertexAttributeValues,
};
use bevy::prelude::*;
use bevy::render::render_resource::VertexFormat;
use std::collections::HashMap;

/// A mesh's vertex layout, as far as it picks a pipeline.
#[derive(Clone, Debug)]
pub struct Layout {
    pub topology: PrimitiveTopology,
    pub attributes: Vec<MeshVertexAttribute>,
}

impl Layout {
    pub fn of(mesh: &Mesh) -> Layout {
        Layout {
            topology: mesh.primitive_topology(),
            attributes: mesh.attributes().map(|(a, _)| *a).collect(),
        }
    }

    fn signature(&self) -> Signature {
        let mut ids: Vec<(MeshVertexAttributeId, VertexFormat)> =
            self.attributes.iter().map(|a| (a.id, a.format)).collect();
        ids.sort_by_key(|x| x.0);
        (self.topology, ids)
    }

    /// Six vertices of zeros with the same attributes: two triangles (or
    /// three lines) of no size, which rasterise nothing.
    pub fn degenerate(&self) -> Mesh {
        let mut mesh = Mesh::new(self.topology, RenderAssetUsages::RENDER_WORLD);
        for a in &self.attributes {
            if let Some(v) = zeros(a.format, 6) {
                mesh.insert_attribute(*a, v);
            }
        }
        mesh
    }
}

fn zeros(format: VertexFormat, n: usize) -> Option<VertexAttributeValues> {
    Some(match format {
        VertexFormat::Float32 => VertexAttributeValues::Float32(vec![0.0; n]),
        VertexFormat::Float32x2 => VertexAttributeValues::Float32x2(vec![[0.0; 2]; n]),
        VertexFormat::Float32x3 => VertexAttributeValues::Float32x3(vec![[0.0; 3]; n]),
        VertexFormat::Float32x4 => VertexAttributeValues::Float32x4(vec![[0.0; 4]; n]),
        VertexFormat::Uint32 => VertexAttributeValues::Uint32(vec![0; n]),
        VertexFormat::Uint16x4 => VertexAttributeValues::Uint16x4(vec![[0; 4]; n]),
        VertexFormat::Unorm8x4 => VertexAttributeValues::Unorm8x4(vec![[0; 4]; n]),
        _ => return None,
    })
}

type Signature = (
    PrimitiveTopology,
    Vec<(MeshVertexAttributeId, VertexFormat)>,
);
type Key = (ThreeKey, Signature, bool);

/// The combinations a scene draws, gathered while it is built.
#[derive(Default)]
pub struct Combos {
    seen: HashMap<Key, usize>,
    list: Vec<(Handle<ThreeMaterial>, Layout, bool)>,
}

impl Combos {
    /// Notes that `material` (whose key is `key`) is drawn on a mesh of
    /// `layout`, casting a shadow or not.
    pub fn note(
        &mut self,
        key: ThreeKey,
        material: &Handle<ThreeMaterial>,
        layout: &Layout,
        casts: bool,
    ) {
        let k = (key, layout.signature(), casts);
        if self.seen.contains_key(&k) {
            return;
        }
        self.seen.insert(k, self.list.len());
        self.list.push((material.clone(), layout.clone(), casts));
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Spawns one stand-in per combination. They sit far outside every
    /// view (10,000 km below the origin: past the far plane and the shadow
    /// box whatever the camera does) and are never culled, so each is
    /// specialised for the main view and the shadow view.
    pub fn spawn(&self, commands: &mut Commands, meshes: &mut Assets<Mesh>) -> usize {
        let mut cache: HashMap<Signature, Handle<Mesh>> = HashMap::new();
        for (material, layout, casts) in &self.list {
            let mesh = cache
                .entry(layout.signature())
                .or_insert_with(|| meshes.add(layout.degenerate()))
                .clone();
            let mut e = commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material.clone()),
                Transform::from_xyz(0.0, -1.0e7, 0.0),
                NoFrustumCulling,
                WarmUp,
                crate::loader::SceneEntity,
                Name::new("warm-up"),
            ));
            if !casts {
                e.insert(NotShadowCaster);
            }
        }
        self.list.len()
    }
}

/// A warm-up stand-in.
#[derive(Component)]
pub struct WarmUp;

/// Once every pipeline has compiled, the stand-ins go.
pub fn end_warm_up(mut commands: Commands, status: Res<Status>, q: Query<Entity, With<WarmUp>>) {
    if !status.ready || q.is_empty() {
        return;
    }
    let mut n = 0;
    for e in &q {
        commands.entity(e).despawn();
        n += 1;
    }
    info!("warm-up done: {n} stand-ins removed");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(colors: bool) -> Layout {
        let mut m = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 3]);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; 3]);
        if colors {
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; 3]);
        }
        Layout::of(&m)
    }

    #[test]
    fn one_stand_in_per_pipeline() {
        let mut c = Combos::default();
        let a = Handle::<ThreeMaterial>::default();
        let key = ThreeKey::default();
        let fog = ThreeKey {
            fog: true,
            ..ThreeKey::default()
        };
        c.note(key, &a, &layout(false), true);
        // Another material with the same key on the same layout: the same
        // pipeline.
        c.note(key, &a, &layout(false), true);
        assert_eq!(c.len(), 1);
        c.note(key, &a, &layout(true), true);
        c.note(fog, &a, &layout(false), true);
        c.note(key, &a, &layout(false), false);
        assert_eq!(c.len(), 4);
    }

    #[test]
    fn degenerate_mesh_keeps_the_layout() {
        let l = layout(true);
        let m = l.degenerate();
        assert_eq!(m.count_vertices(), 6);
        assert_eq!(Layout::of(&m).signature(), l.signature());
    }
}
