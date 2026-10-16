//! What the client was asked to do: the page's query string on the web, and
//! the same names as a `--query` string (or flags) natively (SPEC 8.4), as
//! Playground Kit's [`Params`] reads them (D1234); this module is the
//! game's typed view of those parameters.
//!
//! Names follow the JS game where it has them: `level`, and the debug fly
//! camera's `s`, `h`, `back`, `lat`, `v`, `yaw`, `pitch` (`src/main.js`
//! `fly`). `scene` (a `.mrscene` path or relative URL) and `hq` are the Rust
//! client's own until the menus exist (DECISIONS D102).

pub use pk_shell::params::{Params, number, parse_query};

/// The debug fly camera's parameters (`main.js` `fly`), with its defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlyParams {
    pub s: f64,
    pub h: f64,
    pub back: f64,
    pub lat: f64,
    /// `v`: metres per second along the route.
    pub speed: f64,
    pub yaw: f64,
    pub pitch: f64,
}

impl FlyParams {
    /// The menu's attract camera (`main.js` `tick`, the `else` branch): it
    /// drifts along the first zone at 16 m/s from s = 120.
    pub const ATTRACT: FlyParams = FlyParams {
        s: 120.0,
        h: 7.0,
        back: 22.0,
        lat: 3.0,
        speed: 0.0,
        yaw: 0.0,
        pitch: -0.05,
    };
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// Level id (`sierra`, `coast`, `streets`, `desert`, `seaside`, `cruise`),
    /// or `models` for the car models scene.
    pub level: String,
    /// An explicit scene file: a path natively, a URL relative to the page
    /// on the web. Default: the level's export in the parity cache.
    pub scene: Option<String>,
    /// `?s=` given: the debug fly camera, as in the JS. Otherwise the attract
    /// camera.
    pub fly: Option<FlyParams>,
    /// High quality (the JS `hq` setting): shadows on. Default on, except on
    /// touch devices on the web (as the JS).
    pub hq: Option<bool>,
    /// Native: write a PNG of the window after `after` frames, then exit.
    pub screenshot: Option<String>,
    /// Frames to render after the scene is up before the screenshot.
    pub after: u32,
    /// Native: run to the first frame of the loaded scene, report, exit.
    pub smoke_test: bool,
    /// Window size natively (`--size 1280x800`).
    pub size: Option<(u32, u32)>,
    /// `?freeze=1` (the JS hook): scenery and the sky's clock hold still.
    pub freeze: bool,
    /// `?t=`: the time of day pinned at this fraction of the route.
    pub t: Option<f64>,
    /// `?mat=<names>` / `--materials <names>`: render the material test
    /// scenes (`parity/golden/materials/scenes.json`) instead of a level:
    /// `all`, or names separated by commas.
    pub materials: Option<String>,
    /// Natively: fly to each station of a `stations.json` (as
    /// `tools/parity/shots.mjs` writes it), save a PNG of each in `out`.
    pub stations: Option<String>,
    /// `?cars=0|1`: the stand-in cars the simulation drives (WP 2.4). By
    /// default on for a level, off with `freeze=1` (parity captures), the
    /// material scenes and the stations.
    pub cars: Option<bool>,
    /// `?gpupre=0|1`: Bevy's GPU preprocessing (mesh uniforms built and
    /// culled by compute, indirect draws) where the device has compute.
    /// Default off (DECISIONS D396).
    pub gpu_preprocessing: Option<bool>,
    /// `?scale=`: the render scale on the web (D1194), a fraction of the
    /// race's pixel ratio (`hq ? min(devicePixelRatio, 1.5) : 1`, D437),
    /// 0.25 to 1; a value above 1 is read as a percentage. Default: the
    /// saved Render scale setting.
    pub scale: Option<f64>,
    /// Natively, with `materials` or `stations`: the directory the PNGs go to (one
    /// directory per group, as `tools/parity/materials.mjs` writes them).
    pub out: Option<String>,
    /// Natively (`--smoke-race`): race the level headless with the
    /// autopilot, print the results, exit (no window, no GPU).
    pub smoke_race: bool,
    /// Every query pair as given, in order (for the parameters the
    /// modules read themselves: `play`'s `car`, `seed`, `autodrive`, …).
    pub query: Vec<(String, String)>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            level: "sierra".into(),
            scene: None,
            fly: None,
            hq: None,
            screenshot: None,
            after: 10,
            smoke_test: false,
            size: None,
            freeze: false,
            t: None,
            materials: None,
            stations: None,
            cars: None,
            gpu_preprocessing: None,
            scale: None,
            out: None,
            smoke_race: false,
            query: Vec::new(),
        }
    }
}

/// Whether `id` names one of the menu's levels. `mp_levels::levels()` builds
/// every level (Streets' route among them) each call, and the race and fly
/// systems ask every frame, so the ids are kept once (DECISIONS D863).
pub fn is_level(id: &str) -> bool {
    static IDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| mp_levels::levels().iter().map(|l| l.id).collect())
        .contains(&id)
}

impl Options {
    /// Reads the query-string parameters into options.
    pub fn from_query(q: &str) -> Options {
        Options::from_params(&Params::from_query(q))
    }

    /// The options these parameters ask for (the flags natively are
    /// parameters too, [`ALIASES`]).
    pub fn from_params(p: &Params) -> Options {
        let get = |k: &str| p.get(k);
        let mut o = Options::default();
        if let Some(l) = get("level") {
            o.level = l.to_owned();
        }
        o.scene = get("scene").map(str::to_owned);
        if let Some(s) = get("s") {
            // `Number(params.get('h') ?? 5)` and so on.
            let or = |k: &str, d: f64| get(k).map(number).unwrap_or(d);
            o.fly = Some(FlyParams {
                s: number(s),
                h: or("h", 5.0),
                back: or("back", 14.0),
                lat: or("lat", 0.0),
                speed: or("v", 0.0),
                yaw: or("yaw", 0.0),
                pitch: or("pitch", -0.08),
            });
        }
        o.hq = get("hq").map(|v| v == "1" || v == "true");
        o.freeze = get("freeze").is_some_and(|v| v == "1");
        o.t = get("t").map(number).filter(|t| t.is_finite());
        o.materials = get("mat").map(str::to_owned);
        o.cars = get("cars").map(|v| v == "1" || v == "true");
        o.query = p.pairs().to_vec();
        o.gpu_preprocessing = get("gpupre").map(|v| v == "1" || v == "true");
        o.scale = get("scale").map(number).and_then(|v| {
            let v = if v > 1.0 { v / 100.0 } else { v };
            v.is_finite().then_some(v.clamp(0.25, 1.0))
        });
        // The native flags (`ALIASES`), as parameters.
        o.screenshot = get("screenshot").map(str::to_owned);
        if let Some(a) = p.parse::<u32>("after") {
            o.after = a;
        }
        o.smoke_test = p.flag("smoke-test");
        o.smoke_race = p.flag("smoke-race");
        o.size = get("size").and_then(|v| {
            v.split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
        });
        o.out = get("out").map(str::to_owned);
        o.stations = get("stations").map(str::to_owned);
        o
    }

    /// A query parameter (the first of that name).
    pub fn param(&self, k: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the stand-in cars run (`cars`, else the default above). Never
    /// beside a race, which draws its own; not in the level viewer unless
    /// asked (`?cars=1`).
    pub fn cars_on(&self) -> bool {
        !self.race_on()
            && self.cars.unwrap_or(
                !self.freeze
                    && self.materials.is_none()
                    && self.stations.is_none()
                    && !self.viewer(),
            )
    }

    /// The level viewer (`?view=god`, SPEC 8.6): never a race.
    pub fn viewer(&self) -> bool {
        self.param("view") == Some("god")
    }

    /// Whether this is a race (`play`, roadmap M4): `?race=1|0`, else a
    /// race for a level when no fly camera, `freeze`, material scenes or
    /// stations are asked for (DECISIONS D432).
    pub fn race_on(&self) -> bool {
        is_level(&self.level)
            && !self.viewer()
            && match self.param("race") {
                Some(v) => v == "1",
                None => {
                    self.fly.is_none()
                        && !self.freeze
                        && self.materials.is_none()
                        && self.stations.is_none()
                }
            }
    }

    /// Native command line: `--query "level=sierra&s=300"`, `--level`,
    /// `--scene <file>`, `--screenshot <png>`, `--after <frames>`,
    /// `--smoke-test`, `--size WxH` ([`ALIASES`]). A bare `key=value` is a
    /// query pair.
    pub fn from_args(args: &[String]) -> Result<Options, String> {
        let p = Params::from_args(args, &ALIASES).map_err(|e| format!("{e}\n\n{}", usage()))?;
        if let Some(v) = p.get("after")
            && v.parse::<u32>().is_err()
        {
            return Err(format!("--after {v}: not a count"));
        }
        if let Some(v) = p.get("size")
            && !v
                .split_once('x')
                .is_some_and(|(w, h)| w.parse::<u32>().is_ok() && h.parse::<u32>().is_ok())
        {
            return Err(format!("--size {v}: expected WxH"));
        }
        Ok(Options::from_params(&p))
    }
}

/// The command line's spelling of the parameters (`pk_shell::params`):
/// a flag alone, a flag with a value, and the links (which hold `?`, `&`
/// and `#`, so they go in escaped): a host's WebSocket or an invitation,
/// and the signalling server for hosting (MULTIPLAYER 8). The debug
/// overlay and the run recording are D1020 and D1021.
pub const ALIASES: pk_shell::params::Aliases = pk_shell::params::Aliases {
    flags: &[
        ("--smoke-test", "smoke-test"),
        ("--smoke-race", "smoke-race"),
        ("--autodrive", "autodrive"),
        ("--debug", "debug"),
        ("--record", "record"),
    ],
    valued: &[
        ("--level", "level"),
        ("--scene", "scene"),
        ("--screenshot", "screenshot"),
        ("--after", "after"),
        ("--size", "size"),
        ("--materials", "mat"),
        ("--out", "out"),
        ("--stations", "stations"),
    ],
    escaped: &[("--join", "join"), ("--signal", "signal")],
};

pub fn usage() -> &'static str {
    "usage: midnight-playground [--level <id>] [--scene <file.mrscene>] [--query \"s=300&h=5&v=30\"]\n\
     \x20                     [--screenshot <out.png> [--after <frames>]] [--smoke-test] [--size WxH]\n\
     \x20      midnight-playground --materials all|<name,...> --out <dir>\n\
     \x20      midnight-playground --level <id> --stations <stations.json> --out <dir> [--query freeze=1]\n\
     \x20      midnight-playground --level <id> --smoke-race [--query \"car=super&seed=2\"]\n\
     \n\
     levels: sierra coast streets desert seaside cruise, or models\n\
     query:  the JS game's names: level, s, h, back, lat, v, yaw, pitch, t, freeze=1; hq=0|1;\n\
     \x20       scene=<file>; mat=<names> (the material test scenes); cars=0|1 (stand-in cars);\n\
     \x20       gpupre=0|1 (Bevy's GPU preprocessing); scale=0.25..1 (the web's render scale)\n\
     viewer: view=god (the level viewer, SPEC 8.6): mode=free|orbit|over|ride, cam=x,y,z,yaw,pitch,\n\
     \x20       orbit=x,y,z, fog=0|1, far=0|1, anim=0|1, t=<route fraction>, hide=<groups>, speed=N\n\
     race:   a level without s= is a race (race=0: the attract camera); car=<kind>, seed=N,\n\
     \x20       autodrive=1 (or --autodrive), timescale=N, pursuit=1, cruise=1 (the open world with no race on it), heat=N, touch=0|1,\n\
     \x20       camera=chase|far|bumper (the camera mode it starts in),\n\
     \x20       shots=<dir> (save countdown, race and results PNGs, then exit)\n\
     multi:  --join ws://host:port/ws (an mp-host) or --join '<invitation link>';\n\
     \x20       --signal wss://…/ (a signalling server, to host a game from the menu)\n\
     debug:  debug=1 (or --debug): the frame-time overlay from the start; F3 shows and hides it\n\
     \x20       record=1 (or --record): record the run to recordings/<date>-<level>.jsonl\n\
     \x20       (record=<file.jsonl|dir> elsewhere); replay its races with `mp-sim replay <file>`"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fly_defaults_follow_the_js() {
        let o = Options::from_query("?level=coast&s=300");
        assert_eq!(o.level, "coast");
        assert_eq!(
            o.fly,
            Some(FlyParams {
                s: 300.0,
                h: 5.0,
                back: 14.0,
                lat: 0.0,
                speed: 0.0,
                yaw: 0.0,
                pitch: -0.08
            })
        );
        assert_eq!(Options::from_query("level=desert").fly, None);
    }

    #[test]
    fn args_and_decoding() {
        let args: Vec<String> = [
            "--level",
            "seaside",
            "--query",
            "s=10&v=30&scene=a%20b+c",
            "--after",
            "3",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let o = Options::from_args(&args).unwrap();
        assert_eq!(o.level, "seaside");
        assert_eq!(o.scene.as_deref(), Some("a b c"));
        assert_eq!(o.fly.unwrap().speed, 30.0);
        assert_eq!(o.after, 3);
        // An invitation keeps its `?`, `&` and `#` (MULTIPLAYER 8).
        let link = "https://x/?signal=wss%3A%2F%2Fs%2F&a=1#join=AAAA.BBBB";
        let o = Options::from_args(&[
            "--join".into(),
            link.into(),
            "--signal".into(),
            "wss://s/".into(),
        ])
        .unwrap();
        assert_eq!(o.param("join"), Some(link));
        assert_eq!(o.param("signal"), Some("wss://s/"));
        assert!(Options::from_args(&["--bogus".to_string()]).is_err());
    }
}
