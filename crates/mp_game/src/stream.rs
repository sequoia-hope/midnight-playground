//! Streaming the open world in the client (docs/vision/peninsula.md, stage
//! 4b; DECISIONS D1214). A streamed level is not built whole: it is
//! planned once (`mp_worldgen::stream::Planned`, on the build thread
//! natively, inline on the web behind the loading screen), its base (the
//! coarse terrain, the sky, the sea) becomes the drawn scene as a level's
//! build would, and its route is built in sections of a kilometre round
//! the player, each spawned under a root of its own as the menu's views
//! are (`loader::step_section`), its animators run beside the base's
//! (`animate::run_animators`), and dropped again behind.
//!
//! The level is `ready` once the sections round the start are up; from
//! then on the ring follows the camera's focus along the route: the
//! section it is in, one behind and two ahead, built one at a time on
//! the thread (natively) or by the builder's worker (the web, stage 4c:
//! `crate::worker`, `mp_builder` as its own wasm; the packed section
//! read back here), or, without the worker (`?worker=0`, or its wasm
//! failed), stepped between frames with a small budget, where a kit's
//! build is one job and can hitch a frame.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bevy::math::DVec3;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use mp_worldgen::section::{self, CutStats};
use mp_worldgen::stages::LevelSetup;
use mp_worldgen::stream::Planned;
use mp_worldgen::terrain_mesh::TerrainSetup;
use mp_worldgen::world::{Build, WorldBuild};

use crate::animate::{self, NodeRef, SceneIndex, WorldGen};
use crate::loader::{self, AppState, ImageCache, SceneEntity};
use crate::render::material::SharedImages;
use crate::render::{SkyMaterial, ThreeMaterial};
use crate::status::Status;
use crate::{CameraState, Opts, inbox};

/// Sections ahead of the camera's, and behind, that are kept up.
const AHEAD: i64 = 2;
const BEHIND: i64 = 1;
/// A section this far outside the ring is dropped (hysteresis).
const SLACK: i64 = 1;
/// Milliseconds a frame for stepping a build and for spawning, behind the
/// loading screen and while running (the web; natively the builds are on
/// the thread and only the spawning is stepped).
#[cfg(target_arch = "wasm32")]
const BUILD_LOADING_MS: u128 = 30;
#[cfg(target_arch = "wasm32")]
const BUILD_RUNNING_MS: u128 = 6;
const SPAWN_LOADING_MS: u128 = 40;
const SPAWN_RUNNING_MS: u128 = 5;

/// Whether the level is streamed: an open level the client builds, unless
/// `?stream=0` asks for it whole (the whole loop is 223 MB of buffers).
pub fn streamed(o: &crate::options::Options) -> bool {
    static CACHE: Mutex<Option<(String, bool)>> = Mutex::new(None);
    if !animate::generated(&o.level)
        || o.param("stream") == Some("0")
        || o.materials.is_some()
        || o.scene.is_some()
        || o.param("world") == Some("off")
        || crate::preview::active()
    {
        return false;
    }
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((id, open)) = &*c
        && id == &o.level
    {
        return *open;
    }
    let open = mp_levels::level_by_id(&o.level).open;
    *c = Some((o.level.clone(), open));
    open
}

/// A section's root: its entities are its children.
#[derive(Component)]
pub struct StreamRoot(pub i64);

/// A streamed section's warm-up stand-in: gone once the pipelines it
/// queued have compiled.
#[derive(Component)]
struct StreamWarm;

/// A section's scene, up.
pub struct Up {
    root: Entity,
    wb: WorldBuild,
    index: SceneIndex,
    gathered: bool,
    /// Its materials' range in the block map.
    base: (u32, u32),
}

enum Sec {
    #[cfg(not(target_arch = "wasm32"))]
    Thread,
    /// Asked of the builder's worker.
    #[cfg(target_arch = "wasm32")]
    Worker,
    #[cfg(target_arch = "wasm32")]
    Stepping(Box<Mutex<Build>>),
    Spawning {
        build: Box<loader::Build>,
        wb: Box<WorldBuild>,
        root: Entity,
        base: (u32, u32),
    },
    Up(Box<Up>),
    Failed,
}

enum Phase {
    Off,
    #[cfg(not(target_arch = "wasm32"))]
    Planning,
    #[cfg(not(target_arch = "wasm32"))]
    BaseThread,
    #[cfg(target_arch = "wasm32")]
    BaseStepping(Box<Mutex<Build>>),
    Running,
}

/// The builder's worker, for this level.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkerState {
    /// Not asked (`?worker=0`, or it could not start).
    Off,
    Planning,
    Ready,
    /// It failed: the sections are built here.
    Failed,
}

/// The streamed level's state.
#[derive(Resource)]
pub struct Stream {
    phase: Phase,
    #[cfg(target_arch = "wasm32")]
    worker: WorkerState,
    planned: Option<Arc<Planned>>,
    secs: BTreeMap<i64, Sec>,
    /// The section the camera was last in, and the ring round it.
    at: Option<i64>,
    wanted: Vec<i64>,
    /// `ready` let go: the sections round the start were up once.
    started: bool,
    generation: u64,
    pub image_cache: ImageCache,
    /// The block map's ranges in use: (base, length).
    bases: Vec<(u32, u32)>,
    /// The base scene's materials, which take the map's first range.
    base_materials: u32,
    t0: Option<Instant>,
}

impl Default for Stream {
    fn default() -> Self {
        Stream {
            phase: Phase::Off,
            #[cfg(target_arch = "wasm32")]
            worker: WorkerState::Off,
            planned: None,
            secs: BTreeMap::new(),
            at: None,
            wanted: Vec::new(),
            started: false,
            generation: 0,
            image_cache: Arc::new(Mutex::new(Default::default())),
            bases: Vec::new(),
            base_materials: 0,
            t0: None,
        }
    }
}

impl Stream {
    /// The sections' worlds and scenes the animators run on.
    pub fn animated(&mut self) -> impl Iterator<Item = (&mut WorldBuild, &mut SceneIndex)> {
        self.secs.values_mut().filter_map(|s| match s {
            Sec::Up(u) if u.gathered => Some((&mut u.wb, &mut u.index)),
            _ => None,
        })
    }

    /// Whether the level streams and is running.
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Running)
    }

    /// The sections up, in order (the HUD and the tests).
    pub fn up(&self) -> Vec<i64> {
        self.secs
            .iter()
            .filter_map(|(k, s)| matches!(s, Sec::Up(_)).then_some(*k))
            .collect()
    }

    /// A range of the block map for `n` materials: the first gap.
    fn take_base(&mut self, n: u32) -> (u32, u32) {
        let mut used = self.bases.clone();
        used.push((0, self.base_materials));
        used.sort_unstable();
        let mut at = 0u32;
        for (b, l) in used {
            if b >= at + n {
                break;
            }
            at = at.max(b + l);
        }
        let r = (at, n);
        self.bases.push(r);
        r
    }

    fn free_base(&mut self, r: (u32, u32)) {
        self.bases.retain(|&b| b != r);
    }

    fn reset(&mut self) {
        let g = self.generation + 1;
        *self = Stream {
            generation: g,
            ..Stream::default()
        };
    }
}

/// The thread's results: the plan, the base, and the sections, each with
/// the generation it was asked in.
#[cfg(not(target_arch = "wasm32"))]
type PlanOut = Option<(u64, Result<Arc<Planned>, String>)>;
#[cfg(not(target_arch = "wasm32"))]
static THREAD_PLAN: Mutex<PlanOut> = Mutex::new(None);
#[cfg(not(target_arch = "wasm32"))]
type Built = Result<(WorldBuild, CutStats), String>;
#[cfg(not(target_arch = "wasm32"))]
static THREAD_OUT: Mutex<Vec<(u64, Option<i64>, Built)>> = Mutex::new(Vec::new());

fn setup() -> LevelSetup {
    LevelSetup {
        terrain: TerrainSetup {
            plan: None,
            ..TerrainSetup::default()
        },
        road: None,
    }
}

fn plan(level: mp_track::Level) -> Result<Arc<Planned>, String> {
    Planned::new(
        level,
        None,
        setup(),
        Arc::new(animate::open_world_scenery),
        Arc::new(animate::extra_modules),
    )
    .map(Arc::new)
}

/// Runs a section's build to its end and cuts it (the thread's work).
#[cfg(not(target_arch = "wasm32"))]
fn run(mut b: Build, t: Option<f64>) -> Result<(WorldBuild, CutStats), String> {
    while !b.is_done() {
        animate::section_step(&mut b, t)?;
    }
    Ok(section::finish(b))
}

/// The network's section under the camera's focus.
fn section_under(p: &Planned, focus: DVec3) -> Option<i64> {
    p.section_under(focus.x, focus.z)
}

/// The ring of sections wanted round `k` (`Planned::ring`: along its
/// road, and across a junction).
fn ring(p: &Planned, k: i64, behind: i64, ahead: i64) -> Vec<i64> {
    p.ring(k, behind, ahead)
}

/// Plans, builds the base, then keeps the ring of sections.
#[allow(clippy::too_many_arguments)]
pub fn drive(
    mut commands: Commands,
    mut st: ResMut<Stream>,
    mut wg: ResMut<WorldGen>,
    opts: Res<Opts>,
    state: Res<State<AppState>>,
    cs: Res<CameraState>,
    play: Option<Res<crate::play::Play>>,
    status: Res<Status>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ThreeMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let o = &opts.o;
    if !streamed(o) {
        return;
    }
    let st = &mut *st;
    let loading = !st.started;
    match &mut st.phase {
        Phase::Off => {
            if *state.get() != AppState::Waiting {
                return;
            }
            st.t0 = Some(Instant::now());
            animate::set_pending(true);
            let level = mp_levels::level_by_id(&o.level);
            #[cfg(not(target_arch = "wasm32"))]
            {
                let g = st.generation;
                std::thread::spawn(move || {
                    let r = plan(level);
                    *THREAD_PLAN.lock().unwrap_or_else(|e| e.into_inner()) = Some((g, r));
                });
                st.phase = Phase::Planning;
            }
            #[cfg(target_arch = "wasm32")]
            {
                // The worker plans the same level meanwhile.
                if o.param("worker") != Some("0") && crate::worker::available() {
                    crate::worker::plan(st.generation, &o.level);
                    st.worker = WorkerState::Planning;
                }
                match plan(level) {
                    Ok(p) => {
                        info!(
                            "stream: planned in {:.2} s",
                            st.t0.map_or(0.0, |t| t.elapsed().as_secs_f64())
                        );
                        let b = p.build(p.base_section());
                        st.planned = Some(p);
                        st.phase = Phase::BaseStepping(Box::new(Mutex::new(b)));
                    }
                    Err(e) => {
                        inbox().scene = Some(Err(format!("stream: {e}")));
                        st.phase = Phase::Running;
                    }
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Phase::Planning => {
            let mut out = THREAD_PLAN.lock().unwrap_or_else(|e| e.into_inner());
            let Some((g, r)) = out.take() else { return };
            if g != st.generation {
                return;
            }
            match r {
                Ok(p) => {
                    info!(
                        "stream: planned in {:.2} s",
                        st.t0.map_or(0.0, |t| t.elapsed().as_secs_f64())
                    );
                    let (g, t, p2) = (st.generation, o.t, p.clone());
                    std::thread::spawn(move || {
                        let r = run(p2.build(p2.base_section()), t);
                        THREAD_OUT
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((g, None, r));
                    });
                    st.planned = Some(p);
                    st.phase = Phase::BaseThread;
                }
                Err(e) => {
                    inbox().scene = Some(Err(format!("stream: {e}")));
                    st.phase = Phase::Running;
                }
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        Phase::BaseThread => {}
        #[cfg(target_arch = "wasm32")]
        Phase::BaseStepping(b) => {
            let b = b.get_mut().unwrap_or_else(|e| e.into_inner());
            let t0 = Instant::now();
            let mut r = Ok(());
            while !b.is_done() && r.is_ok() && t0.elapsed().as_millis() < BUILD_LOADING_MS {
                r = animate::section_step(b, o.t);
            }
            match r {
                Err(e) => {
                    inbox().scene = Some(Err(format!("stream: base: {e}")));
                    st.phase = Phase::Running;
                }
                Ok(()) if b.is_done() => {
                    if let Phase::BaseStepping(b) = std::mem::replace(&mut st.phase, Phase::Running)
                    {
                        let b = b.into_inner().unwrap_or_else(|e| e.into_inner());
                        let (wb, _) = section::finish(b);
                        base_up(st, &mut wg, wb);
                    }
                }
                Ok(()) => {}
            }
        }
        Phase::Running => {}
    }

    // What the thread finished: the base, or sections.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let done: Vec<(Option<i64>, Built)> = {
            let mut out = THREAD_OUT.lock().unwrap_or_else(|e| e.into_inner());
            let g = st.generation;
            let (mine, other): (Vec<_>, Vec<_>) = out.drain(..).partition(|(gg, _, _)| *gg == g);
            *out = other;
            mine.into_iter().map(|(_, k, r)| (k, r)).collect()
        };
        for (k, r) in done {
            match (k, r) {
                (None, Ok((wb, _))) => {
                    st.phase = Phase::Running;
                    base_up(st, &mut wg, wb);
                }
                (None, Err(e)) => {
                    inbox().scene = Some(Err(format!("stream: base: {e}")));
                    st.phase = Phase::Running;
                }
                (Some(k), r) => section_built(st, &mut commands, k, r),
            }
        }
    }
    if !matches!(st.phase, Phase::Running) {
        return;
    }
    let Some(p) = st.planned.clone() else { return };

    // The ring round the player's car once the race is on (the camera
    // cuts about before and at the start); round the start until then.
    let car = play.as_ref().and_then(|pl| {
        let r = pl.race.as_ref()?;
        let st = &r.session.curr;
        if st.race.state == mp_sim::race::RaceStateKind::Countdown {
            return None;
        }
        st.players.first().map(|pl| (pl.v.x, pl.v.z))
    });
    let under = match car {
        Some((x, z)) => p.section_under(x, z),
        None => Some(p.section_at(p.track.start_s)),
    };
    let _ = (&cs, section_under);
    if let Some(k) = under {
        if st.at != Some(k) {
            info!(
                "stream: the ring moves to section {k} (road {}, car {:?})",
                p.road_of(k).0,
                car.map(|(x, z)| [x.round(), z.round()])
            );
            st.at = Some(k);
            st.wanted = ring(&p, k, BEHIND, AHEAD);
        }
    } else if st.at.is_none() {
        st.at = Some(p.section_at(p.track.start_s));
        st.wanted = ring(&p, st.at.unwrap_or(0), BEHIND, AHEAD);
    }
    let keep = ring(&p, st.at.unwrap_or(0), BEHIND + SLACK, AHEAD + SLACK);

    // Dropped: the sections outside the ring with its slack.
    let gone: Vec<i64> = st
        .secs
        .keys()
        .copied()
        .filter(|k| !keep.contains(k))
        .collect();
    for k in gone {
        let Some(s) = st.secs.remove(&k) else {
            continue;
        };
        match s {
            Sec::Up(u) => {
                commands.entity(u.root).despawn();
                st.free_base(u.base);
                // Its animation blocks: handed back by `release` below.
                RELEASE
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(u.index);
            }
            Sec::Spawning { root, base, .. } => {
                commands.entity(root).despawn();
                st.free_base(base);
            }
            _ => {}
        }
    }

    // The worker's answers: the plan, the sections built, or its failure
    // (then the sections waiting on it are built here).
    #[cfg(target_arch = "wasm32")]
    {
        let mut failed = false;
        for m in crate::worker::take() {
            match m {
                crate::worker::Msg::Planned(g) if g == st.generation => {
                    st.worker = WorkerState::Ready;
                    info!("stream: the builder's worker planned the level");
                }
                crate::worker::Msg::Built {
                    generation,
                    k,
                    ms,
                    bytes,
                } if generation == st.generation => {
                    if !matches!(st.secs.get(&k), Some(Sec::Worker)) {
                        continue;
                    }
                    let t0 = Instant::now();
                    let r = section::unpack(&bytes);
                    info!(
                        "stream: section {k} from the worker ({ms:.0} ms there, {:.1} MB, read in {:.1} ms)",
                        bytes.len() as f64 / 1048576.0,
                        t0.elapsed().as_secs_f64() * 1e3
                    );
                    section_built(st, &mut commands, k, r);
                }
                crate::worker::Msg::Failed {
                    generation,
                    k,
                    error,
                } if generation == st.generation || generation == 0 => {
                    warn!(
                        "stream: the builder's worker failed{}: {error}; building here",
                        k.map(|k| format!(" on section {k}")).unwrap_or_default()
                    );
                    failed = true;
                }
                _ => {}
            }
        }
        if failed {
            st.worker = WorkerState::Failed;
            let waiting: Vec<i64> = st
                .secs
                .iter()
                .filter_map(|(k, s)| matches!(s, Sec::Worker).then_some(*k))
                .collect();
            for k in waiting {
                st.secs.insert(
                    k,
                    Sec::Stepping(Box::new(Mutex::new(p.build(p.section(k))))),
                );
            }
        }
    }

    // Started: one at a time, nearest first.
    let building = st.secs.values().any(|s| {
        #[cfg(not(target_arch = "wasm32"))]
        let b = matches!(s, Sec::Thread);
        #[cfg(target_arch = "wasm32")]
        let b = matches!(s, Sec::Stepping(_) | Sec::Worker);
        b
    });
    if !building {
        let wanted = st.wanted.clone();
        if let Some(&k) = wanted.iter().find(|k| !st.secs.contains_key(k)) {
            let sec = p.section(k);
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (g, t, p2) = (st.generation, o.t, p.clone());
                std::thread::spawn(move || {
                    let r = run(p2.build(sec), t);
                    THREAD_OUT
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push((g, Some(k), r));
                });
                st.secs.insert(k, Sec::Thread);
            }
            #[cfg(target_arch = "wasm32")]
            {
                match st.worker {
                    WorkerState::Ready => {
                        crate::worker::build(st.generation, k);
                        st.secs.insert(k, Sec::Worker);
                    }
                    // Planning still: wait for its answer rather than
                    // build here (the start's ring is behind the loading
                    // screen either way).
                    WorkerState::Planning => {}
                    WorkerState::Off | WorkerState::Failed => {
                        st.secs
                            .insert(k, Sec::Stepping(Box::new(Mutex::new(p.build(sec)))));
                    }
                }
            }
        }
    }

    // The web: the build in progress, stepped.
    #[cfg(target_arch = "wasm32")]
    {
        let budget = if loading {
            BUILD_LOADING_MS
        } else {
            BUILD_RUNNING_MS
        };
        let mut finished: Vec<(i64, Result<(WorldBuild, CutStats), String>)> = Vec::new();
        for (k, s) in st.secs.iter_mut() {
            let Sec::Stepping(b) = s else { continue };
            let b = b.get_mut().unwrap_or_else(|e| e.into_inner());
            let t0 = Instant::now();
            let mut r = Ok(());
            while !b.is_done() && r.is_ok() && t0.elapsed().as_millis() < budget {
                r = animate::section_step(b, o.t);
            }
            let done = b.is_done();
            match r {
                Err(e) => finished.push((*k, Err(e))),
                Ok(()) if done => {
                    if let Sec::Stepping(b) = std::mem::replace(s, Sec::Failed) {
                        let b = b.into_inner().unwrap_or_else(|e| e.into_inner());
                        finished.push((*k, Ok(section::finish(b))));
                    }
                }
                Ok(()) => {}
            }
        }
        for (k, r) in finished {
            section_built(st, &mut commands, k, r);
        }
    }

    // Spawning, stepped.
    let budget = if loading {
        SPAWN_LOADING_MS
    } else {
        SPAWN_RUNNING_MS
    };
    let mut up: Vec<i64> = Vec::new();
    for (k, s) in st.secs.iter_mut() {
        let Sec::Spawning { build, .. } = s else {
            continue;
        };
        let done = loader::step_section(
            build,
            &mut commands,
            &mut meshes,
            &mut materials,
            &mut sky_materials,
            &mut images,
            budget,
        );
        if done {
            up.push(*k);
        }
        break; // one a frame
    }
    for k in up {
        let Some(Sec::Spawning {
            build,
            wb,
            root,
            base,
        }) = st.secs.remove(&k)
        else {
            continue;
        };
        let scene = loader::finish_section(*build, &mut commands, &mut meshes, &mut images);
        for e in scene.warm_up {
            commands
                .entity(e)
                .remove::<crate::warmup::WarmUp>()
                .insert(StreamWarm);
        }
        info!(
            "stream: section {k} up ({} entities, {} materials from {})",
            scene.loaded.counts.entities, base.1, base.0
        );
        st.secs.insert(
            k,
            Sec::Up(Box::new(Up {
                root,
                wb: *wb,
                index: scene.index,
                gathered: false,
                base,
            })),
        );
    }

    // Ready: the ring round the start is up (or failed), once.
    if !st.started {
        let all = st
            .wanted
            .iter()
            .all(|k| matches!(st.secs.get(k), Some(Sec::Up(_) | Sec::Failed)));
        if all && !st.wanted.is_empty() {
            st.started = true;
            animate::set_pending(false);
            info!(
                "stream: the start's sections up {:.2} s after the level was asked",
                st.t0.map_or(0.0, |t| t.elapsed().as_secs_f64())
            );
        }
    }
    let _ = &status;
}

/// Indexes released with their sections, whose animation blocks go back
/// in `release` (which has the blocks).
static RELEASE: Mutex<Vec<SceneIndex>> = Mutex::new(Vec::new());

/// The base built: its scene is the level's drawn scene, its world the
/// animators' (the sky's).
fn base_up(st: &mut Stream, wg: &mut WorldGen, mut wb: WorldBuild) {
    let scene = std::mem::take(&mut wb.scene);
    st.base_materials = scene.materials.len() as u32;
    info!(
        "stream: base built {:.2} s after the level was asked: {} nodes, {} materials",
        st.t0.map_or(0.0, |t| t.elapsed().as_secs_f64()),
        scene.nodes.len(),
        scene.materials.len()
    );
    animate::set_ready(wg, wb, &scene);
    inbox().scene = Some(Ok(animate::drawable_scene(scene)));
}

/// A section built: its scene goes to the loader under a root of its own.
fn section_built(
    st: &mut Stream,
    commands: &mut Commands,
    k: i64,
    r: Result<(WorldBuild, CutStats), String>,
) {
    // Not wanted any more while it built.
    if !st.wanted.contains(&k) && !st.secs.contains_key(&k) {
        return;
    }
    match r {
        Err(e) => {
            warn!("stream: section {k}: {e}");
            st.secs.insert(k, Sec::Failed);
        }
        Ok((mut wb, stats)) => {
            let scene = std::mem::take(&mut wb.scene);
            let base = st.take_base(scene.materials.len() as u32);
            let root = commands
                .spawn((
                    Transform::default(),
                    Visibility::Inherited,
                    SceneEntity,
                    StreamRoot(k),
                    Name::new(format!("stream {k}")),
                ))
                .id();
            info!(
                "stream: section {k} built ({} nodes, kept {} cut {}, {} animators)",
                scene.nodes.len(),
                stats.kept,
                stats.cut,
                wb.animators.len()
            );
            let build = loader::Build::new(
                scene,
                SHARED
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                    .expect("shared images"),
            )
            .under(root)
            .with_material_base(base.0)
            .with_image_cache(st.image_cache.clone());
            st.secs.insert(
                k,
                Sec::Spawning {
                    build: Box::new(build),
                    wb: Box::new(wb),
                    root,
                    base,
                },
            );
        }
    }
}

/// The shared images, kept for `section_built` (which has no resources).
static SHARED: Mutex<Option<SharedImages>> = Mutex::new(None);

fn note_shared(shared: Res<SharedImages>) {
    *SHARED.lock().unwrap_or_else(|e| e.into_inner()) = Some(shared.clone());
}

/// Marks a newly spawned section's entities by node for its animators.
fn gather(mut st: ResMut<Stream>, kids: Query<(Entity, &NodeRef, &ChildOf)>) {
    for u in st.secs.values_mut().filter_map(|s| match s {
        Sec::Up(u) if !u.gathered => Some(u),
        _ => None,
    }) {
        let mut by_node = vec![Vec::new(); u.index.node_count()];
        for (e, n, c) in &kids {
            if c.parent() == u.root
                && let Some(v) = by_node.get_mut(n.0 as usize)
            {
                v.push(e);
            }
        }
        u.index.set_entities(by_node);
        u.gathered = true;
    }
}

/// Hands dropped sections' animation blocks back.
fn release(mut blocks: ResMut<animate::AnimBlocks>) {
    let mut list = RELEASE.lock().unwrap_or_else(|e| e.into_inner());
    for mut index in list.drain(..) {
        animate::release_blocks(&mut index, &mut blocks);
    }
}

/// A section's warm-up stand-ins go once nothing is compiling.
fn end_warm_up(
    mut commands: Commands,
    status: Res<Status>,
    q: Query<Entity, With<StreamWarm>>,
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

fn reset(mut st: ResMut<Stream>) {
    st.reset();
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Stream>()
        .add_systems(Startup, note_shared)
        .add_systems(OnEnter(AppState::Waiting), reset)
        .add_systems(
            Update,
            (
                drive.after(animate::drive_build),
                gather,
                release,
                end_warm_up,
            )
                .chain(),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The block map's ranges: first fit after the base scene's, and a
    /// freed range is taken again.
    #[test]
    fn material_bases_are_first_fit_and_reused() {
        let mut st = Stream {
            base_materials: 3,
            ..Stream::default()
        };
        let a = st.take_base(29);
        let b = st.take_base(15);
        let c = st.take_base(8);
        assert_eq!((a, b, c), ((3, 29), (32, 15), (47, 8)));
        st.free_base(b);
        assert_eq!(st.take_base(10), (32, 10));
        assert_eq!(st.take_base(10), (55, 10));
        assert_eq!(st.take_base(4), (42, 4));
    }

    /// The ring round a section wraps on the loop.
    #[test]
    fn the_ring_wraps_round_the_loop() {
        let p = plan(mp_levels::level_by_id("peninsula")).unwrap();
        let (_, n) = p.road_sections()[0];
        assert!(n >= 34);
        assert_eq!(ring(&p, 0, 1, 2)[..4], [n - 1, 0, 1, 2]);
        assert_eq!(ring(&p, n - 1, 1, 2)[..4], [n - 2, n - 1, 0, 1]);
        let k = p.section_at(p.track.start_s);
        assert_eq!(k, 1);
        assert_eq!(
            section_under(
                &p,
                DVec3::new(p.track.px[1200] as f64, 0.0, p.track.pz[1200] as f64)
            ),
            Some(1)
        );
        // Off every road: nowhere.
        assert_eq!(section_under(&p, DVec3::new(-20000.0, 0.0, -20000.0)), None);
    }
}
