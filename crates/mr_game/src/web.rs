//! Web glue (SPEC 8.4): the page (`web/index.html`) checks WebGPU, loads
//! this module, calls [`run`], downloads the scene with a progress bar and
//! hands it in with [`load_scene`]. Status goes out on `window.__mr`.
//!
//! The gesture bridge: the page calls [`gesture`] inside its pointer-down,
//! pointer-up, touch-end, click and key-down handlers, which is where audio
//! resume, fullscreen, the landscape lock and the motion permission must
//! happen (they are refused a frame later). The audio wakes there
//! (`play::audio::gesture`, M5); the rest is M6's.

use crate::options::Options;
use crate::status::Status;
use crate::{Opts, inbox};
use bevy::prelude::*;
use js_sys::{Object, Reflect};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen::prelude::*;

static GESTURES: AtomicU32 = AtomicU32::new(0);
static SCREENSHOT: Mutex<Option<String>> = Mutex::new(None);

/// Starts the client: options from the page's query string, then the Bevy
/// app (which returns at once on the web; the browser drives its frames).
#[wasm_bindgen]
pub fn run() {
    let window = web_sys::window().expect("a window");
    let search = window.location().search().unwrap_or_default();
    let o = Options::from_query(&search);
    // The JS game's default: high quality off on touch devices.
    let touch = Reflect::get(&window.navigator(), &JsValue::from_str("maxTouchPoints"))
        .ok()
        .and_then(|v| v.as_f64())
        .is_some_and(|n| n > 0.0);
    let hq = o.hq.unwrap_or(!touch);
    crate::app(o, hq).run();
}

/// The scene file's bytes, downloaded by the page. Parsed here, so the
/// page's copy can be dropped as soon as this returns.
#[wasm_bindgen]
pub fn load_scene(bytes: &[u8]) {
    inbox().scene = Some(mr_scene::read(bytes));
}

/// Tears the current scene down and waits for the next one (handed in with
/// [`load_scene`]), of `level`: the measurement page's reloads (WP 2.6).
#[wasm_bindgen]
pub fn unload_scene(level: String) {
    inbox().unload = Some(level);
}

/// The backend this wasm was built for (SPEC 2: one file each, the page
/// picks).
#[wasm_bindgen]
pub fn backend() -> String {
    BACKEND.into()
}

#[cfg(mr_webgl2)]
const BACKEND: &str = "webgl2";
#[cfg(not(mr_webgl2))]
const BACKEND: &str = "webgpu";

/// The page could not get the scene file.
#[wasm_bindgen]
pub fn scene_failed(message: String) {
    inbox().scene = Some(Err(message));
}

/// Seaside Raceway's survey data (`assets/seaside/survey.bin`), for its Track.
#[wasm_bindgen]
pub fn load_survey(bytes: &[u8]) {
    inbox().survey = Some(Ok(bytes.to_vec()));
}

#[wasm_bindgen]
pub fn survey_failed(message: String) {
    inbox().survey = Some(Err(message));
}

/// The gesture bridge (stub): called by the page inside a user gesture.
#[wasm_bindgen]
pub fn gesture(_kind: &str) {
    GESTURES.fetch_add(1, Ordering::Relaxed);
    crate::play::audio::gesture();
}

/// Captures the next frame with Bevy's screenshot and hands it to the
/// browser as a download named `name` (a dev and test hook: headless Chrome
/// does not composite a WebGPU canvas into its own screenshots). Sets
/// `window.__mr.shot` to the name when done.
#[wasm_bindgen]
pub fn screenshot(name: String) {
    *SCREENSHOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(name);
}

fn take_screenshot(mut commands: Commands) {
    use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
    let Some(name) = SCREENSHOT.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let done = name.clone();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(name))
        .observe(move |_: On<ScreenshotCaptured>| {
            if let Some(w) = web_sys::window()
                && let Ok(mr) = Reflect::get(&w, &JsValue::from_str("__mr"))
                && let Some(mr) = mr.dyn_ref::<Object>()
            {
                set(mr, "shot", done.as_str());
            }
        });
}

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
}

/// Publishes the status on `window.__mr` (created by the page).
fn publish(mut status: ResMut<Status>, opts: Res<Opts>, mut counts_sent: Local<bool>) {
    status.gestures = GESTURES.load(Ordering::Relaxed);
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(mr) = Reflect::get(&window, &JsValue::from_str("__mr")) else {
        return;
    };
    let Some(mr) = mr.dyn_ref::<Object>() else {
        return;
    };
    let now = window.performance().map_or(0.0, |p| p.now());
    if status.frames == 1 {
        set(mr, "firstFrameMs", now);
    }
    if status.ready && status.ready_frames == 1 {
        set(mr, "readyMs", now);
    }
    set(mr, "state", status.state);
    set(mr, "level", opts.o.level.as_str());
    set(mr, "hq", opts.hq);
    set(mr, "progress", status.progress);
    set(mr, "frames", status.frames as f64);
    set(mr, "frameMs", status.frame_ms);
    set(mr, "pipelinesWaiting", status.pipelines_waiting as f64);
    set(mr, "ready", status.ready);
    set(mr, "s", status.s);
    set(mr, "gestures", status.gestures);
    set(mr, "backend", BACKEND);
    set(mr, "warmUp", status.warm_up as f64);
    set(mr, "lateFrames", status.late_frames as f64);
    set(mr, "scenes", status.scenes);
    if let Some((length, road_end, is_loop)) = status.route {
        set(mr, "routeLength", length);
        set(mr, "roadEnd", road_end);
        set(mr, "routeLoop", is_loop);
    }
    if let Some(e) = &status.error {
        set(mr, "error", e.as_str());
    }
    if status.counts.is_none() {
        // A reload: send the next scene's counts too.
        *counts_sent = false;
    }
    if !*counts_sent && let Some(c) = &status.counts {
        *counts_sent = true;
        let hidden: Vec<String> = c
            .hidden_kinds
            .iter()
            .map(|(k, n)| format!("\"{k:?}\": {n}"))
            .collect();
        let json = format!(
            "{{\"entities\": {}, \"meshes\": {}, \"materials\": {}, \"images\": {}, \"hidden\": {{{}}}, \"skipped\": {}, \"invisible\": {}}}",
            c.entities,
            c.meshes,
            c.materials,
            c.images,
            hidden.join(", "),
            c.skipped_nodes,
            c.invisible
        );
        if let Ok(v) = js_sys::JSON::parse(&json) {
            set(mr, "counts", v);
        }
    }
}

/// Whether the client builds this level's scene itself with `?world=gen`
/// (`animate`), so the page need not download it.
#[wasm_bindgen]
pub fn generates_scene(level: &str) -> bool {
    crate::animate::generated(level)
}

/// The client is building the level's world (`animate`, for its
/// animators): the page waits for it before handing in the downloaded
/// scene, so the build's memory is free again when the scene is parsed
/// (the wasm memory's high-water mark is the larger of the two, not their
/// sum; D491).
#[wasm_bindgen]
pub fn world_pending() -> bool {
    crate::animate::pending()
}

static FLY: Mutex<Option<crate::options::FlyParams>> = Mutex::new(None);
static FLY_QUIET: AtomicU32 = AtomicU32::new(0);

/// Moves the fly camera to a screenshot station (a test hook, as the
/// native `--stations`, DECISIONS D17): `__mr.flyQuiet` then counts the
/// frames since the camera arrived with no pipeline compiling and the
/// environment map built; three is enough for a screenshot.
#[wasm_bindgen]
pub fn fly_to(s: f64, h: f64, back: f64, lat: f64, yaw: f64, pitch: f64) {
    *FLY.lock().unwrap_or_else(|e| e.into_inner()) = Some(crate::options::FlyParams {
        s,
        h,
        back,
        lat,
        speed: 0.0,
        yaw,
        pitch,
    });
    FLY_QUIET.store(0, Ordering::Relaxed);
}

fn fly_hook(
    mut cs: ResMut<crate::CameraState>,
    status: Res<Status>,
    env: Res<crate::render::pmrem::EnvRequest>,
    mut frames: Local<u32>,
) {
    if let Some(p) = FLY.lock().unwrap_or_else(|e| e.into_inner()).take() {
        cs.fly = Some(p);
        *frames = 0;
        return;
    }
    *frames += 1;
    let settled = status.ready
        && crate::status::PIPELINES_WAITING.load(Ordering::Relaxed) == 0
        && crate::render::pmrem::ENV_DONE.load(Ordering::Relaxed) == env.generation;
    if settled && *frames > 3 {
        FLY_QUIET.fetch_add(1, Ordering::Relaxed);
    } else {
        FLY_QUIET.store(0, Ordering::Relaxed);
    }
}

fn publish_fly() {
    if let Some(w) = web_sys::window()
        && let Ok(mr) = Reflect::get(&w, &JsValue::from_str("__mr"))
        && let Some(mr) = mr.dyn_ref::<Object>()
    {
        set(mr, "flyQuiet", FLY_QUIET.load(Ordering::Relaxed));
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Last, publish)
        .add_systems(Update, take_screenshot);
    app.add_systems(Update, fly_hook.after(crate::fly_system))
        .add_systems(Last, publish_fly);
}
