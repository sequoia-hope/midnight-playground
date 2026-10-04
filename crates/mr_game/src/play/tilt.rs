//! Tilt steering for phones (port of `src/game/TiltSteer.js`, SPEC 8.2,
//! roadmap WP 6.6): hold the phone like a steering wheel and turn it. The
//! steering follows the screen's roll, how far its left-right axis leans
//! off level, taken from the gravity direction in deviceorientation's beta
//! and gamma. Level is straight ahead, so there is nothing to calibrate,
//! and it reads the same with the phone upright or lying back in your
//! hands, in either landscape and in portrait.
//!
//! Browsers only send the sensor to secure pages (https), and iPhones ask
//! the player first: `requestPermission()` has to run inside a tap, so the
//! page's gesture bridge asks in the Race tap and in the tap that picks
//! Tilt (`crate::ui::web::gesture_at`), and [`TiltSteer::enable`] asks
//! again whenever it is turned on (outside a tap an iPhone refuses, which
//! is the 'ask' state). Chrome sends one event of nulls when there is no
//! sensor, and after that only sends when the reading changes, so a phone
//! held still goes quiet.
//!
//! [`TiltSteer`] is the JS class over a [`TiltWindow`] (the JS's `win`):
//! the browser on the web ([`web`]), no sensor natively. The promise of
//! `requestPermission` comes back as [`TiltSteer::answer`], the events as
//! [`TiltSteer::on_orientation`], and the JS's 2 s `setTimeout` is a
//! deadline that [`TiltSteer::poll`] checks (DECISIONS D841).

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use mr_math::{clamp, js, kernel};

use super::Play;

const DEG: f64 = std::f64::consts::PI / 180.0;
/// A steady hand's wobble round level.
const DEAD: f64 = 2.0 * DEG;
/// s: settles sensor jitter, still quick on the wheel.
const SMOOTH: f64 = 0.05;
/// s: how long `listen` waits for a first reading before saying 'none'.
const WAIT: f64 = 2.0;

/// The screen's roll in radians, positive when its right-hand edge dips,
/// from beta and gamma (degrees) and the angle the picture is turned from
/// the phone's natural orientation (`window.orientation`: 90 with the
/// phone turned anticlockwise, -90 or 270 clockwise).
pub fn screen_roll(beta: f64, gamma: f64, angle: f64) -> f64 {
    let (b, g, a) = (beta * DEG, gamma * DEG, angle * DEG);
    // "Up" in the phone's own axes (x to the right, y to the top, z out of
    // the glass), and the screen's right-hand edge in the same axes.
    let ux = -kernel::cos(b) * kernel::sin(g);
    let uy = kernel::sin(b);
    let rx = kernel::cos(a);
    let ry = -kernel::sin(a);
    kernel::asin(clamp(-(ux * rx + uy * ry), -1.0, 1.0))
}

/// Roll to steering, −1…+1: a small dead zone round level, then a gentle
/// curve (fine corrections near centre) up to full lock at `full` radians.
pub fn roll_to_steer(roll: f64, full: f64) -> f64 {
    let a = roll.abs();
    if a <= DEAD {
        return 0.0;
    }
    js::sign(roll) * kernel::pow(f64::min(1.0, (a - DEAD) / (full - DEAD)), 1.3)
}

/// Sensitivity 0…1 to the roll that gives full lock: 40° down to 12°.
pub fn full_lock_for(k: f64) -> f64 {
    (40.0 - 28.0 * clamp(k, 0.0, 1.0)) * DEG
}

/// `TiltSteer.state`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TiltState {
    /// Not chosen.
    #[default]
    Off,
    /// Listening, nothing yet.
    Waiting,
    Live,
    /// No sensor.
    None,
    /// An http page.
    Insecure,
    /// iPhone: needs a tap.
    Ask,
    Denied,
}

impl TiltState {
    pub fn name(self) -> &'static str {
        match self {
            TiltState::Off => "off",
            TiltState::Waiting => "waiting",
            TiltState::Live => "live",
            TiltState::None => "none",
            TiltState::Insecure => "insecure",
            TiltState::Ask => "ask",
            TiltState::Denied => "denied",
        }
    }

    const ALL: [TiltState; 7] = [
        TiltState::Off,
        TiltState::Waiting,
        TiltState::Live,
        TiltState::None,
        TiltState::Insecure,
        TiltState::Ask,
        TiltState::Denied,
    ];
}

/// What `requestPermission()`'s promise settled with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// `'granted'`.
    Granted,
    /// Any other answer (`'denied'`).
    Denied,
    /// Rejected: not inside a tap.
    Failed,
}

/// What `TiltSteer` uses of the window.
pub trait TiltWindow {
    /// `window.DeviceOrientationEvent` exists.
    fn has_api(&self) -> bool;
    /// `typeof DeviceOrientationEvent.requestPermission === 'function'`.
    fn needs_permission(&self) -> bool;
    /// Calls `requestPermission()`; false if it threw. Its promise's
    /// outcome comes back through [`TiltSteer::answer`].
    fn request_permission(&mut self) -> bool;
    /// Adds the `deviceorientation` listener (removing it first, so there
    /// is only ever one), or removes it.
    fn listen(&mut self, on: bool);
    /// `isSecureContext === false`.
    fn insecure(&self) -> bool;
}

/// No sensor (natively): `DeviceOrientationEvent` is missing.
pub struct NoSensor;

impl TiltWindow for NoSensor {
    fn has_api(&self) -> bool {
        false
    }
    fn needs_permission(&self) -> bool {
        false
    }
    fn request_permission(&mut self) -> bool {
        false
    }
    fn listen(&mut self, _on: bool) {}
    fn insecure(&self) -> bool {
        false
    }
}

/// `TiltSteer`.
#[derive(Clone, Debug)]
pub struct TiltSteer {
    /// The player's choice.
    pub on: bool,
    pub state: TiltState,
    pub granted: bool,
    pub roll: f64,
    pub steer: f64,
    pub full_lock: f64,
    /// When a 'waiting' with no reading becomes 'none' (the JS's timer).
    timer: Option<f64>,
    /// Every state it went to (`onChange`), until the owner takes them.
    pub changes: Vec<TiltState>,
}

impl Default for TiltSteer {
    fn default() -> Self {
        TiltSteer {
            on: false,
            state: TiltState::Off,
            granted: false,
            roll: 0.0,
            steer: 0.0,
            full_lock: full_lock_for(0.5),
            timer: None,
            changes: Vec::new(),
        }
    }
}

impl TiltSteer {
    pub fn new() -> TiltSteer {
        TiltSteer::default()
    }

    pub fn live(&self) -> bool {
        self.on && self.state == TiltState::Live
    }

    pub fn set_sensitivity(&mut self, k: f64) {
        self.full_lock = full_lock_for(k);
    }

    fn set(&mut self, state: TiltState) {
        if state == self.state {
            return;
        }
        self.state = state;
        self.changes.push(state);
    }

    /// Turn tilt steering on or off (`now` in seconds, for the wait). On an
    /// iPhone, it asks for motion access, which only works inside a tap.
    pub fn enable(&mut self, on: bool, w: &mut dyn TiltWindow, now: f64) {
        self.on = on;
        if !on {
            w.listen(false);
            self.timer = None;
            self.steer = 0.0;
            return self.set(TiltState::Off);
        }
        if self.state == TiltState::Live {
            return;
        }
        if !w.has_api() {
            return self.set(TiltState::None);
        }
        if w.needs_permission() && !self.granted {
            if !w.request_permission() {
                self.set(TiltState::Ask);
            }
            return;
        }
        self.listen(w, now);
    }

    /// `requestPermission()`'s promise settled.
    pub fn answer(&mut self, a: Answer, w: &mut dyn TiltWindow, now: f64) {
        match a {
            Answer::Granted => {
                self.granted = true;
                if self.on {
                    self.listen(w, now);
                }
            }
            Answer::Denied => self.set(TiltState::Denied),
            // Not inside a tap.
            Answer::Failed => {
                if self.on {
                    self.set(TiltState::Ask);
                }
            }
        }
    }

    fn listen(&mut self, w: &mut dyn TiltWindow, now: f64) {
        w.listen(true);
        // Plain-http pages get no sensor in most browsers; say so rather
        // than wait. If one answers anyway, `on_orientation` goes live.
        self.set(if w.insecure() {
            TiltState::Insecure
        } else {
            TiltState::Waiting
        });
        self.timer = Some(now + WAIT);
    }

    /// The JS's `setTimeout(…, 2000)`: no reading in time is no sensor.
    pub fn poll(&mut self, now: f64) {
        if let Some(t) = self.timer
            && now >= t
        {
            self.timer = None;
            if self.on && self.state == TiltState::Waiting {
                self.set(TiltState::None);
            }
        }
    }

    /// A `deviceorientation` event: beta and gamma (degrees, `None` for
    /// null) and the screen's angle when it came.
    pub fn on_orientation(&mut self, beta: Option<f64>, gamma: Option<f64>, angle: f64) {
        if !self.on {
            return;
        }
        let (Some(beta), Some(gamma)) = (beta, gamma) else {
            if self.state == TiltState::Waiting {
                self.set(TiltState::None);
            }
            return;
        };
        self.roll = screen_roll(beta, gamma, angle);
        self.set(TiltState::Live);
    }

    /// Smoothed steering for this tick.
    pub fn update(&mut self, dt: f64) -> f64 {
        let target = roll_to_steer(self.roll, self.full_lock);
        self.steer += (target - self.steer) * (1.0 - kernel::exp(-dt / SMOOTH));
        if (target - self.steer).abs() < 1e-3 {
            self.steer = target; // level is dead straight
        }
        self.steer
    }
}

/// The one `TiltSteer`, shared with the race's touch controls (the JS's
/// `Object.assign(touch, { tilt })`).
pub type SharedTilt = Arc<Mutex<TiltSteer>>;

/// The tilt state for the menu's note (`showTiltState`), as an index of
/// [`TiltState::ALL`].
static STATE: AtomicU8 = AtomicU8::new(0);

/// The tilt sensor's state now ('off' until tilt is chosen on a touch
/// screen).
pub fn state() -> TiltState {
    TiltState::ALL[STATE.load(Ordering::Relaxed) as usize % TiltState::ALL.len()]
}

/// [`state`] as a number that changes when it does (the menu rebuilds).
pub fn state_code() -> u8 {
    STATE.load(Ordering::Relaxed)
}

/// `TILT_NOTES[tilt.state]`: why tilt is not steering, when it is not.
pub fn note(s: TiltState) -> &'static str {
    match s {
        TiltState::Insecure => {
            "Tilt needs the game\u{2019}s https:// address; steering with the thumb stick"
        }
        TiltState::None => "No tilt sensor answered; steering with the thumb stick",
        TiltState::Ask => "Tap Race to allow motion access",
        TiltState::Denied => "Motion access was turned down; steering with the thumb stick",
        _ => "",
    }
}

/// The sensor and the settings the touch controls follow.
#[derive(Resource)]
pub struct Tilt {
    pub steer: SharedTilt,
    win: Box<dyn TiltWindow + Send + Sync>,
    /// The settings last applied (`None` before the first frame).
    seen: Option<Seen>,
    /// The race start last given `tilt.enable(true)`.
    start: u32,
    /// The settings the race's controls last took.
    touch: Option<TouchSettings>,
}

#[derive(Clone, Debug, PartialEq)]
struct Seen {
    steering: String,
    sens: f64,
}

/// The settings the touch controls read (`settings.steering`, `.pedals`,
/// `.autogas`, `.tiltSens`).
#[derive(Clone, Debug, PartialEq)]
pub struct TouchSettings {
    pub steering: String,
    pub pedals: String,
    pub autogas: bool,
    pub tilt_sens: f64,
}

impl Default for TouchSettings {
    fn default() -> Self {
        TouchSettings {
            steering: "stick".into(),
            pedals: "slider".into(),
            autogas: false,
            tilt_sens: 0.5,
        }
    }
}

pub fn plugin(app: &mut App) {
    #[cfg(target_arch = "wasm32")]
    let win: Box<dyn TiltWindow + Send + Sync> = Box::new(web::WebTilt);
    #[cfg(not(target_arch = "wasm32"))]
    let win: Box<dyn TiltWindow + Send + Sync> = Box::new(NoSensor);
    app.insert_resource(Tilt {
        steer: SharedTilt::default(),
        win,
        seen: None,
        start: 0,
        touch: None,
    })
    .add_systems(
        Update,
        sync.before(super::PlayFrame)
            .run_if(in_state(crate::loader::AppState::Running)),
    );
}

/// Each frame, before the race's: the sensor's events and answers, the
/// wait, the menu's choices (`opt-steer`'s and `opt-tilt-sens`'s handlers
/// and their start-up calls, `opt-autogas`, `opt-pedals`), `startRace`'s
/// `tilt.enable(true)`, and the race's touch controls set to them.
fn sync(
    time: Res<Time<Real>>,
    mut tilt: ResMut<Tilt>,
    mut play: ResMut<Play>,
    ui: Option<Res<crate::ui::UiState>>,
) {
    let now = time.elapsed_secs_f64();
    let s = ui.map_or_else(TouchSettings::default, |u| TouchSettings {
        steering: u.settings.steering.clone(),
        pedals: u.settings.pedals.clone(),
        autogas: u.settings.autogas,
        tilt_sens: u.settings.tilt_sens,
    });
    let tilt = &mut *tilt;
    let shared = tilt.steer.clone();
    let mut t = shared.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(target_arch = "wasm32")]
    {
        for a in web::answers() {
            t.answer(a, tilt.win.as_mut(), now);
        }
        for (beta, gamma, angle) in web::events() {
            t.on_orientation(beta, gamma, angle);
        }
    }
    t.poll(now);
    // `const tilt = touch ? new TiltSteer() : null`: only on a touch screen.
    if play.touch_ui {
        let want = Seen {
            steering: s.steering.clone(),
            sens: s.tilt_sens,
        };
        match &tilt.seen {
            None => {
                // At start: `if (settings.steering === 'tilt') tilt.enable(true)`.
                t.set_sensitivity(s.tilt_sens);
                if s.steering == "tilt" {
                    t.enable(true, tilt.win.as_mut(), now);
                }
            }
            Some(old) => {
                if old.sens != want.sens {
                    t.set_sensitivity(s.tilt_sens);
                }
                if old.steering != want.steering {
                    t.enable(s.steering == "tilt", tilt.win.as_mut(), now);
                }
            }
        }
        tilt.seen = Some(want);
    }
    let touch_ui = play.touch_ui;
    if let Some(race) = play.race.as_mut() {
        // `startRace`: `if (settings.steering === 'tilt') tilt?.enable(true)`.
        if touch_ui && race.starts != tilt.start {
            tilt.start = race.starts;
            if s.steering == "tilt" {
                t.enable(true, tilt.win.as_mut(), now);
            }
        }
        drop(t);
        // A new race's controls take the settings; after that, a setting
        // reaches them when it changes (the menus' `onchange`s), so a test
        // may set `autoGas` on the controls themselves, as on `__race`.
        let tc = &mut race.touch;
        let fresh = tc.tilt.is_none();
        if fresh {
            tc.tilt = Some(shared.clone());
        }
        let last = if fresh { None } else { tilt.touch.as_ref() };
        if last.is_none_or(|l| l.autogas != s.autogas) {
            tc.auto_gas = s.autogas;
        }
        if last.is_none_or(|l| l.pedals != s.pedals) {
            tc.set_pedals(super::touch::PedalKind::from_name(&s.pedals));
        }
        if last.is_none_or(|l| l.steering != s.steering) {
            tc.set_mode(super::touch::Steering::from_name(&s.steering));
        }
        #[cfg(target_arch = "wasm32")]
        if std::mem::take(&mut tc.buzz) {
            web::vibrate();
        }
    } else {
        drop(t);
    }
    tilt.touch = Some(s.clone());
    let st = shared.lock().unwrap_or_else(|e| e.into_inner()).state;
    let code = TiltState::ALL.iter().position(|x| *x == st).unwrap_or(0) as u8;
    STATE.store(code, Ordering::Relaxed);
    #[cfg(target_arch = "wasm32")]
    web::set_want_ask(touch_ui && s.steering == "tilt");
}

#[cfg(target_arch = "wasm32")]
pub mod web {
    //! The browser's side: `DeviceOrientationEvent` read through
    //! `js_sys::Reflect` (its `requestPermission` is Safari's only), the
    //! listener's events and the permission's answers queued for the next
    //! frame, and the request made inside the page's gesture handler.

    use super::{Answer, TiltWindow};
    use js_sys::{Function, Promise, Reflect};
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicBool, Ordering};
    use wasm_bindgen::prelude::*;
    use wasm_bindgen::{JsCast, JsValue};

    type Reading = (Option<f64>, Option<f64>, f64);
    type Listener = Closure<dyn FnMut(JsValue)>;

    thread_local! {
        static EVENTS: RefCell<Vec<Reading>> = const { RefCell::new(Vec::new()) };
        static ANSWERS: RefCell<Vec<Answer>> = const { RefCell::new(Vec::new()) };
        static HANDLER: RefCell<Option<Listener>> = const { RefCell::new(None) };
    }
    /// A `requestPermission()` is waiting for its answer: another one is
    /// not made meanwhile (the gesture's and the frame's would both ask).
    static PENDING: AtomicBool = AtomicBool::new(false);
    /// Motion access was granted (this page, this visit).
    static GRANTED: AtomicBool = AtomicBool::new(false);
    /// Tilt is chosen on a touch screen: a Race tap asks.
    static WANT_ASK: AtomicBool = AtomicBool::new(false);

    fn get(o: &JsValue, k: &str) -> JsValue {
        Reflect::get(o, &JsValue::from_str(k)).unwrap_or(JsValue::UNDEFINED)
    }

    fn doe() -> Option<JsValue> {
        let w = web_sys::window()?;
        let d = get(&w, "DeviceOrientationEvent");
        (!d.is_undefined() && !d.is_null()).then_some(d)
    }

    fn request() -> Option<Function> {
        doe().and_then(|d| get(&d, "requestPermission").dyn_into::<Function>().ok())
    }

    pub fn answers() -> Vec<Answer> {
        ANSWERS.with(|a| std::mem::take(&mut *a.borrow_mut()))
    }

    pub fn events() -> Vec<Reading> {
        EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
    }

    pub fn set_want_ask(on: bool) {
        WANT_ASK.store(on, Ordering::Relaxed);
    }

    /// `navigator.vibrate?.(8)`: the touch buttons' tick.
    pub fn vibrate() {
        if let Some(w) = web_sys::window() {
            let nav = w.navigator();
            if let Ok(f) = get(&nav, "vibrate").dyn_into::<Function>() {
                let _ = f.call1(&nav, &JsValue::from_f64(8.0));
            }
        }
    }

    /// `requestPermission()` and where its answer goes.
    fn ask() -> bool {
        if PENDING.load(Ordering::Relaxed) {
            return true;
        }
        let Some((d, f)) = doe().zip(request()) else {
            return false;
        };
        let Ok(asked) = f.call0(&d) else {
            return false;
        };
        PENDING.store(true, Ordering::Relaxed);
        let ok = Closure::<dyn FnMut(JsValue)>::new(move |r: JsValue| {
            PENDING.store(false, Ordering::Relaxed);
            let granted = r.as_string().as_deref() == Some("granted");
            GRANTED.fetch_or(granted, Ordering::Relaxed);
            let a = if granted {
                Answer::Granted
            } else {
                Answer::Denied
            };
            ANSWERS.with(|q| q.borrow_mut().push(a));
        });
        let err = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
            PENDING.store(false, Ordering::Relaxed);
            ANSWERS.with(|q| q.borrow_mut().push(Answer::Failed));
        });
        let _ = Promise::resolve(&asked).then2(&ok, &err);
        // Called once, kept: a request or two a visit.
        ok.forget();
        err.forget();
        true
    }

    /// Inside a tap that starts a race or picks Tilt (`startRace`'s and
    /// `opt-steer`'s `tilt.enable(true)`): an iPhone asks for motion access
    /// here, the one place it can.
    pub fn ask_in_gesture(picked_tilt: bool) {
        if !(WANT_ASK.load(Ordering::Relaxed) || picked_tilt) || GRANTED.load(Ordering::Relaxed) {
            return;
        }
        if request().is_some() {
            let _ = ask();
        }
    }

    /// The screen's angle as the JS reads it: `window.orientation` if it is
    /// a number, else `screen.orientation.angle`, else 0.
    fn angle() -> f64 {
        let Some(w) = web_sys::window() else {
            return 0.0;
        };
        if let Some(a) = get(&w, "orientation").as_f64() {
            return a;
        }
        let so = get(&get(&w, "screen"), "orientation");
        if so.is_undefined() || so.is_null() {
            return 0.0;
        }
        get(&so, "angle").as_f64().unwrap_or(0.0)
    }

    pub struct WebTilt;

    impl TiltWindow for WebTilt {
        fn has_api(&self) -> bool {
            doe().is_some()
        }
        fn needs_permission(&self) -> bool {
            request().is_some()
        }
        fn request_permission(&mut self) -> bool {
            ask()
        }
        fn listen(&mut self, on: bool) {
            let Some(w) = web_sys::window() else { return };
            HANDLER.with(|h| {
                let mut h = h.borrow_mut();
                let f = h.get_or_insert_with(|| {
                    Closure::new(|e: JsValue| {
                        let n = |k: &str| get(&e, k).as_f64();
                        let r = (n("beta"), n("gamma"), angle());
                        EVENTS.with(|q| q.borrow_mut().push(r));
                    })
                });
                let f: &Function = f.as_ref().unchecked_ref();
                let _ = w.remove_event_listener_with_callback("deviceorientation", f);
                if on {
                    let _ = w.add_event_listener_with_callback("deviceorientation", f);
                }
            });
        }
        fn insecure(&self) -> bool {
            web_sys::window()
                .map(|w| get(&w, "isSecureContext"))
                .is_some_and(|v| v.as_bool() == Some(false))
        }
    }
}

#[cfg(test)]
mod tests {
    //! `test/unit/tilt.test.js`.
    use super::*;
    use crate::play::input::{Input, TouchSource};

    fn near(a: f64, b: f64, eps: f64, msg: &str) {
        assert!((a - b).abs() <= eps, "{msg} {a} ≉ {b}");
    }

    /// `test/unit/support/pose.js`: a phone held the way a player holds
    /// it, as the beta and gamma deviceorientation would report (lying
    /// face up, turned so the picture is the right way up for `angle`, the
    /// wheel turned right by `turn`, then stood up to face the player,
    /// leaning `back` degrees from upright).
    fn pose(angle: f64, turn: f64, back: f64) -> (f64, f64) {
        let a = (angle - turn) * DEG;
        let t = (90.0 - back) * DEG;
        let ux = kernel::sin(t) * kernel::sin(a);
        let uy = kernel::sin(t) * kernel::cos(a);
        let uz = kernel::cos(t);
        let mut beta = kernel::asin(uy.clamp(-1.0, 1.0)) / DEG;
        let mut gamma = kernel::atan2(-ux, uz) / DEG;
        if uz < 0.0 {
            beta = 180.0 - beta;
            if beta >= 180.0 {
                beta -= 360.0;
            }
            gamma = kernel::atan2(ux, -uz) / DEG;
        }
        (beta, gamma)
    }

    fn expected_roll(turn: f64, back: f64) -> f64 {
        kernel::asin(kernel::sin(turn * DEG) * kernel::cos(back * DEG))
    }

    #[test]
    fn roll_held_still_and_level_reads_0() {
        for angle in [0.0, 90.0, -90.0, 270.0, 180.0] {
            near(screen_roll(0.0, 0.0, angle), 0.0, 1e-12, "flat");
        }
        near(screen_roll(90.0, 0.0, 0.0), 0.0, 1e-12, "portrait, upright");
        near(
            screen_roll(0.0, -90.0, 90.0),
            0.0,
            1e-12,
            "landscape, upright",
        );
        near(
            screen_roll(0.0, 90.0, -90.0),
            0.0,
            1e-12,
            "the other landscape",
        );
    }

    #[test]
    fn roll_right_hand_side_dipping_is_positive() {
        near(
            screen_roll(0.0, 20.0, 0.0),
            20.0 * DEG,
            1e-12,
            "portrait, right",
        );
        near(
            screen_roll(0.0, -20.0, 0.0),
            -20.0 * DEG,
            1e-12,
            "portrait, left",
        );
        near(
            screen_roll(10.0, -90.0, 90.0),
            10.0 * DEG,
            1e-12,
            "top end lifted",
        );
        near(
            screen_roll(-10.0, -90.0, 90.0),
            -10.0 * DEG,
            1e-12,
            "lowered",
        );
        near(
            screen_roll(10.0, 90.0, -90.0),
            -10.0 * DEG,
            1e-12,
            "other way",
        );
        near(screen_roll(10.0, 90.0, 270.0), -10.0 * DEG, 1e-12, "270");
    }

    #[test]
    fn roll_turning_like_a_wheel_steers_that_way_however_held() {
        for angle in [90.0, -90.0, 270.0, 0.0, 180.0] {
            for back in [0.0, 20.0, 45.0, 70.0] {
                for turn in [-35.0, -12.0, -3.0, 0.0, 3.0, 12.0, 35.0] {
                    let (b, g) = pose(angle, turn, back);
                    let roll = screen_roll(b, g, angle);
                    let what = format!("angle {angle} back {back} turn {turn}:");
                    near(roll, expected_roll(turn, back), 1e-9, &what);
                    if turn != 0.0 {
                        assert_eq!(js::sign(roll), js::sign(turn), "{what}");
                    }
                }
            }
        }
        // Lying back further only softens it: 20° of wheel at 45° back is 14°.
        let (b, g) = pose(90.0, 20.0, 45.0);
        near(screen_roll(b, g, 90.0) / DEG, 14.0, 0.05, "softened");
    }

    #[test]
    fn roll_to_steering_dead_zone_curve_full_lock_symmetric() {
        let full = full_lock_for(0.5);
        near(
            full / DEG,
            26.0,
            1e-9,
            "middle sensitivity: full lock at 26°",
        );
        near(full_lock_for(0.0) / DEG, 40.0, 1e-9, "");
        near(full_lock_for(1.0) / DEG, 12.0, 1e-9, "");
        assert_eq!(roll_to_steer(0.0, full), 0.0);
        assert_eq!(roll_to_steer(1.5 * DEG, full), 0.0, "inside the dead zone");
        assert_eq!(roll_to_steer(-1.5 * DEG, full), 0.0);
        assert_eq!(roll_to_steer(full, full), 1.0, "full lock");
        assert_eq!(roll_to_steer(60.0 * DEG, full), 1.0, "and no further");
        assert_eq!(roll_to_steer(-full, full), -1.0);
        let mut prev = 0.0;
        let mut d = 2.0;
        while d <= 26.0 {
            let s = roll_to_steer(d * DEG, full);
            assert!(s >= prev, "rises with the tilt ({d}°)");
            near(roll_to_steer(-d * DEG, full), -s, 1e-12, "symmetric");
            prev = s;
            d += 0.5;
        }
        assert!(
            roll_to_steer(8.0 * DEG, full) < (8.0 - 2.0) / (26.0 - 2.0),
            "softer than linear near the centre"
        );
        assert!(
            roll_to_steer(6.0 * DEG, full_lock_for(1.0))
                > roll_to_steer(6.0 * DEG, full_lock_for(0.0)),
            "sensitivity scales it"
        );
    }

    /// A window with just what TiltSteer uses (the JS test's `fakeWindow`).
    struct FakeWindow {
        secure: bool,
        api: bool,
        permission: bool,
        /// `requestPermission` throws synchronously.
        throws: bool,
        listeners: usize,
        asks: usize,
    }

    impl FakeWindow {
        fn new() -> FakeWindow {
            FakeWindow {
                secure: true,
                api: true,
                permission: false,
                throws: false,
                listeners: 0,
                asks: 0,
            }
        }
    }

    impl TiltWindow for FakeWindow {
        fn has_api(&self) -> bool {
            self.api
        }
        fn needs_permission(&self) -> bool {
            self.permission
        }
        fn request_permission(&mut self) -> bool {
            self.asks += 1;
            !self.throws
        }
        fn listen(&mut self, on: bool) {
            self.listeners = usize::from(on);
        }
        fn insecure(&self) -> bool {
            !self.secure
        }
    }

    fn tilt_to(t: &mut TiltSteer, angle: f64, turn: f64, back: f64) {
        let (b, g) = pose(angle, turn, back);
        t.on_orientation(Some(b), Some(g), angle);
    }

    #[test]
    fn the_sensor_waiting_live_on_first_reading_smoothed_off_again() {
        let mut w = FakeWindow::new();
        let mut steer = TiltSteer::new();
        assert!(!steer.live());
        steer.enable(true, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Waiting);
        assert_eq!(w.listeners, 1);
        steer.enable(true, &mut w, 0.0);
        assert_eq!(w.listeners, 1, "enabling twice listens once");

        tilt_to(&mut steer, 90.0, 30.0, 30.0);
        assert_eq!(steer.state, TiltState::Live);
        assert!(steer.live());
        assert!(steer.update(0.016) > 0.0 && steer.steer < 0.5, "eases in");
        for _ in 0..30 {
            steer.update(0.016);
        }
        near(
            steer.steer,
            roll_to_steer(expected_roll(30.0, 30.0), steer.full_lock),
            1e-4,
            "settles on the tilt",
        );
        tilt_to(&mut steer, 90.0, 0.0, 30.0);
        for _ in 0..30 {
            steer.update(0.016);
        }
        near(steer.steer, 0.0, 1e-4, "back to centre");

        steer.enable(false, &mut w, 1.0);
        assert_eq!(steer.state, TiltState::Off);
        assert!(!steer.live());
        assert_eq!(w.listeners, 0, "stops listening");
        assert_eq!(
            steer.changes,
            [TiltState::Waiting, TiltState::Live, TiltState::Off]
        );
    }

    #[test]
    fn the_sensor_none_then_one_an_http_page_the_screen_turning_round() {
        let mut w = FakeWindow::new();
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        steer.on_orientation(None, None, 90.0); // Chrome with no sensor
        assert_eq!(steer.state, TiltState::None);
        tilt_to(&mut steer, 90.0, 10.0, 30.0);
        assert_eq!(
            steer.state,
            TiltState::Live,
            "a sensor turning up later counts"
        );
        steer.enable(false, &mut w, 0.0);

        let mut w = FakeWindow::new();
        w.secure = false;
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Insecure);
        steer.on_orientation(None, None, 90.0);
        assert_eq!(
            steer.state,
            TiltState::Insecure,
            "the reason stays the useful one"
        );
        steer.poll(5.0);
        assert_eq!(
            steer.state,
            TiltState::Insecure,
            "and the wait does not change it"
        );
        tilt_to(&mut steer, 90.0, 10.0, 30.0);
        assert_eq!(
            steer.state,
            TiltState::Live,
            "a browser that sends it anyway"
        );

        // The same physical right turn, whichever way round the phone is.
        let mut w = FakeWindow::new();
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        tilt_to(&mut steer, -90.0, 15.0, 30.0);
        assert!(steer.roll > 0.0);
        tilt_to(&mut steer, 270.0, 15.0, 30.0);
        near(steer.roll, expected_roll(15.0, 30.0), 1e-9, "270");

        let mut bare = FakeWindow::new();
        bare.api = false;
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut bare, 0.0);
        assert_eq!(
            steer.state,
            TiltState::None,
            "no DeviceOrientationEvent at all"
        );
    }

    #[test]
    fn the_wait_no_reading_in_two_seconds_is_no_sensor() {
        let mut w = FakeWindow::new();
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 10.0);
        steer.poll(11.9);
        assert_eq!(steer.state, TiltState::Waiting);
        steer.poll(12.0);
        assert_eq!(steer.state, TiltState::None);
        // A reading before the wait is up keeps it live.
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        tilt_to(&mut steer, 90.0, 0.0, 30.0);
        steer.poll(3.0);
        assert_eq!(steer.state, TiltState::Live);
    }

    #[test]
    fn iphone_motion_access_asked_remembered_refusals_reported() {
        let mut w = FakeWindow::new();
        w.permission = true;
        let mut steer = TiltSteer::new();
        // At page load (no tap) it can't ask yet: the promise rejects.
        steer.enable(true, &mut w, 0.0);
        steer.answer(Answer::Failed, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Ask);
        assert_eq!(w.listeners, 0);
        // A browser that throws instead.
        w.throws = true;
        steer.enable(true, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Ask);
        w.throws = false;

        // The tap: asked, granted, listening.
        steer.enable(true, &mut w, 0.0);
        steer.answer(Answer::Granted, &mut w, 0.0);
        assert_eq!(w.asks, 3);
        assert_eq!(steer.state, TiltState::Waiting);
        assert_eq!(w.listeners, 1);
        tilt_to(&mut steer, 90.0, -10.0, 30.0);
        assert!(steer.live());
        assert!(steer.roll < 0.0);

        // Off and on again doesn't ask twice.
        steer.enable(false, &mut w, 0.0);
        steer.enable(true, &mut w, 0.0);
        assert_eq!(w.asks, 3);
        assert_eq!(w.listeners, 1);
        steer.enable(false, &mut w, 0.0);

        // Turned down.
        let mut w = FakeWindow::new();
        w.permission = true;
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        steer.answer(Answer::Denied, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Denied);
        assert_eq!(w.listeners, 0);

        // Switched off while the question is up: nothing starts listening.
        let mut w = FakeWindow::new();
        w.permission = true;
        let mut steer = TiltSteer::new();
        steer.enable(true, &mut w, 0.0);
        steer.enable(false, &mut w, 0.0);
        steer.answer(Answer::Granted, &mut w, 0.0);
        assert_eq!(steer.state, TiltState::Off);
        assert_eq!(w.listeners, 0);
    }

    struct TiltTouch {
        steer: f64,
        tilt: Option<f64>,
    }

    impl TouchSource for TiltTouch {
        fn throttle(&self) -> f64 {
            0.0
        }
        fn brake(&self) -> f64 {
            0.0
        }
        fn steer(&self) -> f64 {
            self.steer
        }
        fn handbrake(&self) -> bool {
            false
        }
        fn nitro(&self) -> bool {
            false
        }
        fn analog_steer(&mut self, _dt: f64) -> Option<f64> {
            self.tilt
        }
    }

    #[test]
    fn input_tilt_goes_straight_through_and_a_held_key_wins() {
        let mut input = Input::new();
        let mut touch = TiltTouch {
            steer: 0.0,
            tilt: Some(0.4),
        };
        near(
            input.update(0.016, Some(&mut touch)).steer,
            0.4,
            1e-12,
            "no ramp",
        );
        touch.tilt = Some(-0.7);
        near(input.update(0.016, Some(&mut touch)).steer, -0.7, 1e-12, "");
        input.key_down("KeyD", false);
        let s = input.update(0.1, Some(&mut touch)).steer;
        near(
            s,
            -0.7 + 0.9,
            1e-9,
            "D counter-steers from the tilt at the key rate",
        );
        input.key_up("KeyD");
        near(
            input.update(0.016, Some(&mut touch)).steer,
            -0.7,
            1e-12,
            "back on tilt",
        );
        // Steering on the ◂ ▸ pads (no analogue reading): ramped as before.
        touch.tilt = None;
        touch.steer = 1.0;
        input.steer = 0.0;
        near(
            input.update(0.1, Some(&mut touch)).steer,
            0.36,
            1e-9,
            "ramped",
        );
    }
}
