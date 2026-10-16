//! What the client reports about itself: the load state and readiness are
//! Playground Kit's (`pk_shell::status`, D1234: the pipeline count, the GPU
//! fence, the late frames; `RUST_LOG=pk_shell::status=debug` for the slow
//! frames), published on `window.__game` and in the window title; this
//! module adds what is this game's.

pub use pk_shell::status::{LoadState, LoadStatus, PIPELINES_WAITING, ReadyBlockers};

/// The load status (`pk_shell::status::LoadStatus`).
pub type Status = LoadStatus;

/// The scene's own figures, beside the load status: the loader's counts,
/// the fly camera's s and the route (for the title and the page).
#[derive(bevy::prelude::Resource, Default, Debug, Clone)]
pub struct SceneStatus {
    pub counts: Option<crate::loader::Counts>,
    /// The fly camera's s, for the title and the page.
    pub s: f64,
    /// The fly camera's route, once the Track is known: its length, where
    /// the road ends, and whether it loops (for the measurement page).
    pub route: Option<(f64, f64, bool)>,
}
