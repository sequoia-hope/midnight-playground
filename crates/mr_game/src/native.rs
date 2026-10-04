//! Native glue: the scene file from disk, the window title as the status
//! line, and the `--screenshot` / `--smoke-test` modes (SPEC 8.5).

use crate::options::{Options, usage};
use crate::status::Status;
use crate::{Opts, inbox};
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use std::path::{Path, PathBuf};

/// The repository root: the working directory if it is one, else where
/// this crate was built from.
pub fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("crates/mr_game").is_dir() {
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
    let dir = mr_scene::cache::scenes_dir(root)
        .map_err(|e| format!("hashing the JS tree at {}: {e}", root.display()))?;
    Ok(dir.join(format!("{}.mrscene", o.level)))
}

/// Reads and parses the scene (and Seaside's survey) on a thread.
fn start_loading(opts: Res<Opts>) {
    let o = opts.o.clone();
    if o.materials.is_some() {
        return; // the material test scenes load their own sources
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
    std::thread::spawn(move || {
        let root = repo_root();
        if o.level == "seaside" {
            let p = root.join("assets/seaside/survey.bin");
            inbox().survey = Some(std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())));
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
            let scene = mr_scene::read(&bytes).map_err(|e| format!("{}: {e}", p.display()))?;
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

/// The window title carries the status line natively.
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

/// `--screenshot` and `--smoke-test`: once the scene has drawn `after`
/// frames, capture the window (or just report) and exit.
fn finish(
    mut commands: Commands,
    opts: Res<Opts>,
    status: Res<Status>,
    mut done: Local<bool>,
    mut exit: MessageWriter<AppExit>,
) {
    if *done {
        return;
    }
    if status.state == "failed" && (opts.o.smoke_test || opts.o.screenshot.is_some()) {
        *done = true;
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
        println!("smoke test: ok, {:?}", status.counts);
        exit.write(AppExit::Success);
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, start_loading)
        .add_systems(Update, (title, finish, reload_scene));
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
