//! A playable race (roadmap M4: WP 4.2, 4.3, 4.5, 4.6). The level's scene
//! export is the world; the race is `mr_sim`'s, stepped by the loopback
//! [`session`] at the fixed tick, drawn as an interpolation of the last two
//! ticks ([`pose`], `Vehicle.sync`), followed by the chase camera
//! ([`camera`], `CameraRig.js` and the intro swing), driven by the
//! keyboard or the touch controls through the input layer ([`input`],
//! [`touch`]), from the countdown to a plain results list ([`flow`], the
//! HUD in [`hud`]).
//!
//! The race is the default for a level when no fly-camera parameters are
//! given (`?race=0` gives the attract camera and the stand-in cars back;
//! DECISIONS D432). Query parameters, as the JS game's where it has them:
//! `car` (or `autostart=<car>`), `seed`, `autodrive=1`, `timescale`,
//! `pursuit=1`, `heat`, `touch=0|1`; natively also `shots=<dir>`, which
//! saves the countdown, the race and the results as PNGs and exits.
//!
//! Cars are `CarModel.js`'s models, ported in WP 4.1 ([`models`]); the
//! smoke, sparks, skid marks, flames and headlight pools are `Effects.js`'s
//! ([`effects`], drawn by [`fx`], WP 4.4).

pub mod audio;
pub mod camera;
pub mod effects;
pub mod flow;
mod fx;
mod fx_stage;
pub mod gamepad;
pub mod gamepad_io;
mod hud;
pub mod input;
mod models;
pub mod police;
pub mod pose;
mod pv_stage;
pub mod radio;
pub mod session;
pub mod tilt;
pub mod touch;
mod touch_ui;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

use crate::loader::{AppState, SkyDome};
use crate::render::Lighting;
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::ThreeMaterial;
use crate::render::pmrem::EnvRequest;
use crate::status::Status;
use crate::{CameraState, Opts, SkyRes, TrackRes};
use bevy::camera::Projection;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseButtonInput;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow, WindowFocused};
use flow::{Mode, Race, Setup};
use mr_sim::race::{LevelRuntime, RaceOpts, RaceStateKind};
use mr_worldgen::car_model::Lod;
use touch::{Insets, Layout, TouchControls};

/// The race's options from the query string.
#[derive(Clone, Debug)]
pub struct Params {
    pub car: &'static str,
    pub seed: Option<u32>,
    pub autodrive: bool,
    pub timescale: f64,
    pub pursuit: bool,
    pub heat: f64,
    /// `?cops=N`: Hot Pursuit's cap on units (6 by default).
    pub cops: f64,
    /// The police lights strobe (the menu's flash option, `settings.flash`).
    pub flash: bool,
    /// `?touch=1|0` forces the touch controls on or off.
    pub touch: Option<bool>,
    /// Natively: save the three screenshots here and exit.
    pub shots: Option<String>,
    /// `?fx=0`: the race without its effects (a measurement switch).
    pub fx: bool,
}

impl Params {
    pub fn from_options(o: &crate::options::Options) -> Params {
        let get = |k: &str| o.param(k);
        let car = get("car")
            .or_else(|| get("autostart"))
            .and_then(|c| {
                mr_sim::physics::CAR_SPECS
                    .iter()
                    .map(|(k, _)| *k)
                    .find(|k| *k == c)
            })
            .unwrap_or("sports");
        Params {
            car,
            seed: get("seed").and_then(|s| s.parse().ok()),
            autodrive: get("autodrive") == Some("1"),
            timescale: get("timescale")
                .and_then(|s| s.parse().ok())
                .filter(|t: &f64| *t > 0.0)
                .unwrap_or(1.0),
            pursuit: get("pursuit") == Some("1"),
            heat: get("heat")
                .and_then(|s| s.parse().ok())
                .filter(|h: &f64| *h >= 1.0)
                .unwrap_or(1.0),
            cops: get("cops").and_then(|c| c.parse().ok()).unwrap_or(6.0),
            flash: true,
            touch: get("touch").map(|v| v == "1"),
            shots: get("shots").map(str::to_owned),
            fx: get("fx") != Some("0"),
        }
    }

    fn race_opts(&self) -> RaceOpts {
        RaceOpts {
            car: self.car,
            seed: self.seed.unwrap_or_else(clock_seed),
            pursuit: self.pursuit,
            heat: self.heat,
        }
    }
}

/// A seed for a race nobody asked a seed for: the rivals' nitro timing
/// differs from race to race, as `Math.random` makes it in the JS.
pub fn clock_seed() -> u32 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Math::random() * 4294967296.0) as u32
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
    }
}

/// The race's frame: building it, input, ticks, drawing (the screens of
/// `crate::ui` run before it).
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlayFrame;

/// The race and what draws it.
#[derive(Resource)]
pub struct Play {
    pub params: Params,
    pub race: Option<Race>,
    /// The cars, per slot (players, rivals, the traffic pool).
    models: Option<models::Cars>,
    /// The race's effects (`race.effects`).
    fx: Option<fx::Fx>,
    /// The touch controls are shown (a touch device, or `?touch=1`).
    pub touch_ui: bool,
    pub insets: Insets,
    /// The countdown has been let go (the scene is up and compiled).
    pub started: bool,
    /// CSS px per Bevy UI logical px (the pixel-ratio override, web).
    pub css_scale: f32,
    /// CSS px per logical px of a touch or cursor position: winit's
    /// positions are device pixels, which Bevy divides by the overridden
    /// scale factor, not the device's.
    pub touch_scale: f32,
    /// A race is wanted: [`start`] builds one when there is none (the menu
    /// clears it until Race is pressed; `crate::ui`).
    pub armed: bool,
    /// The countdown waits (the field's pipelines compiling, `startRace`).
    pub hold: bool,
    /// Take the race and its cars away (`race.dispose()`).
    pub stop: bool,
    /// The player's headlight spot's intensity this frame (`Race.update`:
    /// `this.headlight.intensity = lightsOn * 140`).
    headlight: f64,
}

/// A car's root entity.
#[derive(Component)]
pub(super) struct RaceCar;

pub fn plugin(app: &mut App) {
    let (on, params) = {
        let o = &app.world().resource::<Opts>().o;
        (o.race_on(), Params::from_options(o))
    };
    // `?fx=<scene>`: a staged effect scene in a fly-camera station.
    fx_stage::plugin(app);
    // `?pv=<scene>`: a staged pursuit scene in a fly-camera station.
    pv_stage::plugin(app);
    if !on {
        return;
    }
    let touch_ui = params.touch.unwrap_or(false);
    app.insert_resource(Play {
        params,
        race: None,
        models: None,
        fx: None,
        touch_ui,
        insets: Insets::default(),
        started: false,
        css_scale: 1.0,
        touch_scale: 1.0,
        armed: true,
        hold: false,
        stop: false,
        headlight: 0.0,
    })
    .add_systems(Startup, (hud::spawn, touch_ui::spawn))
    .add_systems(
        Update,
        (start, read_input, step, draw)
            .chain()
            .in_set(PlayFrame)
            .run_if(in_state(AppState::Running)),
    )
    .add_systems(
        Update,
        effects_frame
            .after(draw)
            .in_set(PlayFrame)
            .run_if(in_state(AppState::Running)),
    )
    .add_systems(
        Update,
        (hud::update, touch_ui::update, touch_ui::sizes)
            .after(draw)
            .run_if(in_state(AppState::Running)),
    )
    .add_systems(
        PostUpdate,
        headlight
            .after(bevy::transform::TransformSystems::Propagate)
            .before(crate::render::lighting::pack_globals),
    );
    audio::plugin(app);
    hud::plugin(app);
    gamepad_io::plugin(app);
    tilt::plugin(app);
    police::plugin(app);
    #[cfg(target_arch = "wasm32")]
    web::plugin(app);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, shots.after(draw));
}

/// Makes the race once the level's Track is known (Seaside after its
/// survey), and its cars.
#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    mut play: ResMut<Play>,
    tr: Res<TrackRes>,
    shared: Res<SharedImages>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<ThreeMaterial>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut lights: ResMut<MaterialLights>,
    cars: Query<Entity, With<RaceCar>>,
    cams: Query<&Projection, With<Camera3d>>,
) {
    if play.stop {
        // `race.dispose()`: the field's cars go with the race, and the
        // effects (the flames with the cars).
        for e in &cars {
            commands.entity(e).despawn();
        }
        if let Some(f) = play.fx.take() {
            for e in f.entities {
                commands.entity(e).despawn();
            }
        }
        play.race = None;
        play.models = None;
        play.started = false;
        play.stop = false;
    }
    if play.race.is_some() || !play.armed {
        return;
    }
    // The new field's light setters take their slots afresh (D456).
    lights.values.clear();
    let Some(level) = &tr.level else { return };
    let lr = match LevelRuntime::new(level.clone()) {
        Ok(lr) => lr,
        Err(e) => {
            warn!("race: {e}");
            return;
        }
    };
    let layout = windows.single().map_or(Layout::default(), |w| {
        Layout::new(
            f64::from(w.width() * play.css_scale),
            f64::from(w.height() * play.css_scale),
            play.insets,
        )
    });
    let mut tc = TouchControls::new(layout);
    tc.show(play.touch_ui);
    let setup = Setup {
        opts: play.params.race_opts(),
        autodrive: play.params.autodrive,
        touch: play.touch_ui,
    };
    info!(
        "race: {} in the {} car, seed {}{}",
        level.id,
        setup.opts.car,
        setup.opts.seed,
        if setup.autodrive { ", autodrive" } else { "" }
    );
    let mut race = Race::new(lr, setup, tc);
    race.set_pursuit_opts(flow::PursuitOpts {
        cops: play.params.cops,
        flash: play.params.flash,
    });
    let st = &race.session.curr;
    let mut wants = Vec::new();
    for p in &st.players {
        // Race.js: `buildVehicle(carKind, { color, lod: 'high', seed: 1 })`.
        wants.push(models::Want {
            kind: p.v.kind,
            color: p.spec.color,
            seed: 1,
            lod: Lod::High,
            far: false,
            racer: true,
        });
    }
    for (i, r) in st.rivals.iter().enumerate() {
        wants.push(models::Want {
            kind: r.k.v.kind,
            color: r.color,
            seed: 10 + i as u32,
            lod: Lod::High,
            far: false,
            racer: true,
        });
    }
    for (i, c) in st.traffic.cars.iter().enumerate() {
        // Traffic.js: `buildVehicle(k, { color, seed: i * 17 + k.length,
        // lod: 'low', far: true })`.
        wants.push(models::Want {
            kind: c.kind_name,
            color: c.k.v.color,
            seed: (i * 17 + c.kind_name.len()) as u32,
            lod: Lod::Low,
            far: true,
            racer: false,
        });
    }
    // Hot Pursuit: the police cars and sawhorses follow the field.
    let extras = st
        .pv
        .as_ref()
        .map_or(Vec::new(), |pv| police::extras(&pv.pursuit));
    let cars = models::spawn_field(
        &wants,
        &extras,
        &mut commands,
        &mut meshes,
        &mut images,
        &mut mats,
        &shared,
    );
    for (c, w) in cars.cars.iter().zip(&wants) {
        commands
            .entity(c.root)
            .insert((RaceCar, Name::new(format!("car {}", w.kind))));
    }
    // `new Effects(...)`, `addCar` for each car, and `resize` with the
    // drawing buffer's height and the camera's field of view at the start.
    let height = windows
        .single()
        .map_or(800.0, |w| w.physical_height() as f32);
    let fov = cams.iter().next().map_or(62.0, |p| match p {
        Projection::Perspective(pp) => pp.fov.to_degrees(),
        _ => 62.0,
    });
    play.fx = if play.params.fx {
        fx::Fx::spawn(
            &mut commands,
            fx::Assets3 {
                meshes: &mut meshes,
                images: &mut images,
                mats: &mut mats,
                shared: &shared,
                lights: &mut lights,
            },
            &fx::CarSpec::of(&cars, st.players.len()),
            setup.opts.seed,
            (height, fov),
        )
    } else {
        None
    };
    play.models = Some(cars);
    play.race = Some(race);
}

/// Keys, focus, touches (and the mouse as a finger when the touch controls
/// are on) into the input layer and the touch controls; on the results,
/// Enter or a tap races again; while paused, a tap resumes.
#[allow(clippy::too_many_arguments)]
fn read_input(
    mut play: ResMut<Play>,
    mut keys: MessageReader<KeyboardInput>,
    mut focus: MessageReader<WindowFocused>,
    mut touches: MessageReader<TouchInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut cursor: MessageReader<CursorMoved>,
    mut last_cursor: Local<Option<Vec2>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let play = &mut *play;
    let scale = f64::from(play.touch_scale);
    let css = f64::from(play.css_scale);
    let touch_ui = play.touch_ui;
    let insets = play.insets;
    let opts_for_restart = play.params.clone();
    let Some(race) = play.race.as_mut() else {
        keys.clear();
        touches.clear();
        return;
    };
    if let Ok(w) = windows.single() {
        let lay = Layout::new(
            f64::from(w.width()) * css,
            f64::from(w.height()) * css,
            insets,
        );
        if lay != race.touch.layout {
            race.touch.resize(lay);
        }
    }
    // The controls show only while driving (`touch.show(!name && race)`).
    race.touch.show(touch_ui && race.mode == Mode::Race);
    let mut again = false;
    let mut tapped = false;
    for k in keys.read() {
        let Some(code) = input::dom_code(k.key_code) else {
            continue;
        };
        match k.state {
            ButtonState::Pressed => {
                race.input.key_down(code, k.repeat);
                if !k.repeat && (code == "Enter" || code == "NumpadEnter") {
                    again = true;
                }
            }
            ButtonState::Released => race.input.key_up(code),
        }
    }
    for f in focus.read() {
        if !f.focused {
            race.input.blur();
            race.touch.release();
        }
    }
    // A finger's event; true for a tap on nothing in particular.
    let finger = |race: &mut Race, phase: TouchPhase, id: u64, p: Vec2| -> bool {
        let (x, y) = (f64::from(p.x) * scale, f64::from(p.y) * scale);
        match phase {
            TouchPhase::Started => {
                if let Some(name) = race.touch.down(id, x, y) {
                    race.input.press(name);
                    return false;
                }
                return true;
            }
            TouchPhase::Moved => race.touch.moved(id, x, y),
            TouchPhase::Ended | TouchPhase::Canceled => race.touch.up(id),
        }
        false
    };
    for t in touches.read() {
        tapped |= finger(race, t.phase, t.id, t.position);
    }
    // The mouse is a finger too while the touch controls show (pointer
    // events in the JS).
    for c in cursor.read() {
        *last_cursor = Some(c.position);
        if touch_ui && buttons.is_empty() {
            finger(race, TouchPhase::Moved, MOUSE_ID, c.position);
        }
    }
    for b in buttons.read() {
        if b.button != MouseButton::Left {
            continue;
        }
        let Some(p) = *last_cursor else { continue };
        if touch_ui {
            let phase = match b.state {
                ButtonState::Pressed => TouchPhase::Started,
                ButtonState::Released => TouchPhase::Ended,
            };
            tapped |= finger(race, phase, MOUSE_ID, p);
        } else if b.state == ButtonState::Pressed {
            tapped = true;
        }
    }
    // The screens' buttons take taps (`crate::ui`); Enter still races
    // again or resumes.
    let _ = tapped;
    match race.mode {
        Mode::Results if again => {
            let mut opts = race.setup.opts;
            opts.seed = opts_for_restart.seed.unwrap_or_else(clock_seed);
            race.restart(opts);
        }
        Mode::Paused if again => race.pause(false),
        _ => {}
    }
}

/// `Traffic.js`: past FAR_OUT metres from the camera a car switches to its
/// far model, and back inside FAR_IN.
const FAR_OUT: f64 = 95.0;
const FAR_IN: f64 = 85.0;

/// The pointer id the mouse uses as a finger.
const MOUSE_ID: u64 = u64::MAX;

/// Runs the frame's ticks, once the scene is up and compiled.
fn step(time: Res<Time>, mut play: ResMut<Play>, status: Res<Status>) {
    let play = &mut *play;
    if !status.ready || play.hold {
        return;
    }
    let Some(race) = play.race.as_mut() else {
        return;
    };
    play.started = true;
    let dt = f64::from(time.delta_secs()) * play.params.timescale;
    race.frame(dt);
}

type CarFilter = (With<RaceCar>, Without<Camera3d>);
pub(super) type BodyFilter = (Without<RaceCar>, Without<Camera3d>, Without<SkyDome>);
type CamFilter = (With<Camera3d>, Without<RaceCar>, Without<SkyDome>);
type SkyFilter = (With<SkyDome>, Without<RaceCar>, Without<Camera3d>);

/// Draws the frame: every car at its interpolated pose with its springs,
/// the camera, and the sky and fog at the player's place.
#[allow(clippy::too_many_arguments)]
fn draw(
    time: Res<Time>,
    mut play: ResMut<Play>,
    mut cars: Query<(&mut Transform, &mut Visibility), CarFilter>,
    mut bodies: Query<&mut Transform, BodyFilter>,
    mut cam: Query<(&mut Transform, &mut Projection), CamFilter>,
    mut sky: Query<&mut Transform, SkyFilter>,
    mut node_vis: Query<&mut Visibility, Without<RaceCar>>,
    (mut mats, mut lights): (ResMut<Assets<ThreeMaterial>>, ResMut<MaterialLights>),
    windows: Query<&Window, With<PrimaryWindow>>,
    opts: Res<Opts>,
    mut cs: ResMut<CameraState>,
    mut status: ResMut<Status>,
    mut sky_res: ResMut<SkyRes>,
    mut lighting: ResMut<Lighting>,
    mut env: ResMut<EnvRequest>,
) {
    let play = &mut *play;
    let Some(race) = play.race.as_mut() else {
        return;
    };
    let paused = race.mode == Mode::Paused || !play.started;
    let dt = if paused {
        0.0
    } else {
        (f64::from(time.delta_secs()) * play.params.timescale).min(session::MAX_FRAME)
    };
    let s = &race.session;
    let track = s.lr.track.clone();
    let alpha = s.alpha();
    let prev: Vec<_> = flow::slots(&s.prev).collect();
    let Some(models) = play.models.as_mut() else {
        return;
    };
    let roots: Vec<Entity> = models.cars.iter().map(|c| c.root).collect();
    for (i, ((v, active), &root_e)) in flow::slots(&s.curr).zip(roots.iter()).enumerate() {
        let Ok((mut t, mut vis)) = cars.get_mut(root_e) else {
            continue;
        };
        let want = if active {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        if !active {
            continue;
        }
        // A car that came onto the road this tick has no previous pose.
        let a = match prev.get(i) {
            Some((pv, true)) => pose::Pose::of(pv),
            _ => pose::Pose::of(v),
        };
        let p = pose::Pose::lerp(&track, &a, &pose::Pose::of(v), alpha);
        let (pos, q) = pose::root(&track, &p);
        *t = Transform::from_translation(pos.as_vec3()).with_rotation(q.as_quat());
        if let Some(sp) = race.springs.get_mut(i) {
            if dt > 0.0 {
                sp.step(v.accel_long, v.accel_lat, dt);
            }
            models.sync_parts(i, sp, p.speed, p.steer_angle, dt, &mut bodies);
        }
        let e = models.cars[i].model.set_brake(v.brake_light);
        models.apply(e, &mut node_vis, &mut mats, &mut lights);
    }

    // The camera.
    let aspect = windows
        .single()
        .map_or(1.6, |w| f64::from(w.width() / w.height().max(1.0)));
    let car = camera::Car::lerp(
        &track,
        &camera::Car::of(&s.prev.players[0].v),
        &camera::Car::of(&s.curr.players[0].v),
        alpha,
    );
    let st = &s.curr;
    let p0 = &st.players[0];
    let look_back = race.input.state.look_back;
    let mut view = race
        .rig
        .update(dt, &track, &car, look_back, p0.phys.nitro_active, aspect);
    if st.race.state == RaceStateKind::Countdown {
        view = race.rig.intro(dt, &car, view);
    }
    if let Ok((mut t, mut proj)) = cam.single_mut() {
        *t = Transform::from_translation(view.eye.as_vec3())
            .looking_at(view.target.as_vec3(), Vec3::Y);
        if let Projection::Perspective(pp) = &mut *proj {
            let fov = (view.fov as f32).to_radians();
            if (pp.fov - fov).abs() > 1e-6 {
                pp.fov = fov;
            }
        }
    }
    // Lights (Race.update's visual sync): headlights after dusk, reverse
    // and boost on the player, boost on the rivals; the traffic's far
    // models past 95 m from the camera, back inside 85 (`Traffic.farLod`).
    let night = sky_res.sky.as_ref().map_or(0.0, |k| k.night);
    let lights_on = mr_math::smoothstep(0.25, 0.6, night);
    let n_racers = st.players.len() + st.rivals.len();
    for (i, (v, active)) in flow::slots(st).enumerate() {
        let racer = i < n_racers;
        if !active && !racer {
            continue;
        }
        let m = &mut models.cars[i].model;
        let mut e = m.set_headlights(lights_on.max(if racer { 0.15 } else { 0.1 }));
        if i < st.players.len() {
            e.extend(m.set_reverse(st.players[i].phys.gear == -1));
            e.extend(m.set_boost(if st.players[i].phys.nitro_active {
                1.0
            } else {
                0.0
            }));
        } else if racer {
            let r = &st.rivals[i - st.players.len()];
            e.extend(m.set_boost(if r.nitro_active { 1.0 } else { 0.0 }));
        } else {
            let (dx, dz) = (v.x - view.eye.x, v.z - view.eye.z);
            let lim = if m.is_far() { FAR_IN } else { FAR_OUT };
            e.extend(m.set_far(dx * dx + dz * dz > lim * lim));
        }
        models.apply(e, &mut node_vis, &mut mats, &mut lights);
    }
    play.headlight = lights_on * HEADLIGHT.intensity;

    // The world around the player (`world.update(dt, s, focus)`).
    cs.focus = DVec3::new(car.x, car.y, car.z);
    status.s = car.s;
    // `loader::follow_focus`: the sky dome stays round the focus.
    for mut t in sky.iter_mut() {
        t.translation = cs.focus.as_vec3();
    }
    let pixel_ratio = windows
        .single()
        .map_or(1.0, |w| f64::from(w.scale_factor()));
    let world_dt = if opts.o.freeze { 0.0 } else { dt };
    crate::update_sky(
        &mut sky_res,
        &opts,
        (world_dt, pixel_ratio),
        car.s,
        cs.focus,
        &mut lighting,
        &mut env,
    );
}

/// `Race.update`'s effects, once a frame after the cars are drawn
/// (`effects.update(dt, night, this.extras)`; [`fx`]).
#[allow(clippy::too_many_arguments)]
fn effects_frame(
    time: Res<Time>,
    mut play: ResMut<Play>,
    sky_res: Res<SkyRes>,
    mut parts: Query<(&mut Transform, &mut Visibility), With<fx::FxPart>>,
    mut lights: ResMut<MaterialLights>,
    mut mats: ResMut<Assets<ThreeMaterial>>,
    writes: Res<crate::animate::MeshWrites>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<&Projection, With<Camera3d>>,
) {
    let play = &mut *play;
    let (Some(race), Some(models), Some(f)) =
        (play.race.as_ref(), play.models.as_ref(), play.fx.as_mut())
    else {
        return;
    };
    // `main.js`'s resize handler: the new height, the field of view now.
    if let Ok(w) = windows.single() {
        let fov = cams.iter().next().map_or(62.0, |p| match p {
            Projection::Perspective(pp) => pp.fov.to_degrees(),
            _ => 62.0,
        });
        f.resize(w.physical_height() as f32, fov, &mut mats);
    }
    let dt = if race.mode == Mode::Paused || !play.started {
        0.0
    } else {
        (f64::from(time.delta_secs()) * play.params.timescale).min(session::MAX_FRAME)
    };
    let night = sky_res.sky.as_ref().map_or(0.0, |k| k.night);
    fx::frame(f, race, models, dt, night, &mut parts, &mut lights, &writes);
}

/// `Race.js`'s headlights for the player, "one real spotlight": `new
/// THREE.SpotLight(0xfff2d6, 0, 140, 0.55, 0.55, 1.2)` (colour, intensity,
/// distance, angle, penumbra, decay), no shadow, on the model's
/// `headlightAnchor` at its origin, aimed at (0, -2.2, 30) in the anchor's
/// frame; `intensity` here is the full one, `lightsOn` × 140.
struct Headlight {
    color: u32,
    intensity: f64,
    distance: f64,
    angle: f64,
    penumbra: f64,
    decay: f64,
    target: [f32; 3],
}

const HEADLIGHT: Headlight = Headlight {
    color: 0xfff2d6,
    intensity: 140.0,
    distance: 140.0,
    angle: 0.55,
    penumbra: 0.55,
    decay: 1.2,
    target: [0.0, -2.2, 30.0],
};

/// The player's headlight spot where its anchor is this frame (after the
/// transforms propagate, as three reads `matrixWorld` when it renders),
/// with the intensity `draw` set; none without a race (`race.dispose()`
/// removes it, D760).
fn headlight(
    play: Option<Res<Play>>,
    anchors: Query<&GlobalTransform>,
    mut lighting: ResMut<Lighting>,
) {
    let want = play.as_deref().and_then(|p| {
        p.race.as_ref()?;
        let e = p.models.as_ref()?.cars.first()?.headlight?;
        let m = anchors.get(e).ok()?.affine();
        let h = &HEADLIGHT;
        Some(crate::render::lighting::Spot {
            color: crate::render::sky::hex_color(h.color),
            intensity: p.headlight,
            position: m.transform_point3(Vec3::ZERO).as_dvec3(),
            target: m.transform_point3(Vec3::from_array(h.target)).as_dvec3(),
            distance: h.distance,
            decay: h.decay,
            angle: h.angle,
            penumbra: h.penumbra,
        })
    });
    if lighting.headlight != want {
        lighting.headlight = want;
    }
}

/// `shots=<dir>` natively: the countdown (a second in), the race (twenty
/// seconds in, chase view) and the results (a second after they show), as
/// PNGs, then exit.
#[cfg(not(target_arch = "wasm32"))]
fn shots(
    mut commands: Commands,
    play: Res<Play>,
    mut stage: Local<u32>,
    mut wait: Local<u32>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(dir) = &play.params.shots else {
        return;
    };
    let Some(race) = &play.race else { return };
    if !play.started {
        return;
    }
    let st = &race.session.curr;
    let due = match *stage {
        0 => st.race.state == RaceStateKind::Countdown && st.race.countdown < 2.9,
        1 => st.race.time >= 20.0,
        2 => race.mode == Mode::Results,
        3 => {
            *wait += 1;
            if *wait > 30 {
                exit.write(bevy::app::AppExit::Success);
            }
            return;
        }
        _ => return,
    };
    if !due {
        *wait = 0;
        return;
    }
    // Let the HUD lay out first.
    *wait += 1;
    if *wait < 8 {
        return;
    }
    *wait = 0;
    let name = ["countdown", "race", "results"][*stage as usize];
    let _ = std::fs::create_dir_all(dir);
    let path = std::path::Path::new(dir).join(format!("{name}.png"));
    info!("shot: {}", path.display());
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
    *stage += 1;
}

/// `--smoke-race`: the level's race from the grid to the results, headless
/// (no window, no GPU), with the autopilot driving through the client's
/// own frame loop at 60 frames a second; prints the results. Seed 1 unless
/// `seed=` says otherwise, so the run is the recordings' race.
#[cfg(not(target_arch = "wasm32"))]
pub fn smoke_race_cli(o: &crate::options::Options) -> Result<(), String> {
    let params = Params::from_options(o);
    let mut level = mr_levels::level_by_id(&o.level);
    if !mr_levels::levels().iter().any(|l| l.id == o.level) {
        return Err(format!("--smoke-race: no level {}", o.level));
    }
    if o.level == "seaside" {
        let p = crate::native::repo_root().join("assets/seaside/survey.bin");
        let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        let d = mr_levels::SeasideData::parse(&bytes)?;
        mr_levels::seaside::prepare(&mut level, std::sync::Arc::new(d));
    }
    let lr = LevelRuntime::new(level)?;
    let opts = RaceOpts {
        seed: params.seed.unwrap_or(1),
        ..params.race_opts()
    };
    let t0 = std::time::Instant::now();
    let smoke = flow::smoke_race(lr, opts, 1.0 / 60.0)?;
    println!(
        "smoke race: {} in the {} car, seed {}: {} ticks, {:.2} s of race, results at {:.1} s, hash {:016x} ({:.1} s wall)",
        o.level,
        opts.car,
        opts.seed,
        smoke.ticks,
        smoke.race_time,
        f64::from(smoke.ticks) / 120.0,
        smoke.hash,
        t0.elapsed().as_secs_f64()
    );
    println!("shown: {}", smoke.centers.join(" · "));
    for r in &smoke.results {
        println!(
            "{:>2}. {:<8} {}{}{}",
            r.place,
            r.name,
            if r.estimated { "~" } else { " " },
            flow::fmt_time(Some(r.time)),
            if r.player { "  <- you" } else { "" }
        );
    }
    Ok(())
}
