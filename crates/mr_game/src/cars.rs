//! Stand-in cars driven by the Rust simulation (roadmap WP 2.4): a race of
//! the level as `mr_sim` runs it (the player on the autopilot, the rivals,
//! the traffic pool), stepped at the fixed tick in real time, each car drawn
//! as a box of its kind's dimensions in its colour (three_std, standard
//! material). They exist to put moving, shadow-casting objects in front of
//! the renderer for gate G1's frame-time measurement until the car models
//! (`CarModel.js`) and `Vehicle.sync` are ported (M4); orientation is the
//! car's yaw only.
//!
//! On by default for a level, off with `?cars=0`, `freeze=1` (the parity
//! captures, whose JS side has no race), the material scenes and the
//! stations (`Options::cars_on`).

use crate::render::SharedImages;
use crate::render::material::{Model, ThreeKey, ThreeMaterial, ThreeParams};
use crate::render::sky::hex_color;
use crate::{Opts, TrackRes, loader::SceneEntity};
use bevy::math::Vec4;
use bevy::prelude::*;
use mr_scene::three;
use mr_sim::autopilot::autopilot;
use mr_sim::input::{Input, InputFrame};
use mr_sim::race::{DT, LevelRuntime, RaceOpts, SimState, step};
use mr_sim::vehicle::Vehicle;

/// The race behind the stand-ins, and one entity per car slot (the player,
/// the rivals, then every car of the traffic pool).
#[derive(Resource)]
pub struct Cars {
    lr: LevelRuntime,
    st: SimState,
    acc: f64,
    entities: Vec<Entity>,
    events: Vec<mr_sim::race::SimEvent>,
}

/// Marks a stand-in car.
#[derive(Component)]
pub struct StandInCar;

/// At most this many ticks per frame (a quarter of a second): after a long
/// stall the race catches up a little and drops the rest, as the JS frame
/// loop's clamp does.
const MAX_TICKS: u32 = 30;

/// Every car of the race in slot order, with whether it is on the road.
fn bodies(st: &SimState) -> impl Iterator<Item = (&Vehicle, bool)> {
    st.players
        .iter()
        .map(|p| (&p.v, true))
        .chain(st.rivals.iter().map(|r| (&r.k.v, true)))
        .chain(st.traffic.cars.iter().map(|c| (&c.k.v, c.active)))
}

/// Starts the race once the level's Track is known (Seaside after its
/// survey).
pub fn start_cars(
    mut commands: Commands,
    tr: Res<TrackRes>,
    opts: Res<Opts>,
    cars: Option<Res<Cars>>,
    shared: Res<SharedImages>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<ThreeMaterial>>,
) {
    if cars.is_some() || !opts.o.cars_on() {
        return;
    }
    let Some(level) = &tr.level else { return };
    let lr = match LevelRuntime::new(level.clone()) {
        Ok(lr) => lr,
        Err(e) => {
            warn!("stand-in cars: {e}");
            return;
        }
    };
    let st = SimState::new(
        &lr,
        RaceOpts {
            car: "sports",
            seed: 1,
            pursuit: false,
            heat: 1.0,
        },
    );
    let mut entities = Vec::new();
    for (v, active) in bodies(&st) {
        let d = &v.dims;
        let mesh = meshes.add(Cuboid::new(
            d.width as f32,
            d.height as f32,
            d.length as f32,
        ));
        let c = hex_color(v.color);
        let material = mats.add(stand_in_material(c, &shared));
        let e = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                pose(v),
                if active {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                },
                StandInCar,
                SceneEntity,
                Name::new(format!("stand-in {}", v.kind)),
            ))
            .id();
        entities.push(e);
    }
    info!("stand-in cars: {} on {}", entities.len(), level.id);
    commands.insert_resource(Cars {
        lr,
        st,
        acc: 0.0,
        entities,
        events: Vec::new(),
    });
}

/// A plain MeshStandardMaterial of the car's colour (roughness 0.45,
/// metalness 0.3).
fn stand_in_material(c: [f64; 3], shared: &SharedImages) -> ThreeMaterial {
    ThreeMaterial {
        params: ThreeParams {
            diffuse: Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, 1.0),
            pbr: Vec4::new(0.45, 0.3, 0.0, 0.0),
            physical: Vec4::new(1.5, 1.0, 0.0, 0.0),
            specular: Vec4::new(1.0, 1.0, 1.0, 1.0),
            sheen: Vec4::new(0.0, 0.0, 0.0, 1.0),
            ..ThreeParams::default()
        },
        map: None,
        emissive_map: None,
        detail: None,
        aux: None,
        photo: None,
        loose: None,
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key: ThreeKey {
            model: Model::Physical,
            fog: true,
            opaque: true,
            side: three::FRONT_SIDE,
            shadow_side: three::BACK_SIDE,
            blending: three::NO_BLENDING,
            depth_write: true,
            depth_test: true,
            ..ThreeKey::default()
        },
    }
}

/// Where a car's box sits: on the ground under it (`visY`), its length
/// along the heading (the model's +z is (cos yaw, 0, sin yaw), as
/// `Vehicle.sync` builds it).
fn pose(v: &Vehicle) -> Transform {
    let y = v.vis_y.unwrap_or(v.y) + v.dims.height * 0.5;
    let yaw = v.yaw + v.visual_yaw;
    Transform::from_xyz(v.x as f32, y as f32, v.z as f32).with_rotation(Quat::from_rotation_y(
        (std::f64::consts::FRAC_PI_2 - yaw) as f32,
    ))
}

/// Steps the race by the frame's time in fixed ticks and moves the boxes.
pub fn drive_cars(
    time: Res<Time>,
    cars: Option<ResMut<Cars>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<StandInCar>>,
) {
    let Some(mut cars) = cars else { return };
    let c = &mut *cars;
    c.acc += f64::from(time.delta_secs()).min(0.25);
    let mut ticks = 0;
    while c.acc >= DT && ticks < MAX_TICKS {
        let mut inp = Input::default();
        autopilot(&mut inp, &c.st.players[0].v, &c.lr.track);
        let frame = InputFrame::quantise(&inp);
        step(&c.lr, &mut c.st, &[frame], &mut c.events);
        c.events.clear();
        c.acc -= DT;
        ticks += 1;
    }
    if ticks == MAX_TICKS {
        c.acc = 0.0;
    }
    for ((v, active), &e) in bodies(&c.st).zip(&c.entities) {
        let Ok((mut t, mut vis)) = q.get_mut(e) else {
            continue;
        };
        let want = if active && v.alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
        *t = pose(v);
    }
}
