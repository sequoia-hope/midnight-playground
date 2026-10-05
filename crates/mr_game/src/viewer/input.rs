//! The viewer's controls (SPEC 8.6): the keyboard and mouse, the pads
//! through WP 6.4's layer (`play::gamepad_io`, the active pad's standard
//! mapping), and touch, turned each frame into a [`Drive`] for the camera
//! and the panel's actions. Positions are the window's logical px.
//!
//! - **Keyboard:** WASD or the arrows move (free), pan (orbit, overview)
//!   or steer the ride (W/S speed, A/D side); E or Space rise, Q, C or
//!   Ctrl sink; Shift four times faster, Alt a quarter. 1 to 4 pick the
//!   camera, M the next one; F fog, G far plane, T animators, Y the time
//!   of day following or pinned, `[` `]` move it, PageUp and PageDown go
//!   along the route by 500 m, Home back to the start; P folds the panel,
//!   H shows the controls, K takes a screenshot, L copies the link, V
//!   locks the pointer to look with the mouse alone;
//!   Enter in the overview flies down to the middle of the screen.
//! - **Mouse:** drag to look (free, ride) or orbit; the overview drags
//!   the map, the right button turns it, a click flies there; the wheel
//!   changes the free camera's speed or zooms.
//! - **Pad:** left stick moves, right stick looks, the triggers sink (LT)
//!   and rise (RT), the bumpers slow down and speed up (or zoom), Y the
//!   next camera, X folds the panel, A in the overview flies down, the
//!   D-pad goes along the route (◂ ▸) and moves the time of day (▴ ▾),
//!   Start takes a screenshot.
//! - **Touch:** one finger looks, orbits or drags the map, two pinch to
//!   zoom and pan; the stick bottom left moves, the ▲ ▼ buttons bottom
//!   right rise and sink; a tap on the overview flies there.

use super::cams::{Drive, FOV_Y_DEG};
use super::link::Mode;
use super::panel::{self, PanelArea, VControl, VHold, VSlider};
use super::{Act, Sl, Viewer};
use crate::play::gamepad_io::PadsRes;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::{CursorGrabMode, CursorMoved, CursorOptions, PrimaryWindow};

/// What the controls ask this frame.
#[derive(Resource, Default)]
pub struct Intent {
    pub drive: Drive,
    pub acts: Vec<Act>,
    /// Taps and clicks on the view (the overview flies there).
    pub picks: Vec<Vec2>,
}

/// The mouse's pointer id (touches have their own).
const MOUSE: u64 = u64::MAX;
const MOUSE_RIGHT: u64 = u64::MAX - 1;
/// How far a press moves (logical px) before it is a drag, not a tap.
const SLOP: f32 = 8.0;
/// The pads' dead zone and response, as the race's (`Gamepad.js`).
const DEAD: f64 = 0.12;
const CURVE: f64 = 1.4;
/// Radians a second at full right stick.
const LOOK_RATE: f64 = 2.2;

#[derive(Clone, Debug)]
enum Role {
    /// On the panel: the control pressed, where, and whether it became a
    /// scroll of the panel.
    Panel {
        target: Option<(String, Act)>,
        at: Vec2,
        last: Vec2,
        scrolled: bool,
    },
    Slider(Sl, Rect),
    Hold(f64),
    /// The stick's centre and where the finger is.
    Stick(Vec2, Vec2),
    View {
        at: Vec2,
        last: Vec2,
        moved: bool,
        t0: f64,
    },
}

/// The pointers held, the cursor, the touch stick and the pads' last
/// buttons.
#[derive(Resource, Default)]
pub struct Pointers {
    fingers: Vec<(u64, Role)>,
    cursor: Option<Vec2>,
    /// The stick's knob offset (−1 to 1 each way) while a finger holds it.
    pub stick: Option<Vec2>,
    pad_prev: Vec<bool>,
    /// Two fingers on the view: their last distance and midpoint.
    pinch: Option<(f32, Vec2)>,
}

fn shaped(a: f64) -> f64 {
    let m = a.abs();
    if m <= DEAD {
        0.0
    } else {
        a.signum() * ((m - DEAD) / (1.0 - DEAD)).powf(CURVE)
    }
}

/// A node's box in logical px.
pub fn rect(node: &ComputedNode, t: &UiGlobalTransform) -> Rect {
    let k = node.inverse_scale_factor;
    let c = t.affine().translation * k;
    let s = node.size * k;
    Rect::from_center_size(c, s)
}

type Controls<'w, 's> = Query<
    'w,
    's,
    (
        &'static VControl,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
    ),
>;

fn hit_control(controls: &Controls, p: Vec2) -> Option<(String, Act)> {
    let mut best: Option<(f32, &VControl)> = None;
    for (c, n, t, vis) in controls.iter() {
        if !vis.get() || c.act.is_none() {
            continue;
        }
        let r = rect(n, t);
        if !r.contains(p) {
            continue;
        }
        let area = r.width() * r.height();
        if best.as_ref().is_none_or(|(a, _)| area < *a) {
            best = Some((area, c));
        }
    }
    best.and_then(|(_, c)| c.act.clone().map(|a| (c.id.clone(), a)))
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn gather(
    mut intent: ResMut<Intent>,
    mut ptr: ResMut<Pointers>,
    v: Res<Viewer>,
    keys: Res<ButtonInput<KeyCode>>,
    msgs: (
        MessageReader<KeyboardInput>,
        MessageReader<TouchInput>,
        MessageReader<MouseButtonInput>,
        MessageReader<CursorMoved>,
        MessageReader<MouseWheel>,
        MessageReader<MouseMotion>,
    ),
    controls: Controls,
    sliders: Query<(
        &VSlider,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    holds: Query<(
        &VHold,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    area: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility), With<PanelArea>>,
    mut body: Query<&mut ScrollPosition, With<panel::PanelBody>>,
    pads: Option<Res<PadsRes>>,
    mut cursor_opts: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let (mut key_events, mut touches, mut buttons, mut cursor, mut wheel, mut motion) = msgs;
    let mut d = Drive::default();
    let mode = v.mode;
    let view_h = v.view.1.max(1.0);
    // Radians per logical px of drag: the screen's height turns the view
    // by its field of view.
    let k = FOV_Y_DEG.to_radians() / view_h;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    d.boost = if shift {
        super::cams::BOOST
    } else if alt {
        0.25
    } else {
        1.0
    };

    // ── Keyboard ──────────────────────────────────────────────────────
    let axis = |pos: &[KeyCode], neg: &[KeyCode]| {
        f64::from(u8::from(keys.any_pressed(pos.iter().copied())))
            - f64::from(u8::from(keys.any_pressed(neg.iter().copied())))
    };
    d.fwd = axis(
        &[KeyCode::KeyW, KeyCode::ArrowUp],
        &[KeyCode::KeyS, KeyCode::ArrowDown],
    );
    d.right = axis(
        &[KeyCode::KeyD, KeyCode::ArrowRight],
        &[KeyCode::KeyA, KeyCode::ArrowLeft],
    );
    d.up = axis(
        &[KeyCode::KeyE, KeyCode::Space],
        &[
            KeyCode::KeyQ,
            KeyCode::KeyC,
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
        ],
    );
    for e in key_events.read() {
        if e.state != ButtonState::Pressed || e.repeat {
            continue;
        }
        let a = match e.key_code {
            KeyCode::Digit1 => Some(Act::Mode(Mode::Free)),
            KeyCode::Digit2 => Some(Act::Mode(Mode::Orbit)),
            KeyCode::Digit3 => Some(Act::Mode(Mode::Overview)),
            KeyCode::Digit4 => Some(Act::Mode(Mode::Ride)),
            KeyCode::KeyM => Some(Act::NextMode),
            KeyCode::KeyF => Some(Act::Fog),
            KeyCode::KeyG => Some(Act::Far),
            KeyCode::KeyT => Some(Act::Anim),
            KeyCode::KeyY => Some(Act::Follow),
            KeyCode::BracketLeft => Some(Act::TodBy(-0.02)),
            KeyCode::BracketRight => Some(Act::TodBy(0.02)),
            KeyCode::PageUp => Some(Act::RouteBy(500.0)),
            KeyCode::PageDown => Some(Act::RouteBy(-500.0)),
            KeyCode::Home => Some(Act::Home),
            KeyCode::KeyP => Some(Act::Fold),
            KeyCode::KeyH => Some(Act::Help),
            KeyCode::KeyK => Some(Act::Shot),
            KeyCode::KeyL => Some(Act::Link),
            KeyCode::Enter | KeyCode::NumpadEnter => Some(Act::Dive),
            KeyCode::KeyV => {
                // Pointer lock: the mouse looks with no button held (V or
                // Esc again lets go; the browser asks first).
                if let Ok(mut c) = cursor_opts.single_mut() {
                    let lock = c.grab_mode == CursorGrabMode::None;
                    c.grab_mode = if lock {
                        CursorGrabMode::Locked
                    } else {
                        CursorGrabMode::None
                    };
                    c.visible = !lock;
                }
                None
            }
            KeyCode::Escape => {
                if let Ok(mut c) = cursor_opts.single_mut()
                    && c.grab_mode != CursorGrabMode::None
                {
                    c.grab_mode = CursorGrabMode::None;
                    c.visible = true;
                }
                None
            }
            _ => None,
        };
        intent.acts.extend(a);
    }

    // ── Pointers ──────────────────────────────────────────────────────
    let panel_rect = area
        .iter()
        .find(|(_, _, vis)| vis.get())
        .map(|(n, t, _)| rect(n, t));
    let on_panel = |p: Vec2| panel_rect.is_some_and(|r| r.contains(p));
    let to_px = v.ptr as f32;
    let mut events: Vec<(u64, TouchPhase, Vec2)> = Vec::new();
    for t in touches.read() {
        events.push((t.id, t.phase, t.position * to_px));
    }
    for c in cursor.read() {
        let p = c.position * to_px;
        ptr.cursor = Some(p);
        for id in [MOUSE, MOUSE_RIGHT] {
            if ptr.fingers.iter().any(|(i, _)| *i == id) {
                events.push((id, TouchPhase::Moved, p));
            }
        }
    }
    for b in buttons.read() {
        let id = match b.button {
            MouseButton::Left => MOUSE,
            MouseButton::Right | MouseButton::Middle => MOUSE_RIGHT,
            _ => continue,
        };
        let Some(p) = ptr.cursor else { continue };
        events.push((
            id,
            match b.state {
                ButtonState::Pressed => TouchPhase::Started,
                ButtonState::Released => TouchPhase::Ended,
            },
            p,
        ));
    }
    // Pointer lock (natively, and where the browser grants it): the
    // mouse's motion looks with no button held.
    let locked = cursor_opts
        .single()
        .is_ok_and(|c| c.grab_mode == CursorGrabMode::Locked);
    if locked {
        for m in motion.read() {
            d.look.0 -= f64::from(m.delta.x) * k;
            d.look.1 -= f64::from(m.delta.y) * k;
        }
    } else {
        motion.clear();
    }
    for w in wheel.read() {
        let notches = match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 100.0,
        };
        if ptr.cursor.is_some_and(on_panel) {
            if let Ok(mut s) = body.single_mut() {
                s.0.y = (s.0.y - notches * 40.0).max(0.0);
            }
            continue;
        }
        match mode {
            Mode::Free => intent.acts.push(Act::SpeedBy(f64::from(notches))),
            _ => d.zoom += f64::from(notches) * 0.15,
        }
    }
    let now = v.clock;
    for (id, phase, p) in events {
        match phase {
            TouchPhase::Started => {
                if ptr.fingers.iter().any(|(i, _)| *i == id) {
                    continue;
                }
                let role = if on_panel(p) {
                    let slider = sliders.iter().find_map(|(s, n, t, vis)| {
                        let r = rect(n, t);
                        (vis.get() && r.contains(p)).then_some((s.0, r))
                    });
                    match slider {
                        Some((sl, r)) => {
                            intent.acts.push(Act::Slide(sl, frac(r, p)));
                            Role::Slider(sl, r)
                        }
                        None => Role::Panel {
                            target: hit_control(&controls, p),
                            at: p,
                            last: p,
                            scrolled: false,
                        },
                    }
                } else if let Some(h) = holds
                    .iter()
                    .find_map(|(h, n, t, vis)| (vis.get() && rect(n, t).contains(p)).then_some(h.0))
                {
                    Role::Hold(h)
                } else if let Some(c) = panel::stick_centre(&v)
                    && id != MOUSE
                    && id != MOUSE_RIGHT
                    && p.distance(c) < panel::stick_radius(&v) * 1.7
                {
                    Role::Stick(c, p)
                } else {
                    Role::View {
                        at: p,
                        last: p,
                        moved: false,
                        t0: now,
                    }
                };
                ptr.fingers.push((id, role));
            }
            TouchPhase::Moved => {
                let touch = id != MOUSE && id != MOUSE_RIGHT;
                let views = ptr
                    .fingers
                    .iter()
                    .filter(|(_, r)| matches!(r, Role::View { .. }))
                    .count();
                let Some((_, role)) = ptr.fingers.iter_mut().find(|(i, _)| *i == id) else {
                    continue;
                };
                match role {
                    Role::Panel {
                        at, last, scrolled, ..
                    } => {
                        if !*scrolled && p.distance(*at) > SLOP {
                            *scrolled = true;
                        }
                        if *scrolled && let Ok(mut s) = body.single_mut() {
                            s.0.y = (s.0.y - (p.y - last.y)).max(0.0);
                        }
                        *last = p;
                    }
                    Role::Slider(sl, r) => intent.acts.push(Act::Slide(*sl, frac(*r, p))),
                    Role::Stick(_, at) => *at = p,
                    Role::Hold(_) => {}
                    Role::View {
                        at, last, moved, ..
                    } => {
                        if !*moved && p.distance(*at) > SLOP {
                            *moved = true;
                        }
                        let dx = f64::from(p.x - last.x);
                        let dy = f64::from(p.y - last.y);
                        *last = p;
                        // Two fingers: a pinch (below), not a look.
                        if touch && views >= 2 {
                            continue;
                        }
                        let right = id == MOUSE_RIGHT;
                        match mode {
                            Mode::Free | Mode::Ride => {
                                if touch {
                                    // Grab the view (Street View's way).
                                    d.look.0 += dx * k;
                                    d.look.1 += dy * k;
                                } else {
                                    d.look.0 -= dx * k;
                                    d.look.1 -= dy * k;
                                }
                            }
                            Mode::Orbit => {
                                d.look.0 -= dx * k * 2.0;
                                d.look.1 -= dy * k * 2.0;
                            }
                            Mode::Overview => {
                                if right {
                                    d.look.0 += dx * k;
                                } else {
                                    d.pan.0 += dx;
                                    d.pan.1 += dy;
                                }
                            }
                        }
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Canceled => {
                let Some(i) = ptr.fingers.iter().position(|(i, _)| *i == id) else {
                    continue;
                };
                let (_, role) = ptr.fingers.remove(i);
                if phase == TouchPhase::Canceled {
                    continue;
                }
                match role {
                    Role::Panel {
                        target: Some((cid, act)),
                        scrolled: false,
                        ..
                    } => {
                        // A tap: released on the control it pressed.
                        if hit_control(&controls, p).is_some_and(|(id2, _)| id2 == cid) {
                            intent.acts.push(act);
                        }
                    }
                    Role::View {
                        moved: false, t0, ..
                    } if id != MOUSE_RIGHT && now - t0 < 0.6 && mode == Mode::Overview => {
                        intent.picks.push(p);
                    }
                    _ => {}
                }
            }
        }
    }
    // Two fingers on the view: pinch zooms, the midpoint pans.
    let views: Vec<Vec2> = ptr
        .fingers
        .iter()
        .filter_map(|(id, r)| match r {
            Role::View { last, .. } if *id != MOUSE && *id != MOUSE_RIGHT => Some(*last),
            _ => None,
        })
        .collect();
    if views.len() >= 2 {
        let dist = views[0].distance(views[1]).max(1.0);
        let mid = (views[0] + views[1]) / 2.0;
        if let Some((d0, m0)) = ptr.pinch {
            d.zoom += f64::from((dist / d0).ln());
            d.pan.0 += f64::from(mid.x - m0.x);
            d.pan.1 += f64::from(mid.y - m0.y);
        }
        ptr.pinch = Some((dist, mid));
    } else {
        ptr.pinch = None;
    }
    // The stick and the hold buttons.
    let mut stick = None;
    let stick_r = panel::stick_radius(&v);
    for (_, role) in &ptr.fingers {
        match role {
            Role::Stick(c, at) => {
                let off = (*at - *c) / stick_r;
                let off = if off.length() > 1.0 {
                    off.normalize()
                } else {
                    off
                };
                stick = Some(off);
                let x = shaped(f64::from(off.x));
                let y = shaped(f64::from(off.y));
                d.right += x;
                d.fwd -= y;
            }
            Role::Hold(h) => d.up += h,
            _ => {}
        }
    }
    ptr.stick = stick;

    // ── Pads ──────────────────────────────────────────────────────────
    if let Some(pads) = pads
        && let Some(pad) = pads.pads.active.as_ref()
        && pads.pads.state.connected
    {
        let ax = |i: usize| shaped(pad.axes.get(i).copied().unwrap_or(0.0));
        let btn = |i: usize| {
            pad.buttons
                .get(i)
                .map_or(0.0, |b| b.value.max(f64::from(u8::from(b.pressed))))
        };
        let dt = v.frame_ms.clamp(1.0, 50.0) / 1000.0;
        d.right += ax(0);
        d.fwd -= ax(1);
        let (lx, ly) = (ax(2), ax(3));
        match mode {
            Mode::Overview => {
                d.look.0 -= lx * LOOK_RATE * 0.5 * dt;
                d.zoom -= ly * 1.5 * dt;
            }
            _ => {
                d.look.0 -= lx * LOOK_RATE * dt;
                d.look.1 -= ly * LOOK_RATE * dt;
            }
        }
        let trig = |b: f64| if b > 0.05 { b } else { 0.0 };
        d.up += trig(btn(7)) - trig(btn(6));
        let pressed: Vec<bool> = pad.buttons.iter().map(|b| b.pressed).collect();
        let edge = |i: usize| {
            pressed.get(i).copied().unwrap_or(false)
                && !ptr.pad_prev.get(i).copied().unwrap_or(false)
        };
        let mut acts = Vec::new();
        if edge(3) {
            acts.push(Act::NextMode);
        }
        if edge(2) {
            acts.push(Act::Fold);
        }
        if edge(0) {
            acts.push(Act::Dive);
        }
        if edge(9) {
            acts.push(Act::Shot);
        }
        if edge(14) {
            acts.push(Act::RouteBy(-500.0));
        }
        if edge(15) {
            acts.push(Act::RouteBy(500.0));
        }
        if edge(12) {
            acts.push(Act::TodBy(0.02));
        }
        if edge(13) {
            acts.push(Act::TodBy(-0.02));
        }
        match mode {
            Mode::Free | Mode::Ride => {
                if edge(4) {
                    acts.push(Act::SpeedBy(-1.0));
                }
                if edge(5) {
                    acts.push(Act::SpeedBy(1.0));
                }
            }
            Mode::Orbit | Mode::Overview => {
                d.zoom += (btn(5) - btn(4)) * 1.5 * dt;
            }
        }
        intent.acts.extend(acts);
        ptr.pad_prev = pressed;
    }
    intent.drive = d;
}

/// Where along a slider's box a point is, 0 to 1.
fn frac(r: Rect, p: Vec2) -> f64 {
    f64::from(((p.x - r.min.x) / r.width().max(1.0)).clamp(0.0, 1.0))
}
