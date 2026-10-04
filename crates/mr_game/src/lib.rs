//! The client (SPEC 3, 6, 8). Only this crate depends on the engine.
//!
//! Roadmap M2 so far: the Bevy app shell (WP 2.1), the scene loader with
//! the fly camera (WP 2.2), three.js's shading, sky, environment, shadows,
//! fog and post chain (WP 2.3), and the terrain, road, sea and points kinds
//! with stand-in cars from the simulation (WP 2.4), the pipeline warm-up,
//! scene reloads and the measurement page (WP 2.6), and the WebGL2 build
//! (WP 2.7). The client loads a level's scene
//! export (or the car models), draws it as the JS game does for the kinds
//! ported so far and flies the JS game's debug camera along the route, the
//! sky following the route's time of day.
//!
//! - [`options`]: the query string (web) or command line (native).
//! - [`convert`]: `mr_scene` data to Bevy meshes and images.
//! - [`loader`]: builds a scene into entities, a slice per frame.
//! - [`render`]: three_std, the materials, sky, environment, shadow, post.
//! - [`matscene`]: the material test scenes (SPEC 6.2 "Verification").
//! - [`stations`]: the screenshot stations, flown natively (DECISIONS D17).
//! - [`fly`]: the fly and attract cameras of `src/main.js`.
//! - [`cars`]: stand-in cars driven by the simulation (WP 2.4).
//! - [`play`]: a playable race (M4): session, input, camera, flow, HUD.
//! - [`plugins`]: Bevy's default plugins less its 2D sprites.
//! - [`ui`]: the screens (M6): loading, menu, pause, results, controller
//!   setup, and the settings store.
//! - [`warmup`]: every pipeline the scene needs, compiled behind the
//!   loading screen (WP 2.6).
//! - [`status`]: what the page and the window title show.
//!
//! The web build comes in two backends, WebGPU and WebGL2 (the `mr_webgl2`
//! cfg), one wasm file each; the page picks (WP 2.7).

pub mod animate;
pub mod cars;
pub mod convert;
pub mod fly;
pub mod levels;
pub mod loader;
pub mod matscene;
pub mod options;
pub mod play;
pub mod plugins;
pub mod render;
pub mod stations;
pub mod status;
pub mod ui;
pub mod warmup;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use bevy::camera::{Hdr, PerspectiveProjection, Projection};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::light::DirectionalLightShadowMap;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use loader::{AppState, Build, Loaded, SkyDome};
use mr_scene::Scene;
use mr_track::Track;
use options::{FlyParams, Options};
use render::pmrem::EnvRequest;
use render::{Lighting, SharedImages, SkyState};
use status::Status;
use std::sync::Mutex;

/// The line both builds print, so the native and web pipelines are seen to
/// run the same crate.
pub fn banner() -> String {
    format!("Midnight Racer (Rust) {}", env!("CARGO_PKG_VERSION"))
}

/// What arrives from outside the app: the scene file (read by a thread
/// natively, handed in by the page on the web) and Seaside's survey data.
#[derive(Default)]
pub struct Inbox {
    pub scene: Option<Result<Scene, String>>,
    pub survey: Option<Result<Vec<u8>, String>>,
    /// Tear the scene down and wait for the next one, of this level (the
    /// measurement page's reloads, WP 2.6).
    pub unload: Option<String>,
}

pub static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    scene: None,
    survey: None,
    unload: None,
});

fn inbox() -> std::sync::MutexGuard<'static, Inbox> {
    INBOX.lock().unwrap_or_else(|e| e.into_inner())
}

/// The options, resolved.
#[derive(Resource, Clone, Debug)]
pub struct Opts {
    pub o: Options,
    pub hq: bool,
}

/// The level's Track, for the cameras (none for the models scene, or until
/// Seaside's survey has arrived).
#[derive(Resource, Default)]
pub struct TrackRes {
    pub track: Option<Track>,
    /// The level it was built from (Seaside prepared with its survey), for
    /// the simulation of the stand-in cars.
    pub level: Option<mr_track::Level>,
    /// Building it failed or does not apply; the camera stays where the
    /// scene was exported.
    pub none: bool,
}

/// The camera's own state.
#[derive(Resource)]
pub struct CameraState {
    pub fly: Option<FlyParams>,
    pub attract: fly::Attract,
    pub focus: DVec3,
}

/// The sky of the level (none until its Track is built), and where the
/// environment map was last built (`main.js` `envAt`).
#[derive(Resource, Default)]
pub struct SkyRes {
    pub sky: Option<SkyState>,
    pub env_at: Option<f64>,
    /// What `update_sky` was given this frame (`World.update`'s dt, s and
    /// focus), for the scenery's animators (`animate`).
    pub frame: Option<animate::WorldFrame>,
}

/// The JS camera: 62° vertical field of view, near 0.3, far 9000
/// (`main.js:60`); a half-float target with 4× MSAA (`:61`); the bloom,
/// ACES filmic and sRGB of three's post chain (`render::post`) in place of
/// Bevy's tone mapping.
fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Msaa::Sample4,
        Tonemapping::None,
        DebandDither::Disabled,
        render::post::ThreePost,
        Projection::Perspective(PerspectiveProjection {
            fov: 62f32.to_radians(),
            near: 0.3,
            far: 9000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 50.0, 0.0),
    ));
}

/// Builds the level's Track (Seaside once its survey is in) and its sky.
/// Without a level (the models), the sky is Sierra's at the start.
fn make_track(mut tr: ResMut<TrackRes>, mut sky: ResMut<SkyRes>, opts: Res<Opts>) {
    if tr.track.is_some() || tr.none {
        return;
    }
    let id = opts.o.level.as_str();
    let known = mr_levels::levels().iter().any(|l| l.id == id);
    if !known {
        tr.none = true;
        if sky.sky.is_none() {
            let mut s = SkyState::new(&mr_levels::level_by_id("sierra"), false, 1.0);
            s.override_p = Some(0.0);
            sky.sky = Some(s);
        }
        return;
    }
    let mut level = mr_levels::level_by_id(id);
    if id == "seaside" {
        let survey = match inbox().survey.take() {
            None => return, // not yet
            Some(Err(e)) => {
                warn!("seaside survey: {e}; the camera stays at the export's");
                tr.none = true;
                return;
            }
            Some(Ok(b)) => b,
        };
        match mr_levels::SeasideData::parse(&survey) {
            Ok(d) => mr_levels::seaside::prepare(&mut level, std::sync::Arc::new(d)),
            Err(e) => {
                warn!("seaside survey: {e}");
                tr.none = true;
                return;
            }
        }
    }
    match Track::new(&level) {
        Ok(mut t) => {
            // The scenery's runout past the last sample (the world data the
            // simulation uses too), so the fly camera reaches the road's end
            // as the JS one does.
            t.runout = mr_levels::world::world_data(level.id).runout;
            info!("track {id}: {:.0} m", t.length);
            let mut s = SkyState::new(&level, t.is_loop, t.length);
            s.override_p = opts.o.t;
            sky.sky = Some(s);
            tr.track = Some(t);
            tr.level = Some(level);
        }
        Err(e) => {
            warn!("track {id}: {e}");
            tr.none = true;
        }
    }
}

/// Waits for the scene file and starts the build.
fn receive_scene(
    mut commands: Commands,
    mut status: ResMut<Status>,
    mut next: ResMut<NextState<AppState>>,
    shared: Res<SharedImages>,
) {
    let Some(r) = inbox().scene.take() else {
        return;
    };
    match r {
        Ok(scene) => {
            info!(
                "scene: {} nodes, {} meshes, {} materials, {} textures",
                scene.nodes.len(),
                scene.meshes.len(),
                scene.materials.len(),
                scene.textures.len()
            );
            status.state = "building";
            commands.insert_resource(Build::new(scene, shared.clone()));
            next.set(AppState::Building);
        }
        Err(e) => {
            status.fail(format!("scene: {e}"));
            next.set(AppState::Failed);
        }
    }
}

/// The scene is up; its pipelines compile behind the loading screen (the
/// warm-up, `warmup`) until the status turns `ready` and `running`.
fn enter_running(mut status: ResMut<Status>) {
    status.state = "warming";
    status.progress = 1.0;
    status.scenes += 1;
}

/// Tears the scene down when asked (`Inbox::unload`) and waits for the next
/// one: every scene entity (the warm-up's and the stand-in cars too), the
/// build in progress, the cars' race and the night materials go, so their
/// meshes, materials and images are freed; a different level gets its own
/// Track and sky. The measurement page's reloads (WP 2.6) use it.
#[allow(clippy::too_many_arguments)]
fn unload_scene(
    mut commands: Commands,
    scene: Query<Entity, With<loader::SceneEntity>>,
    mut status: ResMut<Status>,
    mut opts: ResMut<Opts>,
    mut tr: ResMut<TrackRes>,
    mut sky: ResMut<SkyRes>,
    mut cs: ResMut<CameraState>,
    mut next: ResMut<NextState<AppState>>,
) {
    let Some(level) = inbox().unload.take() else {
        return;
    };
    for e in &scene {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<Build>();
    commands.remove_resource::<cars::Cars>();
    commands.remove_resource::<Loaded>();
    inbox().scene = None;
    if level != opts.o.level {
        opts.o.level = level;
        *tr = TrackRes::default();
        *sky = SkyRes::default();
        inbox().survey = None;
    }
    sky.env_at = None;
    cs.fly = opts.o.fly;
    cs.attract = fly::Attract::default();
    *status = Status {
        state: "waiting",
        frames: status.frames,
        frame_ms: status.frame_ms,
        gestures: status.gestures,
        scenes: status.scenes,
        ..default()
    };
    next.set(AppState::Waiting);
    info!("scene unloaded; waiting for {}", opts.o.level);
}

/// Where the camera starts once the scene is up: the export's camera, or for
/// the models a view of all of them.
fn place_camera(
    loaded: Res<Loaded>,
    mut cam: Query<&mut Transform, With<Camera3d>>,
    mut cs: ResMut<CameraState>,
) {
    let Ok(mut t) = cam.single_mut() else { return };
    if let Some(c) = loaded.camera {
        *t = c;
        cs.focus = c.translation.as_dvec3();
    } else if loaded.min.is_finite() {
        let mid = (loaded.min + loaded.max) * 0.5;
        let size = (loaded.max - loaded.min).length().max(4.0);
        *t = Transform::from_translation(mid + Vec3::new(0.0, 0.5, 0.8) * size * 0.6)
            .looking_at(mid, Vec3::Y);
    }
}

/// The fly camera (`?s=`), else the attract camera, along the Track. Up and
/// Down change the fly camera's speed by 10 m/s (a dev convenience the JS
/// does not have). Then the sky at the camera's s (`world.update` →
/// `Sky.update`) and the environment map when the time of day has moved
/// (`refreshEnv`).
#[allow(clippy::too_many_arguments)]
pub fn fly_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    tr: Res<TrackRes>,
    opts: Res<Opts>,
    mut cs: ResMut<CameraState>,
    mut cam: Query<&mut Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut sky: Query<&mut Transform, With<SkyDome>>,
    mut status: ResMut<Status>,
    mut sky_res: ResMut<SkyRes>,
    mut lighting: ResMut<Lighting>,
    mut env: ResMut<EnvRequest>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    play: Option<Res<play::Play>>,
) {
    // The race drives the camera and the sky (`play`); without one (the
    // menu, `ui`) the attract camera flies.
    if opts.o.race_on() && play.as_ref().is_none_or(|p| p.race.is_some()) {
        return;
    }
    // `Math.min(frameDt, 1 / 20)` (main.js tick). The camera holds still
    // while the warm-up compiles behind the loading screen, as the JS game
    // starts flying only once it is loaded.
    let dt = if status.ready {
        f64::from(time.delta_secs()).min(1.0 / 20.0)
    } else {
        0.0
    };
    let pixel_ratio = windows
        .single()
        .map_or(1.0, |w| f64::from(w.scale_factor()));
    let Some(track) = &tr.track else {
        // The models: the sky stays at Sierra's start around the grid.
        if tr.none {
            let focus = cs.focus;
            update_sky(
                &mut sky_res,
                &opts,
                (0.0, pixel_ratio),
                0.0,
                focus,
                &mut lighting,
                &mut env,
            );
        }
        return;
    };
    if status.route.is_none() {
        status.route = Some((track.length, track.road_end(), track.is_loop));
    }
    let Ok(mut t) = cam.single_mut() else { return };
    let cs = &mut *cs;
    if let Some(f) = cs.fly.as_mut() {
        if keys.just_pressed(KeyCode::ArrowUp) {
            f.speed += 10.0;
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            f.speed -= 10.0;
        }
        // main.js: in fly mode the focus is frame(s - back).
        let (pose, focus) = fly::fly_camera(track, dt, f);
        cs.focus = focus;
        *t = pose;
        status.s = f.s;
    } else {
        let (pose, focus) = cs.attract.step(track, dt);
        cs.focus = focus;
        *t = pose;
        status.s = cs.attract.s;
    }
    loader::follow_focus(cs.focus, &mut sky);
    // ?freeze=1: the scenery (and the sky's clock) holds still.
    let world_dt = if opts.o.freeze { 0.0 } else { dt };
    update_sky(
        &mut sky_res,
        &opts,
        (world_dt, pixel_ratio),
        status.s,
        cs.focus,
        &mut lighting,
        &mut env,
    );
}

/// `Sky.update`, the updaters' per-frame uniforms (`render::lighting::Anim`)
/// and `refreshEnv` (main.js): the environment is rebuilt when the time of
/// day has moved 2.5 % of the route since the last build. `frame` is the
/// world's dt and the pixel ratio.
pub(crate) fn update_sky(
    sky_res: &mut SkyRes,
    opts: &Opts,
    (dt, pixel_ratio): (f64, f64),
    s: f64,
    focus: DVec3,
    lighting: &mut Lighting,
    env: &mut EnvRequest,
) {
    // The world update this frame, for the scenery's animators.
    let f = sky_res.frame.get_or_insert_with(Default::default);
    f.dt += dt;
    f.s = s;
    f.focus = focus;
    let Some(sky) = sky_res.sky.as_mut() else {
        return;
    };
    let mut next = lighting.clone();
    sky.update(dt, s, focus, &mut next);
    next.anim.advance(dt, sky.night);
    next.anim.pixel_ratio = pixel_ratio;
    next.shadows = opts.hq;
    next.env_intensity = ENV_INTENSITY;
    if *lighting != next {
        *lighting = next;
    }
    let p = sky
        .override_p
        .unwrap_or(if sky.is_loop { 0.5 } else { s / sky.length });
    if sky_res.env_at.is_none_or(|at| (p - at).abs() >= 0.025) {
        sky_res.env_at = Some(p);
        env.generation += 1;
    }
}

/// `scene.environmentIntensity` (main.js `refreshEnv`).
pub const ENV_INTENSITY: f64 = 0.7;

/// The Bevy app for these options. The platform modules add where the scene
/// comes from and how status is shown.
pub fn app(o: Options, hq: bool) -> App {
    let mut app = App::new();
    let title = format!("{} — {}", banner(), o.level);
    let materials = o.materials.clone();
    // DECISIONS D396.
    let gpu_preprocessing = o.gpu_preprocessing.unwrap_or(true);
    #[cfg(not(target_arch = "wasm32"))]
    let window = {
        // The material test scenes are 512 × 512 (scenes.json).
        let (w, h) = o.size.unwrap_or(if materials.is_some() {
            (512, 512)
        } else {
            (1280, 800)
        });
        Window {
            title,
            resolution: bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0),
            // A screenshot run draws without showing a window, where the
            // platform allows it.
            visible: o.screenshot.is_none()
                && o.param("shots").is_none()
                && materials.is_none()
                && o.stations.is_none(),
            ..default()
        }
    };
    #[cfg(target_arch = "wasm32")]
    let window = Window {
        title,
        canvas: Some("#game".into()),
        fit_canvas_to_parent: true,
        ..default()
    };
    app.add_plugins(
        plugins::ClientPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..default()
            })
            .set(bevy::pbr::PbrPlugin {
                use_gpu_instance_buffer_builder: gpu_preprocessing,
                ..default()
            }),
    )
    // The JS shadow map is 2048² (Sky.js).
    .insert_resource(DirectionalLightShadowMap { size: 2048 })
    // three's default clear colour; the sky dome covers it.
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(SkyRes::default())
    .insert_resource(Status {
        state: "waiting",
        ..default()
    })
    .insert_resource(TrackRes::default())
    .insert_resource(CameraState {
        fly: o.fly,
        attract: fly::Attract::default(),
        focus: DVec3::ZERO,
    })
    .insert_resource(Opts { o, hq })
    .init_state::<AppState>()
    .add_systems(Startup, spawn_camera)
    .add_systems(First, status::tick)
    .add_systems(Update, make_track)
    .add_systems(Update, unload_scene.before(receive_scene))
    .add_systems(Update, receive_scene.run_if(in_state(AppState::Waiting)))
    .add_systems(Update, warmup::end_warm_up)
    .add_systems(
        Update,
        loader::build_step.run_if(in_state(AppState::Building)),
    )
    .add_systems(
        OnEnter(AppState::Running),
        (enter_running, place_camera).chain(),
    )
    .add_systems(Update, fly_system.run_if(in_state(AppState::Running)))
    .add_systems(
        Update,
        (cars::start_cars, cars::drive_cars)
            .chain()
            .run_if(in_state(AppState::Running)),
    );
    app.sub_app_mut(bevy::render::RenderApp).add_systems(
        bevy::render::Render,
        (status::count_pipelines, status::gpu_fence).in_set(bevy::render::RenderSystems::Cleanup),
    );
    app.add_plugins(render::ThreeRenderPlugin);
    if let Some(which) = &materials {
        let out = app.world().resource::<Opts>().o.out.clone();
        matscene::plugin(&mut app, which, out);
    }
    let (stations, out) = {
        let o = &app.world().resource::<Opts>().o;
        (o.stations.clone(), o.out.clone())
    };
    if let Some(path) = stations {
        stations::plugin(&mut app, &path, out);
    }
    play::plugin(&mut app);
    ui::plugin(&mut app);
    animate::plugin(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    native::plugin(&mut app);
    #[cfg(target_arch = "wasm32")]
    web::plugin(&mut app);
    app
}
