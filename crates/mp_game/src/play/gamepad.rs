//! Gamepads (port of `src/game/Gamepad.js`, SPEC 8.2, roadmap WP 6.4), read
//! once a frame ([`Pads::poll`]): what each driving action is bound to, the
//! buttons that move around the menus, picking up a new button or stick
//! for an action (the Controller screen), and rumble.
//!
//! A binding is a button (`{ button: 7 }`) or one direction of an axis
//! (`{ axis: 0, dir: 1, rest: 0 }`; rest is where the axis sits untouched,
//! −1 for a trigger reported as an axis). Each reads 0..1. Every controller
//! starts on the standard layout; a remapped one keeps its own map, keyed by
//! its id, so two kinds of pad don't fight over one.
//!
//! This module is the JS's logic over a plain snapshot of the pads ([`Pad`],
//! what `navigator.getGamepads()` hands out with the `standard` mapping's
//! indices); `gamepad_io` reads the platform's pads into it (the browser's
//! Gamepad API on the web, gilrs through Bevy natively) and carries the
//! rumble out through a [`Rumble`] backend (DECISIONS D780–D782).

use mp_math::clamp;
use serde_json::{Map, Value};

/// The driving actions (`ACTIONS`), in the JS order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Left,
    Right,
    Throttle,
    Brake,
    Nitro,
    Handbrake,
    LookBack,
    Camera,
    Reset,
    Pause,
}

impl Action {
    pub const ALL: [Action; 10] = [
        Action::Left,
        Action::Right,
        Action::Throttle,
        Action::Brake,
        Action::Nitro,
        Action::Handbrake,
        Action::LookBack,
        Action::Camera,
        Action::Reset,
        Action::Pause,
    ];

    /// Its key in a map (and the one-shot action's name for the input
    /// layer).
    pub fn key(self) -> &'static str {
        match self {
            Action::Left => "left",
            Action::Right => "right",
            Action::Throttle => "throttle",
            Action::Brake => "brake",
            Action::Nitro => "nitro",
            Action::Handbrake => "handbrake",
            Action::LookBack => "lookBack",
            Action::Camera => "camera",
            Action::Reset => "reset",
            Action::Pause => "pause",
        }
    }

    /// The Controller screen's name for it.
    pub fn label(self) -> &'static str {
        match self {
            Action::Left => "Steer left",
            Action::Right => "Steer right",
            Action::Throttle => "Throttle",
            Action::Brake => "Brake / reverse",
            Action::Nitro => "Nitro",
            Action::Handbrake => "Handbrake",
            Action::LookBack => "Look back",
            Action::Camera => "Camera",
            Action::Reset => "Reset car",
            Action::Pause => "Pause",
        }
    }

    pub fn from_key(k: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.key() == k)
    }

    fn i(self) -> usize {
        self as usize
    }
}

/// One-shot actions: they fire once per press (`Input.consume`).
pub const PRESS: [Action; 3] = [Action::Camera, Action::Reset, Action::Pause];

/// The menu buttons (`NAV`'s keys), in the JS order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Nav {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Back,
    Start,
}

impl Nav {
    pub const ALL: [Nav; 7] = [
        Nav::Up,
        Nav::Down,
        Nav::Left,
        Nav::Right,
        Nav::Confirm,
        Nav::Back,
        Nav::Start,
    ];

    /// The menus use the standard layout whatever the map says, so a bad
    /// map can always be put right from the pad.
    fn buttons(self) -> &'static [usize] {
        match self {
            Nav::Up => &[12],
            Nav::Down => &[13],
            Nav::Left => &[14],
            Nav::Right => &[15],
            Nav::Confirm => &[0],
            Nav::Back => &[1],
            Nav::Start => &[9],
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Nav::Up => "up",
            Nav::Down => "down",
            Nav::Left => "left",
            Nav::Right => "right",
            Nav::Confirm => "confirm",
            Nav::Back => "back",
            Nav::Start => "start",
        }
    }

    fn i(self) -> usize {
        self as usize
    }
}

const STD_BUTTONS: [&str; 17] = [
    "A",
    "B",
    "X",
    "Y",
    "LB",
    "RB",
    "LT",
    "RT",
    "Back",
    "Start",
    "Left stick press",
    "Right stick press",
    "D-pad ↑",
    "D-pad ↓",
    "D-pad ←",
    "D-pad →",
    "Home",
];
const STD_AXES: [[&str; 2]; 4] = [
    ["Left stick ←", "Left stick →"],
    ["Left stick ↑", "Left stick ↓"],
    ["Right stick ←", "Right stick →"],
    ["Right stick ↑", "Right stick ↓"],
];

/// A binding: a button, or one direction of an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Binding {
    Button(usize),
    /// `rest` is `None` where the stored binding had none (`b.rest ?? 0`).
    Axis {
        axis: usize,
        dir: f64,
        rest: Option<f64>,
    },
}

impl Binding {
    pub const fn axis(axis: usize, dir: f64, rest: f64) -> Binding {
        Binding::Axis {
            axis,
            dir,
            rest: Some(rest),
        }
    }

    /// As `JSON.stringify` writes it: `{"button":7}`,
    /// `{"axis":0,"dir":-1,"rest":0}`.
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        match *self {
            Binding::Button(i) => {
                m.insert("button".into(), Value::from(i as u64));
            }
            Binding::Axis { axis, dir, rest } => {
                m.insert("axis".into(), Value::from(axis as u64));
                m.insert("dir".into(), crate::ui::store::num_value(dir));
                if let Some(r) = rest {
                    m.insert("rest".into(), crate::ui::store::num_value(r));
                }
            }
        }
        Value::Object(m)
    }

    /// A stored binding (`b.button != null` first); `None` for anything
    /// the JS could not read either.
    pub fn from_json(v: &Value) -> Option<Binding> {
        let index = |x: &Value| {
            x.as_f64()
                .filter(|f| *f >= 0.0 && f.fract() == 0.0)
                .map(|f| f as usize)
        };
        let o = v.as_object()?;
        if let Some(b) = o.get("button").filter(|b| !b.is_null()) {
            return index(b).map(Binding::Button);
        }
        Some(Binding::Axis {
            axis: index(o.get("axis")?)?,
            dir: o.get("dir")?.as_f64()?,
            rest: o.get("rest").and_then(Value::as_f64),
        })
    }

    fn is_axis(&self) -> bool {
        matches!(self, Binding::Axis { .. })
    }
}

/// `sameBinding`: the same button, or the same direction of one axis.
fn same_binding(a: &Binding, b: &Binding) -> bool {
    match (a, b) {
        (Binding::Button(i), Binding::Button(j)) => i == j,
        (
            Binding::Axis {
                axis: a1, dir: d1, ..
            },
            Binding::Axis {
                axis: a2, dir: d2, ..
            },
        ) => a1 == a2 && d1 == d2,
        _ => false,
    }
}

/// A pad's map: each action's bindings, in the order its keys were
/// written (a JS object).
pub type PadMap = Vec<(String, Vec<Binding>)>;

/// `DEFAULT_MAP`: the standard layout.
pub fn default_map() -> PadMap {
    let b = |i| Binding::Button(i);
    vec![
        ("left".into(), vec![Binding::axis(0, -1.0, 0.0)]),
        ("right".into(), vec![Binding::axis(0, 1.0, 0.0)]),
        ("throttle".into(), vec![b(7)]),
        ("brake".into(), vec![b(6)]),
        ("nitro".into(), vec![b(0)]),
        ("handbrake".into(), vec![b(2), b(5)]),
        ("lookBack".into(), vec![b(1)]),
        ("camera".into(), vec![b(3)]),
        ("reset".into(), vec![b(8)]),
        ("pause".into(), vec![b(9)]),
    ]
}

/// `map[a] ?? []`.
pub fn bindings<'a>(map: &'a PadMap, a: &str) -> &'a [Binding] {
    map.iter()
        .find(|(k, _)| k == a)
        .map_or(&[], |(_, v)| v.as_slice())
}

/// The saved maps (`mr.padMaps`): pad id → its map, in insertion order.
pub type Maps = Vec<(String, PadMap)>;

/// `mr.padMaps` as stored.
pub fn maps_to_json(maps: &Maps) -> Value {
    let mut out = Map::new();
    for (id, map) in maps {
        let mut m = Map::new();
        for (a, list) in map {
            m.insert(
                a.clone(),
                Value::Array(list.iter().map(Binding::to_json).collect()),
            );
        }
        out.insert(id.clone(), Value::Object(m));
    }
    Value::Object(out)
}

/// `store.get('padMaps', {})`: what can be read of it.
pub fn maps_from_json(v: &Value) -> Maps {
    let Some(o) = v.as_object() else {
        return Vec::new();
    };
    o.iter()
        .filter_map(|(id, m)| {
            let m = m.as_object()?;
            let map = m
                .iter()
                .map(|(a, list)| {
                    let list = list
                        .as_array()
                        .map(|l| l.iter().filter_map(Binding::from_json).collect())
                        .unwrap_or_default();
                    (a.clone(), list)
                })
                .collect();
            Some((id.clone(), map))
        })
        .collect()
}

/// A button as the Gamepad API reports it (`value` is `x.value || 0`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Button {
    pub pressed: bool,
    pub value: f64,
}

/// One connected pad, as `navigator.getGamepads()` hands it out this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pad {
    pub id: String,
    pub index: usize,
    /// `"standard"`, or `""` for a layout the browser does not know.
    pub mapping: String,
    pub axes: Vec<f64>,
    pub buttons: Vec<Button>,
}

impl Pad {
    pub fn standard(&self) -> bool {
        self.mapping == "standard"
    }
}

/// `bindingLabel(b, standard)`.
pub fn binding_label(b: Option<&Binding>, standard: bool) -> String {
    match b {
        None => "—".into(),
        Some(Binding::Button(i)) => match STD_BUTTONS.get(*i) {
            Some(n) if standard => (*n).into(),
            _ => format!("Button {i}"),
        },
        Some(Binding::Axis { axis, dir, .. }) => {
            let side = usize::from(*dir > 0.0);
            match STD_AXES.get(*axis) {
                Some(n) if standard => n[side].into(),
                _ => format!("Axis {axis} {}", if *dir > 0.0 { '+' } else { '−' }),
            }
        }
    }
}

/// `bindingValue(p, b)`: 0..1.
pub fn binding_value(p: &Pad, b: &Binding) -> f64 {
    match *b {
        Binding::Button(i) => p.buttons.get(i).map_or(0.0, |x| {
            mp_math::js::max(x.value, if x.pressed { 1.0 } else { 0.0 })
        }),
        Binding::Axis { axis, dir, rest } => {
            let rest = rest.unwrap_or(0.0);
            let v = p.axes.get(axis).copied().unwrap_or(rest);
            clamp((v - rest) / (dir - rest), 0.0, 1.0)
        }
    }
}

/// Sticks don't centre exactly; pedals and buttons bound to one need a dead
/// zone.
const STICK_DEAD: f64 = 0.12;

/// `!b.rest`: an axis resting at 0 (or with no rest given) is a stick.
fn stick(b: &Binding) -> bool {
    match *b {
        Binding::Axis { rest, .. } => rest.is_none_or(|r| r == 0.0 || r.is_nan()),
        Binding::Button(_) => false,
    }
}

fn dead(v: f64, b: &Binding) -> f64 {
    if stick(b) {
        mp_math::js::max(0.0, (v - STICK_DEAD) / (1.0 - STICK_DEAD))
    } else {
        v
    }
}

/// `Pads.state`: what the pads ask for this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub connected: bool,
    /// Per action (`Action::ALL` order), with the stick dead zone.
    pub value: [f64; 10],
    /// Per action: past half travel.
    pub held: [bool; 10],
    /// Per menu button (`Nav::ALL` order).
    pub nav: [bool; 7],
    /// The steering from axes bound to left and right, −1..1.
    pub steer_axis: f64,
    /// Steering bound to buttons: ramped like keys.
    pub digital_left: bool,
    pub digital_right: bool,
    /// One-shot actions pressed this frame.
    pub edges: Vec<Action>,
}

impl State {
    pub fn value(&self, a: Action) -> f64 {
        self.value[a.i()]
    }
    pub fn held(&self, a: Action) -> bool {
        self.held[a.i()]
    }
    pub fn nav(&self, n: Nav) -> bool {
        self.nav[n.i()]
    }
}

/// Where rumble goes: the browser's `vibrationActuator` (or
/// `hapticActuators`), or gilrs's force feedback through Bevy.
pub trait Rumble {
    /// `playEffect('dual-rumble', { duration, strongMagnitude,
    /// weakMagnitude })`, or `hapticActuators[0].pulse(max, duration)`.
    fn play(&mut self, pad: &Pad, strong: f64, weak: f64, duration_ms: f64);
    /// `vibrationActuator.reset()`.
    fn reset(&mut self, pad: &Pad);
}

/// No motors (the tests, a platform without rumble).
pub struct NoRumble;

impl Rumble for NoRumble {
    fn play(&mut self, _: &Pad, _: f64, _: f64, _: f64) {}
    fn reset(&mut self, _: &Pad) {}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mute {
    Act(Action),
    Nav(Nav),
}

/// A binding being picked up (`startCapture`).
#[derive(Clone, Debug)]
pub struct Capture {
    pub action: Action,
    t0: f64,
    armed: bool,
    base: Vec<(usize, Vec<f64>)>,
}

/// What a pad was doing last frame (`noticeActivity`).
#[derive(Clone, Debug)]
struct Last {
    buttons: Vec<bool>,
    axes: Vec<f64>,
}

#[derive(Clone, Copy, Debug)]
struct Kick {
    strong: f64,
    weak: f64,
    ms: f64,
    t0: f64,
}

#[derive(Clone, Copy, Debug)]
struct Cont {
    strong: f64,
    weak: f64,
    t: f64,
}

#[derive(Clone, Copy, Debug)]
struct Sent {
    strong: f64,
    weak: f64,
    t: f64,
}

/// `Pads`: the maps, the pad in use, capture, the mute set and rumble.
#[derive(Clone, Debug)]
pub struct Pads {
    pub maps: Maps,
    /// `onSave`: the maps changed since this was last cleared (the client
    /// writes `mr.padMaps` and clears it).
    pub maps_changed: bool,
    pub rumble_on: bool,
    /// The pad last touched: the Controller screen edits its map.
    pub active: Option<Pad>,
    pub active_index: Option<usize>,
    pub capture: Option<Capture>,
    /// The ends of captures (`done(binding)`, `done(null)`), oldest first,
    /// for the Controller screen to take.
    pub done: Vec<(Action, Option<Binding>)>,
    prev: [bool; 10],
    muted: Vec<Mute>,
    last: Vec<(usize, Last)>,
    kicks: Vec<Kick>,
    cont: Option<Cont>,
    sent: Sent,
    pub state: State,
    /// The time of the last poll, ms (`this.now`).
    pub now: Option<f64>,
    default: PadMap,
}

impl Default for Pads {
    fn default() -> Self {
        Pads::new(Vec::new())
    }
}

impl Pads {
    pub fn new(maps: Maps) -> Pads {
        Pads {
            maps,
            maps_changed: false,
            rumble_on: true,
            active: None,
            active_index: None,
            capture: None,
            done: Vec::new(),
            prev: [false; 10],
            muted: Vec::new(),
            last: Vec::new(),
            kicks: Vec::new(),
            cont: None,
            sent: Sent {
                strong: 0.0,
                weak: 0.0,
                t: -1e9,
            },
            state: State::default(),
            now: None,
            default: default_map(),
        }
    }

    pub fn map_for(&self, p: &Pad) -> &PadMap {
        self.maps
            .iter()
            .find(|(id, _)| *id == p.id)
            .map_or(&self.default, |(_, m)| m)
    }

    /// What an action is on the pad in hand, for prompts ("PRESS BACK").
    pub fn label(&self, a: Action) -> String {
        let p = self.active.as_ref();
        let map = p.map_or(&self.default, |p| self.map_for(p));
        binding_label(bindings(map, a.key()).first(), p.is_none_or(Pad::standard))
    }

    pub fn is_remapped(&self, p: Option<&Pad>) -> bool {
        p.is_some_and(|p| self.maps.iter().any(|(id, _)| *id == p.id))
    }

    fn mute(&mut self, m: Mute) {
        if !self.muted.contains(&m) {
            self.muted.push(m);
        }
    }

    /// `poll(now)`: this frame's pads (the connected ones), `now` in ms.
    pub fn poll(&mut self, now: f64, pads: &[Pad], rumble: &mut dyn Rumble) -> &State {
        self.now = Some(now);
        let mut value = [0.0; 10];
        let mut held = [false; 10];
        let mut nav = [false; 7];
        let mut steer_axis = 0.0;
        let (mut dl, mut dr) = (false, false);
        for p in pads {
            self.notice_activity(p);
        }
        if !pads.iter().any(|p| Some(p.index) == self.active_index) {
            self.active_index = pads.first().map(|p| p.index);
        }
        self.active = pads
            .iter()
            .find(|p| Some(p.index) == self.active_index)
            .cloned();
        for p in pads {
            let map = self.map_for(p);
            for a in Action::ALL {
                for b in bindings(map, a.key()) {
                    let v = binding_value(p, b);
                    value[a.i()] = mp_math::js::max(value[a.i()], dead(v, b));
                    if v > 0.5 {
                        held[a.i()] = true;
                    }
                    if a == Action::Left || a == Action::Right {
                        if b.is_axis() {
                            steer_axis += if a == Action::Right { v } else { -v };
                        } else if v > 0.5 {
                            if a == Action::Left {
                                dl = true;
                            } else {
                                dr = true;
                            }
                        }
                    }
                }
            }
            for k in Nav::ALL {
                if k.buttons()
                    .iter()
                    .any(|&i| p.buttons.get(i).is_some_and(|b| b.pressed))
                {
                    nav[k.i()] = true;
                }
            }
            let x = p.axes.first().copied().unwrap_or(0.0);
            let y = p.axes.get(1).copied().unwrap_or(0.0);
            if x < -0.5 {
                nav[Nav::Left.i()] = true;
            } else if x > 0.5 {
                nav[Nav::Right.i()] = true;
            }
            if y < -0.5 {
                nav[Nav::Up.i()] = true;
            } else if y > 0.5 {
                nav[Nav::Down.i()] = true;
            }
        }
        // Held through a capture, or through the press that left a menu:
        // quiet until let go, so the A that picked Resume doesn't fire the
        // nitro.
        let capturing = self.capture.is_some();
        // This frame was read with the old map, so the new binding is kept
        // quiet by hand until it's seen let go.
        let bound = if capturing { self.listen(pads) } else { None };
        if capturing {
            for a in Action::ALL {
                self.mute(Mute::Act(a));
            }
            for k in Nav::ALL {
                self.mute(Mute::Nav(k));
            }
        }
        for m in self.muted.clone() {
            match m {
                Mute::Nav(k) => {
                    if !nav[k.i()] {
                        self.muted.retain(|x| *x != m);
                        continue;
                    }
                    nav[k.i()] = false;
                }
                Mute::Act(a) => {
                    if Some(a) != bound && !held[a.i()] && value[a.i()] < 0.05 {
                        self.muted.retain(|x| *x != m);
                        continue;
                    }
                    value[a.i()] = 0.0;
                    held[a.i()] = false;
                    if a == Action::Left {
                        dl = false;
                    }
                    if a == Action::Right {
                        dr = false;
                    }
                }
            }
        }
        if self.muted.contains(&Mute::Act(Action::Left))
            || self.muted.contains(&Mute::Act(Action::Right))
        {
            steer_axis = 0.0;
        }
        let mut edges = Vec::new();
        for a in PRESS {
            if held[a.i()] && !self.prev[a.i()] {
                edges.push(a);
            }
            self.prev[a.i()] = held[a.i()];
        }
        self.state = State {
            connected: !pads.is_empty(),
            value,
            held,
            nav,
            steer_axis: clamp(steer_axis, -1.0, 1.0),
            digital_left: dl,
            digital_right: dr,
            edges,
        };
        self.flush_rumble(now, rumble);
        &self.state
    }

    /// Quiet whatever is held now until it's let go.
    pub fn hush(&mut self) {
        for a in Action::ALL {
            if self.state.held[a.i()] || self.state.value[a.i()] > 0.05 {
                self.mute(Mute::Act(a));
            }
        }
        for k in Nav::ALL {
            if self.state.nav[k.i()] {
                self.mute(Mute::Nav(k));
            }
        }
    }

    /// Browsers hand out a fresh snapshot each frame, so the pad is kept by
    /// index.
    fn notice_activity(&mut self, p: &Pad) {
        let now = Last {
            buttons: p.buttons.iter().map(|b| b.pressed).collect(),
            axes: p.axes.clone(),
        };
        let was = match self.last.iter_mut().find(|(i, _)| *i == p.index) {
            Some((_, l)) => Some(std::mem::replace(l, now.clone())),
            None => {
                self.last.push((p.index, now.clone()));
                None
            }
        };
        if let Some(was) = was {
            let pressed = now
                .buttons
                .iter()
                .enumerate()
                .any(|(i, &b)| b && !was.buttons.get(i).copied().unwrap_or(false));
            let moved = now.axes.iter().enumerate().any(|(i, &v)| {
                let w = was.axes.get(i).copied().unwrap_or(v);
                (v - w).abs() > 0.3
            });
            if pressed || moved {
                self.active_index = Some(p.index);
            }
        }
    }

    // ── Remapping ─────────────────────────────────────────────────────
    /// Waits for every button to be let go (the A that chose the action is
    /// still down), then takes the first button pressed or axis pushed past
    /// half travel. Ends in `done` with the binding, or `None` after 8 s.
    pub fn start_capture(&mut self, action: Action) {
        self.capture = Some(Capture {
            action,
            t0: self.now.unwrap_or(0.0),
            armed: false,
            base: Vec::new(),
        });
    }

    pub fn cancel_capture(&mut self) {
        if let Some(c) = self.capture.take() {
            self.done.push((c.action, None));
        }
    }

    fn listen(&mut self, pads: &[Pad]) -> Option<Action> {
        let now = self.now.unwrap_or(0.0);
        let c = self.capture.as_mut()?;
        if now - c.t0 > 8000.0 {
            self.cancel_capture();
            return None;
        }
        if !c.armed {
            let still = pads.iter().all(|p| {
                p.buttons.iter().all(|b| !b.pressed && b.value < 0.3)
                    && p.axes.iter().all(|v| v.abs() < 0.3 || v.abs() > 0.95)
            });
            if !still {
                return None;
            }
            c.armed = true;
            c.base = pads.iter().map(|p| (p.index, p.axes.clone())).collect();
            return None;
        }
        let action = c.action;
        for p in pads {
            let mut b = None;
            if let Some(i) = p.buttons.iter().position(|x| x.pressed || x.value > 0.6) {
                b = Some(Binding::Button(i));
            } else {
                let zeros = vec![0.0; p.axes.len()];
                let base = c
                    .base
                    .iter()
                    .find(|(i, _)| *i == p.index)
                    .map_or(&zeros, |(_, a)| a);
                let at = |k: usize| base.get(k).copied().unwrap_or(0.0);
                if let Some(j) = p
                    .axes
                    .iter()
                    .enumerate()
                    .position(|(k, v)| (v - at(k)).abs() > 0.6)
                {
                    // A standard pad's axes are sticks, centred at 0;
                    // otherwise a resting -1 or +1 is a trigger reported as
                    // an axis.
                    let b0 = at(j);
                    let rest = if !p.standard() && b0.abs() > 0.8 {
                        mp_math::js::sign(b0)
                    } else {
                        0.0
                    };
                    let dir = if p.axes[j] > rest { 1.0 } else { -1.0 };
                    b = Some(Binding::axis(j, dir, rest));
                }
            }
            let Some(b) = b else { continue };
            self.bind(p, action, b);
            self.mute(Mute::Act(action)); // what it does now waits for it to be let go
            self.active_index = Some(p.index);
            self.capture = None;
            self.done.push((action, Some(b)));
            return Some(action);
        }
        None
    }

    /// One button does one thing: taking it for this action frees it
    /// elsewhere.
    pub fn bind(&mut self, p: &Pad, action: Action, b: Binding) {
        let mut map = self.map_for(p).clone();
        for (_, list) in map.iter_mut() {
            list.retain(|x| !same_binding(x, &b));
        }
        match map.iter_mut().find(|(k, _)| k == action.key()) {
            Some((_, list)) => *list = vec![b],
            None => map.push((action.key().into(), vec![b])),
        }
        match self.maps.iter_mut().find(|(id, _)| *id == p.id) {
            Some((_, m)) => *m = map,
            None => self.maps.push((p.id.clone(), map)),
        }
        self.maps_changed = true;
    }

    pub fn reset_map(&mut self, p: Option<&Pad>) {
        let Some(p) = p else { return };
        self.maps.retain(|(id, _)| *id != p.id);
        self.maps_changed = true;
    }

    // ── Rumble ────────────────────────────────────────────────────────
    /// A jolt that fades out over `ms`.
    pub fn kick(&mut self, strong: f64, weak: f64, ms: f64) {
        if !self.rumble_on {
            return;
        }
        self.kicks.push(Kick {
            strong,
            weak,
            ms,
            t0: self.now.unwrap_or(0.0),
        });
    }

    /// The steady buzz this frame (gravel, a wall scrape); it stops when
    /// the race stops calling it.
    pub fn feel(&mut self, strong: f64, weak: f64) {
        self.cont = self.rumble_on.then(|| Cont {
            strong,
            weak,
            t: self.now.unwrap_or(0.0),
        });
    }

    fn rumble_level(&mut self, now: f64) -> (f64, f64) {
        self.kicks.retain(|k| now - k.t0 < k.ms);
        let (mut strong, mut weak) = (0.0, 0.0);
        for k in &self.kicks {
            let f = 1.0 - (now - k.t0) / k.ms;
            strong = mp_math::js::max(strong, k.strong * f);
            weak = mp_math::js::max(weak, k.weak * f);
        }
        if let Some(c) = self.cont
            && now - c.t < 150.0
        {
            strong = mp_math::js::max(strong, c.strong);
            weak = mp_math::js::max(weak, c.weak);
        }
        (clamp(strong, 0.0, 1.0), clamp(weak, 0.0, 1.0))
    }

    fn flush_rumble(&mut self, now: f64, rumble: &mut dyn Rumble) {
        let (strong, weak) = if self.rumble_on {
            self.rumble_level(now)
        } else {
            (0.0, 0.0)
        };
        let p = self.active.as_ref();
        if strong < 0.02 && weak < 0.02 {
            if self.sent.strong != 0.0 || self.sent.weak != 0.0 {
                if let Some(p) = p {
                    rumble.reset(p);
                }
                self.sent = Sent {
                    strong: 0.0,
                    weak: 0.0,
                    t: -1e9,
                };
            }
            return;
        }
        // Each effect runs a little longer than the gap to the next, so a
        // steady buzz doesn't stutter; a new one replaces the last.
        if now - self.sent.t < 80.0
            && (strong - self.sent.strong).abs() < 0.08
            && (weak - self.sent.weak).abs() < 0.08
        {
            return;
        }
        self.sent = Sent {
            strong,
            weak,
            t: now,
        };
        if let Some(p) = p {
            rumble.play(p, strong, weak, 140.0);
        }
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/gamepad.test.js`, with the same fake controllers.
    use super::*;

    fn make_pad(id: &str, index: usize, mapping: &str, axes: usize) -> Pad {
        Pad {
            id: id.into(),
            index,
            mapping: mapping.into(),
            axes: vec![0.0; axes],
            buttons: vec![Button::default(); 17],
        }
    }

    fn pad() -> Pad {
        make_pad("Pad", 0, "standard", 4)
    }

    fn btn(p: &mut Pad, i: usize, on: bool) {
        p.buttons[i].pressed = on;
        p.buttons[i].value = if on { 1.0 } else { 0.0 };
    }

    /// The fake pads' `vibrationActuator`s: effects per pad index; a pad
    /// in `no_motor` has none.
    #[derive(Default)]
    struct Fx {
        effects: Vec<(usize, Effect)>,
        no_motor: Vec<usize>,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Effect {
        Dual {
            duration: f64,
            strong: f64,
            weak: f64,
        },
        Reset,
    }

    impl Rumble for Fx {
        fn play(&mut self, p: &Pad, strong: f64, weak: f64, duration: f64) {
            if !self.no_motor.contains(&p.index) {
                self.effects.push((
                    p.index,
                    Effect::Dual {
                        duration,
                        strong,
                        weak,
                    },
                ));
            }
        }
        fn reset(&mut self, p: &Pad) {
            if !self.no_motor.contains(&p.index) {
                self.effects.push((p.index, Effect::Reset));
            }
        }
    }

    impl Fx {
        fn take(&mut self) -> Vec<Effect> {
            std::mem::take(&mut self.effects)
                .into_iter()
                .map(|(_, e)| e)
                .collect()
        }
    }

    fn poll(pads: &mut Pads, t: f64, list: &[&Pad]) -> State {
        let list: Vec<Pad> = list.iter().map(|p| (*p).clone()).collect();
        pads.poll(t, &list, &mut NoRumble).clone()
    }

    #[test]
    fn binding_values_buttons_stick_directions_and_a_trigger_reported_as_an_axis() {
        let mut p = pad();
        p.buttons[7].value = 0.4;
        assert_eq!(binding_value(&p, &Binding::Button(7)), 0.4);
        p.buttons[3].pressed = true; // a digital button with no value
        assert_eq!(binding_value(&p, &Binding::Button(3)), 1.0);
        p.axes[0] = -0.6;
        assert_eq!(binding_value(&p, &Binding::axis(0, -1.0, 0.0)), 0.6);
        assert_eq!(binding_value(&p, &Binding::axis(0, 1.0, 0.0)), 0.0);
        // Rests at -1, full at +1.
        for (v, want) in [(-1.0, 0.0), (0.0, 0.5), (1.0, 1.0)] {
            p.axes[2] = v;
            assert_eq!(binding_value(&p, &Binding::axis(2, 1.0, -1.0)), want);
        }
        assert_eq!(
            binding_value(&p, &Binding::Button(30)),
            0.0,
            "a button the pad lacks"
        );
    }

    #[test]
    fn labels_standard_names_or_numbers_for_a_pad_that_isnt_standard() {
        let ax = |axis, dir| Binding::Axis {
            axis,
            dir,
            rest: None,
        };
        assert_eq!(binding_label(Some(&Binding::Button(7)), true), "RT");
        assert_eq!(binding_label(Some(&Binding::Button(14)), true), "D-pad ←");
        assert_eq!(binding_label(Some(&ax(0, 1.0)), true), "Left stick →");
        assert_eq!(binding_label(Some(&ax(3, -1.0)), true), "Right stick ↑");
        assert_eq!(binding_label(Some(&Binding::Button(7)), false), "Button 7");
        assert_eq!(binding_label(Some(&ax(5, 1.0)), false), "Axis 5 +");
        assert_eq!(binding_label(None, true), "—");
        let pads = Pads::default();
        assert_eq!(
            pads.label(Action::Reset),
            "Back",
            "no pad: the standard layout"
        );
    }

    #[test]
    fn the_camera_is_y_by_default_and_rb_stays_the_handbrakes() {
        // The owner asked for a face button or RB for the camera
        // (2026-10-05): Y has it in the JS's DEFAULT_MAP already; RB is the
        // handbrake's second button, so nothing moves (D1062).
        assert_eq!(bindings(&default_map(), "camera"), &[Binding::Button(3)]);
        assert_eq!(
            bindings(&default_map(), "handbrake"),
            &[Binding::Button(2), Binding::Button(5)]
        );
        assert_eq!(Pads::default().label(Action::Camera), "Y");
        // Every default button is one action's, so a press does one thing.
        let mut seen = Vec::new();
        for (_, list) in default_map() {
            for b in list {
                assert!(!seen.contains(&b), "{b:?} bound twice");
                seen.push(b);
            }
        }
        let mut p = pad();
        let mut pads = Pads::default();
        poll(&mut pads, 0.0, &[&p]);
        btn(&mut p, 3, true);
        let s = poll(&mut pads, 16.0, &[&p]);
        assert_eq!(s.edges, vec![Action::Camera], "Y: one camera press");
        assert!(!s.held(Action::Handbrake));
        btn(&mut p, 3, false);
        btn(&mut p, 5, true);
        let s = poll(&mut pads, 32.0, &[&p]);
        assert!(s.held(Action::Handbrake), "RB: handbrake");
        assert!(s.edges.is_empty());
        // A map saved before (`mr.padMaps`) keeps its own camera button.
        let saved = r#"{"Pad":{"camera":[{"button":4}],"handbrake":[{"button":5}]}}"#;
        let pads = Pads::new(maps_from_json(&serde_json::from_str(saved).unwrap()));
        assert_eq!(
            bindings(pads.map_for(&pad()), "camera"),
            &[Binding::Button(4)]
        );
    }

    #[test]
    fn the_menu_buttons_dpad_left_stick_a_b_start_from_any_pad() {
        let mut a = pad();
        let mut b = make_pad("Pad", 1, "standard", 4);
        let mut pads = Pads::default();
        assert_eq!(poll(&mut pads, 0.0, &[&a, &b]).nav, [false; 7]);
        btn(&mut a, 13, true);
        b.axes[0] = -0.8;
        btn(&mut b, 0, true);
        let s = poll(&mut pads, 16.0, &[&a, &b]);
        assert!(s.nav(Nav::Down), "D-pad ↓");
        assert!(s.nav(Nav::Left), "stick ←, on the other pad");
        assert!(s.nav(Nav::Confirm), "A");
        b.axes[0] = -0.3;
        assert!(
            !poll(&mut pads, 32.0, &[&a, &b]).nav(Nav::Left),
            "a little stick isn't a push"
        );
    }

    #[test]
    fn hush_whats_held_stays_quiet_until_let_go_then_works_again() {
        let mut p = pad();
        let mut pads = Pads::default();
        btn(&mut p, 0, true);
        btn(&mut p, 9, true);
        p.buttons[7].value = 0.8;
        let s = poll(&mut pads, 0.0, &[&p]);
        assert!(s.held(Action::Nitro));
        assert_eq!(s.edges, vec![Action::Pause]);
        pads.hush();
        let s = poll(&mut pads, 16.0, &[&p]);
        assert!(!s.held(Action::Nitro), "A: quiet");
        assert_eq!(s.value(Action::Throttle), 0.0, "RT: quiet");
        assert!(!s.nav(Nav::Confirm));
        btn(&mut p, 0, false);
        p.buttons[7].value = 0.0;
        poll(&mut pads, 32.0, &[&p]);
        btn(&mut p, 0, true);
        p.buttons[7].value = 0.5;
        let s = poll(&mut pads, 48.0, &[&p]);
        assert!(s.held(Action::Nitro), "pressed again");
        assert_eq!(s.value(Action::Throttle), 0.5);
        btn(&mut p, 9, false);
        poll(&mut pads, 64.0, &[&p]);
        btn(&mut p, 9, true);
        assert_eq!(
            poll(&mut pads, 80.0, &[&p]).edges,
            vec![Action::Pause],
            "Start: one press, one pause"
        );
    }

    #[test]
    fn capture_waits_for_the_button_that_chose_it_to_be_let_go_then_takes_the_next() {
        let mut p = make_pad("Pad A", 0, "standard", 4);
        let mut pads = Pads::default();
        btn(&mut p, 0, true); // A, still down from picking the row
        poll(&mut pads, 0.0, &[&p]);
        pads.start_capture(Action::Throttle);
        poll(&mut pads, 16.0, &[&p]);
        poll(&mut pads, 32.0, &[&p]);
        assert!(pads.done.is_empty(), "not A");
        assert!(
            !pads.state.held(Action::Nitro),
            "and nothing reaches the game while listening"
        );
        btn(&mut p, 0, false);
        poll(&mut pads, 48.0, &[&p]);
        btn(&mut p, 4, true); // LB
        poll(&mut pads, 64.0, &[&p]);
        assert_eq!(
            pads.done.pop(),
            Some((Action::Throttle, Some(Binding::Button(4))))
        );
        assert!(pads.capture.is_none());
        assert_eq!(
            bindings(pads.map_for(&p), "throttle"),
            &[Binding::Button(4)]
        );
        assert!(pads.maps_changed, "saved");
        let saved = maps_to_json(&pads.maps);
        assert_eq!(
            saved["Pad A"]["throttle"],
            serde_json::json!([{ "button": 4 }])
        );
        // The LB that was just bound doesn't fire the throttle until it's
        // pressed again.
        assert_eq!(poll(&mut pads, 80.0, &[&p]).value(Action::Throttle), 0.0);
        btn(&mut p, 4, false);
        poll(&mut pads, 96.0, &[&p]);
        btn(&mut p, 4, true);
        assert_eq!(poll(&mut pads, 112.0, &[&p]).value(Action::Throttle), 1.0);
        // Another controller keeps the standard layout.
        let q = make_pad("Pad B", 1, "standard", 4);
        assert_eq!(*pads.map_for(&q), default_map());
        assert!(pads.is_remapped(Some(&p)));
        assert!(!pads.is_remapped(Some(&q)));
        pads.reset_map(Some(&p));
        assert_eq!(*pads.map_for(&p), default_map());
    }

    #[test]
    fn capture_one_button_one_action_taking_rb_for_nitro_frees_it_from_the_handbrake() {
        let mut p = pad();
        let mut pads = Pads::default();
        poll(&mut pads, 0.0, &[&p]);
        pads.start_capture(Action::Nitro);
        poll(&mut pads, 16.0, &[&p]);
        btn(&mut p, 5, true);
        poll(&mut pads, 32.0, &[&p]);
        let m = pads.map_for(&p);
        assert_eq!(bindings(m, "nitro"), &[Binding::Button(5)]);
        assert_eq!(
            bindings(m, "handbrake"),
            &[Binding::Button(2)],
            "X is still the handbrake"
        );
    }

    #[test]
    fn capture_sticks_and_triggers_that_are_axes() {
        // Standard: an axis is a stick, centred at 0.
        let mut p = pad();
        let mut pads = Pads::default();
        poll(&mut pads, 0.0, &[&p]);
        pads.start_capture(Action::Left);
        poll(&mut pads, 16.0, &[&p]);
        p.axes[2] = -0.9;
        poll(&mut pads, 32.0, &[&p]);
        assert_eq!(
            pads.done.pop(),
            Some((Action::Left, Some(Binding::axis(2, -1.0, 0.0))))
        );
        // Not standard: a resting -1 is a trigger; it reads 0 there and 1
        // pulled.
        let mut q = make_pad("Odd pad", 0, "", 6);
        q.axes[5] = -1.0;
        let mut pads2 = Pads::default();
        poll(&mut pads2, 0.0, &[&q]);
        pads2.start_capture(Action::Throttle);
        poll(&mut pads2, 16.0, &[&q]);
        q.axes[5] = 0.7;
        poll(&mut pads2, 32.0, &[&q]);
        assert_eq!(
            pads2.done.pop(),
            Some((Action::Throttle, Some(Binding::axis(5, 1.0, -1.0))))
        );
        q.axes[5] = 1.0;
        poll(&mut pads2, 48.0, &[&q]);
        q.axes[5] = -1.0;
        poll(&mut pads2, 64.0, &[&q]); // let go (it was held through the capture)
        q.axes[5] = 0.0;
        assert_eq!(
            poll(&mut pads2, 80.0, &[&q]).value(Action::Throttle),
            0.5,
            "half pulled"
        );
        assert_eq!(pads2.label(Action::Throttle), "Axis 5 +");
    }

    #[test]
    fn capture_gives_up_after_8_s_or_when_cancelled() {
        let p = pad();
        let mut pads = Pads::default();
        poll(&mut pads, 0.0, &[&p]);
        pads.start_capture(Action::Camera);
        poll(&mut pads, 4000.0, &[&p]);
        poll(&mut pads, 8100.0, &[&p]);
        assert_eq!(pads.done, vec![(Action::Camera, None)]);
        assert!(pads.capture.is_none());
        pads.start_capture(Action::Camera);
        pads.cancel_capture();
        assert_eq!(
            pads.done,
            vec![(Action::Camera, None), (Action::Camera, None)]
        );
        assert_eq!(*pads.map_for(&p), default_map());
    }

    // 'a remapped D-pad steers like keys' is with the input layer
    // (`input::tests`).

    #[test]
    fn a_stick_bound_to_a_pedal_has_a_dead_zone() {
        let mut p = pad();
        let mut map = default_map();
        map[2].1 = vec![Binding::axis(3, -1.0, 0.0)];
        let mut pads = Pads::new(vec![("Pad".into(), map)]);
        p.axes[3] = -0.1;
        assert_eq!(
            poll(&mut pads, 0.0, &[&p]).value(Action::Throttle),
            0.0,
            "drift at rest"
        );
        p.axes[3] = -1.0;
        assert_eq!(poll(&mut pads, 16.0, &[&p]).value(Action::Throttle), 1.0);
    }

    #[test]
    fn rumble_a_kick_fades_a_steady_buzz_lasts_while_its_fed_and_it_stops_with_a_reset() {
        let p = vec![pad()];
        let mut fx = Fx::default();
        let mut pads = Pads::default();
        pads.poll(0.0, &p, &mut fx);
        pads.kick(0.8, 0.4, 200.0);
        pads.poll(16.0, &p, &mut fx);
        let e = fx.take();
        assert_eq!(e.len(), 1);
        let Effect::Dual {
            duration, strong, ..
        } = e[0]
        else {
            panic!("dual-rumble: {e:?}")
        };
        assert!(strong > 0.7 && strong <= 0.8);
        assert!(duration >= 100.0, "outlasts the gap to the next");
        pads.poll(32.0, &p, &mut fx);
        assert!(fx.effects.is_empty(), "not resent every frame");
        pads.poll(120.0, &p, &mut fx);
        let e = fx.take();
        assert_eq!(e.len(), 1, "refreshed as it fades");
        assert!(matches!(e[0], Effect::Dual { strong, .. } if strong < 0.6));
        pads.poll(230.0, &p, &mut fx);
        assert_eq!(fx.take(), vec![Effect::Reset], "over: stopped");
        pads.poll(246.0, &p, &mut fx);
        assert!(fx.effects.is_empty(), "once");

        // Steady: fed each frame, then not (the race paused).
        let mut t = 300.0;
        while t < 600.0 {
            pads.feel(0.3, 0.2);
            pads.poll(t, &p, &mut fx);
            t += 16.0;
        }
        assert!(
            fx.effects.len() >= 3
                && fx
                    .effects
                    .iter()
                    .all(|(_, e)| matches!(e, Effect::Dual { strong, .. } if *strong == 0.3))
        );
        fx.effects.clear();
        pads.poll(620.0, &p, &mut fx); // within 150 ms of the last feel: still on
        pads.poll(800.0, &p, &mut fx);
        let e = fx.take();
        assert_eq!(e.last(), Some(&Effect::Reset));
        assert_eq!(e.iter().filter(|e| **e == Effect::Reset).count(), 1);

        // Off: nothing.
        pads.rumble_on = false;
        pads.kick(1.0, 1.0, 500.0);
        pads.feel(1.0, 1.0);
        pads.poll(900.0, &p, &mut fx);
        assert!(fx.effects.is_empty());
    }

    #[test]
    fn rumble_goes_to_the_pad_last_used_and_a_pad_without_a_motor_is_fine() {
        let a = pad();
        let mut b = make_pad("Pad", 1, "standard", 4);
        let mut fx = Fx {
            no_motor: vec![0],
            ..Fx::default()
        };
        let mut pads = Pads::default();
        pads.poll(0.0, &[a.clone(), b.clone()], &mut fx);
        pads.kick(1.0, 1.0, 300.0);
        pads.poll(16.0, &[a.clone(), b.clone()], &mut fx); // a is in use: no motor, no error
        btn(&mut b, 0, true);
        pads.poll(32.0, &[a.clone(), b.clone()], &mut fx);
        btn(&mut b, 0, false);
        pads.kick(1.0, 1.0, 300.0);
        pads.poll(200.0, &[a.clone(), b.clone()], &mut fx);
        assert!(
            fx.effects.iter().any(|(i, _)| *i == 1),
            "b, once it was used"
        );
        assert_eq!(pads.active.as_ref().map(|p| p.index), Some(1));
    }

    #[test]
    fn maps_round_trip_through_the_store_as_the_js_writes_them() {
        let js = r#"{"Pad A":{"left":[{"axis":0,"dir":-1,"rest":0}],"right":[{"axis":0,"dir":1,"rest":0}],"throttle":[{"button":4}],"brake":[{"button":6}],"nitro":[{"button":0}],"handbrake":[{"button":2},{"button":5}],"lookBack":[{"button":1}],"camera":[{"button":3}],"reset":[{"button":8}],"pause":[{"button":9}]},"Odd":{"throttle":[{"axis":5,"dir":1,"rest":-1}]}}"#;
        let v: Value = serde_json::from_str(js).unwrap();
        let maps = maps_from_json(&v);
        assert_eq!(maps.len(), 2);
        assert_eq!(bindings(&maps[0].1, "throttle"), &[Binding::Button(4)]);
        assert_eq!(
            bindings(&maps[1].1, "throttle"),
            &[Binding::axis(5, 1.0, -1.0)]
        );
        assert_eq!(bindings(&maps[1].1, "left"), &[] as &[Binding]);
        assert_eq!(crate::ui::store::stringify(&maps_to_json(&maps)), js);
    }
}
