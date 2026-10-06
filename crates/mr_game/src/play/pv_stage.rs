//! The staged Hot Pursuit scenes (roadmap WP 8.1 and 8.2's L4 gate,
//! DECISIONS D930): with `?pv=<name>` and a fly camera, the scene of that
//! name from `parity/golden/pursuit/scenes.json` is drawn by
//! [`super::police`] and [`super::fx`] once the level is up, as
//! `tools/parity/pursuit-scenes.mjs` draws it with the JS's own
//! `PursuitView.js`: a pursuit from the scene's seeded streams, the units
//! activated where the scene puts them, a roadblock or a spike strip laid,
//! a player car on the road, then the frames run at once (PursuitView's
//! clock, the events due, the police sync, its smoke and sparks, the
//! effects' update) and held. `__mr.pvStaged` (web) counts the frames drawn
//! since.

use super::effects::{CarIn, Extras};
use super::fx::{Assets3, CarSpec, Fx, FxPart};
use super::models::{self, Want};
use super::police::{self, EmitIn, Frame, PoliceDrawn, PoliceView, SharedLight};
use super::pose::{self, Pose, Springs};
use super::{BodyFilter, CarFilter, RaceCar};
use crate::Opts;
use crate::animate::MeshWrites;
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::ThreeMaterial;
use crate::status::Status;
use bevy::camera::Projection;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use mr_math::{kernel, smoothstep};
use mr_sim::body::BodyId;
use mr_sim::police::{Mode, Siren};
use mr_sim::pursuit::{Pursuit, PursuitEvent, PursuitOpts, RacerBody, Racers, top_speed};
use mr_sim::race::SimEvent;
use mr_sim::rng::RngStreams;
use mr_sim::vehicle::Vehicle;
use serde_json::Value;

/// The scene definitions (`tools/parity/pursuit-scenes.mjs` reads the same).
pub const SCENES_JSON: &str = include_str!("../../../../parity/golden/pursuit/scenes.json");

/// The staged scene once made, and the frames drawn since.
#[derive(Resource, Default)]
struct Staged {
    made: Option<Made>,
    /// The frames have run.
    done: bool,
    frames: u32,
    failed: bool,
}

/// The scene made, its entities spawned (the frames run the next frame,
/// once the commands have made them).
struct Made {
    def: Value,
    pu: Pursuit,
    player: Vehicle,
    cars: models::Cars,
    fx: Fx,
    view: PoliceView,
    eye: (f64, f64),
}

pub fn plugin(app: &mut App) {
    if app.world().resource::<Opts>().o.param("pv").is_none() {
        return;
    }
    app.init_resource::<Staged>()
        .init_resource::<PoliceDrawn>()
        .init_resource::<SharedLight>()
        .add_systems(Update, (stage, place).chain())
        .add_systems(
            PostUpdate,
            police::light
                .after(bevy::transform::TransformSystems::Propagate)
                .before(crate::render::lighting::pack_globals),
        );
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(d)
}

/// The player's body as the pursuit reads it (`placeSpikes` reads its lat).
struct OnePlayer(RacerBody);

impl Racers for OnePlayer {
    fn body(&self, _id: BodyId) -> RacerBody {
        self.0
    }
    fn ai_finished(&self, _id: BodyId) -> bool {
        false
    }
    fn hold_ai(&mut self, _id: BodyId, _seconds: f64, _hold_lat: f64) {}
    fn spike(&mut self, _id: BodyId) {}
}

fn mode_of(s: &str) -> Mode {
    match s {
        "parked" => Mode::Parked,
        "oncoming" => Mode::Oncoming,
        "search" => Mode::Search,
        "standdown" => Mode::Standdown,
        "hold" => Mode::Hold,
        "disabled" => Mode::Disabled,
        "block" => Mode::Block,
        _ => Mode::Chase,
    }
}

fn siren_of(s: &str) -> Siren {
    match s {
        "flash" => Siren::Flash,
        "disabled" => Siren::Disabled,
        _ => Siren::Off,
    }
}

/// A car as the effects see it, standing where it is.
fn car_in(v: &Vehicle, visible: bool, wheel_base: f64, track: f64) -> CarIn {
    CarIn {
        x: v.x,
        y: v.y,
        z: v.z,
        yaw: v.yaw,
        visual_yaw: v.visual_yaw,
        vx: v.vx,
        vz: v.vz,
        on_ground: true,
        visible,
        wheel_base,
        track,
    }
}

/// The asset stores the scene is made in.
type Stores<'w> = (
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<Image>>,
    ResMut<'w, Assets<ThreeMaterial>>,
);

#[allow(clippy::too_many_arguments)]
fn stage(
    mut commands: Commands,
    mut staged: ResMut<Staged>,
    opts: Res<Opts>,
    status: Res<Status>,
    tr: Res<crate::TrackRes>,
    (mut meshes, mut images, mut mats): Stores<'_>,
    shared: Res<SharedImages>,
    mut lights: ResMut<MaterialLights>,
    writes: Res<MeshWrites>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<(&Projection, &Transform), With<Camera3d>>,
    mut roots: Query<(&mut Transform, &mut Visibility), CarFilter>,
    mut bodies: Query<&mut Transform, BodyFilter>,
    mut node_vis: Query<&mut Visibility, Without<RaceCar>>,
    (mut drawn, mut shared_light): (ResMut<PoliceDrawn>, ResMut<SharedLight>),
) {
    let staged = &mut *staged;
    if staged.done || staged.failed || !status.ready {
        return;
    }
    if let Some(m) = staged.made.as_mut() {
        let Some(track) = tr.track.as_ref() else {
            return;
        };
        run(
            m,
            track,
            &mut roots,
            &mut bodies,
            &mut node_vis,
            &mut mats,
            &mut lights,
            &mut drawn.0,
            &mut commands,
            &mut meshes,
        );
        shared_light.0 = m.view.shared_light();
        // three computes the spheres it sorts by at its first render (D808).
        m.fx.fix_sort_centres();
        m.fx.write(&writes);
        staged.done = true;
        info!("pv: staged {}", m.def["name"].as_str().unwrap_or_default());
        return;
    }
    let (Some(track), Some(level)) = (tr.track.as_ref(), tr.level.as_ref()) else {
        return;
    };
    let name = opts.o.param("pv").unwrap_or_default();
    let defs: Value = serde_json::from_str(SCENES_JSON).expect("scenes.json parses");
    let Some(def) = defs["scenes"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["name"] == name))
        .cloned()
    else {
        warn!("pv: no staged scene {name}");
        staged.failed = true;
        return;
    };
    let seed = num(&def, "seed", 1.0) as u32;
    let flash = def["flash"].as_bool().unwrap_or(true);
    let hq = def["hq"].as_bool().unwrap_or(true);
    let p = &def["player"];
    let kind = mr_sim::physics::CAR_SPECS
        .iter()
        .map(|(k, _)| *k)
        .find(|k| Some(*k) == p["kind"].as_str())
        .unwrap_or("sports");
    let spec = mr_sim::physics::car_spec(kind).expect("a car");
    let color = num(p, "color", f64::from(spec.color)) as u32;

    // The pursuit, from the scene's streams (PursuitView's constructor).
    let mut rng = RngStreams::new(seed);
    let RngStreams {
        pursuit: rp,
        police: rpo,
        ..
    } = &mut rng;
    let mut pu = Pursuit::new(
        track,
        level,
        PursuitOpts {
            heat: num(&def, "heat", 1.0),
            max_units: 6.0,
            player_top: top_speed(&spec),
            flash,
        },
        rp,
        rpo,
    );
    pu.set_racers(vec![(BodyId::Player(0), true, false, "You")]);
    let mut player = Vehicle::new(
        mr_sim::dims::dims(kind).expect("dims"),
        kind,
        spec.mass,
        "You",
        color,
    );
    player.place(track, num(p, "s", 0.0), num(p, "lat", 0.0), 0.0);
    let speed = num(p, "speed", 0.0);
    player.vx = kernel::cos(player.yaw) * speed;
    player.vz = kernel::sin(player.yaw) * speed;
    player.on_ground = true;
    for u in def["units"].as_array().into_iter().flatten() {
        let i = num(u, "unit", 0.0) as usize;
        pu.activate(
            track,
            i,
            num(u, "s", 0.0),
            num(u, "lat", 0.0),
            num(u, "speed", 0.0),
            mode_of(u["mode"].as_str().unwrap_or("chase")),
            num(u, "dir", 1.0) as i32,
        );
        pu.units[i].siren = siren_of(u["siren"].as_str().unwrap_or("off"));
    }
    if let Some(r) = def.get("roadblock").filter(|v| v.is_object()) {
        pu.place_roadblock(track, num(r, "s", 0.0), rp);
    }
    if let Some(sp) = def.get("spikes").filter(|v| v.is_object()) {
        let body = RacerBody {
            s: player.s,
            lat: player.lat,
            ..RacerBody::default()
        };
        pu.place_spikes(track, &OnePlayer(body), num(sp, "s", 0.0));
    }
    pu.events.clear();

    // The player's car and PursuitView's, then the effects with every car
    // (`addCar`: the player, then the police).
    let wants = [Want {
        kind,
        color,
        seed: 1,
        lod: mr_worldgen::car_model::Lod::High,
        far: false,
        racer: true,
    }];
    let cars = models::spawn_field(
        &wants,
        &police::extras(&pu),
        &mut commands,
        &mut meshes,
        &mut images,
        &mut mats,
        &shared,
    );
    for c in &cars.cars {
        commands.entity(c.root).insert(RaceCar);
    }
    let height = windows
        .single()
        .map_or(800.0, |w| w.physical_height() as f32);
    let (fov, eye) = cams.iter().next().map_or((62.0, (0.0, 0.0)), |(p, t)| {
        (
            match p {
                Projection::Perspective(pp) => pp.fov.to_degrees(),
                _ => 62.0,
            },
            (f64::from(t.translation.x), f64::from(t.translation.z)),
        )
    });
    let Some(mut fx) = Fx::spawn(
        &mut commands,
        Assets3 {
            meshes: &mut meshes,
            images: &mut images,
            mats: &mut mats,
            shared: &shared,
            lights: &mut lights,
        },
        &CarSpec::of(&cars, 1),
        seed,
        (height, fov),
    ) else {
        staged.failed = true;
        return;
    };
    fx.fx.reseed(seed);
    let view = PoliceView::new(&pu, hq && flash);
    staged.made = Some(Made {
        def,
        pu,
        player,
        cars,
        fx,
        view,
        eye,
    });
}

/// The scene's frames, once its entities exist: per frame PursuitView's
/// clock, the events due, `player.sync`, `pv.sync` (the models and the
/// light, then its smoke and sparks), `effects.update`; then the spike
/// strip.
#[allow(clippy::too_many_arguments)]
fn run(
    m: &mut Made,
    track: &mr_track::Track,
    roots: &mut Query<(&mut Transform, &mut Visibility), CarFilter>,
    bodies: &mut Query<&mut Transform, BodyFilter>,
    node_vis: &mut Query<&mut Visibility, Without<RaceCar>>,
    mats: &mut Assets<ThreeMaterial>,
    lights: &mut MaterialLights,
    drawn: &mut Vec<bool>,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
) {
    let def = &m.def;
    let p = &def["player"];
    let dt = num(def, "dt", 1.0 / 60.0);
    let frames = num(def, "frames", 1.0) as u32;
    let night = num(def, "night", 1.0);
    let flash = def["flash"].as_bool().unwrap_or(true);
    let damage = num(p, "damage", 0.0);
    let spiked = num(p, "spiked", 0.0);
    let (pu, player, cars) = (&m.pu, &mut m.player, &mut m.cars);
    let mut springs = Springs::default();
    let n = pu.units.len() + pu.block_cars.len();
    for k in 0..frames {
        let t = num(def, "t", 0.0) + f64::from(k) * dt;
        let mut log = Vec::new();
        for e in def["events"].as_array().into_iter().flatten() {
            if num(e, "frame", -1.0) != f64::from(k) {
                continue;
            }
            let f = track.frame(num(e, "s", 0.0));
            let lat = num(e, "lat", 0.0);
            let (x, z) = (f.x + f.rx * lat, f.z + f.rz * lat);
            log.push(SimEvent::Pursuit(match e["type"].as_str() {
                Some("barrier") => PursuitEvent::Barrier { x, z, player: true },
                _ => PursuitEvent::Uturn { unit: 0, x, z },
            }));
        }
        // `player.sync(t, dt)`.
        let pp = Pose::of(player);
        if let Ok((mut tf, mut vis)) = roots.get_mut(cars.cars[0].root) {
            let (pos, q) = pose::root(track, &pp);
            *tf = Transform::from_translation(pos.as_vec3()).with_rotation(q.as_quat());
            *vis = Visibility::Inherited;
        }
        springs.step(player.accel_long, player.accel_lat, dt);
        cars.sync_parts(0, &springs, pp.speed, pp.steer_angle, dt, bodies);
        let e = cars.cars[0].model.set_brake(player.brake_light);
        cars.apply(e, node_vis, mats, lights);
        // `pv.sync(dt, night, lightsOn)`: the models and the light, ...
        let f = Frame {
            track,
            prev: pu,
            curr: pu,
            alpha: 1.0,
            t,
            dt,
            night,
            lights_on: smoothstep(0.25, 0.6, night),
            flash,
            player: (player.x, player.z),
            eye: m.eye,
        };
        m.view
            .sync(&f, cars, roots, bodies, node_vis, mats, lights, drawn);
        // ... its smoke and sparks (the events' first), ...
        let dims = &cars.cars[0].model.dims;
        let pin = car_in(player, true, dims.wheel_base, dims.track);
        police::emit(
            &mut m.fx.fx,
            &EmitIn {
                log: &log,
                pursuit: pu,
                damage,
                spiked,
                player: &pin,
                dt,
                night,
            },
        );
        // ... and the barrier's slowdown after its sparks.
        if log
            .iter()
            .any(|e| matches!(e, SimEvent::Pursuit(PursuitEvent::Barrier { .. })))
        {
            player.vx *= 0.97;
            player.vz *= 0.97;
        }
        // `effects.update(dt, night, extras)`.
        let mut ins = vec![pin];
        for (i, car) in (0..n).zip(cars.cars.iter().skip(cars.police_base)) {
            let u = pu.police(i);
            ins.push(car_in(
                &u.k.v,
                u.active,
                car.model.dims.wheel_base,
                car.model.dims.track,
            ));
        }
        // The player's entry only: the police have none (D945).
        let extras = [Extras::default()];
        m.fx.fx.update(dt, night, &ins, &extras);
        // The pools on the road (D1042).
        let mut hints = vec![player.s];
        hints.extend((0..n).map(|i| pu.police(i).k.v.s));
        m.fx.fx.lay_pools(track, &hints);
    }
    m.view.sync_spikes(
        commands,
        track,
        pu.spikes.as_ref(),
        cars.spike_material.as_ref(),
        meshes,
    );
}

/// Each frame after: the effects' parts placed, the frames counted.
fn place(
    mut staged: ResMut<Staged>,
    mut parts: Query<(&mut Transform, &mut Visibility), With<FxPart>>,
    mut lights: ResMut<MaterialLights>,
) {
    let staged = &mut *staged;
    let Some(m) = staged.made.as_ref().filter(|_| staged.done) else {
        return;
    };
    m.fx.place(&mut parts, &mut lights);
    staged.frames += 1;
    publish(staged.frames);
}

#[cfg(target_arch = "wasm32")]
fn publish(frames: u32) {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    if let Some(w) = web_sys::window()
        && let Ok(mr) = Reflect::get(&w, &JsValue::from_str("__mr"))
        && mr.is_object()
    {
        let _ = Reflect::set(&mr, &JsValue::from_str("pvStaged"), &JsValue::from(frames));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish(_frames: u32) {}
