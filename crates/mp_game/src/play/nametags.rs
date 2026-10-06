//! Name tags over the other humans' cars in a multiplayer race
//! (MULTIPLAYER.md section 3): each name in that player's colour, a little
//! above the car, fading out with distance and hidden behind the camera.
//! Bevy UI text placed by projecting the car through the camera each frame.

use bevy::prelude::*;

use super::{Play, RaceCar};
use crate::net::NetView;

/// One tag per possible human.
const TAGS: usize = 8;
/// Fully shown inside this distance (m), gone past `FAR`.
const NEAR: f32 = 40.0;
const FAR: f32 = 220.0;
/// Above the car's root, m.
const LIFT: f32 = 1.9;

#[derive(Component)]
pub(super) struct NameTag(usize);

pub(super) fn spawn(mut commands: Commands) {
    for i in 0..TAGS {
        commands.spawn((
            NameTag(i),
            Text::new(""),
            TextFont {
                font_size: bevy::text::FontSize::Px(15.0),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::no_wrap(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
            ZIndex(5),
        ));
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn update(
    play: Res<Play>,
    net: Res<NetView>,
    cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    roots: Query<&GlobalTransform, With<RaceCar>>,
    mut tags: Query<(
        &NameTag,
        &mut Text,
        &mut TextColor,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
) {
    let race = play
        .race
        .as_ref()
        .filter(|r| r.online_now() && play.started);
    let models = play.models.as_ref();
    let cam = cams.iter().next();
    for (tag, mut text, mut color, mut node, mut vis, computed) in &mut tags {
        let k = tag.0;
        let shown = (|| {
            let race = race?;
            if k == race.me() || k >= race.session.curr.players.len() {
                return None;
            }
            let (camera, cam_t) = cam?;
            let root = models?.cars.get(k)?.root;
            let at = roots.get(root).ok()?.translation() + Vec3::Y * LIFT;
            let d = cam_t.translation().distance(at);
            if d > FAR {
                return None;
            }
            let px = camera.world_to_viewport(cam_t, at).ok()?;
            let name = net.humans.get(k).map_or("?", |p| p.name.as_str());
            let c = race.session.curr.players[k].spec.color;
            Some((px, name.to_string(), c, d))
        })();
        let Some((px, name, c, d)) = shown else {
            if *vis != Visibility::Hidden {
                *vis = Visibility::Hidden;
            }
            continue;
        };
        if text.0 != name {
            text.0 = name;
        }
        let fade = (1.0 - ((d - NEAR) / (FAR - NEAR)).clamp(0.0, 1.0)) * 0.95 + 0.05;
        let rgb = |s: u32| ((c >> s) & 0xff) as f32 / 255.0;
        // Lifted towards white so dark colours read at night.
        let lift = |x: f32| 0.35 + 0.65 * x;
        color.0 = Color::srgba(lift(rgb(16)), lift(rgb(8)), lift(rgb(0)), fade);
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = Val::Px(px.x - size.x / 2.0);
        node.top = Val::Px(px.y - size.y);
        if *vis != Visibility::Inherited {
            *vis = Visibility::Inherited;
        }
    }
}
