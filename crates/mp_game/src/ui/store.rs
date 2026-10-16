//! Settings and records (SPEC 8.3): `main.js`'s `store` and `settings`.
//!
//! The JS keeps every setting in `localStorage` under `mr.<key>`, each value
//! JSON-encoded (`store.set(k, v)` is `localStorage.setItem('mr.' + k,
//! JSON.stringify(v))`, and `store.get(k, d)` gives `d` for a missing key or
//! one that does not parse). The Rust build is served from the same origin,
//! so it reads and writes the same keys in the same encoding: a player keeps
//! their car, options and best times across the two builds. Natively the
//! same key → value strings live in a JSON file in the user's config
//! directory. The store itself is Playground Kit's `pk_shell::store`
//! (D1234: the `Backend` trait, the JS-exact `stringify`, `Memory`,
//! `LocalStorage`, the atomic `FileStore`); this module is its
//! configuration and `main.js`'s `settings` on top. The full table of keys
//! is in `inventory-ui-input-tests.md` section 3.

#[cfg(test)]
use serde_json::Value;

pub use pk_shell::store::{Backend, Memory, Store, StoreConfig, js_number, num_value, stringify};

/// The JS's key prefix.
pub const PREFIX: &str = "mr.";

/// Where the store lives: `mr.` keys; natively `$MP_STORE`, else
/// `storage.json` under `midnight-playground/` in the user's config
/// directory, filled from the old name's folder (`midnight-racer/`: this
/// store and the window's state) the first time it is missing, so the
/// rename (D1100) keeps the player's settings and best times.
pub const STORE_CONFIG: StoreConfig = StoreConfig {
    prefix: PREFIX,
    app_dir: "midnight-playground",
    env_var: "MP_STORE",
    legacy_dirs: &["midnight-racer"],
};

/// The platform's store: `localStorage` on the web, the file natively.
pub fn platform() -> Store {
    Store::platform(&STORE_CONFIG)
}

/// An empty store in memory (tests, and where nothing persists).
pub fn memory() -> Store {
    Store::memory(PREFIX)
}

/// The car kinds (`CAR_SPECS` keys), in the menu's order.
pub fn car_kinds() -> impl Iterator<Item = &'static str> {
    mp_sim::physics::CAR_SPECS.iter().map(|(k, _)| *k)
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
/// Render scale choices, in percent (the web only, D1194).
pub const SCALES: [&str; 3] = ["100", "75", "50"];

/// A render scale setting as the fraction the pixel ratio is multiplied by.
pub fn scale_of(v: &str) -> f64 {
    v.parse::<f64>()
        .map_or(1.0, |p| (p / 100.0).clamp(0.25, 1.0))
}

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
    /// `mr.classicEngine` (Rust only): the player's engine on the old
    /// wavetable voice instead of the physical exhaust model (D1110).
    pub classic_engine: bool,
    /// `mr.station` (Rust only, D1151): what plays. `auto` (the level's
    /// station), a station key (`mp_music::radio::STATIONS`), or
    /// `playlist` (the seven arranged songs, as the JS game plays them):
    /// the radio off, the default (D1160).
    pub station: String,
    /// `mr.radioStation` (D1160): the station the radio comes back to when
    /// switched on, `auto` or a key.
    pub radio_station: String,
    /// `mr.renderScale` (the web only, D1194): the race's pixel ratio is the
    /// JS's times this percentage: `100`, `75` or `50`.
    pub scale: String,
    /// `mr.physics` (Rust only, D1202): which cars the sim model moves,
    /// `arcade` (the default), `player` or `all`.
    pub physics: String,
}

/// Whether a station setting is one the menu offers.
pub fn station_ok(v: &str) -> bool {
    v == "auto" || v == "playlist" || mp_music::radio::station(v).is_some()
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
            level: store.string("level", "coast"),
            track: store.string("track", "auto"),
            flash: store.bool("flash", true),
            rumble: store.bool("rumble", true),
            guide: store.string("guideLine", if touch_ui { "full" } else { "off" }),
            assist: store.string("steerAssist", if touch_ui { "light" } else { "off" }),
            classic_engine: store.bool("classicEngine", false),
            station: store.string("station", "playlist"),
            radio_station: String::new(),
            scale: store.string("renderScale", "100"),
            physics: store.string("physics", "arcade"),
        };
        if !station_ok(&s.station) {
            s.station = "playlist".into();
        }
        // The radio's station: the one remembered, else the one playing.
        let remembered = store.string("radioStation", &s.station);
        s.radio_station = crate::play::station::radio_on_setting(&remembered);
        if !GUIDE.contains(&s.guide.as_str()) {
            s.guide = if touch_ui { "full" } else { "off" }.into();
        }
        if !ASSIST.contains(&s.assist.as_str()) {
            s.assist = if touch_ui { "light" } else { "off" }.into();
        }
        if !SCALES.contains(&s.scale.as_str()) {
            s.scale = "100".into();
        }
        if mp_sim::model::Physics::parse(&s.physics).is_none() {
            s.physics = "arcade".into();
        }
        if touch_ui && !STEERING.contains(&s.steering.as_str()) {
            s.steering = "stick".into();
        }
        if touch_ui && !PEDALS.contains(&s.pedals.as_str()) {
            s.pedals = "slider".into();
        }
        if s.track != "auto" && !mp_audio::tracks::TRACKS.iter().any(|t| t.id == s.track) {
            s.track = "auto".into();
        }
        if !car_kinds().any(|k| k == s.car) {
            s.car = "sports".into();
        }
        if !mp_levels::levels().iter().any(|l| l.id == s.level) {
            s.level = mp_levels::levels()[0].id.into();
        }
        s
    }
}

/// `modeFor(l)`: Race, Hot Pursuit or Cruise, per level; only levels with
/// police offer Hot Pursuit, and only the open world (`level.open`) a
/// cruise: the road with no race on it, scored as the Night City Cruise
/// is (D1211).
pub fn mode_for(store: &Store, level: &mp_track::Level) -> &'static str {
    let m = store.string(&format!("mode.{}", level.id), "race");
    if level.police.is_some() && m == "pursuit" {
        "pursuit"
    } else if level.open && m == "cruise" {
        "cruise"
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
        Store::new(m, PREFIX)
    }

    /// The keys the game saves read back through the kit's store with the
    /// same bytes: the JS game's `localStorage` strings and the native
    /// file are unchanged by the move (D1234).
    #[test]
    fn the_games_keys_round_trip_unchanged() {
        let saved = [
            ("mr.musicVol", "0.7"),
            ("mr.sfxVol", "0.85"),
            ("mr.mph", "true"),
            ("mr.hq", "false"),
            ("mr.autogas", "false"),
            ("mr.steering", "\"tilt\""),
            ("mr.tiltSens", "0.5"),
            ("mr.pedals", "\"slider\""),
            ("mr.fullscreen", "true"),
            ("mr.car", "\"super\""),
            ("mr.level", "\"coast\""),
            ("mr.track", "\"auto\""),
            ("mr.flash", "true"),
            ("mr.rumble", "true"),
            ("mr.guideLine", "\"full\""),
            ("mr.steerAssist", "\"light\""),
            ("mr.classicEngine", "false"),
            ("mr.station", "\"playlist\""),
            ("mr.radioStation", "\"ridgeline\""),
            ("mr.renderScale", "\"75\""),
            ("mr.physics", "\"arcade\""),
            ("mr.best.sierra", "183.4566666666667"),
            ("mr.best.coast.pursuit", "201.25"),
            ("mr.bestScore.cruise", "12345"),
            ("mr.mode.coast", "\"pursuit\""),
            ("mr.mpName", "\"Ann \\\"Speedy\\\"\""),
            ("mr.mpColor", "14163510"),
            (
                "mr.padMaps",
                r#"{"Pad (STANDARD GAMEPAD)":{"left":[{"axis":0,"dir":-1,"rest":0}],"throttle":[{"button":7}]}}"#,
            ),
        ];
        let store = seeded(&saved);
        let s = Settings::load(&store, true);
        assert_eq!(
            (s.car.as_str(), s.scale.as_str(), s.steering.as_str()),
            ("super", "75", "tilt")
        );
        // Written back by the typed setters, byte for byte.
        let mut out = memory();
        out.set_num("musicVol", 0.7);
        out.set_bool("hq", false);
        out.set_str("car", "super");
        out.set_num("best.sierra", 183.456_666_666_666_7);
        out.set_num("mpColor", 14_163_510.0);
        out.set_str("mpName", "Ann \"Speedy\"");
        out.set(
            "padMaps",
            &serde_json::from_str::<Value>(saved[27].1).unwrap(),
        );
        for k in [
            "musicVol",
            "hq",
            "car",
            "best.sierra",
            "mpColor",
            "mpName",
            "padMaps",
        ] {
            let want = saved
                .iter()
                .find(|(n, _)| *n == format!("{PREFIX}{k}"))
                .map(|(_, v)| *v);
            assert_eq!(out.raw(k).as_deref(), want, "{k}");
        }
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
        let store = memory();
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
                level: "coast".into(),
                track: "auto".into(),
                flash: true,
                rumble: true,
                guide: "off".into(),
                assist: "off".into(),
                classic_engine: false,
                station: "playlist".into(),
                radio_station: "auto".into(),
                scale: "100".into(),
                physics: "arcade".into(),
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
            ("mr.classicEngine", "true"),
            ("mr.station", "\"ridgeline\""),
            ("mr.renderScale", "\"75\""),
            ("mr.physics", "\"all\""),
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
                classic_engine: true,
                station: "ridgeline".into(),
                radio_station: "ridgeline".into(),
                scale: "75".into(),
                physics: "all".into(),
            }
        );
        // A choice the menu does not offer falls back to the default.
        let store = seeded(&[
            ("mr.guideLine", "\"rainbow\""),
            ("mr.steerAssist", "3"),
            ("mr.station", "\"kzzz\""),
            ("mr.renderScale", "\"33\""),
            ("mr.physics", "\"rally\""),
        ]);
        let s = Settings::load(&store, false);
        assert_eq!((s.guide.as_str(), s.assist.as_str()), ("off", "off"));
        assert_eq!(s.station, "playlist");
        assert_eq!(s.scale, "100");
        assert_eq!(s.physics, "arcade");
        assert_eq!((scale_of("75"), scale_of("x")), (0.75, 1.0));
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
        assert_eq!(s.level, "coast");
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
        let mut store = memory();
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
        let coast = mp_levels::level_by_id("coast");
        assert!(coast.police.is_some());
        assert_eq!(mode_for(&store, &coast), "pursuit");
        // A level without police is always a race.
        let seaside = mp_levels::level_by_id("seaside");
        store.set_str("mode.seaside", "pursuit");
        assert_eq!(mode_for(&store, &seaside), "race");
        // Only the open world offers a cruise (D1211).
        store.set_str("mode.seaside", "cruise");
        assert_eq!(mode_for(&store, &seaside), "race");
        let sc = mp_levels::level_by_id("southcoast");
        assert!(sc.open);
        assert_eq!(mode_for(&store, &sc), "race");
        store.set_str("mode.southcoast", "cruise");
        assert_eq!(mode_for(&store, &sc), "cruise");
        assert_eq!(best_key("sierra", false), "best.sierra");
    }

    /// The native file holds the same key → string pairs, and a new store
    /// over it reads them back.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_file_store_round_trips() {
        use pk_shell::store::FileStore;
        let dir = std::env::temp_dir().join(format!("mp-store-test-{}", std::process::id()));
        let path = dir.join("storage.json");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let mut s = Store::new(FileStore::open(path.clone()), PREFIX);
            s.set_str("car", "electric");
            s.set_num("tiltSens", 0.3);
            s.set_bool("hq", false);
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text,
            r#"{"mr.car":"\"electric\"","mr.tiltSens":"0.3","mr.hq":"false"}"#
        );
        let s = Store::new(FileStore::open(path.clone()), PREFIX);
        assert_eq!(s.raw("car").as_deref(), Some("\"electric\""));
        assert_eq!(s.raw("tiltSens").as_deref(), Some("0.3"));
        let set = Settings::load(&s, false);
        assert_eq!(
            (set.car.as_str(), set.tilt_sens, set.hq),
            ("electric", 0.3, false)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `store.get(k, d)`'s default for a value of the wrong type, as the
    /// JS's typed reads do: a string is no number, a number no string,
    /// `null` nothing.
    #[test]
    fn a_value_of_the_wrong_type_reads_as_the_default() {
        let store = seeded(&[
            ("mr.a", "\"0.5\""),
            ("mr.b", "0.5"),
            ("mr.c", "null"),
            ("mr.d", "1"),
            ("mr.e", ""),
        ]);
        assert_eq!(store.num("a", 2.0), 2.0);
        assert_eq!(store.num_or_null("a"), None);
        assert_eq!(store.string("b", "x"), "x");
        assert_eq!(store.num("c", 3.0), 3.0);
        assert_eq!(store.num_or_null("c"), None);
        assert!(store.bool("d", true), "1 is not true");
        assert!(!store.bool("d", false));
        assert_eq!(store.get("e"), None, "an empty string does not parse");
        assert_eq!(store.num("missing", 4.0), 4.0);
        // A NaN saved is `null`, and reads back as missing.
        let mut store = memory();
        store.set_num("best.coast", f64::NAN);
        assert_eq!(store.raw("best.coast").as_deref(), Some("null"));
        assert_eq!(store.num_or_null("best.coast"), None);
    }

    /// Setting a key again replaces its value where it is: the order is
    /// the first insertion's, as `localStorage` keeps it.
    #[test]
    fn rewriting_a_key_keeps_its_place() {
        let mut m = Memory::default();
        m.set_item("a", "1");
        m.set_item("b", "2");
        m.set_item("a", "3");
        assert_eq!(
            m.items,
            [("a".to_string(), "3".to_string()), ("b".into(), "2".into())]
        );
        assert_eq!(m.get_item("a").as_deref(), Some("3"));
        assert_eq!(m.get_item("c"), None);
    }

    /// The multiplayer name and colour (`mpName`, `mpColor`) are stored as
    /// the other settings are, a colour as a plain integer.
    #[test]
    fn the_multiplayer_keys() {
        let mut store = memory();
        store.set_str("mpName", "Ann \"Speedy\"");
        store.set_num("mpColor", f64::from(0xd81e36u32));
        assert_eq!(store.raw("mpColor").as_deref(), Some("14163510"));
        assert_eq!(store.num("mpColor", 0.0) as u32, 0xd81e36);
        assert_eq!(store.string("mpName", "Driver"), "Ann \"Speedy\"");
    }

    /// A broken file, a file that isn't an object, or values that aren't
    /// strings: whatever can't be read is left out, and the store works.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_file_store_survives_a_broken_file() {
        use pk_shell::store::FileStore;
        let dir = std::env::temp_dir().join(format!("mp-store-broken-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("storage.json");
        for text in ["{not json", "[1,2]", "", "null"] {
            std::fs::write(&path, text).unwrap();
            let s = Store::new(FileStore::open(path.clone()), PREFIX);
            assert_eq!(s.raw("car"), None, "{text:?}");
        }
        std::fs::write(&path, r#"{"mr.car":"\"rally\"","mr.hq":false,"mr.x":1}"#).unwrap();
        let mut s = Store::new(FileStore::open(path.clone()), PREFIX);
        assert_eq!(s.raw("car").as_deref(), Some("\"rally\""));
        assert_eq!(s.raw("hq"), None, "not a string: left out");
        s.set_str("car", "super");
        s.set_bool("mph", false);
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"mr.car":"\"super\"","mr.mph":"false"}"#
        );
        // A store whose folder isn't there yet makes it.
        let deep = dir.join("a/b/storage.json");
        Store::new(FileStore::open(deep.clone()), PREFIX).set_num("sfxVol", 0.5);
        assert!(deep.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
