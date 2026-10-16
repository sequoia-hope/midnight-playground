//! The section builder's worker on the web (docs/vision/peninsula.md
//! stage 4c; DECISIONS D1215): `mp_builder` as its own wasm behind
//! `web/builder-worker.js`, planning a streamed level and building its
//! sections off the main thread, each handed back packed
//! (`mp_worldgen::section::unpack` reads it). The executor is Playground
//! Kit's `pk_shell::worker` (D1234); this module names the script and the
//! plan's field. `crate::stream` asks and collects.

pub use pk_shell::worker::Msg;

static WORKER: pk_shell::worker::Worker = pk_shell::worker::Worker {
    script: "builder-worker.js",
};

/// Whether the worker is there to ask (started on the first call).
pub fn available() -> bool {
    WORKER.available()
}

/// Asks the worker to plan `level` (for this generation of the stream).
pub fn plan(generation: u64, level: &str) {
    WORKER.plan(generation, &[("level", level)]);
}

/// Asks for section `k`.
pub fn build(generation: u64, k: i64) {
    WORKER.build(generation, k);
}

/// What the worker has answered since the last call.
pub fn take() -> Vec<Msg> {
    WORKER.take()
}
