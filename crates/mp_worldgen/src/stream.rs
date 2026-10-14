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

    /// Each road's sections as (first index, count): the level's road
    /// first, then its branches in order, `SECTION_LEN` of road each (the
    /// last of a road may be short).
    pub fn road_sections(&self) -> Vec<(i64, i64)> {
        let mut out = Vec::new();
        let mut first = 0;
        for t in self.track.roads() {
            let n = ((t.length / SECTION_LEN).ceil() as i64).max(1);
            out.push((first, n));
            first += n;
        }
        out
    }

    /// The road section `k` is on, and which of that road's it is.
    pub fn road_of(&self, k: i64) -> (usize, i64) {
        let rs = self.road_sections();
        for (r, &(first, n)) in rs.iter().enumerate() {
            if k < first + n {
                return (r, (k - first).max(0));
            }
        }
        let r = rs.len() - 1;
        (r, rs[r].1 - 1)
    }

    /// Section `k` (`SECTION_LEN` of its road, from s 0): what the client
    /// streams.
    pub fn section(&self, k: i64) -> Section {
        let (road, i) = self.road_of(k);
        let s0 = i as f64 * SECTION_LEN;
        Section {
            s0,
            s1: s0 + SECTION_LEN,
            terrain_radius: SECTION_RADIUS,
            scenery_radius: SECTION_RADIUS,
            scenery: true,
            partition: true,
            base: false,
            road,
        }
    }

    /// The section a place `s` on road `road` is in.
    pub fn section_of(&self, road: usize, s: f64) -> i64 {
        let rs = self.road_sections();
        let (first, n) = rs[road.min(rs.len() - 1)];
        let t = self.track.road(road.min(rs.len() - 1));
        let s = if t.is_loop { t.wrap(s) } else { s.max(0.0) };
        first + ((s / SECTION_LEN).floor() as i64).min(n - 1)
    }

    /// The section under (x, z): the nearest road's, when one is near.
    pub fn section_under(&self, x: f64, z: f64) -> Option<i64> {
        Section::s_of(&self.track, x, z).map(|(r, s)| self.section_of(r, s))
    }

    /// The sections kept up round `k`: `behind` and `ahead` along its road
    /// (wrapped on a loop, clamped at a branch's ends), and at a junction
    /// the other road's two sections beside it, so the car turns off onto
    /// a road that is already there: a branch's first two where the ring
    /// covers its mouth on the parent, the parent's round the mouth (and
    /// round the far end, where the branch meets the parent again) when
    /// the ring is at the branch's end.
    pub fn ring(&self, k: i64, behind: i64, ahead: i64) -> Vec<i64> {
        let rs = self.road_sections();
        let (road, i) = self.road_of(k);
        let mut out: Vec<i64> = Vec::new();
        let push = |out: &mut Vec<i64>, k: i64| {
            if !out.contains(&k) {
                out.push(k);
            }
        };
        let along = |r: usize, i: i64, behind: i64, ahead: i64, out: &mut Vec<i64>| {
            let (first, n) = rs[r];
            let t = self.track.road(r);
            for j in i - behind..=i + ahead {
                let idx = if t.is_loop {
                    j.rem_euclid(n)
                } else {
                    j.clamp(0, n - 1)
                };
                push(out, first + idx);
            }
        };
        along(road, i, behind, ahead, &mut out);
        for (bi, b) in self.track.branches.iter().enumerate() {
            let br = bi + 1;
            let (bf, bn) = rs[br];
            let mouth = self.section_of(0, b.s);
            let far = b.end.map(|e| self.section_of(0, e));
            if road == 0 {
                if out.contains(&mouth) {
                    push(&mut out, bf);
                    push(&mut out, bf + 1.min(bn - 1));
                }
                if let Some(fe) = far
                    && out.contains(&fe)
                {
                    push(&mut out, bf + bn - 1);
                    push(&mut out, bf + (bn - 2).max(0));
                }
            } else if road == br {
                if i <= 1 {
                    let (_, m) = self.road_of(mouth);
                    along(0, m, 1, 1, &mut out);
                }
                if let Some(fe) = far
                    && i >= bn - 2
                {
                    let (_, m) = self.road_of(fe);
                    along(0, m, 1, 1, &mut out);
                }
            }
        }
        out
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

    /// How many sections the network has: every road's.
    pub fn sections(&self) -> i64 {
        self.road_sections().iter().map(|r| r.1).sum()
    }

    /// The section a place on the level's road is in.
    pub fn section_at(&self, s: f64) -> i64 {
        self.section_of(0, s)
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
