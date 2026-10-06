//! Settings and records (SPEC 8.3): `main.js`'s `store` and `settings`.
//!
//! The JS keeps every setting in `localStorage` under `mr.<key>`, each value
//! JSON-encoded (`store.set(k, v)` is `localStorage.setItem('mr.' + k,
//! JSON.stringify(v))`, and `store.get(k, d)` gives `d` for a missing key or
//! one that does not parse). The Rust build is served from the same origin,
//! so it reads and writes the same keys in the same encoding: a player keeps
//! their car, options and best times across the two builds. Natively the
//! same key → value strings live in a JSON file in the user's config
//! directory ([`FileStore`]). The full table of keys is in
//! `inventory-ui-input-tests.md` section 3.

use bevy::prelude::Resource;
use serde_json::Value;

/// Where the strings live: `localStorage`'s `getItem` and `setItem`.
pub trait Backend: Send + Sync + 'static {
    fn get_item(&self, key: &str) -> Option<String>;
    fn set_item(&mut self, key: &str, value: &str);
}

/// The JS's key prefix.
pub const PREFIX: &str = "mr.";

/// `store.get` and `store.set`, over a backend.
#[derive(Resource)]
pub struct Store {
    backend: Box<dyn Backend>,
}

impl Store {
    pub fn new(backend: impl Backend) -> Store {
        Store {
            backend: Box::new(backend),
        }
    }

    /// An empty store in memory (tests, and where nothing persists).
    pub fn memory() -> Store {
        Store::new(Memory::default())
    }

    /// The platform's store: `localStorage` on the web, a file natively.
    pub fn platform() -> Store {
        #[cfg(target_arch = "wasm32")]
        {
            Store::new(LocalStorage)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            match FileStore::default_path() {
                Some(p) => Store::new(FileStore::open(p)),
                None => Store::memory(),
            }
        }
    }

    /// The raw string under `mr.<k>`.
    pub fn raw(&self, k: &str) -> Option<String> {
        self.backend.get_item(&format!("{PREFIX}{k}"))
    }

    /// `store.get(k, d)` without the default: the parsed value, or `None`
    /// when the key is missing or does not parse (`JSON.parse` threw).
    pub fn get(&self, k: &str) -> Option<Value> {
        serde_json::from_str(&self.raw(k)?).ok()
    }

    /// `store.set(k, v)`: `JSON.stringify(v)` under `mr.<k>`.
    pub fn set(&mut self, k: &str, v: &Value) {
        let s = stringify(v);
        self.backend.set_item(&format!("{PREFIX}{k}"), &s);
    }

    /// A number, else `d` (a missing key, `null` or another type).
    pub fn num(&self, k: &str, d: f64) -> f64 {
        self.get(k).and_then(|v| v.as_f64()).unwrap_or(d)
    }

    /// A number or `null` (`store.get(k, null)` for best times).
    pub fn num_or_null(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(|v| v.as_f64())
    }

    pub fn bool(&self, k: &str, d: bool) -> bool {
        self.get(k).and_then(|v| v.as_bool()).unwrap_or(d)
    }

    pub fn string(&self, k: &str, d: &str) -> String {
        match self.get(k) {
            Some(Value::String(s)) => s,
            _ => d.to_owned(),
        }
    }

    pub fn set_num(&mut self, k: &str, v: f64) {
        self.set(k, &num_value(v));
    }

    pub fn set_bool(&mut self, k: &str, v: bool) {
        self.set(k, &Value::Bool(v));
    }

    pub fn set_str(&mut self, k: &str, v: &str) {
        self.set(k, &Value::String(v.to_owned()));
    }
}

/// A number as a JSON value; NaN and the infinities are `null`, as
/// `JSON.stringify` writes them.
pub fn num_value(v: f64) -> Value {
    serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// `JSON.stringify(v)`: no spaces, numbers as JS prints them
/// (`Number.prototype.toString`: `1` not `1.0`, `1e+21`, `1e-7`), strings
/// escaped as JSON does.
pub fn stringify(v: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, v);
    out
}

fn write_value(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_f64() {
            Some(f) if n.is_f64() => out.push_str(&js_number(f)),
            _ => out.push_str(&n.to_string()),
        },
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(k).unwrap_or_default());
                out.push(':');
                write_value(out, x);
            }
            out.push('}');
        }
    }
}

/// `Number.prototype.toString()` for a finite number (ECMA-262
/// Number::toString): the shortest digits that round-trip, written plainly
/// for exponents from -7 to 20 and in exponent form outside them. NaN and
/// the infinities are `null` (as JSON writes them).
pub fn js_number(x: f64) -> String {
    if !x.is_finite() {
        return "null".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x < 0.0 {
        return format!("-{}", js_number(-x));
    }
    // Rust's `{:e}` is the shortest round-trip digits: "1.2345e3".
    let e = format!("{x:e}");
    let (mant, exp) = e.split_once('e').unwrap_or((&e, "0"));
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    // x = 0.digits × 10^n
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let rest = if k > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits.clone()
        };
        format!("{rest}e{sign}{}", (n - 1).abs())
    }
}

/// A store in memory, in insertion order.
#[derive(Default, Clone, Debug)]
pub struct Memory {
    pub items: Vec<(String, String)>,
}

impl Backend for Memory {
    fn get_item(&self, key: &str) -> Option<String> {
        self.items
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn set_item(&mut self, key: &str, value: &str) {
        match self.items.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.to_owned(),
            None => self.items.push((key.to_owned(), value.to_owned())),
        }
    }
}

/// `window.localStorage`. Every access is wrapped as the JS wraps it: a
/// browser in private mode, or with storage blocked, throws, and the game
/// carries on with its defaults.
#[cfg(target_arch = "wasm32")]
pub struct LocalStorage;

#[cfg(target_arch = "wasm32")]
impl LocalStorage {
    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }
}

#[cfg(target_arch = "wasm32")]
impl Backend for LocalStorage {
    fn get_item(&self, key: &str) -> Option<String> {
        Self::storage()?.get_item(key).ok()?
    }

    fn set_item(&mut self, key: &str, value: &str) {
        if let Some(s) = Self::storage() {
            let _ = s.set_item(key, value);
        }
    }
}

/// Natively: the same key → string pairs as `localStorage` holds, as one
/// JSON object in a file, rewritten on every change (a few hundred bytes).
#[cfg(not(target_arch = "wasm32"))]
pub struct FileStore {
    path: std::path::PathBuf,
    mem: Memory,
}

#[cfg(not(target_arch = "wasm32"))]
impl FileStore {
    /// `$MR_STORE`, else `storage.json` in the user's config directory
    /// (`$XDG_CONFIG_HOME` or `~/.config` on Linux, `~/Library/Application
    /// Support` on macOS, `%APPDATA%` on Windows) under `midnight-racer/`.
    pub fn default_path() -> Option<std::path::PathBuf> {
        use std::path::PathBuf;
        if let Some(p) = std::env::var_os("MR_STORE") {
            return Some(PathBuf::from(p));
        }
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let dir = if cfg!(windows) {
            env("APPDATA")?
        } else if cfg!(target_os = "macos") {
            env("HOME")?.join("Library/Application Support")
        } else {
            env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))?
        };
        Some(dir.join("midnight-racer").join("storage.json"))
    }

    /// Reads the file if there is one (a missing or broken file is an empty
    /// store).
    pub fn open(path: std::path::PathBuf) -> FileStore {
        let mut mem = Memory::default();
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&text)
        {
            for (k, v) in m {
                if let Value::String(s) = v {
                    mem.items.push((k, s));
                }
            }
        }
        FileStore { path, mem }
    }

    fn save(&self) -> std::io::Result<()> {
        // In insertion order, as `localStorage` keeps them.
        let mut text = String::from("{");
        for (i, (k, v)) in self.mem.items.iter().enumerate() {
            if i > 0 {
                text.push(',');
            }
            text.push_str(&stringify(&Value::String(k.clone())));
            text.push(':');
            text.push_str(&stringify(&Value::String(v.clone())));
        }
        text.push('}');
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Backend for FileStore {
    fn get_item(&self, key: &str) -> Option<String> {
        self.mem.get_item(key)
    }

    fn set_item(&mut self, key: &str, value: &str) {
        self.mem.set_item(key, value);
        if let Err(e) = self.save() {
            bevy::log::warn!("settings: {}: {e}", self.path.display());
        }
    }
}

/// The car kinds (`CAR_SPECS` keys), in the menu's order.
pub fn car_kinds() -> impl Iterator<Item = &'static str> {
    mr_sim::physics::CAR_SPECS.iter().map(|(k, _)| *k)
}

/// Steering choices on a touch screen (`#opt-steer`).
pub const STEERING: [&str; 3] = ["stick", "buttons", "tilt"];
/// Pedal choices (`#opt-pedals`).
pub const PEDALS: [&str; 2] = ["slider", "buttons"];
/// Guide line choices (Rust only, D1083): the whole line, only where it
/// says brake, none.
pub const GUIDE: [&str; 3] = ["full", "brake", "off"];
/// Steering assist choices (Rust only, D1083).
pub const ASSIST: [&str; 3] = ["off", "light", "strong"];

/// `main.js`'s `settings`, read from the store at start.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `mr.musicVol`, 0..1.
    pub music: f64,
    /// `mr.sfxVol`, 0..1.
    pub sfx: f64,
    pub mph: bool,
    /// High quality: shadows and the pixel ratio (`applyQuality`).
    pub hq: bool,
    pub autogas: bool,
    /// `stick`, `buttons` or `tilt`.
    pub steering: String,
    pub tilt_sens: f64,
    /// `slider` or `buttons`.
    pub pedals: String,
    pub fullscreen: bool,
    pub car: String,
    pub level: String,
    /// Music: `auto` (the level's own track) or a track id.
    pub track: String,
    /// Police lights strobe (off: they glow steadily).
    pub flash: bool,
    /// Gamepad rumble.
    pub rumble: bool,
    /// `mr.guideLine` (Rust only): `full`, `brake` or `off`; on (`full`)
    /// by default on a touch screen (D1083).
    pub guide: String,
    /// `mr.steerAssist` (Rust only): `off`, `light` or `strong`; `light` by
    /// default on a touch screen (D1083).
    pub assist: String,
}

impl Settings {
    /// `const settings = { music: store.get('musicVol', 0.7), … }`, with
    /// the menu's checks: a steering or pedal choice the select does not
    /// offer becomes the default (touch screens only, as the JS checks them
    /// only there), a track that is not in the picker becomes `auto`. A car
    /// or level that does not exist becomes the default, as `levelById`
    /// falls back to the first level. Nothing is written back.
    pub fn load(store: &Store, touch_ui: bool) -> Settings {
        let legacy_tilt = store.bool("tilt", false);
        let mut s = Settings {
            music: store.num("musicVol", 0.7),
            sfx: store.num("sfxVol", 0.85),
            mph: store.bool("mph", true),
            hq: store.bool("hq", !touch_ui),
            autogas: store.bool("autogas", false),
            steering: store.string("steering", if legacy_tilt { "tilt" } else { "stick" }),
            tilt_sens: store.num("tiltSens", 0.5),
            pedals: store.string("pedals", "slider"),
            fullscreen: store.bool("fullscreen", true),
            car: store.string("car", "sports"),
            level: store.string("level", "sierra"),
            track: store.string("track", "auto"),
            flash: store.bool("flash", true),
            rumble: store.bool("rumble", true),
            guide: store.string("guideLine", if touch_ui { "full" } else { "off" }),
            assist: store.string("steerAssist", if touch_ui { "light" } else { "off" }),
        };
        if !GUIDE.contains(&s.guide.as_str()) {
            s.guide = if touch_ui { "full" } else { "off" }.into();
        }
        if !ASSIST.contains(&s.assist.as_str()) {
            s.assist = if touch_ui { "light" } else { "off" }.into();
        }
        if touch_ui && !STEERING.contains(&s.steering.as_str()) {
            s.steering = "stick".into();
        }
        if touch_ui && !PEDALS.contains(&s.pedals.as_str()) {
            s.pedals = "slider".into();
        }
        if s.track != "auto" && !mr_audio::tracks::TRACKS.iter().any(|t| t.id == s.track) {
            s.track = "auto".into();
        }
        if !car_kinds().any(|k| k == s.car) {
            s.car = "sports".into();
        }
        if !mr_levels::levels().iter().any(|l| l.id == s.level) {
            s.level = mr_levels::levels()[0].id.into();
        }
        s
    }
}

/// `modeFor(l)`: Race or Hot Pursuit, per level; only levels with police
/// offer the choice.
pub fn mode_for(store: &Store, level: &mr_track::Level) -> &'static str {
    if level.police.is_some() && store.string(&format!("mode.{}", level.id), "race") == "pursuit" {
        "pursuit"
    } else {
        "race"
    }
}

/// `bestKey(l)`: winning times are kept apart for Hot Pursuit.
pub fn best_key(id: &str, pursuit: bool) -> String {
    format!("best.{id}{}", if pursuit { ".pursuit" } else { "" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seeded(pairs: &[(&str, &str)]) -> Store {
        let mut m = Memory::default();
        for (k, v) in pairs {
            m.set_item(k, v);
        }
        Store::new(m)
    }

    #[test]
    fn numbers_as_js_prints_them() {
        for (x, s) in [
            (0.7, "0.7"),
            (0.85, "0.85"),
            (1.0, "1"),
            (0.0, "0"),
            (-0.0, "0"),
            (12345.0, "12345"),
            (83.456_789_012_345_67, "83.45678901234567"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (1e-7, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (-2.5, "-2.5"),
            (123.0e-2, "1.23"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
        ] {
            assert_eq!(js_number(x), s, "{x:e}");
        }
    }

    #[test]
    fn values_as_json_stringify_writes_them() {
        assert_eq!(stringify(&json!("sports")), "\"sports\"");
        assert_eq!(stringify(&json!(true)), "true");
        assert_eq!(stringify(&num_value(0.35)), "0.35");
        assert_eq!(stringify(&num_value(f64::NAN)), "null");
        assert_eq!(stringify(&json!("a\"b\\c\n\u{1}")), r#""a\"b\\c\n\u0001""#);
        assert_eq!(
            stringify(
                &json!({"Pad (STANDARD GAMEPAD)": {"left": [{"axis": 0, "dir": -1, "rest": 0}], "throttle": [{"button": 7}]}})
            ),
            r#"{"Pad (STANDARD GAMEPAD)":{"left":[{"axis":0,"dir":-1,"rest":0}],"throttle":[{"button":7}]}}"#
        );
    }

    /// Every key of `main.js`'s settings, under its JS name, with the JS
    /// defaults when the store is empty.
    #[test]
    fn defaults_and_the_js_keys() {
        let store = Store::memory();
        let s = Settings::load(&store, false);
        assert_eq!(
            s,
            Settings {
                music: 0.7,
                sfx: 0.85,
                mph: true,
                hq: true,
                autogas: false,
                steering: "stick".into(),
                tilt_sens: 0.5,
                pedals: "slider".into(),
                fullscreen: true,
                car: "sports".into(),
                level: "sierra".into(),
                track: "auto".into(),
                flash: true,
                rumble: true,
                guide: "off".into(),
                assist: "off".into(),
            }
        );
        // Touch screens get lighter rendering by default, and the guide
        // line and the steering assist (D1083).
        let t = Settings::load(&store, true);
        assert!(!t.hq);
        assert_eq!((t.guide.as_str(), t.assist.as_str()), ("full", "light"));

        let store = seeded(&[
            ("mr.musicVol", "0.25"),
            ("mr.sfxVol", "0"),
            ("mr.mph", "false"),
            ("mr.hq", "false"),
            ("mr.autogas", "true"),
            ("mr.steering", "\"buttons\""),
            ("mr.tiltSens", "0.8"),
            ("mr.pedals", "\"buttons\""),
            ("mr.fullscreen", "false"),
            ("mr.car", "\"rally\""),
            ("mr.level", "\"seaside\""),
            ("mr.track", "\"neon-rush\""),
            ("mr.flash", "false"),
            ("mr.rumble", "false"),
            ("mr.guideLine", "\"brake\""),
            ("mr.steerAssist", "\"off\""),
        ]);
        let s = Settings::load(&store, true);
        assert_eq!(
            s,
            Settings {
                music: 0.25,
                sfx: 0.0,
                mph: false,
                hq: false,
                autogas: true,
                steering: "buttons".into(),
                tilt_sens: 0.8,
                pedals: "buttons".into(),
                fullscreen: false,
                car: "rally".into(),
                level: "seaside".into(),
                track: "neon-rush".into(),
                flash: false,
                rumble: false,
                guide: "brake".into(),
                assist: "off".into(),
            }
        );
        // A choice the menu does not offer falls back to the default.
        let store = seeded(&[("mr.guideLine", "\"rainbow\""), ("mr.steerAssist", "3")]);
        let s = Settings::load(&store, false);
        assert_eq!((s.guide.as_str(), s.assist.as_str()), ("off", "off"));
    }

    /// `store.get(k, d)` gives `d` for a value that does not parse, as the
    /// JS's try/catch does; the legacy `mr.tilt` checkbox picks tilt
    /// steering when `mr.steering` was never saved; choices the menu does
    /// not offer fall back.
    #[test]
    fn broken_legacy_and_unknown_values() {
        let store = seeded(&[
            ("mr.musicVol", "not json"),
            ("mr.tilt", "true"),
            ("mr.car", "\"hovercraft\""),
            ("mr.level", "\"moon\""),
            ("mr.track", "\"nope\""),
            ("mr.pedals", "\"pogo\""),
        ]);
        let s = Settings::load(&store, true);
        assert_eq!(s.music, 0.7);
        assert_eq!(s.steering, "tilt");
        assert_eq!(s.car, "sports");
        assert_eq!(s.level, "sierra");
        assert_eq!(s.track, "auto");
        assert_eq!(s.pedals, "slider");
        // The JS checks the selects only on touch screens.
        assert_eq!(Settings::load(&store, false).pedals, "pogo");
        let store = seeded(&[("mr.tilt", "true"), ("mr.steering", "\"stick\"")]);
        assert_eq!(Settings::load(&store, true).steering, "stick");
    }

    /// What `store.set` writes is what the JS writes, under `mr.` and
    /// JSON-encoded, and what the JS wrote reads back.
    #[test]
    fn writes_are_the_js_strings() {
        let mut store = Store::memory();
        store.set_str("car", "muscle");
        store.set_str("level", "cruise");
        store.set_bool("mph", false);
        store.set_num("musicVol", 0.42);
        store.set_num("best.sierra", 183.456_666_666_666_7);
        store.set_num("bestScore.cruise", 12345.0);
        store.set_str("mode.coast", "pursuit");
        store.set_num(&best_key("coast", true), 201.25);
        assert_eq!(store.raw("car").as_deref(), Some("\"muscle\""));
        assert_eq!(store.raw("level").as_deref(), Some("\"cruise\""));
        assert_eq!(store.raw("mph").as_deref(), Some("false"));
        assert_eq!(store.raw("musicVol").as_deref(), Some("0.42"));
        assert_eq!(
            store.raw("best.sierra").as_deref(),
            Some("183.4566666666667")
        );
        assert_eq!(store.raw("bestScore.cruise").as_deref(), Some("12345"));
        assert_eq!(store.raw("mode.coast").as_deref(), Some("\"pursuit\""));
        assert_eq!(store.raw("best.coast.pursuit").as_deref(), Some("201.25"));
        assert_eq!(
            store.num_or_null("best.sierra"),
            Some(183.456_666_666_666_7)
        );
        assert_eq!(store.num_or_null("bestLap.seaside"), None);
        let coast = mr_levels::level_by_id("coast");
        assert!(coast.police.is_some());
        assert_eq!(mode_for(&store, &coast), "pursuit");
        // A level without police is always a race.
        let seaside = mr_levels::level_by_id("seaside");
        store.set_str("mode.seaside", "pursuit");
        assert_eq!(mode_for(&store, &seaside), "race");
        assert_eq!(best_key("sierra", false), "best.sierra");
    }

    /// The native file holds the same key → string pairs, and a new store
    /// over it reads them back.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_file_store_round_trips() {
        let dir = std::env::temp_dir().join(format!("mr-store-test-{}", std::process::id()));
        let path = dir.join("storage.json");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = Store::new(FileStore::open(path.clone()));
            s.set_str("car", "electric");
            s.set_num("tiltSens", 0.3);
            s.set_bool("hq", false);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            r#"{"mr.car":"\"electric\"","mr.tiltSens":"0.3","mr.hq":"false"}"#
        );
        let s = Store::new(FileStore::open(path.clone()));
        assert_eq!(s.raw("car").as_deref(), Some("\"electric\""));
        assert_eq!(s.raw("tiltSens").as_deref(), Some("0.3"));
        let set = Settings::load(&s, false);
        assert_eq!(
            (set.car.as_str(), set.tilt_sens, set.hq),
            ("electric", 0.3, false)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
