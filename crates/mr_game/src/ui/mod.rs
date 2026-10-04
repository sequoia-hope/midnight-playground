//! The front end's screens (roadmap M6, WP 6.1 and 6.2; SPEC 8.1): the
//! loading screen, the main menu, pause, results and the controller setup,
//! as `index.html`, `hud.css` and `main.js` have them, in Bevy UI (D431).
//!
//! - [`store`]: the settings and records under the JS's `localStorage` keys.
//! - [`widgets`]: tokens, breakpoints, the font and the controls.
//! - `menu`: the main menu; `screens`: loading, pause, results, controller.
//!
//! The session flow is `main.js`'s: the menu over the attract camera; Race
//! loads the chosen level if it is not the one built (`loadLevel`), builds
//! the field and holds the countdown until its pipelines are compiled
//! (`startRace`'s `compileAsync`), then drives; pause, results, Restart,
//! Race again and Main menu as there. A level tab only selects: the menu's
//! attract camera shows that level's section (`crate::preview`, D676,
//! D740 on), and Race builds or downloads the level whole. Opening the page with `?level=` still races straight
//! away (DECISIONS D432); the menu is the default with no level given
//! (D570).

pub mod store;
pub mod widgets;

mod menu;
pub(crate) mod nav;
pub(crate) mod pad_setup;
mod screens;
#[cfg(target_arch = "wasm32")]
mod web;

use crate::options::Options;
use crate::play::flow::{Mode, fmt_time, ordinal};
use crate::play::{Play, PlayFrame};
use crate::status::Status;
use crate::{CameraState, Opts, TrackRes};
use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::ui::{ComputedNode, ScrollPosition, UiGlobalTransform};
use bevy::window::{CursorMoved, PrimaryWindow};
use mr_sim::race::{RaceStateKind, cruise_results, pursuit_stats};
use store::{Settings, Store};
use widgets::{Bp, Control, Icons, SliderTrack, Value};

/// Which screen shows (`screenNow`; `None` while driving).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Loading,
    Menu,
    Pause,
    Results,
    PadSetup,
    None,
}

impl Screen {
    pub fn name(self) -> &'static str {
        match self {
            Screen::Loading => "loading",
            Screen::Menu => "menu",
            Screen::Pause => "pause",
            Screen::Results => "results",
            Screen::PadSetup => "padsetup",
            Screen::None => "none",
        }
    }
}

/// The checkboxes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    Mph,
    Hq,
    Flash,
    Autogas,
    Fullscreen,
    Rumble,
}

/// The range sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sl {
    Music,
    Sfx,
    TiltSens,
}

/// The selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sel {
    Track,
    Steer,
    Pedals,
}

/// What activating a control does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Level(&'static str),
    Car(&'static str),
    Mode(&'static str),
    Start,
    Toggle(Opt),
    Slide(Sl),
    Open(Sel),
    Choose(Sel, String),
    CloseDropdown,
    MusicLink,
    PadSetup,
    Resume,
    EndRun,
    Restart,
    Quit,
    NextTrack,
    Again,
    Menu,
    PadBind(&'static str),
    PadDefaults,
    PadDone,
}

/// Frames between freeing the menu's views and asking for the level: the
/// despawned assets are released over the next frames (D751).
const FREE_FRAMES: u32 = 4;

/// A Race tap in progress (`startRace`): waiting for the level to load,
/// then for the field's pipelines (at most three seconds, as the JS's
/// `compileAsync` race).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Starting {
    /// The menu's views are being freed; the level is asked for once they
    /// are (frames waited so far, D751).
    Free(u32),
    Level,
    Build {
        frames: u32,
        quiet: u32,
        t: f64,
    },
}

/// One line of the results table.
#[derive(Clone, Debug, PartialEq)]
pub struct ResRow {
    pub place: String,
    pub name: String,
    pub color: Option<u32>,
    pub value: String,
    pub me: bool,
}

/// One stat tile under the results (`.res-stat`): value, label, and stars
/// drawn after the label (★ on the best lap) or as the value (top heat).
#[derive(Clone, Debug, PartialEq)]
pub struct ResTile {
    pub value: String,
    pub label: String,
    pub label_star: bool,
    pub stars: usize,
}

/// What `showResults` put on the screen.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ResultsView {
    pub title: String,
    pub rows: Vec<ResRow>,
    pub tiles: Vec<ResTile>,
    pub best: String,
}

/// The front end's state.
#[derive(Resource)]
pub struct UiState {
    pub settings: Settings,
    /// Started without `?level=`: the menu comes first, and the race built
    /// behind the loading screen is only a warm-up for the cars' pipelines.
    pub menu_first: bool,
    preview: bool,
    pub screen: Screen,
    pad_return: Screen,
    starting: Option<Starting>,
    /// Rebuild the screen's nodes.
    dirty: bool,
    bp: Option<Bp>,
    built: Option<Screen>,
    scroll: f32,
    /// The control with the keyboard focus, by id; the focus order of the
    /// last build.
    focus: Option<String>,
    focus_order: Vec<String>,
    dropdown: Option<(Sel, (f32, f32, f32))>,
    pub results: Option<ResultsView>,
    /// A pointer down: where, on what, and whether it turned into a scroll
    /// or a slider drag.
    press: Option<Press>,
    /// Races started (the test bridge's count).
    pub races: u32,
    /// `__mr.reveal(id)`: scroll that control to the middle.
    pub reveal: Option<String>,
    /// For the sound (`sync_audio`): the menu buttons' clicks, a level
    /// tab's music, Next track, and `toMenu`.
    clicks: Vec<&'static str>,
    audio_level: Option<String>,
    next_track: bool,
    to_menu: bool,
    /// Gamepads connected (`body.pad`): none until WP 6.4 reads them.
    pub pads: bool,
    /// The page's safe-area inset at the top, CSS px.
    pub inset_top: f32,
}

#[derive(Clone, Debug)]
struct Press {
    id: u64,
    at: Vec2,
    last: Vec2,
    target: Option<(Entity, String, Option<Act>)>,
    scrolling: bool,
    slider: Option<(Sl, f32, f32)>,
}

/// The root of the shown screen's nodes.
#[derive(Component)]
struct UiRoot;

/// The screen's scrolling node.
#[derive(Component)]
struct Scroller;

/// Whether this run opens on the menu (`?level=` races at once, D432; so do
/// `autostart`, `race=1` and the native `shots=`), for a level that races.
pub fn menu_first(o: &Options) -> bool {
    o.race_on()
        && o.param("level").is_none()
        && o.param("autostart").is_none()
        && o.param("race").is_none()
        && o.param("shots").is_none()
}

/// Before the app is built: a menu-first run builds the saved level
/// (`settings.level`) behind the menu, as `loadLevel(settings.level)` does
/// at boot; and the high-quality setting is the saved one unless `?hq=`
/// says. Returns `hq`.
pub fn prepare(o: &mut Options, touch: bool) -> bool {
    let store = Store::platform();
    let settings = Settings::load(&store, touch);
    let level_given = o.param("level").is_some();
    if !level_given && (menu_first(o) || o.param("autostart").is_some()) {
        o.level = settings.level.clone();
    }
    o.hq.unwrap_or(settings.hq)
}

pub fn plugin(app: &mut App) {
    if app.world().get_resource::<Play>().is_none() {
        return;
    }
    let o = app.world().resource::<Opts>().o.clone();
    // As the page decided it (play's own copy is set at Startup).
    let touch = touch_ui(&o);
    let store = Store::platform();
    let settings = Settings::load(&store, touch);
    let first = menu_first(&o);
    {
        let mut play = app.world_mut().resource_mut::<Play>();
        // The car and the mode a race gets unless the address says
        // (`car=`/`autostart=`, `pursuit=`): the saved ones (D570).
        if o.param("car").is_none()
            && let Some(k) = store::car_kinds().find(|k| *k == settings.car)
            && o.param("autostart")
                .is_none_or(|a| !store::car_kinds().any(|k| k == a))
        {
            play.params.car = k;
        }
        if o.param("pursuit").is_none() {
            let level = mr_levels::level_by_id(&o.level);
            play.params.pursuit = store::mode_for(&store, &level) == "pursuit";
        }
        // Over the menu's views no field is built at boot (D751): Race
        // builds the level first, then the cars and the sound's graph.
        if crate::preview::wanted(&o) {
            play.armed = false;
        }
    }
    app.insert_resource(store)
        .insert_resource(UiState {
            settings,
            menu_first: first,
            preview: first,
            screen: Screen::Loading,
            pad_return: Screen::Menu,
            starting: None,
            dirty: true,
            bp: None,
            built: None,
            scroll: 0.0,
            focus: None,
            focus_order: Vec::new(),
            dropdown: None,
            results: None,
            press: None,
            races: 0,
            reveal: None,
            clicks: Vec::new(),
            audio_level: None,
            next_track: false,
            to_menu: false,
            pads: false,
            inset_top: 0.0,
        })
        .init_resource::<nav::MenuNav>()
        .init_resource::<pad_setup::PadSetup>()
        .add_systems(Startup, (widgets::load_fonts, widgets::make_icons))
        .add_systems(
            Update,
            (
                pointer,
                keys,
                sync_audio,
                flow,
                nav::update,
                build,
                after_layout,
            )
                .chain()
                .before(PlayFrame),
        );
    crate::play::gamepad_io::pad_setup_frame(app, pad_setup::frame);
    #[cfg(not(target_arch = "wasm32"))]
    if o.param("uiscript").is_some() {
        app.add_systems(Update, ui_script.before(pointer));
    }
    #[cfg(target_arch = "wasm32")]
    web::plugin(app);
}

/// The touch UI as the page decided it (`__mr.touch`), or `?touch=`.
pub fn touch_ui(o: &Options) -> bool {
    if let Some(t) = o.param("touch") {
        return t == "1";
    }
    #[cfg(target_arch = "wasm32")]
    {
        web::page_touch()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

/// The level the page downloads first (`start_level()` on the web).
pub fn set_start_level(level: &str) {
    #[cfg(target_arch = "wasm32")]
    web::set_start_level(level);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = level;
}

/// Asks the platform for another level's scene (`loadLevel`): the page
/// downloads it (`__mr.reload`), natively a thread reads it.
fn request_level(level: &str) {
    #[cfg(target_arch = "wasm32")]
    web::reload(level);
    #[cfg(not(target_arch = "wasm32"))]
    crate::native::reload(level);
}

/// The session's mode as `window.__game.mode` reads it.
pub fn mode_name(ui: &UiState, play: &Play, status: &Status) -> &'static str {
    if !status.ready || matches!(ui.starting, Some(Starting::Level | Starting::Free(_))) {
        return "loading";
    }
    match (&play.race, ui.preview, ui.starting) {
        (Some(r), false, None) => match r.mode {
            Mode::Race => "race",
            Mode::Paused => "paused",
            Mode::Results => "results",
        },
        _ => "menu",
    }
}

/// The session flow, once a frame: the warm-up race let go, a Race tap
/// carried through, the screen that shows, and the results kept.
#[allow(clippy::too_many_arguments)]
fn flow(
    mut ui: ResMut<UiState>,
    mut play: ResMut<Play>,
    mut store: ResMut<Store>,
    status: Res<Status>,
    opts: Res<Opts>,
    time: Res<Time>,
    tr: Res<TrackRes>,
    mut cs: ResMut<CameraState>,
    previews: Res<crate::preview::Previews>,
) {
    let ui = &mut *ui;
    // The race built behind the loading screen of a menu-first run warmed
    // the cars' pipelines; the menu shows the attract camera.
    if ui.preview && status.ready && play.race.is_some() {
        ui.preview = false;
        play.stop = true;
        play.armed = false;
        to_attract(&mut cs, &tr);
    }
    // With the menu's views no field is built behind the menu (D751): the
    // race's cars come up after the level, as a race from the address.
    if ui.preview && status.ready && (tr.none || crate::preview::active()) {
        ui.preview = false;
    }
    // Race: the views freed, then the level, then the field and its
    // pipelines (D751).
    match ui.starting {
        Some(Starting::Free(n)) => {
            if n >= FREE_FRAMES {
                request_level(&ui.settings.level.clone());
                ui.starting = Some(Starting::Level);
            } else {
                ui.starting = Some(Starting::Free(n + 1));
            }
        }
        Some(Starting::Level) => {
            // The level drawn whole, not its menu section (D743).
            if status.ready
                && opts.o.level == ui.settings.level
                && previews.has_full(&ui.settings.level)
                && !ui.preview
            {
                arm(ui, &mut play, &store, &opts.o);
                ui.starting = Some(Starting::Build {
                    frames: 0,
                    quiet: 0,
                    t: time.elapsed_secs_f64(),
                });
            }
        }
        Some(Starting::Build { frames, quiet, t }) if play.race.is_some() => {
            let quiet = if status.pipelines_waiting == 0 {
                quiet + 1
            } else {
                0
            };
            let waited = time.elapsed_secs_f64() - t;
            if (frames >= 3 && quiet >= 3) || waited > 3.0 {
                play.hold = false;
                ui.starting = None;
                ui.races += 1;
            } else {
                ui.starting = Some(Starting::Build {
                    frames: frames + 1,
                    quiet,
                    t,
                });
            }
        }
        _ => {}
    }
    // The screen.
    let racing = play.race.is_some() && !ui.preview && ui.starting.is_none();
    let mut screen =
        if !status.ready || matches!(ui.starting, Some(Starting::Level | Starting::Free(_))) {
            Screen::Loading
        } else if racing {
            match play.race.as_ref().map(|r| r.mode) {
                Some(Mode::Paused) => Screen::Pause,
                Some(Mode::Results) => Screen::Results,
                _ => Screen::None,
            }
        } else {
            Screen::Menu
        };
    // The Controller screen sits over the menu or the pause screen it was
    // opened from; Esc (which the race takes as un-pause) leaves it and
    // keeps the pause.
    if ui.screen == Screen::PadSetup {
        if screen == ui.pad_return {
            screen = Screen::PadSetup;
        } else if ui.pad_return == Screen::Pause
            && screen == Screen::None
            && let Some(r) = play.race.as_mut()
        {
            r.pause(true);
            screen = Screen::Pause;
        }
    }
    if screen == Screen::Results && ui.screen != Screen::Results {
        let id = opts.o.level.clone();
        if let Some(r) = play.race.as_ref() {
            ui.results = Some(show_results(&mut store, &ui.settings, &id, r));
        }
    }
    if screen != ui.screen {
        if screen == Screen::Menu {
            ui.scroll = 0.0;
        }
        ui.screen = screen;
        ui.dropdown = None;
        ui.focus = None;
        ui.dirty = true;
    }
}

/// The menus and the race's sound (`play::audio`) share the volumes and the
/// track choice (the same store keys): a change on the menus reaches the
/// sound (`applyVolume`, `pickMusic`), and one the sound makes (M toggles
/// the music) reaches the menus. Then the queued clicks, a level tab's
/// music, Next track and `toMenu`.
fn sync_audio(
    mut ui: ResMut<UiState>,
    shared: Option<NonSend<crate::play::audio::Shared>>,
    mut last: Local<Option<(f64, f64, String)>>,
) {
    let Some(shared) = shared else { return };
    let Ok(mut a) = shared.0.try_borrow_mut() else {
        return;
    };
    let ours = (
        ui.settings.music,
        ui.settings.sfx,
        ui.settings.track.clone(),
    );
    let theirs = (a.settings.music, a.settings.sfx, a.settings.track.clone());
    if last.as_ref().is_some_and(|l| *l != theirs) {
        ui.settings.music = theirs.0;
        ui.settings.sfx = theirs.1;
        ui.settings.track = theirs.2.clone();
        ui.dirty = true;
    } else if last.as_ref() != Some(&ours) || theirs != ours {
        let level = ui.settings.level.clone();
        a.menu_settings(
            crate::play::audio::Settings {
                music: ours.0,
                sfx: ours.1,
                track: ours.2.clone(),
            },
            &level,
        );
    }
    *last = Some((a.settings.music, a.settings.sfx, a.settings.track.clone()));
    if std::mem::take(&mut ui.to_menu) {
        a.to_menu();
    }
    if let Some(l) = ui.audio_level.take() {
        a.menu_level(&l);
    }
    if std::mem::take(&mut ui.next_track) {
        a.next_track();
    }
    for c in std::mem::take(&mut ui.clicks) {
        if a.audio.ready() {
            a.ui_click(c);
        }
    }
}

/// `attract.s = world.track.startS + 60`.
fn to_attract(cs: &mut CameraState, tr: &TrackRes) {
    if let Some(t) = &tr.track {
        cs.attract.s = t.start_s + 60.0;
    }
}

/// `startRace`'s setup of the field: the chosen car, the level's mode,
/// `?heat=`; the countdown held until the cars' pipelines are compiled.
fn arm(ui: &mut UiState, play: &mut Play, store: &Store, o: &Options) {
    let s = &ui.settings;
    if let Some(k) = store::car_kinds().find(|k| *k == s.car) {
        play.params.car = k;
    }
    let level = mr_levels::level_by_id(&s.level);
    // `?pursuit=1` / `0` forces it.
    let forced = o.param("pursuit").map(|v| v == "1");
    play.params.pursuit =
        level.police.is_some() && forced.unwrap_or(store::mode_for(store, &level) == "pursuit");
    play.stop = true;
    play.armed = true;
    play.hold = true;
}

/// `showResults(res)`: the title, the table, the stat tiles and the best
/// line, saving a new best time, score or lap record.
fn show_results(
    store: &mut Store,
    settings: &Settings,
    id: &str,
    race: &crate::play::flow::Race,
) -> ResultsView {
    let st = &race.session.curr;
    let mut v = ResultsView::default();
    if st.race.cruise {
        let res = cruise_results(st);
        let key = format!("bestScore.{id}");
        let prev = store.num(&key, 0.0);
        let is_best = res.score > prev;
        if is_best {
            store.set_num(&key, res.score);
        }
        v.title = if is_best { "New best!" } else { "Run over" }.into();
        let km = res.dist / 1000.0;
        let rows = [
            ("Score", widgets::locale_int(res.score)),
            (
                "Distance",
                if settings.mph {
                    format!("{} mi", widgets::to_fixed(km / 1.60934, 2))
                } else {
                    format!("{} km", widgets::to_fixed(km, 2))
                },
            ),
            (
                "Top speed",
                if settings.mph {
                    format!("{} mph", mr_math::js::round(res.top * 2.23694))
                } else {
                    format!("{} km/h", mr_math::js::round(res.top * 3.6))
                },
            ),
            ("Near misses", res.near_misses.to_string()),
            ("Time", fmt_time(Some(res.time))),
        ];
        v.rows = rows
            .into_iter()
            .map(|(a, b)| ResRow {
                place: String::new(),
                name: a.into(),
                color: None,
                value: b,
                me: false,
            })
            .collect();
        v.best = format!("Best score: {}", widgets::locale_int(prev.max(res.score)));
        return v;
    }
    let rows = race.results.clone().unwrap_or_default();
    let Some(me) = rows.iter().find(|r| r.player).cloned() else {
        return v;
    };
    v.title = if me.place == 1 {
        "You win!".into()
    } else {
        format!("{} place", ordinal(me.place))
    };
    v.rows = rows
        .iter()
        .map(|r| ResRow {
            place: r.place.to_string(),
            name: r.name.into(),
            color: Some(r.color),
            value: format!(
                "{}{}",
                if r.estimated { "~" } else { "" },
                fmt_time(Some(r.time))
            ),
            me: r.player,
        })
        .collect();
    let pursuit = pursuit_stats(st);
    let key = store::best_key(id, pursuit.is_some());
    let best = store.num_or_null(&key);
    if me.place == 1 && best.is_none_or(|b| b == 0.0 || me.time < b) {
        store.set_num(&key, me.time);
    }
    v.best = match store.num_or_null(&key) {
        Some(b) if b != 0.0 => format!("Best winning time: {}", fmt_time(Some(b))),
        _ => "Win the race to set a best time".into(),
    };
    // Hot Pursuit: what the police cost you (and what you cost them).
    // Circuits: each lap's time, and the best lap you've ever done here.
    if let Some(p) = pursuit {
        v.tiles = vec![
            tile(p.busts.to_string(), "Busted"),
            tile(p.wrecks.to_string(), "Wrecked"),
            tile(p.takedowns.to_string(), "Takedowns"),
            tile(format!("+{} s", widgets::to_fixed(p.penalty, 1)), "Penalty"),
            ResTile {
                value: String::new(),
                label: "Top heat".into(),
                label_star: false,
                stars: p.heat.max(0) as usize,
            },
        ];
    } else if st.race.laps > 0 {
        let times = &st.players[0].rules.lap_times;
        if !times.is_empty() {
            let best_lap = times.iter().cloned().fold(f64::INFINITY, f64::min);
            let lkey = format!("bestLap.{id}");
            let prev = store.num_or_null(&lkey);
            if prev.is_none_or(|p| best_lap < p) {
                store.set_num(&lkey, best_lap);
            }
            for (i, t) in times.iter().enumerate() {
                v.tiles.push(ResTile {
                    value: fmt_time(Some(*t)),
                    label: format!("Lap {}", i + 1),
                    label_star: *t == best_lap,
                    stars: 0,
                });
            }
            v.tiles.push(tile(
                fmt_time(store.num_or_null(&lkey)),
                if prev.is_none_or(|p| best_lap < p) {
                    "New lap record"
                } else {
                    "Lap record"
                },
            ));
        }
    }
    v
}

fn tile(value: String, label: &str) -> ResTile {
    ResTile {
        value,
        label: label.into(),
        label_star: false,
        stars: 0,
    }
}

/// A control's box in CSS px: `(left, top, width, height)`.
fn css_rect(node: &ComputedNode, t: &UiGlobalTransform, css: f32) -> (f32, f32, f32, f32) {
    let k = node.inverse_scale_factor * css;
    let c = t.affine().translation * k;
    let s = node.size * k;
    (c.x - s.x / 2.0, c.y - s.y / 2.0, s.x, s.y)
}

fn inside(r: (f32, f32, f32, f32), p: Vec2) -> bool {
    p.x >= r.0 && p.x <= r.0 + r.2 && p.y >= r.1 && p.y <= r.1 + r.3
}

type ControlQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static Control,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
        Option<&'static GlobalZIndex>,
    ),
>;

/// The control under a point (CSS px): the dropdown's first, then the
/// smallest box (the innermost control).
fn hit(controls: &ControlQuery, css: f32, p: Vec2) -> Option<(Entity, String, Option<Act>)> {
    let mut best: Option<(i32, f32, Entity, &Control)> = None;
    for (e, c, n, t, vis, z) in controls.iter() {
        if !vis.get() || c.act.is_none() {
            continue;
        }
        let r = css_rect(n, t, css);
        if !inside(r, p) {
            continue;
        }
        let z = z.map_or(0, |z| z.0);
        let area = r.2 * r.3;
        let better = match &best {
            None => true,
            Some((bz, ba, _, _)) => z > *bz || (z == *bz && area < *ba),
        };
        if better {
            best = Some((z, area, e, c));
        }
    }
    best.map(|(_, _, e, c)| (e, c.id.clone(), c.act.clone()))
}

/// How far a finger moves before a press becomes a scroll (CSS px).
const SLOP: f32 = 10.0;

/// Taps and clicks on the controls, drags on the sliders, scrolling by
/// finger and by wheel.
#[allow(clippy::too_many_arguments)]
fn pointer(
    mut ui: ResMut<UiState>,
    mut touches: MessageReader<TouchInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut cursor: MessageReader<CursorMoved>,
    mut wheel: MessageReader<MouseWheel>,
    mut last_cursor: Local<Option<Vec2>>,
    controls: ControlQuery,
    sliders: Query<(&SliderTrack, &ComputedNode, &UiGlobalTransform)>,
    mut scroller: Query<&mut ScrollPosition, With<Scroller>>,
    mut ctx: ActCtx,
) {
    let play_scale = ctx.play.touch_scale;
    let css = ctx.play.css_scale.max(0.01);
    // Pointer positions in CSS px.
    let to_css = |p: Vec2| p * play_scale;
    let mut events: Vec<(u64, TouchPhase, Vec2)> = Vec::new();
    for t in touches.read() {
        events.push((t.id, t.phase, to_css(t.position)));
    }
    for c in cursor.read() {
        *last_cursor = Some(to_css(c.position));
        if ui.press.as_ref().is_some_and(|p| p.id == MOUSE) {
            events.push((MOUSE, TouchPhase::Moved, to_css(c.position)));
        }
    }
    for b in buttons.read() {
        if b.button != MouseButton::Left {
            continue;
        }
        let Some(p) = *last_cursor else { continue };
        let phase = match b.state {
            ButtonState::Pressed => TouchPhase::Started,
            ButtonState::Released => TouchPhase::Ended,
        };
        events.push((MOUSE, phase, p));
    }
    for w in wheel.read() {
        let dy = match w.unit {
            MouseScrollUnit::Line => w.y * 40.0,
            MouseScrollUnit::Pixel => w.y,
        };
        if let Ok(mut s) = scroller.single_mut() {
            s.0.y = (s.0.y - dy / css).max(0.0);
        }
    }
    if ui.screen == Screen::None || ui.screen == Screen::Loading {
        ui.press = None;
        return;
    }
    for (id, phase, p) in events {
        match phase {
            TouchPhase::Started => {
                if ui.press.is_some() {
                    continue; // one pointer at a time on the menus
                }
                let target = hit(&controls, css, p);
                let slider = target.as_ref().and_then(|(e, _, act)| match act {
                    Some(Act::Slide(which)) => sliders.get(*e).ok().map(|(_, n, t)| {
                        let r = css_rect(n, t, css);
                        (*which, r.0, r.2)
                    }),
                    _ => None,
                });
                ui.press = Some(Press {
                    id,
                    at: p,
                    last: p,
                    target,
                    scrolling: false,
                    slider,
                });
                if let Some((which, x, w)) = slider {
                    slide(&mut ui, &mut ctx, which, x, w, p.x);
                }
            }
            TouchPhase::Moved => {
                let Some(mut pr) = ui.press.clone() else {
                    continue;
                };
                if pr.id != id {
                    continue;
                }
                if let Some((which, x, w)) = pr.slider {
                    slide(&mut ui, &mut ctx, which, x, w, p.x);
                } else {
                    if !pr.scrolling && (p - pr.at).length() > SLOP && id != MOUSE {
                        pr.scrolling = true;
                    }
                    if pr.scrolling
                        && let Ok(mut s) = scroller.single_mut()
                    {
                        s.0.y = (s.0.y - (p.y - pr.last.y) / css).max(0.0);
                    }
                }
                pr.last = p;
                ui.press = Some(pr);
            }
            TouchPhase::Ended => {
                let Some(pr) = ui.press.take() else { continue };
                if pr.id != id || pr.scrolling || pr.slider.is_some() {
                    continue;
                }
                // A tap on no control of the pause screen resumes, as the
                // M4 pause card did (the owner's phone flow; D578).
                if pr.target.is_none() && ui.screen == Screen::Pause {
                    if hit(&controls, css, p).is_none() {
                        activate(&mut ui, &mut ctx, &controls, Act::Resume);
                    }
                    continue;
                }
                // A tap: released on the control it pressed.
                let Some((_, cid, Some(act))) = pr.target else {
                    continue;
                };
                if hit(&controls, css, p).is_some_and(|(_, id2, _)| id2 == cid) {
                    activate(&mut ui, &mut ctx, &controls, act);
                }
            }
            TouchPhase::Canceled => {
                if ui.press.as_ref().is_some_and(|p| p.id == id) {
                    ui.press = None;
                }
            }
        }
    }
}

const MOUSE: u64 = u64::MAX - 1;

/// What the actions change.
#[derive(bevy::ecs::system::SystemParam)]
struct ActCtx<'w, 's> {
    play: ResMut<'w, Play>,
    store: ResMut<'w, Store>,
    opts: ResMut<'w, Opts>,
    cs: ResMut<'w, CameraState>,
    tr: Res<'w, TrackRes>,
    status: Res<'w, Status>,
    previews: ResMut<'w, crate::preview::Previews>,
    /// The gamepads and the Controller screen (WP 6.4).
    pads: ResMut<'w, crate::play::gamepad_io::PadsRes>,
    pad_setup: ResMut<'w, pad_setup::PadSetup>,
    windows: Query<'w, 's, &'static mut Window, With<PrimaryWindow>>,
}

/// A slider follows the pointer: `el.value` from where it is along the
/// track, then `oninput`.
fn slide(ui: &mut UiState, ctx: &mut ActCtx, which: Sl, x: f32, w: f32, px: f32) {
    let thumb = 16.0;
    let v = (((px - x - thumb / 2.0) / (w - thumb).max(1.0)).clamp(0.0, 1.0) * 100.0).round();
    let v = f64::from(v) / 100.0;
    let s = &mut ui.settings;
    let (slot, key) = match which {
        Sl::Music => (&mut s.music, "musicVol"),
        Sl::Sfx => (&mut s.sfx, "sfxVol"),
        Sl::TiltSens => (&mut s.tilt_sens, "tiltSens"),
    };
    if *slot != v {
        *slot = v;
        ctx.store.set_num(key, v);
        ui.dirty = true;
    }
}

/// A control activated by a tap, a click or Enter.
fn activate(ui: &mut UiState, ctx: &mut ActCtx, controls: &ControlQuery, act: Act) {
    let busy = ui.starting.is_some();
    ui.dirty = true;
    // The menus' buttons click (`main.js`'s document click handler; Race,
    // Race again and Restart make the audio's "start" as the race starts).
    if matches!(
        act,
        Act::Level(_)
            | Act::Car(_)
            | Act::Mode(_)
            | Act::Resume
            | Act::EndRun
            | Act::Quit
            | Act::Menu
            | Act::PadSetup
            | Act::PadDone
            | Act::PadDefaults
            | Act::PadBind(_)
            | Act::NextTrack
    ) {
        ui.clicks.push("click");
    }
    match act {
        Act::Level(id) => {
            // `if (mode !== 'menu') return;`
            if ui.screen != Screen::Menu || busy {
                return;
            }
            ui.settings.level = id.into();
            ctx.store.set_str("level", id);
            ui.audio_level = Some(id.into());
            // The JS builds the new level behind the loading screen here;
            // the menu shows its section instead, at once if it is up, and
            // Race builds the level (D676, D743).
            ctx.previews.select(id);
        }
        Act::Car(k) => {
            ui.settings.car = k.into();
            ctx.store.set_str("car", k);
        }
        Act::Mode(m) => {
            let level = mr_levels::level_by_id(&ui.settings.level);
            if level.police.is_none() {
                return;
            }
            ctx.store.set_str(&format!("mode.{}", level.id), m);
        }
        Act::Start => {
            // A double tap starts one race.
            if busy || ui.screen != Screen::Menu {
                return;
            }
            // The menu's sections go; the level is built or downloaded
            // whole behind the loading screen unless it is drawn whole
            // already (D743).
            let level = ui.settings.level.clone();
            ctx.previews.free_sections();
            ui.starting = Some(if ctx.previews.has_full(&level) {
                Starting::Level
            } else {
                // The views' memory goes before the level's build takes
                // its own (D751).
                Starting::Free(0)
            });
        }
        Act::Toggle(o) => {
            let s = &mut ui.settings;
            let (slot, key) = match o {
                Opt::Mph => (&mut s.mph, "mph"),
                Opt::Hq => (&mut s.hq, "hq"),
                Opt::Flash => (&mut s.flash, "flash"),
                Opt::Autogas => (&mut s.autogas, "autogas"),
                Opt::Fullscreen => (&mut s.fullscreen, "fullscreen"),
                Opt::Rumble => (&mut s.rumble, "rumble"),
            };
            *slot = !*slot;
            let v = *slot;
            ctx.store.set_bool(key, v);
            if o == Opt::Hq {
                // `applyQuality`: shadows, and the pixel ratio on the web.
                ctx.opts.hq = v;
                #[cfg(target_arch = "wasm32")]
                if let Ok(mut w) = ctx.windows.single_mut() {
                    web::apply_pixel_ratio(&mut w, v);
                }
            }
        }
        Act::Slide(_) => {}
        Act::Open(sel) => {
            let id = sel_id(sel);
            let at = controls
                .iter()
                .find(|(_, c, ..)| c.id == id)
                .map(|(_, _, n, t, ..)| {
                    let r = css_rect(n, t, ctx.play.css_scale.max(0.01));
                    (r.0, r.1 + r.3, r.2)
                })
                .unwrap_or((0.0, 0.0, 120.0));
            ui.dropdown = Some((sel, at));
        }
        Act::Choose(sel, v) => {
            ui.dropdown = None;
            match sel {
                Sel::Track => {
                    ui.settings.track = v.clone();
                    ctx.store.set_str("track", &v);
                }
                Sel::Steer => {
                    ui.settings.steering = v.clone();
                    ctx.store.set_str("steering", &v);
                }
                Sel::Pedals => {
                    ui.settings.pedals = v.clone();
                    ctx.store.set_str("pedals", &v);
                }
            }
        }
        Act::CloseDropdown => ui.dropdown = None,
        Act::MusicLink => {
            #[cfg(target_arch = "wasm32")]
            web::open_music_player();
        }
        Act::PadSetup => {
            if ui.screen == Screen::Menu || ui.screen == Screen::Pause {
                ui.pad_return = ui.screen;
                ui.screen = Screen::PadSetup;
                // An Esc left over from the menu would close it at once.
                if let Some(r) = ctx.play.race.as_mut() {
                    r.input.consume("pause");
                }
                ctx.pad_setup.show(&mut ctx.pads.pads);
            }
        }
        Act::PadDone => {
            if ctx.pad_setup.open {
                ctx.pad_setup.close(&mut ctx.pads.pads, ui);
            } else {
                ui.screen = ui.pad_return;
            }
        }
        Act::NextTrack => ui.next_track = true,
        Act::PadBind(id) => {
            if let Some(a) = crate::play::gamepad::Action::from_key(id) {
                ctx.pad_setup.pick(&mut ctx.pads.pads, a);
            }
        }
        Act::PadDefaults => ctx.pad_setup.defaults(&mut ctx.pads.pads),
        Act::Resume => {
            if let Some(r) = ctx.play.race.as_mut() {
                r.pause(false);
            }
        }
        Act::EndRun => {
            if let Some(r) = ctx.play.race.as_mut()
                && r.session.curr.race.cruise
            {
                r.mode = Mode::Results;
            }
        }
        Act::Restart | Act::Again => {
            let seed = ctx.play.params.seed;
            if let Some(r) = ctx.play.race.as_mut() {
                let mut o = r.setup.opts;
                o.seed = seed.unwrap_or_else(crate::play::clock_seed);
                r.restart(o);
                ui.races += 1;
            }
        }
        Act::Quit | Act::Menu => {
            // `toMenu`.
            ctx.play.stop = true;
            ctx.play.armed = false;
            to_attract(&mut ctx.cs, &ctx.tr);
            ui.screen = Screen::Menu;
            ui.scroll = 0.0;
            ui.to_menu = true;
            // The level stays drawn for its tab; the sections are prepared
            // again behind the menu (D743).
            let level = ctx.opts.o.level.clone();
            ctx.previews.back_to_menu(&level);
        }
    }
    let _ = &ctx.status;
}

pub fn sel_id(sel: Sel) -> &'static str {
    match sel {
        Sel::Track => "opt-track",
        Sel::Steer => "opt-steer",
        Sel::Pedals => "opt-pedals",
    }
}

/// Natively, `--query uiscript=lvl-tab-seaside,lvl-tab-sierra,btn-start`:
/// activates those controls in turn, each once the client is ready on the
/// menu (a level tab's build done), logs each step, and exits when the
/// race after the last step is racing. A smoke test of the menu flow
/// where there is no page to drive it (the web's is `__mr`).
#[cfg(not(target_arch = "wasm32"))]
fn ui_script(
    mut ui: ResMut<UiState>,
    status: Res<Status>,
    controls: ControlQuery,
    mut ctx: ActCtx,
    mut step: Local<usize>,
    mut wait: Local<u32>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    let steps: Vec<String> = ctx
        .opts
        .o
        .param("uiscript")
        .unwrap_or("")
        .split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    if *step >= steps.len() {
        if ctx
            .play
            .race
            .as_ref()
            .is_some_and(|r| r.state() == RaceStateKind::Racing)
            && ui.starting.is_none()
        {
            info!("uiscript: racing on {}; done", ctx.opts.o.level);
            exit.write(bevy::app::AppExit::Success);
        }
        return;
    }
    if !status.ready || ui.screen != Screen::Menu || ui.starting.is_some() {
        *wait = 0;
        return;
    }
    // A few frames on the menu first, so its layout is there.
    *wait += 1;
    if *wait < 10 {
        return;
    }
    *wait = 0;
    let id = &steps[*step];
    let act = controls
        .iter()
        .find(|(_, c, ..)| &c.id == id)
        .and_then(|(_, c, ..)| c.act.clone());
    let Some(act) = act else {
        error!("uiscript: no control {id}");
        exit.write(bevy::app::AppExit::error());
        return;
    };
    info!(
        "uiscript: {id} (level {}, scenes {})",
        ctx.opts.o.level, status.scenes
    );
    activate(&mut ui, &mut ctx, &controls, act);
    *step += 1;
}

/// The keyboard on the menus: Tab and Shift+Tab move the focus through the
/// controls, Enter or Space activates the focused one (the browser's
/// behaviour with the DOM's buttons).
fn keys(
    mut ui: ResMut<UiState>,
    mut keys: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    controls: ControlQuery,
    mut ctx: ActCtx,
) {
    let mut moves: Vec<KeyCode> = Vec::new();
    for k in keys.read() {
        if k.state == ButtonState::Pressed && !k.repeat {
            moves.push(k.key_code);
        }
    }
    if matches!(ui.screen, Screen::None | Screen::Loading) {
        return;
    }
    for code in moves {
        match code {
            KeyCode::Tab => {
                let order = &ui.focus_order;
                if order.is_empty() {
                    continue;
                }
                let back = held.pressed(KeyCode::ShiftLeft) || held.pressed(KeyCode::ShiftRight);
                let i = ui
                    .focus
                    .as_ref()
                    .and_then(|f| order.iter().position(|o| o == f));
                let n = order.len();
                let next = match (i, back) {
                    (None, false) => 0,
                    (None, true) => n - 1,
                    (Some(i), false) => (i + 1) % n,
                    (Some(i), true) => (i + n - 1) % n,
                };
                ui.focus = Some(order[next].clone());
                ui.dirty = true;
            }
            // Esc on the Controller screen is `pad_setup::frame`'s (it
            // stops listening, or leaves).
            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Space => {
                let Some(f) = ui.focus.clone() else { continue };
                let act = controls
                    .iter()
                    .find(|(_, c, ..)| c.id == f)
                    .and_then(|(_, c, ..)| c.act.clone());
                if let Some(a) = act {
                    activate(&mut ui, &mut ctx, &controls, a);
                }
            }
            _ => {}
        }
    }
}

/// Rebuilds the shown screen when it, the viewport or a setting changed.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    mut ui: ResMut<UiState>,
    play: Res<Play>,
    store: Res<Store>,
    opts: Res<Opts>,
    status: Res<Status>,
    icons: Option<Res<Icons>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    pad_setup: Res<pad_setup::PadSetup>,
    roots: Query<Entity, With<UiRoot>>,
    scroller: Query<&ScrollPosition, With<Scroller>>,
    mut loading: Local<(f32, String)>,
) {
    let Some(icons) = icons else { return };
    let Ok(w) = windows.single() else { return };
    let css = play.css_scale.max(0.01);
    let bp = Bp::new(
        w.width() * css,
        w.height() * css,
        1.0 / css,
        play.touch_ui,
        ui.inset_top,
    );
    if ui.bp != Some(bp) {
        ui.bp = Some(bp);
        ui.dirty = true;
    }
    if let Ok(s) = scroller.single() {
        ui.scroll = s.0.y;
    }
    // The loading screen follows the build's progress.
    if ui.screen == Screen::Loading {
        let label = screens::loading_label(&status);
        if (loading.0 - status.progress).abs() > 0.01 || loading.1 != label {
            *loading = (status.progress, label);
            ui.dirty = true;
        }
    }
    if !ui.dirty && ui.built == Some(ui.screen) {
        return;
    }
    ui.dirty = false;
    ui.built = Some(ui.screen);
    for e in &roots {
        commands.entity(e).despawn();
    }
    // `menu=0`: the screens are not drawn (pictures of what is behind).
    if ui.screen == Screen::None || opts.o.param("menu") == Some("0") {
        ui.focus_order.clear();
        return;
    }
    let mut cx = screens::Cx {
        bp,
        icons: &icons,
        focus: ui.focus.clone(),
        order: Vec::new(),
    };
    let root = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                ..default()
            },
            GlobalZIndex(10),
            UiRoot,
        ))
        .id();
    let screen = ui.screen;
    let scroll = ui.scroll;
    let mut screen_node = None;
    commands.entity(root).with_children(|p| {
        screen_node = Some(match screen {
            Screen::Loading => screens::loading(p, &mut cx, &status),
            Screen::Menu => menu::menu(p, &mut cx, &ui, &store, &play, &opts),
            Screen::Pause => screens::pause(p, &mut cx, &ui, &play),
            Screen::Results => screens::results(p, &mut cx, &ui),
            Screen::PadSetup => screens::padsetup(p, &mut cx, &ui, &pad_setup.view),
            Screen::None => return,
        });
        if screen == Screen::Menu && bp.touch && bp.portrait {
            menu::rotate_hint(p, &cx.bp);
        }
    });
    if let Some(e) = screen_node {
        commands
            .entity(e)
            .insert((Scroller, ScrollPosition(Vec2::new(0.0, scroll))));
    }
    if let Some((sel, at)) = ui.dropdown {
        let (opts_list, current) = menu::select_options(sel, &ui.settings);
        let dd = widgets::dropdown(&mut commands, &bp, at, &opts_list, &current, move |v| {
            Act::Choose(sel, v.to_owned())
        });
        commands.entity(root).add_child(dd);
    }
    ui.focus_order = cx.order;
}

/// After the frame's layout is known: a pending `reveal` scrolls its
/// control to the middle (`scrollIntoView({block: 'center'})`).
fn after_layout(
    mut ui: ResMut<UiState>,
    play: Res<Play>,
    controls: ControlQuery,
    mut scroller: Query<(&mut ScrollPosition, &ComputedNode), With<Scroller>>,
) {
    let css = play.css_scale.max(0.01);
    let Ok((mut s, n)) = scroller.single_mut() else {
        ui.reveal = None;
        return;
    };
    // A screen rebuilt this frame has no layout yet: its offset (carried
    // over by `build`) and a pending reveal wait for it.
    if n.size.y <= 0.0 {
        return;
    }
    // Keep the offset inside the content, as a browser does.
    let max = ((n.content_size.y - n.size.y) * n.inverse_scale_factor).max(0.0);
    let mut y = s.0.y.clamp(0.0, max);
    if let Some(id) = ui.reveal.take()
        && let Some((_, _, cn, t, ..)) = controls.iter().find(|(_, c, ..)| c.id == id)
    {
        let r = css_rect(cn, t, css);
        let view_h = n.size.y * n.inverse_scale_factor * css;
        let dy = (r.1 + r.3 / 2.0) - view_h / 2.0;
        y = (y + dy / css).clamp(0.0, max);
    }
    if s.0.y != y {
        s.0.y = y;
    }
}

/// The values the bridge reports for a control (CSS px).
#[allow(clippy::type_complexity)]
pub fn snapshot(
    controls: &ControlQuery,
    css: f32,
) -> Vec<(String, (f32, f32, f32, f32), bool, bool, Value, bool, i32)> {
    let mut out = Vec::new();
    for (_, c, n, t, vis, z) in controls.iter() {
        out.push((
            c.id.clone(),
            css_rect(n, t, css),
            vis.get(),
            c.act.is_some(),
            c.value.clone(),
            c.sel,
            z.map_or(0, |z| z.0),
        ));
    }
    out
}

/// Whether the race is counting down or running (for the tests).
pub fn race_state(play: &Play) -> Option<RaceStateKind> {
    play.race.as_ref().map(|r| r.state())
}
