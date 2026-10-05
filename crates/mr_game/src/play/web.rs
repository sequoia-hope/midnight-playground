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
use mr_track::Track;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use wasm_bindgen::prelude::{Closure, wasm_bindgen};
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
    hud: Option<Res<super::hud::HudState>>,
) {
    let Some(mr) = mr() else { return };
    bridge_page(&mr, hud.as_deref());
    bridge_pursuit(&mr, play.race.as_ref());
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
    set(
        &to,
        "u",
        t.slide
            .as_ref()
            .and_then(|s| s.u)
            .map_or(JsValue::NULL, JsValue::from_f64),
    );
    set(&o, "touch", to);
    set(&o, "touchUi", touch_ui);
    bridge_race(&o, race, &cams, hud.as_deref());
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

// ── The test bridge (SPEC 8.5, WP 6.7) ─────────────────────────────────

/// The race's road, for `__mr.trackFrame(s)` and `__mr.trackWrap(s)` (the
/// suites' `__race.track.frame(s)` and `.wrap(s)`, read synchronously).
static TRACK: Mutex<Option<Arc<Track>>> = Mutex::new(None);

fn track() -> Option<Arc<Track>> {
    TRACK.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// `track.frame(s)`: `[x, y, z, fx, fz, rx, rz, hw]`, empty with no race.
#[wasm_bindgen]
pub fn track_frame(s: f64) -> Vec<f64> {
    track().map_or_else(Vec::new, |t| {
        let f = t.frame(s);
        vec![f.x, f.y, f.z, f.fx, f.fz, f.rx, f.rz, f.hw]
    })
}

/// `track.wrap(s)`.
#[wasm_bindgen]
pub fn track_wrap(s: f64) -> f64 {
    track().map_or(s, |t| t.wrap(s))
}

fn opt(v: Option<f64>) -> JsValue {
    v.map_or(JsValue::NULL, JsValue::from_f64)
}

/// What the page shows outside the race's own state: the HUD
/// (`__mr.hud`) and the countdown's lamps (`__mr.lamps`).
fn bridge_page(mr: &Object, hud: Option<&super::hud::HudState>) {
    let h = Object::new();
    let (shown, laps, texts) = hud.map_or((false, false, None), |h| h.bridge());
    set(&h, "shown", shown);
    set(&h, "laps", laps);
    let t = Object::new();
    if let Some(x) = texts {
        for (k, v) in [
            ("pos", &x.pos),
            ("time", &x.time),
            ("zone", &x.zone),
            ("speed", &x.speed),
            ("gear", &x.gear),
            ("lapN", &x.lap_n),
            ("lapTime", &x.lap_time),
            ("lapBest", &x.lap_best),
            ("score", &x.score),
            ("dist", &x.dist),
            ("best", &x.best),
            ("pen", &x.pen),
        ] {
            set(&t, k, v.as_str());
        }
    }
    set(&h, "texts", t);
    // Hot Pursuit's furniture: what shows, the stars' fills and the lines.
    let pz = hud.map(|h| h.bridge_pz()).unwrap_or_default();
    set(&h, "pz", pz.pz);
    set(&h, "pzBar", pz.bar);
    set(&h, "dmg", pz.dmg);
    set(&h, "hold", pz.hold);
    set(&h, "pen", pz.pen);
    set(&h, "radio", pz.radio);
    set(&h, "radioText", pz.radio_text.as_str());
    set(&h, "pzLabel", pz.label.as_str());
    let stars = js_sys::Array::new();
    for f in pz.stars {
        stars.push(&JsValue::from_f64(f));
    }
    set(&h, "stars", stars);
    set(mr, "hud", h);
    let l = Object::new();
    set(&l, "n", crate::animate::LAMPS.load(Ordering::Relaxed));
    set(&l, "lit", crate::animate::LAMPS_LIT.load(Ordering::Relaxed));
    set(mr, "lamps", l);
}

/// `window.__pursuit` (the race's `Pursuit`) and `race.pv`'s fields, for
/// the pursuit suite: null without a pursuit, as the JS clears it.
fn bridge_pursuit(mr: &Object, race: Option<&super::flow::Race>) {
    use mr_sim::pursuit::{HoldReason, State};
    let Some(pv) = race.and_then(|r| r.session.curr.pv.as_ref()) else {
        set(mr, "pursuit", JsValue::NULL);
        return;
    };
    let pu = &pv.pursuit;
    let o = Object::new();
    set(&o, "available", true);
    set(
        &o,
        "state",
        match pu.state {
            State::Patrol => "patrol",
            State::Pursuit => "pursuit",
            State::Cooldown => "cooldown",
        },
    );
    set(&o, "heat", pu.heat);
    set(&o, "maxHeat", pu.max_heat);
    set(&o, "heatMeter", pu.heat_meter);
    set(&o, "bust", pu.bust);
    set(&o, "evade", pu.evade);
    set(&o, "busts", pu.busts);
    set(&o, "takedowns", pu.takedowns);
    set(&o, "flash", pu.flash);
    set(&o, "maxUnits", pu.max_units as f64);
    let units = js_sys::Array::new();
    for u in &pu.units {
        let uo = Object::new();
        set(&uo, "active", u.active);
        set(&uo, "mode", mode_name(u.mode));
        set(
            &uo,
            "siren",
            match u.siren {
                mr_sim::police::Siren::Off => "off",
                mr_sim::police::Siren::Flash => "flash",
                mr_sim::police::Siren::Disabled => "disabled",
            },
        );
        set(&uo, "s", u.k.s);
        set(&uo, "lat", u.k.lat);
        set(&uo, "speed", u.k.speed);
        set(&uo, "callsign", u.callsign);
        set(&uo, "type", format!("{:?}", u.unit_type).to_lowercase());
        set(&uo, "x", u.k.v.x);
        set(&uo, "z", u.k.v.z);
        set(&uo, "health", u.health);
        set(
            &uo,
            "target",
            u.target
                .map_or(JsValue::NULL, |t| JsValue::from_f64(t as f64)),
        );
        // `u.v.model.root.visible`: TODO(merge with WP 8.1) read
        // `crate::play::police::PoliceDrawn` (one bool per unit, filled by
        // the police draw); until the police are drawn, nothing is.
        set(&uo, "visible", false);
        units.push(&uo);
    }
    set(&o, "units", units);
    if let Some(p) = pu.player.map(|i| &pu.racers[i]) {
        let po = Object::new();
        set(&po, "hold", p.hold);
        set(
            &po,
            "holdReason",
            match p.hold_reason {
                Some(HoldReason::Busted) => JsValue::from_str("busted"),
                Some(HoldReason::Wrecked) => JsValue::from_str("wrecked"),
                None => JsValue::UNDEFINED,
            },
        );
        set(&po, "holdTotal", p.hold_total);
        set(&po, "grace", p.grace);
        set(&po, "bust", p.bust);
        set(&o, "player", po);
    }
    let v = Object::new();
    set(&v, "damage", pv.damage);
    set(&v, "wrecks", pv.wrecks);
    set(&v, "penalty", pv.penalty);
    set(&o, "pv", v);
    set(mr, "pursuit", o);
}

/// A unit's mode as the JS names it.
fn mode_name(m: mr_sim::police::Mode) -> &'static str {
    use mr_sim::police::Mode as M;
    match m {
        M::Parked => "parked",
        M::Chase => "chase",
        M::Oncoming => "oncoming",
        M::Search => "search",
        M::Standdown => "standdown",
        M::Hold => "hold",
        M::Disabled => "disabled",
        M::Block => "block",
    }
}

fn mode_of(name: &str) -> Option<mr_sim::police::Mode> {
    use mr_sim::police::Mode as M;
    Some(match name {
        "parked" => M::Parked,
        "chase" => M::Chase,
        "oncoming" => M::Oncoming,
        "search" => M::Search,
        "standdown" => M::Standdown,
        "hold" => M::Hold,
        "disabled" => M::Disabled,
        "block" => M::Block,
        _ => return None,
    })
}

/// The rest of `__mr.race`: what the JS suites read off `window.__race`
/// (the car, its physics and rules, the rivals, the road, the camera).
fn bridge_race(
    o: &Object,
    race: &super::flow::Race,
    cams: &Query<&GlobalTransform, With<Camera3d>>,
    _hud: Option<&super::hud::HudState>,
) {
    let st = &race.session.curr;
    let p = &st.players[0];
    let t = &race.session.lr.track;
    {
        let mut slot = TRACK.lock().unwrap_or_else(|e| e.into_inner());
        if !slot.as_ref().is_some_and(|a| Arc::ptr_eq(a, t)) {
            *slot = Some(t.clone());
        }
    }
    set(o, "x", p.v.x);
    set(o, "y", p.v.y);
    set(o, "z", p.v.z);
    set(o, "along", p.v.speed);
    set(o, "prog", opt(p.v.prog));
    set(o, "skid", p.phys.skid);
    set(o, "damage", p.phys.damage);
    set(o, "lap", p.rules.lap);
    let laps = js_sys::Array::new();
    for l in &p.rules.lap_times {
        laps.push(&JsValue::from_f64(*l));
    }
    set(o, "lapTimes", laps);
    set(o, "playerFinished", p.rules.finished);
    set(o, "playerTime", opt(p.rules.finish_time));
    set(o, "dist", p.rules.dist);
    set(o, "lastS", opt(p.rules.last_s));
    set(o, "odo", opt(p.rules.odo));
    set(o, "cruise", st.race.cruise);
    set(o, "score", p.rules.score);
    set(o, "nearMisses", p.rules.near_misses);
    set(o, "pursuitOn", st.race.pursuit_on);
    set(o, "traffic", true);
    let standings = mr_sim::race::standings(st);
    let place = standings.iter().position(|s| s.player).map_or(1, |i| i + 1);
    set(o, "place", place as f64);
    set(o, "racers", standings.len() as f64);
    let ais = js_sys::Array::new();
    for a in &st.rivals {
        let ao = Object::new();
        let v = &a.k.v;
        set(&ao, "s", a.k.s);
        set(&ao, "lat", a.k.lat);
        set(&ao, "speed", a.k.speed);
        set(&ao, "prog", opt(a.prog));
        set(&ao, "finished", a.finished);
        set(&ao, "finishTime", opt(a.finish_time));
        set(&ao, "x", v.x);
        set(&ao, "y", v.y);
        set(&ao, "z", v.z);
        set(&ao, "vx", v.vx);
        set(&ao, "vz", v.vz);
        set(&ao, "yaw", v.yaw);
        ais.push(&ao);
    }
    set(o, "ais", ais);
    let tr = Object::new();
    set(&tr, "startS", t.start_s);
    set(&tr, "finishS", t.finish_s);
    set(&tr, "n", t.n as f64);
    set(&tr, "length", t.length);
    set(&tr, "loop", t.is_loop);
    set(&tr, "roadEnd", t.road_end());
    set(o, "track", tr);
    if let Some(c) = cams.iter().next() {
        let pos = c.translation();
        let cp = Object::new();
        set(&cp, "x", pos.x);
        set(&cp, "y", pos.y);
        set(&cp, "z", pos.z);
        set(o, "camPos", cp);
    }
    if let Ok(inp) = Reflect::get(o, &JsValue::from_str("input"))
        && let Some(inp) = inp.dyn_ref::<Object>()
    {
        set(inp, "lookBack", race.input.state.look_back);
    }
}

/// The race's staging commands (`__mr.stage`, from `ui::web`): what the
/// suites write into `window.__race`. Returns false for a command that is
/// not the race's.
pub fn stage_race(play: &mut Play, v: &serde_json::Value) -> bool {
    let cmd = v["cmd"].as_str().unwrap_or("");
    if !matches!(
        cmd,
        "reset" | "set" | "aiWritePos" | "event" | "unit" | "hurt" | "say" | "roadblock" | "spikes"
    ) {
        return false;
    }
    let Some(r) = play.race.as_mut() else {
        return true;
    };
    let t = r.session.lr.track.clone();
    let num = |k: &str| v[k].as_f64();
    match cmd {
        // `race.phys.reset(s, lat)`.
        "reset" => {
            let p = &mut r.session.curr.players[0];
            let s = num("s").unwrap_or(p.v.s);
            p.phys.reset(&mut p.v, &t, s, num("lat").unwrap_or(0.0));
        }
        // `a.writePos()`.
        "aiWritePos" => {
            let i = v["i"].as_u64().unwrap_or(0) as usize;
            if let Some(a) = r.session.curr.rivals.get_mut(i) {
                a.write_pos(&t);
            }
        }
        // `race.phys.events.push({type: 'impact', strength, x, y, z, side})`.
        "event" => {
            if v["type"].as_str() == Some("impact") {
                r.inject.push(mr_sim::race::SimEvent::Phys {
                    player: 0,
                    e: mr_sim::physics::PhysEvent::Impact {
                        strength: num("strength").unwrap_or(0.5),
                        x: num("x").unwrap_or(0.0),
                        y: num("y").unwrap_or(0.0),
                        z: num("z").unwrap_or(0.0),
                        side: num("side").unwrap_or(1.0) as i32,
                    },
                });
            }
        }
        // `__pursuit.activate(unit, s, lat, speed, mode, dir)`.
        "unit" => {
            let i = v["i"].as_u64().unwrap_or(0) as usize;
            let mode = v["mode"].as_str().and_then(mode_of);
            let st = &mut r.session.curr;
            if let (Some(pv), Some(mode)) = (st.pv.as_mut(), mode)
                && i < pv.pursuit.units.len()
            {
                pv.pursuit.activate(
                    &t,
                    i,
                    num("s").unwrap_or(0.0),
                    num("lat").unwrap_or(0.0),
                    num("speed").unwrap_or(0.0),
                    mode,
                    num("dir").unwrap_or(1.0) as i32,
                );
            }
        }
        // `race.pv.hurt(d)`.
        "hurt" => {
            mr_sim::race::hurt_player(&r.session.lr, &mut r.session.curr, num("d").unwrap_or(0.0))
        }
        // `race.pv.say(line, now)`: through the radio's own `say` in the
        // next frame (`play::radio`).
        "say" => {
            let text = v["text"].as_str().unwrap_or("").to_owned();
            let parts = v["parts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|p| p.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_else(|| vec![text.clone()]);
            r.stage_say.push((
                mr_audio::radio::lines::Line { text, parts },
                v["force"].as_bool().unwrap_or(false),
            ));
            return true;
        }
        // `__pursuit.placeRoadblock(s)`, `__pursuit.placeSpikes(s)`.
        "roadblock" | "spikes" => {
            let st = &mut r.session.curr;
            let s = num("s").unwrap_or(st.players[0].v.s + 300.0);
            if let Some(pv) = st.pv.as_mut() {
                if cmd == "roadblock" {
                    pv.pursuit.place_roadblock(&t, s, &mut st.rng.pursuit);
                } else {
                    let racers = mr_sim::field::RacerAccess {
                        players: &mut st.players,
                        rivals: &mut st.rivals,
                    };
                    pv.pursuit.place_spikes(&t, &racers, s);
                }
            }
        }
        // A field written: `race.player.vx = …`, `race.ais[i].s = …`.
        _ => set_field(r, v["path"].as_str().unwrap_or(""), &v["value"]),
    }
    r.session.prev = r.session.curr.clone();
    true
}

fn set_field(r: &mut super::flow::Race, path: &str, value: &serde_json::Value) {
    let x = value.as_f64();
    let b = value.as_bool();
    let st = &mut r.session.curr;
    let parts: Vec<&str> = path.split('.').collect();
    let p = &mut st.players[0];
    match parts.as_slice() {
        ["player", f] => {
            let Some(x) = x else { return };
            match *f {
                "x" => p.v.x = x,
                "z" => p.v.z = x,
                "vx" => p.v.vx = x,
                "vz" => p.v.vz = x,
                "yaw" => p.v.yaw = x,
                "s" => p.v.s = x,
                "lat" => p.v.lat = x,
                "speed" => p.v.speed = x,
                "prog" => p.v.prog = Some(x),
                _ => warn!("__mr.stage: no player.{f}"),
            }
        }
        ["phys", "nitro"] => p.phys.nitro = x.unwrap_or(p.phys.nitro),
        ["phys", "damage"] => p.phys.damage = x.unwrap_or(p.phys.damage),
        ["lastS"] => p.rules.last_s = x,
        ["odo"] => p.rules.odo = x,
        ["score"] => p.rules.score = x.unwrap_or(p.rules.score),
        ["nearMisses"] => p.rules.near_misses = x.unwrap_or(0.0) as i32,
        ["cam", "snap"] => r.rig.snap = b.unwrap_or(true),
        // `race.pv.penalty = x`, `race.pv.damage = x`.
        ["pv", f] => {
            let Some(pv) = st.pv.as_mut() else { return };
            match *f {
                "penalty" => pv.penalty = x.unwrap_or(pv.penalty),
                "damage" => pv.damage = x.unwrap_or(pv.damage),
                _ => warn!("__mr.stage: no pv.{f}"),
            }
        }
        // `__pursuit.state = 'pursuit'`.
        ["pursuit", "state"] => {
            use mr_sim::pursuit::State;
            let Some(pv) = st.pv.as_mut() else { return };
            match value.as_str() {
                Some("patrol") => pv.pursuit.state = State::Patrol,
                Some("pursuit") => pv.pursuit.state = State::Pursuit,
                Some("cooldown") => pv.pursuit.state = State::Cooldown,
                _ => warn!("__mr.stage: no pursuit state {value}"),
            }
        }
        // A unit's field: `u.speed = 0`, `u.target = race.playerBody` (the
        // racer's index, the player's or a rival's).
        ["pursuit", "units", i, f] => {
            let Some(pv) = st.pv.as_mut() else { return };
            let pu = &mut pv.pursuit;
            let target = match value.as_str() {
                Some("player") => pu.player,
                _ => value
                    .as_str()
                    .and_then(|r| r.strip_prefix("rival:"))
                    .and_then(|k| k.parse::<usize>().ok())
                    .and_then(|k| pu.racer_for(mr_sim::body::BodyId::Rival(k))),
            };
            let Some(u) = i.parse::<usize>().ok().and_then(|i| pu.units.get_mut(i)) else {
                return;
            };
            match *f {
                "speed" => u.k.speed = x.unwrap_or(u.k.speed),
                "s" => u.k.s = x.unwrap_or(u.k.s),
                "lat" => u.k.lat = x.unwrap_or(u.k.lat),
                "target" => u.target = target,
                _ => warn!("__mr.stage: no unit.{f}"),
            }
        }
        ["touch", "autoGas"] => r.touch.auto_gas = b.unwrap_or(true),
        ["progS", i] => {
            if let (Ok(i), Some(x)) = (i.parse::<usize>(), x)
                && let Some(slot) = st.race.prog_s.get_mut(i)
            {
                *slot = x;
            }
        }
        ["ais", i, f] => {
            let Some(a) = i.parse::<usize>().ok().and_then(|i| st.rivals.get_mut(i)) else {
                return;
            };
            match *f {
                "s" => a.k.s = x.unwrap_or(a.k.s),
                "lat" => a.k.lat = x.unwrap_or(a.k.lat),
                "speed" => a.k.speed = x.unwrap_or(a.k.speed),
                "prog" => a.prog = x,
                "finished" => a.finished = b.unwrap_or(false),
                "finishTime" => a.finish_time = x,
                _ => warn!("__mr.stage: no ais[].{f}"),
            }
        }
        _ => warn!("__mr.stage: cannot set {path}"),
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(Update, visibility.before(super::PlayFrame))
        .add_systems(Last, frame);
}
