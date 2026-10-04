//! The front end's screens (roadmap M6, WP 6.1 and 6.2; SPEC 8.1), in
//! Bevy UI (D431).
//!
//! - [`store`]: the settings and records under the JS's `localStorage` keys.
//! - [`widgets`]: tokens, breakpoints, the font and the controls.

pub mod store;
pub mod widgets;

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
