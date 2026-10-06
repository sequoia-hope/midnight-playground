//! The Controller screen's logic (port of `src/game/PadSetup.js`, WP 6.4;
//! the screen is `screens::padsetup`): what each action is bound to on the
//! pad last touched, lit while held so you can try it, and picking a new
//! button or stick for one (`Pads::start_capture`). Defaults puts that pad
//! back on the standard layout.

use super::{Screen, UiState};
use crate::play::Play;
use crate::play::gamepad::{Action, Pad, Pads, binding_label, bindings, default_map};
use crate::play::gamepad_io::PadsRes;
use bevy::input::ButtonState;
use bevy::input::keyboard::{KeyCode, KeyboardInput};
use bevy::prelude::*;

/// What the screen shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PadView {
    /// `#pad-name`.
    pub name: String,
    /// Each action's bindings, as the row's `<b>` reads (`X / RB`, `—`).
    pub binds: Vec<(Action, String)>,
    /// `.on`: held now.
    pub on: Vec<Action>,
    /// `.listening`: the row whose binding is being picked up.
    pub listening: Option<Action>,
    /// `#pad-hint`.
    pub hint: String,
}

impl PadView {
    pub fn bind(&self, a: Action) -> &str {
        self.binds
            .iter()
            .find(|(x, _)| *x == a)
            .map_or("—", |(_, s)| s.as_str())
    }
}

/// `PadSetup`.
#[derive(Resource, Default)]
pub struct PadSetup {
    pub open: bool,
    /// `undefined`: the default hint for the pad in hand.
    hint: Option<String>,
    pad_key: Option<String>,
    listening: Option<Action>,
    pub view: PadView,
}

/// "Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product:
/// 0b13)" → "Xbox Wireless Controller" (`/\s*\((STANDARD GAMEPAD|Vendor:)
/// [^)]*\)/i`, the first match).
pub fn pad_name(id: &str) -> String {
    let lower = id.to_ascii_lowercase();
    let mut out = id.to_string();
    for (j, _) in id.match_indices('(') {
        let rest = &lower[j + 1..];
        if !(rest.starts_with("standard gamepad") || rest.starts_with("vendor:")) {
            continue;
        }
        let Some(close) = id[j..].find(')') else {
            continue;
        };
        let start = id[..j].trim_end_matches(char::is_whitespace).len();
        out = format!("{}{}", &id[..start], &id[j + close + 1..]);
        break;
    }
    let t = out.trim();
    if t.is_empty() {
        "Controller".into()
    } else {
        t.into()
    }
}

fn default_hint(pads: &Pads) -> String {
    if pads.active.is_some() {
        "Pick an action, then press the button or move the stick you want for it".into()
    } else {
        "Press a button on your controller".into()
    }
}

impl PadSetup {
    pub fn show(&mut self, pads: &mut Pads) {
        self.open = true;
        self.pad_key = None;
        self.hint = None;
        self.listening = None;
        self.update(pads);
    }

    /// `close()`: stop listening, and back to the screen it came from.
    pub fn close(&mut self, pads: &mut Pads, ui: &mut UiState) {
        if !self.open {
            return;
        }
        pads.cancel_capture();
        self.take_done(pads);
        self.open = false;
        ui.screen = ui.pad_return;
        ui.dirty = true;
    }

    /// Esc, P or Start (Start is only heard when not listening): stop
    /// listening, or leave.
    pub fn escape(&mut self, pads: &mut Pads, ui: &mut UiState) {
        if pads.capture.is_some() {
            pads.cancel_capture();
            self.take_done(pads);
        } else {
            self.close(pads, ui);
        }
    }

    /// Defaults: that pad back on the standard layout.
    pub fn defaults(&mut self, pads: &mut Pads) {
        pads.cancel_capture();
        self.take_done(pads);
        let p = pads.active.clone();
        pads.reset_map(p.as_ref());
        self.hint = Some("Back to the standard layout".into());
    }

    /// A row picked: listen for its new binding (again: stop listening).
    pub fn pick(&mut self, pads: &mut Pads, a: Action) {
        let was = pads.capture.as_ref().map(|c| c.action);
        pads.cancel_capture();
        self.take_done(pads);
        if was == Some(a) {
            return;
        }
        if pads.active.is_none() {
            self.hint = None;
            return;
        }
        self.listening = Some(a);
        self.hint = Some(format!(
            "Press a button or move a stick for {} (Esc to cancel)",
            a.label()
        ));
        pads.start_capture(a);
    }

    /// The captures that ended (`done(b)`): the row stops listening, the
    /// hint says what it is now.
    fn take_done(&mut self, pads: &mut Pads) {
        for (a, b) in std::mem::take(&mut pads.done) {
            if self.listening == Some(a) {
                self.listening = None;
            }
            self.hint = b.map(|b| {
                format!(
                    "{}: {}",
                    a.label(),
                    binding_label(Some(&b), pads.active.as_ref().is_some_and(Pad::standard))
                )
            });
        }
    }

    /// Every frame while open: the labels when the pad or its map changes,
    /// and which actions are held.
    pub fn update(&mut self, pads: &mut Pads) {
        self.take_done(pads);
        let p = pads.active.clone();
        let pad_key = p
            .as_ref()
            .map_or(String::new(), |p| format!("{}{}", p.index, p.id));
        if self.pad_key.as_ref() != Some(&pad_key) {
            self.pad_key = Some(pad_key);
            if pads.capture.is_none() {
                self.hint = None;
            }
        }
        let def = default_map();
        let map = p.as_ref().map_or(&def, |p| pads.map_for(p));
        let std = p.as_ref().is_none_or(Pad::standard);
        let name = match &p {
            Some(p) => {
                pad_name(&p.id)
                    + if pads.is_remapped(Some(p)) {
                        " · remapped"
                    } else if std {
                        ""
                    } else {
                        " · not a standard layout: check the buttons"
                    }
            }
            None => "No controller yet".into(),
        };
        let binds = Action::ALL
            .into_iter()
            .map(|a| {
                let list = bindings(map, a.key());
                let s = list
                    .iter()
                    .map(|b| binding_label(Some(b), std))
                    .collect::<Vec<_>>()
                    .join(" / ");
                (a, if s.is_empty() { "—".into() } else { s })
            })
            .collect();
        let on = if pads.capture.is_some() {
            Vec::new()
        } else {
            Action::ALL
                .into_iter()
                .filter(|a| pads.state.held(*a))
                .collect()
        };
        self.view = PadView {
            name,
            binds,
            on,
            listening: self.listening,
            hint: self.hint.clone().unwrap_or_else(|| default_hint(pads)),
        };
    }
}

/// Each frame: Esc, P or Start on the Controller screen leave it (or stop
/// listening), before the race could take them as un-pause (`main.js`:
/// `if (screenNow === 'padsetup' && input.consume('pause'))
/// padSetup.escape()`); then the screen follows the pads. Runs in the
/// race's frame, after the pads reach its input layer and before its
/// ticks.
pub fn frame(
    mut setup: ResMut<PadSetup>,
    mut pads: ResMut<PadsRes>,
    mut ui: ResMut<UiState>,
    mut play: ResMut<Play>,
    mut keys: MessageReader<KeyboardInput>,
) {
    let setup = &mut *setup;
    let pads = &mut pads.pads;
    let ui = &mut *ui;
    let keyed = keys.read().any(|k| {
        k.state == ButtonState::Pressed
            && !k.repeat
            && matches!(k.key_code, KeyCode::Escape | KeyCode::KeyP)
    });
    if !setup.open {
        return;
    }
    if ui.screen != Screen::PadSetup {
        // Left some other way: stop listening.
        pads.cancel_capture();
        pads.done.clear();
        setup.open = false;
        return;
    }
    let esc = match play.race.as_mut() {
        Some(r) => r.input.consume("pause"),
        None => keyed || pads.state.edges.contains(&Action::Pause),
    };
    if esc {
        setup.escape(pads, ui);
        if !setup.open {
            return;
        }
    }
    let before = setup.view.clone();
    setup.update(pads);
    if setup.view != before {
        ui.dirty = true;
    }
    #[cfg(target_arch = "wasm32")]
    publish(&setup.view);
}

/// `window.__mp.padsetup`: the screen as the tests read its DOM (`#pad-name`,
/// each row's `<b>`, `.on`, `.listening`, `#pad-hint`).
#[cfg(target_arch = "wasm32")]
fn publish(v: &PadView) {
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::JsValue;
    let Some(w) = web_sys::window() else { return };
    let Ok(mr) = Reflect::get(&w, &"__mp".into()) else {
        return;
    };
    if !mr.is_object() {
        return;
    }
    let set = |o: &Object, k: &str, v: JsValue| {
        let _ = Reflect::set(o, &JsValue::from_str(k), &v);
    };
    let o = Object::new();
    set(&o, "name", v.name.as_str().into());
    let binds = Object::new();
    for (a, s) in &v.binds {
        set(&binds, a.key(), s.as_str().into());
    }
    set(&o, "binds", binds.into());
    let on = Array::new();
    for a in &v.on {
        on.push(&a.key().into());
    }
    set(&o, "on", on.into());
    set(
        &o,
        "listening",
        v.listening.map_or(JsValue::NULL, |a| a.key().into()),
    );
    set(&o, "hint", v.hint.as_str().into());
    let _ = Reflect::set(&mr, &"padsetup".into(), &o);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_names_lose_the_browsers_suffix() {
        assert_eq!(
            pad_name("Xbox Wireless Controller (STANDARD GAMEPAD Vendor: 045e Product: 0b13)"),
            "Xbox Wireless Controller"
        );
        assert_eq!(
            pad_name("Test Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)"),
            "Test Pad"
        );
        assert_eq!(
            pad_name("054c-09cc-Wireless Controller (Vendor: 054c Product: 09cc)"),
            "054c-09cc-Wireless Controller"
        );
        assert_eq!(pad_name("Odd (thing) pad"), "Odd (thing) pad");
        assert_eq!(pad_name(" (STANDARD GAMEPAD)"), "Controller");
        assert_eq!(pad_name(""), "Controller");
    }

    #[test]
    fn the_screen_follows_a_capture() {
        let mut pads = Pads::default();
        let mut p = Pad {
            id: "Test Pad (STANDARD GAMEPAD Vendor: 045e Product: 028e)".into(),
            index: 0,
            mapping: "standard".into(),
            axes: vec![0.0; 4],
            buttons: vec![Default::default(); 17],
        };
        let mut s = PadSetup::default();
        s.show(&mut pads);
        assert_eq!(s.view.name, "No controller yet");
        assert_eq!(s.view.hint, "Press a button on your controller");
        assert_eq!(s.view.bind(Action::Handbrake), "X / RB");
        let mut rum = crate::play::gamepad::NoRumble;
        pads.poll(0.0, std::slice::from_ref(&p), &mut rum);
        s.update(&mut pads);
        assert_eq!(s.view.name, "Test Pad");
        p.buttons[7].pressed = true;
        p.buttons[7].value = 1.0;
        pads.poll(16.0, std::slice::from_ref(&p), &mut rum);
        s.update(&mut pads);
        assert_eq!(s.view.on, vec![Action::Throttle], "RT lights its row");
        p.buttons[7] = Default::default();
        pads.poll(32.0, std::slice::from_ref(&p), &mut rum);
        s.pick(&mut pads, Action::Throttle);
        s.update(&mut pads);
        assert_eq!(s.view.listening, Some(Action::Throttle));
        assert!(
            s.view
                .hint
                .starts_with("Press a button or move a stick for Throttle")
        );
        pads.poll(48.0, std::slice::from_ref(&p), &mut rum); // armed
        p.buttons[4].pressed = true;
        p.buttons[4].value = 1.0;
        pads.poll(64.0, std::slice::from_ref(&p), &mut rum);
        s.update(&mut pads);
        assert_eq!(s.view.listening, None);
        assert_eq!(s.view.bind(Action::Throttle), "LB");
        assert_eq!(s.view.hint, "Throttle: LB");
        assert_eq!(s.view.name, "Test Pad · remapped");
        s.defaults(&mut pads);
        s.update(&mut pads);
        assert_eq!(s.view.bind(Action::Throttle), "RT");
        assert_eq!(s.view.hint, "Back to the standard layout");
        // Picking the row that is listening stops it.
        s.pick(&mut pads, Action::Nitro);
        s.pick(&mut pads, Action::Nitro);
        assert!(pads.capture.is_none());
        s.update(&mut pads);
        assert_eq!(s.view.listening, None);
    }
}
