//! The radio on the race screen (DECISIONS D1160): a small head unit the
//! player can tap or click while driving. Off, it is one button, RADIO,
//! that switches the radio on (to the station it last played). On, it
//! shows the station and the song, with ◂ and ▸ to step along the dial
//! and OFF to go back to the music.
//!
//! Where it sits: on a desktop in the bottom-left corner (above the route
//! bar on a narrow window, where the bar moves down there); on a touch
//! screen under the reset, camera and pause buttons, the free space
//! between them and the speed box. Shown only while driving, and only
//! where the radio can run (an AudioWorklet).
//!
//! Its buttons' boxes go into [`Play::radio`] each frame, in CSS px, so
//! the race's input (`read_input`) takes a tap on them before the touch
//! controls see it: the stick's zone covers the whole left side.

use super::Play;
use super::audio::Shared;
use super::flow::Mode;
use crate::ui::widgets::{self, Bp, T};
use bevy::prelude::*;
use bevy::text::{Justify, LineBreak, TextLayout};
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

/// The widget's buttons.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioBtn {
    /// RADIO when off (switches on), OFF when on.
    Power,
    Prev,
    Next,
}

/// The buttons on screen and the taps waiting for the audio.
#[derive(Default, Debug)]
pub struct Taps {
    /// Each button's box, CSS px: (left, top, width, height).
    pub rects: Vec<(RadioBtn, (f32, f32, f32, f32))>,
    pub queued: Vec<RadioBtn>,
}

impl Taps {
    /// The button under (x, y), CSS px, a little grown for fingers.
    pub fn hit(&self, x: f32, y: f32) -> Option<RadioBtn> {
        const SLOP: f32 = 4.0;
        self.rects
            .iter()
            .find(|(_, r)| {
                x >= r.0 - SLOP && x <= r.0 + r.2 + SLOP && y >= r.1 - SLOP && y <= r.1 + r.3 + SLOP
            })
            .map(|(b, _)| *b)
    }
}

#[derive(Component)]
pub(super) struct Root;

/// The texts the update writes.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Line {
    Name,
    Song,
}

/// What the widget was built for: a change rebuilds it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Key {
    w: f32,
    h: f32,
    k: f32,
    touch: bool,
    narrow: bool,
    /// The touch buttons' bottom, CSS px (where the widget goes below).
    under: f32,
    left: f32,
    on: bool,
}

#[derive(Default)]
pub(super) struct WState {
    key: Option<Key>,
    name: String,
    song: String,
}

/// Sizes in CSS px, desktop or touch.
struct Size {
    h: f32,
    btn: f32,
    name: f32,
    song: f32,
    pad: f32,
    max_w: f32,
}

fn size(touch: bool) -> Size {
    if touch {
        Size {
            h: 38.0,
            btn: 34.0,
            name: 13.0,
            song: 11.0,
            pad: 4.0,
            max_w: 230.0,
        }
    } else {
        Size {
            h: 48.0,
            btn: 40.0,
            name: 16.0,
            song: 13.0,
            pad: 5.0,
            max_w: 340.0,
        }
    }
}

/// The station and the song, from the station's now-playing line
/// (`"The Tide 88.1 · Song · style"`).
fn split(text: &str) -> (String, String) {
    let mut it = text.splitn(3, " · ");
    let name = it.next().unwrap_or("").to_owned();
    let song = it.next().unwrap_or("").to_owned();
    (name, song)
}

/// Builds, steers and hides the widget; applies its taps.
#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut commands: Commands,
    mut play: ResMut<Play>,
    shared: Option<NonSend<Shared>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    roots: Query<Entity, With<Root>>,
    mut lines: Query<(&Line, &mut Text)>,
    buttons: Query<(&RadioBtn, &ComputedNode, &UiGlobalTransform)>,
    mut st: Local<WState>,
) {
    let play = &mut *play;
    let driving = play
        .race
        .as_ref()
        .is_some_and(|r| r.mode == Mode::Race && !play.hold);
    let audio = shared.as_ref().and_then(|s| s.0.try_borrow_mut().ok());
    let usable = audio
        .as_ref()
        .is_some_and(|a| a.audio.ready() && !a.audio.radio_unavailable());
    let (Some(mut a), true, Ok(w)) = (audio, driving && usable, windows.single()) else {
        for e in &roots {
            commands.entity(e).despawn();
        }
        st.key = None;
        play.radio.rects.clear();
        play.radio.queued.clear();
        return;
    };
    // The taps first, so this frame shows their result.
    for b in std::mem::take(&mut play.radio.queued) {
        match b {
            RadioBtn::Power => {
                let on = a.radio_on();
                a.set_radio(!on);
            }
            RadioBtn::Prev => a.step_station(-1),
            RadioBtn::Next => a.step_station(1),
        }
    }
    let on = a.radio_on();
    let (name, song) = match a.station_text() {
        Some(t) => split(t),
        None => (String::new(), String::new()),
    };
    drop(a);

    let css = play.css_scale.max(0.01);
    let bp = Bp::new(
        w.width() * css,
        w.height() * css,
        1.0 / css,
        play.touch_ui,
        play.insets.top as f32,
    );
    let (under, left) = play.race.as_ref().map_or((0.0, 0.0), |r| {
        let t = &r.touch.layout.taps;
        (
            t.iter().map(|r| r.bottom).fold(0.0, f64::max) as f32,
            t.iter().map(|r| r.left).fold(f64::INFINITY, f64::min) as f32,
        )
    });
    let key = Key {
        w: bp.w,
        h: bp.h,
        k: bp.k,
        touch: bp.touch,
        narrow: bp.narrow,
        under,
        left,
        on,
    };
    if st.key != Some(key) {
        for e in &roots {
            commands.entity(e).despawn();
        }
        build(&mut commands, &bp, &key, &name, &song);
        st.key = Some(key);
        st.name = name.clone();
        st.song = song.clone();
    } else if st.name != name || st.song != song {
        for (l, mut t) in &mut lines {
            t.0 = match l {
                Line::Name => name.clone(),
                Line::Song => song.clone(),
            };
        }
        st.name = name;
        st.song = song;
    }
    // The buttons' boxes for the input, CSS px.
    play.radio.rects.clear();
    for (b, n, t) in &buttons {
        let k = n.inverse_scale_factor * css;
        let c = t.affine().translation * k;
        let s = n.size * k;
        if s.x > 0.0 {
            play.radio
                .rects
                .push((*b, (c.x - s.x / 2.0, c.y - s.y / 2.0, s.x, s.y)));
        }
    }
}

fn build(commands: &mut Commands, bp: &Bp, key: &Key, name: &str, song: &str) {
    let z = size(key.touch);
    let px = |v: f32| bp.px(v);
    let panel = widgets::rgba(0x0a0e18, 0.62);
    let line = widgets::white(0.22);
    let mut node = Node {
        position_type: PositionType::Absolute,
        height: px(z.h),
        max_width: px(z.max_w),
        padding: UiRect::all(px(z.pad)),
        column_gap: px(z.pad),
        align_items: AlignItems::Center,
        border: UiRect::all(px(1.0)),
        border_radius: BorderRadius::all(px(z.h / 2.0)),
        overflow: Overflow::clip(),
        ..default()
    };
    if key.touch {
        node.left = px(key.left.max(8.0));
        node.top = px(key.under + 10.0);
    } else {
        node.left = px(28.0);
        node.bottom = px(if key.narrow { 56.0 } else { 22.0 });
    }
    let root = commands
        .spawn((
            node,
            BackgroundColor(panel),
            BorderColor::all(line),
            GlobalZIndex(5),
            Root,
        ))
        .id();
    let btn = |commands: &mut Commands, b: RadioBtn, label: &str, wide: bool| {
        let id = commands
            .spawn((
                Node {
                    height: px(z.btn),
                    min_width: px(z.btn),
                    padding: UiRect::horizontal(px(if wide { 12.0 } else { 0.0 })),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(z.btn / 2.0)),
                    ..default()
                },
                BackgroundColor(widgets::white(0.08)),
                BorderColor::all(line),
                b,
                ChildOf(root),
            ))
            .id();
        let t = T::new(z.name).bold().ls(0.08);
        commands.spawn((
            Text::new(label),
            t.font(bp.k),
            TextColor(widgets::fg()),
            t.spacing(bp.k),
            ChildOf(id),
        ));
    };
    if !key.on {
        btn(commands, RadioBtn::Power, "RADIO", true);
        return;
    }
    btn(commands, RadioBtn::Prev, "<", false);
    let info = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                flex_shrink: 1.0,
                min_width: px(0.0),
                overflow: Overflow::clip(),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    let tn = T::new(z.name).bold();
    commands.spawn((
        Text::new(name),
        tn.font(bp.k),
        TextColor(widgets::fg()),
        tn.spacing(bp.k),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        Line::Name,
        ChildOf(info),
    ));
    let ts = T::new(z.song);
    commands.spawn((
        Text::new(song),
        ts.font(bp.k),
        TextColor(widgets::dim()),
        ts.spacing(bp.k),
        TextLayout::new(Justify::Left, LineBreak::NoWrap),
        Line::Song,
        ChildOf(info),
    ));
    btn(commands, RadioBtn::Next, ">", false);
    btn(commands, RadioBtn::Power, "OFF", true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_now_playing_line_splits_into_station_and_song() {
        assert_eq!(
            split("The Tide 88.1 · Night Drive · synthwave"),
            ("The Tide 88.1".into(), "Night Drive".into())
        );
        assert_eq!(split(""), (String::new(), String::new()));
    }

    #[test]
    fn a_tap_finds_its_button_with_a_little_slop() {
        let t = Taps {
            rects: vec![
                (RadioBtn::Prev, (10.0, 10.0, 30.0, 30.0)),
                (RadioBtn::Next, (100.0, 10.0, 30.0, 30.0)),
            ],
            queued: Vec::new(),
        };
        assert_eq!(t.hit(20.0, 20.0), Some(RadioBtn::Prev));
        assert_eq!(t.hit(132.0, 20.0), Some(RadioBtn::Next));
        assert_eq!(t.hit(70.0, 20.0), None);
        assert_eq!(t.hit(20.0, 60.0), None);
    }
}
