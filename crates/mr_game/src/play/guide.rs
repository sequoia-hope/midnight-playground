//! The guide line (Rust only, the owner's request of 2026-10-05; DECISIONS
//! D1080, D1081, D1083): chevrons on the racing line ahead of the player,
//! coloured by how soon they have to brake for each one
//! ([`mr_sim::assist::brake_urgency`]: green, orange, red), fading out
//! with distance. Full shows the whole line, Braking only only where it
//! turns orange or red, Off nothing.
//!
//! One mesh of [`CHEVRONS`] chevrons, six vertices each, drawn with an
//! unlit transparent material (vertex colours with alpha, double-sided,
//! no depth write, the skid marks' polygon offset) a few centimetres over
//! the road. The chevrons sit at fixed places on the road (every
//! [`SPACING`] metres of arc length), so they stay put as the car passes
//! them; each frame the visible ones are written into the mesh's place in
//! the vertex slab (`animate::MeshWrites`), the rest collapsed to nothing.
//!
//! This module also hands the settings (or `?line=`, `?assist=`) to the
//! race: the guide's mode here, the steering assist on `Race::assist`.

use super::Play;
use super::flow::Mode;
use crate::animate::MeshWrites;
use crate::render::SharedImages;
use crate::render::material::{Model, Patch, ThreeKey, ThreeMaterial, ThreeParams};
use crate::warmup::{Combos, Layout};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Vec4;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use mr_scene::three;
use mr_sim::assist::{Assist, brake_urgency};
use mr_track::Track;

/// The guide line's setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuideMode {
    #[default]
    Off,
    /// Only where the line says brake (orange or red).
    Brake,
    Full,
}

impl GuideMode {
    /// `full`, `brake`, `off` (the store and `?line=`).
    pub fn parse(s: &str) -> Option<GuideMode> {
        match s {
            "full" | "1" => Some(GuideMode::Full),
            "brake" => Some(GuideMode::Brake),
            "off" | "0" => Some(GuideMode::Off),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            GuideMode::Off => "off",
            GuideMode::Brake => "brake",
            GuideMode::Full => "full",
        }
    }
}

/// Metres of road between chevrons.
pub const SPACING: f64 = 3.0;
/// Chevrons in the mesh: the longest line, 300 m, and one to spare.
pub const CHEVRONS: usize = 102;
/// The line's reach: 5.5 s of road at the car's speed, 140 to 300 m.
fn reach(speed: f64) -> f64 {
    (speed * 5.5).clamp(140.0, 300.0)
}
/// A chevron: its width across the road, how far its tip leads its arms'
/// ends, and its arms' thickness along the road (metres).
const WIDTH: f64 = 1.1;
const TIP: f64 = 0.55;
const THICK: f64 = 0.32;
/// Over the road surface, as the skid marks are (`Effects.js`: +0.03), a
/// little more because a chevron spans more of a bend.
const LIFT: f64 = 0.05;
/// The line's opacity at its strongest.
const OPACITY: f32 = 0.82;

/// The colours, as sRGB bytes: green (fine), orange (brake soon), red
/// (brake now).
const GREEN: [u8; 3] = [40, 225, 95];
const ORANGE: [u8; 3] = [255, 150, 20];
const RED: [u8; 3] = [255, 35, 35];

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

/// The colour for an urgency of 0 (green) to 1 (orange) to 2 (red), in
/// linear RGB.
pub fn colour(u: f64) -> Vec3 {
    let u = u.clamp(0.0, 2.0) as f32;
    if u <= 1.0 {
        linear(GREEN).lerp(linear(ORANGE), u)
    } else {
        linear(ORANGE).lerp(linear(RED), u - 1.0)
    }
}

/// The chevron's six corners (across the road, along it): the tip's outer
/// and inner corners, then each arm's outer and inner ends.
const CORNERS: [(f64, f64); 6] = [
    (0.0, TIP),
    (0.0, TIP - THICK),
    (-WIDTH / 2.0, 0.0),
    (-WIDTH / 2.0, -THICK),
    (WIDTH / 2.0, 0.0),
    (WIDTH / 2.0, -THICK),
];
const TRIS: [u32; 12] = [0, 2, 1, 1, 2, 3, 0, 1, 4, 1, 5, 4];

fn guide_mesh() -> Mesh {
    let n = CHEVRONS * 6;
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32; 3]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0f32, 1.0, 0.0]; n]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0f32; 4]; n]);
    let idx: Vec<u32> = (0..CHEVRONS as u32)
        .flat_map(|k| TRIS.map(|i| k * 6 + i))
        .collect();
    m.insert_indices(Indices::U32(idx));
    m
}

/// Unlit, vertex colours with their alpha, transparent, no depth write,
/// both faces, fog; the skid marks' polygon offset keeps it over the road.
fn guide_material(shared: &SharedImages) -> ThreeMaterial {
    ThreeMaterial {
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
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key: ThreeKey {
            model: Model::Basic,
            fog: true,
            side: three::DOUBLE_SIDE,
            shadow_side: three::DOUBLE_SIDE,
            blending: three::NORMAL_BLENDING,
            depth_write: false,
            depth_test: true,
            depth_bias: 256,
            depth_slope: 4,
            patch: Patch::None,
            // After the effects' ranks (D808): drawn over the skid marks
            // where they tie.
            sort_rank: 6,
            ..ThreeKey::default()
        },
    }
}

/// The line's entity.
#[derive(Component)]
pub struct GuideLine;

/// What draws the line for the race on screen.
struct Drawn {
    entity: Entity,
    mesh: Handle<Mesh>,
    cpu: Mesh,
    /// Whether the last write left any chevron showing.
    shown: bool,
}

/// The guide line and the settings it and the assist follow.
#[derive(Resource, Default)]
pub struct Guide {
    drawn: Option<Drawn>,
    pub mode: GuideMode,
    /// What the last frame drew: chevrons shown and their urgencies' range
    /// (for the test bridge).
    pub stats: Stats,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub shown: usize,
    pub max_urgency: f64,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Guide>().add_systems(
        Update,
        (
            spawn.after(super::start).before(super::read_input),
            settings.before(super::step),
            draw.after(super::draw),
        )
            .in_set(super::PlayFrame)
            .run_if(in_state(crate::loader::AppState::Running)),
    );
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Last, bridge);
}

/// `__mr.aids`: the guide's mode, the assist on the race, and what the
/// line showed last frame (`shown` chevrons, the highest urgency), for
/// the test scripts.
#[cfg(target_arch = "wasm32")]
fn bridge(play: Option<Res<Play>>, guide: Res<Guide>) {
    use wasm_bindgen::JsValue;
    let Some(w) = web_sys::window() else { return };
    let Ok(mr) = js_sys::Reflect::get(&w, &JsValue::from_str("__mr")) else {
        return;
    };
    if !mr.is_object() {
        return;
    }
    let o = js_sys::Object::new();
    let set = |k: &str, v: JsValue| {
        let _ = js_sys::Reflect::set(&o, &JsValue::from_str(k), &v);
    };
    set("guide", JsValue::from_str(guide.mode.name()));
    let assist = play
        .as_ref()
        .and_then(|p| p.race.as_ref())
        .map_or("off", |r| r.assist.name());
    set("assist", JsValue::from_str(assist));
    set("shown", JsValue::from_f64(guide.stats.shown as f64));
    set("maxUrgency", JsValue::from_f64(guide.stats.max_urgency));
    let _ = js_sys::Reflect::set(&mr, &JsValue::from_str("aids"), &o);
}

/// The line's entity with a race, gone with it (`play.stop` takes the
/// race away; a new one gets a new line). Its pipeline is warmed up with
/// the field's (D458).
fn spawn(
    mut commands: Commands,
    play: Res<Play>,
    mut guide: ResMut<Guide>,
    shared: Res<SharedImages>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<ThreeMaterial>>,
) {
    if play.race.is_none() {
        if let Some(d) = guide.drawn.take() {
            commands.entity(d.entity).despawn();
        }
        return;
    }
    if guide.drawn.is_some() {
        return;
    }
    let cpu = guide_mesh();
    let layout = Layout::of(&cpu);
    let mesh = meshes.add(cpu.clone());
    let m = guide_material(&shared);
    let key = m.key;
    let material = mats.add(m);
    let mut combos = Combos::default();
    combos.note(key, &material, &layout, false);
    combos.spawn(&mut commands, &mut meshes);
    let entity = commands
        .spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            Visibility::Hidden,
            NoFrustumCulling,
            NotShadowCaster,
            NotShadowReceiver,
            GuideLine,
            crate::loader::SceneEntity,
            Name::new("guide line"),
        ))
        .id();
    guide.drawn = Some(Drawn {
        entity,
        mesh,
        cpu,
        shown: false,
    });
}

/// The settings (or the query's `line=` and `assist=`) onto the race:
/// the guide's mode, and the assist, which the autopilot never gets.
fn settings(mut play: ResMut<Play>, ui: Option<Res<crate::ui::UiState>>, mut guide: ResMut<Guide>) {
    let play = &mut *play;
    let touch = play.touch_ui;
    let (g, a) = ui.as_ref().map_or_else(
        || {
            if touch {
                ("full", "light")
            } else {
                ("off", "off")
            }
        },
        |u| (u.settings.guide.as_str(), u.settings.assist.as_str()),
    );
    guide.mode = play
        .params
        .line
        .or_else(|| GuideMode::parse(g))
        .unwrap_or_default();
    let assist = play
        .params
        .assist
        .or_else(|| Assist::parse(a))
        .unwrap_or_default();
    if let Some(race) = play.race.as_mut() {
        race.assist = if race.setup.autodrive {
            Assist::Off
        } else {
            assist
        };
    }
}

/// One chevron's place on the road: arc length `s` and the line there,
/// interpolated between samples.
fn line_at(t: &Track, s: f64) -> f64 {
    let (i, f, j, _) = t.locate(s);
    let a = f64::from(t.racing_line[i]);
    let b = f64::from(t.racing_line[j]);
    a + (b - a) * f
}

/// Writes the chevrons for this frame: where the player is drawn (between
/// the last two ticks), at their speed.
fn draw(
    play: Res<Play>,
    mut guide: ResMut<Guide>,
    writes: Res<MeshWrites>,
    mut q: Query<(&mut Transform, &mut Visibility), With<GuideLine>>,
) {
    let guide = &mut *guide;
    let mode = guide.mode;
    let Some(d) = guide.drawn.as_mut() else {
        return;
    };
    let Some(race) = play.race.as_ref() else {
        return;
    };
    let s = &race.session;
    let p0 = &s.curr.players[0];
    let on =
        mode != GuideMode::Off && race.mode != Mode::Results && !p0.rules.finished && play.started;
    let Ok((mut tf, mut vis)) = q.get_mut(d.entity) else {
        return;
    };
    if !on {
        if *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        }
        guide.stats = Stats::default();
        return;
    }
    let track = &*s.lr.track;
    let alpha = s.alpha();
    let (a, b) = (&s.prev.players[0].v, &p0.v);
    let ps = a.s + track.ds(a.s, b.s) * alpha;
    let speed = {
        let l = |p: f64, q: f64| p + (q - p) * alpha;
        mr_math::kernel::hypot(l(a.vx, b.vx), l(a.vz, b.vz))
    };
    let reach = reach(speed);
    // The sort point: the middle of the line's reach (D808's way: the
    // entity sits there and the vertices are written relative to it).
    let mid = track.point_at(ps + reach * 0.5, line_at(track, ps + reach * 0.5));
    let centre = Vec3::new(mid.x as f32, mid.y as f32, mid.z as f32);
    let end = if track.is_loop {
        f64::INFINITY
    } else {
        track.length
    };
    let first = (ps / SPACING).floor() + 1.0;
    let mut stats = Stats::default();
    let (pos, col) = {
        let mut pos = None;
        let mut col = None;
        for (id, v) in d.cpu.attributes_mut() {
            match v {
                VertexAttributeValues::Float32x3(x) if id.id == Mesh::ATTRIBUTE_POSITION.id => {
                    pos = Some(x)
                }
                VertexAttributeValues::Float32x4(x) if id.id == Mesh::ATTRIBUTE_COLOR.id => {
                    col = Some(x)
                }
                _ => {}
            }
        }
        let (Some(p), Some(c)) = (pos, col) else {
            return;
        };
        (p, c)
    };
    for k in 0..CHEVRONS {
        let sc = (first + k as f64) * SPACING;
        let dist = sc - ps;
        let verts = k * 6..k * 6 + 6;
        // Near the car it would only sit under it or the camera.
        let fade = mr_math::smoothstep(3.0, 9.0, dist)
            * (1.0 - mr_math::smoothstep(reach * 0.55, reach, dist));
        let u = brake_urgency(speed, f64::from(track.speed_profile[track.idx(sc)]), dist);
        let a = fade
            * match mode {
                GuideMode::Full => 1.0,
                _ => u.min(1.0),
            };
        if a <= 0.004 || sc > end {
            for i in verts {
                pos[i] = [0.0; 3];
                col[i] = [0.0; 4];
            }
            continue;
        }
        stats.shown += 1;
        stats.max_urgency = stats.max_urgency.max(u);
        let c = colour(u);
        let rgba = [c.x, c.y, c.z, OPACITY * a as f32];
        for (i, &(x, y)) in verts.zip(CORNERS.iter()) {
            let sv = sc + y;
            let p = track.point_at(sv, line_at(track, sv) + x);
            pos[i] = [
                p.x as f32 - centre.x,
                (p.y + LIFT) as f32 - centre.y,
                p.z as f32 - centre.z,
            ];
            col[i] = rgba;
        }
    }
    guide.stats = stats;
    let show = stats.shown > 0;
    if show || d.shown {
        writes.push(&d.mesh, &d.cpu);
    }
    d.shown = show;
    if tf.translation != centre {
        tf.translation = centre;
    }
    let want = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *vis != want {
        *vis = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_parse() {
        for m in [GuideMode::Off, GuideMode::Brake, GuideMode::Full] {
            assert_eq!(GuideMode::parse(m.name()), Some(m));
        }
        assert_eq!(GuideMode::parse("1"), Some(GuideMode::Full));
        assert_eq!(GuideMode::parse("0"), Some(GuideMode::Off));
        assert_eq!(GuideMode::parse("nope"), None);
    }

    #[test]
    fn colours_run_green_orange_red() {
        let (g, o, r) = (colour(0.0), colour(1.0), colour(2.0));
        assert!(g.y > g.x && g.y > g.z, "green {g}");
        assert!(o.x > o.y && o.y > o.z, "orange {o}");
        assert!(r.x > 0.9 && r.y < 0.05, "red {r}");
        assert_eq!(colour(-1.0), g);
        assert_eq!(colour(5.0), r);
    }

    #[test]
    fn the_reach_follows_the_speed() {
        assert_eq!(reach(0.0), 140.0);
        assert_eq!(reach(40.0), 220.0);
        assert_eq!(reach(80.0), 300.0);
        assert!(reach(80.0) / SPACING < CHEVRONS as f64);
    }

    #[test]
    fn the_mesh_has_six_corners_a_chevron() {
        let m = guide_mesh();
        assert_eq!(m.count_vertices(), CHEVRONS * 6);
        assert_eq!(m.indices().map(|i| i.len()), Some(CHEVRONS * 12));
    }
}
