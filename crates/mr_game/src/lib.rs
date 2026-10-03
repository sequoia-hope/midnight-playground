//! The client (SPEC 3, 6, 8). Only this crate depends on the engine.
//!
//! Roadmap M2 so far: the Bevy app shell (WP 2.1) and the scene loader with
//! the fly camera (WP 2.2). The client loads a level's scene export (or the
//! car models), draws it with stand-in materials and flies the JS game's
//! debug camera along the route.
//!
//! - [`options`]: the query string (web) or command line (native).
//! - [`convert`]: `mr_scene` data to Bevy meshes, images and materials.
//! - [`loader`]: builds a scene into entities, a slice per frame.
//! - [`fly`]: the fly and attract cameras of `src/main.js`.
//! - [`status`]: what the page and the window title show.

pub mod convert;
pub mod fly;
pub mod loader;
pub mod options;
pub mod status;

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
mod web;

use bevy::camera::{Hdr, PerspectiveProjection, Projection};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::DirectionalLightShadowMap;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::render::view::Msaa;
use loader::{AppState, Build, Loaded, SkyDome};
use mr_scene::Scene;
use mr_track::Track;
use options::{FlyParams, Options};
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
}

pub static INBOX: Mutex<Inbox> = Mutex::new(Inbox {
    scene: None,
    survey: None,
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

/// The JS camera: 62° vertical field of view, near 0.3, far 9000
/// (`main.js:60`); HDR with 4× MSAA (`:61`); ACES filmic (`:57`).
fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Msaa::Sample4,
        Tonemapping::AcesFitted,
        Projection::Perspective(PerspectiveProjection {
            fov: 62f32.to_radians(),
            near: 0.3,
            far: 9000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 50.0, 0.0),
    ));
}

/// Builds the level's Track (Seaside once its survey is in).
fn make_track(mut tr: ResMut<TrackRes>, opts: Res<Opts>) {
    if tr.track.is_some() || tr.none {
        return;
    }
    let id = opts.o.level.as_str();
    let known = mr_levels::levels().iter().any(|l| l.id == id);
    if !known {
        tr.none = true;
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
        Ok(t) => {
            info!("track {id}: {:.0} m", t.length);
            tr.track = Some(t);
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
            commands.insert_resource(Build::new(scene));
            next.set(AppState::Building);
        }
        Err(e) => {
            status.fail(format!("scene: {e}"));
            next.set(AppState::Failed);
        }
    }
}

fn enter_running(mut status: ResMut<Status>) {
    status.state = "running";
    status.progress = 1.0;
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
/// does not have).
fn fly_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    tr: Res<TrackRes>,
    mut cs: ResMut<CameraState>,
    mut cam: Query<&mut Transform, (With<Camera3d>, Without<SkyDome>)>,
    mut sky: Query<&mut Transform, With<SkyDome>>,
    mut status: ResMut<Status>,
) {
    let Some(track) = &tr.track else { return };
    let Ok(mut t) = cam.single_mut() else { return };
    // `Math.min(frameDt, 1 / 20)` (main.js tick).
    let dt = f64::from(time.delta_secs()).min(1.0 / 20.0);
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
}

/// The Bevy app for these options. The platform modules add where the scene
/// comes from and how status is shown.
pub fn app(o: Options, hq: bool) -> App {
    let mut app = App::new();
    let title = format!("{} — {}", banner(), o.level);
    #[cfg(not(target_arch = "wasm32"))]
    let window = {
        let (w, h) = o.size.unwrap_or((1280, 800));
        Window {
            title,
            resolution: bevy::window::WindowResolution::new(w, h),
            // A screenshot run draws without showing a window, where the
            // platform allows it.
            visible: o.screenshot.is_none(),
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
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window),
        ..default()
    }))
    // The JS shadow map is 2048² (Sky.js).
    .insert_resource(DirectionalLightShadowMap { size: 2048 })
    .insert_resource(ClearColor(Color::linear_rgb(0.02, 0.025, 0.04)))
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
    .add_systems(Update, receive_scene.run_if(in_state(AppState::Waiting)))
    .add_systems(
        Update,
        loader::build_step.run_if(in_state(AppState::Building)),
    )
    .add_systems(
        OnEnter(AppState::Running),
        (enter_running, place_camera).chain(),
    )
    .add_systems(Update, fly_system.run_if(in_state(AppState::Running)));
    app.sub_app_mut(bevy::render::RenderApp).add_systems(
        bevy::render::Render,
        status::count_pipelines.in_set(bevy::render::RenderSystems::Cleanup),
    );
    loader::tint_plugin(&mut app);
    #[cfg(not(target_arch = "wasm32"))]
    native::plugin(&mut app);
    #[cfg(target_arch = "wasm32")]
    web::plugin(&mut app);
    app
}
