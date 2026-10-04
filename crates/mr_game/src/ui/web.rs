//! The screens' web glue: the page's touch decision, another level's
//! scene through the page (`__mr.reload`), the pixel ratio when High
//! quality changes, fullscreen and the landscape lock inside the Race tap
//! (SPEC 8.4), and the test bridge's part for the screens (SPEC 8.5):
//! `__mr.screen`, `__mr.mode`, `__mr.ui(id)`, `__mr.reveal(id)` and a few
//! `__mr.stage(cmd)` commands. WP 6.7 owns the full bridge.

use super::{Screen, UiState, mode_name, snapshot};
use crate::play::Play;
use crate::status::Status;
use bevy::prelude::*;
use js_sys::{Object, Reflect};
use std::sync::Mutex;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

static START_LEVEL: Mutex<String> = Mutex::new(String::new());
static REVEAL: Mutex<Option<String>> = Mutex::new(None);
static STAGE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The controls that go fullscreen when tapped on a phone (`startRace`
/// calls `enterFullscreen` inside the tap), in CSS px, and whether the
/// setting allows it.
type Rects = Vec<(f32, f32, f32, f32)>;
static FULLSCREEN: Mutex<(bool, Rects)> = Mutex::new((false, Vec::new()));

fn mr() -> Option<Object> {
    let w = web_sys::window()?;
    Reflect::get(&w, &JsValue::from_str("__mr"))
        .ok()?
        .dyn_into::<Object>()
        .ok()
}

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
}

/// `isTouchDevice` as the page decided it (`__mr.touch`).
pub fn page_touch() -> bool {
    mr().and_then(|m| Reflect::get(&m, &JsValue::from_str("touch")).ok())
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// The level this run builds first (the page downloads its scene).
pub(crate) fn set_start_level(level: &str) {
    *START_LEVEL.lock().unwrap_or_else(|e| e.into_inner()) = level.to_owned();
}

#[wasm_bindgen]
pub fn start_level() -> String {
    START_LEVEL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// `__mr.reload(level)`: the page tears the scene down and loads the other
/// level behind its loading screen.
pub fn reload(level: &str) {
    let f = js_sys::Function::new_with_args(
        "lv",
        "const r = window.__mr && window.__mr.reload; if (r) r(lv).catch((e) => console.error('loading ' + lv + ': ' + (e && e.message || e)));",
    );
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(level));
}

/// `applyQuality`'s pixel ratio: `hq ? min(devicePixelRatio, 1.5) : 1`.
pub fn apply_pixel_ratio(w: &mut Window, hq: bool) {
    w.resolution
        .set_scale_factor_override(Some(pixel_ratio(hq, false)));
}

/// The pixel ratio the canvas draws at: the JS's for the race; on the
/// menu and the loading screen, at least `min(devicePixelRatio, 2)`, because the screens' text
/// is drawn into the same canvas, where the DOM's was always sharp
/// (DECISIONS D575).
fn pixel_ratio(hq: bool, screen: bool) -> f32 {
    let dpr = web_sys::window().map_or(1.0, |w| w.device_pixel_ratio());
    let race = if hq { dpr.min(1.5) } else { 1.0 };
    (if screen { race.max(dpr.min(2.0)) } else { race }) as f32
}

/// Sharp text on the screens, the JS's pixel ratio while driving.
fn screen_pixel_ratio(
    ui: Res<UiState>,
    play: Res<Play>,
    opts: Res<crate::Opts>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    // Only with no race: a race's HUD text, laid out at the race's ratio,
    // is not laid out again when the ratio changes under it (pause).
    let want = pixel_ratio(opts.hq, ui.screen != Screen::None && play.race.is_none());
    if let Ok(mut w) = windows.single_mut()
        && (w.resolution.scale_factor() - want).abs() > 1e-3
    {
        w.resolution.set_scale_factor_override(Some(want));
    }
}

/// The menu's "Music player" link (the JS page until M5's player).
pub fn open_music_player() {
    if let Some(w) = web_sys::window() {
        let _ = w.location().set_href("../../music.html");
    }
}

/// `__mr.reveal(id)`: scroll a control to the middle of its screen.
#[wasm_bindgen]
pub fn ui_reveal(id: String) {
    *REVEAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
}

/// `__mr.stage(json)`: the test staging the flow suites use: `{"cmd":
/// "finish", "rivalWon": bool}` puts the player 250 m from the line at 45
/// m/s and the rivals back near the start (one of them already finished
/// if asked); `{"cmd": "cruise", "score": n, "nearMisses": n}` sets the
/// cruise score; `{"cmd": "padsetup"}` opens the Controller screen.
#[wasm_bindgen]
pub fn stage(cmd: String) {
    STAGE.lock().unwrap_or_else(|e| e.into_inner()).push(cmd);
}

/// The page's gesture handlers call this inside the event (SPEC 8.4): a
/// tap or click on Race, Race again or Restart on a touch screen goes
/// fullscreen and asks for landscape, as `enterFullscreen` does inside
/// `startRace`.
#[wasm_bindgen]
pub fn gesture_at(_kind: &str, x: f64, y: f64) {
    let (on, rects) = FULLSCREEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if !on {
        return;
    }
    let (x, y) = (x as f32, y as f32);
    if rects
        .iter()
        .any(|r| x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3)
    {
        let f = js_sys::Function::new_no_args(
            "if (document.fullscreenElement) return; const el = document.documentElement; \
             const req = el.requestFullscreen || el.webkitRequestFullscreen; if (!req) return; \
             try { Promise.resolve(req.call(el, { navigationUI: 'hide' })) \
               .then(() => screen.orientation && screen.orientation.lock && screen.orientation.lock('landscape')) \
               .catch(() => {}); } catch (e) { /* not allowed here */ }",
        );
        let _ = f.call0(&JsValue::NULL);
    }
}

/// Staging commands and reveals from the page.
fn take_commands(mut ui: ResMut<UiState>, mut play: ResMut<Play>) {
    if let Some(id) = REVEAL.lock().unwrap_or_else(|e| e.into_inner()).take() {
        ui.reveal = Some(id);
    }
    let cmds: Vec<String> = std::mem::take(&mut *STAGE.lock().unwrap_or_else(|e| e.into_inner()));
    for c in cmds {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&c) else {
            continue;
        };
        match v["cmd"].as_str() {
            Some("padsetup") => {
                if ui.screen == Screen::Menu || ui.screen == Screen::Pause {
                    ui.pad_return = ui.screen;
                    ui.screen = Screen::PadSetup;
                    ui.dirty = true;
                }
            }
            Some("finish") => {
                let won = v["rivalWon"].as_bool().unwrap_or(false);
                if let Some(r) = play.race.as_mut() {
                    let t = r.session.lr.track.clone();
                    let st = &mut r.session.curr;
                    let s = t.finish_s - 250.0;
                    let p = &mut st.players[0];
                    p.phys.reset(&mut p.v, &t, s, 0.0);
                    let f = t.frame(s);
                    p.v.vx = f.fx * 45.0;
                    p.v.vz = f.fz * 45.0;
                    p.rules.last_s = Some(s);
                    p.rules.odo = Some(s);
                    for (i, a) in st.rivals.iter_mut().enumerate() {
                        a.k.s = t.start_s + 40.0 - i as f64 * 12.0;
                        a.k.lat = if i % 2 == 1 { 2.0 } else { -2.0 };
                        a.k.speed = 0.0;
                        a.write_pos(&t);
                    }
                    if won && let Some(a) = st.rivals.first_mut() {
                        a.finished = true;
                        a.finish_time = Some(1.0);
                    }
                    r.session.prev = r.session.curr.clone();
                }
            }
            Some("cruise") => {
                if let Some(r) = play.race.as_mut() {
                    let rules = &mut r.session.curr.players[0].rules;
                    if let Some(s) = v["score"].as_f64() {
                        rules.score = s;
                    }
                    if let Some(n) = v["nearMisses"].as_i64() {
                        rules.near_misses = n as i32;
                    }
                }
            }
            _ => warn!("__mr.stage: unknown command {c}"),
        }
    }
}

/// Publishes the screens' state on `window.__mr`.
fn publish(
    mut ui: ResMut<UiState>,
    play: Res<Play>,
    status: Res<Status>,
    controls: super::ControlQuery,
    mut last: Local<String>,
) {
    let Some(mr) = mr() else { return };
    set(&mr, "screen", ui.screen.name());
    set(&mr, "mode", mode_name(&ui, &play, &status));
    set(&mr, "races", ui.races);
    set(&mr, "touchUi", play.touch_ui);
    set(
        &mr,
        "focus",
        ui.focus.as_deref().map_or(JsValue::NULL, JsValue::from_str),
    );
    if play.race.is_none() {
        let _ = Reflect::delete_property(&mr, &JsValue::from_str("race"));
    }
    // The safe-area inset at the top (the rotate hint keeps under it).
    if let Ok(ins) = Reflect::get(&mr, &JsValue::from_str("insets"))
        && let Some(top) = Reflect::get(&ins, &JsValue::from_str("top"))
            .ok()
            .and_then(|v| v.as_f64())
        && (top as f32 - ui.inset_top).abs() > 0.5
    {
        ui.inset_top = top as f32;
        ui.dirty = true;
    }
    let css = play.css_scale.max(0.01);
    let mut nodes = serde_json::Map::new();
    let mut full = Vec::new();
    for (id, r, vis, enabled, value, sel, z) in snapshot(&controls, css) {
        if matches!(id.as_str(), "btn-start" | "btn-again" | "btn-restart") && vis {
            full.push(r);
        }
        let value = match value {
            super::Value::None => serde_json::Value::Null,
            super::Value::Bool(b) => serde_json::Value::Bool(b),
            super::Value::Num(n) => super::store::num_value(n),
            super::Value::Text(t) => serde_json::Value::String(t),
        };
        nodes.insert(
            id,
            serde_json::json!({
                "x": r.0, "y": r.1, "w": r.2, "h": r.3,
                "visible": vis, "enabled": enabled, "value": value, "sel": sel, "z": z,
            }),
        );
    }
    // The touch controls' taps (`#touch [data-tap=…]`), from their layout.
    if let Some(race) = &play.race
        && play.touch_ui
    {
        let shown = race.touch.visible;
        for (i, name) in crate::play::touch::TAPS.iter().enumerate() {
            let t = race.touch.layout.taps[i];
            nodes.insert(
                format!("touch-{name}"),
                serde_json::json!({
                    "x": t.left, "y": t.top, "w": t.width(), "h": t.height(),
                    "visible": shown, "enabled": true, "value": null, "sel": false, "z": 0,
                }),
            );
        }
    }
    *FULLSCREEN.lock().unwrap_or_else(|e| e.into_inner()) =
        (play.touch_ui && ui.settings.fullscreen, full);
    let json = serde_json::Value::Object(nodes).to_string();
    if *last != json {
        if let Ok(v) = js_sys::JSON::parse(&json) {
            set(&mr, "uiNodes", v);
        }
        *last = json;
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, take_commands.before(super::pointer))
        .add_systems(Update, screen_pixel_ratio.after(super::build))
        .add_systems(Last, publish);
}
