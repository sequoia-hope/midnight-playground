//! The staged effect scenes (roadmap WP 4.4's L4 gate, DECISIONS D804):
//! with `?fx=<name>` and a fly camera, the scene of that name from
//! `parity/golden/effects/scenes.json` is drawn by [`super::fx`] once the
//! level is up, as `tools/parity/effects-scenes.mjs` draws it with the
//! JS's `Effects.js`: the cars on the road, the frames run at once with
//! the scene's random stream, the bursts before each frame's update, then
//! held. `__mr.fxStaged` (web) counts the frames drawn since.

use super::effects::{CarIn, Extras};
use super::fx::{Assets3, CarSpec, Fx, FxPart};
use crate::Opts;
use crate::animate::MeshWrites;
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::ThreeMaterial;
use crate::status::Status;
use bevy::camera::Projection;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use mr_math::kernel;
use serde_json::Value;

/// The scene definitions (`tools/parity/effects-scenes.mjs` reads the same).
pub const SCENES_JSON: &str = include_str!("../../../../parity/golden/effects/scenes.json");

/// The staged scene once made, and the frames drawn since.
#[derive(Resource, Default)]
struct Staged {
    fx: Option<Fx>,
    frames: u32,
    failed: bool,
}

pub fn plugin(app: &mut App) {
    if matches!(
        app.world().resource::<Opts>().o.param("fx"),
        None | Some("0")
    ) {
        return;
    }
    app.init_resource::<Staged>().add_systems(Update, stage);
}

fn num(v: &Value, k: &str, d: f64) -> f64 {
    v.get(k).and_then(Value::as_f64).unwrap_or(d)
}

/// A staged car at frame `k`: on the road at `s + speed × k × dt`, `lat`
/// across, heading along the road plus `yaw`.
fn pose(track: &mr_track::Track, c: &Value, k: f64, dt: f64) -> CarIn {
    let speed = num(c, "speed", 0.0);
    let s = num(c, "s", 0.0) + speed * k * dt;
    let f = track.frame(s);
    let lat = num(c, "lat", 0.0);
    let yaw = kernel::atan2(f.fz, f.fx) + num(c, "yaw", 0.0);
    CarIn {
        x: f.x + f.rx * lat,
        y: f.y,
        z: f.z + f.rz * lat,
        yaw,
        visual_yaw: 0.0,
        vx: kernel::cos(yaw) * speed,
        vz: kernel::sin(yaw) * speed,
        on_ground: true,
        visible: true,
        wheel_base: num(c, "wheelBase", 0.0),
        track: num(c, "track", 0.0),
    }
}

/// The asset stores the effects are made in.
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
    cams: Query<&Projection, With<Camera3d>>,
    mut parts: Query<(&mut Transform, &mut Visibility), With<FxPart>>,
) {
    let staged = &mut *staged;
    if let Some(fx) = &staged.fx {
        fx.place(&mut parts, &mut lights);
        staged.frames += 1;
        publish(staged.frames);
        return;
    }
    if staged.failed || !status.ready {
        return;
    }
    let Some(track) = tr.track.as_ref() else {
        return;
    };
    let name = opts.o.param("fx").unwrap_or_default();
    let defs: Value = serde_json::from_str(SCENES_JSON).expect("scenes.json parses");
    let Some(def) = defs["scenes"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["name"] == name))
        .cloned()
    else {
        warn!("fx: no staged scene {name}");
        staged.failed = true;
        return;
    };
    let dt = num(&def, "dt", 1.0 / 60.0);
    let frames = num(&def, "frames", 1.0) as u32;
    let night = num(&def, "night", 1.0);
    let cars = def["cars"].as_array().cloned().unwrap_or_default();
    let last = f64::from(frames.saturating_sub(1));
    // A body at each car's last pose (`body.rotation.y = π/2 − yaw`).
    let mut specs = Vec::new();
    for c in &cars {
        let p = pose(track, c, last, dt);
        let body = commands
            .spawn((
                Transform::from_xyz(p.x as f32, p.y as f32, p.z as f32).with_rotation(
                    Quat::from_rotation_y((std::f64::consts::FRAC_PI_2 - p.yaw) as f32),
                ),
                Visibility::Inherited,
                crate::loader::SceneEntity,
                Name::new("fx stage body"),
            ))
            .id();
        let exhausts = c["exhausts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|e| {
                        let g = |i: usize| e.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                        [g(0), g(1), g(2)]
                    })
                    .collect()
            })
            .unwrap_or_default();
        specs.push(CarSpec {
            player: c["player"].as_bool().unwrap_or(false),
            exhausts,
            body: Some(body),
        });
    }
    let height = windows
        .single()
        .map_or(800.0, |w| w.physical_height() as f32);
    let fov = cams.iter().next().map_or(62.0, |p| match p {
        Projection::Perspective(pp) => pp.fov.to_degrees(),
        _ => 62.0,
    });
    let seed = num(&def, "seed", 1.0) as u32;
    let Some(mut fx) = Fx::spawn(
        &mut commands,
        Assets3 {
            meshes: &mut meshes,
            images: &mut images,
            mats: &mut mats,
            shared: &shared,
            lights: &mut lights,
        },
        &specs,
        seed,
        (height, fov),
    ) else {
        staged.failed = true;
        return;
    };
    // The scene's stream from its first frame (`Math.random =
    // mulberry32(seed)` in the JS page).
    fx.fx.reseed(seed);
    let extras: Vec<Extras> = cars
        .iter()
        .map(|c| Extras {
            nitro: c["nitro"].as_bool().unwrap_or(false),
            skid: num(c, "skid", 0.0),
            launch: c["launch"].as_bool().unwrap_or(false),
        })
        .collect();
    let bursts = def["bursts"].as_array().cloned().unwrap_or_default();
    for k in 0..frames {
        let ins: Vec<CarIn> = cars
            .iter()
            .map(|c| pose(track, c, f64::from(k), dt))
            .collect();
        for b in bursts
            .iter()
            .filter(|b| num(b, "frame", -1.0) == f64::from(k))
        {
            let f = track.frame(num(b, "s", 0.0));
            let lat = num(b, "lat", 0.0);
            let (x, y, z) = (f.x + f.rx * lat, f.y + num(b, "h", 0.0), f.z + f.rz * lat);
            let (vx, vz) = (num(b, "vx", 0.0), num(b, "vz", 0.0));
            if b["kind"] == "sparks" {
                fx.fx.sparks_at(x, y, z, num(b, "n", 0.0), vx, vz);
            } else {
                fx.fx
                    .smoke_at(x, y, z, num(b, "amount", 0.0), vx, vz, night);
            }
        }
        fx.fx.update(dt, night, &ins, &extras);
    }
    // three computes the spheres it sorts by at its first render, which
    // comes after the frames here (D808).
    fx.fix_sort_centres();
    fx.write(&writes);
    info!("fx: staged {name}, {frames} frames");
    staged.fx = Some(fx);
}

#[cfg(target_arch = "wasm32")]
fn publish(frames: u32) {
    use js_sys::Reflect;
    use wasm_bindgen::JsValue;
    if let Some(w) = web_sys::window()
        && let Ok(mr) = Reflect::get(&w, &JsValue::from_str("__mr"))
        && mr.is_object()
    {
        let _ = Reflect::set(&mr, &JsValue::from_str("fxStaged"), &JsValue::from(frames));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn publish(_frames: u32) {}
