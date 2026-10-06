//! The level's scenery modules, ported or replayed (DECISIONS D330).
//!
//! `World.build` makes each scenery module a level names, runs every
//! `plan()` in that order (flattens, carves, the railway bed, fence gaps,
//! unpainted stretches, the runout), then each `build()` after the road.
//! A module's registrations take effect in that order: the terrain's
//! flattens are sequential blends, so the order is part of the heights.
//!
//! Until every module is ported, the ones that are not are replayed from
//! the recordings (`parity/golden/terrain/`, `parity/golden/road/`, which
//! hold each level's registrations as one list) by a [`RecordedScenery`]
//! that registers its own slice of them (`parity/golden/scenery-plan/`,
//! `tools/parity/scenery-plan.mjs`) in its own place, and builds nothing.
//! [`scenery_factory`] gives [`level_jobs`](crate::world::level_jobs) the
//! ported module where there is one ([`PORTED`]), the recording otherwise.
//!
//! Porting a module is one line in [`PORTED`].

use std::ops::Range;
use std::sync::Arc;

use mp_levels::world;
use mp_track::FenceGap;
use serde_json::Value;

use crate::road::MarkGap;
use crate::stages::RoadPlan;
use crate::terrain::{Carve, DesertRail, Flatten, TerrainPlan};
use crate::world::{Scenery, SceneryInfo, World};

/// A ported module's constructor: `new mod.default({ zone, key, level })`.
pub type MakeScenery = fn(&SceneryInfo) -> Box<dyn Scenery>;

/// The ported scenery modules by JS class name. Add one line per module.
pub const PORTED: &[(&str, MakeScenery)] = &[
    ("Mountain", |info| {
        Box::new(crate::mountain::Mountain::new(info))
    }),
    ("Valley", |info| Box::new(crate::valley::Valley::new(info))),
    ("City", |info| Box::new(crate::city::City::new(info))),
    ("Coast", |info| Box::new(crate::coast::Coast::new(info))),
    ("Beach", |info| Box::new(crate::beach::Beach::new(info))),
    ("Harbor", |info| Box::new(crate::harbor::Harbor::new(info))),
    ("Desert", |info| Box::new(crate::desert::Desert::new(info))),
    ("Raceway", |info| {
        Box::new(crate::raceway::Raceway::new(info))
    }),
    ("Streets", |info| {
        Box::new(crate::streets::Streets::new(info))
    }),
];

/// The ported module of that name, made as `loadScenery` makes it.
pub fn ported(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    PORTED
        .iter()
        .find(|(n, _)| *n == info.name)
        .map(|(_, make)| make(info))
}

// ── The recordings ──────────────────────────────────────────────────────

/// What one module's `plan()` registered: ranges into the level's
/// recorded lists.
#[derive(Clone, Debug, PartialEq)]
pub struct ModuleRecord {
    pub name: String,
    pub zone: usize,
    pub key: String,
    /// `s.label`.
    pub label: Option<String>,
    /// The message of a `plan()` that threw (the module is dropped).
    pub failed: Option<String>,
    pub flattens: Range<usize>,
    pub carves: Range<usize>,
    pub fence_gaps: Range<usize>,
    pub no_marks: Range<usize>,
    /// Whether it set the terrain's railway bed.
    pub desert_rail: bool,
    /// `track.runout` after its `plan()`, where it changed it. Not replayed:
    /// the stand-in computes it by the module's rule (`mp_levels::world`),
    /// and the road test holds that to this.
    pub runout: Option<f64>,
}

/// A level's recorded plan stage: everything registered, and which
/// module registered what.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanRecording {
    pub terrain: TerrainPlan,
    pub road: RoadPlan,
    pub modules: Vec<ModuleRecord>,
}

fn hex(v: &Value) -> Result<f64, String> {
    let s = v.as_str().ok_or("hex bits expected")?;
    u64::from_str_radix(s, 16)
        .map(f64::from_bits)
        .map_err(|e| e.to_string())
}

fn opt_hex(v: Option<&Value>) -> Result<Option<f64>, String> {
    match v {
        None | Some(Value::Null) => Ok(None),
        Some(v) => hex(v).map(Some),
    }
}

fn list<'a>(j: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    j[key].as_array().ok_or_else(|| format!("{key}: a list"))
}

fn range(v: &Value) -> Result<Range<usize>, String> {
    let a = v[0].as_u64().ok_or("a range")? as usize;
    let b = v[1].as_u64().ok_or("a range")? as usize;
    Ok(a..b)
}

/// `parity/golden/terrain/<level>.json` (`terrain-plan.mjs`): the plan the
/// terrain was given.
pub fn parse_terrain_plan(j: &Value) -> Result<TerrainPlan, String> {
    let flattens = list(j, "flattens")?
        .iter()
        .map(|f| {
            Ok(Flatten {
                x: hex(&f["x"])?,
                z: hex(&f["z"])?,
                r: hex(&f["r"])?,
                falloff: hex(&f["falloff"])?,
                y: opt_hex(f.get("y"))?,
                y_resolved: None,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let carves = list(j, "carves")?
        .iter()
        .map(|c| {
            let points = list(c, "points")?
                .iter()
                .map(|p| Ok([hex(&p[0])?, hex(&p[1])?]))
                .collect::<Result<Vec<_>, String>>()?;
            Ok(Carve {
                points,
                width: hex(&c["width"])?,
                depth: hex(&c["depth"])?,
                under_road: c["underRoad"].as_bool().ok_or("underRoad")?,
                min_x: 0.0,
                max_x: 0.0,
                min_z: 0.0,
                max_z: 0.0,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let r = &j["desertRail"];
    let desert_rail = if r.is_null() {
        None
    } else {
        Some(DesertRail {
            lat: hex(&r["lat"])?,
            half: hex(&r["half"])?,
            drop: hex(&r["drop"])?,
            s0: hex(&r["s0"])?,
            s1: hex(&r["s1"])?,
        })
    };
    Ok(TerrainPlan {
        flattens,
        carves,
        desert_rail,
    })
}

/// `parity/golden/road/<level>.json` (`road-plan.mjs`): what the road was
/// told.
pub fn parse_road_plan(j: &Value) -> Result<RoadPlan, String> {
    Ok(RoadPlan {
        fence_gaps: list(j, "fenceGaps")?
            .iter()
            .map(|f| {
                Ok(FenceGap {
                    s0: hex(&f["s0"])?,
                    s1: hex(&f["s1"])?,
                    side: hex(&f["side"])?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        no_marks: list(j, "noMarks")?
            .iter()
            .map(|f| {
                Ok(MarkGap {
                    s0: hex(&f["s0"])?,
                    s1: hex(&f["s1"])?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?,
        runout: hex(&j["runout"])?,
    })
}

impl PlanRecording {
    /// From the three goldens' text: the terrain plan, the road plan and
    /// the per-module split (`parity/golden/scenery-plan/<level>.json`).
    pub fn parse(terrain: &str, road: &str, split: &str) -> Result<PlanRecording, String> {
        let tj: Value = serde_json::from_str(terrain).map_err(|e| e.to_string())?;
        let rj: Value = serde_json::from_str(road).map_err(|e| e.to_string())?;
        let sj: Value = serde_json::from_str(split).map_err(|e| e.to_string())?;
        let terrain = parse_terrain_plan(&tj)?;
        let road = parse_road_plan(&rj)?;
        let modules = list(&sj, "modules")?
            .iter()
            .map(|m| {
                Ok(ModuleRecord {
                    name: m["name"].as_str().ok_or("name")?.to_string(),
                    zone: m["zone"].as_u64().ok_or("zone")? as usize,
                    key: m["key"].as_str().ok_or("key")?.to_string(),
                    label: m["label"].as_str().map(str::to_string),
                    failed: m["failed"].as_str().map(str::to_string),
                    flattens: range(&m["flattens"])?,
                    carves: range(&m["carves"])?,
                    fence_gaps: range(&m["fenceGaps"])?,
                    no_marks: range(&m["noMarks"])?,
                    desert_rail: m["desertRail"].as_bool().ok_or("desertRail")?,
                    runout: opt_hex(m.get("runout"))?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let rec = PlanRecording {
            terrain,
            road,
            modules,
        };
        rec.check()?;
        Ok(rec)
    }

    /// The modules' ranges cover each list, in order.
    fn check(&self) -> Result<(), String> {
        let mut at = [0usize; 4];
        for m in &self.modules {
            for (k, r) in [&m.flattens, &m.carves, &m.fence_gaps, &m.no_marks]
                .into_iter()
                .enumerate()
            {
                if r.start != at[k] || r.end < r.start {
                    return Err(format!("{}: its ranges do not follow on", m.name));
                }
                at[k] = r.end;
            }
        }
        let want = [
            self.terrain.flattens.len(),
            self.terrain.carves.len(),
            self.road.fence_gaps.len(),
            self.road.no_marks.len(),
        ];
        if at != want {
            return Err(format!(
                "the modules register {at:?}, the recordings hold {want:?}"
            ));
        }
        Ok(())
    }

    /// The record of a module by name.
    pub fn module(&self, name: &str) -> Option<&ModuleRecord> {
        self.modules.iter().find(|m| m.name == name)
    }

    /// One module's part of the recordings, as a stand-in for it.
    pub fn recorded(&self, name: &str) -> Option<RecordedScenery> {
        let m = self.module(name)?;
        Some(RecordedScenery {
            terrain: TerrainPlan {
                flattens: self.terrain.flattens[m.flattens.clone()].to_vec(),
                carves: self.terrain.carves[m.carves.clone()].to_vec(),
                desert_rail: if m.desert_rail {
                    self.terrain.desert_rail
                } else {
                    None
                },
            },
            fence_gaps: self.road.fence_gaps[m.fence_gaps.clone()].to_vec(),
            no_marks: self.road.no_marks[m.no_marks.clone()].to_vec(),
            record: m.clone(),
        })
    }
}

// ── A module replayed ──────────────────────────────────────────────────

/// A scenery module that is not ported yet: its `plan()` registers what
/// the JS module registered, in its place; its `build()` makes nothing.
/// What the module gives the simulation (the runout, Harbor's opposite
/// carriageway) is computed by its rules in `mp_levels::world`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordedScenery {
    pub record: ModuleRecord,
    pub terrain: TerrainPlan,
    pub fence_gaps: Vec<FenceGap>,
    pub no_marks: Vec<MarkGap>,
}

impl Scenery for RecordedScenery {
    fn name(&self) -> &str {
        &self.record.name
    }

    fn label(&self) -> Option<&str> {
        self.record.label.as_deref()
    }

    fn plan(&mut self, w: &mut World) -> Result<(), String> {
        if let Some(e) = &self.record.failed {
            return Err(e.clone());
        }
        let terrain = w.terrain.as_mut().ok_or("no terrain")?;
        self.terrain.apply(terrain);
        let track = w.track.as_mut().ok_or("the route is surveyed first")?;
        track.fence_gaps.extend(self.fence_gaps.iter().copied());
        // The runout by the module's own rule (mp_levels::world, which the
        // simulation reads too), not the recording's; the road test holds it
        // to the recording.
        track.runout = world::plan_runout(&self.record.name, track, track.runout);
        w.sim_data.runout = track.runout;
        w.no_marks.extend(self.no_marks.iter().copied());
        Ok(())
    }

    /// Builds nothing, but gives the simulation the opposite carriageway
    /// the module's `build()` would set (Harbor's), by its own rule.
    fn build(&mut self, w: &mut World) -> Result<(), String> {
        let track = w.track.as_ref().ok_or("the route is surveyed first")?;
        if let Some(oc) = world::built_carriageway(&self.record.name, track, self.record.zone) {
            w.sim_data.opposite_carriageway = Some(oc);
        }
        Ok(())
    }
}

/// The factory for [`level_jobs`](crate::world::level_jobs): the ported
/// module where there is one, else the module's part of `recording`, else
/// none (missing, as a failed `import()`). With a recording, the terrain
/// and road stages are given no recorded plan of their own
/// (`TerrainSetup::plan`, `LevelSetup::road`: `None`).
pub fn scenery_factory(
    recording: Option<Arc<PlanRecording>>,
) -> impl Fn(&SceneryInfo) -> Option<Box<dyn Scenery>> + Send + 'static {
    move |info| {
        ported(info).or_else(|| {
            recording
                .as_ref()?
                .recorded(info.name)
                .map(|r| Box::new(r) as Box<dyn Scenery>)
        })
    }
}

/// A module whose `plan()` runs and whose `build()` is skipped: for builds
/// that look at the terrain or the road only.
pub struct PlanOnly(pub Box<dyn Scenery>);

impl Scenery for PlanOnly {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn label(&self) -> Option<&str> {
        self.0.label()
    }

    fn plan(&mut self, w: &mut World) -> Result<(), String> {
        self.0.plan(w)
    }

    fn build(&mut self, _w: &mut World) -> Result<(), String> {
        Ok(())
    }
}

/// [`scenery_factory`] with every module's `build()` skipped.
pub fn plan_only_factory(
    recording: Option<Arc<PlanRecording>>,
) -> impl Fn(&SceneryInfo) -> Option<Box<dyn Scenery>> + Send + 'static {
    let f = scenery_factory(recording);
    move |info| f(info).map(|s| Box::new(PlanOnly(s)) as Box<dyn Scenery>)
}

/// The same with every module replayed from the recording, ported or not
/// (the recording's own build, for comparisons).
pub fn recorded_factory(
    recording: Arc<PlanRecording>,
) -> impl Fn(&SceneryInfo) -> Option<Box<dyn Scenery>> + Send + 'static {
    move |info| {
        recording
            .recorded(info.name)
            .map(|r| Box::new(r) as Box<dyn Scenery>)
    }
}
