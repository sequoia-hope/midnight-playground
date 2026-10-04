//! `InstancedMesh` as three draws it (DECISIONS D450 to D453): one entity
//! per InstancedMesh (per material group), one draw call with its instance
//! count, the per-instance data in a vertex buffer stepped per instance.
//!
//! The entity carries [`Instances`]: its instances' world matrices (the
//! InstancedMesh's `matrixWorld` × `instanceMatrix`, multiplied in f64) and
//! colours (`instanceColor`, or white) and the node's `receiveShadow`, 80
//! bytes an instance, uploaded once and then dropped from the CPU. Its
//! material is a [`ThreeMaterial`](super::ThreeMaterial) whose key has
//! `instanced` set, which adds the instance buffer to the pipeline's vertex
//! layout (shader locations 9 to 13) and `MR_INSTANCED` to the shader defs:
//! `three_material.wgsl` reads the matrix from there instead of Bevy's mesh
//! uniform, and the shadow pass uses `three_prepass_instanced.wgsl`.
//!
//! Bevy's material pipeline draws a mesh entity with `DrawMesh`, one
//! instance (or a batch of entities). [`DrawThreeMesh`] is `DrawMesh` with
//! one difference: for an entity that has an instance stream it binds the
//! stream at vertex slot 1 and draws the mesh once with the stream's
//! instance count. Every other entity goes to `DrawMesh` as before. It
//! takes the place of `DrawMesh` in Bevy's `DrawMaterial`, `DrawPrepass` and
//! `DrawDepthOnlyPrepass` (the draw functions Bevy's `MaterialPlugin` gives
//! every material for the main pass and the shadow pass), so the main pass,
//! the transparent sort and the shadow map handle these entities like any
//! other mesh: culled by their `Aabb` (three's bounding sphere over all the
//! instances), sorted by its centre, specialised and warmed up as usual.
//! The entities are `NoAutomaticBatching`, since each has its own stream.

use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transparent3d};
use bevy::core_pipeline::prepass::{AlphaMask3dPrepass, Opaque3dPrepass};
use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::math::{DMat4, DVec3};
use bevy::mesh::VertexBufferLayout;
use bevy::pbr::{
    DrawDepthOnlyPrepass, DrawMaterial, DrawMesh, DrawPrepass, RenderMeshInstances,
    SetMaterialBindGroup, SetMeshBindGroup, SetMeshViewBindGroup, SetMeshViewBindingArrayBindGroup,
    SetPrepassEmptyMaterialBindGroup, SetPrepassViewBindGroup, SetPrepassViewEmptyBindGroup,
    Shadow,
};
use bevy::prelude::*;
use bevy::render::mesh::allocator::MeshAllocator;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_phase::{
    DrawFunctions, PhaseItem, RenderCommand, RenderCommandResult, RenderCommandState,
    SetItemPipeline, TrackedRenderPass,
};
use bevy::render::render_resource::{
    Buffer, BufferInitDescriptor, BufferUsages, VertexAttribute, VertexFormat, VertexStepMode,
};
use bevy::render::renderer::RenderDevice;
use bevy::render::sync_world::{MainEntity, MainEntityHashMap};
use bevy::render::{Extract, ExtractSchedule, RenderApp};
use std::sync::{Arc, Mutex, OnceLock};

/// Floats per instance: the matrix's four columns, then the colour (rgb)
/// and `receiveShadow` (1 or 0), then the geometry's instance-rate
/// attributes, one float each in their order (the harbour containers'
/// `aVar`, the desert pools' `ph` and `fl`; zeros without; D499).
pub const INSTANCE_FLOATS: usize = 24;
/// The shader location of the instance-rate attributes (after the stream's
/// 9 to 13; 14 is the traffic streams' second attribute).
pub const INSTANCE_EXTRA_LOCATION: u32 = 15;
/// The first shader location of the instance stream (Bevy's standard
/// attributes use 0 to 7, the patch attribute 8).
pub const INSTANCE_LOCATION: u32 = 9;

/// The instance stream's vertex layout: the matrix columns at locations 9
/// to 12, the colour and shadow flag at 13, the instance-rate attributes at
/// 15. The shadow pass reads only the matrix (`with_color` false).
pub fn instance_layout(with_color: bool) -> VertexBufferLayout {
    let n = if with_color { 6 } else { 4 };
    VertexBufferLayout {
        array_stride: (INSTANCE_FLOATS * 4) as u64,
        step_mode: VertexStepMode::Instance,
        attributes: (0..n)
            .map(|i| VertexAttribute {
                format: VertexFormat::Float32x4,
                offset: u64::from(i) * 16,
                shader_location: if i == 5 {
                    INSTANCE_EXTRA_LOCATION
                } else {
                    INSTANCE_LOCATION + i
                },
            })
            .collect(),
    }
}

/// One InstancedMesh's instances, shared by the entities of its material
/// groups. The bytes go to the GPU the first time it is extracted, then
/// are dropped.
pub struct InstanceStream {
    count: u32,
    bytes: Mutex<Option<Vec<u8>>>,
    buffer: OnceLock<Buffer>,
    /// New contents of the same size, written into the buffer in place
    /// (`write_instance_updates`): the scenery's animators move instances
    /// every frame, and a new buffer each time would be a GPU allocation
    /// per frame (WP 3.9, D497).
    update: Mutex<Option<Vec<u8>>>,
}

impl InstanceStream {
    /// From [`INSTANCE_FLOATS`] floats per instance.
    pub fn new(data: &[f32]) -> InstanceStream {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for x in data {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        InstanceStream {
            count: (data.len() / INSTANCE_FLOATS) as u32,
            bytes: Mutex::new(Some(bytes)),
            buffer: OnceLock::new(),
            update: Mutex::new(None),
        }
    }

    /// New contents for the stream, the same number of instances, to be
    /// written in place; false (nothing done) if the count differs.
    pub fn update(&self, data: &[f32]) -> bool {
        if (data.len() / INSTANCE_FLOATS) as u32 != self.count {
            return false;
        }
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for x in data {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        if self.buffer.get().is_none()
            && let Ok(mut b) = self.bytes.lock()
            && b.is_some()
        {
            // Not on the GPU yet: it goes up with these contents.
            *b = Some(bytes);
            return true;
        }
        if let Ok(mut u) = self.update.lock() {
            *u = Some(bytes);
        }
        true
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    fn buffer(&self, device: &RenderDevice) -> &Buffer {
        self.buffer.get_or_init(|| {
            let bytes = self
                .bytes
                .lock()
                .ok()
                .and_then(|mut b| b.take())
                .unwrap_or_default();
            device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("mr_instances"),
                contents: &bytes,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            })
        })
    }
}

/// Writes the streams' pending updates into their buffers (render world,
/// at extraction).
pub fn write_instance_updates(
    queue: Res<bevy::render::renderer::RenderQueue>,
    q: Extract<Query<&Instances>>,
) {
    for inst in &q {
        let Some(buffer) = inst.0.buffer.get() else {
            continue;
        };
        let Some(bytes) = inst.0.update.lock().ok().and_then(|mut u| u.take()) else {
            continue;
        };
        queue.write_buffer(buffer, 0, &bytes);
    }
}

/// The instance stream of an InstancedMesh's entity.
#[derive(Component, Clone)]
pub struct Instances(pub Arc<InstanceStream>);

/// Appends one instance: `m` (world from local) and its colour.
pub fn push_instance(out: &mut Vec<f32>, m: &DMat4, color: [f32; 3], receive: bool) {
    out.extend(m.to_cols_array().iter().map(|&x| x as f32));
    out.extend_from_slice(&color);
    out.push(if receive { 1.0 } else { 0.0 });
    out.extend_from_slice(&[0.0; 4]);
}

/// Sets the instance-rate attributes of the instance [`push_instance`]
/// appended last.
pub fn set_instance_extra(out: &mut [f32], extra: [f32; 4]) {
    let n = out.len();
    if n >= 4 {
        out[n - 4..].copy_from_slice(&extra);
    }
}

/// three's `Sphere.applyMatrix4`: the centre transformed, the radius
/// scaled by `getMaxScaleOnAxis`.
pub fn sphere_to_world(m: &DMat4, centre: DVec3, radius: f64) -> (DVec3, f64) {
    let sx = m.x_axis.truncate().length_squared();
    let sy = m.y_axis.truncate().length_squared();
    let sz = m.z_axis.truncate().length_squared();
    (
        m.transform_point3(centre),
        radius * sx.max(sy).max(sz).sqrt(),
    )
}

/// The render world's view of the streams, by main-world entity (Bevy's
/// mesh phase items name their entity by its main-world id).
#[derive(Resource, Default)]
pub struct InstanceStreams(MainEntityHashMap<(Buffer, u32)>);

fn extract_instances(
    mut streams: ResMut<InstanceStreams>,
    device: Res<RenderDevice>,
    q: Extract<Query<(Entity, &Instances)>>,
) {
    streams.0.clear();
    for (e, inst) in &q {
        let buffer = inst.0.buffer(&device).clone();
        streams
            .0
            .insert(MainEntity::from(e), (buffer, inst.0.count()));
    }
}

/// Bevy's `DrawMesh`, drawing an entity with an instance stream once with
/// the stream's instance count (three's `renderBufferDirect` with
/// `object.isInstancedMesh`: `renderer.renderInstances(start, count,
/// object.count)`).
pub struct DrawThreeMesh;

impl<P: PhaseItem> RenderCommand<P> for DrawThreeMesh {
    type Param = (SRes<InstanceStreams>, <DrawMesh as RenderCommand<P>>::Param);
    type ViewQuery = <DrawMesh as RenderCommand<P>>::ViewQuery;
    type ItemQuery = ();

    #[inline]
    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _entity: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (streams, inner): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some((stream, instances)) = streams.into_inner().0.get(&item.main_entity()) else {
            return DrawMesh::render(item, view, None, inner, pass);
        };
        let (meshes, mesh_instances, _, _, mesh_allocator, _, _) = inner;
        let meshes: &'w RenderAssets<RenderMesh> = meshes.into_inner();
        let mesh_instances: &'w RenderMeshInstances = mesh_instances.into_inner();
        let mesh_allocator: &'w MeshAllocator = mesh_allocator.into_inner();
        let Some(mesh_asset_id) = mesh_instances.mesh_asset_id(item.main_entity()) else {
            return RenderCommandResult::Skip;
        };
        let Some(gpu_mesh) = meshes.get(mesh_asset_id) else {
            return RenderCommandResult::Skip;
        };
        let Some(vertex) = mesh_allocator.mesh_vertex_slice(&mesh_asset_id) else {
            return RenderCommandResult::Skip;
        };
        pass.set_vertex_buffer(0, vertex.buffer.slice(..));
        pass.set_vertex_buffer(1, stream.slice(..));
        match &gpu_mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                index_format,
                count,
            } => {
                let Some(index) = mesh_allocator.mesh_index_slice(&mesh_asset_id) else {
                    return RenderCommandResult::Skip;
                };
                pass.set_index_buffer(index.buffer.slice(..), *index_format);
                pass.draw_indexed(
                    index.range.start..(index.range.start + *count),
                    vertex.range.start as i32,
                    0..*instances,
                );
            }
            RenderMeshBufferInfo::NonIndexed => {
                pass.draw(vertex.range, 0..*instances);
            }
        }
        RenderCommandResult::Success
    }
}

/// Bevy 0.19.1's `DrawMaterial` with [`DrawThreeMesh`] for `DrawMesh`.
type DrawThreeMaterial = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetMeshViewBindingArrayBindGroup<1>,
    SetMeshBindGroup<2>,
    SetMaterialBindGroup<3>,
    DrawThreeMesh,
);

/// Bevy 0.19.1's `DrawPrepass` with [`DrawThreeMesh`].
type DrawThreePrepass = (
    SetItemPipeline,
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    SetMeshBindGroup<2>,
    SetMaterialBindGroup<3>,
    DrawThreeMesh,
);

/// Bevy 0.19.1's `DrawDepthOnlyPrepass` with [`DrawThreeMesh`].
type DrawThreeDepthOnly = (
    SetItemPipeline,
    SetPrepassViewBindGroup<0>,
    SetPrepassViewEmptyBindGroup<1>,
    SetMeshBindGroup<2>,
    SetPrepassEmptyMaterialBindGroup<3>,
    DrawThreeMesh,
);

/// Makes `T`'s id in `P`'s draw functions name `C` instead: every material
/// prepared from then on (`MaterialPlugin` looks its draw functions up by
/// these ids) draws with `C`.
fn replace<P: PhaseItem, T: 'static, C>(world: &mut World)
where
    C: RenderCommand<P> + Send + Sync + 'static,
    C::Param: bevy::ecs::system::ReadOnlySystemParam,
{
    if !world.contains_resource::<DrawFunctions<P>>() {
        return;
    }
    let state = RenderCommandState::<P, C>::new(world);
    world
        .resource::<DrawFunctions<P>>()
        .write()
        .add_with::<T, _>(state);
}

pub struct InstancingPlugin;

impl Plugin for InstancingPlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<InstanceStreams>()
            .add_systems(ExtractSchedule, extract_instances);
        let w = render_app.world_mut();
        replace::<Opaque3d, DrawMaterial, DrawThreeMaterial>(w);
        replace::<AlphaMask3d, DrawMaterial, DrawThreeMaterial>(w);
        replace::<Transparent3d, DrawMaterial, DrawThreeMaterial>(w);
        replace::<Shadow, DrawPrepass, DrawThreePrepass>(w);
        replace::<Shadow, DrawDepthOnlyPrepass, DrawThreeDepthOnly>(w);
        replace::<Opaque3dPrepass, DrawPrepass, DrawThreePrepass>(w);
        replace::<AlphaMask3dPrepass, DrawPrepass, DrawThreePrepass>(w);
        replace::<Opaque3dPrepass, DrawDepthOnlyPrepass, DrawThreeDepthOnly>(w);
    }
}
