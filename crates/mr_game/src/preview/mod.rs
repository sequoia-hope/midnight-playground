//! The menu's flyover sections (DECISIONS D676, D740 to D749): a short
//! stretch of every level, prepared when the menu opens, so a level tab
//! switches what the attract camera flies over at once. A deviation from
//! the JS game, whose level tab rebuilds the whole world behind the loading
//! screen.
//!
//! **What a section is.** The level's own world build (`mr_worldgen`)
//! restricted to the attract camera's stretch (`mr_worldgen::section`):
//! the terrain tiles near it, the scenery modules whose zones reach it, and
//! of what they build only the drawables near it. Its animators and sky run
//! as on the level. A level the client cannot build whole yet
//! (`animate::generated` false) gets terrain, road, sky and sea only, from
//! the same stages, with no scenery.
//!
//! **How they are held.** Each section is spawned once, under a root
//! entity of its own, hidden unless shown; its GPU copies stay, its CPU
//! copy goes as a level's does (`loader`). Showing one makes its Track,
//! sky, lights, sky noise and animators the client's (`TrackRes`,
//! `SkyRes`, `Lighting`, `EnvRequest`, `animate::WorldGen` and
//! `SceneIndex`) and its root visible: a few component writes, in one
//! frame.
//!
//! **Order.** At boot the saved level's section is built first, behind the
//! loading screen; the others follow behind the menu, one at a time
//! (natively on a thread; on the web a slice of jobs a frame), a tab that
//! asks for one moving it to the front.
//!
//! **Race.** The sections are freed (their entities despawned, their
//! worlds dropped) and the chosen level is built or downloaded whole behind
//! the loading screen, as before (`ui`, `__mr.reload`). **Main menu** after
//! a race keeps the raced level drawn whole for its tab, as the JS does,
//! and prepares the sections again behind the menu; the whole level is
//! freed when another level is shown.

mod hints;

use crate::animate::{self, AnimBlocks, NodeRef, SceneIndex, WorldGen};
use crate::loader::{self, AppState, Loaded, SceneEntity, SectionScene};
use crate::render::lighting::{Lighting, Point, Spot};
use crate::render::pmrem::EnvRequest;
use crate::render::{SharedImages, SkyMaterial, SkyState, ThreeMaterial};
use crate::status::Status;
use crate::{CameraState, Opts, SkyRes, TrackRes};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use mr_levels::SeasideData;
use mr_track::{Level, Track};
use mr_worldgen::section::Section;
use mr_worldgen::world::{Build, WorldBuild};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Where the attract camera starts on a section: `startS + 60`, as
/// `toMenu` puts it.
const CAMERA_FROM: f64 = 60.0;
/// How far it flies before starting over (31 s at 16 m/s).
pub const CAMERA_RUN: f64 = 500.0;
/// The stretch built: from the camera's eye at its start (22 m behind)
/// to past where it looks at its end (D741).
const BEHIND: f64 = 40.0;
const AHEAD: f64 = 100.0;
/// Terrain tiles within this of the stretch are meshed; road (and what
/// else the build draws) within this is kept (D741, D748).
pub const TERRAIN_RADIUS: f64 = 1500.0;
pub const SCENERY_RADIUS: f64 = 600.0;

/// Web: world-build time per frame while the loading screen is up, and
/// behind the menu.
#[cfg(target_arch = "wasm32")]
const BUDGET_LOADING_MS: u128 = 30;
#[cfg(target_arch = "wasm32")]
const BUDGET_MENU_MS: u128 = 8;
/// Spawning a built section's entities, per frame.
const SPAWN_LOADING_MS: u128 = 40;
const SPAWN_MENU_MS: u128 = 6;

/// The menu draws sections (no whole level behind it unless one was raced).
static ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Whether this run opens on sections: a menu-first run (`ui::menu_first`)
/// that is not a test scene, a station run, a fly camera or a given scene
/// file; `?sections=0` keeps the JS's boot (the saved level whole).
pub fn wanted(o: &crate::options::Options) -> bool {
    crate::ui::menu_first(o)
        && o.materials.is_none()
        && o.stations.is_none()
        && o.scene.is_none()
        && o.fly.is_none()
        && o.param("sections") != Some("0")
}

/// Seaside's photo at a quarter of its size, for its menu view (web).
#[cfg(target_arch = "wasm32")]
static SMALL_PHOTO: Mutex<Option<Result<Arc<mr_worldgen::textures::Texture>, String>>> =
    Mutex::new(None);

/// Seaside Raceway's survey, for its Track (the page fetches it at boot
/// in a sections run; natively it is read from the file).
static SURVEY: Mutex<Option<Result<Arc<SeasideData>, String>>> = Mutex::new(None);

pub fn set_survey(bytes: &[u8]) {
    let r = SeasideData::parse(bytes)
        .map(Arc::new)
        .map_err(|e| e.to_string());
    // Seaside's world build reads the same copy (`levels::seaside`).
    crate::levels::seaside::survey_parsed(r.clone());
    *SURVEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
}

fn survey() -> Option<Result<Arc<SeasideData>, String>> {
    SURVEY.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// A section's root: its entities are its children.
#[derive(Component)]
pub struct SectionRoot(pub usize);

/// A section's entity (gathered when it is spawned).
#[derive(Component)]
pub struct SectionPart;

/// A background section's warm-up stand-in (SPEC 6.3): gone once the
/// pipelines have compiled.
#[derive(Component)]
struct SectionWarm;

/// A section that is up: what showing it puts in place.
struct Up {
    root: Entity,
    track: Track,
    level: Level,
    world: Option<WorldBuild>,
    index: Option<SceneIndex>,
    spot: Option<Spot>,
    point: Option<Point>,
    noise: Option<Handle<Image>>,
    loaded: Loaded,
    gathered: bool,
}

/// A built section whose entities are being spawned.
struct Spawn {
    build: loader::Build,
    world: WorldBuild,
    root: Entity,
    track: Track,
    level: Level,
}

enum Stage {
    Queued,
    /// The world build, natively on a thread.
    #[cfg(not(target_arch = "wasm32"))]
    Thread,
    /// The world build, a few jobs a frame (web).
    #[cfg(target_arch = "wasm32")]
    Stepping(Box<Mutex<Build>>),
    /// Built; its entities being spawned.
    Spawning(Box<Spawn>),
    Up(Box<Up>),
    Failed,
}

/// One level's section and what it cost.
struct Sec {
    id: &'static str,
    stage: Stage,
    started: Option<Instant>,
    /// Milliseconds: the world build, then spawning the entities.
    build_ms: f64,
    spawn_ms: f64,
    nodes: usize,
    entities: usize,
    /// Built from the level's own world generation, else terrain and road
    /// only.
    full_kind: bool,
}

/// The menu's sections.
#[derive(Resource)]
pub struct Previews {
    secs: Vec<Sec>,
    /// The section drawn now.
    shown: Option<usize>,
    /// The level the menu has selected.
    want: Option<String>,
    /// The level drawn whole (a race's), if any.
    pub full: Option<String>,
    /// Free the sections next frame (Race).
    free: bool,
    /// Generation of the native build threads (a freed set's results are
    /// dropped).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    generation: u64,
    /// When the sections were asked for, and when the first was shown.
    t0: Option<Instant>,
    pub first_ms: Option<f64>,
    pub all_ms: Option<f64>,
}

/// The native build thread's result: generation, section, the build.
#[cfg(not(target_arch = "wasm32"))]
type ThreadOut = (u64, usize, Result<Box<WorldBuild>, String>);
#[cfg(not(target_arch = "wasm32"))]
static THREAD_OUT: Mutex<Vec<ThreadOut>> = Mutex::new(Vec::new());

impl Previews {
    fn new(level: &str) -> Previews {
        Previews {
            secs: mr_levels::levels()
                .iter()
                .map(|l| Sec {
                    id: l.id,
                    stage: Stage::Queued,
                    started: None,
                    build_ms: 0.0,
                    spawn_ms: 0.0,
                    nodes: 0,
                    entities: 0,
                    full_kind: false,
                })
                .collect(),
            shown: None,
            // The saved level's first (`ui::prepare` put it in the options).
            want: Some(level.to_owned()),
            full: None,
            free: false,
            generation: 0,
            t0: None,
            first_ms: None,
            all_ms: None,
        }
    }

    fn index_of(&self, id: &str) -> Option<usize> {
        self.secs.iter().position(|s| s.id == id)
    }

    /// A level tab: show that level's section (now if it is up, else as
    /// soon as it is).
    pub fn select(&mut self, id: &str) {
        self.want = Some(id.to_owned());
    }

    /// Race: the sections go (next frame); the menu stops drawing them.
    pub fn free_sections(&mut self) {
        ACTIVE.store(false, Ordering::Relaxed);
        self.free = true;
        self.want = None;
    }

    /// Main menu after a race: the raced level stays drawn whole for its
    /// tab; the sections are prepared again behind the menu.
    pub fn back_to_menu(&mut self, level: &str) {
        ACTIVE.store(true, Ordering::Relaxed);
        self.free = false;
        self.want = Some(level.to_owned());
        self.t0 = Some(Instant::now());
        self.all_ms = None;
    }

    /// Whether the level is drawn whole (Race needs no load).
    pub fn has_full(&self, id: &str) -> bool {
        self.full.as_deref() == Some(id)
    }

    /// Whether the selected level's section (or the level) is drawn.
    pub fn showing(&self, id: &str) -> bool {
        self.has_full(id) || self.shown.is_some_and(|k| self.secs[k].id == id)
    }

    /// The next section to build: the selected level's, then the others in
    /// menu order; a level drawn whole goes last.
    fn next(&self) -> Option<usize> {
        let queued = |k: &usize| matches!(self.secs[*k].stage, Stage::Queued);
        if let Some(k) = self.want.as_deref().and_then(|w| self.index_of(w))
            && queued(&k)
            && !self.has_full(self.secs[k].id)
        {
            return Some(k);
        }
        (0..self.secs.len())
            .filter(queued)
            .min_by_key(|&k| self.has_full(self.secs[k].id))
    }

    fn building(&self) -> bool {
        self.secs.iter().any(|s| match s.stage {
            #[cfg(not(target_arch = "wasm32"))]
            Stage::Thread => true,
            #[cfg(target_arch = "wasm32")]
            Stage::Stepping(_) => true,
            Stage::Spawning(_) => true,
            _ => false,
        })
    }
}

/// The section of a level's route that the attract camera flies.
pub fn section_of(track: &Track) -> Section {
    let a = track.start_s + CAMERA_FROM;
    Section {
        s0: a - BEHIND,
        s1: a + CAMERA_RUN + AHEAD,
        terrain_radius: TERRAIN_RADIUS,
        scenery_radius: SCENERY_RADIUS,
        // A simplified view: no scenery module builds (D748).
        scenery: false,
    }
}

/// The level, ready to survey (Seaside with its survey); `None` until the
/// survey is in.
fn level_ready(id: &str) -> Option<Result<Level, String>> {
    // The level's own inputs (Seaside's survey and, on the web, the view's
    // small copy of its photo, D748).
    #[cfg(target_arch = "wasm32")]
    if id == "seaside"
        && SMALL_PHOTO
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
    {
        return None;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if animate::generated(id) && !crate::levels::inputs_ready(id, true) {
        return None;
    }
    let mut level = mr_levels::level_by_id(id);
    if id == "seaside" {
        match survey()? {
            Ok(d) => mr_levels::seaside::prepare(&mut level, d),
            Err(e) => return Some(Err(format!("seaside survey: {e}"))),
        }
    }
    Some(Ok(level))
}

/// A section's world build: the level's own (`animate::section_build`) if
/// world generation builds it, else terrain, road, sky and sea.
fn new_build(level: &Level, section: Section) -> Build {
    use mr_worldgen::stages::{LevelSetup, level_stages};
    use mr_worldgen::terrain_mesh::{TerrainSetup, seaside_ground_color};
    use mr_worldgen::world::{World, level_jobs};
    // On the web, Seaside's view drapes a quarter-size copy of the photo
    // (1 MB, not 16; D748), so it is built here rather than by the level's
    // own setup.
    let small_photo = cfg!(target_arch = "wasm32") && level.id == "seaside";
    if animate::generated(level.id) && !small_photo {
        let mut b = animate::section_build(level.id, section);
        b.push_job(hints::job());
        return b;
    }
    let mut world = World::new(level.clone());
    let mut terrain = TerrainSetup {
        plan: None,
        ..TerrainSetup::default()
    };
    if level.id == "seaside"
        && let Some(Ok(d)) = survey()
    {
        terrain.ground_color = Some(seaside_ground_color(d.clone()));
        #[cfg(target_arch = "wasm32")]
        if let Some(Ok(t)) = SMALL_PHOTO
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            let mut p = mr_worldgen::terrain_mesh::GroundPhoto::seaside(
                &d,
                mr_worldgen::textures::Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![0; 4],
                    source: mr_scene::TextureSource::Image,
                    repeat: false,
                    srgb: true,
                    anisotropy: 8.0,
                },
                crate::levels::seaside::PHOTO_URL,
            );
            p.tex = t;
            terrain.photo = Some(p);
        }
        world = world.with_level_data(d);
    }
    world.section = Some(section);
    let setup = LevelSetup {
        terrain,
        road: None,
    };
    let mut b = Build::new(world, level_jobs(level_stages(setup), |_| None));
    b.push_job(hints::job());
    b
}

/// Runs a section's world build to the end.
#[cfg(not(target_arch = "wasm32"))]
fn run_build(mut b: Build, t: Option<f64>) -> Result<Box<WorldBuild>, String> {
    while !b.is_done() {
        animate::section_step(&mut b, t)?;
    }
    Ok(Box::new(finish_build(b)))
}

/// The cut and the scene (`mr_worldgen::section::finish`).
fn finish_build(b: Build) -> WorldBuild {
    let (wb, stats) = mr_worldgen::section::finish(b);
    debug!("section cut: {stats:?}");
    wb
}

/// Starts, steps and spawns the sections.
#[allow(clippy::too_many_arguments)]
fn drive(
    mut commands: Commands,
    mut pv: ResMut<Previews>,
    opts: Res<Opts>,
    shared: Res<SharedImages>,
    mut status: ResMut<Status>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ThreeMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    if !active() {
        return;
    }
    let pv = &mut *pv;
    let loading = pv.shown.is_none() && pv.full.is_none();
    // Start the next one: the first behind the loading screen, the others
    // once the menu is up (its warm-up done), not while it is compiling.
    if !pv.building()
        && (loading || status.ready)
        && let Some(k) = pv.next()
    {
        let id = pv.secs[k].id;
        match level_ready(id) {
            None => {} // the survey is not in yet
            Some(Err(e)) => {
                warn!("section {id}: {e}");
                pv.secs[k].stage = Stage::Failed;
            }
            Some(Ok(level)) => {
                let track = match Track::new(&level) {
                    Ok(t) => t,
                    Err(e) => {
                        warn!("section {id}: {e}");
                        pv.secs[k].stage = Stage::Failed;
                        return;
                    }
                };
                let sec = section_of(&track);
                let b = new_build(&level, sec);
                let s = &mut pv.secs[k];
                s.started = Some(Instant::now());
                s.full_kind = animate::generated(id);
                pv.t0.get_or_insert_with(Instant::now);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (g, t) = (pv.generation, opts.o.t);
                    std::thread::spawn(move || {
                        let r = run_build(b, t);
                        THREAD_OUT
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((g, k, r));
                    });
                    s.stage = Stage::Thread;
                }
                #[cfg(target_arch = "wasm32")]
                {
                    s.stage = Stage::Stepping(Box::new(Mutex::new(b)));
                }
                if loading {
                    status.state = "building";
                    status.progress = 0.0;
                }
            }
        }
    }
    // A finished world build: the scene goes to the loader, under a root.
    let mut built: Vec<(usize, Result<Box<WorldBuild>, String>)> = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut out = THREAD_OUT.lock().unwrap_or_else(|e| e.into_inner());
        for (g, k, r) in out.drain(..) {
            if g == pv.generation {
                built.push((k, r));
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    for (k, s) in pv.secs.iter_mut().enumerate() {
        let Stage::Stepping(b) = &mut s.stage else {
            continue;
        };
        let b = b.get_mut().unwrap_or_else(|e| e.into_inner());
        let budget = if loading {
            BUDGET_LOADING_MS
        } else {
            BUDGET_MENU_MS
        };
        let t0 = Instant::now();
        let mut r = Ok(());
        while !b.is_done() && r.is_ok() && t0.elapsed().as_millis() < budget {
            r = animate::section_step(b, opts.o.t);
        }
        if loading && let Some((_, f)) = b.progress() {
            status.progress = 0.8 * f as f32;
        }
        match r {
            Err(e) => built.push((k, Err(e))),
            Ok(()) if b.is_done() => {
                if let Stage::Stepping(b) = std::mem::replace(&mut s.stage, Stage::Queued) {
                    let b = b.into_inner().unwrap_or_else(|e| e.into_inner());
                    built.push((k, Ok(Box::new(finish_build(b)))));
                }
            }
            Ok(()) => {}
        }
    }
    for (k, r) in built {
        let s = &mut pv.secs[k];
        match r {
            Err(e) => {
                warn!("section {}: {e}", s.id);
                s.stage = Stage::Failed;
            }
            Ok(mut wb) => {
                s.build_ms = s
                    .started
                    .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
                let scene = std::mem::take(&mut wb.scene);
                s.nodes = scene.nodes.len();
                let Some(Ok(level)) = level_ready(s.id) else {
                    s.stage = Stage::Failed;
                    continue;
                };
                let Ok(mut track) = Track::new(&level) else {
                    s.stage = Stage::Failed;
                    continue;
                };
                // As `make_track` keeps it: the race may start on this Track.
                track.runout = mr_levels::world::world_data(level.id).runout;
                let root = commands
                    .spawn((
                        Transform::default(),
                        Visibility::Hidden,
                        SceneEntity,
                        SectionRoot(k),
                        Name::new(format!("section {}", s.id)),
                    ))
                    .id();
                info!(
                    "section {}: built in {:.0} ms ({} nodes, {} meshes, {} textures, {} animators)",
                    s.id,
                    s.build_ms,
                    scene.nodes.len(),
                    scene.meshes.len(),
                    scene.textures.len(),
                    wb.animators.len()
                );
                s.started = Some(Instant::now());
                s.stage = Stage::Spawning(Box::new(Spawn {
                    build: loader::Build::new(scene, shared.clone()).under(root),
                    world: *wb,
                    root,
                    track,
                    level,
                }));
            }
        }
    }
    // Spawning.
    let budget = if loading {
        SPAWN_LOADING_MS
    } else {
        SPAWN_MENU_MS
    };
    for s in pv.secs.iter_mut() {
        let Stage::Spawning(sp) = &mut s.stage else {
            continue;
        };
        let build = &mut sp.build;
        let done = loader::step_section(
            build,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut sky_materials,
            &mut images,
            budget,
        );
        if loading {
            status.progress = 0.8 + 0.2 * build.progress();
        }
        if !done {
            continue;
        }
        let Stage::Spawning(sp) = std::mem::replace(&mut s.stage, Stage::Queued) else {
            continue;
        };
        let Spawn {
            build,
            world,
            root,
            track,
            level,
        } = *sp;
        let SectionScene {
            loaded,
            index,
            spot,
            point,
            noise,
            warm_up,
        } = loader::finish_section(build, &mut commands, &mut meshes, &mut images);
        // Behind the menu, the stand-ins are the section's own; the first
        // section's are the loading screen's warm-up (`WarmUp`).
        if !loading {
            for e in warm_up {
                commands
                    .entity(e)
                    .remove::<crate::warmup::WarmUp>()
                    .insert(SectionWarm);
            }
        }
        s.spawn_ms = s
            .started
            .map_or(0.0, |t| t.elapsed().as_secs_f64() * 1000.0);
        s.entities = loaded.counts.entities;
        info!(
            "section {}: spawned in {:.0} ms ({} entities)",
            s.id, s.spawn_ms, s.entities
        );
        s.stage = Stage::Up(Box::new(Up {
            root,
            track,
            level,
            world: Some(world),
            index: Some(index),
            spot,
            point,
            noise,
            loaded,
            gathered: false,
        }));
    }
    if pv.all_ms.is_none()
        && pv
            .secs
            .iter()
            .all(|s| matches!(s.stage, Stage::Up(_) | Stage::Failed))
        && let Some(t0) = pv.t0
    {
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        pv.all_ms = Some(ms);
        info!("sections: all up {:.0} ms after the menu asked", ms);
    }
}

/// Background stand-ins go once nothing is compiling.
fn end_section_warm_up(
    mut commands: Commands,
    status: Res<Status>,
    q: Query<Entity, With<SectionWarm>>,
    mut quiet: Local<u32>,
) {
    if q.is_empty() {
        *quiet = 0;
        return;
    }
    *quiet = if status.pipelines_waiting == 0 {
        *quiet + 1
    } else {
        0
    };
    if *quiet >= 3 {
        for e in &q {
            commands.entity(e).despawn();
        }
        *quiet = 0;
    }
}

/// Shows the selected level's section, frees the sections after Race, and
/// a level drawn whole when another is shown.
fn switch(world: &mut World) {
    // Free (Race).
    if world.resource::<Previews>().free {
        let mut roots = Vec::new();
        {
            let mut pv = world.resource_mut::<Previews>();
            pv.free = false;
            pv.generation += 1;
            pv.shown = None;
            for s in pv.secs.iter_mut() {
                match std::mem::replace(&mut s.stage, Stage::Queued) {
                    Stage::Up(up) => roots.push(up.root),
                    Stage::Spawning(sp) => roots.push(sp.root),
                    _ => {}
                }
            }
            pv.t0 = None;
        }
        let warm: Vec<Entity> = world
            .query_filtered::<Entity, With<SectionWarm>>()
            .iter(world)
            .collect();
        for e in roots.into_iter().chain(warm) {
            if let Ok(em) = world.get_entity_mut(e) {
                em.despawn();
            }
        }
        // The shown section's animators and index were the client's.
        if world.resource::<Previews>().full.is_none() {
            animate::take_world(&mut world.resource_mut::<WorldGen>());
            world.remove_resource::<SceneIndex>();
            let mut blocks = world.resource_mut::<AnimBlocks>();
            blocks.texels.clear();
            blocks.map.clear();
        }
        info!("sections freed");
        return;
    }
    if !active() {
        return;
    }
    gather(world);
    let (want, shown, full) = {
        let pv = world.resource::<Previews>();
        (pv.want.clone(), pv.shown, pv.full.clone())
    };
    let Some(want) = want else {
        // At boot, before any tab: the saved level.
        let level = world.resource::<Opts>().o.level.clone();
        world.resource_mut::<Previews>().want = Some(level);
        return;
    };
    if full.as_deref() == Some(want.as_str()) {
        return; // drawn whole
    }
    let Some(k) = world.resource::<Previews>().index_of(&want) else {
        return;
    };
    if shown == Some(k) {
        return;
    }
    {
        let pv = world.resource::<Previews>();
        if !matches!(&pv.secs[k].stage, Stage::Up(up) if up.gathered) {
            return; // shown once it is up
        }
    }
    show(world, k, shown);
}

/// Marks a newly spawned section's entities and indexes them by node.
fn gather(world: &mut World) {
    let todo: Vec<(usize, Entity)> = world
        .resource::<Previews>()
        .secs
        .iter()
        .enumerate()
        .filter_map(|(k, s)| match &s.stage {
            Stage::Up(up) if !up.gathered => Some((k, up.root)),
            _ => None,
        })
        .collect();
    for (k, root) in todo {
        let kids: Vec<(Entity, u32)> = world
            .query::<(Entity, &NodeRef, &ChildOf)>()
            .iter(world)
            .filter(|(_, _, c)| c.parent() == root)
            .map(|(e, n, _)| (e, n.0))
            .collect();
        let others: Vec<Entity> = world
            .query::<(Entity, &ChildOf)>()
            .iter(world)
            .filter(|(_, c)| c.parent() == root)
            .map(|(e, _)| e)
            .collect();
        for e in others {
            world.entity_mut(e).insert(SectionPart);
        }
        let mut pv = world.resource_mut::<Previews>();
        if let Stage::Up(up) = &mut pv.secs[k].stage {
            if let Some(index) = up.index.as_mut() {
                let mut by_node = vec![Vec::new(); index.node_count()];
                for (e, n) in kids {
                    if let Some(v) = by_node.get_mut(n as usize) {
                        v.push(e);
                    }
                }
                index.set_entities(by_node);
            }
            up.gathered = true;
        }
    }
}

/// Makes section `k` the drawn scene: hides the one shown before (or frees
/// the level drawn whole), swaps the Track, sky, lights, noise and
/// animators, and puts the attract camera at its start.
fn show(world: &mut World, k: usize, before: Option<usize>) {
    let t0 = Instant::now();
    // What was drawn goes back to its section, or (a level drawn whole) is
    // freed.
    let old_world = animate::take_world(&mut world.resource_mut::<WorldGen>());
    let old_index = world.remove_resource::<SceneIndex>();
    if let Some(p) = before {
        let mut pv = world.resource_mut::<Previews>();
        if let Stage::Up(up) = &mut pv.secs[p].stage {
            up.world = old_world;
            up.index = old_index;
            let root = up.root;
            world.entity_mut(root).insert(Visibility::Hidden);
        }
    } else if world.resource::<Previews>().full.is_some() {
        drop((old_world, old_index));
        let whole: Vec<Entity> = world
            .query_filtered::<Entity, (
                With<SceneEntity>,
                Without<SectionPart>,
                Without<SectionRoot>,
                Without<SectionWarm>,
            )>()
            .iter(world)
            .collect();
        for e in whole {
            if let Ok(em) = world.get_entity_mut(e) {
                em.despawn();
            }
        }
        world.resource_mut::<Previews>().full = None;
        info!("the level drawn whole is freed");
    }
    // The section's.
    let (root, track, level, wb, index, spot, point, noise, loaded) = {
        let mut pv = world.resource_mut::<Previews>();
        pv.shown = Some(k);
        let Stage::Up(up) = &mut pv.secs[k].stage else {
            return;
        };
        (
            up.root,
            up.track.clone(),
            up.level.clone(),
            up.world.take(),
            up.index.take(),
            up.spot,
            up.point,
            up.noise.clone(),
            up.loaded.clone(),
        )
    };
    world.entity_mut(root).insert(Visibility::Inherited);
    {
        let mut blocks = world.resource_mut::<AnimBlocks>();
        blocks.texels.clear();
        blocks.map.clear();
    }
    if let (Some(wb), Some(mut index)) = (wb, index) {
        index.reset_blocks();
        animate::install_world(&mut world.resource_mut::<WorldGen>(), wb, &index);
        world.insert_resource(index);
    }
    let t = world.resource::<Opts>().o.t;
    let mut sky = SkyState::new(&level, track.is_loop, track.length);
    sky.override_p = t;
    let start = track.start_s + CAMERA_FROM;
    world.resource_mut::<Opts>().o.level = level.id.to_owned();
    *world.resource_mut::<SkyRes>() = SkyRes {
        sky: Some(sky),
        env_at: None,
        frame: None,
    };
    *world.resource_mut::<TrackRes>() = TrackRes {
        track: Some(track),
        level: Some(level),
        none: false,
    };
    {
        let mut l = world.resource_mut::<Lighting>();
        l.spot = spot;
        l.point = point;
    }
    world.resource_mut::<EnvRequest>().noise = noise;
    world.resource_mut::<CameraState>().attract.s = start;
    world.resource_mut::<Status>().route = None;
    world.insert_resource(loaded.clone());
    // The first: the loading screen's warm-up, then the menu.
    if *world.resource::<State<AppState>>().get() != AppState::Running {
        world.resource_mut::<Status>().counts = Some(loaded.counts.clone());
        world
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Running);
    }
    let mut pv = world.resource_mut::<Previews>();
    if pv.first_ms.is_none()
        && let Some(t0) = pv.t0
    {
        pv.first_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
    }
    info!(
        "section {} shown in {:.2} ms",
        pv.secs[k].id,
        t0.elapsed().as_secs_f64() * 1000.0
    );
}

/// The attract camera starts over at the end of a section's stretch (the
/// JS's runs the whole first zone, which a section does not have).
fn wrap_attract(pv: Res<Previews>, tr: Res<TrackRes>, mut cs: ResMut<CameraState>) {
    if !active() || pv.shown.is_none() {
        return;
    }
    let Some(t) = &tr.track else { return };
    let a = t.start_s + CAMERA_FROM;
    if cs.attract.s > a + CAMERA_RUN || cs.attract.s < a - 1.0 {
        cs.attract.s = a;
    }
    // `sectionshots=`: the camera held at `shot_s=` metres into the run.
    if let Some(at) = SHOT_AT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        cs.attract.s = a + at;
    }
}

/// `sectionshots=<dir>` (native): the camera's place in the run.
static SHOT_AT: Mutex<Option<f64>> = Mutex::new(None);

/// Natively, `--query sectionshots=<dir>[&shot_s=200]`: once every
/// section is up, shows each level's in turn, the camera held `shot_s` m
/// into its run, saves `<dir>/<level>.png` and the timing of each switch,
/// then exits (`menu=0` leaves the menu off the pictures).
#[cfg(not(target_arch = "wasm32"))]
fn section_shots(
    mut commands: Commands,
    mut pv: ResMut<Previews>,
    opts: Res<Opts>,
    status: Res<Status>,
    mut step: Local<(usize, u32)>,
) {
    use bevy::render::view::screenshot::{Screenshot, save_to_disk};
    let Some(dir) = opts.o.param("sectionshots").map(str::to_owned) else {
        return;
    };
    let at = opts
        .o
        .param("shot_s")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200.0);
    *SHOT_AT.lock().unwrap_or_else(|e| e.into_inner()) = Some(at);
    if !status.ready || pv.all_ms.is_none() {
        return;
    }
    let (k, frames) = *step;
    if k > pv.secs.len() {
        return;
    }
    if k == pv.secs.len() {
        if frames > 30 {
            info!("sectionshots: done");
            std::process::exit(0);
        }
        step.1 += 1;
        return;
    }
    let id = pv.secs[k].id;
    if frames == 0 {
        pv.select(id);
    }
    step.1 += 1;
    if pv.shown_id() == Some(id) && frames == 60 {
        let _ = std::fs::create_dir_all(&dir);
        let path = format!("{dir}/{id}.png");
        info!("sectionshots: {path}");
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if frames >= 90 {
        *step = (k + 1, 0);
    }
}

/// A level's whole scene is up (a race's): the menu may draw it for its tab.
fn note_full(mut pv: ResMut<Previews>, opts: Res<Opts>) {
    if !active() {
        pv.full = Some(opts.o.level.clone());
    }
}

/// A scene torn down (Race's load): nothing is drawn whole.
fn note_unload(mut pv: ResMut<Previews>) {
    if !active() {
        pv.full = None;
        pv.shown = None;
    }
}

/// What the sections cost, for the page and the tests (`__mr.sections`).
pub struct Report {
    pub id: &'static str,
    pub state: &'static str,
    pub build_ms: f64,
    pub spawn_ms: f64,
    pub nodes: usize,
    pub entities: usize,
    pub generated: bool,
}

impl Previews {
    pub fn report(&self) -> Vec<Report> {
        self.secs
            .iter()
            .map(|s| Report {
                id: s.id,
                state: match s.stage {
                    Stage::Queued => "queued",
                    Stage::Up(_) => "up",
                    Stage::Failed => "failed",
                    _ => "building",
                },
                build_ms: s.build_ms,
                spawn_ms: s.spawn_ms,
                nodes: s.nodes,
                entities: s.entities,
                generated: s.full_kind,
            })
            .collect()
    }

    /// The section drawn now, by level.
    pub fn shown_id(&self) -> Option<&'static str> {
        self.shown.map(|k| self.secs[k].id)
    }
}

pub fn plugin(app: &mut App) {
    let (on, level) = {
        let o = &app.world().resource::<Opts>().o;
        (wanted(o), o.level.clone())
    };
    ACTIVE.store(on, Ordering::Relaxed);
    #[cfg(not(target_arch = "wasm32"))]
    if on {
        let p = crate::native::repo_root().join("assets/seaside/survey.bin");
        match std::fs::read(&p) {
            Ok(b) => set_survey(&b),
            Err(e) => {
                let e = format!("{}: {e}", p.display());
                crate::levels::seaside::survey_parsed(Err(e.clone()));
                *SURVEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(e));
            }
        }
    }
    app.insert_resource(Previews::new(&level))
        .add_systems(Update, (drive, switch).chain())
        .add_systems(Update, end_section_warm_up)
        .add_systems(Update, wrap_attract.before(crate::fly_system))
        .add_systems(OnEnter(AppState::Running), note_full)
        .add_systems(OnEnter(AppState::Waiting), note_unload);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(Update, section_shots.after(switch));
    #[cfg(target_arch = "wasm32")]
    web::plugin(app);
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::*;
    use js_sys::{Array, Object, Reflect};
    use wasm_bindgen::prelude::*;

    /// Whether this run opens on the sections (the page then downloads no
    /// level, and fetches Seaside's survey for its section).
    #[wasm_bindgen]
    pub fn menu_sections() -> bool {
        active()
    }

    /// Seaside Raceway's survey, for its section.
    #[wasm_bindgen]
    pub fn section_survey(bytes: &[u8]) {
        set_survey(bytes);
    }

    /// Seaside's photo, scaled to a quarter by the page, for its view.
    #[wasm_bindgen]
    pub fn section_photo(width: u32, height: u32, rgba: Vec<u8>) {
        let r = if rgba.len() == (width * height * 4) as usize {
            Ok(Arc::new(mr_worldgen::textures::Texture {
                width,
                height,
                rgba,
                source: mr_scene::TextureSource::Image,
                repeat: false,
                srgb: true,
                anisotropy: 8.0,
            }))
        } else {
            Err(format!(
                "section photo: {} bytes for {width} × {height}",
                rgba.len()
            ))
        };
        *SMALL_PHOTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
    }

    #[wasm_bindgen]
    pub fn section_photo_failed(message: String) {
        *SMALL_PHOTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(message));
    }

    #[wasm_bindgen]
    pub fn section_survey_failed(message: String) {
        crate::levels::seaside::survey_parsed(Err(message.clone()));
        *SURVEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(message));
    }

    fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
        let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
    }

    /// `__mr.sections`: per level its state and costs, the one shown, and
    /// the times to the first and to all.
    fn publish(pv: Res<Previews>, mut last: Local<String>) {
        let rep = pv.report();
        let key = format!(
            "{:?}{:?}{:?}{:?}{}",
            rep.iter().map(|r| r.state).collect::<Vec<_>>(),
            pv.shown_id(),
            pv.full,
            pv.all_ms,
            active()
        );
        if *last == key {
            return;
        }
        *last = key;
        let Some(w) = web_sys::window() else { return };
        let Ok(mr) = Reflect::get(&w, &JsValue::from_str("__mr")) else {
            return;
        };
        let Some(mr) = mr.dyn_ref::<Object>() else {
            return;
        };
        let o = Object::new();
        let list = Array::new();
        for r in rep {
            let e = Object::new();
            set(&e, "id", r.id);
            set(&e, "state", r.state);
            set(&e, "buildMs", r.build_ms.round());
            set(&e, "spawnMs", r.spawn_ms.round());
            set(&e, "nodes", r.nodes as f64);
            set(&e, "entities", r.entities as f64);
            set(&e, "generated", r.generated);
            list.push(&e);
        }
        set(&o, "list", list);
        set(&o, "active", active());
        set(
            &o,
            "shown",
            pv.shown_id().map_or(JsValue::NULL, JsValue::from_str),
        );
        set(
            &o,
            "full",
            pv.full.as_deref().map_or(JsValue::NULL, JsValue::from_str),
        );
        set(
            &o,
            "firstMs",
            pv.first_ms.map_or(JsValue::NULL, JsValue::from_f64),
        );
        set(
            &o,
            "allMs",
            pv.all_ms.map_or(JsValue::NULL, JsValue::from_f64),
        );
        set(mr, "sections", o);
    }

    pub fn plugin(app: &mut App) {
        app.add_systems(Last, publish);
    }
}
