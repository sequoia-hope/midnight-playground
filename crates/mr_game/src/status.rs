//! What the client reports about itself: the page's loading bar and
//! `window.__mr` on the web (the start of SPEC 8.5's test bridge), the window
//! title and the log natively.

use crate::loader::Counts;
use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Resource, Default, Debug, Clone)]
pub struct Status {
    /// `waiting`, `building`, `running` or `failed`.
    pub state: &'static str,
    /// Build progress, 0 to 1.
    pub progress: f32,
    pub error: Option<String>,
    /// Frames rendered since start.
    pub frames: u64,
    /// Frames rendered since the scene came up.
    pub scene_frames: u64,
    /// Smoothed frame time, ms.
    pub frame_ms: f32,
    /// Worst frame since the last report, ms.
    pub worst_ms: f32,
    pub counts: Option<Counts>,
    /// The fly camera's s, for the title and the page.
    pub s: f64,
    /// Gestures the page has passed in (the bridge stub, SPEC 8.4).
    pub gestures: u32,
    /// Render pipelines still compiling (natively they compile in the
    /// background, and a mesh draws only once its pipeline is ready).
    pub pipelines_waiting: usize,
    /// The scene is up and every pipeline it needs is compiled.
    pub ready: bool,
    /// Frames since `ready`.
    pub ready_frames: u64,
}

/// Pipelines waiting in the render world's cache, published each frame.
pub static PIPELINES_WAITING: AtomicUsize = AtomicUsize::new(0);

/// Render world: how many pipelines are still compiling.
pub fn count_pipelines(cache: Res<PipelineCache>) {
    PIPELINES_WAITING.store(cache.waiting_pipelines().count(), Ordering::Relaxed);
}

impl Status {
    pub fn fail(&mut self, e: impl Into<String>) {
        let e = e.into();
        error!("{e}");
        self.error = Some(e);
        self.state = "failed";
    }
}

/// Counts frames and keeps a smoothed frame time.
pub fn tick(time: Res<Time>, mut status: ResMut<Status>) {
    status.frames += 1;
    status.pipelines_waiting = PIPELINES_WAITING.load(Ordering::Relaxed);
    if status.state == "running" {
        status.scene_frames += 1;
        // Ready once the first frames have queued their pipelines and the
        // queue has drained.
        if !status.ready && status.scene_frames > 10 && status.pipelines_waiting == 0 {
            status.ready = true;
            info!("ready: scene up and every pipeline compiled");
        }
    }
    if status.ready {
        status.ready_frames += 1;
    }
    let ms = time.delta_secs() * 1000.0;
    status.frame_ms = if status.frame_ms == 0.0 {
        ms
    } else {
        status.frame_ms * 0.95 + ms * 0.05
    };
    status.worst_ms = status.worst_ms.max(ms);
}
