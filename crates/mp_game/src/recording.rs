//! The run recording (the owner's request of 2026-10-05; DECISIONS D1021,
//! the format in `docs/rust-port/RECORDING.md`): a JSON-lines log of a
//! session for an agent to examine afterwards, and every race in it
//! replayable headless (`mp-sim replay <file>`). The recorder itself (the
//! file, the `start`, `frames`, `slow_frame`, `status`, `mode`, `adapter`,
//! `focus`, `gamepad`, `overlay`, `window`, `log` and `end` lines) is
//! Playground Kit's `pk_shell::recorder` (D1234); this module writes what
//! is this game's: each race's start (its seed and options), its inputs and
//! checkpoints (`ticks`, one line per 120 ticks: every tick's `InputFrame`
//! as the simulation took it, the state's hash after the last, the
//! player's car, the simulation's events), pause and results, the results,
//! the settings and every change to them, the gamepads and the sound's
//! state, and the client's mode (`race/none`) for the shell's lines.
//!
//! `record=1` (`--record` natively) writes `recordings/<UTC date>-<level>.jsonl`
//! under the repository root natively (`record=<path>` names the file, or
//! a directory for it); the path is printed at the start and the end. On
//! the web the lines are kept in memory, read with `__mp.recording()` and
//! saved as a download with `__mp.saveRecording()`.

use crate::Opts;
use crate::play::Play;
use crate::play::flow::Mode;
use crate::status::Status;
use bevy::prelude::*;
use mp_sim::input::InputFrame;
use pk_shell::recorder::{
    InputLog, RecordLine, Recorder, RecorderMode, RecorderPlugin, now, round3,
};
use serde_json::{Value, json};
use std::fmt::Write;

/// Ticks per `ticks` line (one second of race).
pub const CHUNK: usize = 120;

/// `record=1|<path>`: where to write, if anywhere.
pub fn wanted(o: &crate::options::Options) -> Option<String> {
    pk_shell::recorder::wanted(&pk_shell::params::Params::from_pairs(o.query.clone()))
}

/// The race being recorded.
struct RaceRec {
    log: InputLog<InputFrame>,
    mode: Mode,
    ended: bool,
}

/// The race being recorded, if one is.
#[derive(Resource, Default)]
struct RaceSlot(Option<RaceRec>);

/// What was last written, to write changes only.
#[derive(Resource, Default)]
struct Seen {
    settings: Option<crate::ui::store::Settings>,
    pads: String,
    audio: String,
    polled_at: f64,
}

pub fn plugin(app: &mut App) {
    let o = app.world().resource::<Opts>().o.clone();
    let Some(want) = wanted(&o) else { return };
    app.add_plugins(RecorderPlugin {
        want,
        name: o.level.clone(),
        root: root(),
        banner: crate::banner(),
        web_label: "__mp.recording()".into(),
        extra: json!({
            "level": o.level,
            "hq": o.hq,
            "race_on": o.race_on(),
            "tick_hz": 120,
            "chunk": CHUNK,
        }),
        query: o.query.clone(),
    });
    if app.world().get_resource::<Recorder>().is_none() {
        return; // the file could not be made
    }
    app.init_resource::<Seen>()
        .init_resource::<RaceSlot>()
        .add_systems(Update, race.after(crate::play::PlayFrame))
        .add_systems(Last, changes.before(pk_shell::recorder::frame));
}

#[cfg(not(target_arch = "wasm32"))]
fn root() -> std::path::PathBuf {
    crate::native::repo_root()
}

#[cfg(target_arch = "wasm32")]
fn root() -> std::path::PathBuf {
    std::path::PathBuf::new()
}

/// The race's side, once a frame after its ticks.
fn race(
    mut rec: ResMut<Recorder>,
    mut slot: ResMut<RaceSlot>,
    play: Option<Res<Play>>,
    opts: Res<Opts>,
) {
    let rec = &mut *rec;
    let t = now();
    let race = play.as_ref().and_then(|p| p.race.as_ref());
    // The race went (to the menu) or another took its place.
    let same = matches!((&slot.0, race), (Some(r), Some(n)) if r.log.id == n.starts);
    if !same && let Some(mut r) = slot.0.take() {
        let tick = r.log.tick;
        // The state the last inputs led to is gone: no hash.
        chunk(rec, &mut r.log, t, None);
        rec.write(
            t,
            "race_stop",
            Some(json!({"race": r.log.id, "tick": tick})),
        );
    }
    let (Some(play), Some(race)) = (play.as_ref(), race) else {
        return;
    };
    let st = &race.session.curr;
    if slot.0.is_none() {
        let o = race.setup.opts;
        let p = &play.params;
        rec.write(
            t,
            "race_start",
            Some(json!({
                "race": race.starts,
                "level": st_level(race, &opts),
                "car": o.car,
                "seed": o.seed,
                "pursuit": o.pursuit,
                "heat": o.heat,
                "physics": o.physics.name(),
                "cops": race.pursuit_opts.cops,
                "flash": race.pursuit_opts.flash,
                "pursuit_on": st.race.pursuit_on,
                "cruise": st.race.cruise,
                "laps": st.race.laps,
                "autodrive": race.setup.autodrive,
                "touch": race.setup.touch,
                "timescale": p.timescale,
                "rivals": st.rivals.len(),
                "traffic": st.traffic.cars.len(),
                "tick": st.tick,
            })),
        );
        slot.0 = Some(RaceRec {
            log: InputLog::new("race", race.starts, CHUNK),
            mode: Mode::Race,
            ended: false,
        });
    }
    let Some(r) = slot.0.as_mut() else { return };
    if st.tick > r.log.tick {
        let n = st.tick - r.log.tick;
        let got = &race.session.inputs;
        if let Some((ticks, inputs)) = r.log.advance(st.tick, got) {
            rec.write(
                t,
                "gap",
                Some(json!({"race": r.log.id, "ticks": ticks, "inputs": inputs, "tick": st.tick})),
            );
        }
        rec.add_ticks(n);
        for e in &race.log {
            r.log.events.push((st.tick, format!("{e:?}")));
        }
    }
    if r.log.due() {
        chunk(rec, &mut r.log, t, Some(race));
    }
    if race.mode != r.mode {
        chunk(rec, &mut r.log, t, Some(race));
        let name = match race.mode {
            Mode::Race => "race",
            Mode::Paused => "paused",
            Mode::Results => "results",
        };
        rec.write(
            t,
            "race_mode",
            Some(json!({"race": r.log.id, "mode": name, "tick": st.tick})),
        );
        r.mode = race.mode;
    }
    if !r.ended && race.mode == Mode::Results {
        r.ended = true;
        let pr = &st.players[0].rules;
        let rows: Vec<Value> = race
            .results
            .iter()
            .flatten()
            .map(|row| {
                json!({
                    "place": row.place,
                    "name": row.name,
                    "player": row.player,
                    "time": round3(row.time),
                    "estimated": row.estimated,
                })
            })
            .collect();
        let cruise = st
            .race
            .cruise
            .then(|| format!("{:?}", mp_sim::race::cruise_results(st)));
        let pursuit = mp_sim::race::pursuit_stats(st).map(|p| format!("{p:?}"));
        rec.write(
            t,
            "race_end",
            Some(json!({
                "race": r.log.id,
                "tick": st.tick,
                "time": round3(st.race.time),
                "finish_time": pr.finish_time,
                "lap_times": pr.lap_times,
                "results": rows,
                "cruise": cruise,
                "pursuit": pursuit,
            })),
        );
    }
}

/// The race's level id.
fn st_level(race: &crate::play::flow::Race, opts: &Opts) -> String {
    let id = race.session.lr.level.id;
    if id.is_empty() {
        opts.o.level.clone()
    } else {
        id.to_string()
    }
}

/// Writes the inputs not yet written as a `ticks` line.
fn chunk(
    rec: &mut Recorder,
    log: &mut InputLog<InputFrame>,
    t: f64,
    race: Option<&crate::play::flow::Race>,
) {
    if let Some(l) = ticks_line(log, t, race) {
        rec.raw(&l);
    }
}

/// The inputs not yet written as a `ticks` line, with the state's hash
/// after the last of them (when the race is at hand) and the player's car;
/// `None` when there are none.
fn ticks_line(
    log: &mut InputLog<InputFrame>,
    t: f64,
    race: Option<&crate::play::flow::Race>,
) -> Option<String> {
    let to = log.from + log.inputs.len() as u32 - 1;
    let check = race
        .filter(|x| x.session.curr.tick == to && !log.inputs.is_empty())
        .map(|race| {
            let st = &race.session.curr;
            let p = &st.players[0];
            let (v, ph) = (&p.v, &p.phys);
            let mut extra = String::with_capacity(256);
            let _ = write!(
                extra,
                ",\"race_state\":\"{:?}\",\"race_time\":{:.3},\"p\":{{\"s\":{:.2},\"lat\":{:.2},\
                 \"x\":{:.2},\"y\":{:.2},\"z\":{:.2},\"yaw\":{:.3},\"kmh\":{:.1},\"gear\":{},\
                 \"rpm\":{:.0},\"nitro\":{:.3},\"nitro_on\":{},\"drift\":{},\"on_ground\":{},\
                 \"lap\":{},\"alive\":{}}}",
                st.race.state,
                st.race.time,
                v.s,
                v.lat,
                v.x,
                v.y,
                v.z,
                v.yaw,
                v.speed * 3.6,
                ph.gear,
                ph.rpm,
                ph.nitro,
                ph.nitro_active,
                ph.drifting,
                v.on_ground,
                p.rules.lap,
                v.alive
            );
            (mp_sim::race::hash(st), extra)
        });
    log.line(
        t,
        &|inputs, out| mp_sim::replay::encode(inputs, out),
        check.as_ref().map(|(h, e)| (*h, e.as_str())),
    )
}

#[allow(clippy::too_many_arguments)]
/// The game's changes, once a frame before the shell's lines: the mode,
/// the settings, and four times a second the pads and the sound, by what
/// they say about themselves.
fn changes(
    mut mode: ResMut<RecorderMode>,
    mut lines: MessageWriter<RecordLine>,
    mut seen: ResMut<Seen>,
    status: Res<Status>,
    play: Option<Res<Play>>,
    ui: Option<Res<crate::ui::UiState>>,
    pads: Option<Res<crate::play::gamepad_io::PadsRes>>,
    audio: Option<NonSend<crate::play::audio::Shared>>,
) {
    let m = match (&ui, &play) {
        (Some(ui), Some(play)) => format!(
            "{}/{}",
            crate::ui::mode_name(ui, play, &status),
            ui.screen.name()
        ),
        _ => status.state.name().to_string(),
    };
    if mode.0.as_deref() != Some(m.as_str()) {
        mode.0 = Some(m);
    }
    if let Some(ui) = &ui
        && seen.settings.as_ref() != Some(&ui.settings)
    {
        let s = &ui.settings;
        lines.write(RecordLine::new(
            "settings",
            json!({
                "music": s.music, "sfx": s.sfx, "mph": s.mph, "hq": s.hq,
                "autogas": s.autogas, "steering": s.steering, "tilt_sens": s.tilt_sens,
                "pedals": s.pedals, "fullscreen": s.fullscreen, "car": s.car,
                "level": s.level, "track": s.track, "flash": s.flash, "rumble": s.rumble,
            }),
        ));
        seen.settings = Some(s.clone());
    }
    let t = now();
    if t - seen.polled_at < 0.25 {
        return;
    }
    seen.polled_at = t;
    if let Some(p) = &pads {
        let pads = &p.pads;
        let sig = format!(
            "{} {:?}",
            pads.state.connected,
            pads.active.as_ref().map(|a| (&a.id, a.index, &a.mapping))
        );
        if sig != seen.pads {
            lines.write(RecordLine::new(
                "pads",
                json!({
                    "connected": pads.state.connected,
                    "active": pads.active.as_ref().map(|a| json!({
                        "id": a.id, "index": a.index, "mapping": a.mapping,
                    })),
                }),
            ));
            seen.pads = sig;
        }
    }
    if let Some(a) = &audio
        && let Ok(a) = a.0.try_borrow()
    {
        let g = &a.audio;
        let v = json!({
            "ready": g.ready(),
            "context": g.ctx().map(|c| c.state().as_str()),
            "music_on": g.music_on(),
            "paused": g.paused(),
            "environment": g.environment(),
            "track": g.track_info().map(|i| i.id),
            "volumes": [g.volumes().0, g.volumes().1, g.volumes().2],
        });
        let sig = v.to_string();
        if sig != seen.audio {
            lines.write(RecordLine::new("audio", v));
            seen.audio = sig;
        }
    }
}

/// The recording so far (the page's `__mp.recording()`).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn recording_text() -> String {
    pk_shell::recorder::recording_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A race driven by keys through the client's frame loop (ragged
    /// frames, steering, the handbrake, a reset, a pause), its inputs taken
    /// as the recorder takes them and written as its `ticks` lines, replays
    /// in the simulation alone to the same state at every checkpoint.
    #[test]
    fn a_recorded_race_replays() {
        use crate::play::flow::{Race, Setup};
        use crate::play::touch::TouchControls;
        use mp_sim::race::{LevelRuntime, RaceOpts, hash, step};
        let opts = RaceOpts {
            car: "rally",
            seed: 7,
            pursuit: false,
            heat: 1.0,
            physics: mp_sim::model::Physics::Arcade,
        };
        let lr = LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap();
        let setup = Setup {
            opts,
            autodrive: false,
            touch: false,
        };
        let mut race = Race::new(lr, setup, TouchControls::default());
        let mut text = format!(
            "{{\"t\":0,\"type\":\"race_start\",\"race\":{},\"level\":\"coast\",\"car\":\"rally\",\
             \"seed\":7,\"pursuit\":false,\"heat\":1,\"cops\":6,\"flash\":true}}\n",
            race.starts
        );
        let mut log: InputLog<InputFrame> = InputLog::new("race", race.starts, CHUNK);
        let frames = [1.0 / 60.0, 1.0 / 144.0, 1.0 / 30.0, 0.0, 0.2];
        let mut paused = 0;
        // (frame, key, down)
        let script: &[(usize, &'static str, bool)] = &[
            (100, "ArrowUp", true),
            (400, "ArrowLeft", true),
            (460, "ArrowLeft", false),
            (900, "Space", true),
            (950, "Space", false),
            (1200, "KeyR", true),
            (1202, "KeyR", false),
            (1500, "Escape", true),
            (1502, "Escape", false),
            (1600, "KeyP", true),
            (1602, "KeyP", false),
        ];
        for i in 0..3000 {
            for &(_, code, down) in script.iter().filter(|s| s.0 == i) {
                if down {
                    race.input.key_down(code, false);
                } else {
                    race.input.key_up(code);
                }
            }
            race.frame(frames[i % frames.len()]);
            paused += u32::from(race.mode == Mode::Paused);
            let st = &race.session.curr;
            if st.tick > log.tick {
                assert_eq!(race.session.inputs.len() as u32, st.tick - log.tick);
                assert_eq!(log.advance(st.tick, &race.session.inputs), None);
            }
            if log.due() {
                text.push_str(&ticks_line(&mut log, 0.0, Some(&race)).unwrap());
                text.push('\n');
            }
        }
        text.push_str(&ticks_line(&mut log, 0.0, Some(&race)).unwrap());
        assert!(paused > 50, "{paused}");
        let rec = mp_sim::replay::parse(&text).unwrap();
        assert_eq!(rec.len(), 1);
        let rr = &rec[0];
        assert_eq!(rr.gaps, 0);
        assert!(rr.checks.len() > 20, "{}", rr.checks.len());
        let lr = LevelRuntime::new(mp_levels::level_by_id("coast")).unwrap();
        let mut st = rr.start(&lr).unwrap();
        let mut ev = Vec::new();
        let mut checks = rr.checks.iter().peekable();
        for f in &rr.inputs {
            step(&lr, &mut st, &[*f], &mut ev);
            ev.clear();
            if let Some(&&(t, h)) = checks.peek()
                && t == st.tick
            {
                assert_eq!(hash(&st), h, "tick {t}");
                checks.next();
            }
        }
        assert!(checks.peek().is_none());
        assert_eq!(st.tick, race.session.curr.tick);
        assert_eq!(hash(&st), hash(&race.session.curr));
    }
}
