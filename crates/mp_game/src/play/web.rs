//! The race's web glue: whether this is a touch device (the page decides,
//! as `isTouchDevice` does, into `__mp.touch`), the safe-area insets the
//! page measures (`__mp.insets`), a hidden page pausing the race
//! (`visibilitychange`), the JS's pixel ratio (`applyQuality`), and the
//! race's state on `window.__mp.race` for the tests.

use super::Play;
use super::flow::Mode;
use super::touch::{Hold, Insets};
use crate::bridge::{Obj, V};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use js_sys::{Object, Reflect};
use mp_track::Track;
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
    Reflect::get(&w, &JsValue::from_str("__mp"))
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

/// Each frame: the insets, the CSS scale, the hidden page, `__mp.race`.
fn frame(
    mut play: ResMut<Play>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    hud: Option<Res<super::hud::HudState>>,
    drawn: Option<Res<super::police::PoliceDrawn>>,
) {
    let Some(mr) = mr() else { return };
    bridge_page(hud.as_deref());
    bridge_pursuit(play.race.as_ref(), drawn.as_deref());
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
    let mut o = Obj::new();
    o.set("state", format!("{:?}", st.race.state).to_lowercase());
    o.set(
        "mode",
        match race.mode {
            Mode::Race => "race",
            Mode::Paused => "paused",
            Mode::Results => "results",
        },
    );
    o.set("tick", st.tick);
    o.set("time", st.race.time);
    o.set("countdown", st.race.countdown);
    o.set("s", p.v.s);
    o.set("lat", p.v.lat);
    o.set("speed", mp_math::kernel::hypot(p.v.vx, p.v.vz));
    o.set("gear", p.phys.gear);
    o.set("finished", p.rules.finished);
    o.set("locked", p.phys.locked);
    let mut inp = Obj::new();
    inp.set("steer", s.steer);
    inp.set("throttle", s.throttle);
    inp.set("brake", s.brake);
    inp.set("handbrake", s.handbrake);
    inp.set("nitro", s.nitro);
    inp.set("analog", s.analog);
    o.set("input", inp);
    // What the controls suites read off `__race`: the car, the camera's
    // right (`__camera.matrixWorld`'s x axis) and `input.touch`.
    o.set("yaw", p.v.yaw);
    o.set("steerAngle", p.v.steer_angle);
    // The road there: its half width, and the heading off it (`yawToRoad`).
    let f = race.session.lr.track.frame(p.v.s);
    let ang = mp_math::kernel::atan2(f.fz, f.fx) - p.v.yaw;
    o.set("hw", f.hw);
    o.set(
        "yawToRoad",
        mp_math::kernel::atan2(mp_math::kernel::sin(ang), mp_math::kernel::cos(ang)),
    );
    o.set("vx", p.v.vx);
    o.set("vz", p.v.vz);
    o.set("nitro", p.phys.nitro);
    o.set("nitroActive", p.phys.nitro_active);
    o.set("camMode", race.rig.mode as f64);
    if let Some(c) = cams.iter().next() {
        let r = c.right();
        let mut cr = Obj::new();
        cr.set("x", r.x);
        cr.set("z", r.z);
        o.set("camRight", cr);
    }
    let t = &race.touch;
    let mut to = Obj::new();
    to.set("visible", t.visible);
    to.set("mode", t.mode.name());
    to.set("steering", t.steering.map_or("", |s| s.name()));
    to.set("pedals", t.pedals.name());
    to.set("autoGas", t.auto_gas);
    to.set("stickR", t.layout.stick_r);
    let mut held = Obj::new();
    for h in [
        Hold::Throttle,
        Hold::Brake,
        Hold::Left,
        Hold::Right,
        Hold::Handbrake,
        Hold::Nitro,
    ] {
        held.set(h.name(), t.held.get(h));
    }
    to.set("held", held);
    if let Some(st) = t.stick {
        let mut so = Obj::new();
        so.set("id", st.id as f64);
        so.set("x0", st.x0);
        so.set("y0", st.y0);
        so.set("x", st.x);
        to.set("stick", so);
    } else {
        to.set("stick", V::Null);
    }
    to.set("lock", t.stick_offset().abs() >= t.layout.stick_r - 0.5);
    to.set("knob", t.stick_offset());
    let mut panel = Vec::<V>::new();
    for c in t.panel_classes() {
        panel.push(c.into());
    }
    to.set("panel", panel);
    to.set("wheel", t.wheel);
    if let Some(tl) = &t.tilt {
        let tl = tl.lock().unwrap_or_else(|e| e.into_inner());
        let mut tt = Obj::new();
        tt.set("state", tl.state.name());
        tt.set("live", tl.live());
        tt.set("roll", tl.roll);
        tt.set("steer", tl.steer);
        tt.set("fullLock", tl.full_lock);
        to.set("tilt", tt);
    }
    to.set(
        "u",
        t.slide
            .as_ref()
            .and_then(|s| s.u)
            .map(V::from)
            .unwrap_or(V::Null),
    );
    o.set("touch", to);
    o.set("touchUi", touch_ui);
    bridge_race(&mut o, race, &cams, hud.as_deref());
    if let Some(rows) = &race.results {
        let mut arr = Vec::<V>::new();
        for r in rows {
            let mut row = Obj::new();
            row.set("place", r.place as f64);
            row.set("name", r.name);
            row.set("player", r.player);
            row.set("time", r.time);
            row.set("estimated", r.estimated);
            arr.push(row.into());
        }
        o.set("results", arr);
    }
    crate::bridge::publish("race", Some(o.into()));
}

// ── The test bridge (SPEC 8.5, WP 6.7) ─────────────────────────────────

/// The race's road, for `__mp.trackFrame(s)` and `__mp.trackWrap(s)` (the
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

fn opt(v: Option<f64>) -> V {
    V::from(v)
}

/// What the page shows outside the race's own state: the HUD
/// (`__mp.hud`) and the countdown's lamps (`__mp.lamps`).
fn bridge_page(hud: Option<&super::hud::HudState>) {
    let mut h = Obj::new();
    let (shown, laps, texts) = hud.map_or((false, false, None), |h| h.bridge());
    h.set("shown", shown);
    h.set("laps", laps);
    let mut t = Obj::new();
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
            t.set(k, v.as_str());
        }
    }
    h.set("texts", t);
    // Hot Pursuit's furniture: what shows, the stars' fills and the lines.
    let pz = hud.map(|h| h.bridge_pz()).unwrap_or_default();
    h.set("pz", pz.pz);
    h.set("pzBar", pz.bar);
    h.set("dmg", pz.dmg);
    h.set("hold", pz.hold);
    h.set("pen", pz.pen);
    h.set("radio", pz.radio);
    h.set("radioText", pz.radio_text.as_str());
    h.set("pzLabel", pz.label.as_str());
    let mut stars = Vec::<V>::new();
    for f in pz.stars {
        stars.push(f.into());
    }
    h.set("stars", stars);
    crate::bridge::publish("hud", Some(h.into()));
    let mut l = Obj::new();
    l.set("n", crate::animate::LAMPS.load(Ordering::Relaxed));
    l.set("lit", crate::animate::LAMPS_LIT.load(Ordering::Relaxed));
    crate::bridge::publish("lamps", Some(l.into()));
}

/// `window.__pursuit` (the race's `Pursuit`) and `race.pv`'s fields, for
/// the pursuit suite: null without a pursuit, as the JS clears it.
fn bridge_pursuit(race: Option<&super::flow::Race>, drawn: Option<&super::police::PoliceDrawn>) {
    use mp_sim::pursuit::{HoldReason, State};
    let Some(pv) = race.and_then(|r| r.session.curr.pv.as_ref()) else {
        crate::bridge::publish("pursuit", Some(V::Null));
        return;
    };
    let pu = &pv.pursuit;
    let mut o = Obj::new();
    o.set("available", true);
    o.set(
        "state",
        match pu.state {
            State::Patrol => "patrol",
            State::Pursuit => "pursuit",
            State::Cooldown => "cooldown",
        },
    );
    o.set("heat", pu.heat);
    o.set("maxHeat", pu.max_heat);
    o.set("heatMeter", pu.heat_meter);
    o.set("bust", pu.bust);
    o.set("evade", pu.evade);
    o.set("busts", pu.busts);
    o.set("takedowns", pu.takedowns);
    o.set("flash", pu.flash);
    o.set("maxUnits", pu.max_units as f64);
    let mut units = Vec::<V>::new();
    for (i, u) in pu.units.iter().enumerate() {
        let mut uo = Obj::new();
        uo.set("active", u.active);
        uo.set("mode", mode_name(u.mode));
        uo.set(
            "siren",
            match u.siren {
                mp_sim::police::Siren::Off => "off",
                mp_sim::police::Siren::Flash => "flash",
                mp_sim::police::Siren::Disabled => "disabled",
            },
        );
        uo.set("s", u.k.s);
        uo.set("lat", u.k.lat);
        uo.set("speed", u.k.speed);
        uo.set("callsign", u.callsign);
        uo.set("type", format!("{:?}", u.unit_type).to_lowercase());
        uo.set("x", u.k.v.x);
        uo.set("z", u.k.v.z);
        uo.set("health", u.health);
        uo.set("target", u.target.map_or(V::Null, |t| V::from(t as f64)));
        // `u.v.model.root.visible`: whether the police draw shows this
        // unit's model this frame (`play::police::PoliceDrawn`, D923).
        uo.set(
            "visible",
            drawn.and_then(|d| d.0.get(i).copied()).unwrap_or(false),
        );
        units.push(uo.into());
    }
    o.set("units", units);
    if let Some(p) = pu.player.map(|i| &pu.racers[i]) {
        let mut po = Obj::new();
        po.set("hold", p.hold);
        po.set(
            "holdReason",
            match p.hold_reason {
                Some(HoldReason::Busted) => V::from("busted"),
                Some(HoldReason::Wrecked) => V::from("wrecked"),
                None => V::Undefined,
            },
        );
        po.set("holdTotal", p.hold_total);
        po.set("grace", p.grace);
        po.set("bust", p.bust);
        o.set("player", po);
    }
    let mut v = Obj::new();
    v.set("damage", pv.damage);
    v.set("wrecks", pv.wrecks);
    v.set("penalty", pv.penalty);
    o.set("pv", v);
    crate::bridge::publish("pursuit", Some(o.into()));
}

/// A unit's mode as the JS names it.
fn mode_name(m: mp_sim::police::Mode) -> &'static str {
    use mp_sim::police::Mode as M;
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

fn mode_of(name: &str) -> Option<mp_sim::police::Mode> {
    use mp_sim::police::Mode as M;
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

/// The rest of `__mp.race`: what the JS suites read off `window.__race`
/// (the car, its physics and rules, the rivals, the road, the camera).
fn bridge_race(
    o: &mut Obj,
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
    o.set("x", p.v.x);
    o.set("y", p.v.y);
    o.set("z", p.v.z);
    o.set("along", p.v.speed);
    o.set("prog", opt(p.v.prog));
    o.set("skid", p.phys.skid);
    o.set("damage", p.phys.damage);
    o.set("lap", p.rules.lap);
    let mut laps = Vec::<V>::new();
    for l in &p.rules.lap_times {
        laps.push((*l).into());
    }
    o.set("lapTimes", laps);
    o.set("playerFinished", p.rules.finished);
    o.set("playerTime", opt(p.rules.finish_time));
    o.set("dist", p.rules.dist);
    o.set("lastS", opt(p.rules.last_s));
    o.set("odo", opt(p.rules.odo));
    o.set("cruise", st.race.cruise);
    o.set("score", p.rules.score);
    o.set("nearMisses", p.rules.near_misses);
    o.set("pursuitOn", st.race.pursuit_on);
    o.set("traffic", true);
    let standings = mp_sim::race::standings(st);
    let place = standings.iter().position(|s| s.player).map_or(1, |i| i + 1);
    o.set("place", place as f64);
    o.set("racers", standings.len() as f64);
    let mut ais = Vec::<V>::new();
    for a in &st.rivals {
        let mut ao = Obj::new();
        let v = &a.k.v;
        ao.set("s", a.k.s);
        ao.set("lat", a.k.lat);
        ao.set("speed", a.k.speed);
        ao.set("prog", opt(a.prog));
        ao.set("finished", a.finished);
        ao.set("finishTime", opt(a.finish_time));
        ao.set("x", v.x);
        ao.set("y", v.y);
        ao.set("z", v.z);
        ao.set("vx", v.vx);
        ao.set("vz", v.vz);
        ao.set("yaw", v.yaw);
        ais.push(ao.into());
    }
    o.set("ais", ais);
    let mut tr = Obj::new();
    tr.set("startS", t.start_s);
    tr.set("finishS", t.finish_s);
    tr.set("n", t.n as f64);
    tr.set("length", t.length);
    tr.set("loop", t.is_loop);
    tr.set("roadEnd", t.road_end());
    o.set("track", tr);
    if let Some(c) = cams.iter().next() {
        let pos = c.translation();
        let mut cp = Obj::new();
        cp.set("x", pos.x);
        cp.set("y", pos.y);
        cp.set("z", pos.z);
        o.set("camPos", cp);
    }
    if let Some(inp) = o.get_mut("input") {
        inp.set("lookBack", race.input.state.look_back);
    }
}

/// The race's staging commands (`__mp.stage`, from `ui::web`): what the
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
                r.inject.push(mp_sim::race::SimEvent::Phys {
                    player: 0,
                    e: mp_sim::physics::PhysEvent::Impact {
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
            mp_sim::race::hurt_player(&r.session.lr, &mut r.session.curr, num("d").unwrap_or(0.0))
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
                mp_audio::radio::lines::Line { text, parts },
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
                    let racers = mp_sim::field::RacerAccess {
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
                _ => warn!("__mp.stage: no player.{f}"),
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
                _ => warn!("__mp.stage: no pv.{f}"),
            }
        }
        // `__pursuit.state = 'pursuit'`.
        ["pursuit", "state"] => {
            use mp_sim::pursuit::State;
            let Some(pv) = st.pv.as_mut() else { return };
            match value.as_str() {
                Some("patrol") => pv.pursuit.state = State::Patrol,
                Some("pursuit") => pv.pursuit.state = State::Pursuit,
                Some("cooldown") => pv.pursuit.state = State::Cooldown,
                _ => warn!("__mp.stage: no pursuit state {value}"),
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
                    .and_then(|k| pu.racer_for(mp_sim::body::BodyId::Rival(k))),
            };
            let Some(u) = i.parse::<usize>().ok().and_then(|i| pu.units.get_mut(i)) else {
                return;
            };
            match *f {
                "speed" => u.k.speed = x.unwrap_or(u.k.speed),
                "s" => u.k.s = x.unwrap_or(u.k.s),
                "lat" => u.k.lat = x.unwrap_or(u.k.lat),
                "target" => u.target = target,
                _ => warn!("__mp.stage: no unit.{f}"),
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
                _ => warn!("__mp.stage: no ais[].{f}"),
            }
        }
        _ => warn!("__mp.stage: cannot set {path}"),
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(Update, visibility.before(super::PlayFrame))
        .add_systems(Last, frame);
}
