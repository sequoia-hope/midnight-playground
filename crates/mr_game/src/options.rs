//! What the client was asked to do: the page's query string on the web, and
//! the same names as a `--query` string (or flags) natively (SPEC 8.4).
//!
//! Names follow the JS game where it has them: `level`, and the debug fly
//! camera's `s`, `h`, `back`, `lat`, `v`, `yaw`, `pitch` (`src/main.js`
//! `fly`). `scene` (a `.mrscene` path or relative URL) and `hq` are the Rust
//! client's own until the menus exist (DECISIONS D102).

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
            out: None,
            smoke_race: false,
            query: Vec::new(),
        }
    }
}

/// Whether `id` names one of the menu's levels. `mr_levels::levels()` builds
/// every level (Streets' route among them) each call, and the race and fly
/// systems ask every frame, so the ids are kept once (DECISIONS D863).
pub fn is_level(id: &str) -> bool {
    static IDS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| mr_levels::levels().iter().map(|l| l.id).collect())
        .contains(&id)
}

/// Splits `a=1&b=2` (a leading `?` is ignored) into decoded pairs, in order.
pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.trim_start_matches('?')
        .split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (decode(k), decode(v))
        })
        .collect()
}

/// `%XX` and `+` decoding (URLSearchParams' rules).
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = |c: u8| (c as char).to_digit(16);
                match (hex(b[i + 1]), hex(b[i + 2])) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// JS `Number(x)` for the simple forms the debug parameters use; NaN when
/// it does not parse (as `Number('abc')`), 0 for an empty string.
fn number(v: &str) -> f64 {
    let t = v.trim();
    if t.is_empty() {
        return 0.0;
    }
    t.parse().unwrap_or(f64::NAN)
}

impl Options {
    /// Reads the query-string parameters into options.
    pub fn from_query(q: &str) -> Options {
        let pairs = parse_query(q);
        let get = |k: &str| pairs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
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
        o.query = pairs.clone();
        o.gpu_preprocessing = get("gpupre").map(|v| v == "1" || v == "true");
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
    /// `--smoke-test`, `--size WxH`. A bare `key=value` is a query pair.
    pub fn from_args(args: &[String]) -> Result<Options, String> {
        let mut query: Vec<String> = Vec::new();
        let mut screenshot = None;
        let mut after = None;
        let mut smoke = false;
        let mut size = None;
        let mut materials = None;
        let mut out = None;
        let mut stations = None;
        let mut smoke_race = false;
        let mut it = args.iter();
        while let Some(a) = it.next() {
            let mut val = |name: &str| {
                it.next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match a.as_str() {
                "--query" => query.push(val("--query")?),
                "--level" => query.push(format!("level={}", val("--level")?)),
                "--scene" => query.push(format!("scene={}", val("--scene")?)),
                "--screenshot" => screenshot = Some(val("--screenshot")?),
                "--after" => {
                    let v = val("--after")?;
                    after = Some(v.parse().map_err(|_| format!("--after {v}: not a count"))?);
                }
                "--size" => {
                    let v = val("--size")?;
                    let (w, h) = v
                        .split_once('x')
                        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                        .ok_or_else(|| format!("--size {v}: expected WxH"))?;
                    size = Some((w, h));
                }
                "--smoke-test" => smoke = true,
                "--smoke-race" => smoke_race = true,
                "--autodrive" => query.push("autodrive=1".into()),
                // The debug overlay and the run recording (D1020, D1021).
                "--debug" => query.push("debug=1".into()),
                "--record" => query.push("record=1".into()),
                "--materials" => materials = Some(val("--materials")?),
                "--out" => out = Some(val("--out")?),
                "--stations" => stations = Some(val("--stations")?),
                s if s.contains('=') && !s.starts_with('-') => query.push(s.to_owned()),
                other => return Err(format!("unknown argument `{other}`\n\n{}", usage())),
            }
        }
        let mut o = Options::from_query(&query.join("&"));
        o.screenshot = screenshot;
        if let Some(a) = after {
            o.after = a;
        }
        o.smoke_test = smoke;
        o.size = size;
        if materials.is_some() {
            o.materials = materials;
        }
        o.out = out;
        o.stations = stations;
        o.smoke_race = smoke_race;
        Ok(o)
    }
}

pub fn usage() -> &'static str {
    "usage: midnight-racer [--level <id>] [--scene <file.mrscene>] [--query \"s=300&h=5&v=30\"]\n\
     \x20                     [--screenshot <out.png> [--after <frames>]] [--smoke-test] [--size WxH]\n\
     \x20      midnight-racer --materials all|<name,...> --out <dir>\n\
     \x20      midnight-racer --level <id> --stations <stations.json> --out <dir> [--query freeze=1]\n\
     \x20      midnight-racer --level <id> --smoke-race [--query \"car=super&seed=2\"]\n\
     \n\
     levels: sierra coast streets desert seaside cruise, or models\n\
     query:  the JS game's names: level, s, h, back, lat, v, yaw, pitch, t, freeze=1; hq=0|1;\n\
     \x20       scene=<file>; mat=<names> (the material test scenes); cars=0|1 (stand-in cars);\n\
     \x20       gpupre=0|1 (Bevy's GPU preprocessing)\n\
     viewer: view=god (the level viewer, SPEC 8.6): mode=free|orbit|over|ride, cam=x,y,z,yaw,pitch,\n\
     \x20       orbit=x,y,z, fog=0|1, far=0|1, anim=0|1, t=<route fraction>, hide=<groups>, speed=N\n\
     race:   a level without s= is a race (race=0: the attract camera); car=<kind>, seed=N,\n\
     \x20       autodrive=1 (or --autodrive), timescale=N, pursuit=1, heat=N, touch=0|1,\n\
     \x20       shots=<dir> (save countdown, race and results PNGs, then exit)\n\
     debug:  debug=1 (or --debug): the frame-time overlay from the start; F3 shows and hides it\n\
     \x20       record=1 (or --record): record the run to recordings/<date>-<level>.jsonl\n\
     \x20       (record=<file.jsonl|dir> elsewhere); replay its races with `mr-sim replay <file>`"
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
        assert!(Options::from_args(&["--bogus".to_string()]).is_err());
    }
}
