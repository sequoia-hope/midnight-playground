//! The viewer's link (SPEC 8.6): the query string that opens the viewer on
//! a level at a camera pose with the panel's state, so a view can be sent
//! and opened again exactly. The same string is the native `--query`.
//!
//! `view=god&level=coast&mode=orbit&cam=x,y,z,yaw,pitch&orbit=x,y,z&fog=0&
//! far=1&anim=0&t=0.35&hide=a,b&speed=40&panel=0`, where
//! - `mode`: `free`, `orbit`, `over` (the overview) or `ride`;
//! - `cam`: the camera's pose (metres; yaw and pitch in radians, see
//!   `cams::Pose`), for every mode;
//! - `orbit`: the orbit's centre (orbit mode);
//! - `s`, `h`, `back`, `lat`, `v`, `yaw`, `pitch`: the ride (the game's fly
//!   camera parameters, as `?s=` gives them);
//! - `fog`, `far`, `anim`: the panel's toggles (1 on, 0 off);
//! - `t`: the time of day pinned at that fraction of the route (the game's
//!   own `?t=`); absent, it follows the camera;
//! - `hide`: scene groups hidden, by name;
//! - `speed`: the free camera's speed (m/s); `panel=0`: the panel folded.

use super::cams::Pose;
use crate::options::{FlyParams, Options};
use bevy::math::DVec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Free,
    Orbit,
    Overview,
    Ride,
}

impl Mode {
    pub const ALL: [Mode; 4] = [Mode::Free, Mode::Orbit, Mode::Overview, Mode::Ride];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Free => "free",
            Mode::Orbit => "orbit",
            Mode::Overview => "over",
            Mode::Ride => "ride",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Free => "Free",
            Mode::Orbit => "Orbit",
            Mode::Overview => "Overview",
            Mode::Ride => "Ride",
        }
    }

    pub fn from_key(k: &str) -> Option<Mode> {
        Mode::ALL
            .into_iter()
            .find(|m| m.key() == k || m.label().eq_ignore_ascii_case(k))
    }

    pub fn next(self) -> Mode {
        let i = Mode::ALL.iter().position(|m| *m == self).unwrap_or(0);
        Mode::ALL[(i + 1) % Mode::ALL.len()]
    }
}

/// A view as a link carries it.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Link {
    pub mode: Mode,
    pub cam: Option<Pose>,
    pub orbit: Option<DVec3>,
    pub ride: Option<FlyParams>,
    pub fog: Option<bool>,
    pub far: Option<bool>,
    pub anim: Option<bool>,
    pub t: Option<f64>,
    pub hide: Vec<String>,
    pub speed: Option<f64>,
    pub panel: Option<bool>,
}

/// Whether this run is the viewer: `?view=god` on a level.
pub fn wanted(o: &Options) -> bool {
    o.param("view") == Some("god")
}

fn nums(v: &str) -> Vec<f64> {
    v.split(',')
        .map(|x| x.trim().parse().unwrap_or(f64::NAN))
        .collect()
}

fn flag(v: Option<&str>) -> Option<bool> {
    v.map(|v| v == "1" || v == "true" || v == "on")
}

impl Link {
    pub fn parse(o: &Options) -> Link {
        let cam = o.param("cam").and_then(|v| match nums(v)[..] {
            [x, y, z, yaw, pitch] if [x, y, z, yaw, pitch].iter().all(|n| n.is_finite()) => {
                Some(Pose::new(DVec3::new(x, y, z), yaw, pitch))
            }
            [x, y, z] if [x, y, z].iter().all(|n| n.is_finite()) => {
                Some(Pose::new(DVec3::new(x, y, z), 0.0, 0.0))
            }
            _ => None,
        });
        let orbit = o.param("orbit").and_then(|v| match nums(v)[..] {
            [x, y, z] if [x, y, z].iter().all(|n| n.is_finite()) => Some(DVec3::new(x, y, z)),
            _ => None,
        });
        let mode = o.param("mode").and_then(Mode::from_key).unwrap_or(
            if o.fly.is_some() && cam.is_none() {
                Mode::Ride
            } else if orbit.is_some() {
                Mode::Orbit
            } else {
                Mode::Free
            },
        );
        Link {
            mode,
            cam,
            orbit,
            ride: o.fly,
            fog: flag(o.param("fog")),
            far: flag(o.param("far")),
            anim: flag(o.param("anim")),
            t: o.t,
            hide: o
                .param("hide")
                .map(|v| {
                    v.split(',')
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            speed: o
                .param("speed")
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| v.is_finite()),
            panel: flag(o.param("panel")),
        }
    }

    /// The query string, `level` first after `view`.
    pub fn query(&self, level: &str) -> String {
        let mut q = vec![
            "view=god".to_string(),
            format!("level={}", encode(level)),
            format!("mode={}", self.mode.key()),
        ];
        if let Some(p) = &self.cam {
            q.push(format!(
                "cam={},{},{},{},{}",
                fixed(p.pos.x, 2),
                fixed(p.pos.y, 2),
                fixed(p.pos.z, 2),
                fixed(p.yaw, 5),
                fixed(p.pitch, 5)
            ));
        }
        if self.mode == Mode::Orbit
            && let Some(c) = self.orbit
        {
            q.push(format!(
                "orbit={},{},{}",
                fixed(c.x, 2),
                fixed(c.y, 2),
                fixed(c.z, 2)
            ));
        }
        if self.mode == Mode::Ride
            && let Some(f) = &self.ride
        {
            q.push(format!("s={}", fixed(f.s, 1)));
            q.push(format!("h={}", fixed(f.h, 2)));
            q.push(format!("back={}", fixed(f.back, 2)));
            q.push(format!("lat={}", fixed(f.lat, 2)));
            q.push(format!("v={}", fixed(f.speed, 1)));
            q.push(format!("yaw={}", fixed(f.yaw, 5)));
            q.push(format!("pitch={}", fixed(f.pitch, 5)));
        }
        for (k, v) in [("fog", self.fog), ("far", self.far), ("anim", self.anim)] {
            if let Some(v) = v {
                q.push(format!("{k}={}", u8::from(v)));
            }
        }
        if let Some(t) = self.t {
            q.push(format!("t={}", fixed(t, 4)));
        }
        if !self.hide.is_empty() {
            let names: Vec<String> = self.hide.iter().map(|h| encode(h)).collect();
            q.push(format!("hide={}", names.join(",")));
        }
        if let Some(s) = self.speed {
            q.push(format!("speed={}", fixed(s, 1)));
        }
        if self.panel == Some(false) {
            q.push("panel=0".into());
        }
        q.join("&")
    }
}

/// `x` with at most `d` decimals, trailing zeros dropped (`-0` as `0`).
pub fn fixed(x: f64, d: usize) -> String {
    let s = format!("{x:.d$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

/// Percent-encoding for a query value: letters, digits and `-_.~:` kept.
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_round_trips() {
        let l = Link {
            mode: Mode::Orbit,
            cam: Some(Pose::new(DVec3::new(1234.5678, -3.25, 0.0), 1.234567, -0.5)),
            orbit: Some(DVec3::new(1300.0, 0.0, -20.126)),
            ride: None,
            fog: Some(false),
            far: Some(true),
            anim: Some(false),
            t: Some(0.35),
            hide: vec!["world sea".into(), "Mountain".into()],
            speed: Some(40.0),
            panel: Some(false),
        };
        let q = l.query("coast");
        assert!(q.starts_with("view=god&level=coast&mode=orbit&cam=1234.57,-3.25,0,1.23457,-0.5"));
        let o = Options::from_query(&q);
        assert!(wanted(&o));
        assert_eq!(o.level, "coast");
        let back = Link::parse(&o);
        assert_eq!(back.mode, Mode::Orbit);
        assert_eq!(back.hide, l.hide);
        assert_eq!(back.orbit, Some(DVec3::new(1300.0, 0.0, -20.13)));
        assert_eq!(
            (back.fog, back.far, back.anim),
            (Some(false), Some(true), Some(false))
        );
        assert_eq!(back.t, Some(0.35));
        assert_eq!(back.panel, Some(false));
        let c = back.cam.unwrap();
        assert_eq!((c.pos.x, c.yaw), (1234.57, 1.23457));
        // Its own link is itself.
        assert_eq!(back.query("coast"), q);
    }

    #[test]
    fn the_ride_is_the_fly_cameras_parameters() {
        let o = Options::from_query("view=god&level=sierra&s=300&h=8");
        let l = Link::parse(&o);
        assert_eq!(l.mode, Mode::Ride);
        assert_eq!(l.ride.unwrap().s, 300.0);
        let q = l.query("sierra");
        assert!(
            q.contains("&s=300&h=8&back=14&lat=0&v=0&yaw=0&pitch=-0.08"),
            "{q}"
        );
        assert_eq!(Link::parse(&Options::from_query(&q)), l);
        // Without parameters: free.
        let l = Link::parse(&Options::from_query("view=god"));
        assert_eq!(l.mode, Mode::Free);
        assert!(l.cam.is_none());
    }

    #[test]
    fn numbers_print_short() {
        assert_eq!(fixed(1.5, 2), "1.5");
        assert_eq!(fixed(-0.0001, 2), "0");
        assert_eq!(fixed(12.0, 3), "12");
        assert_eq!(encode("a b/c"), "a%20b%2Fc");
    }
}
