//! What the client reports about itself: the page's loading bar and
//! `window.__mr` on the web (the start of SPEC 8.5's test bridge), the window
//! title and the log natively.

use crate::loader::Counts;
use bevy::prelude::*;
use bevy::render::render_resource::PipelineCache;
use bevy::render::renderer::RenderQueue;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

#[derive(Resource, Default, Debug, Clone)]
pub struct Status {
    /// `waiting`, `building`, `warming` (the scene is up, its pipelines
    /// compiling behind the loading screen: `crate::warmup`), `running` or
    /// `failed`.
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
    /// Warm-up stand-ins spawned for the scene (material × mesh-layout
    /// combinations).
    pub warm_up: usize,
    /// Scenes loaded since start (a reload adds one).
    pub scenes: u32,
    /// The fly camera's route, once the Track is known: its length, where
    /// the road ends, and whether it loops (for the measurement page).
    pub route: Option<(f64, f64, bool)>,
    /// The GPU fence the warm-up waits on (`gpu_fence`).
    pub fence: Option<u32>,
    /// Frames since `ready` with a render pipeline still compiling (the
    /// warm-up missed one; zero when it works).
    pub late_frames: u64,
}

/// Pipelines waiting in the render world's cache, published each frame.
pub static PIPELINES_WAITING: AtomicUsize = AtomicUsize::new(0);

/// The warm-up is done (`Status::ready`), for the render world: a render
/// pipeline queued after it is logged by its label, unless a background
/// menu section's stand-ins queued it (`preview::section_warming`).
static READY: AtomicBool = AtomicBool::new(false);

/// The GPU fence of the warm-up: the main world asks (a new number), the
/// render world hands the number to the queue after this frame's
/// submission, and the queue sets `FENCE_DONE` to it once that work is done.
static FENCE_NEXT: AtomicU32 = AtomicU32::new(1);
static FENCE_ASKED: AtomicU32 = AtomicU32::new(0);
static FENCE_SENT: AtomicU32 = AtomicU32::new(0);
static FENCE_DONE: AtomicU32 = AtomicU32::new(0);

/// Render world: passes the fence asked for to the GPU queue.
pub fn gpu_fence(queue: Res<RenderQueue>) {
    let asked = FENCE_ASKED.load(Ordering::Relaxed);
    if asked != FENCE_SENT.swap(asked, Ordering::Relaxed) {
        queue.on_submitted_work_done(move || FENCE_DONE.store(asked, Ordering::Relaxed));
    }
}

/// Render world: how many pipelines are still compiling.
///
/// Render pipelines only: on WebGL2 Bevy 0.19 queues its "sparse buffer
/// update" compute pipeline although the device has no compute, and it
/// waits for ever for a shader that is never loaded (DECISIONS D392). The
/// client has no compute pipelines of its own. A pipeline still waiting
/// after 600 frames is logged once, with its state, and one queued after
/// the warm-up once, with its label (its shader defs at debug level).
pub fn count_pipelines(
    cache: Res<PipelineCache>,
    mut stuck: Local<u32>,
    mut late: Local<Vec<usize>>,
) {
    use bevy::render::render_resource::PipelineDescriptor;
    let waiting: Vec<usize> = cache.waiting_pipelines().collect();
    if waiting.is_empty() {
        PIPELINES_WAITING.store(0, Ordering::Relaxed);
        *stuck = 0;
        return;
    }
    let mut render = 0;
    for (i, p) in cache.pipelines().enumerate() {
        if !waiting.contains(&i) {
            continue;
        }
        let label = match &p.descriptor {
            PipelineDescriptor::RenderPipelineDescriptor(d) => {
                render += 1;
                if READY.load(Ordering::Relaxed)
                    && !crate::preview::section_warming()
                    && !late.contains(&i)
                {
                    late.push(i);
                    let label = d.label.as_deref().unwrap_or("?");
                    info!("pipeline {i} queued after the warm-up: {label}");
                    // `RUST_LOG=mr_game::status=debug`: which material and
                    // mesh layout it is for.
                    debug!(
                        "pipeline {i}: vertex {:?} {:?}, fragment {:?}",
                        d.vertex.shader_defs,
                        d.vertex
                            .buffers
                            .iter()
                            .map(|b| b.attributes.len())
                            .collect::<Vec<_>>(),
                        d.fragment.as_ref().map(|f| &f.shader_defs),
                    );
                }
                &d.label
            }
            PipelineDescriptor::ComputePipelineDescriptor(d) => &d.label,
        };
        if *stuck == 600 {
            warn!("pipeline {i} still waiting: {label:?} {:?}", p.state);
        }
    }
    PIPELINES_WAITING.store(render, Ordering::Relaxed);
    *stuck = if waiting.is_empty() { 0 } else { *stuck + 1 };
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
    if status.state == "warming" || status.state == "running" {
        status.scene_frames += 1;
        // Ready once the first frames have queued their pipelines (the
        // warm-up's among them), the queue has drained, and the GPU has
        // finished the work submitted since: a browser creates a pipeline
        // in its GPU process after the call returns, and the first draw
        // with it waits there (a one-second frame just after "ready" in
        // Chrome before this fence; DECISIONS D390).
        // The world build for the animators blocks it too (`animate`).
        if !status.ready && status.scene_frames > 10 && !crate::animate::pending() {
            if status.pipelines_waiting > 0 {
                status.fence = None;
            } else {
                match status.fence {
                    None => {
                        let n = FENCE_NEXT.fetch_add(1, Ordering::Relaxed);
                        FENCE_ASKED.store(n, Ordering::Relaxed);
                        status.fence = Some(n);
                    }
                    Some(n) if FENCE_DONE.load(Ordering::Relaxed) == n => {
                        status.ready = true;
                        status.state = "running";
                        info!("ready: scene up, every pipeline compiled and the GPU done");
                    }
                    Some(_) => {}
                }
            }
        }
    }
    READY.store(status.ready, Ordering::Relaxed);
    if status.ready {
        status.ready_frames += 1;
        // A pipeline queued after the warm-up is one it missed (each costs
        // a hitch on the web); a background menu section's own warm-up is
        // not.
        if status.pipelines_waiting > 0 && !crate::preview::section_warming() {
            status.late_frames += 1;
            if status.late_frames == 1 {
                warn!(
                    "{} pipeline(s) compiling after the warm-up, at s {:.0}",
                    status.pipelines_waiting, status.s
                );
            }
        }
    }
    let ms = time.delta_secs() * 1000.0;
    // `RUST_LOG=mr_game::status=debug`: the frames a player would see as a
    // hitch.
    if ms > 50.0 && status.frames > 2 {
        debug!(
            "slow frame {ms:.0} ms (frame {}, {}, {} pipeline(s) waiting)",
            status.frames, status.state, status.pipelines_waiting
        );
    }
    status.frame_ms = if status.frame_ms == 0.0 {
        ms
    } else {
        status.frame_ms * 0.95 + ms * 0.05
    };
    status.worst_ms = status.worst_ms.max(ms);
}
