//! The race's web glue: whether this is a touch device (the page decides,
//! as `isTouchDevice` does, into `__mr.touch`), the safe-area insets the
//! page measures (`__mr.insets`), a hidden page pausing the race
//! (`visibilitychange`), the JS's pixel ratio (`applyQuality`), and the
//! race's state on `window.__mr.race` for the tests.

use super::Play;
use super::flow::Mode;
use super::touch::Insets;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use js_sys::{Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};

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
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    let pr = if opts.hq { dpr.min(1.5) } else { 1.0 };
    if let Ok(mut w) = windows.single_mut()
        && (pr - dpr).abs() > 1e-3
    {
        w.resolution.set_scale_factor_override(Some(pr as f32));
    }
}

/// Each frame: the insets, the CSS scale, the hidden page, `__mr.race`.
fn frame(mut play: ResMut<Play>, windows: Query<&Window, With<PrimaryWindow>>) {
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
    set(&o, "touch", touch_ui);
    let inp = Object::new();
    set(&inp, "steer", s.steer);
    set(&inp, "throttle", s.throttle);
    set(&inp, "brake", s.brake);
    set(&inp, "handbrake", s.handbrake);
    set(&inp, "nitro", s.nitro);
    set(&inp, "analog", s.analog);
    set(&o, "input", inp);
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
    app.add_systems(Startup, setup).add_systems(Last, frame);
}
