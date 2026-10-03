//! Shared by mr_sim's tests: the levels (Seaside with its survey), staged
//! worlds, and the module trace goldens.

#![allow(dead_code)]

use std::io::Read;
use std::sync::{Arc, OnceLock};

use mr_levels::{SeasideData, levels, seaside};
use mr_sim::dims::Dims;
use mr_sim::staged::{Stage, stage_level};
use mr_sim::trace::{ReadTrace, read_trace};
use mr_sim::vehicle::Vehicle;
use mr_track::{Level, Mode, Route, Track, Zone, seg};

pub const SURVEY: &[u8] = include_bytes!("../../../../assets/seaside/survey.bin");

pub fn survey() -> Arc<SeasideData> {
    static DATA: OnceLock<Arc<SeasideData>> = OnceLock::new();
    DATA.get_or_init(|| Arc::new(SeasideData::parse(SURVEY).expect("survey.bin parses")))
        .clone()
}

pub fn level(id: &str) -> Level {
    let mut l = levels().into_iter().find(|l| l.id == id).unwrap();
    if l.id == "seaside" {
        seaside::prepare(&mut l, survey());
    }
    l
}

/// A level staged as node-sim's `level()` stages it, built once per test
/// binary.
pub fn stage(id: &str) -> &'static Stage {
    static STAGES: OnceLock<Vec<Stage>> = OnceLock::new();
    let all = STAGES.get_or_init(|| {
        ["sierra", "coast", "streets", "desert", "seaside", "cruise"]
            .iter()
            .map(|id| stage_level(level(id)))
            .collect()
    });
    all.iter().find(|s| s.level.id == id).unwrap()
}

/// A committed module trace golden, compiled in so the replay runs in wasm
/// too.
pub fn golden(id: &str) -> ReadTrace {
    let gz: &[u8] = match id {
        "ai-field-coast" => {
            include_bytes!("../../../../parity/golden/sim/module/ai-field-coast.trace.gz")
        }
        "ai-field-desert" => {
            include_bytes!("../../../../parity/golden/sim/module/ai-field-desert.trace.gz")
        }
        "ai-field-seaside" => {
            include_bytes!("../../../../parity/golden/sim/module/ai-field-seaside.trace.gz")
        }
        "ai-field-sierra" => {
            include_bytes!("../../../../parity/golden/sim/module/ai-field-sierra.trace.gz")
        }
        "ai-field-streets" => {
            include_bytes!("../../../../parity/golden/sim/module/ai-field-streets.trace.gz")
        }
        "collide-rear-pin" => {
            include_bytes!("../../../../parity/golden/sim/module/collide-rear-pin.trace.gz")
        }
        "full-field-sierra" => {
            include_bytes!("../../../../parity/golden/sim/module/full-field-sierra.trace.gz")
        }
        "phys-analog" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-analog.trace.gz")
        }
        "phys-autopilot-coast" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-coast.trace.gz")
        }
        "phys-autopilot-cruise" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-cruise.trace.gz")
        }
        "phys-autopilot-desert" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-desert.trace.gz")
        }
        "phys-autopilot-seaside" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-seaside.trace.gz")
        }
        "phys-autopilot-sierra" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-sierra.trace.gz")
        }
        "phys-autopilot-streets" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-autopilot-streets.trace.gz")
        }
        "phys-frame-dt" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-frame-dt.trace.gz")
        }
        "phys-handbrake" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-handbrake.trace.gz")
        }
        "phys-launch-electric" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-launch-electric.trace.gz")
        }
        "phys-launch-muscle" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-launch-muscle.trace.gz")
        }
        "phys-launch-rally" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-launch-rally.trace.gz")
        }
        "phys-launch-sports" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-launch-sports.trace.gz")
        }
        "phys-launch-super" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-launch-super.trace.gz")
        }
        "phys-reverse" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-reverse.trace.gz")
        }
        "phys-spiked-damaged" => {
            include_bytes!("../../../../parity/golden/sim/module/phys-spiked-damaged.trace.gz")
        }
        "phys-wall" => include_bytes!("../../../../parity/golden/sim/module/phys-wall.trace.gz"),
        "pursuit-coast" => {
            include_bytes!("../../../../parity/golden/sim/module/pursuit-coast.trace.gz")
        }
        "pursuit-desert" => {
            include_bytes!("../../../../parity/golden/sim/module/pursuit-desert.trace.gz")
        }
        "pursuit-heat5-props" => {
            include_bytes!("../../../../parity/golden/sim/module/pursuit-heat5-props.trace.gz")
        }
        "pursuit-sierra" => {
            include_bytes!("../../../../parity/golden/sim/module/pursuit-sierra.trace.gz")
        }
        "pursuit-streets" => {
            include_bytes!("../../../../parity/golden/sim/module/pursuit-streets.trace.gz")
        }
        "traffic-coast" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-coast.trace.gz")
        }
        "traffic-cruise" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-cruise.trace.gz")
        }
        "traffic-desert" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-desert.trace.gz")
        }
        "traffic-seaside" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-seaside.trace.gz")
        }
        "traffic-sierra" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-sierra.trace.gz")
        }
        "traffic-streets" => {
            include_bytes!("../../../../parity/golden/sim/module/traffic-streets.trace.gz")
        }
        _ => panic!("no module golden {id}"),
    };
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .read_to_end(&mut bytes)
        .unwrap();
    read_trace(&bytes).unwrap()
}

/// Keep a failing replay's own trace in target/parity/, for
/// `node tools/parity/trace-inspect.mjs <rust> --diff <golden>` (native only).
pub fn save_trace(id: &str, bytes: &[u8]) {
    if cfg!(target_arch = "wasm32") {
        return;
    }
    let dir = format!("{}/../../target/parity", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(format!("{dir}/{id}.trace"), bytes).unwrap();
}

/// A dead-straight, flat road (x grows along it; +z is to the right):
/// `straightTrack` in test/unit/support/sim.js.
pub fn straight_track(length: f64, road: &'static str) -> Track {
    Track::new(&straight_level(length, road)).unwrap()
}

/// The level of [`straight_track`].
pub fn straight_level(length: f64, road: &'static str) -> Level {
    Level {
        id: "test-straight",
        mode: Mode::Race,
        num: "",
        title: "",
        desc: "",
        laps: None,
        lap_length: None,
        start_height: Some(0.0),
        start_heading: Some(0.0),
        start_x: None,
        start_z: None,
        finish_runoff: Some(180.0),
        elevation_smooth: None,
        route: Route::Segments(vec![seg(length, 0.0, 0.0).zone(0).road(road)]),
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: vec![Zone {
            key: "test",
            name: "TEST",
            sub: "",
            landform: "valley",
            scenery: "",
            color: "",
            blend: None,
            blend_offset: None,
        }],
        sky: Vec::new(),
        sun_azimuth: 0.0,
        moon_dir: None,
        traffic_paint: None,
        traffic: Vec::new(),
        police: None,
        rivals: Vec::new(),
    }
}

/// A car with no model: the dimensions physics and AI read (`makeVehicle`).
pub fn make_vehicle(kind: &'static str, mass: f64) -> Vehicle {
    let (length, width, wheel_base) = match kind {
        "sports" => (4.47, 1.9, 2.6),
        "muscle" => (4.86, 1.95, 2.8),
        "super" => (4.57, 2.05, 2.7),
        "electric" => (4.74, 1.98, 2.9),
        "rally" => (4.12, 1.9, 2.55),
        _ => (4.6, 1.95, 2.7),
    };
    let dims = Dims {
        length,
        width,
        height: 1.3,
        wheel_radius: 0.34,
        wheel_base,
        track: None,
    };
    Vehicle::new(dims, kind, mass, "", 0xffffff)
}
