//! The input layer (port of the keyboard and touch parts of
//! `src/game/Input.js`, SPEC 8.2, roadmap WP 4.5).
//!
//! Keyboard, gamepad ([`super::gamepad`], polled once a frame and handed in
//! with [`Input::pad_frame`]) and on-screen touch controls feed one state.
//! Keyboard and ◂ ▸
//! steering are ramped so tapping gives small corrections and holding gives
//! full lock, like an analogue stick. The layer runs at the tick rate (SPEC
//! 8.2): the session calls [`Input::update`] with the fixed tick before each
//! tick and quantises the result into an `InputFrame`.
//!
//! Keys are named by their DOM `KeyboardEvent.code` (`KeyW`, `ArrowUp`, …),
//! as in the JS; the Bevy glue maps its key codes to them ([`dom_code`]).
//! The gamepad came with WP 6.4 (DECISIONS D433, D782).

use super::gamepad::{self, Action};
use bevy::input::keyboard::KeyCode;
use mp_math::{clamp, kernel};
use mp_sim::input::Input as SimInput;

/// Held controls (`KEYMAP`).
pub const THROTTLE: &[&str] = &["KeyW", "ArrowUp"];
pub const BRAKE: &[&str] = &["KeyS", "ArrowDown"];
pub const LEFT: &[&str] = &["KeyA", "ArrowLeft"];
pub const RIGHT: &[&str] = &["KeyD", "ArrowRight"];
pub const HANDBRAKE: &[&str] = &["Space"];
pub const NITRO: &[&str] = &["ShiftLeft", "ShiftRight", "KeyN"];
pub const LOOK_BACK: &[&str] = &["KeyB"];
const KEYMAP: [&[&str]; 7] = [THROTTLE, BRAKE, LEFT, RIGHT, HANDBRAKE, NITRO, LOOK_BACK];

/// One-shot actions (`PRESS`), in the JS order.
pub const PRESS: [(&str, &[&str]); 4] = [
    ("camera", &["KeyC"]),
    ("reset", &["KeyR"]),
    ("pause", &["Escape", "KeyP"]),
    ("music", &["KeyM"]),
];

/// `Input.state`: what the controls ask for this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct State {
    pub throttle: f64,
    pub brake: f64,
    pub steer: f64,
    /// The steering is analogue (the stick, tilt or a gamepad): physics
    /// scales it to the grip (`CarPhysics` `ANALOG_LOCK`).
    pub analog: bool,
    pub handbrake: bool,
    pub nitro: bool,
    pub look_back: bool,
}

impl State {
    /// The controls physics reads (`cruise` stays off: only Race's
    /// cool-down driver sets it).
    pub fn sim(&self) -> SimInput {
        SimInput {
            steer: self.steer,
            throttle: self.throttle,
            brake: self.brake,
            handbrake: self.handbrake,
            nitro: self.nitro,
            analog: self.analog,
            cruise: false,
        }
    }
}

/// What on-screen controls offer `Input.update` (the `input.touch` object
/// the JS merges in): `TouchControls` in the game, a stand-in in the tests.
pub trait TouchSource {
    fn throttle(&self) -> f64;
    fn brake(&self) -> f64;
    /// The ◂ ▸ pads' target: −1, 0 or +1.
    fn steer(&self) -> f64;
    fn handbrake(&self) -> bool;
    fn nitro(&self) -> bool;
    /// This tick's analogue steering (the stick, or tilt), or `None` when
    /// steering is on the ◂ ▸ pads (`analogSteer?.(dt) ?? null`).
    fn analog_steer(&mut self, dt: f64) -> Option<f64>;
}

/// `Input`: keys held (`down`), one-shot actions waiting (`pressed`), the
/// ramped digital steering and the merged state.
#[derive(Clone, Debug)]
pub struct Input {
    /// Codes held, in the order they went down (a JS `Set`).
    pub down: Vec<&'static str>,
    /// One-shot actions since they were last consumed.
    pub pressed: Vec<&'static str>,
    /// The ramped steering.
    pub steer: f64,
    pub state: State,
    /// Off: every control reads zero (one-shot actions still get through).
    pub enabled: bool,
    /// The pads as this frame's poll left them (`this.pads.poll()`).
    pub pad: gamepad::State,
}

impl Default for Input {
    fn default() -> Self {
        Input {
            down: Vec::new(),
            pressed: Vec::new(),
            steer: 0.0,
            state: State::default(),
            enabled: true,
            pad: gamepad::State::default(),
        }
    }
}

/// The DOM code of a key the game knows by name, `None` for any other.
pub fn dom_code(k: KeyCode) -> Option<&'static str> {
    Some(match k {
        KeyCode::KeyW => "KeyW",
        KeyCode::ArrowUp => "ArrowUp",
        KeyCode::KeyS => "KeyS",
        KeyCode::ArrowDown => "ArrowDown",
        KeyCode::KeyA => "KeyA",
        KeyCode::ArrowLeft => "ArrowLeft",
        KeyCode::KeyD => "KeyD",
        KeyCode::ArrowRight => "ArrowRight",
        KeyCode::Space => "Space",
        KeyCode::ShiftLeft => "ShiftLeft",
        KeyCode::ShiftRight => "ShiftRight",
        KeyCode::KeyN => "KeyN",
        KeyCode::KeyB => "KeyB",
        KeyCode::KeyC => "KeyC",
        KeyCode::KeyR => "KeyR",
        KeyCode::Escape => "Escape",
        KeyCode::KeyP => "KeyP",
        KeyCode::KeyM => "KeyM",
        KeyCode::KeyT => "KeyT",
        KeyCode::Enter => "Enter",
        KeyCode::NumpadEnter => "NumpadEnter",
        KeyCode::Tab => "Tab",
        KeyCode::KeyQ => "KeyQ",
        _ => return None,
    })
}

/// JS `Math.sign` for the ramp's comparison (−0 and +0 compare equal there
/// too, so the sign of zero does not matter).
fn sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

impl Input {
    pub fn new() -> Input {
        Input::default()
    }

    /// `isGameKey`: the page keeps these keys (no scrolling).
    pub fn is_game_key(code: &str) -> bool {
        KEYMAP.iter().any(|c| c.contains(&code)) || PRESS.iter().any(|(_, c)| c.contains(&code))
    }

    /// The `keydown` listener. Returns whether the event is kept from the
    /// page (`preventDefault`): game keys are, repeats included. A repeat
    /// changes nothing else.
    pub fn key_down(&mut self, code: &'static str, repeat: bool) -> bool {
        if repeat {
            return Self::is_game_key(code);
        }
        if !self.down.contains(&code) {
            self.down.push(code);
        }
        for (name, codes) in PRESS {
            if codes.contains(&code) && !self.pressed.contains(&name) {
                self.pressed.push(name);
            }
        }
        Self::is_game_key(code)
    }

    /// The `keyup` listener.
    pub fn key_up(&mut self, code: &str) {
        self.down.retain(|&c| c != code);
    }

    /// The window lost focus: let go of every key.
    pub fn blur(&mut self) {
        self.down.clear();
    }

    /// Posts a one-shot action (a touch tap does this).
    pub fn press(&mut self, name: &'static str) {
        if !self.pressed.contains(&name) {
            self.pressed.push(name);
        }
    }

    /// This frame's poll of the pads (`const g = this.pads.poll()`): kept
    /// for the ticks' [`Input::update`]s, its one-shot presses posted once.
    pub fn pad_frame(&mut self, g: &gamepad::State) {
        self.pad = g.clone();
        for a in &g.edges {
            self.press(a.key());
        }
    }

    fn any(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.down.contains(c))
    }

    /// One-shot actions since the last call.
    pub fn consume(&mut self, name: &str) -> bool {
        let n = self.pressed.len();
        self.pressed.retain(|&p| p != name);
        self.pressed.len() != n
    }

    /// `update(dt)`: merges keys, the pads and touch into the state.
    pub fn update(&mut self, dt: f64, touch: Option<&mut dyn TouchSource>) -> State {
        let t = touch.as_deref();
        let mut throttle = if self.any(THROTTLE) {
            1.0
        } else {
            t.map_or(0.0, |t| t.throttle())
        };
        let mut brake = if self.any(BRAKE) {
            1.0
        } else {
            t.map_or(0.0, |t| t.brake())
        };
        // A pad's steering bound to buttons (the D-pad, say) ramps like keys.
        let l = self.any(LEFT) || self.pad.digital_left;
        let r = self.any(RIGHT) || self.pad.digital_right;
        let t_steer = t.map(|t| t.steer());
        let t_handbrake = t.is_some_and(|t| t.handbrake());
        let t_nitro = t.is_some_and(|t| t.nitro());
        // The thumb stick and tilt are analogue already, so they aren't
        // ramped; a held key still wins over them.
        let analog = touch.and_then(|t| t.analog_steer(dt));
        // Analogue steering (the stick, tilt or a gamepad) asks for a share of
        // what the tyres can give; keys and pads ask for a wheel angle.
        let mut analog_steer = analog.is_some() && !(l || r);
        if analog_steer {
            self.steer = analog.unwrap_or(0.0);
        } else {
            let target = if l || r {
                (if r { 1.0 } else { 0.0 }) - (if l { 1.0 } else { 0.0 })
            } else {
                t_steer.unwrap_or(0.0)
            };
            // Ramp toward target; snap back faster than we turn in.
            let rate = if target == 0.0 {
                7.0
            } else if sign(target) != sign(self.steer) && self.steer != 0.0 {
                9.0
            } else {
                3.6
            };
            self.steer += clamp(target - self.steer, -rate * dt, rate * dt);
        }
        let mut steer = self.steer;
        let mut handbrake = self.any(HANDBRAKE) || t_handbrake;
        let mut nitro = self.any(NITRO) || t_nitro;
        let mut look_back = self.any(LOOK_BACK);

        // Gamepad sticks and triggers (the standard layout unless remapped).
        let g = &self.pad;
        let ax = g.steer_axis;
        if ax.abs() > 0.12 {
            steer = mp_math::js::sign(ax) * kernel::pow((ax.abs() - 0.12) / 0.88, 1.4);
            analog_steer = true;
        }
        if g.value(Action::Throttle) > 0.05 {
            throttle = mp_math::js::max(throttle, g.value(Action::Throttle));
        }
        if g.value(Action::Brake) > 0.05 {
            brake = mp_math::js::max(brake, g.value(Action::Brake));
        }
        if g.held(Action::Nitro) {
            nitro = true;
        }
        if g.held(Action::Handbrake) {
            handbrake = true;
        }
        if g.held(Action::LookBack) {
            look_back = true;
        }
        if !self.enabled {
            throttle = 0.0;
            brake = 0.0;
            steer = 0.0;
            handbrake = false;
            nitro = false;
            analog_steer = false;
        }
        self.state = State {
            throttle,
            brake,
            steer: clamp(steer, -1.0, 1.0),
            analog: analog_steer,
            handbrake,
            nitro,
            look_back,
        };
        self.state
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/input.test.js`, and `gamepad.test.js`'s case with the
    //! input layer.
    use super::*;
    use crate::play::gamepad::{Binding, Button, NoRumble, Pad, Pads, default_map};

    /// `input.update(dt)` with `navigator.getGamepads()` returning `list`:
    /// the poll, then the update.
    fn pad_update(input: &mut Input, pads: &mut Pads, list: &[Pad], t: &mut f64, dt: f64) -> State {
        *t += dt * 1000.0;
        let g = pads.poll(*t, list, &mut NoRumble).clone();
        input.pad_frame(&g);
        input.update(dt, None)
    }

    fn std_pad(id: &str) -> Pad {
        Pad {
            id: id.into(),
            index: 0,
            mapping: "standard".into(),
            axes: vec![0.0; 4],
            buttons: vec![Button::default(); 17],
        }
    }

    fn near(a: f64, b: f64, eps: f64) {
        assert!((a - b).abs() <= eps, "{a} ≉ {b}");
    }

    #[derive(Default)]
    struct FakeTouch {
        throttle: f64,
        brake: f64,
        steer: f64,
        handbrake: bool,
        nitro: bool,
        stick: Option<f64>,
    }

    impl TouchSource for FakeTouch {
        fn throttle(&self) -> f64 {
            self.throttle
        }
        fn brake(&self) -> f64 {
            self.brake
        }
        fn steer(&self) -> f64 {
            self.steer
        }
        fn handbrake(&self) -> bool {
            self.handbrake
        }
        fn nitro(&self) -> bool {
            self.nitro
        }
        fn analog_steer(&mut self, _dt: f64) -> Option<f64> {
            self.stick
        }
    }

    #[test]
    fn the_keymap_pedals_handbrake_nitro_and_look_back() {
        let mut input = Input::new();
        type Get = fn(&State) -> f64;
        let cases: [(&'static str, Get, f64); 9] = [
            ("KeyW", |s| s.throttle, 1.0),
            ("ArrowUp", |s| s.throttle, 1.0),
            ("KeyS", |s| s.brake, 1.0),
            ("ArrowDown", |s| s.brake, 1.0),
            ("Space", |s| f64::from(u8::from(s.handbrake)), 1.0),
            ("ShiftLeft", |s| f64::from(u8::from(s.nitro)), 1.0),
            ("ShiftRight", |s| f64::from(u8::from(s.nitro)), 1.0),
            ("KeyN", |s| f64::from(u8::from(s.nitro)), 1.0),
            ("KeyB", |s| f64::from(u8::from(s.look_back)), 1.0),
        ];
        for (code, get, on) in cases {
            input.key_down(code, false);
            assert_eq!(get(&input.update(0.016, None)), on, "{code}");
            input.key_up(code);
            assert_eq!(get(&input.update(0.016, None)), 0.0, "{code} released");
        }
    }

    #[test]
    fn game_keys_are_kept_from_the_page_other_keys_are_not() {
        let mut input = Input::new();
        for code in ["ArrowUp", "ArrowDown", "Space", "KeyW", "Escape"] {
            assert!(input.key_down(code, false), "{code}");
        }
        assert!(!input.key_down("KeyQ", false));
        assert!(!input.key_down("Tab", false), "Tab still moves focus");
        assert!(input.key_down("ArrowUp", true), "auto-repeat too");
    }

    #[test]
    fn one_shot_actions_consumed_once() {
        let mut input = Input::new();
        for (code, name) in [
            ("KeyC", "camera"),
            ("KeyR", "reset"),
            ("Escape", "pause"),
            ("KeyP", "pause"),
            ("KeyM", "music"),
        ] {
            input.key_down(code, false);
            assert!(input.consume(name), "{code} → {name}");
            assert!(!input.consume(name), "only once");
            input.key_up(code);
        }
        // Holding a key down (auto-repeat) doesn't fire it again.
        input.key_down("KeyC", false);
        input.consume("camera");
        input.key_down("KeyC", true);
        assert!(!input.consume("camera"));
        assert!(!input.consume("nothing"));
    }

    #[test]
    fn steering_ramps_in_snaps_back_faster_and_full_lock_is_one() {
        let mut input = Input::new();
        input.key_down("KeyD", false);
        input.update(0.1, None);
        near(input.state.steer, 0.36, 1e-9); // 3.6 per second turning in
        for _ in 0..10 {
            input.update(0.1, None);
        }
        assert_eq!(input.state.steer, 1.0, "full lock");
        input.key_up("KeyD");
        input.update(0.1, None);
        near(input.state.steer, 0.3, 1e-9); // 7 per second back to centre
        input.update(0.1, None);
        input.update(0.1, None);
        assert_eq!(input.state.steer, 0.0, "centred, no overshoot");
        // Flicking from full right to left counter-steers at 9 per second.
        input.key_down("KeyD", false);
        for _ in 0..10 {
            input.update(0.1, None);
        }
        input.key_up("KeyD");
        input.key_down("KeyA", false);
        input.update(0.1, None);
        near(input.state.steer, 0.1, 1e-9);
        // Both at once cancel out.
        input.key_down("KeyD", false);
        for _ in 0..20 {
            input.update(0.1, None);
        }
        assert_eq!(input.state.steer, 0.0);
    }

    #[test]
    fn losing_focus_lets_go_of_every_key() {
        let mut input = Input::new();
        input.key_down("KeyW", false);
        input.key_down("KeyD", false);
        input.blur();
        let s = input.update(0.016, None);
        assert_eq!(s.throttle, 0.0);
        input.update(1.0, None);
        assert_eq!(input.state.steer, 0.0);
    }

    #[test]
    fn the_touch_pads_merge_in_and_the_keyboard_wins() {
        let mut input = Input::new();
        let mut touch = FakeTouch {
            throttle: 1.0,
            steer: -1.0,
            handbrake: true,
            nitro: true,
            ..FakeTouch::default()
        };
        let mut s = input.update(0.1, Some(&mut touch));
        assert_eq!(s.throttle, 1.0);
        assert!(s.handbrake);
        assert!(s.nitro);
        near(s.steer, -0.36, 1e-9); // touch steering ramps like a key
        touch.throttle = 0.0;
        touch.brake = 1.0;
        s = input.update(0.1, Some(&mut touch));
        assert_eq!(s.throttle, 0.0);
        assert_eq!(s.brake, 1.0);
        input.key_down("KeyD", false);
        for _ in 0..20 {
            s = input.update(0.1, Some(&mut touch));
        }
        assert_eq!(s.steer, 1.0, "a held key overrides the steering pad");
        input.key_down("KeyW", false);
        assert_eq!(input.update(0.1, Some(&mut touch)).throttle, 1.0);
    }

    #[test]
    fn steering_is_analogue_from_the_stick_not_keys_or_pads() {
        let mut input = Input::new();
        let mut touch = FakeTouch {
            stick: Some(0.4),
            ..FakeTouch::default()
        };
        let s = input.update(0.016, Some(&mut touch));
        assert!(s.analog, "the stick");
        near(s.steer, 0.4, 1e-9);
        input.key_down("KeyA", false);
        let s = input.update(0.016, Some(&mut touch));
        assert!(!s.analog, "a held key wins, and is a key");
        input.key_up("KeyA");
        touch.stick = None; // ◂ ▸ pads: no analogue reading
        touch.steer = 1.0;
        assert!(!input.update(0.016, Some(&mut touch)).analog, "the pads");
    }

    #[test]
    fn enabled_false_zeroes_everything() {
        let mut input = Input::new();
        let mut touch = FakeTouch {
            throttle: 1.0,
            brake: 1.0,
            steer: 1.0,
            handbrake: true,
            nitro: true,
            stick: None,
        };
        input.key_down("KeyW", false);
        input.key_down("Space", false);
        input.key_down("ShiftLeft", false);
        for _ in 0..5 {
            input.update(0.1, Some(&mut touch));
        }
        input.enabled = false;
        let s = input.update(0.1, Some(&mut touch));
        assert_eq!(
            State {
                look_back: false,
                ..s
            },
            State::default()
        );
        // One-shot actions still get through (pause works on the menu).
        input.key_down("Escape", false);
        assert!(input.consume("pause"));
    }

    #[test]
    fn a_gamepad_analogue_steering_with_a_dead_zone_triggers_and_buttons() {
        let mut input = Input::new();
        let mut pads = Pads::default();
        let mut t = 0.0;
        // `{ connected: true, axes: [0, 0], buttons }`: no id, no mapping.
        let mut pad = Pad {
            axes: vec![0.0, 0.0],
            buttons: vec![Button::default(); 17],
            ..Pad::default()
        };
        let mut up = |input: &mut Input, pad: &Pad| {
            pad_update(input, &mut pads, std::slice::from_ref(pad), &mut t, 0.016)
        };
        assert_eq!(up(&mut input, &pad).steer, 0.0);
        pad.axes[0] = 0.1;
        assert_eq!(up(&mut input, &pad).steer, 0.0, "inside the dead zone");
        pad.axes[0] = 0.56;
        near(
            up(&mut input, &pad).steer,
            kernel::pow((0.56 - 0.12) / 0.88, 1.4),
            1e-9,
        );
        pad.axes[0] = -1.0;
        assert_eq!(up(&mut input, &pad).steer, -1.0);
        pad.axes[0] = 0.0;
        pad.buttons[7].value = 0.6;
        pad.buttons[6].value = 0.3;
        let s = up(&mut input, &pad);
        near(s.throttle, 0.6, 1e-9);
        near(s.brake, 0.3, 1e-9);
        pad.buttons[0].pressed = true;
        pad.buttons[2].pressed = true;
        pad.buttons[1].pressed = true;
        let s = up(&mut input, &pad);
        assert!(s.nitro, "A: nitro");
        assert!(s.handbrake, "X: handbrake");
        assert!(s.look_back, "B: look back");
        pad.buttons[2].pressed = false;
        pad.buttons[5].pressed = true;
        assert!(up(&mut input, &pad).handbrake, "RB: handbrake");
        // Y, Start and Back fire once per press.
        for (i, name) in [(3, "camera"), (9, "pause"), (8, "reset")] {
            pad.buttons[i].pressed = true;
            up(&mut input, &pad);
            assert!(input.consume(name), "button {i} → {name}");
            up(&mut input, &pad);
            assert!(!input.consume(name), "held: no repeat");
            pad.buttons[i].pressed = false;
            up(&mut input, &pad);
        }
        // A disconnected pad is ignored (`connectedPads` leaves it out).
        pad.buttons[7].value = 1.0;
        let s = pad_update(&mut input, &mut pads, &[], &mut t, 0.016);
        assert_eq!(s.throttle, 0.0);
    }

    #[test]
    fn steering_is_flagged_analogue_from_a_gamepad_stick_not_at_rest() {
        let mut input = Input::new();
        let mut pads = Pads::default();
        let mut t = 0.0;
        let mut pad = std_pad("");
        pad.axes[0] = 0.6;
        let s = pad_update(
            &mut input,
            &mut pads,
            std::slice::from_ref(&pad),
            &mut t,
            0.016,
        );
        assert!(s.analog, "a gamepad stick");
        pad.axes[0] = 0.05;
        let s = pad_update(
            &mut input,
            &mut pads,
            std::slice::from_ref(&pad),
            &mut t,
            0.016,
        );
        assert!(!s.analog, "a gamepad at rest leaves it to the keys");
    }

    #[test]
    fn a_remapped_dpad_steers_like_keys_ramped_not_analogue() {
        let mut input = Input::new();
        let mut map = default_map();
        map[0].1 = vec![Binding::Button(14)];
        map[1].1 = vec![Binding::Button(15)];
        let mut pads = Pads::new(vec![("Pad".into(), map)]);
        let mut t = 0.0;
        let mut p = std_pad("Pad");
        p.buttons[15] = Button {
            pressed: true,
            value: 1.0,
        };
        let mut s = pad_update(&mut input, &mut pads, std::slice::from_ref(&p), &mut t, 0.1);
        near(s.steer, 0.36, 1e-9); // ramps in like a key
        assert!(!s.analog);
        for _ in 0..10 {
            s = pad_update(&mut input, &mut pads, std::slice::from_ref(&p), &mut t, 0.1);
        }
        assert_eq!(s.steer, 1.0);
        p.buttons[15] = Button::default();
        p.axes[0] = 0.8; // the stick isn't bound any more
        for _ in 0..10 {
            s = pad_update(&mut input, &mut pads, std::slice::from_ref(&p), &mut t, 0.1);
        }
        assert_eq!(s.steer, 0.0);
    }
}
