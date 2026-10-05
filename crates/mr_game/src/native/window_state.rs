//! The player's window natively, as the Electron shell kept it
//! (`desktop/main.cjs`, "Window state"): it opens where it was closed, at
//! that size, maximised or fullscreen as it was, 1600 × 900 the first time
//! and never smaller than 800 × 450; F11 toggles fullscreen, Ctrl+Q (Cmd+Q)
//! quits; the icon is `desktop/icon.png` (DECISIONS D1000).
//!
//! The state is `window-state.json` beside the settings store
//! (`ui::store::FileStore::default_path`), `{x, y, width, height,
//! fullscreen, maximized}` as the JS writes it, saved on close with the
//! last normal bounds when the window is maximised or fullscreen. Runs that
//! make pictures or check the build (`--smoke-test`, `--screenshot`,
//! `shots=`, `--stations`, `--materials`) and any run given `--size` keep
//! their own window and neither read nor write it.

use crate::options::Options;
use bevy::app::AppExit;
use bevy::ecs::system::NonSendMarker;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, PrimaryWindow, WindowCloseRequested, WindowCreated, WindowFocused,
    WindowMode, WindowMoved, WindowPosition, WindowResizeConstraints, WindowResized,
    WindowResolution,
};
use bevy::winit::WINIT_WINDOWS;
use serde_json::Value;
use std::path::PathBuf;

/// `width: st.width || 1600, height: st.height || 900`.
pub const DEFAULT_SIZE: (u32, u32) = (1600, 900);
/// `minWidth: 800, minHeight: 450`.
pub const MIN_SIZE: (f32, f32) = (800.0, 450.0);
/// The Electron window's title (`title: 'Midnight Racer'`).
pub const TITLE: &str = "Midnight Racer";
/// The X11 class and Wayland app id: the desktop entry's `StartupWMClass`
/// (`tools/install-desktop-entry.sh`), so the desktop claims the window
/// with the entry's icon, as `CHROME_DESKTOP` makes it for Electron.
pub const APP_ID: &str = "midnight-racer";

/// What `window-state.json` holds. The bounds are the window's pixels (the
/// client draws at a scale factor of 1 natively): `x`, `y` the outer
/// position (none where the platform does not tell it, as on Wayland),
/// `width`, `height` the inner size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub maximized: bool,
    pub fullscreen: bool,
}

impl WindowState {
    /// `JSON.parse` of the file, `{}` when it is missing or broken
    /// (`loadState`'s `catch`). A zero or missing size is the default
    /// (`st.width || 1600`).
    pub fn parse(text: &str) -> WindowState {
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(text) else {
            return WindowState::default();
        };
        let num = |k: &str| m.get(k).and_then(Value::as_f64).filter(|v| v.is_finite());
        let int = |k: &str| num(k).map(|v| v.round() as i32);
        let size = |k: &str| {
            num(k)
                .filter(|v| *v >= 1.0 && *v < 65536.0)
                .map(|v| v.round() as u32)
        };
        let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
        WindowState {
            x: int("x"),
            y: int("y"),
            width: size("width"),
            height: size("height"),
            maximized: flag("maximized"),
            fullscreen: flag("fullscreen"),
        }
    }

    /// `JSON.stringify({ ...bounds, fullscreen, maximized })`.
    pub fn to_json(&self) -> String {
        let mut m = serde_json::Map::new();
        if let (Some(x), Some(y)) = (self.x, self.y) {
            m.insert("x".into(), x.into());
            m.insert("y".into(), y.into());
        }
        if let (Some(w), Some(h)) = (self.width, self.height) {
            m.insert("width".into(), w.into());
            m.insert("height".into(), h.into());
        }
        m.insert("fullscreen".into(), self.fullscreen.into());
        m.insert("maximized".into(), self.maximized.into());
        Value::Object(m).to_string()
    }
}

/// `window-state.json` in the settings store's directory.
pub fn path() -> Option<PathBuf> {
    let store = crate::ui::store::FileStore::default_path()?;
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

fn load() -> WindowState {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map_or_else(WindowState::default, |t| WindowState::parse(&t))
}

fn save(st: &WindowState) {
    // "not worth failing over"
    let Some(p) = path() else { return };
    let r = p
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&p, st.to_json()));
    match r {
        Ok(()) => info!("window state: {} {}", p.display(), st.to_json()),
        Err(e) => warn!("window state: {}: {e}", p.display()),
    }
}

/// The primary window for these options. Runs that do not keep the state
/// get the window they always had: `--size`, else 512 × 512 for the
/// material scenes and 1280 × 800 for the rest, hidden where a picture is
/// all that is wanted.
pub fn window(o: &Options, title: String) -> Window {
    if !persists(o) {
        // The material test scenes are 512 × 512 (scenes.json).
        let (w, h) = o.size.unwrap_or(if o.materials.is_some() {
            (512, 512)
        } else {
            (1280, 800)
        });
        return Window {
            title,
            name: Some(APP_ID.into()),
            resolution: WindowResolution::new(w, h).with_scale_factor_override(1.0),
            // A screenshot run draws without showing a window, where the
            // platform allows it.
            visible: o.screenshot.is_none()
                && o.param("shots").is_none()
                && o.materials.is_none()
                && o.stations.is_none(),
            ..default()
        };
    }
    let st = load();
    let w = st.width.unwrap_or(DEFAULT_SIZE.0);
    let h = st.height.unwrap_or(DEFAULT_SIZE.1);
    Window {
        // The status line goes to the title with `stats=1` (`native::title`).
        title: TITLE.into(),
        name: Some(APP_ID.into()),
        resolution: WindowResolution::new(w, h).with_scale_factor_override(1.0),
        position: match (st.x, st.y) {
            (Some(x), Some(y)) => WindowPosition::At(IVec2::new(x, y)),
            _ => WindowPosition::Automatic,
        },
        resize_constraints: WindowResizeConstraints {
            min_width: MIN_SIZE.0,
            min_height: MIN_SIZE.1,
            ..default()
        },
        ..default()
    }
}

/// The state being kept: what was loaded (maximised, fullscreen to apply
/// once the window exists), and the last bounds seen while the window was
/// neither (`win._lastBounds`).
#[derive(Resource, Default)]
struct Kept {
    loaded: WindowState,
    last: Option<WindowState>,
    saved: bool,
}

/// The icon, once the window is made (every interactive window).
fn icon(mut created: MessageReader<WindowCreated>, _main_thread: NonSendMarker) {
    if created.read().count() > 0 {
        set_icon();
    }
}

/// The window was made: `if (st.maximized) win.maximize(); if
/// (st.fullscreen) win.setFullScreen(true)`.
fn created(
    mut created: MessageReader<WindowCreated>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor)>,
    mut kept: ResMut<Kept>,
    _main_thread: NonSendMarker,
) {
    if created.read().count() == 0 {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    // The bounds asked for, until the window reports its own.
    let st = kept.loaded;
    kept.last = Some(WindowState {
        width: Some(st.width.unwrap_or(DEFAULT_SIZE.0)),
        height: Some(st.height.unwrap_or(DEFAULT_SIZE.1)),
        maximized: false,
        fullscreen: false,
        ..st
    });
    if st.maximized {
        w.set_maximized(true);
    }
    if st.fullscreen {
        let at = st.x.zip(st.y).map(|(x, y)| IVec2::new(x, y));
        w.mode = WindowMode::BorderlessFullscreen(monitor_at(&monitors, at));
    }
}

/// The monitor a point of the desktop is on, else the one winit guesses
/// (`Current`, from where the window was first placed).
fn monitor_at(monitors: &Query<(Entity, &Monitor)>, p: Option<IVec2>) -> MonitorSelection {
    p.and_then(|p| {
        monitors.iter().find(|(_, m)| {
            let d = p - m.physical_position;
            d.x >= 0
                && d.y >= 0
                && (d.x as u32) < m.physical_width
                && (d.y as u32) < m.physical_height
        })
    })
    .map_or(MonitorSelection::Current, |(e, _)| {
        MonitorSelection::Entity(e)
    })
}

const ICON_SIZE: u32 = 64;

/// `desktop/icon.png` (256 × 256), built in. On Wayland the window takes
/// its icon from the desktop entry its app id names instead.
fn set_icon() {
    static ICON: &[u8] = include_bytes!("../../../../desktop/icon.png");
    let icon = image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
        .map_err(|e| e.to_string())
        .and_then(|img| {
            // 64 × 64: X11 got an empty `_NET_WM_ICON` from the 256 × 256
            // one (a quarter of a megabyte in one property request), and
            // the desktops draw window icons at 48 px or less.
            let img = image::imageops::resize(
                &img.to_rgba8(),
                ICON_SIZE,
                ICON_SIZE,
                image::imageops::FilterType::Triangle,
            );
            let (w, h) = img.dimensions();
            winit::window::Icon::from_rgba(img.into_raw(), w, h).map_err(|e| e.to_string())
        });
    match icon {
        Ok(icon) => WINIT_WINDOWS.with_borrow(|ww| {
            for w in ww.windows.values() {
                w.set_window_icon(Some(icon.clone()));
            }
        }),
        Err(e) => warn!("window icon: {e}"),
    }
}

/// The window as it is now (`getBounds`, `isMaximized`, `isFullScreen`).
fn current(entity: Entity) -> Option<WindowState> {
    WINIT_WINDOWS.with_borrow(|ww| {
        let w = ww.get_window(entity)?;
        let pos = w.outer_position().ok();
        let size = w.inner_size();
        // Where the platform tells the position (X11), a fullscreen window
        // is one over the whole of a monitor (any monitor: `current_monitor`
        // is a cached guess): winit's `fullscreen()` is the last it asked
        // for, and misses the window manager's own changes (its key,
        // `wmctrl`) both ways. Elsewhere (Wayland) it is what the
        // compositor said.
        let covers = w
            .available_monitors()
            .any(|m| m.size() == size && pos == Some(m.position()));
        Some(WindowState {
            x: pos.map(|p| p.x),
            y: pos.map(|p| p.y),
            width: Some(size.width).filter(|v| *v > 0),
            height: Some(size.height).filter(|v| *v > 0),
            maximized: w.is_maximized(),
            fullscreen: if pos.is_some() {
                covers
            } else {
                w.fullscreen().is_some()
            },
        })
    })
}

/// `win.on('resize' | 'move', …)`: the bounds while the window is neither
/// maximised nor fullscreen.
fn track(
    mut resized: MessageReader<WindowResized>,
    mut moved: MessageReader<WindowMoved>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut kept: ResMut<Kept>,
    _main_thread: NonSendMarker,
) {
    let changed = resized.read().count() + moved.read().count() > 0;
    let Ok(e) = windows.single() else { return };
    if !changed {
        return;
    }
    if let Some(now) = current(e)
        && !now.maximized
        && !now.fullscreen
    {
        let last = kept.last.get_or_insert(now);
        // Wayland does not tell the position; keep the one loaded.
        if now.x.is_some() {
            (last.x, last.y) = (now.x, now.y);
        }
        (last.width, last.height) = (now.width, now.height);
    }
}

/// `saveState(win)` once, before the window goes: the last normal bounds
/// if it is maximised or fullscreen, else the bounds it has.
fn save_now(e: Entity, kept: &mut Kept) {
    if kept.saved {
        return;
    }
    let Some(now) = current(e) else { return };
    kept.saved = true;
    let b = if now.maximized || now.fullscreen {
        kept.last.unwrap_or(now)
    } else {
        now
    };
    save(&WindowState {
        x: b.x.or(kept.loaded.x),
        y: b.y.or(kept.loaded.y),
        width: b.width,
        height: b.height,
        maximized: now.maximized,
        fullscreen: now.fullscreen,
    });
}

/// `win.on('close', () => saveState(win))`, before Bevy closes it.
fn on_close(
    mut close: MessageReader<WindowCloseRequested>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut kept: ResMut<Kept>,
    _main_thread: NonSendMarker,
) {
    for c in close.read() {
        if windows.contains(c.window) {
            save_now(c.window, &mut kept);
        }
    }
}

/// Any other way out (Ctrl+Q, the app exiting) saves too, while the window
/// is still there.
fn on_exit(
    mut exit: MessageReader<AppExit>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut kept: ResMut<Kept>,
    _main_thread: NonSendMarker,
) {
    if exit.read().count() > 0
        && let Ok(e) = windows.single()
    {
        save_now(e, &mut kept);
    }
}

/// The shell's keys (`before-input-event`): F11 toggles fullscreen, Ctrl+Q
/// or Cmd+Q quits. Neither is a key the game binds (`play::input::dom_code`
/// has no F11; a Ctrl+Q leaves the game before any binding sees a Q).
#[allow(clippy::too_many_arguments)]
fn keys(
    keys: Res<ButtonInput<KeyCode>>,
    mut events: MessageReader<KeyboardInput>,
    mut focus: MessageReader<WindowFocused>,
    mut modifier: Local<bool>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor)>,
    mut exit: MessageWriter<AppExit>,
    _main_thread: NonSendMarker,
) {
    if keys.just_pressed(KeyCode::F11)
        && let Ok((e, mut w)) = windows.single_mut()
    {
        // `win.setFullScreen(!win.isFullScreen())`, on the monitor the
        // window's middle is on.
        let now = current(e);
        let full = now.map_or(w.mode != WindowMode::Windowed, |n| n.fullscreen);
        let middle = now.and_then(|n| {
            let (x, y) = n.x.zip(n.y)?;
            let (w, h) = n.width.zip(n.height)?;
            Some(IVec2::new(x + w as i32 / 2, y + h as i32 / 2))
        });
        info!("F11: fullscreen {}", if full { "off" } else { "on" });
        let mode = if full {
            WindowMode::Windowed
        } else {
            WindowMode::BorderlessFullscreen(monitor_at(&monitors, middle))
        };
        if (w.mode == WindowMode::Windowed) == (mode == WindowMode::Windowed) {
            // Bevy has it so already (the window manager changed it behind
            // its back): tell winit straight, as Bevy would.
            let target = (!full).then_some(winit::window::Fullscreen::Borderless(None));
            WINIT_WINDOWS.with_borrow(|ww| {
                if let Some(win) = ww.get_window(e) {
                    win.set_fullscreen(target);
                }
            });
        } else {
            w.mode = mode;
        }
    }
    // In the order the keys came: a frame can hold the Ctrl's press and
    // release both (a slow frame, a quick chord).
    for k in events.read() {
        let down = k.state == ButtonState::Pressed;
        match k.key_code {
            KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight => {
                *modifier = down;
            }
            KeyCode::KeyQ if down && *modifier => {
                info!("Ctrl+Q: quit");
                exit.write(AppExit::Success);
            }
            _ => {}
        }
    }
    if focus.read().filter(|f| !f.focused).count() > 0 {
        // Its release goes to the other window.
        *modifier = false;
    }
}

/// The shell's keys for every interactive window; the saved state for the
/// player's window only (`persists`).
pub fn plugin(app: &mut App, o: &Options) {
    if !headless(o) {
        app.add_systems(PreUpdate, (icon, keys));
    }
    if !persists(o) {
        return;
    }
    app.insert_resource(Kept {
        loaded: load(),
        ..default()
    })
    .add_systems(PreUpdate, (created, track, on_close).chain())
    .add_systems(Last, on_exit);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_the_electron_file() {
        // As `saveState` writes it.
        let js =
            r#"{"x":120,"y":64,"width":1700,"height":950,"fullscreen":false,"maximized":true}"#;
        let st = WindowState::parse(js);
        assert_eq!(
            st,
            WindowState {
                x: Some(120),
                y: Some(64),
                width: Some(1700),
                height: Some(950),
                maximized: true,
                fullscreen: false,
            }
        );
        assert_eq!(st.to_json(), js);
    }

    #[test]
    fn missing_broken_and_zero_are_the_defaults() {
        assert_eq!(WindowState::parse(""), WindowState::default());
        assert_eq!(WindowState::parse("[1,2]"), WindowState::default());
        let st = WindowState::parse(r#"{"width":0,"height":"900","fullscreen":1}"#);
        assert_eq!((st.width, st.height, st.fullscreen), (None, None, false));
        // No position (Wayland): none written.
        let st = WindowState {
            width: Some(1600),
            height: Some(900),
            ..default()
        };
        assert_eq!(
            st.to_json(),
            r#"{"width":1600,"height":900,"fullscreen":false,"maximized":false}"#
        );
    }

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
