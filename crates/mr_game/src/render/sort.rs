//! The transparent pass in three's order (DECISIONS D810). three draws its
//! transparent list sorted by `renderOrder`, then by `z` (the object's
//! sort point through `projScreenMatrix`, far first), then by object id
//! (`reversePainterSortStable`). Bevy sorts its `Transparent3d` items by the
//! sort point's view-space depth plus the material's `depth_bias`, and
//! knows nothing of `renderOrder`. After Bevy's sort this system sorts each
//! view's items again, stably, by three's key:
//!
//! - `renderOrder` first: the node's, as the scene carries it
//!   ([`RenderOrder`], on drawn entities whose order is not 0);
//! - then `z`: in front of the camera three's NDC depth orders as the view
//!   depth does, so Bevy's distance stands (with the material's sort rank,
//!   `ThreeKey::sort_rank`, a centimetre a step); behind the camera the
//!   perspective divide by a negative w puts a sort point past the far
//!   plane, nearer behind first, so such items draw before everything in
//!   front, in that order;
//! - ties keep their order (the stable sort over Bevy's, which keeps the
//!   order the items were queued in, the scene's).

use bevy::core_pipeline::core_3d::{Transparent3d, TransparentSortingInfo3d};
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_phase::{ViewSortedRenderPhases, sort_phase_system};
use bevy::render::sync_world::MainEntity;
use bevy::render::view::ExtractedView;
use bevy::render::{Render, RenderApp, RenderSystems};
use std::collections::HashMap;

/// A drawn object's `renderOrder` (absent: 0).
#[derive(Component, Clone, Copy, Debug, PartialEq, ExtractComponent)]
pub struct RenderOrder(pub f32);

impl RenderOrder {
    /// The component for a node's `renderOrder`, if it is not 0.
    pub fn of(order: f64) -> Option<RenderOrder> {
        (order != 0.0).then_some(RenderOrder(order as f32))
    }
}

/// The sort key of an item whose sort point is `z` along the view (Bevy's
/// distance: negative in front of the camera), `bias` its material's.
pub fn three_key(z: f32, bias: f32) -> f32 {
    if z < 0.0 {
        z + bias
    } else {
        // NDC z = (f + n)/(f − n) + 2fn/((f − n) z): past 1, larger the
        // nearer behind; drawn first.
        -1.0e7 - 1.0e7 / z.max(1.0e-6) + bias
    }
}

fn three_order(
    views: Query<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    orders: Query<(&MainEntity, &RenderOrder)>,
) {
    // A phase item names its main-world entity (its render entity is a
    // placeholder), so the orders are looked up by that.
    let orders: HashMap<Entity, f32> = orders.iter().map(|(m, o)| (m.id(), o.0)).collect();
    let order = |e: &MainEntity| orders.get(&e.id()).copied().unwrap_or(0.0);
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let rangefinder = view.rangefinder3d();
        let mut any_order = false;
        for item in phase.items.values_mut() {
            if let TransparentSortingInfo3d::Sorted {
                mesh_center,
                depth_bias,
            } = item.sorting_info
            {
                item.distance = three_key(rangefinder.distance(&mesh_center), depth_bias);
            }
            any_order |= orders.contains_key(&item.entity.1.id());
        }
        if any_order {
            phase.items.sort_by(|_, a, _, b| {
                order(&a.entity.1)
                    .total_cmp(&order(&b.entity.1))
                    .then(a.distance.total_cmp(&b.distance))
            });
        } else {
            phase
                .items
                .sort_by(|_, a, _, b| a.distance.total_cmp(&b.distance));
        }
    }
}

pub fn plugin(app: &mut App) {
    app.add_plugins(bevy::render::extract_component::ExtractComponentPlugin::<
        RenderOrder,
    >::default());
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(
            Render,
            three_order
                .in_set(RenderSystems::PhaseSort)
                .after(sort_phase_system::<Transparent3d>),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn behind_the_camera_draws_first_nearest_first() {
        let far_front = three_key(-5000.0, 0.0);
        let near_front = three_key(-2.0, 0.0);
        let near_behind = three_key(1.0, 0.0);
        let far_behind = three_key(300.0, 0.0);
        assert!(near_behind < far_behind);
        assert!(far_behind < far_front);
        assert!(far_front < near_front);
        assert_eq!(RenderOrder::of(0.0), None);
        assert_eq!(RenderOrder::of(2.0), Some(RenderOrder(2.0)));
    }
}
