//! The level viewer, "god mode" (SPEC 8.6; roadmap WP 6.9; DECISIONS D679,
//! D880 to D899): the built level seen through a free camera, for
//! reviewing it from any point of view. Rust only: nothing here is in the
//! JS game, and nothing here runs in a race, on the menu or in a parity
//! capture. A run is the viewer with `?view=god` on a level (natively
//! `--query "view=god&level=coast"`), or from the menu's "Level viewer"
//! button, which opens that address.
//!
//! The level is built whole as for any run that is not a race (the
//! client's own build by default, D678; `?world=export` draws the export),
//! with no menu, no race field and no stand-in cars. Then each frame:
//!
//! - [`input`] turns the keyboard, mouse, touch and the pads (WP 6.4's
//!   layer) into a [`cams::Drive`] and the panel's actions;
//! - [`drive`] steps the camera of the mode ([`cams`]: free fly, orbit,
//!   overview, and the ride, which is the game's fly camera, `fly.rs`),
//!   puts its pose on the camera, and makes the camera the world's focus
//!   as the game makes the player's car (`World.update(dt, s, focus)`: the
//!   sky dome, the sun's shadow box, the scenery's distance-shown nodes and
//!   LOD, the animators' `s`), with the time of day following the camera's
//!   route position or pinned, fog and the far plane as the panel says;
//! - [`panel`] draws the panel, the overview's route and zones and the
//!   touch controls, through the widget module (`ui::widgets`).
//!
//! The link ([`link`]) carries the pose and the panel's state; the readout
//! counts draws and triangles in the render world ([`stats`]).

pub mod cams;
pub mod groups;
mod input;
pub mod link;
mod panel;
pub mod stats;
#[cfg(target_arch = "wasm32")]
mod web;

use crate::animate::{NodeRef, SceneIndex};
use crate::loader::{AppState, SkyDome};
use crate::options::{FlyParams, Options};
use crate::render::Lighting;
use crate::render::pmrem::EnvRequest;
use crate::status::Status;
use crate::{CameraState, Opts, SkyRes, TrackRes};
use bevy::camera::Projection;
use bevy::camera::visibility::RenderLayers;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy::time::Real;
use bevy::window::PrimaryWindow;
use cams::{Flight, Free, Orbit, Overview, Pose};
use link::{Link, Mode};
use mr_track::Track;

/// Whether this run is the viewer (`?view=god` on a level).
pub fn on(o: &Options) -> bool {
    link::wanted(o) && mr_levels::levels().iter().any(|l| l.id == o.level)
}

/// The game's far plane (`main.js:60`) and the extended one (to see the
/// whole level). Bevy's projection is infinite reversed-z, so the far
/// plane only culls.
pub const FAR_GAME: f32 = 9000.0;
pub const FAR_EXTENDED: f32 = 300_000.0;
/// The game's near plane; higher up, the viewer moves it out with the
/// height above the route (a thousandth of it, at most 50 m), for depth
/// precision from far away.
pub const NEAR_GAME: f32 = 0.3;

/// A render layer nothing draws: a hidden scene group's entities go there.
const HIDDEN_LAYER: usize = 31;

/// The panel's sliders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sl {
    Route,
    Tod,
    Speed,
    Height,
}

/// What a control or a key asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Mode(Mode),
    NextMode,
    Fold,
    Help,
    Menu,
    Fog,
    Far,
    Anim,
    Follow,
    Group(usize),
    Shot,
    Link,
    /// A slider set to a fraction (the pointer's, or a key's step).
    Slide(Sl, f64),
    /// Along the route by metres; the time of day by a fraction.
    RouteBy(f64),
    TodBy(f64),
    /// The free camera's speed by steps (`Free::nudge_speed`).
    SpeedBy(f64),
    /// The overview: fly to the point at the middle of the screen.
    Dive,
    /// Back to the start of the route.
    Home,
}

/// The viewer's state.
#[derive(Resource)]
pub struct Viewer {
    pub mode: Mode,
    pub free: Free,
    pub orbit: Orbit,
    pub over: Overview,
    pub ride: FlyParams,
    /// A flight in progress, and the mode it lands in.
    pub flight: Option<(Flight, Mode)>,
    pub fog: bool,
    pub far: bool,
    pub anim: bool,
    /// The time of day pinned at this route fraction; `None` follows the
    /// camera (`Sky.update`'s `p`).
    pub tod: Option<f64>,
    /// Per scene group, hidden.
    pub hidden: Vec<bool>,
    /// Group names from the link, until the scene's groups are known.
    hide_names: Option<Vec<String>>,
    groups_gen: u32,
    groups_applied: u32,
    /// Fog and far plane as they were before the overview turned them.
    saved: Option<(bool, bool)>,
    pub folded: bool,
    pub help: bool,
    /// Count draws and triangles with the panel folded too (the tools).
    pub count: bool,
    /// The camera's pose this frame.
    pub pose: Pose,
    /// The nearest route position (m) and its zone.
    pub s: f64,
    pub zone: String,
    /// The route's length for the slider, the level's lowest road height,
    /// its box (min x, max x, min z, max z), the height that shows it all.
    pub route_len: f64,
    pub ground: f64,
    pub bounds: [f64; 4],
    pub fit: f64,
    started: bool,
    link: Link,
    /// A short message on the panel (Copied, Saved), and until when.
    pub note: Option<(String, f64)>,
    /// Rebuild the panel.
    pub dirty: bool,
    pub shots: u32,
    /// Frame time: smoothed, and the worst of the last second (ms).
    pub frame_ms: f64,
    pub worst_ms: f64,
    worst_acc: (f64, f64),
    /// Seconds since start (real time).
    pub clock: f64,
    /// The view's size in logical px, and CSS px per logical px.
    pub view: (f64, f64),
    pub css: f64,
    /// The link as of the last settled frame.
    pub query: String,
    level: String,
    /// A touch screen: the stick and the rise and sink buttons.
    pub touch: bool,
    /// The page's safe-area insets, CSS px: top, right, bottom, left.
    pub insets: [f32; 4],
}

impl Viewer {
    pub fn new(o: &Options, touch: bool) -> Viewer {
        let link = Link::parse(o);
        let zero = Pose::new(DVec3::ZERO, 0.0, 0.0);
        Viewer {
            mode: link.mode,
            free: Free::new(zero),
            orbit: Orbit {
                centre: DVec3::ZERO,
                dist: 100.0,
                yaw: 0.0,
                pitch: -0.5,
            },
            over: Overview {
                centre: DVec3::ZERO,
                height: 1000.0,
                yaw: 0.0,
            },
            ride: FlyParams {
                s: 0.0,
                h: 5.0,
                back: 14.0,
                lat: 0.0,
                speed: 0.0,
                yaw: 0.0,
                pitch: -0.08,
            },
            flight: None,
            fog: true,
            far: false,
            anim: true,
            tod: None,
            hidden: Vec::new(),
            hide_names: Some(link.hide.clone()),
            groups_gen: 0,
            groups_applied: 0,
            saved: None,
            // Folded at first on a touch screen, where it would cover the
            // view; open on a desktop.
            folded: link.panel.map_or(touch, |p| !p),
            help: false,
            count: false,
            pose: zero,
            s: 0.0,
            zone: String::new(),
            route_len: 1.0,
            ground: 0.0,
            bounds: [0.0; 4],
            fit: 1000.0,
            started: false,
            link,
            note: None,
            dirty: true,
            shots: 0,
            frame_ms: 0.0,
            worst_ms: 0.0,
            worst_acc: (0.0, 0.0),
            clock: 0.0,
            view: (1280.0, 800.0),
            css: 1.0,
            query: String::new(),
            level: o.level.clone(),
            touch,
            insets: [0.0; 4],
        }
    }

    pub fn level(&self) -> &str {
        &self.level
    }

    /// The first frame with a Track: every camera placed, from the link
    /// where it says.
    fn start(&mut self, track: &Track) {
        self.started = true;
        self.route_len = if track.is_loop {
            track.length
        } else {
            (track.road_end() - 1.0).max(1.0)
        };
        let n = track.n.min(track.px.len());
        let mut b = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut ground = f64::INFINITY;
        for i in 0..n {
            let (x, y, z) = (
                f64::from(track.px[i]),
                f64::from(track.py[i]),
                f64::from(track.pz[i]),
            );
            b = [b[0].min(x), b[1].max(x), b[2].min(z), b[3].max(z)];
            ground = ground.min(y);
        }
        self.bounds = b;
        self.ground = if ground.is_finite() { ground } else { 0.0 };
        let aspect = self.view.0 / self.view.1.max(1.0);
        self.fit = cams::fit_height(b[1] - b[0], b[3] - b[2], aspect);
        let l = self.link.clone();
        let home = start_pose(track, track.start_s);
        self.free = Free::new(l.cam.unwrap_or(home));
        if let Some(s) = l.speed {
            self.free.speed = s.clamp(cams::SPEED_MIN, cams::SPEED_MAX);
        }
        self.orbit = match (l.orbit, l.cam) {
            (Some(c), Some(p)) => Orbit::from_pose(&p, (c - p.pos).length().max(cams::ORBIT_MIN)),
            (Some(c), None) => Orbit {
                centre: c,
                dist: 120.0,
                yaw: home.yaw,
                pitch: -0.5,
            },
            (None, Some(p)) => Orbit::from_pose(&p, 120.0),
            (None, None) => self.orbit_at(track, track.start_s, home.yaw),
        };
        self.over = match (l.mode, l.cam) {
            (Mode::Overview, Some(p)) => Overview::from_pose(&p, self.ground),
            _ => self.whole(),
        };
        self.ride = l.ride.unwrap_or(FlyParams {
            s: track.start_s,
            ..self.ride
        });
        let over = l.mode == Mode::Overview;
        self.fog = l.fog.unwrap_or(!over);
        self.far = l.far.unwrap_or(over);
        if over && l.fog.is_none() && l.far.is_none() {
            self.saved = Some((true, false));
        }
        self.anim = l.anim.unwrap_or(true);
        self.tod = l.t.map(|t| t.clamp(0.0, 1.0));
        self.mode = l.mode;
        self.dirty = true;
    }

    /// The overview of the whole level.
    fn whole(&self) -> Overview {
        let b = self.bounds;
        Overview {
            centre: DVec3::new((b[0] + b[1]) / 2.0, self.ground, (b[2] + b[3]) / 2.0),
            height: self.fit,
            yaw: 0.0,
        }
    }

    fn orbit_at(&self, track: &Track, s: f64, yaw: f64) -> Orbit {
        let f = track.frame(s);
        Orbit {
            centre: DVec3::new(f.x, f.y + 1.0, f.z),
            dist: 90.0,
            yaw,
            pitch: -0.45,
        }
    }

    /// The pose of the current mode (no flight).
    fn mode_pose(&self, m: Mode, track: &Track) -> Pose {
        match m {
            Mode::Free => self.free.pose,
            Mode::Orbit => self.orbit.pose(),
            Mode::Overview => self.over.pose(),
            Mode::Ride => {
                let mut f = self.ride;
                Pose::from_transform(&crate::fly::fly_camera(track, 0.0, &mut f).0)
            }
        }
    }

    fn fly_to(&mut self, to: Pose, then: Mode, dur: f64) {
        self.flight = Some((Flight::new(self.pose, to, dur), then));
        self.mode = then;
        self.dirty = true;
    }

    /// Leaving the overview gives fog and the far plane back.
    fn leave_overview(&mut self) {
        if let Some((fog, far)) = self.saved.take() {
            self.fog = fog;
            self.far = far;
        }
    }

    fn set_mode(&mut self, m: Mode, track: &Track) {
        if m == self.mode && self.flight.is_none() {
            return;
        }
        let from = self.mode;
        let p = self.pose;
        self.dirty = true;
        if from == Mode::Overview && m != Mode::Overview {
            self.leave_overview();
            // Down to the point at the middle of the screen.
            let target = self.ground_point(track, p.pos, p.forward());
            let o = Orbit {
                centre: target,
                dist: 400.0,
                yaw: self.over.yaw,
                pitch: -0.6,
            };
            match m {
                Mode::Free => {
                    self.free = Free::new(o.pose());
                    self.free.speed = self.free.speed.max(40.0);
                }
                Mode::Orbit => self.orbit = o,
                _ => {}
            }
            if m == Mode::Ride {
                self.ride.s = nearest_s(track, target.x, target.z, self.ride.s);
            }
            let to = if m == Mode::Ride {
                self.mode_pose(Mode::Ride, track)
            } else {
                o.pose()
            };
            self.fly_to(to, m, 1.6);
            return;
        }
        match m {
            Mode::Free => {
                let speed = self.free.speed;
                self.free = Free::new(p);
                self.free.speed = speed;
                self.mode = m;
                self.flight = None;
            }
            Mode::Orbit => {
                let target = self.ground_point(track, p.pos, p.forward());
                let d = (target - p.pos).length();
                let d = if d.is_finite() && (3.0..3000.0).contains(&d) {
                    d
                } else {
                    100.0
                };
                self.orbit = Orbit::from_pose(&p, d);
                self.mode = m;
                self.flight = None;
            }
            Mode::Overview => {
                self.saved = Some((self.fog, self.far));
                self.fog = false;
                self.far = true;
                self.over = self.whole();
                let to = self.over.pose();
                self.fly_to(to, m, 1.6);
            }
            Mode::Ride => {
                self.ride.s = nearest_s(track, p.pos.x, p.pos.z, self.s);
                let to = self.mode_pose(Mode::Ride, track);
                self.fly_to(to, m, 1.0);
            }
        }
    }

    /// Where a ray from `from` meets the ground near the route: the plane
    /// at the nearest road's height, refined twice. There is no terrain
    /// height in the client after the build (the world keeps none), so
    /// far from the road this is the road's height (D884).
    fn ground_point(&self, track: &Track, from: DVec3, dir: DVec3) -> DVec3 {
        let mut y = ground_at(track, from.x, from.z);
        let mut hit = None;
        for _ in 0..3 {
            match cams::hit_plane(from, dir, y) {
                Some(h) => {
                    hit = Some(h);
                    y = ground_at(track, h.x, h.z);
                }
                None => break,
            }
        }
        match hit {
            Some(h) => DVec3::new(h.x, y, h.z),
            None => from + dir * 100.0,
        }
    }

    /// A tap on the overview: fly down to orbit the point under it.
    fn pick(&mut self, track: &Track, at: Vec2) {
        if self.mode != Mode::Overview || self.flight.is_some() {
            return;
        }
        let p = self.pose;
        let dir = cams::ray(
            &p,
            self.view.0,
            self.view.1,
            f64::from(at.x),
            f64::from(at.y),
        );
        let target = self.ground_point(track, p.pos, dir);
        self.leave_overview();
        self.orbit = Orbit {
            centre: target,
            dist: 350.0,
            yaw: self.over.yaw,
            pitch: -0.6,
        };
        let to = self.orbit.pose();
        self.fly_to(to, Mode::Orbit, 1.6);
    }

    /// The route position set (the slider, a key): the camera jumps there.
    fn jump(&mut self, track: &Track, s: f64) {
        let s = if track.is_loop {
            track.wrap(s)
        } else {
            s.clamp(0.0, self.route_len)
        };
        self.flight = None;
        match self.mode {
            Mode::Free => {
                let speed = self.free.speed;
                self.free = Free::new(start_pose(track, s));
                self.free.speed = speed;
            }
            Mode::Orbit => {
                let f = track.frame(s);
                self.orbit.centre = DVec3::new(f.x, f.y + 1.0, f.z);
            }
            Mode::Overview => {
                let f = track.frame(s);
                self.over.centre = DVec3::new(f.x, self.ground, f.z);
            }
            Mode::Ride => self.ride.s = s,
        }
    }

    fn act(&mut self, a: Act, track: &Track) {
        // What changes the panel's shape (a slider or a step moves in
        // place, `panel::refresh`).
        let pinned = self.tod.is_some();
        if !matches!(
            a,
            Act::Slide(..) | Act::RouteBy(_) | Act::TodBy(_) | Act::SpeedBy(_) | Act::Home
        ) {
            self.dirty = true;
        }
        match a {
            Act::Mode(m) => self.set_mode(m, track),
            Act::NextMode => self.set_mode(self.mode.next(), track),
            Act::Fold => self.folded = !self.folded,
            Act::Help => self.help = !self.help,
            Act::Menu => platform::open_menu(&self.level),
            Act::Fog => {
                self.fog = !self.fog;
                self.saved = None;
            }
            Act::Far => {
                self.far = !self.far;
                self.saved = None;
            }
            Act::Anim => self.anim = !self.anim,
            Act::Follow => {
                self.tod = match self.tod {
                    Some(_) => None,
                    None => Some(self.tod_now(track)),
                }
            }
            Act::Group(i) => {
                if let Some(h) = self.hidden.get_mut(i) {
                    *h = !*h;
                    self.groups_gen += 1;
                }
            }
            Act::Shot => {
                self.shots += 1;
                let name = format!("viewer-{}-{}.png", self.level, self.shots);
                platform::screenshot(&name);
                self.say(format!("Saved {name}"));
            }
            Act::Link => {
                let q = self.link_now().query(&self.level);
                let shown = platform::copy_link(&q);
                self.query = q;
                self.say(shown);
            }
            Act::Slide(sl, x) => self.slide(sl, x, track),
            Act::RouteBy(d) => self.jump(track, self.s + d),
            Act::TodBy(d) => self.tod = Some((self.tod_now(track) + d).clamp(0.0, 1.0)),
            Act::SpeedBy(k) => match self.mode {
                Mode::Ride => self.ride.speed = (self.ride.speed + k * 10.0).clamp(-300.0, 600.0),
                _ => self.free.nudge_speed(k),
            },
            Act::Dive => {
                let c = Vec2::new(self.view.0 as f32 / 2.0, self.view.1 as f32 / 2.0);
                self.pick(track, c);
            }
            Act::Home => self.jump(track, track.start_s),
        }
        if self.tod.is_some() != pinned {
            self.dirty = true;
        }
    }

    fn slide(&mut self, sl: Sl, x: f64, track: &Track) {
        let x = x.clamp(0.0, 1.0);
        match sl {
            Sl::Route => self.jump(track, x * self.route_len),
            Sl::Tod => self.tod = Some(x),
            Sl::Speed => match self.mode {
                Mode::Ride => self.ride.speed = -100.0 + x * 400.0,
                _ => {
                    // Logarithmic, from the slowest to the fastest.
                    let (a, b) = (cams::SPEED_MIN.ln(), cams::SPEED_MAX.ln());
                    self.free.speed = (a + (b - a) * x).exp();
                }
            },
            Sl::Height => self.ride.h = (0.3f64.ln() + (2000f64.ln() - 0.3f64.ln()) * x).exp(),
        }
    }

    /// A slider's position, 0 to 1.
    pub fn slider(&self, sl: Sl, track: Option<&Track>) -> f64 {
        let v = match sl {
            Sl::Route => self.s / self.route_len.max(1.0),
            Sl::Tod => track.map_or(0.0, |t| self.tod_now(t)),
            Sl::Speed => match self.mode {
                Mode::Ride => (self.ride.speed + 100.0) / 400.0,
                _ => {
                    let (a, b) = (cams::SPEED_MIN.ln(), cams::SPEED_MAX.ln());
                    (self.free.speed.ln() - a) / (b - a)
                }
            },
            Sl::Height => (self.ride.h.ln() - 0.3f64.ln()) / (2000f64.ln() - 0.3f64.ln()),
        };
        v.clamp(0.0, 1.0)
    }

    /// The time of day shown now, as a route fraction.
    pub fn tod_now(&self, track: &Track) -> f64 {
        self.tod.unwrap_or(if track.is_loop {
            0.5
        } else {
            (self.s / track.length.max(1.0)).clamp(0.0, 1.0)
        })
    }

    fn say(&mut self, s: String) {
        self.note = Some((s, self.clock + 4.0));
        self.dirty = true;
    }

    /// The view as a link carries it now.
    pub fn link_now(&self) -> Link {
        let mode = self.mode;
        Link {
            mode,
            cam: Some(self.pose),
            orbit: (mode == Mode::Orbit).then_some(self.orbit.centre),
            ride: (mode == Mode::Ride).then_some(self.ride),
            fog: Some(self.fog),
            far: Some(self.far),
            anim: Some(self.anim),
            t: self.tod,
            hide: self.hidden_names(),
            speed: (mode == Mode::Free).then_some(self.free.speed),
            panel: self.folded.then_some(false),
        }
    }

    /// `__mr.viewer.set(o)` (and the tools): `mode`, `cam: [x, y, z, yaw,
    /// pitch]` (the pose, at once, no flight), `orbit: [x, y, z]`, `ride:
    /// {s, h, back, lat, v, yaw, pitch}`, `fog`, `far`, `anim`, `t` (a
    /// route fraction, or null to follow), `hide: [names]`, `speed`,
    /// `route` (metres: the camera jumps there), `folded`, `help`.
    pub fn apply(&mut self, j: &serde_json::Value, track: &Track) {
        let nums = |k: &str| -> Option<Vec<f64>> {
            j.get(k)?.as_array()?.iter().map(|x| x.as_f64()).collect()
        };
        let cam = nums("cam").and_then(|c| match c[..] {
            [x, y, z, yaw, pitch] => Some(Pose::new(DVec3::new(x, y, z), yaw, pitch)),
            _ => None,
        });
        let orbit = nums("orbit").and_then(|c| match c[..] {
            [x, y, z] => Some(DVec3::new(x, y, z)),
            _ => None,
        });
        if let Some(m) = j
            .get("mode")
            .and_then(|m| m.as_str())
            .and_then(Mode::from_key)
            && m != self.mode
        {
            if cam.is_some() || orbit.is_some() || (m == Mode::Ride && j.get("ride").is_some()) {
                if m == Mode::Overview {
                    self.saved = Some((self.fog, self.far));
                    self.fog = false;
                    self.far = true;
                } else if self.mode == Mode::Overview {
                    self.leave_overview();
                }
                self.mode = m;
                self.flight = None;
            } else {
                self.set_mode(m, track);
            }
        }
        if let Some(p) = cam {
            self.flight = None;
            match self.mode {
                Mode::Free => {
                    let speed = self.free.speed;
                    self.free = Free::new(p);
                    self.free.speed = speed;
                }
                Mode::Orbit => {
                    let d = orbit.map_or(self.orbit.dist, |c| (c - p.pos).length());
                    self.orbit = Orbit::from_pose(&p, d.max(cams::ORBIT_MIN));
                }
                Mode::Overview => self.over = Overview::from_pose(&p, self.ground),
                Mode::Ride => {}
            }
        } else if let Some(c) = orbit {
            self.orbit.centre = c;
        }
        if let Some(r) = j.get("ride") {
            let f = &mut self.ride;
            for (k, slot) in [
                ("s", &mut f.s),
                ("h", &mut f.h),
                ("back", &mut f.back),
                ("lat", &mut f.lat),
                ("v", &mut f.speed),
                ("yaw", &mut f.yaw),
                ("pitch", &mut f.pitch),
            ] {
                if let Some(x) = r.get(k).and_then(|x| x.as_f64()) {
                    *slot = x;
                }
            }
        }
        let flag = |k: &str| j.get(k).and_then(|x| x.as_bool());
        if let Some(b) = flag("fog") {
            self.fog = b;
            self.saved = None;
        }
        if let Some(b) = flag("far") {
            self.far = b;
            self.saved = None;
        }
        if let Some(b) = flag("anim") {
            self.anim = b;
        }
        if let Some(b) = flag("folded") {
            self.folded = b;
        }
        if let Some(b) = flag("help") {
            self.help = b;
        }
        if let Some(b) = flag("count") {
            self.count = b;
        }
        if let Some(t) = j.get("t") {
            self.tod = t.as_f64().map(|t| t.clamp(0.0, 1.0));
        }
        if let Some(s) = j.get("speed").and_then(|x| x.as_f64()) {
            self.free.speed = s.clamp(cams::SPEED_MIN, cams::SPEED_MAX);
        }
        if let Some(list) = j.get("hide").and_then(|x| x.as_array()) {
            let names: Vec<String> = list
                .iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect();
            self.hidden.fill(false);
            self.hide_names = Some(names);
        }
        if let Some(s) = j.get("route").and_then(|x| x.as_f64()) {
            self.jump(track, s);
        }
        self.dirty = true;
    }

    /// What `__mr.viewer` reports.
    pub fn report(&self, status: &Status) -> serde_json::Value {
        let p = self.pose;
        let c = stats::counts();
        let f = &self.ride;
        serde_json::json!({
            "ready": self.started && status.ready,
            "level": self.level,
            "mode": self.mode.key(),
            "flying": self.flight.is_some(),
            "pose": {"x": p.pos.x, "y": p.pos.y, "z": p.pos.z, "yaw": p.yaw, "pitch": p.pitch},
            "orbit": {"x": self.orbit.centre.x, "y": self.orbit.centre.y, "z": self.orbit.centre.z, "dist": self.orbit.dist},
            "overview": {"x": self.over.centre.x, "z": self.over.centre.z, "height": self.over.height, "yaw": self.over.yaw, "fit": self.fit},
            "ride": {"s": f.s, "h": f.h, "back": f.back, "lat": f.lat, "v": f.speed, "yaw": f.yaw, "pitch": f.pitch},
            "speed": self.free.speed,
            "s": self.s,
            "routeLength": self.route_len,
            "zone": self.zone,
            "fog": self.fog,
            "far": self.far,
            "anim": self.anim,
            "t": self.tod,
            "follow": self.tod.is_none(),
            "groups": GROUP_NAMES.with_names(|n| n.to_vec()),
            "hidden": self.hidden_names(),
            "folded": self.folded,
            "link": self.link_now().query(&self.level),
            "frameMs": self.frame_ms,
            "worstMs": self.worst_ms,
            "draws": c.draws,
            "tris": c.tris,
            "shadowDraws": c.shadow_draws,
            "shadowTris": c.shadow_tris,
            "touch": self.touch,
        })
    }

    fn hidden_names(&self) -> Vec<String> {
        // Names come from the scene (`apply_groups` keeps them in step).
        GROUP_NAMES.with_names(|names| {
            self.hidden
                .iter()
                .zip(names)
                .filter(|(h, _)| **h)
                .map(|(_, n)| n.clone())
                .collect()
        })
    }
}

/// The scene groups' names, for the link and the panel (set once the
/// scene is indexed).
pub struct GroupNames(std::sync::Mutex<Vec<String>>);

impl GroupNames {
    pub fn with_names<R>(&self, f: impl FnOnce(&[String]) -> R) -> R {
        f(&self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn set(&self, names: Vec<String>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = names;
    }
}

pub static GROUP_NAMES: GroupNames = GroupNames(std::sync::Mutex::new(Vec::new()));

/// Behind the road at `s`, above it, looking along it.
fn start_pose(track: &Track, s: f64) -> Pose {
    let a = track.frame(s - 35.0);
    let b = track.frame(s + 45.0);
    Pose::looking(
        DVec3::new(a.x, a.y + 14.0, a.z),
        DVec3::new(b.x, b.y + 2.0, b.z),
    )
}

/// The nearest route position to (x, z): every eighth sample, then the
/// track's own projection about the best.
pub fn nearest_s(track: &Track, x: f64, z: f64, _hint: f64) -> f64 {
    let n = track.n.min(track.px.len());
    if n == 0 {
        return 0.0;
    }
    let mut best = 0;
    let mut bd = f64::INFINITY;
    let mut i = 0;
    while i < n {
        let dx = x - f64::from(track.px[i]);
        let dz = z - f64::from(track.pz[i]);
        let d = dx * dx + dz * dz;
        if d < bd {
            bd = d;
            best = i;
        }
        i += 8;
    }
    track.project_window(x, z, best as f64, 12).s
}

/// The road's height nearest (x, z).
fn ground_at(track: &Track, x: f64, z: f64) -> f64 {
    let s = nearest_s(track, x, z, 0.0);
    track.frame(s).y
}

/// The zone at route position `s`.
fn zone_at(track: &Track, s: f64) -> &str {
    let s = track.wrap(s);
    track
        .zones
        .iter()
        .find(|z| s >= z.s0 && s < z.s1)
        .or(track.zones.last())
        .map_or("", |z| z.zone.name)
}

/// Each frame: the actions, the camera, the world's focus and the sky.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn drive(
    time: Res<Time<Real>>,
    mut v: ResMut<Viewer>,
    mut intent: ResMut<input::Intent>,
    tr: Res<TrackRes>,
    opts: Res<Opts>,
    mut status: ResMut<Status>,
    mut cs: ResMut<CameraState>,
    mut cam: Query<(&mut Transform, &mut Projection), (With<Camera3d>, Without<SkyDome>)>,
    mut dome: Query<&mut Transform, With<SkyDome>>,
    mut sky_res: ResMut<SkyRes>,
    mut lighting: ResMut<Lighting>,
    mut env: ResMut<EnvRequest>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let real = time.delta_secs_f64();
    v.clock += real;
    let ms = real * 1000.0;
    v.frame_ms = if v.frame_ms == 0.0 {
        ms
    } else {
        v.frame_ms * 0.95 + ms * 0.05
    };
    v.worst_acc.1 = v.worst_acc.1.max(ms);
    if v.clock - v.worst_acc.0 >= 1.0 {
        v.worst_ms = v.worst_acc.1;
        v.worst_acc = (v.clock, 0.0);
    }
    if let Some((_, until)) = &v.note
        && v.clock > *until
    {
        v.note = None;
        v.dirty = true;
    }
    let Ok(window) = windows.single() else { return };
    let view = (
        f64::from(window.width()).max(1.0),
        f64::from(window.height()).max(1.0),
    );
    if view != v.view {
        v.view = view;
        v.dirty = true;
    }
    let Some(track) = &tr.track else { return };
    if !v.started {
        v.start(track);
    }
    // `Math.min(frameDt, 1 / 20)`, and still until the scene is ready.
    let dt = if status.ready { real.min(0.05) } else { 0.0 };
    let acts = std::mem::take(&mut intent.acts);
    for a in acts {
        v.act(a, track);
    }
    let picks = std::mem::take(&mut intent.picks);
    for p in picks {
        v.pick(track, p);
    }
    let d = std::mem::take(&mut intent.drive);
    let mut ride_focus = None;
    let mut ride_pose = None;
    let pose = if let Some((f, then)) = v.flight.as_mut() {
        let then = *then;
        let (p, done) = f.step(real.min(0.1));
        if done {
            v.flight = None;
            // Land in the mode's own state.
            match then {
                Mode::Free => v.free.pose = p,
                Mode::Orbit | Mode::Overview | Mode::Ride => {}
            }
        }
        p
    } else {
        let v = &mut *v;
        match v.mode {
            Mode::Free => {
                // Two fingers slide the camera across and up.
                let p = v.free.pose;
                let k = v.free.speed * 0.004;
                v.free.pose.pos += (-p.right() * d.pan.0 + DVec3::Y * d.pan.1) * k;
                v.free.step(dt, &d);
                v.free.pose
            }
            Mode::Orbit => {
                // Two fingers pan the centre across the screen.
                let mut d = d;
                let k = v.orbit.dist * 2.0 * (cams::FOV_Y_DEG.to_radians() / 2.0).tan() / v.view.1;
                let p = v.orbit.pose();
                v.orbit.centre += (-p.right() * d.pan.0 + p.up() * d.pan.1) * k;
                d.pan = (0.0, 0.0);
                v.orbit.step(dt, &d);
                v.orbit.pose()
            }
            Mode::Overview => {
                let max = v.fit * 3.0;
                v.over.step(dt, &d, v.view.1, max);
                v.over.pose()
            }
            Mode::Ride => {
                cams::ride_step(&mut v.ride, dt, &d);
                let (t, focus) = crate::fly::fly_camera(track, dt, &mut v.ride);
                ride_focus = Some(focus);
                ride_pose = Some(t);
                Pose::from_transform(&t)
            }
        }
    };
    v.pose = pose;
    // The route position, the zone.
    let s = nearest_s(track, pose.pos.x, pose.pos.z, v.s);
    if (s - v.s).abs() > 1e-6 {
        v.s = s;
    }
    let zone = zone_at(track, s);
    if zone != v.zone {
        v.zone = zone.to_owned();
    }
    // The camera, its near and far planes.
    let ground_y = track.frame(s).y;
    if let Ok((mut t, mut proj)) = cam.single_mut() {
        *t = ride_pose.unwrap_or_else(|| pose.transform());
        if let Projection::Perspective(p) = &mut *proj {
            let far = if v.far { FAR_EXTENDED } else { FAR_GAME };
            let near = (((pose.pos.y - ground_y).abs() * 0.001) as f32).clamp(NEAR_GAME, 50.0);
            if p.far != far || p.near != near {
                p.far = far;
                p.near = near;
            }
        }
    }
    // The world's focus is the camera (the game's is the player's car;
    // the ride keeps the fly camera's, `main.js`'s `frame(s - back)`).
    let focus = ride_focus.unwrap_or(pose.pos);
    cs.focus = focus;
    status.s = s;
    if status.route.is_none() {
        status.route = Some((track.length, track.road_end(), track.is_loop));
    }
    crate::loader::follow_focus(focus, &mut dome);
    if let Some(sky) = sky_res.sky.as_mut() {
        sky.override_p = v.tod;
    }
    let world_dt = if v.anim { dt } else { 0.0 };
    let mut next = lighting.clone();
    crate::update_sky(
        &mut sky_res,
        &opts,
        (world_dt, window.scale_factor().into()),
        s,
        focus,
        &mut next,
        &mut env,
    );
    if !v.fog {
        next.fog = next.fog.map(|(c, _)| (c, 0.0));
    }
    if *lighting != next {
        *lighting = next;
    }
}

/// The scene groups: their names once the scene is indexed, the link's
/// hidden ones resolved, and a hidden group's entities on a layer nothing
/// draws (`RenderLayers`; the animators' visibility edits untouched).
fn apply_groups(
    mut commands: Commands,
    mut v: ResMut<Viewer>,
    index: Option<Res<SceneIndex>>,
    nodes: Query<(Entity, &NodeRef, Has<RenderLayers>)>,
    mut known: Local<usize>,
) {
    let Some(index) = index else { return };
    let g = &index.groups;
    if v.hidden.len() != g.names.len() {
        v.hidden = vec![false; g.names.len()];
        GROUP_NAMES.set(g.names.clone());
        v.dirty = true;
    }
    if let Some(names) = v.hide_names.take() {
        for n in names {
            if let Some(i) = g.names.iter().position(|x| *x == n) {
                v.hidden[i] = true;
            } else {
                warn!("viewer: no scene group named {n:?} (have {:?})", g.names);
            }
        }
        v.groups_gen += 1;
    }
    let count = nodes.iter().len();
    if v.groups_gen == v.groups_applied && count == *known {
        return;
    }
    v.groups_applied = v.groups_gen;
    *known = count;
    for (e, n, layered) in &nodes {
        let hide = g.group_of(n.0).is_some_and(|k| v.hidden[k]);
        if hide && !layered {
            commands.entity(e).insert(RenderLayers::layer(HIDDEN_LAYER));
        } else if !hide && layered {
            commands.entity(e).remove::<RenderLayers>();
        }
    }
}

/// Settled for half a second: the link is brought up to date (the
/// page's address on the web, so the address is always the view).
fn keep_link(mut v: ResMut<Viewer>, mut last: Local<(Option<Pose>, f64)>) {
    if !v.started {
        return;
    }
    let pose = Some(v.pose);
    if last.0 != pose {
        *last = (pose, v.clock);
        return;
    }
    if v.clock - last.1 < 0.5 || v.flight.is_some() {
        return;
    }
    let q = v.link_now().query(&v.level);
    if q != v.query {
        platform::set_address(&q);
        v.query = q;
    }
}

pub fn plugin(app: &mut App) {
    let o = app.world().resource::<Opts>().o.clone();
    if !on(&o) {
        return;
    }
    let touch = crate::ui::touch_ui(&o);
    app.insert_resource(Viewer::new(&o, touch))
        .init_resource::<input::Intent>()
        .init_resource::<input::Pointers>()
        .add_systems(
            Startup,
            (
                crate::ui::widgets::load_fonts,
                crate::ui::widgets::make_icons,
            ),
        )
        .add_systems(
            Update,
            (
                input::gather,
                drive,
                apply_groups,
                keep_link,
                panel::build,
                panel::refresh,
                panel::overlay,
            )
                .chain()
                .after(crate::fly_system)
                .run_if(in_state(AppState::Running)),
        );
    stats::plugin(app);
    crate::play::gamepad_io::pads_only(app);
    #[cfg(target_arch = "wasm32")]
    web::plugin(app);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, native::announce.run_if(in_state(AppState::Running)));
}

/// What the viewer asks of the platform: the screenshot, the link, the
/// page's address, the menu.
mod platform {
    #[cfg(target_arch = "wasm32")]
    pub use super::web::{copy_link, open_menu, screenshot, set_address};

    #[cfg(not(target_arch = "wasm32"))]
    pub use super::native::{copy_link, open_menu, screenshot, set_address};
}

/// Natively: the screenshot is a file in the working directory, the link
/// is printed (the same string is `--query`), and the menu is the client
/// started again without the viewer.
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use bevy::prelude::*;
    use std::sync::Mutex;

    static SHOT: Mutex<Option<String>> = Mutex::new(None);
    static MENU: Mutex<bool> = Mutex::new(false);

    pub fn screenshot(name: &str) {
        *SHOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_owned());
    }

    pub fn copy_link(q: &str) -> String {
        println!("viewer link: --query \"{q}\"  (web: index.html?{q})");
        "Link printed".into()
    }

    pub fn set_address(_q: &str) {}

    pub fn open_menu(_level: &str) {
        *MENU.lock().unwrap_or_else(|e| e.into_inner()) = true;
    }

    /// The screenshot and the way back to the menu, from the frame.
    pub fn announce(mut commands: Commands, mut exit: MessageWriter<AppExit>) {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        if let Some(name) = SHOT.lock().unwrap_or_else(|e| e.into_inner()).take() {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(name));
        }
        let mut menu = MENU.lock().unwrap_or_else(|e| e.into_inner());
        if *menu {
            *menu = false;
            // The client again, on its menu (D881).
            match std::env::current_exe() {
                Ok(exe) => {
                    if let Err(e) = std::process::Command::new(exe).spawn() {
                        warn!("viewer: starting the menu: {e}");
                        return;
                    }
                    exit.write(AppExit::Success);
                }
                Err(e) => warn!("viewer: starting the menu: {e}"),
            }
        }
    }
}

/// The menu's "Level viewer" button: the viewer on `level` (the page at
/// `?view=god&level=…`; natively the client started again with that
/// query, D881).
pub fn open(level: &str) {
    let q = format!("view=god&level={}", link::encode(level));
    #[cfg(target_arch = "wasm32")]
    web::navigate(&q);
    #[cfg(not(target_arch = "wasm32"))]
    match std::env::current_exe() {
        Ok(exe) => match std::process::Command::new(exe)
            .args(["--query", &q])
            .spawn()
        {
            Ok(_) => std::process::exit(0),
            Err(e) => warn!("viewer: {e}"),
        },
        Err(e) => warn!("viewer: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        let mut t = Track::new(&mr_levels::level_by_id(id)).unwrap();
        t.runout = mr_levels::world::world_data(id).runout;
        t
    }

    #[test]
    fn nearest_finds_the_route_position() {
        let t = track("coast");
        for s in [10.0, 1234.0, 7000.5, t.length - 20.0] {
            let f = t.frame(s);
            let got = nearest_s(&t, f.x + f.rx * 3.0, f.z + f.rz * 3.0, 0.0);
            assert!((got - s).abs() < 1.0, "{s} → {got}");
        }
    }

    #[test]
    fn the_viewer_starts_where_the_link_says() {
        let t = track("sierra");
        let q =
            "view=god&level=sierra&mode=orbit&cam=100,50,200,0.5,-0.4&orbit=60,10,150&fog=0&t=0.3";
        let o = Options::from_query(q);
        assert!(on(&o));
        let mut v = Viewer::new(&o, false);
        v.start(&t);
        assert_eq!(v.mode, Mode::Orbit);
        let p = v.mode_pose(Mode::Orbit, &t);
        assert!((p.pos - DVec3::new(100.0, 50.0, 200.0)).length() < 1e-6);
        assert!((p.yaw - 0.5).abs() < 1e-9 && (p.pitch + 0.4).abs() < 1e-9);
        assert!(!v.fog && !v.far && v.anim);
        assert_eq!(v.tod, Some(0.3));
        // The overview turns fog off and the far plane out, and gives them
        // back on the way down.
        v.pose = p;
        v.set_mode(Mode::Overview, &t);
        assert!(!v.fog && v.far);
        v.flight = None;
        v.pose = v.over.pose();
        v.set_mode(Mode::Free, &t);
        assert!(!v.fog && !v.far);
        assert!(!on(&Options::from_query("view=god&level=models")));
    }
}
