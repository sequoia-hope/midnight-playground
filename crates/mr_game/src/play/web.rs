//! The race's web glue: whether this is a touch device (the page decides,
//! as `isTouchDevice` does, into `__mr.touch`), the safe-area insets the
//! page measures (`__mr.insets`), a hidden page pausing the race
//! (`visibilitychange`), the JS's pixel ratio (`applyQuality`), and the
//! race's state on `window.__mr.race` for the tests.

use super::Play;
use super::flow::Mode;
use super::touch::{Hold, Insets};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use js_sys::{Object, Reflect};
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::{JsCast, JsValue};

/// The page went hidden since the last frame (`visibilitychange`). A hidden
/// page gets no frames, so the event is latched and the race pauses in the
/// first frame back, before its ticks.
static WENT_HIDDEN: AtomicBool = AtomicBool::new(false);

fn mr() -> Option<Object> {
    let w = web_sys::window()?;
    Reflect::get(&w, &JsValue::from_str("__mr"))
        .ok()?
        .dyn_into::<Object>()
        .ok()
}

fn get(o: &JsValue, k: &str) -> Option<JsValue> {
    Reflect::get(o, &JsValue::from_str(k))
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null())
}

fn num(o: &JsValue, k: &str) -> f64 {
    get(o, k).and_then(|v| v.as_f64()).unwrap_or(0.0)
}

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
}

/// At start: the touch controls on a touch device (unless `?touch=` said),
/// and the JS's pixel ratio: `hq ? min(devicePixelRatio, 1.5) : 1`.
fn setup(
    mut play: ResMut<Play>,
    opts: Res<crate::Opts>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if let Some(mr) = mr()
        && play.params.touch.is_none()
    {
        play.touch_ui = get(mr.as_ref(), "touch")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
    // `document.addEventListener('visibilitychange', …)`.
    if let Some(d) = web_sys::window().and_then(|w| w.document()) {
        let f = Closure::<dyn FnMut()>::new(|| {
            let hidden = web_sys::window()
                .and_then(|w| w.document())
                .is_some_and(|d| d.hidden());
            if hidden {
                WENT_HIDDEN.store(true, Ordering::Relaxed);
            }
        });
        let _ = d.add_event_listener_with_callback("visibilitychange", f.as_ref().unchecked_ref());
        f.forget();
    }
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    let pr = if opts.hq { dpr.min(1.5) } else { 1.0 };
    if let Ok(mut w) = windows.single_mut()
        && (pr - dpr).abs() > 1e-3
    {
        w.resolution.set_scale_factor_override(Some(pr as f32));
    }
}

/// Leaving the page (switching apps, locking the phone) pauses the race
/// (`if (document.hidden && mode === 'race') pause(true)`), before the
/// frame's ticks.
fn visibility(mut play: ResMut<Play>) {
    let went = WENT_HIDDEN.swap(false, Ordering::Relaxed);
    let hidden = went
        || web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.hidden());
    if let Some(race) = play.race.as_mut()
        && hidden
        && race.mode == Mode::Race
    {
        race.pause(true);
    }
}

/// Each frame: the insets, the CSS scale, the hidden page, `__mr.race`.
fn frame(
    mut play: ResMut<Play>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
) {
    let Some(mr) = mr() else { return };
    if let Some(ins) = get(mr.as_ref(), "insets") {
        let i = Insets {
            top: num(&ins, "top"),
            right: num(&ins, "right"),
            bottom: num(&ins, "bottom"),
            left: num(&ins, "left"),
        };
        if i != play.insets {
            play.insets = i;
        }
    }
    // CSS px per logical px, measured: the canvas's CSS width over the
    // window's logical width (they differ under the pixel-ratio override).
    let css_w = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("game"))
        .map_or(0, |c| c.client_width());
    if let Ok(w) = windows.single()
        && css_w > 0
        && w.width() > 0.0
    {
        let k = css_w as f32 / w.width();
        if (k - play.css_scale).abs() > 1e-3 {
            play.css_scale = k;
        }
        let t = w.resolution.scale_factor() / w.resolution.base_scale_factor().max(0.01);
        if (t - play.touch_scale).abs() > 1e-4 {
            play.touch_scale = t;
        }
    }
    let hidden = web_sys::window()
        .and_then(|w| w.document())
        .is_some_and(|d| d.hidden());
    let touch_ui = play.touch_ui;
    let Some(race) = play.race.as_mut() else {
        return;
    };
    // Leaving the page (switching apps, locking the phone) pauses the race.
    if hidden && race.mode == Mode::Race {
        race.pause(true);
    }
    let st = &race.session.curr;
    let p = &st.players[0];
    let s = race.input.state;
    let o = Object::new();
    set(&o, "state", format!("{:?}", st.race.state).to_lowercase());
    set(
        &o,
        "mode",
        match race.mode {
            Mode::Race => "race",
            Mode::Paused => "paused",
            Mode::Results => "results",
        },
    );
    set(&o, "tick", st.tick);
    set(&o, "time", st.race.time);
    set(&o, "countdown", st.race.countdown);
    set(&o, "s", p.v.s);
    set(&o, "lat", p.v.lat);
    set(&o, "speed", mr_math::kernel::hypot(p.v.vx, p.v.vz));
    set(&o, "gear", p.phys.gear);
    set(&o, "finished", p.rules.finished);
    set(&o, "locked", p.phys.locked);
    let inp = Object::new();
    set(&inp, "steer", s.steer);
    set(&inp, "throttle", s.throttle);
    set(&inp, "brake", s.brake);
    set(&inp, "handbrake", s.handbrake);
    set(&inp, "nitro", s.nitro);
    set(&inp, "analog", s.analog);
    set(&o, "input", inp);
    // What the controls suites read off `__race`: the car, the camera's
    // right (`__camera.matrixWorld`'s x axis) and `input.touch`.
    set(&o, "yaw", p.v.yaw);
    set(&o, "steerAngle", p.v.steer_angle);
    // The road there: its half width, and the heading off it (`yawToRoad`).
    let f = race.session.lr.track.frame(p.v.s);
    let ang = mr_math::kernel::atan2(f.fz, f.fx) - p.v.yaw;
    set(&o, "hw", f.hw);
    set(
        &o,
        "yawToRoad",
        mr_math::kernel::atan2(mr_math::kernel::sin(ang), mr_math::kernel::cos(ang)),
    );
    set(&o, "vx", p.v.vx);
    set(&o, "vz", p.v.vz);
    set(&o, "nitro", p.phys.nitro);
    set(&o, "nitroActive", p.phys.nitro_active);
    set(&o, "camMode", race.rig.mode as f64);
    if let Some(c) = cams.iter().next() {
        let r = c.right();
        let cr = Object::new();
        set(&cr, "x", r.x);
        set(&cr, "z", r.z);
        set(&o, "camRight", cr);
    }
    let t = &race.touch;
    let to = Object::new();
    set(&to, "visible", t.visible);
    set(&to, "mode", t.mode.name());
    set(&to, "steering", t.steering.map_or("", |s| s.name()));
    set(&to, "pedals", t.pedals.name());
    set(&to, "autoGas", t.auto_gas);
    set(&to, "stickR", t.layout.stick_r);
    let held = Object::new();
    for h in [
        Hold::Throttle,
        Hold::Brake,
        Hold::Left,
        Hold::Right,
        Hold::Handbrake,
        Hold::Nitro,
    ] {
        set(&held, h.name(), t.held.get(h));
    }
    set(&to, "held", held);
    if let Some(st) = t.stick {
        let so = Object::new();
        set(&so, "id", st.id as f64);
        set(&so, "x0", st.x0);
        set(&so, "y0", st.y0);
        set(&so, "x", st.x);
        set(&to, "stick", so);
    } else {
        set(&to, "stick", JsValue::NULL);
    }
    set(
        &to,
        "lock",
        t.stick_offset().abs() >= t.layout.stick_r - 0.5,
    );
    set(&to, "knob", t.stick_offset());
    let panel = js_sys::Array::new();
    for c in t.panel_classes() {
        panel.push(&JsValue::from_str(c));
    }
    set(&to, "panel", panel);
    set(&to, "wheel", t.wheel);
    if let Some(tl) = &t.tilt {
        let tl = tl.lock().unwrap_or_else(|e| e.into_inner());
        let tt = Object::new();
        set(&tt, "state", tl.state.name());
        set(&tt, "live", tl.live());
        set(&tt, "roll", tl.roll);
        set(&tt, "steer", tl.steer);
        set(&tt, "fullLock", tl.full_lock);
        set(&to, "tilt", tt);
    }
    set(&o, "touch", to);
    set(&o, "touchUi", touch_ui);
    if let Some(rows) = &race.results {
        let arr = js_sys::Array::new();
        for r in rows {
            let row = Object::new();
            set(&row, "place", r.place as f64);
            set(&row, "name", r.name);
            set(&row, "player", r.player);
            set(&row, "time", r.time);
            set(&row, "estimated", r.estimated);
            arr.push(&row);
        }
        set(&o, "results", arr);
    }
    set(&mr, "race", o);
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(Update, visibility.before(super::PlayFrame))
        .add_systems(Last, frame);
}
