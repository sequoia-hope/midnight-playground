//! The pads in the client (WP 6.4): the platform's gamepads read into
//! [`gamepad::Pad`]s once a frame and polled ([`gamepad::Pads::poll`])
//! before anything else of the frame runs, as `main.js`'s `tick` runs
//! `input.update` first; the poll's state handed to the race's input layer
//! ([`super::input::Input::pad_frame`]); the race's rumble (`kick`, `feel`)
//! and quieting (`hush`) carried to the pads; the maps saved under
//! `mr.padMaps` and the Rumble setting followed.
//!
//! - **Web:** `navigator.getGamepads()` read through `js_sys::Reflect`,
//!   property by property, as the JS reads it (the `standard` mapping's
//!   button and axis indices, the browser's id string, `mapping`), and the
//!   rumble through `vibrationActuator.playEffect('dual-rumble')` or
//!   `hapticActuators[0].pulse` (DECISIONS D780).
//! - **Native:** gilrs through Bevy (`bevy_gilrs`), from its raw events
//!   (not Bevy's filtered `Gamepad` state), laid out as the standard
//!   mapping; rumble as Bevy's `GamepadRumbleRequest` (D781).

use super::gamepad::{self, Action, Pads};
use super::{Play, PlayFrame};
use crate::loader::AppState;
use crate::ui::UiState;
use crate::ui::store::Store;
use bevy::ecs::system::ScheduleSystem;
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::time::Real;

/// The pads, for every screen and the race.
#[derive(Resource, Default)]
pub struct PadsRes {
    pub pads: Pads,
    loaded: bool,
    /// The Rumble setting last seen.
    rumble: Option<bool>,
    logged: String,
}

pub fn plugin(app: &mut App) {
    app.insert_resource(PadsRes::default())
        .add_systems(PreUpdate, poll.after(InputSystems))
        .add_systems(
            Update,
            feed.in_set(PlayFrame)
                .after(super::read_input)
                .before(super::step)
                .run_if(in_state(AppState::Running)),
        );
    #[cfg(not(target_arch = "wasm32"))]
    app.init_resource::<native::NativePads>();
}

/// Adds the Controller screen's frame (`crate::ui::pad_setup::frame`)
/// where `main.js` ran `padSetup.escape()`: after the pads reach the race's
/// input layer, before its ticks could take Esc or Start as un-pause.
pub fn pad_setup_frame<M>(app: &mut App, sys: impl IntoScheduleConfigs<ScheduleSystem, M>) {
    app.add_systems(
        Update,
        sys.in_set(PlayFrame)
            .after(feed)
            .before(super::step)
            .run_if(in_state(AppState::Running)),
    );
}

/// The frame's poll, after what the last frame asked of the pads: the
/// maps from the store (once), the Rumble setting, the race's hush, kicks
/// and buzz; then the pads read and polled, and changed maps saved.
#[allow(clippy::too_many_arguments)]
fn poll(
    mut res: ResMut<PadsRes>,
    play: Option<ResMut<Play>>,
    store: Option<ResMut<Store>>,
    ui: Option<Res<UiState>>,
    time: Res<Time<Real>>,
    #[cfg(not(target_arch = "wasm32"))] mut native: ResMut<native::NativePads>,
    #[cfg(not(target_arch = "wasm32"))] mut connections: MessageReader<
        bevy::input::gamepad::GamepadConnectionEvent,
    >,
    #[cfg(not(target_arch = "wasm32"))] mut raw: MessageReader<
        bevy::input::gamepad::RawGamepadEvent,
    >,
    #[cfg(not(target_arch = "wasm32"))] mut rumble_out: MessageWriter<
        bevy::input::gamepad::GamepadRumbleRequest,
    >,
) {
    let res = &mut *res;
    let mut store = store;
    if !res.loaded {
        res.loaded = true;
        // `maps: store.get('padMaps', {})`.
        if let Some(v) = store.as_ref().and_then(|s| s.get("padMaps")) {
            res.pads.maps = gamepad::maps_from_json(&v);
        }
    }
    // `rumbleOn: settings.rumble`; the switch turned: `pads.rumbleOn =
    // el.checked; pads.kick(0.5, 0.7, 300)` (a buzz to show it's on).
    if let Some(ui) = &ui {
        let on = ui.settings.rumble;
        if res.rumble != Some(on) {
            res.pads.rumble_on = on;
            if res.rumble.is_some() {
                res.pads.kick(0.5, 0.7, 300.0);
            }
            res.rumble = Some(on);
        }
    }
    if let Some(mut play) = play
        && let Some(race) = play.race.as_mut()
    {
        if std::mem::take(&mut race.hush_pads) {
            res.pads.hush();
        }
        for (s, w, ms) in race.kicks.drain(..) {
            res.pads.kick(s, w, ms);
        }
        if let Some((s, w)) = race.feel.take()
            && res.pads.state.connected
        {
            res.pads.feel(s, w);
        }
    }
    let now = time.elapsed_secs_f64() * 1000.0;
    #[cfg(target_arch = "wasm32")]
    {
        let list = web::read();
        res.pads.poll(now, &list, &mut web::WebRumble);
        web::publish(&res.pads);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native.connect(connections.read());
        native.apply(raw.read());
        let list = native.pads();
        let mut out = native::NativeRumble {
            native: &native,
            out: &mut rumble_out,
        };
        res.pads.poll(now, &list, &mut out);
    }
    log_changes(&res.pads, &mut res.logged);
    if res.pads.maps_changed {
        res.pads.maps_changed = false;
        if let Some(store) = store.as_mut() {
            store.set("padMaps", &gamepad::maps_to_json(&res.pads.maps));
        }
    }
}

/// `RUST_LOG=mr_game::play::gamepad_io=debug`: what the pads ask for,
/// each time it changes (for trying a controller natively).
fn log_changes(pads: &Pads, last: &mut String) {
    let s = &pads.state;
    let held: Vec<&str> = Action::ALL
        .into_iter()
        .filter(|a| s.held(*a))
        .map(Action::key)
        .collect();
    let nav: Vec<&str> = gamepad::Nav::ALL
        .into_iter()
        .filter(|n| s.nav(*n))
        .map(gamepad::Nav::key)
        .collect();
    let line = format!(
        "pads: connected {} active {:?} held {held:?} nav {nav:?} steer {:.2} throttle {:.2} brake {:.2}",
        s.connected,
        pads.active.as_ref().map(|p| p.id.as_str()),
        s.steer_axis,
        s.value(Action::Throttle),
        s.value(Action::Brake),
    );
    if line != *last {
        debug!("{line}");
        *last = line;
    }
}

/// The race's input layer takes this frame's poll (its one-shot presses
/// once), and the stuck hint the reset button's name.
fn feed(res: Res<PadsRes>, mut play: ResMut<Play>) {
    let Some(race) = play.race.as_mut() else {
        return;
    };
    let pads = &res.pads;
    race.input.pad_frame(&pads.state);
    race.pad_reset = pads.state.connected.then(|| pads.label(Action::Reset));
}

#[cfg(target_arch = "wasm32")]
mod web {
    //! `navigator.getGamepads()` and the actuators, through `Reflect` so a
    //! page's own `getGamepads` (the tests' fake pad) is read as the JS
    //! game reads it.
    use super::gamepad::{Action, Button, Nav, Pad, Pads, Rumble};
    use js_sys::{Array, Function, Object, Promise, Reflect};
    use std::cell::RefCell;
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    thread_local! {
        /// This frame's pad objects by index, for the rumble.
        static LIVE: RefCell<Vec<(usize, JsValue)>> = const { RefCell::new(Vec::new()) };
        /// `() => {}` for a promise's `catch`.
        static NOOP: Closure<dyn FnMut(JsValue)> = Closure::new(|_| {});
    }

    fn get(o: &JsValue, k: &str) -> JsValue {
        Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
    }

    fn truthy(v: &JsValue) -> bool {
        v.is_truthy()
    }

    /// `connectedPads()`.
    pub fn read() -> Vec<Pad> {
        let Some(w) = web_sys::window() else {
            return Vec::new();
        };
        let nav: JsValue = w.navigator().into();
        let f = get(&nav, "getGamepads");
        let Some(f) = f.dyn_ref::<Function>() else {
            LIVE.with(|l| l.borrow_mut().clear());
            return Vec::new();
        };
        let Ok(list) = f.call0(&nav) else {
            return Vec::new();
        };
        let list = Array::from(&list);
        let mut pads = Vec::new();
        let mut live = Vec::new();
        for p in list.iter() {
            if p.is_null() || p.is_undefined() || !truthy(&get(&p, "connected")) {
                continue;
            }
            let index = get(&p, "index").as_f64().unwrap_or(0.0) as usize;
            let axes = Array::from(&get(&p, "axes"))
                .iter()
                .map(|v| v.as_f64().unwrap_or(f64::NAN))
                .collect();
            let buttons = Array::from(&get(&p, "buttons"))
                .iter()
                .map(|b| Button {
                    pressed: truthy(&get(&b, "pressed")),
                    // `x.value || 0`.
                    value: get(&b, "value")
                        .as_f64()
                        .filter(|v| !v.is_nan())
                        .unwrap_or(0.0),
                })
                .collect();
            pads.push(Pad {
                id: get(&p, "id").as_string().unwrap_or_default(),
                index,
                mapping: get(&p, "mapping").as_string().unwrap_or_default(),
                axes,
                buttons,
            });
            live.push((index, p));
        }
        LIVE.with(|l| *l.borrow_mut() = live);
        pads
    }

    fn live(index: usize) -> Option<JsValue> {
        LIVE.with(|l| {
            l.borrow()
                .iter()
                .find(|(i, _)| *i == index)
                .map(|(_, p)| p.clone())
        })
    }

    /// `promise?.catch?.(() => {})`.
    fn quiet(v: JsValue) {
        if let Some(p) = v.dyn_ref::<Promise>() {
            NOOP.with(|n| {
                let _ = p.catch(n);
            });
        } else if let Some(c) = get(&v, "catch").dyn_ref::<Function>() {
            NOOP.with(|n| {
                let _ = c.call1(&v, n.as_ref());
            });
        }
    }

    pub struct WebRumble;

    impl Rumble for WebRumble {
        fn play(&mut self, pad: &Pad, strong: f64, weak: f64, duration: f64) {
            let Some(p) = live(pad.index) else { return };
            let act = get(&p, "vibrationActuator");
            if let Some(f) = get(&act, "playEffect").dyn_ref::<Function>() {
                let o = Object::new();
                let _ = Reflect::set(&o, &"duration".into(), &duration.into());
                let _ = Reflect::set(&o, &"strongMagnitude".into(), &strong.into());
                let _ = Reflect::set(&o, &"weakMagnitude".into(), &weak.into());
                if let Ok(r) = f.call2(&act, &"dual-rumble".into(), &o) {
                    quiet(r);
                }
            } else {
                let h = get(&get(&p, "hapticActuators"), "0");
                if let Some(f) = get(&h, "pulse").dyn_ref::<Function>()
                    && let Ok(r) = f.call2(&h, &strong.max(weak).into(), &duration.into())
                {
                    quiet(r);
                }
            }
        }

        fn reset(&mut self, pad: &Pad) {
            let Some(p) = live(pad.index) else { return };
            let act = get(&p, "vibrationActuator");
            if let Some(f) = get(&act, "reset").dyn_ref::<Function>()
                && let Ok(r) = f.call0(&act)
            {
                quiet(r);
            }
        }
    }

    /// `window.__mr.pads` for the tests (`window.__pads` in the JS): the
    /// state of the last poll, the pad in hand, a capture in progress.
    pub fn publish(pads: &Pads) {
        let Some(w) = web_sys::window() else { return };
        let mr = get(&w.into(), "__mr");
        if !mr.is_object() {
            return;
        }
        let set = |o: &Object, k: &str, v: JsValue| {
            let _ = Reflect::set(o, &JsValue::from_str(k), &v);
        };
        let s = &pads.state;
        let o = Object::new();
        set(&o, "connected", s.connected.into());
        let value = Object::new();
        let held = Object::new();
        for a in Action::ALL {
            set(&value, a.key(), s.value(a).into());
            set(&held, a.key(), s.held(a).into());
        }
        set(&o, "value", value.into());
        set(&o, "held", held.into());
        let nav = Object::new();
        for n in Nav::ALL {
            set(&nav, n.key(), s.nav(n).into());
        }
        set(&o, "nav", nav.into());
        set(&o, "steerAxis", s.steer_axis.into());
        set(
            &o,
            "active",
            pads.active
                .as_ref()
                .map_or(JsValue::NULL, |p| (p.index as f64).into()),
        );
        set(
            &o,
            "capture",
            pads.capture
                .as_ref()
                .map_or(JsValue::NULL, |c| c.action.key().into()),
        );
        set(&o, "rumbleOn", pads.rumble_on.into());
        set(&o, "resetLabel", pads.label(Action::Reset).into());
        let _ = Reflect::set(&mr, &"pads".into(), &o);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    //! gilrs's pads (through Bevy's raw gamepad events) as the browser's
    //! standard mapping lays them out: buttons 0–16 (A B X Y, LB RB, LT RT,
    //! Back Start, the stick presses, the D-pad, Home), axes 0–3 (the
    //! sticks, y down).
    use super::gamepad::{Button, Pad, Rumble};
    use bevy::input::gamepad::{
        GamepadAxis, GamepadButton, GamepadConnection, GamepadConnectionEvent,
        GamepadRumbleIntensity, GamepadRumbleRequest, RawGamepadEvent,
    };
    use bevy::prelude::*;
    use std::time::Duration;

    /// Chrome's `pressed` for an analogue button (`kDefaultButtonPressedThreshold`,
    /// 30/255).
    const PRESSED: f32 = 30.0 / 255.0;

    #[derive(Clone, Debug)]
    struct Slot {
        entity: Entity,
        pad: Pad,
    }

    #[derive(Resource, Default)]
    pub struct NativePads {
        slots: Vec<Slot>,
    }

    fn button_index(b: GamepadButton) -> Option<usize> {
        Some(match b {
            GamepadButton::South => 0,
            GamepadButton::East => 1,
            GamepadButton::West => 2,
            GamepadButton::North => 3,
            GamepadButton::LeftTrigger => 4,
            GamepadButton::RightTrigger => 5,
            GamepadButton::LeftTrigger2 => 6,
            GamepadButton::RightTrigger2 => 7,
            GamepadButton::Select => 8,
            GamepadButton::Start => 9,
            GamepadButton::LeftThumb => 10,
            GamepadButton::RightThumb => 11,
            GamepadButton::DPadUp => 12,
            GamepadButton::DPadDown => 13,
            GamepadButton::DPadLeft => 14,
            GamepadButton::DPadRight => 15,
            GamepadButton::Mode => 16,
            _ => return None,
        })
    }

    /// The standard mapping's axis, and the sign that turns gilrs's (y up)
    /// into the browser's (y down).
    fn axis_index(a: GamepadAxis) -> Option<(usize, f64)> {
        Some(match a {
            GamepadAxis::LeftStickX => (0, 1.0),
            GamepadAxis::LeftStickY => (1, -1.0),
            GamepadAxis::RightStickX => (2, 1.0),
            GamepadAxis::RightStickY => (3, -1.0),
            _ => return None,
        })
    }

    /// The browser's id string, so the Controller screen names it the same
    /// way: "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e
    /// Product: 0b13)".
    fn id_of(name: &str, vendor: Option<u16>, product: Option<u16>) -> String {
        match (vendor, product) {
            (Some(v), Some(p)) => {
                format!("{name} (STANDARD GAMEPAD Vendor: {v:04x} Product: {p:04x})")
            }
            _ => format!("{name} (STANDARD GAMEPAD)"),
        }
    }

    impl NativePads {
        /// Pads coming and going (gilrs reports the ones there at start
        /// only as `GamepadConnectionEvent`s, not raw events).
        pub fn connect<'a>(&mut self, events: impl Iterator<Item = &'a GamepadConnectionEvent>) {
            for c in events {
                match &c.connection {
                    GamepadConnection::Connected {
                        name,
                        vendor_id,
                        product_id,
                    } => {
                        if self.slots.iter().any(|s| s.entity == c.gamepad) {
                            continue;
                        }
                        // The lowest free index, as a browser hands
                        // them out.
                        let index = (0..)
                            .find(|i| !self.slots.iter().any(|s| s.pad.index == *i))
                            .unwrap_or(0);
                        info!("gamepad {index}: {name}");
                        self.slots.push(Slot {
                            entity: c.gamepad,
                            pad: Pad {
                                id: id_of(name, *vendor_id, *product_id),
                                index,
                                mapping: "standard".into(),
                                axes: vec![0.0; 4],
                                buttons: vec![Button::default(); 17],
                            },
                        });
                    }
                    GamepadConnection::Disconnected => {
                        self.slots.retain(|s| s.entity != c.gamepad);
                    }
                }
            }
        }

        /// Button and axis changes (connections are [`NativePads::connect`]'s).
        pub fn apply<'a>(&mut self, events: impl Iterator<Item = &'a RawGamepadEvent>) {
            for e in events {
                match e {
                    RawGamepadEvent::Connection(_) => {}
                    RawGamepadEvent::Button(b) => {
                        let Some(i) = button_index(b.button) else {
                            continue;
                        };
                        if let Some(s) = self.slots.iter_mut().find(|s| s.entity == b.gamepad) {
                            s.pad.buttons[i] = Button {
                                pressed: b.value > PRESSED,
                                value: f64::from(b.value),
                            };
                        }
                    }
                    RawGamepadEvent::Axis(a) => {
                        let Some((i, sign)) = axis_index(a.axis) else {
                            continue;
                        };
                        if let Some(s) = self.slots.iter_mut().find(|s| s.entity == a.gamepad) {
                            s.pad.axes[i] = f64::from(a.value) * sign;
                        }
                    }
                }
            }
        }

        /// The connected pads, by index (`navigator.getGamepads()`'s order).
        pub fn pads(&self) -> Vec<Pad> {
            let mut v: Vec<Pad> = self.slots.iter().map(|s| s.pad.clone()).collect();
            v.sort_by_key(|p| p.index);
            v
        }

        fn entity(&self, index: usize) -> Option<Entity> {
            self.slots
                .iter()
                .find(|s| s.pad.index == index)
                .map(|s| s.entity)
        }
    }

    /// Bevy's rumble requests: a new effect replaces the last (Stop, then
    /// Add), as `playEffect` does.
    pub struct NativeRumble<'a, 'w> {
        pub native: &'a NativePads,
        pub out: &'a mut MessageWriter<'w, GamepadRumbleRequest>,
    }

    impl Rumble for NativeRumble<'_, '_> {
        fn play(&mut self, pad: &Pad, strong: f64, weak: f64, duration: f64) {
            let Some(gamepad) = self.native.entity(pad.index) else {
                return;
            };
            self.out.write(GamepadRumbleRequest::Stop { gamepad });
            self.out.write(GamepadRumbleRequest::Add {
                gamepad,
                duration: Duration::from_secs_f64(duration / 1000.0),
                intensity: GamepadRumbleIntensity {
                    strong_motor: strong as f32,
                    weak_motor: weak as f32,
                },
            });
        }

        fn reset(&mut self, pad: &Pad) {
            if let Some(gamepad) = self.native.entity(pad.index) {
                self.out.write(GamepadRumbleRequest::Stop { gamepad });
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use bevy::input::gamepad::{RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent};

        #[test]
        fn gilrs_events_read_as_the_standard_mapping() {
            let e = Entity::from_raw_u32(7).unwrap();
            let mut n = NativePads::default();
            n.connect(
                [GamepadConnectionEvent::new(
                    e,
                    GamepadConnection::Connected {
                        name: "Xbox Wireless Controller".into(),
                        vendor_id: Some(0x045e),
                        product_id: Some(0x0b13),
                    },
                )]
                .iter(),
            );
            let events = [
                RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                    e,
                    GamepadButton::RightTrigger2,
                    0.4,
                )),
                RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                    e,
                    GamepadButton::DPadLeft,
                    1.0,
                )),
                RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
                    e,
                    GamepadAxis::LeftStickY,
                    0.5,
                )),
            ];
            n.apply(events.iter());
            let p = &n.pads()[0];
            assert_eq!(
                p.id,
                "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)"
            );
            assert_eq!(p.index, 0);
            assert!(p.standard());
            assert!(p.buttons[7].pressed);
            assert!((p.buttons[7].value - 0.4).abs() < 1e-6);
            assert!(p.buttons[14].pressed);
            assert_eq!(p.axes[1], -0.5, "stick up is negative, as in a browser");
            n.connect(
                [GamepadConnectionEvent::new(
                    e,
                    GamepadConnection::Disconnected,
                )]
                .iter(),
            );
            assert!(n.pads().is_empty());
        }
    }
}
