//! Moving around the menus with a gamepad (port of `src/game/MenuNav.js`,
//! WP 6.4; `Pads.state.nav`: the D-pad or left stick, A, B, Start). A
//! highlight goes to the nearest control in the direction pushed; A presses
//! it. Sliders and drop-downs: A to adjust, ◂ ▸ to change, A or B when done
//! (otherwise ◂ ▸ would be stuck on them). B and Start are the screen's
//! (back, start). A screen opens on its first control (`[data-nav-first]`),
//! or else its primary button. The mouse or a finger hides the highlight
//! again.
//!
//! The highlight is the screens' focus ring (`UiState::focus`, which Tab
//! moves too, D573); `.pad-edit` draws it in #ffcf4d ([`EDITING`]). A
//! control's box is its laid-out rectangle in CSS px, as
//! `getBoundingClientRect` gives it (DECISIONS D784).

use super::{Act, ActCtx, ControlQuery, Screen, Sl, UiState, activate, css_rect};
use crate::play::gamepad::Nav;
use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::time::Real;
use bevy::window::CursorMoved;
use std::sync::atomic::{AtomicBool, Ordering};

const DIRS: [Nav; 4] = [Nav::Up, Nav::Down, Nav::Left, Nav::Right];
const REPEAT_DELAY: f64 = 380.0;
const REPEAT_EVERY: f64 = 110.0;
const SLIDER_STEP: f64 = 5.0;

/// The highlighted slider or drop-down is being adjusted (`.pad-edit`):
/// the focus ring is drawn in #ffcf4d.
pub static EDITING: AtomicBool = AtomicBool::new(false);

/// `MenuNav`'s state.
#[derive(Resource, Default)]
pub struct MenuNav {
    prev: [bool; 7],
    next: [f64; 7],
    /// The highlighted control, by id.
    cur: Option<String>,
    root: Option<Screen>,
    /// `body.pad-nav`: the highlight shows.
    pub shown: bool,
    pub editing: bool,
    /// Screen → its last highlighted control.
    memory: Vec<(Screen, String)>,
    /// A new screen: its highlight waits for its controls to be built.
    pending: bool,
    /// This frame's controls and the window's height, CSS px.
    list: List,
    view_h: f32,
    /// The highlight to scroll into view.
    reveal: Option<String>,
}

type List = Vec<(String, Act, (f32, f32, f32, f32))>;

/// The kind of control: a range slider, a select, anything else.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Range(Sl),
    Select(super::Sel),
    Other,
}

fn tag(act: &Act) -> Tag {
    match act {
        Act::Slide(s) => Tag::Range(*s),
        Act::Open(s) => Tag::Select(*s),
        _ => Tag::Other,
    }
}

/// `root()`: the screen to move around, or none while driving or loading.
fn root_of(ui: &UiState) -> Option<Screen> {
    match ui.screen {
        Screen::None | Screen::Loading => None,
        s => Some(s),
    }
}

/// The focusable controls of the screen shown: id, act, box (CSS px).
fn focusables(controls: &ControlQuery, css: f32) -> List {
    let mut v: Vec<_> = controls
        .iter()
        .filter(|(_, c, _, _, vis, _)| vis.get() && c.focus)
        .filter_map(|(_, c, n, t, ..)| match &c.act {
            // The open drop-down's list is not one of the screen's controls.
            Some(Act::Choose(..) | Act::CloseDropdown) | None => None,
            Some(a) => Some((c.id.clone(), a.clone(), css_rect(n, t, css))),
        })
        .collect();
    v.retain(|(_, _, r)| r.2 > 0.0 && r.3 > 0.0);
    v
}

/// `defaultFor(root)`: the screen's `[data-nav-first]` control, else its
/// primary button, else its first control.
fn default_for(screen: Screen, list: &List, order: &[String]) -> Option<String> {
    let want = match screen {
        Screen::PadSetup => "pad-bind-left",
        Screen::Menu => "btn-start",
        Screen::Pause => "btn-resume",
        Screen::Results => "btn-again",
        Screen::Lobby => "mp-go",
        _ => "",
    };
    let has = |id: &str| list.iter().any(|(i, ..)| i == id);
    if has(want) {
        return Some(want.into());
    }
    order
        .iter()
        .find(|id| has(id))
        .cloned()
        .or_else(|| list.first().map(|(i, ..)| i.clone()))
}

impl MenuNav {
    fn set_cur(&mut self, ui: &mut UiState, id: Option<String>) {
        self.cur = id;
        let want = if self.shown { self.cur.clone() } else { None };
        if ui.focus != want {
            debug!("nav: highlight {want:?}");
            ui.focus = want.clone();
            ui.dirty = true;
        }
        // `scrollIntoView({ block: 'nearest' })`, once the screen rebuilt
        // with the highlight is laid out.
        self.reveal = want;
    }

    /// Only a control not wholly in view is scrolled to (to the middle,
    /// as `__mp.reveal` does), from the last frame's layout; a screen being
    /// rebuilt this frame waits a frame.
    fn scroll_into_view(&mut self, ui: &mut UiState) {
        if ui.dirty {
            return;
        }
        let Some(id) = self.reveal.take() else { return };
        if let Some((_, _, r)) = self.list.iter().find(|(i, ..)| *i == id)
            && (r.1 < 0.0 || r.1 + r.3 > self.view_h)
        {
            ui.reveal = Some(id);
        }
    }

    fn set_editing(&mut self, ui: &mut UiState, on: bool) {
        if self.editing != on {
            self.editing = on;
            ui.dirty = true;
        }
        EDITING.store(self.editing, Ordering::Relaxed);
    }

    fn hide(&mut self, ui: &mut UiState) {
        if !self.shown {
            return;
        }
        self.shown = false;
        self.set_editing(ui, false);
        if ui.focus.is_some() {
            ui.focus = None;
            ui.dirty = true;
        }
    }

    /// The nearest control whose middle lies that way, preferring ones in
    /// line.
    fn mv(&mut self, ui: &mut UiState, dir: Nav) {
        let list = std::mem::take(&mut self.list);
        self.mv_in(ui, &list, dir);
        self.list = list;
    }

    fn mv_in(&mut self, ui: &mut UiState, list: &List, dir: Nav) {
        let Some(cur) = &self.cur else { return };
        if let Some(b) = nearest(list, cur, dir).cloned() {
            self.set_cur(ui, Some(b));
        }
    }
}

/// Puts every control off to the side behind every one in line.
const OFF_LINE: f32 = 1e6;

/// The control `dir` of `cur`: the nearest whose middle lies that way,
/// one in line first. Only when none is in line does one off to the side
/// count: a near control a row down whose middle is just right of this
/// one's (Race under the last row of cars) would otherwise beat the next
/// control along the same row.
fn nearest<'a>(list: &'a List, cur: &str, dir: Nav) -> Option<&'a String> {
    let (_, _, from) = list.iter().find(|(i, ..)| i == cur)?;
    let (fl, ft, fw, fh) = *from;
    let (fr, fb) = (fl + fw, ft + fh);
    let fx = fl + fw / 2.0;
    let fy = ft + fh / 2.0;
    let vert = matches!(dir, Nav::Up | Nav::Down);
    let sign = if matches!(dir, Nav::Up | Nav::Left) {
        -1.0
    } else {
        1.0
    };
    let mut best: Option<&String> = None;
    let mut best_score = f32::INFINITY;
    for (id, _, r) in list {
        if id == cur || *r == *from {
            continue;
        }
        let cx = r.0 + r.2 / 2.0;
        let cy = r.1 + r.3 / 2.0;
        let along = (if vert { cy - fy } else { cx - fx }) * sign;
        if along <= 1.0 {
            continue;
        }
        // How far off to the side it is, 0 where the two overlap.
        let off = if vert {
            0f32.max(r.0 - fr).max(fl - (r.0 + r.2))
        } else {
            0f32.max(r.1 - fb).max(ft - (r.1 + r.3))
        };
        let score = along + off * 3.0 + if off > 0.0 { OFF_LINE } else { 0.0 };
        if score < best_score {
            best = Some(id);
            best_score = score;
        }
    }
    best
}

/// `stepRange`: a slider by five of its steps (its value is 0–100, the
/// setting a hundredth of it), then its `input` handler.
fn step_range(ui: &mut UiState, ctx: &mut ActCtx, sl: Sl, side: f64) {
    let s = &mut ui.settings;
    let (slot, key) = match sl {
        Sl::Music => (&mut s.music, "musicVol"),
        Sl::Sfx => (&mut s.sfx, "sfxVol"),
        Sl::TiltSens => (&mut s.tilt_sens, "tiltSens"),
    };
    let was = mp_math::js::round(*slot * 100.0);
    let v = (was + side * SLIDER_STEP).clamp(0.0, 100.0);
    if v == was {
        return;
    }
    *slot = v / 100.0;
    let val = *slot;
    ctx.store.set_num(key, val);
    ui.dirty = true;
}

/// `stepSelect`: the next or previous option, then its `change` handler.
fn step_select(
    ui: &mut UiState,
    ctx: &mut ActCtx,
    controls: &ControlQuery,
    sel: super::Sel,
    side: i32,
) {
    let (opts, current) = super::menu::select_options(sel, &ui.settings);
    let i = opts.iter().position(|(v, _)| *v == current).unwrap_or(0) as i32 + side;
    if i < 0 || i >= opts.len() as i32 {
        return;
    }
    let v = opts[i as usize].0.clone();
    activate(ui, ctx, controls, Act::Choose(sel, v));
}

/// `menuNav.update(pads.nav)`, every frame, menu or not, so a button held
/// from the race (A for nitro, Start to pause) doesn't count as a press on
/// the screen that opens. Also `body.pad` (a pad connected), and the mouse
/// or a finger hiding the highlight.
#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut nav: ResMut<MenuNav>,
    mut ui: ResMut<UiState>,
    time: Res<Time<Real>>,
    controls: ControlQuery,
    mut cursor: MessageReader<CursorMoved>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut touches: MessageReader<TouchInput>,
    mut last_cursor: Local<Option<Vec2>>,
    mut ctx: ActCtx,
) {
    let nav = &mut *nav;
    let ui = &mut *ui;
    let state = ctx.pads.pads.state.clone();
    if ui.pads != state.connected {
        ui.pads = state.connected;
        ui.dirty = true;
    }
    // `pointerdown` and a mouse that moved hide the highlight.
    let mut pointer = false;
    for c in cursor.read() {
        if last_cursor.is_some_and(|l| l != c.position) {
            pointer = true;
        }
        *last_cursor = Some(c.position);
    }
    pointer |= buttons.read().any(|b| b.state == ButtonState::Pressed);
    pointer |= touches.read().any(|t| t.phase == TouchPhase::Started);
    if pointer {
        nav.hide(ui);
    }
    // Tab moved the focus: the highlight is there now.
    if ui.focus.is_some() && ui.focus != nav.cur {
        nav.cur = ui.focus.clone();
    }

    let now = time.elapsed_secs_f64() * 1000.0;
    let mut fired = [false; 7];
    for k in Nav::ALL {
        let i = k as usize;
        let on = state.nav(k);
        if on && !nav.prev[i] {
            fired[i] = true;
            nav.next[i] = now + REPEAT_DELAY;
        } else if on && DIRS.contains(&k) && now >= nav.next[i] {
            fired[i] = true;
            nav.next[i] = now + REPEAT_EVERY;
        }
        nav.prev[i] = on;
    }
    #[cfg(target_arch = "wasm32")]
    publish(nav);
    let css = ctx.play.css_scale.max(0.01);
    let list = focusables(&controls, css);
    nav.list = list.clone();
    nav.view_h = ctx
        .windows
        .single()
        .map_or(f32::INFINITY, |w| w.height() * css);
    nav.scroll_into_view(ui);
    let root = root_of(ui);
    if root != nav.root {
        if let (Some(r), Some(c)) = (nav.root, nav.cur.clone()) {
            nav.memory.retain(|(s, _)| *s != r);
            nav.memory.push((r, c));
        }
        nav.root = root;
        nav.set_editing(ui, false);
        // Already steering with the pad: the new screen opens highlighted
        // (its memory, else its default once its controls are built).
        let pick = root.and_then(|r| {
            nav.memory
                .iter()
                .find(|(s, _)| *s == r)
                .map(|(_, c)| c.clone())
        });
        nav.cur = pick;
        nav.pending = true;
    }
    let Some(root) = root else { return };
    // The screen's controls are up once its build has run.
    if ui.built != Some(root) || list.is_empty() {
        return;
    }
    if nav.pending {
        nav.pending = false;
        let cur = nav
            .cur
            .clone()
            .filter(|c| list.iter().any(|(i, ..)| i == c))
            .or_else(|| {
                nav.shown
                    .then(|| default_for(root, &list, &ui.focus_order))
                    .flatten()
            });
        nav.set_cur(ui, cur);
    }
    if nav
        .cur
        .as_ref()
        .is_some_and(|c| !list.iter().any(|(i, ..)| i == c))
    {
        nav.set_cur(ui, None);
    }
    if !fired.iter().any(|f| *f) {
        return;
    }
    let cur = nav
        .cur
        .clone()
        .or_else(|| default_for(root, &list, &ui.focus_order));
    if !nav.shown {
        nav.shown = true;
        nav.set_cur(ui, cur.clone());
        if DIRS.iter().any(|d| fired[*d as usize]) {
            return; // the first push only shows where you are
        }
    }
    let Some(cur) = cur else { return };
    let Some((_, act, _)) = list.iter().find(|(i, ..)| *i == cur).cloned() else {
        return;
    };
    let t = tag(&act);
    if fired[Nav::Back as usize] {
        if nav.editing {
            nav.set_editing(ui, false);
        } else {
            // B goes back a screen.
            let a = match root {
                Screen::Pause => Some(Act::Resume),
                Screen::Results => Some(Act::Menu),
                Screen::PadSetup => Some(Act::PadDone),
                _ => None,
            };
            if let Some(a) = a {
                activate(ui, &mut ctx, &controls, a);
            }
        }
        return;
    }
    if fired[Nav::Start as usize] {
        nav.set_editing(ui, false);
        // Start races from the menu and the results.
        let a = match root {
            Screen::Menu => Some(Act::Start),
            Screen::Results => Some(Act::Again),
            _ => None,
        };
        if let Some(a) = a {
            activate(ui, &mut ctx, &controls, a);
        }
        return;
    }
    if fired[Nav::Confirm as usize] {
        if t == Tag::Other {
            activate(ui, &mut ctx, &controls, act);
        } else {
            let e = !nav.editing;
            nav.set_editing(ui, e);
        }
        return;
    }
    for d in DIRS {
        if !fired[d as usize] {
            continue;
        }
        let side = match d {
            Nav::Left => -1.0,
            Nav::Right => 1.0,
            _ => 0.0,
        };
        if side != 0.0 && nav.editing {
            match t {
                Tag::Range(sl) => step_range(ui, &mut ctx, sl, side),
                Tag::Select(sel) => step_select(ui, &mut ctx, &controls, sel, side as i32),
                Tag::Other => {}
            }
        } else {
            nav.set_editing(ui, false);
            nav.mv(ui, d);
        }
    }
}

/// `window.__mp.padNav` for the tests (`body.pad-nav`, `.pad-edit`; the
/// highlighted control is `__mp.focus`).
#[cfg(target_arch = "wasm32")]
fn publish(nav: &MenuNav) {
    use js_sys::{Object, Reflect};
    let Some(w) = web_sys::window() else { return };
    let Ok(mr) = Reflect::get(&w, &"__mp".into()) else {
        return;
    };
    if !mr.is_object() {
        return;
    }
    let o = Object::new();
    let _ = Reflect::set(&o, &"shown".into(), &nav.shown.into());
    let _ = Reflect::set(&o, &"editing".into(), &nav.editing.into());
    let _ = Reflect::set(&mr, &"padNav".into(), &o);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_from_the_menus_last_row_reaches_quit() {
        // The native menu's bottom at 1280 × 1500 (CSS px, as laid out):
        // the Race button, the options row, then Quit (D1060).
        let c = |id: &str, act: Act, r: (f32, f32, f32, f32)| (id.to_string(), act, r);
        let list: List = vec![
            c("btn-start", Act::Start, (530.0, 899.0, 220.0, 50.0)),
            c(
                "vol-music",
                Act::Slide(Sl::Music),
                (190.0, 972.0, 110.0, 16.0),
            ),
            c(
                "opt-mph",
                Act::Toggle(super::super::Opt::Mph),
                (638.0, 972.0, 50.0, 16.0),
            ),
            c("btn-pad", Act::PadSetup, (967.0, 965.0, 168.0, 32.0)),
            c("btn-exit", Act::Exit, (530.0, 1149.0, 220.0, 52.0)),
        ];
        for from in ["vol-music", "opt-mph", "btn-pad"] {
            assert_eq!(
                nearest(&list, from, Nav::Down).map(String::as_str),
                Some("btn-exit"),
                "down from {from}"
            );
        }
        assert_eq!(nearest(&list, "btn-exit", Nav::Down), None, "the last");
        assert_eq!(
            nearest(&list, "btn-exit", Nav::Up).map(String::as_str),
            Some("opt-mph"),
            "back up to the row above, the control in line first"
        );
    }

    /// Right from the last row of cars is the next car, not Race a row
    /// down whose middle is a little right of this car's (the menu at
    /// 1280 × 800, its buttons held to the column).
    #[test]
    fn right_along_a_row_beats_a_nearer_control_below() {
        let c = |id: &str, r: (f32, f32, f32, f32)| (id.to_string(), Act::Start, r);
        let list: List = vec![
            c("pick-rally", (360.0, 664.0, 275.0, 61.0)),
            c("pick-electric", (645.0, 664.0, 275.0, 61.0)),
            c("btn-start", (404.0, 742.0, 220.0, 50.0)),
            c("btn-mp", (636.0, 742.0, 241.0, 52.0)),
        ];
        assert_eq!(
            nearest(&list, "pick-rally", Nav::Right).map(String::as_str),
            Some("pick-electric")
        );
        assert_eq!(
            nearest(&list, "pick-electric", Nav::Right),
            None,
            "nothing right of the row's end"
        );
        assert_eq!(
            nearest(&list, "pick-rally", Nav::Down).map(String::as_str),
            Some("btn-start")
        );
    }

    /// A screen opens on its own control when it has it (the lobby's Start
    /// for the leader), else on the first of the build's focus order that
    /// is there, else on the first control.
    #[test]
    fn a_screen_opens_on_its_first_control() {
        let c = |id: &str| {
            (
                id.to_string(),
                Act::Mp(super::super::lobby::MpAct::Go),
                (0.0, 0.0, 10.0, 10.0),
            )
        };
        let order: Vec<String> = ["mp-name", "mp-car", "mp-go", "mp-leave"]
            .map(String::from)
            .to_vec();
        let leader: List = vec![c("mp-leave"), c("mp-name"), c("mp-go")];
        assert_eq!(
            default_for(Screen::Lobby, &leader, &order).as_deref(),
            Some("mp-go")
        );
        let other: List = vec![c("mp-leave"), c("mp-car"), c("mp-name")];
        assert_eq!(
            default_for(Screen::Lobby, &other, &order).as_deref(),
            Some("mp-name"),
            "the first in focus order, not in the list"
        );
        assert_eq!(
            default_for(Screen::Lobby, &other, &[]).as_deref(),
            Some("mp-leave")
        );
        assert_eq!(default_for(Screen::Lobby, &Vec::new(), &order), None);
        for (screen, id) in [
            (Screen::Menu, "btn-start"),
            (Screen::Pause, "btn-resume"),
            (Screen::Results, "btn-again"),
            (Screen::PadSetup, "pad-bind-left"),
        ] {
            let list: List = vec![c("x"), c(id)];
            assert_eq!(default_for(screen, &list, &[]).as_deref(), Some(id));
        }
    }

    /// Right and left on a slider step it by five of its hundred steps,
    /// within 0–100, and save it; at an end nothing changes.
    #[test]
    fn a_slider_steps_by_five_and_stops_at_the_ends() {
        use super::super::tests::world;
        use bevy::ecs::system::RunSystemOnce;
        let mut w = world(
            Screen::Menu,
            crate::net::NetView::default(),
            crate::options::Options::default(),
        );
        let step = |w: &mut World, sl: Sl, side: f64| -> (f64, bool) {
            w.resource_mut::<UiState>().dirty = false;
            w.run_system_once(move |mut ui: ResMut<UiState>, mut ctx: ActCtx| {
                step_range(&mut ui, &mut ctx, sl, side)
            })
            .unwrap();
            let ui = w.resource::<UiState>();
            let v = match sl {
                Sl::Music => ui.settings.music,
                Sl::Sfx => ui.settings.sfx,
                Sl::TiltSens => ui.settings.tilt_sens,
            };
            (v, ui.dirty)
        };
        // The default 0.7 up to 1, then no further.
        assert_eq!(step(&mut w, Sl::Music, 1.0), (0.75, true));
        for _ in 0..5 {
            step(&mut w, Sl::Music, 1.0);
        }
        assert_eq!(step(&mut w, Sl::Music, 1.0), (1.0, false));
        assert_eq!(
            w.resource::<super::super::store::Store>()
                .num("musicVol", 0.0),
            1.0
        );
        // 0.85 down by fives: never below 0.
        for _ in 0..30 {
            step(&mut w, Sl::Sfx, -1.0);
        }
        assert_eq!(step(&mut w, Sl::Sfx, -1.0), (0.0, false));
        assert_eq!(
            w.resource::<super::super::store::Store>()
                .raw("sfxVol")
                .as_deref(),
            Some("0")
        );
        assert_eq!(step(&mut w, Sl::TiltSens, 1.0), (0.55, true));
        assert_eq!(
            w.resource::<super::super::store::Store>()
                .num("tiltSens", 0.0),
            0.55
        );
    }
}
