//! Native glue: the scene file from disk, the player's window
//! ([`window_state`]: saved bounds, F11, Ctrl+Q, the icon), a race pausing
//! when the window loses the focus (SPEC 8.4), the window title as the
//! status line (`stats=1`, and the runs that make pictures), and the
//! `--screenshot` / `--smoke-test` modes (SPEC 8.5).

pub mod window_state;

use crate::options::{Options, usage};
use crate::play::Play;
use crate::play::flow::Mode;
use crate::status::Status;
use crate::{Opts, inbox};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::window::WindowFocused;
use std::path::{Path, PathBuf};

/// The repository root: the working directory if it is one, else where
/// this crate was built from.
pub fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("crates/mp_game").is_dir() {
        return cwd;
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or(cwd)
}

/// The scene file for these options: `--scene`, or the level's export in
/// the parity cache of the current JS tree.
pub fn scene_path(o: &Options, root: &Path) -> Result<PathBuf, String> {
    if let Some(s) = &o.scene {
        return Ok(PathBuf::from(s));
    }
    let dir = mp_scene::cache::scenes_dir(root)
        .map_err(|e| format!("hashing the JS tree at {}: {e}", root.display()))?;
    Ok(dir.join(format!("{}.mrscene", o.level)))
}

/// Reads and parses the scene (and Seaside's survey) on a thread.
fn start_loading(opts: Res<Opts>) {
    let o = opts.o.clone();
    if o.materials.is_some() {
        return; // the material test scenes load their own sources
    }
    if crate::preview::active() {
        return; // the menu's sections are built in the client (D742)
    }
    load(o);
}

/// A level the menu asked for (`loadLevel`), loaded once the current
/// scene is torn down.
static RELOAD: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Tears the scene down and reads another level's (the menu's level tabs).
pub fn reload(level: &str) {
    inbox().unload = Some(level.to_owned());
    *RELOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(level.to_owned());
}

fn reload_scene(opts: Res<Opts>, status: Res<Status>) {
    let mut r = RELOAD.lock().unwrap_or_else(|e| e.into_inner());
    if r.as_deref() == Some(opts.o.level.as_str())
        && status.state == "waiting"
        && inbox().unload.is_none()
    {
        *r = None;
        load(opts.o.clone());
    }
}

fn load(o: Options) {
    // The scene is the client's own world build (`animate`) unless
    // `?world=export` (D678).
    let generated = crate::animate::draws_generated(&o);
    std::thread::spawn(move || {
        let root = repo_root();
        if o.level == "seaside" {
            let p = root.join("assets/seaside/survey.bin");
            inbox().survey = Some(std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())));
        }
        if generated {
            return; // Seaside's world build needs the survey all the same
        }
        let r = scene_path(&o, &root).and_then(|p| {
            let t0 = std::time::Instant::now();
            let bytes = std::fs::read(&p).map_err(|e| {
                format!(
                    "{}: {e}\n(export the scenes with `node tools/parity/scene-export.mjs`, \
                     or pass --scene <file>)",
                    p.display()
                )
            })?;
            let scene = mp_scene::read(&bytes).map_err(|e| format!("{}: {e}", p.display()))?;
            info!(
                "read {} ({:.0} MB) in {:.2} s",
                p.display(),
                bytes.len() as f64 / 1e6,
                t0.elapsed().as_secs_f64()
            );
            Ok(scene)
        });
        inbox().scene = Some(r);
    });
}

/// The window title carries the status line natively: with `stats=1` (the
/// web build's stats panel, `fps` and the worst frame), and in the runs
/// that are not the player's window (`--size`, the pictures). The player's
/// window is the game's name, as the Electron shell's (DECISIONS D1001, D1100).
pub fn status_title(o: &Options) -> bool {
    !window_state::persists(o) || o.param("stats").is_some()
}

fn title(
    time: Res<Time>,
    mut last: Local<f32>,
    mut status: ResMut<Status>,
    opts: Res<Opts>,
    mut windows: Query<&mut Window>,
) {
    let now = time.elapsed_secs();
    if now - *last < 0.5 {
        return;
    }
    *last = now;
    if !status_title(&opts.o) {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    let fps = if status.frame_ms > 0.0 {
        1000.0 / status.frame_ms
    } else {
        0.0
    };
    w.title = match status.state {
        "running" => format!(
            "{} — {} — s {:.0} — {:.0} fps (worst {:.0} ms)",
            crate::banner(),
            opts.o.level,
            status.s,
            fps,
            status.worst_ms
        ),
        "failed" => format!("{} — failed", crate::banner()),
        s => format!(
            "{} — {} — {s} {:.0} %",
            crate::banner(),
            opts.o.level,
            status.progress * 100.0
        ),
    };
    status.worst_ms = 0.0;
}

/// The window lost the focus: a race in progress pauses, the counterpart
/// of the web build's `visibilitychange` (`play::web::visibility`, `if
/// (document.hidden && mode === 'race') pause(true)`), before the frame's
/// ticks. Not in the runs that make pictures or check the build, whose
/// window may never have the focus, nor with `autodrive=1`, where nobody
/// drives and a measurement would stall (DECISIONS D1002).
fn focus_pause(
    mut focus: MessageReader<WindowFocused>,
    opts: Res<Opts>,
    play: Option<ResMut<Play>>,
) {
    let lost = focus.read().filter(|f| !f.focused).count() > 0;
    if !lost || window_state::headless(&opts.o) || opts.o.param("autodrive") == Some("1") {
        return;
    }
    if let Some(mut play) = play
        && let Some(race) = play.race.as_mut()
        && race.mode == Mode::Race
    {
        info!("window lost the focus: the race pauses");
        race.pause(true);
    }
}

/// What the log said at warning and error level, for `--smoke-test`'s
/// report: the Electron smoke test lists the page's console warnings and
/// errors and fails on an error (`smokeTest`). A tracing layer beside
/// Bevy's, under the same filter (`wgpu=error,naga=warn`, `RUST_LOG`).
pub mod log_tally {
    use bevy::app::App;
    use bevy::log::BoxedLayer;
    use bevy::log::tracing::field::{Field, Visit};
    use bevy::log::tracing::{Event, Level};
    use bevy::log::tracing_subscriber::Layer;
    use bevy::log::tracing_subscriber::layer::Context;
    use bevy::log::tracing_subscriber::registry::Registry;
    use std::sync::Mutex;

    /// Errors, warnings, and the first [`KEEP`] messages of either.
    pub static TALLY: Mutex<(usize, usize, Vec<String>)> = Mutex::new((0, 0, Vec::new()));
    pub const KEEP: usize = 20;

    struct Tally;

    /// The message and its fields; a `log` crate record's own target in
    /// place of `log` (the bridge's `log.target`, its other `log.*` fields
    /// left out).
    #[derive(Default)]
    struct Message(String, Option<String>);

    impl Visit for Message {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "log.target" {
                self.1 = Some(value.to_owned());
            } else {
                self.record_debug(field, &value);
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            use std::fmt::Write;
            match field.name() {
                "message" => {
                    let _ = write!(self.0, "{value:?}");
                }
                n if n.starts_with("log.") => {}
                n => {
                    let _ = write!(self.0, " {n}={value:?}");
                }
            }
        }
    }

    /// Errors that say nothing about the build, counted as warnings (as the
    /// Electron one filters the favicon's 404): the sound's output stream
    /// running dry while a software renderer holds the frame up.
    pub const BENIGN: &[(&str, &str)] = &[("web_audio_api::io", "buffer underrun or overrun")];

    /// Counts and keeps one record.
    pub fn note(level: Level, target: &str, message: &str) {
        let error = level == Level::ERROR
            && !BENIGN
                .iter()
                .any(|(t, m)| target.starts_with(t) && message.contains(m));
        if !error && level > Level::WARN {
            return;
        }
        let mut t = TALLY.lock().unwrap_or_else(|e| e.into_inner());
        if error {
            t.0 += 1;
        } else {
            t.1 += 1;
        }
        if t.2.len() < KEEP {
            let kind = if error { "error" } else { "warn" };
            t.2.push(format!("[{kind}] {target}: {}", message.trim_end()));
        }
    }

    impl Layer<Registry> for Tally {
        fn on_event(&self, event: &Event<'_>, _: Context<'_, Registry>) {
            let meta = event.metadata();
            // The run recording keeps the client's own info lines too
            // (`crate::recording`, D1021).
            let keep = crate::recording::wants_log(*meta.level(), meta.target());
            if *meta.level() > Level::WARN && !keep {
                return;
            }
            let mut m = Message::default();
            event.record(&mut m);
            let target = m.1.as_deref().unwrap_or(meta.target());
            if keep {
                crate::recording::log(*meta.level(), target, &m.0);
            }
            if *meta.level() <= Level::WARN {
                note(*meta.level(), target, &m.0);
            }
        }
    }

    #[cfg(test)]
    #[test]
    fn errors_fail_and_the_benign_ones_do_not() {
        use bevy::log::tracing_subscriber::layer::SubscriberExt;
        let sub = bevy::log::tracing_subscriber::registry().with(Tally);
        bevy::log::tracing::subscriber::with_default(sub, || {
            bevy::log::info!("not counted");
            bevy::log::warn!("a warning");
            bevy::log::error!(target: "web_audio_api::io::cpal", "an error occurred on the output audio stream: A buffer underrun or overrun occurred.");
            bevy::log::error!(n = 3, "a real one");
        });
        let t = std::mem::take(&mut *TALLY.lock().unwrap());
        assert_eq!((t.0, t.1), (1, 2), "{:?}", t.2);
        assert!(
            t.2[2].starts_with("[error] mp_game::native::log_tally: a real one n=3"),
            "{:?}",
            t.2
        );
    }

    /// `LogPlugin::custom_layer`.
    pub fn layer(_: &mut App) -> Option<BoxedLayer> {
        Some(Box::new(Tally))
    }
}

/// `--smoke-test` gives up when the scene is not up in this long (the
/// Electron one waits 60 s for `__ready`; a software renderer in CI
/// compiles the pipelines far slower than a GPU).
const SMOKE_TIMEOUT_S: f64 = 600.0;

/// `--screenshot` and `--smoke-test`: once the scene has drawn `after`
/// frames, capture the window (or just report) and exit. The smoke test
/// prints what it saw (the load time, the adapter, the scene's counts, the
/// log's warnings and errors) and fails on a logged error, a failed load,
/// or no scene after [`SMOKE_TIMEOUT_S`].
#[allow(clippy::too_many_arguments)]
fn finish(
    mut commands: Commands,
    opts: Res<Opts>,
    status: Res<Status>,
    time: Res<Time<Real>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    mut ready_at: Local<Option<f64>>,
    mut done: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    if status.state == "failed" && (opts.o.smoke_test || opts.o.screenshot.is_some()) {
        *done = true;
        if opts.o.smoke_test {
            println!(
                "smoke test: failed: {}",
                status.error.as_deref().unwrap_or("?")
            );
        }
        exit.write(AppExit::error());
        return;
    }
    let now = time.elapsed_secs_f64();
    if status.ready && ready_at.is_none() {
        *ready_at = Some(now);
    }
    if opts.o.smoke_test && !status.ready && now > SMOKE_TIMEOUT_S {
        *done = true;
        println!(
            "smoke test: failed: not ready after {SMOKE_TIMEOUT_S:.0} s ({} {:.0} %)",
            status.state,
            status.progress * 100.0
        );
        exit.write(AppExit::error());
        return;
    }
    if status.ready_frames < u64::from(opts.o.after.max(1)) {
        return;
    }
    if let Some(path) = &opts.o.screenshot {
        *done = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
    } else if opts.o.smoke_test {
        *done = true;
        let (errors, warnings, messages) =
            std::mem::take(&mut *log_tally::TALLY.lock().unwrap_or_else(|e| e.into_inner()));
        let ok = errors == 0;
        println!(
            "smoke test: {}: {}, ready in {:.1} s, {} frames after; adapter {}; \
             {errors} errors, {warnings} warnings; {:?}",
            if ok { "ok" } else { "failed" },
            opts.o.level,
            ready_at.unwrap_or(now),
            status.ready_frames,
            adapter.map_or_else(
                || "?".to_string(),
                |a| format!("{} ({:?}, {:?})", a.name, a.backend, a.device_type)
            ),
            status.counts,
        );
        for m in &messages {
            println!("  {m}");
        }
        exit.write(if ok {
            AppExit::Success
        } else {
            AppExit::error()
        });
    }
}

pub fn plugin(app: &mut App) {
    let o = app.world().resource::<Opts>().o.clone();
    window_state::plugin(app, &o);
    app.add_systems(Startup, start_loading)
        .add_systems(Update, (title, finish, reload_scene))
        .add_systems(Update, focus_pause.before(crate::play::PlayFrame));
}

/// The native entry point: parse the command line, run the app.
pub fn run() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}\n\n{}", crate::banner(), usage());
        return AppExit::Success;
    }
    let o = match Options::from_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return AppExit::error();
        }
    };
    if o.smoke_race {
        return match crate::play::smoke_race_cli(&o) {
            Ok(()) => AppExit::Success,
            Err(e) => {
                eprintln!("smoke race: {e}");
                AppExit::error()
            }
        };
    }
    // The saved level for the menu, the saved High quality (DECISIONS
    // D570).
    let mut o = o;
    let touch = crate::ui::touch_ui(&o);
    let hq = crate::ui::prepare(&mut o, touch);
    crate::app(o, hq).run()
}
