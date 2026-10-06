//! The run recording (the owner's request of 2026-10-05; DECISIONS D1021,
//! the format in `docs/rust-port/RECORDING.md`): a JSON-lines log of a
//! session for an agent to examine afterwards, and every race in it
//! replayable headless (`mp-sim replay <file>`).
//!
//! `record=1` (`--record` natively) writes `recordings/<UTC date>-<level>.jsonl`
//! under the repository root natively (`record=<path>` names the file, or
//! a directory for it); the path is printed at the start and the end. On
//! the web the lines are kept in memory, read with `__mp.recording()` and
//! saved as a download with `__mp.saveRecording()`.
//!
//! What goes in (each line `{"t": seconds since start, "type": …, …}`):
//! the start (build, options, the date), the adapter, the settings and
//! every change to them, the client's mode and screen, the loading status,
//! one `frames` line a second (the frame interval's count, mean, min, p99
//! and max, the main and render worlds' CPU times from
//! `debug_overlay::FrameTimes`, pipelines waiting), a `slow_frame` line for
//! every frame over 50 ms, each race's start (its seed and options), its
//! inputs and checkpoints (`ticks`, one line per 120 ticks: every tick's
//! `InputFrame` as the simulation took it, the state's hash after the last,
//! the player's car, the simulation's events), pause and results, the
//! results, the window's focus, size and mode, gamepads, the sound's state,
//! the debug overlay toggled, the log's warnings and errors (and the
//! client's own info lines, natively), and the end.

use crate::Opts;
use crate::debug_overlay::{FrameTimes, Overlay};
use crate::play::Play;
use crate::play::flow::Mode;
use crate::status::Status;
use bevy::app::AppExit;
use bevy::input::gamepad::GamepadConnectionEvent;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::{PrimaryWindow, WindowFocused};
use mp_sim::input::InputFrame;
use serde_json::{Value, json};
use std::fmt::Write;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// The format's version (the `start` line's `version`).
pub const VERSION: u32 = 1;
/// A frame over this is a `slow_frame` line (the status log's threshold,
/// D1005).
pub const SLOW_MS: f32 = 50.0;
/// Ticks per `ticks` line (one second of race).
pub const CHUNK: usize = 120;

static START: OnceLock<Instant> = OnceLock::new();
/// Recording is on: the log layer keeps lines for it.
static ON: AtomicBool = AtomicBool::new(false);
/// Log lines from any thread, with their time, until the next frame
/// writes them.
static LOG: Mutex<Vec<(f64, &'static str, String, String)>> = Mutex::new(Vec::new());
/// The web's recording, for the page (`recording_text`).
#[cfg(target_arch = "wasm32")]
static WEB: Mutex<String> = Mutex::new(String::new());

/// Seconds since the recording started.
fn now() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Whether the log layer should keep a record of this level and target:
/// warnings and errors, and the client's own info lines.
pub fn wants_log(level: bevy::log::Level, target: &str) -> bool {
    ON.load(Ordering::Relaxed)
        && (level <= bevy::log::Level::WARN
            || (level == bevy::log::Level::INFO && target.starts_with("mp_")))
}

/// A log record (from the native log layer, `native::log_tally`).
pub fn log(level: bevy::log::Level, target: &str, message: &str) {
    let kind = match level {
        bevy::log::Level::ERROR => "error",
        bevy::log::Level::WARN => "warn",
        _ => "info",
    };
    let mut l = LOG.lock().unwrap_or_else(|e| e.into_inner());
    // A runaway logger must not fill the memory between two frames.
    if l.len() < 1000 {
        l.push((
            now(),
            kind,
            target.to_owned(),
            message.trim_end().to_owned(),
        ));
    }
}

/// `record=1|<path>`: where to write, if anywhere.
pub fn wanted(o: &crate::options::Options) -> Option<String> {
    o.param("record")
        .filter(|v| !v.is_empty() && *v != "0" && *v != "false")
        .map(str::to_owned)
}

enum Out {
    #[cfg(not(target_arch = "wasm32"))]
    File(std::io::BufWriter<std::fs::File>),
    #[cfg(target_arch = "wasm32")]
    Web,
}

/// The second's frames.
#[derive(Default)]
struct Second {
    start: f64,
    frames: Vec<f32>,
    main_sum: f64,
    main_max: f32,
    render_sum: f64,
    render_max: f32,
    slow: u32,
    ticks: u32,
}

/// A race being recorded.
struct RaceRec {
    id: u32,
    /// The state's tick when last seen.
    tick: u32,
    /// The first tick of the inputs not yet written.
    from: u32,
    inputs: Vec<InputFrame>,
    events: Vec<(u32, String)>,
    mode: Mode,
    ended: bool,
}

/// What was last written, to write changes only.
#[derive(Default)]
struct Seen {
    status: &'static str,
    ready: bool,
    mode: String,
    settings: Option<crate::ui::store::Settings>,
    pads: String,
    audio: String,
    window: String,
    overlay: Option<bool>,
    adapter: bool,
}

#[derive(Resource)]
pub struct Recorder {
    out: Out,
    path: String,
    line: String,
    sec: Second,
    seen: Seen,
    race: Option<RaceRec>,
    races: u32,
    frames: u64,
    slow: u64,
    lines: u64,
    bytes: u64,
    flushed_at: f64,
    polled_at: f64,
    ended: bool,
}

impl Recorder {
    /// Writes `{"t":…,"type":"kind",…}`: `rest` is the line's other fields,
    /// as a JSON object (its braces dropped) or `None`.
    fn write(&mut self, t: f64, kind: &str, rest: Option<Value>) {
        let mut l = std::mem::take(&mut self.line);
        l.clear();
        let _ = write!(l, "{{\"t\":{t:.4},\"type\":\"{kind}\"");
        if let Some(Value::Object(m)) = rest
            && !m.is_empty()
        {
            let s = Value::Object(m).to_string();
            l.push(',');
            l.push_str(&s[1..]);
        } else {
            l.push('}');
        }
        self.raw(&l);
        self.line = l;
    }

    /// One whole line, as given.
    fn raw(&mut self, l: &str) {
        self.lines += 1;
        self.bytes += l.len() as u64 + 1;
        match &mut self.out {
            #[cfg(not(target_arch = "wasm32"))]
            Out::File(f) => {
                use std::io::Write;
                let _ = f.write_all(l.as_bytes());
                let _ = f.write_all(b"\n");
            }
            #[cfg(target_arch = "wasm32")]
            Out::Web => {
                let mut w = WEB.lock().unwrap_or_else(|e| e.into_inner());
                w.push_str(l);
                w.push('\n');
            }
        }
    }

    fn flush(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::io::Write;
            let Out::File(f) = &mut self.out;
            let _ = f.flush();
        }
    }

    /// The `end` line, once.
    fn end(&mut self, why: &str) {
        if self.ended {
            return;
        }
        self.ended = true;
        let t = now();
        self.write(
            t,
            "end",
            Some(json!({
                "why": why,
                "seconds": round3(t),
                "frames": self.frames,
                "slow_frames": self.slow,
                "races": self.races,
                "lines": self.lines + 1,
            })),
        );
        self.flush();
        ON.store(false, Ordering::Relaxed);
        let msg = format!(
            "recording: wrote {} ({} lines, {:.0} KB)",
            self.path,
            self.lines,
            self.bytes as f64 / 1024.0
        );
        info!("{msg}");
        #[cfg(not(target_arch = "wasm32"))]
        println!("{msg}");
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.end("dropped");
    }
}

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn r2(x: f32) -> f64 {
    (f64::from(x) * 100.0).round() / 100.0
}

/// A date and time in UTC from the system clock: (`20261005-221500`,
/// `2026-10-05T22:15:00Z`). Days to a civil date as in H. Hinnant's
/// `civil_from_days`.
#[cfg(not(target_arch = "wasm32"))]
fn utc_now() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (hh, mm, ss) = (rem / 3600, rem / 60 % 60, rem % 60);
    (
        format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}"),
        format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z"),
    )
}

/// Where a native recording goes: `record=1` → `recordings/` under the
/// repository root; a path ending in `.jsonl` is the file; any other path
/// is a directory for it.
#[cfg(not(target_arch = "wasm32"))]
fn native_path(want: &str, level: &str, stamp: &str) -> std::path::PathBuf {
    let name = format!("{stamp}Z-{level}.jsonl");
    match want {
        "1" | "true" => crate::native::repo_root().join("recordings").join(name),
        p if p.ends_with(".jsonl") => std::path::PathBuf::from(p),
        dir => std::path::Path::new(dir).join(name),
    }
}

pub fn plugin(app: &mut App) {
    let o = app.world().resource::<Opts>().o.clone();
    let Some(want) = wanted(&o) else { return };
    let t0 = now();
    #[cfg(not(target_arch = "wasm32"))]
    let (out, path, date) = {
        let (stamp, date) = utc_now();
        let path = native_path(&want, &o.level, &stamp);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::File::create(&path) {
            Ok(f) => (
                Out::File(std::io::BufWriter::with_capacity(64 * 1024, f)),
                path.display().to_string(),
                date,
            ),
            Err(e) => {
                eprintln!("recording: {}: {e}; not recording", path.display());
                return;
            }
        }
    };
    #[cfg(target_arch = "wasm32")]
    let (out, path, date) = {
        let _ = &want;
        WEB.lock().unwrap_or_else(|e| e.into_inner()).clear();
        (Out::Web, "__mp.recording()".to_string(), String::new())
    };
    ON.store(true, Ordering::Relaxed);
    let mut rec = Recorder {
        out,
        path: path.clone(),
        line: String::with_capacity(4096),
        sec: Second {
            start: t0,
            frames: Vec::with_capacity(512),
            ..default()
        },
        seen: Seen::default(),
        race: None,
        races: 0,
        frames: 0,
        slow: 0,
        lines: 0,
        bytes: 0,
        flushed_at: t0,
        polled_at: f64::NEG_INFINITY,
        ended: false,
    };
    let query: Vec<Value> = o.query.iter().map(|(k, v)| json!([k, v])).collect();
    #[cfg(not(target_arch = "wasm32"))]
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(target_arch = "wasm32")]
    let args: Vec<String> = Vec::new();
    // The checkout's commit, for a replay on the same code (natively).
    #[cfg(not(target_arch = "wasm32"))]
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(crate::native::repo_root())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned());
    #[cfg(target_arch = "wasm32")]
    let commit: Option<String> = None;
    rec.write(
        t0,
        "start",
        Some(json!({
            "version": VERSION,
            "banner": crate::banner(),
            "build": if cfg!(debug_assertions) { "debug" } else { "release" },
            "platform": if cfg!(target_arch = "wasm32") {
                if cfg!(mp_webgl2) { "web-webgl2" } else { "web-webgpu" }
            } else {
                std::env::consts::OS
            },
            "date_utc": date,
            "commit": commit,
            "file": path,
            "args": args,
            "query": query,
            "level": o.level,
            "hq": o.hq,
            "race_on": o.race_on(),
            "tick_hz": 120,
            "chunk": CHUNK,
            "slow_ms": SLOW_MS,
        })),
    );
    rec.flush();
    let msg = format!("recording to {path}");
    info!("{msg}");
    #[cfg(not(target_arch = "wasm32"))]
    println!("{msg}");
    app.insert_resource(rec)
        .add_systems(Update, race.after(crate::play::PlayFrame))
        .add_systems(Last, frame.after(crate::debug_overlay::frame_end));
}

/// The race's side, once a frame after its ticks.
fn race(mut rec: ResMut<Recorder>, play: Option<Res<Play>>, opts: Res<Opts>) {
    let rec = &mut *rec;
    let t = now();
    let race = play.as_ref().and_then(|p| p.race.as_ref());
    // The race went (to the menu) or another took its place.
    let same = matches!((&rec.race, race), (Some(r), Some(n)) if r.id == n.starts);
    if !same && let Some(mut r) = rec.race.take() {
        let tick = r.tick;
        // The state the last inputs led to is gone: no hash.
        chunk(rec, &mut r, t, None);
        rec.write(t, "race_stop", Some(json!({"race": r.id, "tick": tick})));
    }
    let (Some(play), Some(race)) = (play.as_ref(), race) else {
        return;
    };
    let st = &race.session.curr;
    if rec.race.is_none() {
        rec.races += 1;
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
        rec.race = Some(RaceRec {
            id: race.starts,
            tick: 0,
            from: 1,
            inputs: Vec::with_capacity(CHUNK + 8),
            events: Vec::new(),
            mode: Mode::Race,
            ended: false,
        });
    }
    let Some(mut r) = rec.race.take() else { return };
    if st.tick > r.tick {
        let n = (st.tick - r.tick) as usize;
        let got = &race.session.inputs;
        if got.len() != n {
            rec.write(
                t,
                "gap",
                Some(json!({"race": r.id, "ticks": n, "inputs": got.len(), "tick": st.tick})),
            );
        }
        r.inputs.extend_from_slice(got);
        rec.sec.ticks += n as u32;
        for e in &race.log {
            r.events.push((st.tick, format!("{e:?}")));
        }
        r.tick = st.tick;
    }
    if r.inputs.len() >= CHUNK {
        chunk(rec, &mut r, t, Some(race));
    }
    if race.mode != r.mode {
        chunk(rec, &mut r, t, Some(race));
        let name = match race.mode {
            Mode::Race => "race",
            Mode::Paused => "paused",
            Mode::Results => "results",
        };
        rec.write(
            t,
            "race_mode",
            Some(json!({"race": r.id, "mode": name, "tick": st.tick})),
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
                "race": r.id,
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
    rec.race = Some(r);
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
fn chunk(rec: &mut Recorder, r: &mut RaceRec, t: f64, race: Option<&crate::play::flow::Race>) {
    if let Some(l) = ticks_line(r, t, race) {
        rec.raw(&l);
    }
}

/// The inputs not yet written as a `ticks` line, with the state's hash
/// after the last of them (when the race is at hand) and the player's car;
/// `None` when there are none.
fn ticks_line(r: &mut RaceRec, t: f64, race: Option<&crate::play::flow::Race>) -> Option<String> {
    if r.inputs.is_empty() {
        return None;
    }
    let mut l = String::with_capacity(r.inputs.len() * 12 + 512);
    let to = r.from + r.inputs.len() as u32 - 1;
    let _ = write!(
        l,
        "{{\"t\":{t:.4},\"type\":\"ticks\",\"race\":{},\"from\":{},\"tick\":{to}",
        r.id, r.from
    );
    if let Some(race) = race.filter(|x| x.session.curr.tick == to) {
        let st = &race.session.curr;
        let _ = write!(l, ",\"hash\":\"{:016x}\"", mp_sim::race::hash(st));
        l.push_str(",\"in\":\"");
        mp_sim::replay::encode(&r.inputs, &mut l);
        let p = &st.players[0];
        let (v, ph) = (&p.v, &p.phys);
        let _ = write!(
            l,
            "\",\"race_state\":\"{:?}\",\"race_time\":{:.3},\"p\":{{\"s\":{:.2},\"lat\":{:.2},\
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
    } else {
        l.push_str(",\"in\":\"");
        mp_sim::replay::encode(&r.inputs, &mut l);
        l.push('"');
    }
    if !r.events.is_empty() {
        l.push_str(",\"ev\":");
        let ev: Vec<Value> = r.events.iter().map(|(k, e)| json!([k, e])).collect();
        l.push_str(&Value::Array(ev).to_string());
    }
    l.push('}');
    r.from = to + 1;
    r.inputs.clear();
    r.events.clear();
    Some(l)
}

/// What the rest of the client is doing, read once a frame.
#[derive(bevy::ecs::system::SystemParam)]
struct World<'w, 's> {
    status: Res<'w, Status>,
    ft: Res<'w, FrameTimes>,
    play: Option<Res<'w, Play>>,
    ui: Option<Res<'w, crate::ui::UiState>>,
    overlay: Option<Res<'w, Overlay>>,
    adapter: Option<Res<'w, RenderAdapterInfo>>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    pads: Option<Res<'w, crate::play::gamepad_io::PadsRes>>,
    audio: Option<NonSend<'w, crate::play::audio::Shared>>,
}

/// The frame's timing, every second's summary, and the changes.
fn frame(
    mut rec: ResMut<Recorder>,
    w: World,
    mut focus: MessageReader<WindowFocused>,
    mut pads: MessageReader<GamepadConnectionEvent>,
    mut exit: MessageReader<AppExit>,
) {
    let rec = &mut *rec;
    if rec.ended {
        return;
    }
    let t = now();
    let st = &*w.status;
    let ft = *w.ft;
    rec.frames += 1;
    // The first frames' intervals are the start-up's.
    if st.frames > 2 {
        let s = &mut rec.sec;
        s.frames.push(ft.frame_ms);
        s.main_sum += f64::from(ft.main_ms);
        s.main_max = s.main_max.max(ft.main_ms);
        s.render_sum += f64::from(ft.render_ms);
        s.render_max = s.render_max.max(ft.render_ms);
        // `ms` is the interval that ended as this frame began; `main_ms`
        // this frame's own work, which shows in the next frame's interval.
        if ft.frame_ms > SLOW_MS || ft.main_ms > SLOW_MS {
            s.slow += 1;
            rec.slow += 1;
            let mode = mode(&w);
            rec.write(
                t,
                "slow_frame",
                Some(json!({
                    "frame": st.frames,
                    "ms": r2(ft.frame_ms),
                    "main_ms": r2(ft.main_ms),
                    "render_ms": r2(ft.render_ms),
                    "mode": mode,
                    "pipelines_waiting": st.pipelines_waiting,
                })),
            );
        }
    }
    if t - rec.sec.start >= 1.0 {
        let mut s = std::mem::take(&mut rec.sec);
        let n = s.frames.len();
        if n > 0 {
            let sum: f64 = s.frames.iter().map(|x| f64::from(*x)).sum();
            let min = s.frames.iter().copied().fold(f32::INFINITY, f32::min);
            let max = s.frames.iter().copied().fold(0.0, f32::max);
            let k = ((n as f64 * 0.99).ceil() as usize).clamp(1, n) - 1;
            let p99 = *s.frames.select_nth_unstable_by(k, f32::total_cmp).1;
            let mode = mode(&w);
            rec.write(
                t,
                "frames",
                Some(json!({
                    "n": n,
                    "fps": round3(n as f64 / (t - s.start)),
                    "mean": round3(sum / n as f64),
                    "min": r2(min),
                    "p99": r2(p99),
                    "max": r2(max),
                    "main_mean": round3(s.main_sum / n as f64),
                    "main_max": r2(s.main_max),
                    "render_mean": round3(s.render_sum / n as f64),
                    "render_max": r2(s.render_max),
                    "slow": s.slow,
                    "ticks": s.ticks,
                    "mode": mode,
                    "pipelines_waiting": st.pipelines_waiting,
                    "late_frames": st.late_frames,
                })),
            );
        }
        s.frames.clear();
        rec.sec = Second {
            start: t,
            frames: s.frames,
            ..default()
        };
    }
    // The loading status.
    if st.state != rec.seen.status || st.ready != rec.seen.ready {
        rec.seen.status = st.state;
        rec.seen.ready = st.ready;
        rec.write(
            t,
            "status",
            Some(json!({
                "state": st.state,
                "ready": st.ready,
                "frame": st.frames,
                "scenes": st.scenes,
                "error": st.error,
                "counts": st.counts.as_ref().map(|c| json!({
                    "entities": c.entities, "meshes": c.meshes,
                    "materials": c.materials, "images": c.images,
                })),
            })),
        );
    }
    let m = mode(&w);
    if m != rec.seen.mode {
        rec.write(t, "mode", Some(json!({"mode": m, "frame": st.frames})));
        rec.seen.mode = m;
    }
    if let Some(ui) = &w.ui
        && rec.seen.settings.as_ref() != Some(&ui.settings)
    {
        let s = &ui.settings;
        rec.write(
            t,
            "settings",
            Some(json!({
                "music": s.music, "sfx": s.sfx, "mph": s.mph, "hq": s.hq,
                "autogas": s.autogas, "steering": s.steering, "tilt_sens": s.tilt_sens,
                "pedals": s.pedals, "fullscreen": s.fullscreen, "car": s.car,
                "level": s.level, "track": s.track, "flash": s.flash, "rumble": s.rumble,
            })),
        );
        rec.seen.settings = Some(s.clone());
    }
    if !rec.seen.adapter
        && let Some(a) = &w.adapter
    {
        rec.seen.adapter = true;
        rec.write(
            t,
            "adapter",
            Some(json!({
                "name": a.name,
                "backend": format!("{:?}", a.backend),
                "device_type": format!("{:?}", a.device_type),
                "driver": a.driver,
                "driver_info": a.driver_info,
            })),
        );
    }
    for f in focus.read() {
        rec.write(t, "focus", Some(json!({"focused": f.focused})));
    }
    for p in pads.read() {
        rec.write(
            t,
            "gamepad",
            Some(json!({"gamepad": format!("{:?}", p.gamepad), "connection": format!("{:?}", p.connection)})),
        );
    }
    if let Some(ov) = &w.overlay
        && rec.seen.overlay != Some(ov.on)
    {
        rec.seen.overlay = Some(ov.on);
        rec.write(t, "overlay", Some(json!({"on": ov.on})));
    }
    // The log's lines since the last frame.
    let logs = std::mem::take(&mut *LOG.lock().unwrap_or_else(|e| e.into_inner()));
    for (lt, kind, target, msg) in logs {
        rec.write(
            lt,
            "log",
            Some(json!({"level": kind, "target": target, "msg": msg})),
        );
    }
    // Four times a second: the window, the pads and the sound, by what
    // they say about themselves.
    if t - rec.polled_at >= 0.25 {
        rec.polled_at = t;
        if let Ok(win) = w.windows.single() {
            let sig = format!(
                "{}x{} {:.2} {:?} {}",
                win.physical_width(),
                win.physical_height(),
                win.scale_factor(),
                win.mode,
                win.focused
            );
            if sig != rec.seen.window {
                rec.write(
                    t,
                    "window",
                    Some(json!({
                        "width": win.physical_width(),
                        "height": win.physical_height(),
                        "scale": win.scale_factor(),
                        "mode": format!("{:?}", win.mode),
                        "focused": win.focused,
                    })),
                );
                rec.seen.window = sig;
            }
        }
        if let Some(p) = &w.pads {
            let pads = &p.pads;
            let sig = format!(
                "{} {:?}",
                pads.state.connected,
                pads.active.as_ref().map(|a| (&a.id, a.index, &a.mapping))
            );
            if sig != rec.seen.pads {
                rec.write(
                    t,
                    "pads",
                    Some(json!({
                        "connected": pads.state.connected,
                        "active": pads.active.as_ref().map(|a| json!({
                            "id": a.id, "index": a.index, "mapping": a.mapping,
                        })),
                    })),
                );
                rec.seen.pads = sig;
            }
        }
        if let Some(a) = &w.audio
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
            if sig != rec.seen.audio {
                rec.write(t, "audio", Some(v));
                rec.seen.audio = sig;
            }
        }
    }
    if exit.read().next().is_some() {
        rec.end("exit");
        return;
    }
    if t - rec.flushed_at >= 1.0 {
        rec.flushed_at = t;
        rec.flush();
    }
}

/// The client's mode as `window.__game.mode` names it, and the screen.
fn mode(w: &World) -> String {
    match (&w.ui, &w.play) {
        (Some(ui), Some(play)) => format!(
            "{}/{}",
            crate::ui::mode_name(ui, play, &w.status),
            ui.screen.name()
        ),
        _ => w.status.state.to_string(),
    }
}

/// The recording so far (the page's `__mp.recording()`).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn recording_text() -> String {
    WEB.lock().unwrap_or_else(|e| e.into_inner()).clone()
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
        let mut r = RaceRec {
            id: race.starts,
            tick: 0,
            from: 1,
            inputs: Vec::new(),
            events: Vec::new(),
            mode: Mode::Race,
            ended: false,
        };
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
            if st.tick > r.tick {
                assert_eq!(race.session.inputs.len() as u32, st.tick - r.tick);
                r.inputs.extend_from_slice(&race.session.inputs);
                r.tick = st.tick;
            }
            if r.inputs.len() >= CHUNK {
                text.push_str(&ticks_line(&mut r, 0.0, Some(&race)).unwrap());
                text.push('\n');
            }
        }
        text.push_str(&ticks_line(&mut r, 0.0, Some(&race)).unwrap());
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

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_date_is_a_civil_date() {
        let (stamp, date) = utc_now();
        assert_eq!(stamp.len(), 15);
        assert!(date.ends_with('Z') && date.starts_with("20"), "{date}");
    }
}
