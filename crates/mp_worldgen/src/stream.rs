//! Streaming the open world (docs/vision/peninsula.md, stage 4b; DECISIONS
//! D1213): the level planned once, then built in sections of its route
//! round the player, each a [`Build`] of its own.
//!
//! A level build plans before it builds: the track is surveyed, the
//! terrain made, every scenery module's `plan()` run (flattens, carves,
//! fence gaps) and the terrain's fields rasterised. [`Planned`] does that
//! once and keeps the result; [`Planned::build`] then makes the jobs of
//! one [`Section`] over it: the modules plan afresh on the unplanned
//! terrain (their own state, which `build()` needs, comes from `plan()`),
//! the planned terrain stands in, and the section's tiles, its stretch of
//! the road and the kits' builds follow, cut to what belongs to the
//! stretch. A `base` section is the horizon under them all: the coarse
//! tiles, the sky and the sea.

use std::sync::Arc;

use mp_levels::SeasideData;
use mp_track::{Level, Track};

use crate::road::MarkGap;
use crate::section::Section;
use crate::stages::{LevelSetup, level_stages};
use crate::terrain::Terrain;
use crate::world::{Build, Job, Scenery, SceneryInfo, SimWorldData, Stages, World, run_stage};

/// A level's scenery modules by name (`loadScenery`).
pub type SceneryFn = Arc<dyn Fn(&SceneryInfo) -> Option<Box<dyn Scenery>> + Send + Sync>;
/// The extra modules built after the level's own (the living world).
pub type ExtraFn = Arc<dyn Fn(&Level) -> Vec<Box<dyn Scenery>> + Send + Sync>;

/// Metres of route a streamed section covers.
pub const SECTION_LEN: f64 = 1000.0;
/// How far round its stretch a section looks for scenery modules and
/// terrain (the kits' chunks reach 720 m; what is kept is by the centre).
pub const SECTION_RADIUS: f64 = 700.0;

/// The level planned: what every section builds on.
pub struct Planned {
    pub level: Level,
    pub track: Track,
    /// The terrain as made (the recorded plan applied), before the
    /// modules' plans and the fields: what a section's modules plan on.
    base: Terrain,
    /// The terrain planned and rasterised: what a section builds on.
    terrain: Terrain,
    no_marks: Vec<MarkGap>,
    sim_data: SimWorldData,
    setup: LevelSetup,
    scenery: SceneryFn,
    extra: ExtraFn,
    level_data: Option<Arc<SeasideData>>,
}

impl Planned {
    /// Surveys the route, makes the terrain, plans the modules and builds
    /// the fields, as "Surveying the route" and "Shaping the land" do.
    pub fn new(
        level: Level,
        level_data: Option<Arc<SeasideData>>,
        setup: LevelSetup,
        scenery: SceneryFn,
        extra: ExtraFn,
    ) -> Result<Planned, String> {
        let mut w = World::new(level);
        if let Some(d) = &level_data {
            w = w.with_level_data(d.clone());
        }
        w.track = Some(Track::new(&w.level)?);
        let Stages {
            terrain, fields, ..
        } = level_stages(setup.clone());
        run_stage(terrain, &mut w)?;
        let base = w.terrain.clone().ok_or("no terrain")?;
        let _ = crate::world::plan_modules(&mut w, &*scenery, &*extra);
        run_stage(fields, &mut w)?;
        Ok(Planned {
            track: w.track.take().ok_or("no track")?,
            terrain: w.terrain.take().ok_or("no terrain")?,
            no_marks: w.no_marks.clone(),
            sim_data: w.sim_data.clone(),
            level: w.level,
            base,
            setup,
            scenery,
            extra,
            level_data,
        })
    }

    /// The section of the route `k` sections along (`SECTION_LEN` each,
    /// from s 0): what the client streams.
    pub fn section(&self, k: i64) -> Section {
        let s0 = k as f64 * SECTION_LEN;
        Section {
            s0,
            s1: s0 + SECTION_LEN,
            terrain_radius: SECTION_RADIUS,
            scenery_radius: SECTION_RADIUS,
            scenery: true,
            partition: true,
            base: false,
        }
    }

    /// The horizon layer.
    pub fn base_section(&self) -> Section {
        Section {
            s0: 0.0,
            s1: self.track.length,
            scenery: false,
            partition: true,
            base: true,
            ..Section::default()
        }
    }

    /// How many sections the route has (the last may be short; on a loop
    /// the wrap belongs to it).
    pub fn sections(&self) -> i64 {
        (self.track.length / SECTION_LEN).ceil() as i64
    }

    /// The section a place on the route is in.
    pub fn section_at(&self, s: f64) -> i64 {
        let s = if self.track.is_loop {
            self.track.wrap(s)
        } else {
            s.max(0.0)
        };
        ((s / SECTION_LEN).floor() as i64).min(self.sections() - 1)
    }

    /// The build of one section (or the base) over the plan: its jobs run
    /// as a level's do, then [`crate::section::finish`] cuts and finishes.
    pub fn build(&self, sec: Section) -> Build {
        let mut w = World::new(self.level.clone());
        if let Some(d) = &self.level_data {
            w = w.with_level_data(d.clone());
        }
        w.track = Some(self.track.clone());
        w.terrain = Some(self.base.clone());
        w.section = Some(sec.clone());
        let planned = self.terrain.clone();
        let no_marks = self.no_marks.clone();
        let sim_data = self.sim_data.clone();
        let (scenery, extra) = (self.scenery.clone(), self.extra.clone());
        let Stages {
            terrain_meshes,
            road,
            sky,
            sea,
            ..
        } = level_stages(self.setup.clone());
        let base = sec.base;
        let shape = Job::serial("Shaping the land", 0.06, move |w| {
            let kept = crate::world::plan_modules(w, &*scenery, &*extra);
            w.terrain = Some(planned);
            w.no_marks = no_marks;
            w.sim_data = sim_data;
            let mut out = vec![Job::serial("Sculpting terrain", 0.1, move |w| {
                run_stage(terrain_meshes, w)
            })];
            if base {
                out.push(Job::serial("Paving roads", 0.67, move |w| {
                    run_stage(sky, w)
                }));
                if w.level.sea_y.is_some() {
                    out.push(Job::serial("Filling the sea", 0.7, move |w| {
                        run_stage(sea, w)
                    }));
                }
            } else {
                out.push(Job::serial("Paving roads", 0.67, move |w| {
                    run_stage(road, w)
                }));
                out.extend(crate::world::build_jobs(kept));
            }
            out.push(Job::serial("Ready", 1.0, |_| Ok(Vec::new())));
            Ok(out)
        });
        Build::new(w, vec![shape])
    }
}
