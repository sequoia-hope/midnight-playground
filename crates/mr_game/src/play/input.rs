//! The input layer (port of the keyboard and touch parts of
//! `src/game/Input.js`, SPEC 8.2, roadmap WP 4.5).
//!
//! Keyboard and on-screen touch controls feed one state. Keyboard and ◂ ▸
//! steering are ramped so tapping gives small corrections and holding gives
//! full lock, like an analogue stick. The layer runs at the tick rate (SPEC
//! 8.2): the session calls [`Input::update`] with the fixed tick before each
//! tick and quantises the result into an `InputFrame`.
//!
//! Keys are named by their DOM `KeyboardEvent.code` (`KeyW`, `ArrowUp`, …),
//! as in the JS; the Bevy glue maps its key codes to them ([`dom_code`]).
//! The gamepad (`Pads`) arrives with M6 (DECISIONS D433).

use bevy::input::keyboard::KeyCode;
use mr_math::clamp;
use mr_sim::input::Input as SimInput;

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
}

impl Default for Input {
    fn default() -> Self {
        Input {
            down: Vec::new(),
            pressed: Vec::new(),
            steer: 0.0,
            state: State::default(),
            enabled: true,
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

    fn any(&self, codes: &[&str]) -> bool {
        codes.iter().any(|c| self.down.contains(c))
    }

    /// One-shot actions since the last call.
    pub fn consume(&mut self, name: &str) -> bool {
        let n = self.pressed.len();
        self.pressed.retain(|&p| p != name);
        self.pressed.len() != n
    }

    /// `update(dt)`: merges keys and touch into the state.
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
        let l = self.any(LEFT);
        let r = self.any(RIGHT);
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
        let look_back = self.any(LOOK_BACK);
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
    //! `test/unit/input.test.js`, the assertions that apply without a
    //! gamepad (M6).
    use super::*;

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
}
