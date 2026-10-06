//! Hot Pursuit on screen (roadmap WP 8.1, 8.2): the per-frame visuals of
//! `src/game/PursuitView.js` (`sync` and `syncSpikes`) on the police cars,
//! sawhorses and spike strips the pursuit in `mp_sim` moves.
//!
//! - **Models**: PursuitView's `makeUnit` builds a unit's model as
//!   `MODELS[type]` (the patrol car, the muscle car in police livery, the
//!   SUV) at low detail with its far model, seeds from 300 in the order the
//!   Pursuit asks (the units, then the roadblock cars), and a sawhorse as
//!   `sawhorseModel()`; [`extras`] lists them for `models::spawn_field`.
//! - **Per frame** ([`PoliceView::sync`]): each active car's `Vehicle.sync`
//!   (its pose between the last two ticks, body springs, wheels, brake
//!   lights), `farLod` from the camera, headlights at `max(0.15,
//!   lightsOn)`, `setSiren(mode, t)` with flash turned to steady when the
//!   "Police lights flash" option is off, and the glow dimmed by day
//!   (`0.3 + 0.7 × night`); the active sawhorses' sync.
//! - **The shared light**: one red/blue PointLight, made only with the High
//!   quality and flash options on at the start (`PursuitView` constructor),
//!   at the siren anchor of the nearest flashing unit within 40 m of the
//!   player, fading out from 25 m ([`PoliceView::light`]).
//! - **Spike strips**: rebuilt when the pursuit lays a new one
//!   ([`PoliceView::sync_spikes`]).
//!
//! The smoke and sparks PursuitView sends to `Effects` (u-turn smoke,
//! barrier sparks, disabled units' smoke, damage smoke, the rims' sparks)
//! are [`emit`], run by `play::fx` before the effects' update, in the JS's
//! order. [`PoliceDrawn`] tells the test bridge which units are shown.

use super::effects::{CarIn, Effects, rate60};
use super::models::{Cars, Extra};
use super::pose::{self, Pose, Springs};
use super::{BodyFilter, CarFilter, Play, PlayFrame};
use crate::convert::{self, Draw, MeshKey};
use crate::loader::{AppState, SceneEntity};
use crate::render::Lighting;
use crate::render::lighting::{MaterialLights, Point};
use crate::render::material::ThreeMaterial;
use crate::{Opts, SkyRes};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use mp_math::{js, kernel, smoothstep};
use mp_sim::police::{Mode as UnitMode, Siren, UnitType};
use mp_sim::pursuit::{Pursuit, PursuitEvent, Spikes};
use mp_sim::race::SimEvent;
use mp_track::Track;
use mp_worldgen::car_model::SirenMode;
use mp_worldgen::object::SceneGraph;
use mp_worldgen::pursuit_props::{PropKit, spike_strip};

/// `new THREE.PointLight(0xff2030, 0, 38, 1.6)`: its distance and decay.
const LIGHT_DISTANCE: f64 = 38.0;
const LIGHT_DECAY: f64 = 1.6;

/// Per `pursuit.units` index: the unit's model is shown this frame
/// (`u.v.model.root.visible`), for the test bridge (`__pursuit`). Empty
/// outside a pursuit race.
#[derive(Resource, Default, Debug, Clone, PartialEq)]
pub struct PoliceDrawn(pub Vec<bool>);

/// `MODELS[type]`: the model kind and whether it takes the police livery.
fn model_of(t: UnitType) -> (&'static str, bool) {
    match t {
        UnitType::Patrol => ("police", false),
        UnitType::Interceptor => ("muscle", true),
        UnitType::Suv => ("policeSuv", false),
    }
}

/// What PursuitView's `makeUnit` builds, in the Pursuit's order: each unit,
/// then each roadblock car (seeds 300 on), then the sawhorses.
pub(crate) fn extras(pu: &Pursuit) -> Vec<Extra> {
    let mut out = Vec::new();
    for (seed, u) in (300..).zip(pu.units.iter().chain(&pu.block_cars)) {
        let (kind, livery) = model_of(u.unit_type);
        out.push(Extra::Police { kind, livery, seed });
    }
    out.extend(pu.sawhorses.iter().map(|_| Extra::Sawhorse));
    out
}

/// What one frame of the pursuit's visuals reads.
pub(crate) struct Frame<'a> {
    pub track: &'a Track,
    /// The pursuit at the last two ticks (the same twice when staged).
    pub prev: &'a Pursuit,
    pub curr: &'a Pursuit,
    /// Between them, as the cars are drawn (SPEC 6.5).
    pub alpha: f64,
    /// PursuitView's clock (`this.t`), for the sirens.
    pub t: f64,
    pub dt: f64,
    pub night: f64,
    /// `smoothstep(0.25, 0.6, night)`.
    pub lights_on: f64,
    /// The "Police lights flash" option (`this.flash`).
    pub flash: bool,
    /// The player's car (x, z), for the shared light.
    pub player: (f64, f64),
    /// The camera (x, z), for the far models.
    pub eye: (f64, f64),
}

/// The pursuit's drawn state (PursuitView's, beside the models).
pub(crate) struct PoliceView {
    /// Per police car (units, then roadblock cars), then per sawhorse.
    springs: Vec<Springs>,
    /// The shared flashing light exists (`hq && flash` at the start).
    pub has_light: bool,
    /// The light this frame (`None`: none near, intensity 0).
    near: Option<LightNear>,
    /// The spike strip drawn, and the placement it shows (`spikeFor`).
    spike: Option<Entity>,
    spike_for: Option<[u64; 3]>,
}

/// Where the shared light goes this frame: the entity it sits over (the
/// unit's siren anchor, else its root), how far above, its colour and
/// intensity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LightNear {
    at: Entity,
    up: f64,
    color: [f64; 3],
    intensity: f64,
}

/// The shared light as the frame left it, for [`light`] once the anchors
/// have moved: `None` without one (no pursuit, or not `hq && flash` at the
/// start), else where it is (`None` within: no unit near, intensity 0).
#[derive(Resource, Default)]
pub(crate) struct SharedLight(pub Option<Option<LightNear>>);

/// A siren mode as PursuitView turns it: `flash` is steady with the flash
/// option off.
fn siren_mode(s: Siren, flash: bool) -> SirenMode {
    match s {
        Siren::Off => SirenMode::Off,
        Siren::Flash if !flash => SirenMode::Steady,
        Siren::Flash => SirenMode::Flash,
        Siren::Disabled => SirenMode::Disabled,
    }
}

impl PoliceView {
    pub fn new(pu: &Pursuit, has_light: bool) -> PoliceView {
        PoliceView {
            springs: vec![
                Springs::default();
                pu.units.len() + pu.block_cars.len() + pu.sawhorses.len()
            ],
            has_light,
            near: None,
            spike: None,
            spike_for: None,
        }
    }

    /// PursuitView.sync's models and lights (everything but the events and
    /// the effects, which are [`emit`]).
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &mut self,
        f: &Frame,
        cars: &mut Cars,
        roots: &mut Query<(&mut Transform, &mut Visibility), CarFilter>,
        bodies: &mut Query<&mut Transform, BodyFilter>,
        node_vis: &mut Query<&mut Visibility, Without<super::RaceCar>>,
        mats: &mut Assets<ThreeMaterial>,
        lights: &mut MaterialLights,
        drawn: &mut Vec<bool>,
    ) {
        let (track, pu) = (f.track, f.curr);
        let np = pu.units.len() + pu.block_cars.len();
        let base = cars.police_base;
        drawn.clear();
        drawn.extend(pu.units.iter().map(|u| u.active));
        let mut near: Option<usize> = None;
        let mut near_d = 40.0;
        // The glow reads from 300 m at night but is a big halo up close by
        // day: dim it in daylight.
        let k = 0.3 + 0.7 * f.night;
        for i in 0..np {
            let u = pu.police(i);
            let slot = base + i;
            let Some(root) = cars.cars.get(slot).map(|c| c.root) else {
                continue;
            };
            let p = show(
                roots,
                root,
                u.active,
                || {
                    let a = f.prev.police(i);
                    let a = if a.active { &a.k.v } else { &u.k.v };
                    Pose::lerp(track, &Pose::of(a), &Pose::of(&u.k.v), f.alpha)
                },
                track,
            );
            let Some(p) = p else { continue };
            // `u.v.sync(t, dt)`: springs, wheels, steering, brake lights.
            let sp = &mut self.springs[i];
            if f.dt > 0.0 {
                sp.step(u.k.v.accel_long, u.k.v.accel_lat, f.dt);
            }
            let sp = *sp;
            cars.sync_parts(slot, &sp, p.speed, p.steer_angle, f.dt, bodies);
            let m = &mut cars.cars[slot].model;
            let mut e = m.set_brake(u.k.v.brake_light);
            // `farLod(u.v, cam.x, cam.z)`.
            let (dx, dz) = (p.x - f.eye.0, p.z - f.eye.1);
            let lim = if m.is_far() { 85.0 } else { 95.0 };
            e.extend(m.set_far(dx * dx + dz * dz > lim * lim));
            e.extend(m.set_headlights(js::max(0.15, f.lights_on)));
            e.extend(m.set_siren(siren_mode(u.siren, f.flash), f.t));
            let (lr, lb) = m.siren_color();
            cars.apply(e, node_vis, mats, lights);
            cars.set_glow(slot, lr * k, lb * k, mats, lights);
            if u.siren == Siren::Flash {
                let d = kernel::hypot(p.x - f.player.0, p.z - f.player.1);
                if d < near_d {
                    near_d = d;
                    near = Some(slot);
                }
            }
        }
        for (b, s) in pu.sawhorses.iter().enumerate() {
            let Some(root) = cars.props.get(b).map(|p| p.root) else {
                continue;
            };
            let shown = show(
                roots,
                root,
                s.active,
                || {
                    let a = &f.prev.sawhorses[b];
                    let a = if a.active { &a.k.v } else { &s.k.v };
                    Pose::lerp(track, &Pose::of(a), &Pose::of(&s.k.v), f.alpha)
                },
                track,
            );
            if shown.is_some() {
                let sp = &mut self.springs[np + b];
                if f.dt > 0.0 {
                    sp.step(s.k.v.accel_long, s.k.v.accel_lat, f.dt);
                }
                let sp = *sp;
                cars.sync_prop(b, &sp, bodies);
            }
        }
        // The shared light near the closest flashing unit.
        self.near = None;
        if self.has_light
            && let Some(slot) = near
        {
            let car = &cars.cars[slot];
            let (r, b) = car.model.siren_color();
            let kk = js::or(r + b, 0.001);
            let intensity =
                (r + b) * 70.0 * (0.25 + 0.75 * f.night) * (1.0 - smoothstep(25.0, 40.0, near_d));
            // `a.getWorldPosition(light.position)`, else the car + 1.8 m;
            // then 0.6 m up.
            let (at, up) = match car.model.siren.and_then(|s| cars.entity_of(s.anchor)) {
                Some(e) => (e, 0.6),
                None => (car.root, 1.8 + 0.6),
            };
            self.near = Some(LightNear {
                at,
                up,
                color: [r / kk, 0.08, b / kk],
                intensity,
            });
        }
    }

    /// The shared light as this frame left it.
    pub fn shared_light(&self) -> Option<Option<LightNear>> {
        self.has_light.then_some(self.near)
    }

    /// `syncSpikes()`: a new strip where the pursuit laid one, the old one
    /// gone.
    pub fn sync_spikes(
        &mut self,
        commands: &mut Commands,
        track: &Track,
        spikes: Option<&Spikes>,
        material: Option<&Handle<ThreeMaterial>>,
        meshes: &mut Assets<Mesh>,
    ) {
        let key = spikes.map(|s| [s.s.to_bits(), s.lat0.to_bits(), s.lat1.to_bits()]);
        if key == self.spike_for {
            return;
        }
        self.spike_for = key;
        self.clear(commands);
        let (Some(s), Some(mat)) = (spikes, material) else {
            return;
        };
        let mut graph = SceneGraph::new();
        let mut kit = PropKit::default();
        let n = spike_strip(&mut graph, &mut kit, track, s.s, s.lat0, s.lat1);
        graph.roots.push(n);
        let (scene, handles) = graph.finish();
        let Some(node) = handles.node(n).map(|i| &scene.nodes[i as usize]) else {
            return;
        };
        let Some(mi) = node.mesh else { return };
        let (start, count) = convert::draw_span(&scene, &scene.meshes[mi as usize], None);
        let Some(mesh) = convert::build_mesh(
            &scene,
            MeshKey {
                mesh: mi,
                start,
                count,
                draw: Draw::Triangles,
                colors: false,
                lit: true,
                extra: None,
            },
        ) else {
            return;
        };
        let e = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mat.clone()),
                Transform::from_matrix(convert::mat4(&node.matrix)),
                Visibility::Inherited,
                NotShadowCaster,
                SceneEntity,
                Name::new("spike strip"),
            ))
            .id();
        self.spike = Some(e);
    }

    /// Takes the drawn strip away (`race.dispose()`, a new placement).
    pub fn clear(&mut self, commands: &mut Commands) {
        if let Some(e) = self.spike.take() {
            commands.entity(e).despawn();
        }
    }
}

/// Shows or hides a model's root and, when shown, places it at the pose
/// `pose` gives (`Vehicle.sync`'s root).
fn show(
    roots: &mut Query<(&mut Transform, &mut Visibility), CarFilter>,
    root: Entity,
    active: bool,
    pose: impl FnOnce() -> Pose,
    track: &Track,
) -> Option<Pose> {
    let Ok((mut t, mut vis)) = roots.get_mut(root) else {
        return None;
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
        return None;
    }
    let p = pose();
    let (pos, q) = pose::root(track, &p);
    *t = Transform::from_translation(pos.as_vec3()).with_rotation(q.as_quat());
    Some(p)
}

/// What [`emit`] reads of the race this frame.
pub(crate) struct EmitIn<'a> {
    /// The ticks' events (`pu.events` as PursuitView reads them).
    pub log: &'a [SimEvent],
    pub pursuit: &'a Pursuit,
    /// PursuitView's damage, and the player's spiked tyres (`phys.spiked`).
    pub damage: f64,
    pub spiked: f64,
    /// The player's car as the effects see it.
    pub player: &'a CarIn,
    pub dt: f64,
    pub night: f64,
}

/// PursuitView.sync's smoke and sparks, in its order, before
/// `effects.update`: the events' (a u-turn's two puffs, a barrier's
/// sparks), the disabled units' smoke, the smoke from under a damaged
/// bonnet, and the sparks off spiked rims. A chance the JS takes once a
/// frame is the same chance per 1/60 s (D802).
pub(crate) fn emit(fx: &mut Effects, e: &EmitIn) {
    let v = e.player;
    for ev in e.log {
        match ev {
            SimEvent::Pursuit(PursuitEvent::Uturn { x, z, .. }) => {
                fx.smoke_at(*x, v.y, *z, 1.4, 0.0, 0.0, e.night);
                fx.smoke_at(*x, v.y, *z, 1.4, 0.0, 0.0, e.night);
            }
            SimEvent::Pursuit(PursuitEvent::Barrier { x, z, player: true }) => {
                fx.sparks_at(*x, v.y + 0.8, *z, 14.0, v.vx, v.vz);
            }
            _ => {}
        }
    }
    // Disabled units smoke.
    for u in &e.pursuit.units {
        if u.active && u.mode == UnitMode::Disabled && fx.random() < rate60(0.3, e.dt) {
            let w = &u.k.v;
            fx.smoke_at(w.x, w.y + 0.6, w.z, 0.6, 0.0, 0.0, e.night);
        }
    }
    // Damage: smoke from under the bonnet, heavier and darker as it goes.
    let (fx_, fz) = (kernel::cos(v.yaw), kernel::sin(v.yaw));
    let d = e.damage;
    if d > 0.45 && fx.random() < rate60((d - 0.35) * 1.2, e.dt) {
        fx.smoke_at(
            v.x + fx_ * 1.5,
            v.y + 0.7,
            v.z + fz * 1.5,
            d,
            v.vx,
            v.vz,
            js::min(1.0, e.night + (d - 0.6) * 2.0),
        );
    }
    // Spiked: sparks off the rims.
    if e.spiked > 0.0 && kernel::hypot(v.vx, v.vz) > 5.0 && fx.random() < rate60(0.7, e.dt) {
        for side in [-1.0, 1.0] {
            let w = Effects::wheel_world(v, side);
            fx.sparks_at(w.x, w.y + 0.15, w.z, 2.0, v.vx, v.vz);
        }
    }
}

/// The pursuit view of the race, kept beside `Play`.
#[derive(Resource, Default)]
struct State {
    view: Option<PoliceView>,
    /// `Race::starts` it belongs to (a restart is a new PursuitView).
    starts: u32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<PoliceDrawn>()
        .init_resource::<State>()
        .init_resource::<SharedLight>()
        .add_systems(
            Update,
            frame
                .after(super::draw)
                .before(super::effects_frame)
                .in_set(PlayFrame)
                .run_if(in_state(AppState::Running)),
        )
        .add_systems(
            PostUpdate,
            light
                .after(bevy::transform::TransformSystems::Propagate)
                .before(crate::render::lighting::pack_globals),
        );
}

/// The "Police lights flash" setting (`settings.flash`; on without the
/// menu).
fn flash_setting(ui: Option<&crate::ui::UiState>) -> bool {
    ui.is_none_or(|u| u.settings.flash)
}

/// One frame of PursuitView.sync's visuals, after the field is drawn and
/// before the effects.
#[allow(clippy::too_many_arguments)]
fn frame(
    mut commands: Commands,
    time: Res<Time>,
    mut play: ResMut<Play>,
    mut state: ResMut<State>,
    mut drawn: ResMut<PoliceDrawn>,
    mut shared: ResMut<SharedLight>,
    sky_res: Res<SkyRes>,
    opts: Res<Opts>,
    ui: Option<Res<crate::ui::UiState>>,
    mut roots: Query<(&mut Transform, &mut Visibility), CarFilter>,
    mut bodies: Query<&mut Transform, BodyFilter>,
    mut node_vis: Query<&mut Visibility, Without<super::RaceCar>>,
    cams: Query<&Transform, super::CamFilter>,
    (mut mats, mut lights, mut meshes): (
        ResMut<Assets<ThreeMaterial>>,
        ResMut<MaterialLights>,
        ResMut<Assets<Mesh>>,
    ),
) {
    let play = &mut *play;
    let pursuit = play
        .race
        .as_ref()
        .filter(|_| play.models.is_some())
        .and_then(|r| r.session.curr.pv.as_ref().map(|_| r.starts));
    let Some(starts) = pursuit else {
        if let Some(mut v) = state.view.take() {
            v.clear(&mut commands);
        }
        if !drawn.0.is_empty() {
            drawn.0.clear();
        }
        shared.0 = None;
        return;
    };
    let race = play.race.as_ref().expect("a race");
    let s = &race.session;
    let curr = &s.curr.pv.as_ref().expect("a pursuit").pursuit;
    if state.view.is_none() || state.starts != starts {
        if let Some(mut v) = state.view.take() {
            v.clear(&mut commands);
        }
        // `if (hq && flash)`: the light is made with the race or never.
        let light = opts.hq && flash_setting(ui.as_deref());
        state.view = Some(PoliceView::new(curr, light));
        state.starts = starts;
    }
    let paused = race.mode == super::flow::Mode::Paused || !play.started;
    let dt = if paused {
        0.0
    } else {
        (f64::from(time.delta_secs()) * play.params.timescale).min(super::session::MAX_FRAME)
    };
    let prev = s.prev.pv.as_ref().map_or(curr, |p| &p.pursuit);
    let alpha = s.alpha();
    let lerp = |a: f64, b: f64| a + (b - a) * alpha;
    let pt = s.prev.pv.as_ref().map_or(0.0, |p| p.t);
    let ct = s.curr.pv.as_ref().map_or(0.0, |p| p.t);
    let night = sky_res.sky.as_ref().map_or(0.0, |k| k.night);
    let (a, b) = (&s.prev.players[0].v, &s.curr.players[0].v);
    let eye = cams.iter().next().map_or((0.0, 0.0), |t| {
        (f64::from(t.translation.x), f64::from(t.translation.z))
    });
    let f = Frame {
        track: &s.lr.track,
        prev,
        curr,
        alpha,
        t: lerp(pt, ct),
        dt,
        night,
        lights_on: smoothstep(0.25, 0.6, night),
        flash: flash_setting(ui.as_deref()),
        player: (lerp(a.x, b.x), lerp(a.z, b.z)),
        eye,
    };
    let Some(models) = play.models.as_mut() else {
        return;
    };
    let view = state.view.as_mut().expect("a view");
    view.sync(
        &f,
        models,
        &mut roots,
        &mut bodies,
        &mut node_vis,
        &mut mats,
        &mut lights,
        &mut drawn.0,
    );
    shared.0 = view.shared_light();
    view.sync_spikes(
        &mut commands,
        &s.lr.track,
        curr.spikes.as_ref(),
        models.spike_material.as_ref(),
        &mut meshes,
    );
}

/// The shared light onto the lighting, once the anchors have moved (as
/// three reads `matrixWorld` when it renders).
pub(crate) fn light(
    shared: Res<SharedLight>,
    anchors: Query<&GlobalTransform>,
    mut lighting: ResMut<Lighting>,
    mut had: Local<bool>,
) {
    let want = shared.0.map(|near| {
        let dark = Point {
            color: [1.0, 0.08, 0.0],
            intensity: 0.0,
            position: bevy::math::DVec3::ZERO,
            distance: LIGHT_DISTANCE,
            decay: LIGHT_DECAY,
        };
        let Some(n) = near else { return dark };
        let Ok(g) = anchors.get(n.at) else {
            return dark;
        };
        Point {
            color: n.color,
            intensity: n.intensity,
            position: g.translation().as_dvec3() + bevy::math::DVec3::Y * n.up,
            ..dark
        }
    });
    // Only the pursuit's light: no level has a point light of its own.
    if want.is_none() && !*had {
        return;
    }
    *had = want.is_some();
    if lighting.point != want {
        lighting.point = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_sim::autopilot::autopilot;
    use mp_sim::input::{Input, InputFrame};
    use mp_sim::race::{LevelRuntime, RaceOpts, SimState, step};

    fn coast(heat: f64) -> (LevelRuntime, SimState) {
        let lr = LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap();
        let st = SimState::new(
            &lr,
            RaceOpts {
                car: "sports",
                seed: 1,
                pursuit: true,
                heat,
            },
        );
        (lr, st)
    }

    /// PursuitView's `makeUnit` order and seeds: the seven units, the five
    /// roadblock cars (seeds 300 to 311), then the two sawhorses.
    #[test]
    fn extras_follow_make_unit() {
        let (_, st) = coast(3.0);
        let ex = extras(&st.pv.as_ref().unwrap().pursuit);
        let police: Vec<_> = ex
            .iter()
            .filter_map(|e| match e {
                Extra::Police { kind, livery, seed } => Some((*kind, *livery, *seed)),
                Extra::Sawhorse => None,
            })
            .collect();
        assert_eq!(police.len(), 12);
        assert_eq!(police[0], ("police", false, 300));
        assert_eq!(police[3], ("muscle", true, 303));
        assert_eq!(police[5], ("policeSuv", false, 305));
        assert_eq!(police[7], ("police", false, 307));
        assert_eq!(police[11], ("policeSuv", false, 311));
        assert_eq!(ex[12..], [Extra::Sawhorse, Extra::Sawhorse]);
    }

    /// The siren modes PursuitView draws: flash is steady with the option
    /// off; disabled and off stay.
    #[test]
    fn siren_modes() {
        assert_eq!(siren_mode(Siren::Flash, true), SirenMode::Flash);
        assert_eq!(siren_mode(Siren::Flash, false), SirenMode::Steady);
        assert_eq!(siren_mode(Siren::Disabled, false), SirenMode::Disabled);
        assert_eq!(siren_mode(Siren::Off, true), SirenMode::Off);
    }

    /// Particles alive: (smoke, sparks).
    fn live(fx: &Effects) -> (usize, usize) {
        let n = |l: &[f32], a: &[f32]| l.iter().zip(a).filter(|(l, a)| **l > **a).count();
        (
            n(&fx.smoke.life, &fx.smoke.age),
            n(&fx.sparks.life, &fx.sparks.age),
        )
    }

    /// The smoke and sparks PursuitView adds: none in a quiet frame, a
    /// u-turn's two puffs, smoke from a damaged car (its chance per 1/60 s),
    /// sparks off spiked rims only above 5 m/s.
    #[test]
    fn emits_as_pursuit_view() {
        let (_, st) = coast(1.0);
        let pu = &st.pv.as_ref().unwrap().pursuit;
        let car = CarIn {
            x: 10.0,
            y: 1.0,
            z: 5.0,
            visible: true,
            on_ground: true,
            wheel_base: 2.6,
            track: 1.6,
            ..CarIn::default()
        };
        let input = |log, damage, spiked, player| EmitIn {
            log,
            pursuit: pu,
            damage,
            spiked,
            player,
            dt: 1.0 / 60.0,
            night: 1.0,
        };
        let mut fx = Effects::new(3);
        emit(&mut fx, &input(&[], 0.4, 0.0, &car));
        assert_eq!(live(&fx), (0, 0));
        let log = [SimEvent::Pursuit(PursuitEvent::Uturn {
            unit: 0,
            x: 1.0,
            z: 2.0,
        })];
        emit(&mut fx, &input(&log, 0.0, 0.0, &car));
        assert_eq!(live(&fx), (2, 0));
        // At full damage the chance is 0.78 a frame: smoke within 20.
        let mut fx = Effects::new(3);
        for _ in 0..20 {
            emit(&mut fx, &input(&[], 1.0, 0.0, &car));
        }
        assert!(live(&fx).0 > 0);
        // Spiked but stopped: no sparks; at speed, two from each rim.
        let mut fx = Effects::new(3);
        for _ in 0..20 {
            emit(&mut fx, &input(&[], 0.0, 2.0, &car));
        }
        assert_eq!(live(&fx).1, 0);
        let fast = CarIn { vx: 20.0, ..car };
        for _ in 0..20 {
            emit(&mut fx, &input(&[], 0.0, 2.0, &fast));
        }
        let n = live(&fx).1;
        assert!(n >= 4 && n.is_multiple_of(4), "{n} sparks");
    }

    /// When a seeded pursuit on Coast lays its props, for the race pictures
    /// (`tools/parity/pursuit-race.mjs`): printed, not asserted.
    #[test]
    #[ignore]
    fn coast_props_times() {
        let (lr, mut st) = coast(3.0);
        let mut ev = Vec::new();
        for _ in 0..120 * 200 {
            let mut inp = Input::default();
            autopilot(&mut inp, &st.players[0].v, &lr.track);
            ev.clear();
            step(&lr, &mut st, &[InputFrame::quantise(&inp)], &mut ev);
            for e in &ev {
                if let SimEvent::Pursuit(p) = e
                    && matches!(
                        p,
                        PursuitEvent::Roadblock { .. }
                            | PursuitEvent::Spikes { .. }
                            | PursuitEvent::Spiked { .. }
                            | PursuitEvent::Barrier { .. }
                            | PursuitEvent::Uturn { .. }
                            | PursuitEvent::Takedown { .. }
                            | PursuitEvent::Dodge { .. }
                    )
                {
                    println!("{:.2} s: {p:?}", st.race.time);
                }
            }
        }
    }
}
