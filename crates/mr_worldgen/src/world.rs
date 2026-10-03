//! The world build of `src/world/World.js` as data and jobs (SPEC 5.1, 5.5;
//! roadmap WP 3.3): [`World`] is what the JS `World` holds while a level is
//! built, [`WorldBuild`] what a finished build hands the client, an
//! [`Animator`] stands in for a closure in `world.updaters`, and a build is
//! a list of [`Job`]s carrying the JS progress labels.
//!
//! The JS builds in stages with `setTimeout(0)` yields between them,
//! reporting `progress(label, fraction)` before each:
//!
//! | Label | Fraction | Work |
//! |---|---|---|
//! | Surveying the route | 0.02 | `level.prepare()`, `new Track(level)` |
//! | Shaping the land | 0.06 | `new Terrain`, load scenery, each `plan()`, `buildFields`, `resolveFlattens` |
//! | Sculpting terrain | 0.1 (then 0.1 + f × 0.55) | terrain meshes |
//! | Paving roads | 0.67 | Road, its night parameters and dew updater, Sky |
//! | Filling the sea | 0.7 | Sea (levels with a sea) |
//! | each scenery's label | 0.72 + k / n × 0.26 | its `build()` |
//! | Ready | 1 | |
//!
//! [`level_jobs`] makes that list. The stages that later packages port
//! (terrain, road, sky, sea: WP 3.4 and 3.5; the scenery modules: WP 3.6
//! on) plug in through [`Stages`] and the [`Scenery`] trait; until then a
//! stage does nothing but report its label.
//!
//! A job runs on the [`World`] and may return more jobs, which run next
//! (the terrain reports its tiles that way, the scenery builds are queued
//! once `plan()` has said which modules survive). A [`Job::Parallel`] job
//! is a set of pure pieces that each build a [`Part`] from a read-only
//! world; the parts are merged in their order, so the result is the same
//! whether they ran one after another (wasm, which has one thread) or on
//! threads (native, with the `parallel` feature). The client drives a
//! [`Build`]: on the web a few jobs per frame, natively all at once.

use std::collections::VecDeque;

use mr_levels::world::WorldData;
use mr_scene::{NightParam, Scene};
use mr_track::{Level, Track};

use crate::object::{Bases, GeoId, HandleMap, MaterialId, NodeId, SceneGraph, TextureId};
use crate::road::{MarkGap, Road};
use crate::sea::Sea;
use crate::sky::{Sky, SkyFrame};
use crate::terrain::Terrain;
use crate::textures::TextureCache;

/// What the simulation needs from the world (SPEC 4.3): the runout and the
/// opposite carriageway, which the JS scenery sets while it builds.
pub type SimWorldData = WorldData;

// ── Animators ───────────────────────────────────────────────────────────

/// What an updater sees of the camera: `camera.position`, `camera.fov`,
/// and the drawing buffer's height (`renderer.domElement.height`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    pub position: [f64; 3],
    pub fov: f64,
    pub viewport_height: f64,
}

/// An updater's arguments: `(dt, night, camera, s)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UpdateCtx {
    pub dt: f64,
    /// The sky's night factor n (0 day, 1 night).
    pub night: f64,
    /// `None` where the JS passes no camera.
    pub camera: Option<CameraView>,
    /// The player's distance along the track.
    pub s: f64,
}

/// What an edit addresses: a handle of the world that was built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Node(NodeId),
    Material(MaterialId),
    Geometry(GeoId),
    Texture(TextureId),
}

/// The same target as an index into the assembled [`Scene`]'s nodes,
/// materials, meshes or textures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneRef {
    Node(u32),
    Material(u32),
    Mesh(u32),
    Texture(u32),
}

/// One change an animator makes for this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// `object.visible = v`.
    Visible(bool),
    /// An object's position, rotation (quaternion x, y, z, w) and scale.
    Transform {
        position: [f64; 3],
        quaternion: [f64; 4],
        scale: [f64; 3],
    },
    /// `setMatrixAt(index, m)` (column-major, as stored).
    InstanceMatrix { index: u32, matrix: [f32; 16] },
    /// `setColorAt(index, c)`.
    InstanceColor { index: u32, rgb: [f32; 3] },
    /// `instancedMesh.count = n`.
    InstanceCount(u32),
    /// Values written into a geometry attribute from `offset` (in floats).
    Attribute {
        name: &'static str,
        offset: usize,
        values: Vec<f32>,
    },
    /// A material number or a uniform's value (`emissiveIntensity`,
    /// `size`, `uTime`, ...).
    Number { prop: &'static str, value: f64 },
    /// A material colour or colour uniform, linear.
    Color { prop: &'static str, rgb: [f64; 3] },
    /// `texture.offset.set(u, v)`.
    TextureOffset([f64; 2]),
    /// A vector uniform (`uSunDir`, the sea's `uOff2`), as many components
    /// as it has.
    Vector { prop: &'static str, value: Vec<f64> },
    /// A light's colour and intensity (linear), and a hemisphere light's
    /// ground colour.
    Light {
        color: [f64; 3],
        intensity: f64,
        ground_color: Option<[f64; 3]>,
    },
}

/// An animator's edit.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub target: Handle,
    pub change: Change,
}

/// An edit resolved to the scene.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneEdit {
    pub target: SceneRef,
    pub change: Change,
}

/// A closure of `world.updaters` (SPEC 5.1): called every frame, in the
/// order registered, after the night parameters are applied; it writes its
/// changes as [`Edit`]s.
pub trait Animator: Send + Sync {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>);
}

impl<F: FnMut(&UpdateCtx, &mut Vec<Edit>) + Send + Sync> Animator for F {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        self(u, out)
    }
}

/// An animator built in a [`Part`], its handles moved by the merge.
struct Rebased {
    inner: Box<dyn Animator>,
    bases: Bases,
}

impl Animator for Rebased {
    fn update(&mut self, u: &UpdateCtx, out: &mut Vec<Edit>) {
        let from = out.len();
        self.inner.update(u, out);
        let b = self.bases;
        for e in &mut out[from..] {
            e.target = match e.target {
                Handle::Node(n) => Handle::Node(b.node(n)),
                Handle::Material(m) => Handle::Material(b.material(m)),
                Handle::Geometry(g) => Handle::Geometry(b.geometry(g)),
                Handle::Texture(t) => Handle::Texture(b.texture(t)),
            };
        }
    }
}

// ── The world being built ───────────────────────────────────────────────

/// The JS `World` during a build: the level, the track once surveyed,
/// everything made so far, and the per-frame hooks. Later packages add
/// what they port (terrain: WP 3.4; road, sky, sea: WP 3.5).
pub struct World {
    pub level: Level,
    pub track: Option<Track>,
    pub graph: SceneGraph,
    /// `world.root` (`world.scene`): the group `world:<id>` scenery adds to.
    pub root: NodeId,
    /// `world.updaters`.
    pub animators: Vec<Box<dyn Animator>>,
    /// The texture module's cache (`world/textures.js`).
    pub textures: TextureCache,
    pub sim_data: SimWorldData,
    /// `world.terrain`, from "Shaping the land" on (WP 3.4).
    pub terrain: Option<Terrain>,
    /// `world.terrainMaterial`.
    pub terrain_material: Option<MaterialId>,
    /// `track.noMarks`: where scenery's `plan()` blanks the road paint
    /// (mr_track's Track does not carry it, so the world does).
    pub no_marks: Vec<MarkGap>,
    /// `world.road`, from "Paving roads" on (WP 3.5).
    pub road: Option<Road>,
    /// `world.sky`.
    pub sky: Option<Sky>,
    /// `world.sea`, on levels with a sea.
    pub sea: Option<Sea>,
}

impl World {
    /// `new World(scene, renderer, level)`: the root group, added to the
    /// scene. `level` is ready to survey (Seaside prepared, DECISIONS D54).
    pub fn new(level: Level) -> World {
        let mut graph = SceneGraph::new();
        let root = graph.group(&format!("world:{}", level.id));
        graph.add_root(root);
        World {
            level,
            track: None,
            graph,
            root,
            animators: Vec::new(),
            textures: TextureCache::new(),
            sim_data: WorldData {
                runout: 0.0,
                opposite_carriageway: None,
            },
            terrain: None,
            terrain_material: None,
            no_marks: Vec::new(),
            road: None,
            sky: None,
            sea: None,
        }
    }

    /// The track (surveyed by the first job).
    pub fn track(&self) -> &Track {
        self.track.as_ref().expect("the route is surveyed first")
    }

    /// `zoneIndex(key)`: `None` where the JS gives -1.
    pub fn zone_index(&self, key: &str) -> Option<usize> {
        self.level.zones.iter().position(|z| z.key == key)
    }

    /// `addNight(material, prop, day, night)`: register a material
    /// property that should follow nightfall.
    pub fn add_night(&mut self, material: Option<MaterialId>, prop: &str, day: f64, night: f64) {
        self.graph.add_night(material, prop, day, night);
    }

    /// `updaters.push(fn)`.
    pub fn add_animator(&mut self, a: impl Animator + 'static) {
        self.animators.push(Box::new(a));
    }

    /// Merges a part built by a parallel job: its roots go under `parent`
    /// (the world root if `None`), its animators after the world's.
    pub fn merge(&mut self, part: Part, parent: Option<NodeId>) {
        let bases = self
            .graph
            .append(part.graph, Some(parent.unwrap_or(self.root)));
        for a in part.animators {
            self.animators.push(Box::new(Rebased { inner: a, bases }));
        }
    }

    /// The scene and everything the client needs to run it.
    pub fn finish(self) -> WorldBuild {
        let (mut scene, handles) = self.graph.finish();
        scene.meta = serde_json::json!({
            "name": self.level.id,
            "level": self.level.id,
            "generator": "mr_worldgen",
        });
        WorldBuild {
            scene,
            animators: self.animators,
            sim_data: self.sim_data,
            handles,
            log: self.graph.log,
            sky: self.sky,
        }
    }
}

/// What a parallel piece builds: a graph of its own (its handles count
/// from zero; its roots are added under the job's parent) and animators
/// addressing it.
#[derive(Default)]
pub struct Part {
    pub graph: SceneGraph,
    pub animators: Vec<Box<dyn Animator>>,
}

/// What a level build returns (SPEC 5.1).
pub struct WorldBuild {
    pub scene: Scene,
    pub animators: Vec<Box<dyn Animator>>,
    pub sim_data: SimWorldData,
    /// Handles of the build → indices in `scene`.
    pub handles: HandleMap,
    /// What the JS would have logged.
    pub log: Vec<String>,
    /// The sky (`world.sky`): the time of day along the route and the
    /// handles of the dome and the lights.
    pub sky: Option<Sky>,
}

/// A night parameter's value at night factor n: `day + (night - day) * n`.
pub fn night_value(p: &NightParam, n: f64) -> f64 {
    p.day + (p.night - p.day) * n
}

impl WorldBuild {
    /// The start of `world.update(dt, s, focus)`: the sky at the player's
    /// distance `s` (its frame: lights, fog, exposure, the night factor)
    /// and its edits to the dome, the lights and their nodes, resolved to
    /// the scene. Then the client applies the night parameters at
    /// `frame.night` and runs [`WorldBuild::update`].
    pub fn update_sky(
        &mut self,
        dt: f64,
        s: f64,
        focus: Option<[f64; 3]>,
    ) -> Option<(SkyFrame, Vec<SceneEdit>)> {
        let sky = self.sky.as_mut()?;
        let mut edits = Vec::new();
        let frame = sky.update(dt, s, focus, &mut edits);
        Some((frame, self.resolve(edits)))
    }

    /// Edits addressed by handle → edits of the scene (those of things
    /// the scene left out dropped).
    pub fn resolve(&self, edits: Vec<Edit>) -> Vec<SceneEdit> {
        edits
            .into_iter()
            .filter_map(|e| {
                let target = match e.target {
                    Handle::Node(n) => SceneRef::Node(self.handles.node(n)?),
                    Handle::Material(m) => SceneRef::Material(self.handles.material(m)?),
                    Handle::Geometry(g) => SceneRef::Mesh(self.handles.mesh(g)?),
                    Handle::Texture(t) => SceneRef::Texture(self.handles.texture(t)?),
                };
                Some(SceneEdit {
                    target,
                    change: e.change,
                })
            })
            .collect()
    }

    /// One frame of `world.update` after the sky: every animator in order,
    /// its edits resolved to the scene (edits of things the scene left out
    /// are dropped). The night parameters are the client's to apply first
    /// ([`night_value`]).
    pub fn update(&mut self, u: &UpdateCtx) -> Vec<SceneEdit> {
        let mut edits = Vec::new();
        for a in &mut self.animators {
            a.update(u, &mut edits);
        }
        edits
            .into_iter()
            .filter_map(|e| {
                let target = match e.target {
                    Handle::Node(n) => SceneRef::Node(self.handles.node(n)?),
                    Handle::Material(m) => SceneRef::Material(self.handles.material(m)?),
                    Handle::Geometry(g) => SceneRef::Mesh(self.handles.mesh(g)?),
                    Handle::Texture(t) => SceneRef::Texture(self.handles.texture(t)?),
                };
                Some(SceneEdit {
                    target,
                    change: e.change,
                })
            })
            .collect()
    }
}

// ── Jobs ────────────────────────────────────────────────────────────────

/// Work on the world; returns jobs to run next.
pub type JobFn = Box<dyn FnOnce(&mut World) -> Result<Vec<Job>, String> + Send>;

/// One pure piece of a parallel job.
pub type PartFn = Box<dyn FnOnce(&World) -> Part + Send>;

/// One step of a build. Its label and fraction are what the loading screen
/// shows while it runs (`progress(label, frac)` before the work, as the
/// JS reports them).
pub enum Job {
    Serial {
        label: String,
        progress: f64,
        run: JobFn,
    },
    Parallel {
        label: String,
        progress: f64,
        /// Where the parts' roots go (the world root if `None`).
        parent: Option<NodeId>,
        parts: Vec<PartFn>,
    },
}

impl Job {
    pub fn serial(
        label: &str,
        progress: f64,
        run: impl FnOnce(&mut World) -> Result<Vec<Job>, String> + Send + 'static,
    ) -> Job {
        Job::Serial {
            label: label.to_string(),
            progress,
            run: Box::new(run),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Job::Serial { label, .. } | Job::Parallel { label, .. } => label,
        }
    }

    pub fn progress(&self) -> f64 {
        match self {
            Job::Serial { progress, .. } | Job::Parallel { progress, .. } => *progress,
        }
    }
}

/// Runs the pieces and returns their parts in order.
fn run_parts(world: &World, parts: Vec<PartFn>) -> Vec<Part> {
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        if threads > 1 && parts.len() > 1 {
            let per = parts.len().div_ceil(threads);
            let mut batches: Vec<Vec<PartFn>> = Vec::new();
            let mut it = parts.into_iter().peekable();
            while it.peek().is_some() {
                batches.push(it.by_ref().take(per).collect());
            }
            return std::thread::scope(|sc| {
                let handles: Vec<_> = batches
                    .into_iter()
                    .map(|b| sc.spawn(move || b.into_iter().map(|f| f(world)).collect::<Vec<_>>()))
                    .collect();
                handles
                    .into_iter()
                    .flat_map(|h| h.join().expect("a build job panicked"))
                    .collect()
            });
        }
    }
    parts.into_iter().map(|f| f(world)).collect()
}

/// A build in progress: the world and the jobs still to run.
pub struct Build {
    pub world: World,
    queue: VecDeque<Job>,
}

impl Build {
    pub fn new(world: World, jobs: Vec<Job>) -> Build {
        Build {
            world,
            queue: jobs.into(),
        }
    }

    /// The label and fraction of the job that runs next (`None` when
    /// done): what the loading bar shows.
    pub fn progress(&self) -> Option<(&str, f64)> {
        self.queue.front().map(|j| (j.label(), j.progress()))
    }

    pub fn is_done(&self) -> bool {
        self.queue.is_empty()
    }

    /// Runs the next job. Jobs it returns run next, before the rest.
    pub fn step(&mut self) -> Result<(), String> {
        let Some(job) = self.queue.pop_front() else {
            return Ok(());
        };
        let more = match job {
            Job::Serial { run, .. } => run(&mut self.world)?,
            Job::Parallel { parent, parts, .. } => {
                for part in run_parts(&self.world, parts) {
                    self.world.merge(part, parent);
                }
                Vec::new()
            }
        };
        for j in more.into_iter().rev() {
            self.queue.push_front(j);
        }
        Ok(())
    }

    /// Runs every job, calling `progress(label, fraction)` before each.
    pub fn run(mut self, mut progress: impl FnMut(&str, f64)) -> Result<WorldBuild, String> {
        while let Some((label, frac)) = self.progress() {
            progress(label, frac);
            self.step()?;
        }
        Ok(self.world.finish())
    }

    pub fn finish(self) -> WorldBuild {
        self.world.finish()
    }
}

// ── The level's job list ────────────────────────────────────────────────

/// A scenery module (`world/<Name>.js`'s default export): constructed with
/// `{ zone, key, level }`, it may `plan()` before the terrain's fields are
/// built and `build()` after the road.
pub trait Scenery: Send {
    /// The JS class name, for messages.
    fn name(&self) -> &str;
    /// `s.label` (the loading screen shows `'Building scenery'` without).
    fn label(&self) -> Option<&str>;
    /// `plan(world)`: flattens, carves. An error drops the module.
    fn plan(&mut self, _world: &mut World) -> Result<(), String> {
        Ok(())
    }
    /// `build(world)`. An error is logged and the build carries on.
    fn build(&mut self, world: &mut World) -> Result<(), String>;
}

/// One scenery module a level asks for: `{ zone, key }` as `loadScenery`
/// passes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneryInfo {
    pub name: &'static str,
    /// The first zone that names it.
    pub zone: usize,
    pub key: &'static str,
}

/// `loadScenery`'s list: each distinct `zone.scenery` in order of first
/// appearance.
pub fn scenery_modules(level: &Level) -> Vec<SceneryInfo> {
    let mut out: Vec<SceneryInfo> = Vec::new();
    for z in &level.zones {
        if z.scenery.is_empty() || out.iter().any(|s| s.name == z.scenery) {
            continue;
        }
        let zone = level
            .zones
            .iter()
            .position(|y| y.scenery == z.scenery)
            .expect("found");
        out.push(SceneryInfo {
            name: z.scenery,
            zone,
            key: level.zones[zone].key,
        });
    }
    out
}

/// The `label` each scenery class sets.
pub const SCENERY_LABELS: &[(&str, &str)] = &[
    ("Mountain", "Mountain scenery"),
    ("Valley", "Planting the valley"),
    ("City", "Building the city"),
    ("Coast", "Carving the coast"),
    ("Beach", "Building Seabright"),
    ("Harbor", "Building the harbour"),
    ("Streets", "Building downtown"),
    ("Desert", "Painting the desert"),
    ("Raceway", "Building the raceway"),
];

/// The label of a scenery class by name.
pub fn scenery_label(name: &str) -> Option<&'static str> {
    SCENERY_LABELS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|&(_, l)| l)
}

/// A stage's work, where a later package supplies it.
pub type StageFn = Box<dyn FnOnce(&mut World) -> Result<Vec<Job>, String> + Send>;

/// The stages of `World.build` that other packages port. Each runs inside
/// the job of its label; a stage left `None` does nothing yet.
#[derive(Default)]
pub struct Stages {
    /// "Shaping the land", first: `new Terrain(track, level)`.
    pub terrain: Option<StageFn>,
    /// "Shaping the land", after the scenery plans: `buildFields()`,
    /// `resolveFlattens()`.
    pub fields: Option<StageFn>,
    /// "Sculpting terrain": the terrain meshes. It may return jobs that
    /// report `0.1 + f * 0.55` as tiles are done.
    pub terrain_meshes: Option<StageFn>,
    /// "Paving roads": the road, its night parameters and dew updater.
    pub road: Option<StageFn>,
    /// The sky (in the "Paving roads" job, as in the JS).
    pub sky: Option<StageFn>,
    /// "Filling the sea", on levels with a sea.
    pub sea: Option<StageFn>,
}

fn run_stage(stage: Option<StageFn>, w: &mut World) -> Result<Vec<Job>, String> {
    match stage {
        Some(f) => f(w),
        None => Ok(Vec::new()),
    }
}

/// `World.build` as jobs. `scenery` makes the module for each entry of
/// [`scenery_modules`] (`None`: the module is missing, logged and skipped,
/// as a failed `import()` is).
pub fn level_jobs(
    stages: Stages,
    scenery: impl Fn(&SceneryInfo) -> Option<Box<dyn Scenery>> + Send + 'static,
) -> Vec<Job> {
    let Stages {
        terrain,
        fields,
        terrain_meshes,
        road,
        sky,
        sea,
    } = stages;
    let survey = Job::serial("Surveying the route", 0.02, |w| {
        // Levels built from data files load them first (Seaside Raceway):
        // the caller prepares the level before the build (D54).
        w.track = Some(Track::new(&w.level)?);
        Ok(Vec::new())
    });
    let shape = Job::serial("Shaping the land", 0.06, move |w| {
        let mut out = run_stage(terrain, w)?;
        let mut modules: Vec<Box<dyn Scenery>> = Vec::new();
        for info in scenery_modules(&w.level) {
            match scenery(&info) {
                Some(s) => modules.push(s),
                None => w.graph.log.push(format!("{} scenery missing", info.name)),
            }
        }
        // Scenery may carve creeks or flatten building pads before heights are
        // final. A broken scenery module logs and is skipped rather than
        // stopping the level from loading.
        let mut kept = Vec::new();
        for mut s in modules {
            match s.plan(w) {
                Ok(()) => kept.push(s),
                Err(e) => w.graph.log.push(format!("{}.plan failed {e}", s.name())),
            }
        }
        out.extend(run_stage(fields, w)?);

        out.push(Job::serial("Sculpting terrain", 0.1, move |w| {
            run_stage(terrain_meshes, w)
        }));
        out.push(Job::serial("Paving roads", 0.67, move |w| {
            let mut more = run_stage(road, w)?;
            more.extend(run_stage(sky, w)?);
            Ok(more)
        }));
        if w.level.sea_y.is_some() {
            out.push(Job::serial("Filling the sea", 0.7, move |w| {
                run_stage(sea, w)
            }));
        }
        let n = kept.len();
        for (k, mut s) in kept.into_iter().enumerate() {
            let label = s.label().unwrap_or("Building scenery").to_string();
            let frac = 0.72 + (k as f64 / n as f64) * 0.26;
            out.push(Job::serial(&label, frac, move |w| {
                if let Err(e) = s.build(w) {
                    w.graph.log.push(format!("{}.build failed {e}", s.name()));
                }
                Ok(Vec::new())
            }));
        }
        out.push(Job::serial("Ready", 1.0, |_| Ok(Vec::new())));
        Ok(out)
    });
    vec![survey, shape]
}
