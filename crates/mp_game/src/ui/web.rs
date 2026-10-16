//! The screens' web glue: the page's touch decision, another level's
//! scene through the page (`__mp.reload`), the pixel ratio (the screens'
//! sharper than the race's, over the scene at the race's, D1199: Playground
//! Kit's `pk_shell::web`, D1234), fullscreen and the landscape lock inside
//! the Race tap (SPEC 8.4), and the test bridge's part for the screens
//! (SPEC 8.5): `__mp.screen`, `__mp.mode`, `__mp.ui(id)`, `__mp.reveal(id)`
//! and the `__mp.stage(cmd)` commands. WP 6.7 owns the full bridge.

use super::{Screen, UiState, mode_name, snapshot};
use crate::play::Play;
use crate::status::Status;
use bevy::prelude::*;
use pk_shell::bridge;
use pk_shell::web::{FullscreenOnTap, PermissionOnTap, PixelRatioSet, StageSet};
use std::sync::Mutex;
use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::*;

static START_LEVEL: Mutex<String> = Mutex::new(String::new());
static REVEAL: Mutex<Option<String>> = Mutex::new(None);

pub use pk_shell::web::{copy_text, page_touch, prompt};

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

/// `__mp.reload(level)`: the page tears the scene down and loads the other
/// level behind its loading screen.
pub fn reload(level: &str) {
    let f = js_sys::Function::new_with_args(
        "lv",
        "const r = window.__game && window.__game.reload; if (r) r(lv).catch((e) => console.error('loading ' + lv + ': ' + (e && e.message || e)));",
    );
    let _ = f.call1(&JsValue::NULL, &JsValue::from_str(level));
}

/// The menu's "Music player" link: the JS game's page. Beside the build at
/// `dist/next/` (the registered server) it is two levels up; on GitHub
/// Pages, where the build is the site root, the JS game is under `legacy/`
/// (D1112).
pub fn open_music_player() {
    if let Some(w) = web_sys::window() {
        let path = w.location().pathname().unwrap_or_default();
        let _ = w.location().set_href(music_player_href(&path));
    }
}

fn music_player_href(path: &str) -> &'static str {
    if path.contains("/dist/next/") {
        "../../music.html"
    } else {
        "legacy/music.html"
    }
}

/// `__mp.reveal(id)`: scroll a control to the middle of its screen.
#[wasm_bindgen]
pub fn ui_reveal(id: String) {
    *REVEAL.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
}

/// `__mp.stage(json)`: the test staging the flow suites use: `{"cmd":
/// "finish", "rivalWon": bool}` puts the player 250 m from the line at 45
/// m/s and the rivals back near the start (one of them already finished
/// if asked); `{"cmd": "cruise", "score": n, "nearMisses": n}` sets the
/// cruise score; `{"cmd": "padsetup"}` opens the Controller screen. The
/// queue and the dispatch are `pk_shell::web`'s; the handlers are
/// registered in [`plugin`].
#[wasm_bindgen]
pub fn stage(cmd: String) {
    pk_shell::web::stage(cmd);
}

/// The page's gesture handlers call this inside the event (SPEC 8.4): a
/// tap or click on Race, Race again or Restart on a touch screen goes
/// fullscreen and asks for landscape, as `enterFullscreen` does inside
/// `startRace`; with tilt chosen, it asks an iPhone for motion access
/// (`startRace`'s `tilt.enable(true)`), as a tap on the steering choice's
/// Tilt does (`opt-steer`'s change). The motion request comes first: a
/// fullscreen request uses the tap up, and a browser with both would then
/// refuse the other (DECISIONS D843). `pk_shell::web::gesture_at` does it
/// from the controls marked `FullscreenOnTap` and `PermissionOnTap`.
#[wasm_bindgen]
pub fn gesture_at(kind: &str, x: f64, y: f64) {
    pk_shell::web::gesture_at(kind, x, y);
}

/// Marks the controls whose tap goes fullscreen or asks the motion
/// permission (`pk_shell::web`), as they are built.
fn mark_taps(
    mut commands: Commands,
    added: Query<(Entity, &super::Control), Added<super::Control>>,
) {
    for (e, c) in &added {
        match c.id.as_str() {
            "btn-start" | "btn-again" | "btn-restart" => {
                commands.entity(e).insert(FullscreenOnTap);
            }
            "option-tilt" => {
                commands.entity(e).insert(PermissionOnTap);
            }
            _ => {}
        }
    }
}

/// Whether a screen is up this frame, for the shell's pixel ratio (D1199).
fn screen_state(ui: Res<UiState>, mut policy: ResMut<pk_shell::web::PixelRatioPolicy>) {
    let up = ui.screen != Screen::None;
    if policy.screen_up != up {
        policy.screen_up = up;
    }
}

/// The test bridge's control commands (WP 6.7): `{"cmd": "choose", "id":
/// "opt-track", "value": v}` (a select's `value = v` and its `change`),
/// `{"cmd": "slide", "id": "vol-music", "value": 0..100}` (a range's value
/// and its `input`), `{"cmd": "focus", "id"}` (`el.focus()`), `{"cmd":
/// "act", "id"}` (`el.click()`), `{"cmd": "audio", "op": "suspend"}`.
/// Returns false for a command that is not one of these.
fn stage_control(
    ui: &mut UiState,
    ctx: &mut super::ActCtx,
    controls: &super::ControlQuery,
    v: &serde_json::Value,
) -> bool {
    use super::{Act, Sel, Sl};
    let id = v["id"].as_str().unwrap_or("");
    match v["cmd"].as_str() {
        Some("choose") => {
            let sel = match id {
                "opt-track" => Sel::Track,
                "opt-steer" => Sel::Steer,
                "opt-pedals" => Sel::Pedals,
                "opt-guide" => Sel::Guide,
                "opt-assist" => Sel::Assist,
                "opt-station" => Sel::Station,
                "opt-scale" => Sel::Scale,
                "opt-physics" => Sel::Physics,
                _ => return true,
            };
            let value = v["value"].as_str().unwrap_or("").to_owned();
            super::activate(ui, ctx, controls, Act::Choose(sel, value));
        }
        Some("slide") => {
            let (which, key) = match id {
                "vol-music" => (Sl::Music, "musicVol"),
                "vol-sfx" => (Sl::Sfx, "sfxVol"),
                "opt-tilt-sens" => (Sl::TiltSens, "tiltSens"),
                _ => return true,
            };
            // `el.value` is a whole number 0..100; the setting its hundredth.
            let x = (v["value"].as_f64().unwrap_or(0.0).clamp(0.0, 100.0)).round() / 100.0;
            let s = &mut ui.settings;
            let slot = match which {
                Sl::Music => &mut s.music,
                Sl::Sfx => &mut s.sfx,
                Sl::TiltSens => &mut s.tilt_sens,
            };
            *slot = x;
            ctx.store.set_num(key, x);
            ui.dirty = true;
        }
        Some("focus") => {
            ui.focus = Some(id.to_owned());
            ui.dirty = true;
        }
        Some("act") => {
            let act = controls
                .iter()
                .find(|(_, c, ..)| c.id == id)
                .and_then(|(_, c, ..)| c.act.clone());
            if let Some(act) = act {
                super::activate(ui, ctx, controls, act);
            }
        }
        Some("audio") => crate::play::audio::stage_ctx(v["op"].as_str().unwrap_or("")),
        _ => return false,
    }
    true
}

/// Every stage command the suites send: the controls' (`stage_control`),
/// the race's (`play::web::stage_race`), and the flow's.
fn apply_stage(
    In(v): In<serde_json::Value>,
    mut ui: ResMut<UiState>,
    mut ctx: super::ActCtx,
    controls: super::ControlQuery,
) {
    if stage_control(&mut ui, &mut ctx, &controls, &v)
        || crate::play::web::stage_race(&mut ctx.play, &v)
    {
        return;
    }
    let play = &mut ctx.play;
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
        // The controls suites' `placeCar` (and `phys.reset`): on the
        // road `ahead` m past the start (or `fromFinish` m before the
        // line) at `lat` (or `latFrac` of the half width), pointing
        // along it at `speed` m/s, its heading turned by `yaw` radians.
        Some("place") => {
            if let Some(r) = play.race.as_mut() {
                let t = r.session.lr.track.clone();
                let st = &mut r.session.curr;
                let p = &mut st.players[0];
                let s = match v["fromFinish"].as_f64() {
                    Some(d) => t.finish_s - d,
                    None => t.start_s + v["ahead"].as_f64().unwrap_or(40.0),
                };
                let f = t.frame(s);
                let lat = v["latFrac"]
                    .as_f64()
                    .map_or(v["lat"].as_f64().unwrap_or(0.0), |k| k * f.hw);
                p.phys.reset(&mut p.v, &t, s, lat);
                let speed = v["speed"].as_f64().unwrap_or(0.0);
                p.v.vx = f.fx * speed;
                p.v.vz = f.fz * speed;
                p.v.yaw += v["yaw"].as_f64().unwrap_or(0.0);
                p.rules.last_s = Some(s);
                r.session.prev = r.session.curr.clone();
                // `window.__race.cam.snap = true`: the camera behind it now.
                r.rig.snap = true;
            }
        }
        // `window.__race.phys.nitro = n`.
        Some("nitro") => {
            if let Some(r) = play.race.as_mut() {
                r.session.curr.players[0].phys.nitro = v["amount"].as_f64().unwrap_or(1.0);
            }
        }
        // `window.__race.input.touch.autoGas = on`.
        Some("autogas") => {
            if let Some(r) = play.race.as_mut() {
                r.touch.auto_gas = v["on"].as_bool().unwrap_or(true);
            }
        }
        _ => warn!("stage: unknown command {v}"),
    }
}

/// The stage commands this game answers (`pk_shell::web::register_stage`).
const STAGE_COMMANDS: &[&str] = &[
    "choose",
    "slide",
    "focus",
    "act",
    "audio",
    "reset",
    "set",
    "aiWritePos",
    "event",
    "unit",
    "hurt",
    "say",
    "roadblock",
    "spikes",
    "padsetup",
    "finish",
    "cruise",
    "place",
    "nitro",
    "autogas",
];

fn run_stage(world: &mut World, v: &serde_json::Value) {
    if let Err(e) = world.run_system_cached_with(apply_stage, v.clone()) {
        warn!("stage: {e}");
    }
}

/// Reveals from the page.
fn take_reveal(mut ui: ResMut<UiState>) {
    if let Some(id) = REVEAL.lock().unwrap_or_else(|e| e.into_inner()).take() {
        ui.reveal = Some(id);
    }
}

/// Publishes the screens' state on the status global.
fn publish(
    mut ui: ResMut<UiState>,
    play: Res<Play>,
    status: Res<Status>,
    controls: super::ControlQuery,
    parents: Query<&ChildOf>,
    mut taps: ResMut<pk_shell::web::TapPolicy>,
) {
    publish_settings(&ui.settings);
    bridge::set("screen", ui.screen.name());
    bridge::set("mode", mode_name(&ui, &play, &status));
    bridge::set("races", ui.races);
    bridge::set("touchUi", play.touch_ui);
    bridge::set(
        "focus",
        ui.focus.as_deref().map_or(JsValue::NULL, JsValue::from_str),
    );
    if play.race.is_none() {
        crate::bridge::publish("race", None);
    }
    // The safe-area inset at the top (the rotate hint keeps under it).
    if let Some(ins) = pk_shell::web::insets()
        && (ins.top as f32 - ui.inset_top).abs() > 0.5
    {
        ui.inset_top = ins.top as f32;
        ui.dirty = true;
    }
    // How far the build line reaches up from the bottom (the menu keeps
    // clear of it).
    if let Some(f) = pk_shell::web::footer()
        && (f as f32 - ui.footer).abs() > 0.5
    {
        ui.footer = f as f32;
        ui.dirty = true;
    }
    // A tap on Race, Race again or Restart goes fullscreen on a touch
    // screen when the setting allows (`FullscreenOnTap`, `mark_taps`); the
    // Tilt option asks an iPhone for motion access (`PermissionOnTap`).
    let want = pk_shell::web::TapPolicy {
        fullscreen: play.touch_ui && ui.settings.fullscreen,
        permission: play.touch_ui,
    };
    if (taps.fullscreen, taps.permission) != (want.fullscreen, want.permission) {
        *taps = want;
    }
    let css = play.css_scale.max(0.01);
    let mut nodes = serde_json::Map::new();
    let snaps = snapshot(&controls, css);
    let ids: std::collections::BTreeMap<Entity, &str> =
        snaps.iter().map(|s| (s.entity, s.id.as_str())).collect();
    // The nearest control a control sits inside (for the overlap tests:
    // a control may cover the one it is part of, never any other).
    let parent_of = |e: Entity| {
        let mut at = e;
        while let Ok(p) = parents.get(at) {
            at = p.parent();
            if let Some(id) = ids.get(&at) {
                return Some(id.to_string());
            }
        }
        None
    };
    let parent_ids: Vec<Option<String>> = snaps.iter().map(|s| parent_of(s.entity)).collect();
    for (s, parent) in snaps.into_iter().zip(parent_ids) {
        let super::Snap {
            id,
            rect: r,
            shown,
            visible: vis,
            enabled,
            value,
            sel,
            z,
            ..
        } = s;
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
                "shown": { "x": shown.0, "y": shown.1, "w": shown.2, "h": shown.3 },
                "parent": parent,
            }),
        );
    }
    // The touch controls (`#touch [data-tap=…]`, `[data-act=…]` and the
    // stick, slider, strip, panel and wheel), from their layout; a hidden
    // one is not visible, as its zero-sized box was in the DOM.
    if let Some(race) = &play.race
        && play.touch_ui
    {
        use crate::play::touch::{DIRS, PEDAL_PADS, PedalKind, Rect, Steering, TAPS};
        let t = &race.touch;
        let lay = &t.layout;
        let shown = t.visible;
        let steering = t.steering.unwrap_or(Steering::Stick);
        let slider = t.pedals == PedalKind::Slider;
        let mut add = |name: &str, r: Rect, vis: bool| {
            nodes.insert(
                format!("touch-{name}"),
                serde_json::json!({
                    "x": r.left, "y": r.top, "w": r.width(), "h": r.height(),
                    "visible": shown && vis, "enabled": true, "value": null, "sel": false, "z": 0,
                }),
            );
        };
        for (i, name) in TAPS.iter().enumerate() {
            add(name, lay.taps[i], true);
        }
        for (i, h) in DIRS.iter().enumerate() {
            add(h.name(), lay.dirs[i], steering == Steering::Buttons);
        }
        for (i, h) in PEDAL_PADS.iter().enumerate() {
            add(h.name(), lay.pedals[i], !slider);
        }
        let stick = t.stick.map_or(lay.stick_box(), |s| {
            let hw = lay.stick_r + 32.0;
            Rect {
                left: s.x0 - hw,
                top: s.y0 - 32.0,
                right: s.x0 + hw,
                bottom: s.y0 + 32.0,
            }
        });
        add("stick", stick, steering == Steering::Stick);
        add("wheel", lay.wheel, steering == Steering::Tilt);
        add("slider", lay.track, slider);
        add("pedal", lay.panel, slider);
    }
    crate::bridge::publish(
        "uiNodes",
        Some(crate::bridge::V::Json(serde_json::Value::Object(nodes))),
    );
}

/// `settings` (the options as the menu holds them, for a control not
/// on screen) and `selects` (each drop-down's choices, a `<select>`'s
/// `options`), when they change.
fn publish_settings(s: &super::store::Settings) {
    use super::Sel;
    thread_local! {
        static LAST: std::cell::RefCell<Option<super::store::Settings>> = const { std::cell::RefCell::new(None) };
    }
    if LAST.with(|last| last.borrow().as_ref() == Some(s)) {
        return;
    }
    let mut selects = serde_json::Map::new();
    for (sel, id) in [
        (Sel::Track, "opt-track"),
        (Sel::Steer, "opt-steer"),
        (Sel::Pedals, "opt-pedals"),
        (Sel::Guide, "opt-guide"),
        (Sel::Assist, "opt-assist"),
        (Sel::Station, "opt-station"),
        (Sel::Scale, "opt-scale"),
        (Sel::Physics, "opt-physics"),
    ] {
        let (opts, _) = super::menu::select_options(sel, s);
        selects.insert(
            id.into(),
            serde_json::Value::Array(opts.into_iter().map(|(k, _)| k.into()).collect()),
        );
    }
    let settings = serde_json::json!({
        "music": s.music, "sfx": s.sfx, "mph": s.mph, "hq": s.hq, "autogas": s.autogas,
        "steering": s.steering, "tiltSens": s.tilt_sens, "pedals": s.pedals,
        "fullscreen": s.fullscreen, "car": s.car, "level": s.level, "track": s.track,
        "flash": s.flash, "rumble": s.rumble,
        "guide": s.guide, "assist": s.assist, "scale": s.scale,
        "physics": s.physics,
    });
    crate::bridge::publish("settings", Some(crate::bridge::V::Json(settings)));
    crate::bridge::publish(
        "selects",
        Some(crate::bridge::V::Json(serde_json::Value::Object(selects))),
    );
    LAST.with(|last| *last.borrow_mut() = Some(s.clone()));
}

pub fn plugin(app: &mut App) {
    for c in STAGE_COMMANDS {
        pk_shell::web::register_stage(c, run_stage);
    }
    app.configure_sets(Update, StageSet.before(super::pointer))
        .configure_sets(Update, PixelRatioSet.after(screen_state))
        .add_systems(Update, take_reveal.before(super::pointer))
        .add_systems(Update, screen_state.after(super::build))
        .add_systems(PostUpdate, mark_taps)
        .add_systems(Last, publish);
}
