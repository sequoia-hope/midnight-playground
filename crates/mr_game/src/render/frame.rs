//! Bevy's `render_system` without its empty submission (DECISIONS D862).
//!
//! Each frame `bevy_render::renderer::render_system` runs the render graph
//! (whose camera driver submits the frame's command buffers), then makes a
//! second command encoder for screenshot copies and GPU readbacks and
//! submits it whether or not there are any, then presents. On the web every
//! submission is a `GPUQueue.submit` that the browser's GPU process carries
//! out as a queue submission of its own (in Chrome, Dawn's `Queue::Submit`
//! and a `vkQueueSubmit`), which with nothing in it is pure overhead on the
//! process that sets an uncapped WebGPU frame's pace.
//!
//! [`frame`] takes `render_system`'s place: while a screenshot or a GPU
//! readback is in flight (an entity with [`Screenshot`] or [`Readback`] in
//! the main world) it runs Bevy's `render_system` itself, so those work as
//! before; otherwise it runs the render graph and presents, as
//! `render_system` does, and leaves the empty submission out. A Bevy
//! upgrade (0.20, D100) must check this against its `render_system`.

use bevy::camera::NormalizedRenderTarget;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::gpu_readback::Readback;
use bevy::render::renderer::{RenderGraph, render_system};
use bevy::render::view::ViewTarget;
use bevy::render::view::screenshot::Screenshot;
use bevy::render::view::window::ExtractedWindows;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

/// Render world: a screenshot or a GPU readback is in flight.
#[derive(Resource, Default)]
struct CaptureInFlight(bool);

fn extract_capture(
    mut flight: ResMut<CaptureInFlight>,
    shots: Extract<Query<(), With<Screenshot>>>,
    readbacks: Extract<Query<(), With<Readback>>>,
) {
    flight.0 = !shots.is_empty() || !readbacks.is_empty();
}

/// `render_system` (Bevy 0.19.1) less the screenshot and readback
/// submission when there is nothing to capture.
fn frame(world: &mut World, state: &mut SystemState<Query<(&ViewTarget, &ExtractedCamera)>>) {
    if world.resource::<CaptureInFlight>().0 {
        render_system(world, state);
        return;
    }

    world.run_schedule(RenderGraph);

    world.resource_scope(|world, mut windows: Mut<ExtractedWindows>| {
        let views = state.get(world).unwrap();
        for window in windows.values_mut() {
            let view_needs_present = views.iter().any(|(view_target, camera)| {
                matches!(
                    camera.target,
                    Some(NormalizedRenderTarget::Window(w)) if w.entity() == window.entity
                ) && view_target.needs_present()
            });

            if view_needs_present || window.needs_initial_present {
                window.present();
                window.needs_initial_present = false;
            }
        }
    });
}

/// Puts [`frame`] where Bevy's `render_system` was. Called from
/// `ThreeRenderPlugin::finish`, after `RenderPlugin` has added it.
pub fn replace_render_system(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if let Err(e) = render_app.remove_systems_in_set(
        Render,
        render_system,
        bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSetAndSystems,
    ) {
        warn!("render_system not replaced: {e:?}");
        return;
    }
    render_app
        .init_resource::<CaptureInFlight>()
        .add_systems(ExtractSchedule, extract_capture)
        .add_systems(
            Render,
            // After the Render set, which now holds only the pipeline
            // cache's queue processing (`render_system` ran right after it),
            // and before Cleanup.
            frame
                .after(RenderSystems::Render)
                .before(RenderSystems::Cleanup),
        );
}
