//! The beacons of events at places (docs/vision/peninsula.md, stage 5): a
//! column of light over each start of an open level's events, and a pool
//! of it on the road where the car stops to begin one, coloured by what it
//! is (a race orange, a time trial blue, a delivery green), so a cruise
//! finds them from down the road. A delivery's drop has one while the
//! delivery runs; the starts' show while cruising.
//!
//! One entity per beacon, its mesh built once in the beacon's own frame
//! (the entity at its foot, so transparency sorts by beacon), drawn as the
//! guide line is: unlit, vertex colours with alpha, both faces, no depth
//! write, but added to what is behind it.

use super::Play;
use super::flow::Mode;
use crate::render::SharedImages;
use crate::render::material::{Model, Patch, ThreeBindings, ThreeKey, ThreeMaterial, ThreeParams};
use crate::warmup::{Combos, Layout};
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Vec4;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mp_scene::three;
use mp_sim::event;
use mp_track::{EventKind, Track};

/// The column: its radius and height (metres), its sides.
const RADIUS: f32 = 1.6;
const HEIGHT: f32 = 70.0;
const SIDES: usize = 16;
/// The pool on the road: its radius, and how far over the road.
const POOL: f32 = 5.5;
const LIFT: f32 = 0.08;
/// Opacity at the column's foot (it fades to nothing at the top) and at
/// the pool's middle.
const FOOT: f32 = 0.4;
const POOL_ALPHA: f32 = 0.35;

fn linear(c: [u8; 3]) -> Vec3 {
    let f = |u: u8| {
        let s = f32::from(u) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    Vec3::new(f(c[0]), f(c[1]), f(c[2]))
}

/// A kind's colour, `0xrrggbb` (the minimap's dots too).
pub fn srgb(kind: EventKind) -> u32 {
    match kind {
        EventKind::Race => 0xff781e,
        EventKind::TimeTrial => 0x28beff,
        EventKind::Delivery => 0x3ceb6e,
    }
}

/// A kind's colour, linear RGB.
pub fn colour(kind: EventKind) -> Vec3 {
    let c = srgb(kind);
    linear([(c >> 16) as u8, (c >> 8) as u8, c as u8])
}

/// A beacon's mesh about its foot: the column (a ring of quads fading
/// upward) and the pool (a fan fading outward), in `c`.
fn beacon_mesh(c: Vec3) -> Mesh {
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut col: Vec<[f32; 4]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let tau = std::f32::consts::TAU;
    for k in 0..=SIDES {
        let a = tau * k as f32 / SIDES as f32;
        let (x, z) = (a.cos() * RADIUS, a.sin() * RADIUS);
        pos.push([x, 0.0, z]);
        col.push([c.x, c.y, c.z, FOOT]);
        pos.push([x, HEIGHT, z]);
        col.push([c.x, c.y, c.z, 0.0]);
    }
    for k in 0..SIDES as u32 {
        let (a, b) = (k * 2, k * 2 + 2);
        idx.extend([a, b, a + 1, a + 1, b, b + 1]);
    }
    let centre = pos.len() as u32;
    pos.push([0.0, LIFT, 0.0]);
    col.push([c.x, c.y, c.z, POOL_ALPHA]);
    for k in 0..=SIDES {
        let a = tau * k as f32 / SIDES as f32;
        pos.push([a.cos() * POOL, LIFT, a.sin() * POOL]);
        col.push([c.x, c.y, c.z, 0.0]);
    }
    for k in 0..SIDES as u32 {
        idx.extend([centre, centre + 1 + k, centre + 2 + k]);
    }
    let n = pos.len();
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    m
}

/// Unlit, vertex colours with their alpha, added, both faces, no depth
/// write, fog.
fn beacon_material(shared: &SharedImages) -> ThreeMaterial {
    ThreeMaterial::new(ThreeBindings {
        params: ThreeParams {
            diffuse: Vec4::ONE,
            ..ThreeParams::default()
        },
        map: None,
        emissive_map: None,
        detail: None,
        aux: None,
        photo: None,
        loose: None,
        env: shared.env.clone(),
        key: ThreeKey {
            model: Model::Basic,
            fog: true,
            side: three::DOUBLE_SIDE,
            shadow_side: three::DOUBLE_SIDE,
            blending: three::ADDITIVE_BLENDING,
            depth_write: false,
            depth_test: true,
            patch: Patch::None,
            sort_rank: 6,
            ..ThreeKey::default()
        },
    })
}

/// Where a start's beacon stands: at the front of its marker, on the
/// right-hand shoulder (so a car stopped on the marker sees the column
/// just ahead, beside the road, not round it; the pool reaches across
/// the lane).
pub fn start_foot(t: &Track, e: &mp_track::EventSpot) -> Vec3 {
    let s = e.s + event::SPOT_AHEAD;
    let f = t.frame(s);
    let p = t.point_at(s, f.hw + 1.0);
    Vec3::new(p.x as f32, p.y as f32, p.z as f32)
}

/// What a beacon marks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marks {
    /// Event `i`'s start.
    Start(usize),
    /// Event `i`'s drop.
    Drop(usize),
}

#[derive(Component)]
pub struct Beacon;

/// The race's beacons.
#[derive(Resource, Default)]
pub struct Beacons {
    /// The level they were made for, and each beacon.
    level: Option<&'static str>,
    drawn: Vec<(Entity, Marks)>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Beacons>().add_systems(
        Update,
        (
            spawn.after(super::start).before(super::read_input),
            show.after(super::draw),
        )
            .in_set(super::PlayFrame)
            .run_if(in_state(crate::loader::AppState::Running)),
    );
}

/// The beacons with a race on a level that has events at places, gone
/// with it. Their pipeline is warmed up as the guide line's is.
fn spawn(
    mut commands: Commands,
    play: Res<Play>,
    mut b: ResMut<Beacons>,
    shared: Res<SharedImages>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<ThreeMaterial>>,
    alive: Query<(), With<Beacon>>,
) {
    let level = play.race.as_ref().map(|r| r.session.lr.level.id);
    let gone = b.drawn.iter().any(|(e, _)| !alive.contains(*e));
    if level == b.level && !gone && (level.is_some() || b.drawn.is_empty()) {
        return;
    }
    for (e, _) in b.drawn.drain(..) {
        if alive.contains(e) {
            commands.entity(e).despawn();
        }
    }
    b.level = level;
    let Some(race) = play.race.as_ref() else {
        return;
    };
    let lr = &race.session.lr;
    if lr.level.events.is_empty() {
        return;
    }
    let m = beacon_material(&shared);
    let key = m.key;
    let material = mats.add(m);
    let mut combos = Combos::default();
    let mut put = |commands: &mut Commands, foot: Vec3, kind: EventKind, marks: Marks| {
        let cpu = beacon_mesh(colour(kind));
        combos.note(key, &material, &Layout::of(&cpu), false);
        let entity = commands
            .spawn((
                Mesh3d(meshes.add(cpu)),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(foot),
                Visibility::Hidden,
                NotShadowCaster,
                NotShadowReceiver,
                Beacon,
                crate::loader::SceneEntity,
                Name::new("beacon"),
            ))
            .id();
        (entity, marks)
    };
    let t = &*lr.track;
    for (i, e) in lr.level.events.iter().enumerate() {
        let d = put(&mut commands, start_foot(t, e), e.kind, Marks::Start(i));
        b.drawn.push(d);
        if let Some([x, z]) = e.drop {
            // On whichever road the drop is.
            let (road, q) = t.project_road(x, z, 0, 0.0);
            let y = t.road(road).ground_y(x, z, q.s, q.lat);
            let d = put(
                &mut commands,
                Vec3::new(x as f32, y as f32, z as f32),
                e.kind,
                Marks::Drop(i),
            );
            b.drawn.push(d);
        }
    }
    combos.spawn(&mut commands, &mut meshes);
}

/// The starts while cruising, a delivery's drop while it runs.
fn show(play: Res<Play>, b: Res<Beacons>, mut q: Query<&mut Visibility, With<Beacon>>) {
    let Some(race) = play.race.as_ref() else {
        return;
    };
    let st = &race.session.curr;
    let cruising = st.race.cruise && race.mode != Mode::Results;
    let running = st.race.event.as_ref().map(|e| e.index);
    for &(e, marks) in &b.drawn {
        let on = match marks {
            Marks::Start(_) => cruising,
            Marks::Drop(i) => running == Some(i) && !st.players[0].rules.finished,
        };
        if let Ok(mut v) = q.get_mut(e) {
            let want = if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *v != want {
                *v = want;
            }
        }
    }
}
