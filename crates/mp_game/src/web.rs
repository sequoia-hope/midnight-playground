//! Web glue (SPEC 8.4): the page (`web/index.html`) checks WebGPU, loads
//! this module, calls [`run`], downloads the scene with a progress bar and
//! hands it in with [`load_scene`]. Status goes out on `window.__game`,
//! also reachable as `window.__mp` for the e2e suites (Playground Kit's
//! `pk_shell::bridge`, D1234): the shell publishes the contract's fields,
//! this module the game's own (the level, the quality, the fly camera's
//! position and route, the scene's counts).
//!
//! The gesture bridge: the page calls [`gesture`] inside its pointer-down,
//! pointer-up, touch-end, click and key-down handlers, which is where audio
//! resume, fullscreen, the landscape lock and the motion permission must
//! happen (they are refused a frame later). The audio wakes there
//! (`play::audio::gesture`, M5); the rest is `pk_shell::web`'s.

use crate::options::Options;
use crate::status::{SceneStatus, Status};
use crate::{Opts, inbox};
use bevy::prelude::*;
use pk_shell::bridge;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen::prelude::*;

/// Starts the client: options from the page's query string, then the Bevy
/// app (which returns at once on the web; the browser drives its frames).
#[wasm_bindgen]
pub fn run() {
    let mut o = Options::from_query(&pk_shell::web::query());
    // The saved level for the menu, and the saved High quality, which the
    // JS game defaults to off on touch devices (DECISIONS D570).
    let touch = crate::ui::touch_ui(&o);
    let resolved = crate::ui::prepare(&mut o, touch);
    crate::ui::set_start_level(&o.level);
    crate::app(o, resolved).run();
}

/// The scene file's bytes, downloaded by the page. Parsed here, so the
/// page's copy can be dropped as soon as this returns.
#[wasm_bindgen]
pub fn load_scene(bytes: &[u8]) {
    inbox().scene = Some(mp_scene::read(bytes));
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
    pk_shell::backend().into()
}

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

/// The gesture bridge: called by the page inside a user gesture.
#[wasm_bindgen]
pub fn gesture(kind: &str) {
    pk_shell::web::gesture(kind);
}

/// The sound wakes inside the gesture.
fn wake_audio(_kind: &str) {
    crate::play::audio::gesture();
}

/// Captures the next frame and hands it to the browser as a download named
/// `name` (a dev and test hook); sets `shot` to the name when done.
#[wasm_bindgen]
pub fn screenshot(name: String) {
    pk_shell::web::screenshot(name);
}

/// Publishes the game's own fields on the status global (the shell's are
/// `pk_shell::web::publish_status`).
fn publish(
    status: Res<Status>,
    scene: Res<SceneStatus>,
    opts: Res<Opts>,
    mut counts_sent: Local<bool>,
) {
    bridge::set("level", opts.o.level.as_str());
    bridge::set("hq", opts.hq);
    bridge::set("scale", opts.scale);
    bridge::set("s", scene.s);
    if let Some((length, road_end, is_loop)) = scene.route {
        bridge::set("routeLength", length);
        bridge::set("roadEnd", road_end);
        bridge::set("routeLoop", is_loop);
    }
    if scene.counts.is_none() {
        // A reload: send the next scene's counts too.
        *counts_sent = false;
    }
    if !*counts_sent && let Some(c) = &scene.counts {
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
            bridge::set("counts", v);
        }
    }
    let _ = &status;
}

/// Whether the client builds this level's scene itself (`animate`; the
/// default, D678), so the page need not download it.
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
/// native `--stations`, DECISIONS D17): `__mp.flyQuiet` then counts the
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
    bridge::set("flyQuiet", FLY_QUIET.load(Ordering::Relaxed));
}

/// The pixel-ratio policy follows the options: High quality and the render
/// scale (D437, D1194), which Settings may change mid-race.
fn sync_render_scale(opts: Res<Opts>, mut scale: ResMut<pk_shell::web::RenderScale>) {
    if scale.hq != opts.hq || scale.scale != opts.scale {
        scale.hq = opts.hq;
        scale.scale = opts.scale;
    }
}

pub fn plugin(app: &mut App) {
    // The status global: the kit's name, and this game's for its suites.
    app.add_plugins(pk_shell::web::WebPlugin {
        global: "__game",
        aliases: &["__mp"],
    });
    pk_shell::web::on_gesture(wake_audio);
    pk_shell::web::set_permission_hook(crate::play::tilt::web::ask_in_gesture);
    {
        let o = app.world().resource::<Opts>().clone();
        app.insert_resource(pk_shell::web::RenderScale {
            hq: o.hq,
            scale: o.scale,
            ..default()
        });
    }
    app.add_systems(PreUpdate, sync_render_scale)
        .add_systems(Last, publish);
    app.add_systems(Update, fly_hook.after(crate::fly_system))
        .add_systems(Last, publish_fly);
}
