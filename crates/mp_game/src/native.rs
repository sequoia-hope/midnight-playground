//! Native glue: the scene file from disk, the player's window (Playground
//! Kit's `pk_shell::native`, D1234: saved bounds, F11, Ctrl+Q, the icon;
//! WP 6.8, D1000), a race pausing when the window loses the focus (SPEC
//! 8.4, D1002), the window title as the status line (`stats=1`, and the
//! runs that make pictures), and the `--screenshot` / `--smoke-test` modes
//! (SPEC 8.5, D1003).

use crate::options::{Options, usage};
use crate::play::Play;
use crate::play::flow::Mode;
use crate::status::{LoadState, SceneStatus, Status};
use crate::{Opts, inbox};
use bevy::app::AppExit;
use bevy::prelude::*;
use pk_shell::FocusLost;
use pk_shell::native::{SmokeTest, SmokeTestPlugin, StatusTitle, WindowConfig, WindowStatePlugin};
use std::path::{Path, PathBuf};

pub use pk_shell::native::log_tally;

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
        && status.state == LoadState::Waiting
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

// ── The window ───────────────────────────────────────────────────────────

/// `width: st.width || 1600, height: st.height || 900`.
pub const DEFAULT_SIZE: (u32, u32) = (1600, 900);
/// `minWidth: 800, minHeight: 450`.
pub const MIN_SIZE: (f32, f32) = (800.0, 450.0);

/// The player's window: the game's name and app id (D1100), the icon
/// `desktop/icon.png` (256 × 256, built in), the Electron shell's sizes,
/// and `window-state.json` beside the settings store (D1000).
pub fn window_config() -> WindowConfig {
    static ICON: &[u8] = include_bytes!("../../../desktop/icon.png");
    let info = crate::app_info();
    WindowConfig {
        app_id: info.app_id,
        title: info.name,
        icon_png: Some(ICON),
        default_size: DEFAULT_SIZE,
        min_size: MIN_SIZE,
        state_path: window_state_path(),
    }
}

/// `window-state.json` in the settings store's directory.
pub fn window_state_path() -> Option<PathBuf> {
    let store = pk_shell::store::FileStore::default_path(&crate::ui::store::STORE_CONFIG)?;
    Some(store.parent()?.join("window-state.json"))
}

/// Whether this run is the player's window: one that keeps its state.
pub fn persists(o: &Options) -> bool {
    o.size.is_none() && !headless(o)
}

/// The runs that make pictures or check the build: their window is theirs,
/// and the race does not pause when it loses the focus.
pub fn headless(o: &Options) -> bool {
    o.smoke_test
        || o.screenshot.is_some()
        || o.param("shots").is_some()
        || o.materials.is_some()
        || o.stations.is_some()
}

/// The primary window for these options. Runs that do not keep the state
/// get the window they always had: `--size`, else 512 × 512 for the
/// material scenes and 1280 × 800 for the rest, hidden where a picture is
/// all that is wanted.
pub fn window(o: &Options, title: String) -> Window {
    let cfg = window_config();
    if !persists(o) {
        // The material test scenes are 512 × 512 (scenes.json).
        let size = o.size.unwrap_or(if o.materials.is_some() {
            (512, 512)
        } else {
            (1280, 800)
        });
        // A screenshot run draws without showing a window, where the
        // platform allows it.
        let visible = o.screenshot.is_none()
            && o.param("shots").is_none()
            && o.materials.is_none()
            && o.stations.is_none();
        return cfg.plain(title, size, visible);
    }
    cfg.window()
}

/// The window title carries the status line natively: with `stats=1` (the
/// web build's stats panel, `fps` and the worst frame), and in the runs
/// that are not the player's window (`--size`, the pictures). The player's
/// window is the game's name, as the Electron shell's (DECISIONS D1001, D1100).
pub fn status_title(o: &Options) -> bool {
    !persists(o) || o.param("stats").is_some()
}

/// The status line (`pk_shell::native::StatusTitle`).
fn title_line(world: &World, status: &Status) -> String {
    let level = &world.resource::<Opts>().o.level;
    let s = world.resource::<SceneStatus>().s;
    let fps = if status.frame_ms > 0.0 {
        1000.0 / status.frame_ms
    } else {
        0.0
    };
    match status.state {
        LoadState::Running => format!(
            "{} — {level} — s {s:.0} — {fps:.0} fps (worst {:.0} ms)",
            crate::banner(),
            status.worst_ms
        ),
        LoadState::Failed => format!("{} — failed", crate::banner()),
        st => format!(
            "{} — {level} — {} {:.0} %",
            crate::banner(),
            st.name(),
            status.progress * 100.0
        ),
    }
}

/// The window lost the focus (`pk_shell::FocusLost`): a race in progress
/// pauses, the counterpart of the web build's `visibilitychange`
/// (`play::web`, `if (document.hidden && mode === 'race') pause(true)`),
/// before the frame's ticks. Not in the runs that make pictures or check
/// the build, whose window may never have the focus, nor with
/// `autodrive=1`, where nobody drives and a measurement would stall
/// (DECISIONS D1002).
fn focus_pause(mut lost: MessageReader<FocusLost>, opts: Res<Opts>, play: Option<ResMut<Play>>) {
    let lost = lost.read().count() > 0;
    if !lost || headless(&opts.o) || opts.o.param("autodrive") == Some("1") {
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

/// The smoke test's extra: the scene's counts.
fn smoke_extra(world: &World) -> String {
    format!("{:?}", world.resource::<SceneStatus>().counts)
}

pub fn plugin(app: &mut App) {
    let o = app.world().resource::<Opts>().o.clone();
    app.add_plugins(WindowStatePlugin {
        config: window_config(),
        keys: !headless(&o),
        persist: persists(&o),
    });
    app.add_plugins(SmokeTestPlugin(SmokeTest {
        label: o.level.clone(),
        after: o.after,
        timeout_s: pk_shell::native::SMOKE_TIMEOUT_S,
        screenshot: o.screenshot.clone(),
        smoke: o.smoke_test,
        extra: Some(smoke_extra),
    }));
    if status_title(&o) {
        app.insert_resource(StatusTitle(Box::new(title_line)))
            .add_systems(Update, pk_shell::native::title);
    }
    app.add_systems(Startup, start_loading)
        .add_systems(Update, reload_scene)
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
    let resolved = crate::ui::prepare(&mut o, touch);
    crate::app(o, resolved).run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_players_window_keeps_its_state() {
        let args = |a: &[&str]| {
            Options::from_args(&a.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
        };
        assert!(persists(&args(&[])));
        assert!(persists(&args(&["--query", "level=coast&autostart=super"])));
        for a in [
            &["--smoke-test"][..],
            &["--screenshot", "a.png"],
            &["--size", "1280x800"],
            &["--query", "shots=out"],
            &["--materials", "all"],
            &["--stations", "s.json"],
        ] {
            assert!(!persists(&args(a)), "{a:?}");
        }
        // `--size` alone is still an interactive window.
        assert!(!headless(&args(&["--size", "1280x800"])));
        // The pictures' windows are what they were.
        let w = window(&args(&["--screenshot", "a.png"]), "t".into());
        assert_eq!((w.width(), w.height(), w.visible), (1280.0, 800.0, false));
        let w = window(&args(&["--materials", "all"]), "t".into());
        assert_eq!((w.width(), w.height()), (512.0, 512.0));
        let w = window(&args(&["--size", "390x844"]), "t".into());
        assert_eq!((w.width(), w.height(), w.visible), (390.0, 844.0, true));
    }
}
