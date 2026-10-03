//! The ported stages of `World.build` together (roadmap WP 3.5): the
//! terrain's (WP 3.4, [`terrain_stages`]), then "Paving roads" (the road,
//! its night parameters and dew updater, the sky) and "Filling the sea".
//! [`level_stages`] fills a [`Stages`] for [`level_jobs`](crate::world::level_jobs),
//! so a level build makes the terrain, road, sky and sea in the JS order.
//!
//! Until the scenery modules are ported, what their `plan()` registers
//! for the road comes from a recording ([`RoadPlan`], DECISIONS D273), as
//! the terrain's does ([`TerrainPlan`](crate::terrain::TerrainPlan)).

use mr_track::FenceGap;

use crate::road::{MarkGap, Road, RoadCtx};
use crate::sea::Sea;
use crate::sky::Sky;
use crate::terrain_mesh::{TerrainSetup, terrain_stages};
use crate::world::{StageFn, Stages, World};

/// What the scenery's `plan()` tells the road before it is built: the
/// openings in roadside fences (`track.fenceGaps`), the stretches without
/// paint (`track.noMarks`) and how far the road runs on past the finish
/// (`track.runout`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadPlan {
    pub fence_gaps: Vec<FenceGap>,
    pub no_marks: Vec<MarkGap>,
    pub runout: f64,
}

impl RoadPlan {
    /// Registers the plan as the scenery would.
    pub fn apply(&self, w: &mut World) {
        if let Some(t) = w.track.as_mut() {
            t.fence_gaps.extend(self.fence_gaps.iter().copied());
            t.runout = self.runout;
        }
        w.no_marks.extend(self.no_marks.iter().copied());
    }
}

/// What a level build needs besides the level.
#[derive(Clone, Default)]
pub struct LevelSetup {
    pub terrain: TerrainSetup,
    /// The recorded road plan, until the scenery is ported.
    pub road: Option<RoadPlan>,
}

/// "Paving roads": `new Road(track, terrain)`, built and added to the
/// world's root; the markings' and chevrons' night parameters; the dew
/// updater.
pub fn road_stage() -> StageFn {
    Box::new(|w: &mut World| {
        let World {
            graph,
            textures,
            track,
            terrain,
            no_marks,
            root,
            ..
        } = w;
        let track = track.as_ref().ok_or("the route is surveyed first")?;
        let terrain = terrain.as_ref().ok_or("no terrain")?;
        let mut road = Road::new(graph, textures);
        let group = road.build(&mut RoadCtx {
            track,
            terrain,
            graph,
            textures,
            no_marks,
        });
        graph.add(*root, group);
        w.add_night(road.material("markings"), "emissiveIntensity", 0.0, 0.06);
        w.add_night(road.material("chevron"), "emissiveIntensity", 0.12, 0.9);
        // Dew on the tarmac after dark: lamps and headlights glint off it.
        w.add_animator(road.dew_animator());
        w.road = Some(road);
        Ok(Vec::new())
    })
}

/// `new Sky(scene, renderer, track, level.sky, level.sunAzimuth)`: the dome
/// and lights as scene roots. The scene shows the sky as a first update at
/// the start of the route would leave it, without a focus (the client
/// updates it every frame: `WorldBuild::update_sky`).
pub fn sky_stage() -> StageFn {
    Box::new(|w: &mut World| {
        let track = w.track.as_ref().ok_or("the route is surveyed first")?;
        let mut sky = Sky::new(&mut w.graph, &mut w.textures, &w.level, track);
        let mut edits = Vec::new();
        sky.update(0.0, 0.0, None, &mut edits);
        sky.apply(&mut w.graph, &edits);
        w.sky = Some(sky);
        Ok(Vec::new())
    })
}

/// "Filling the sea": `new Sea(world, level.sea.y)`, its mesh added to the
/// world's root and its updater registered.
pub fn sea_stage() -> StageFn {
    Box::new(|w: &mut World| {
        let Some(sea_y) = w.level.sea_y else {
            return Ok(Vec::new());
        };
        let terrain = w.terrain.as_ref().ok_or("no terrain")?;
        let sea = Sea::new(&mut w.graph, &mut w.textures, terrain, sea_y);
        w.graph.add(w.root, sea.mesh);
        w.add_animator(sea.animator());
        w.sea = Some(sea);
        Ok(Vec::new())
    })
}

/// Every ported stage of `World.build`: the terrain (with its recorded
/// plan), the road plan registered after the scenery plans, the road, the
/// sky and the sea.
pub fn level_stages(setup: LevelSetup) -> Stages {
    let (terrain, fields, terrain_meshes) = terrain_stages(setup.terrain);
    let plan = setup.road;
    let fields: StageFn = Box::new(move |w: &mut World| {
        // What the scenery's plan() registers for the road happens with its
        // flattens and carves, before buildFields.
        if let Some(p) = &plan {
            p.apply(w);
        }
        fields(w)
    });
    Stages {
        terrain: Some(terrain),
        fields: Some(fields),
        terrain_meshes: Some(terrain_meshes),
        road: Some(road_stage()),
        sky: Some(sky_stage()),
        sea: Some(sea_stage()),
    }
}
