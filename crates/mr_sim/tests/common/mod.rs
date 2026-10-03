//! Shared by mr_sim's tests: the levels (Seaside with its survey), staged
//! worlds, and the module trace goldens.

#![allow(dead_code)]

use std::io::Read;
use std::sync::{Arc, OnceLock};

use mr_levels::{SeasideData, levels, seaside};
use mr_sim::staged::{Stage, stage_level};
use mr_sim::trace::{ReadTrace, read_trace};
use mr_track::Level;

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
