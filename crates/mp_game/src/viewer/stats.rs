//! Draw calls and triangles for the viewer's readout (SPEC 8.6), counted
//! in the render world as three's `renderer.info.render` counts them: one
//! call per drawn object, an instanced mesh's triangles times its instance
//! count. A drawn object is a mesh entity visible from a view: the camera
//! (`main`) or the sun's shadow map (`shadow`; three draws the shadow pass
//! with the same renderer, so its figures include it). Bevy may batch some
//! of these into fewer GPU draws; the count is the objects, which is what
//! SPEC 6.6's budgets were measured in. The post chain's passes are not
//! counted.
//!
//! Only while the viewer's readout is shown (`COUNTING`): the walk over the
//! visible lists is CPU time, and the lists are reused, not reallocated.

use crate::render::instancing::Instances;
use bevy::pbr::RenderMeshInstances;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::mesh::{RenderMesh, RenderMeshBufferInfo};
use bevy::render::render_asset::RenderAssets;
use bevy::render::sync_world::{MainEntity, MainEntityHashMap};
use bevy::render::view::{RenderShadowMapVisibleEntities, RenderVisibleEntities};
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// The readout wants the counts.
pub static COUNTING: AtomicBool = AtomicBool::new(false);
static MAIN_DRAWS: AtomicU64 = AtomicU64::new(0);
static MAIN_TRIS: AtomicU64 = AtomicU64::new(0);
static SHADOW_DRAWS: AtomicU64 = AtomicU64::new(0);
static SHADOW_TRIS: AtomicU64 = AtomicU64::new(0);

/// The last frame's counts.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub draws: u64,
    pub tris: u64,
    pub shadow_draws: u64,
    pub shadow_tris: u64,
}

pub fn counts() -> Counts {
    Counts {
        draws: MAIN_DRAWS.load(Ordering::Relaxed),
        tris: MAIN_TRIS.load(Ordering::Relaxed),
        shadow_draws: SHADOW_DRAWS.load(Ordering::Relaxed),
        shadow_tris: SHADOW_TRIS.load(Ordering::Relaxed),
    }
}

/// Instance counts of the instanced meshes, by main-world entity.
#[derive(Resource, Default)]
struct InstanceCounts(MainEntityHashMap<u32>);

fn extract_instances(mut counts: ResMut<InstanceCounts>, q: Extract<Query<(Entity, &Instances)>>) {
    counts.0.clear();
    if !COUNTING.load(Ordering::Relaxed) {
        return;
    }
    for (e, i) in &q {
        counts.0.insert(MainEntity::from(e), i.0.count());
    }
}

fn tally(
    list: &RenderVisibleEntities,
    meshes: &RenderMeshInstances,
    assets: &RenderAssets<RenderMesh>,
    instances: &InstanceCounts,
) -> (u64, u64) {
    let Some(class) = list.get::<Mesh3d>() else {
        return (0, 0);
    };
    let mut draws = 0;
    let mut tris = 0;
    let all = class
        .entities_cpu_culling
        .iter()
        .map(|(_, m)| *m)
        .chain(class.entities_gpu_culling.keys().copied());
    for main in all {
        let Some(mesh) = meshes.mesh_asset_id(main).and_then(|id| assets.get(id)) else {
            continue;
        };
        let per = match mesh.buffer_info {
            RenderMeshBufferInfo::Indexed { count, .. } => u64::from(count) / 3,
            RenderMeshBufferInfo::NonIndexed => u64::from(mesh.vertex_count) / 3,
        };
        let n = instances.0.get(&main).copied().unwrap_or(1);
        draws += 1;
        tris += per * u64::from(n);
    }
    (draws, tris)
}

fn count(
    views: Query<&RenderVisibleEntities, With<ExtractedCamera>>,
    lights: Query<&RenderShadowMapVisibleEntities>,
    meshes: Option<Res<RenderMeshInstances>>,
    assets: Option<Res<RenderAssets<RenderMesh>>>,
    instances: Res<InstanceCounts>,
) {
    if !COUNTING.load(Ordering::Relaxed) {
        return;
    }
    let (Some(meshes), Some(assets)) = (meshes, assets) else {
        return;
    };
    let (mut d, mut t) = (0, 0);
    for v in &views {
        let (a, b) = tally(v, &meshes, &assets, &instances);
        d += a;
        t += b;
    }
    let (mut sd, mut st) = (0, 0);
    for l in &lights {
        for v in l.subviews.values() {
            let (a, b) = tally(v, &meshes, &assets, &instances);
            sd += a;
            st += b;
        }
    }
    MAIN_DRAWS.store(d, Ordering::Relaxed);
    MAIN_TRIS.store(t, Ordering::Relaxed);
    SHADOW_DRAWS.store(sd, Ordering::Relaxed);
    SHADOW_TRIS.store(st, Ordering::Relaxed);
}

pub fn plugin(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app
        .init_resource::<InstanceCounts>()
        .add_systems(ExtractSchedule, extract_instances)
        .add_systems(Render, count.in_set(RenderSystems::PrepareResources));
}
