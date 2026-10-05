//! Level 1 built by `mr_worldgen` in the client, and its animators run on
//! the drawn scene every frame (roadmap WP 3.9; DECISIONS D490 to D497).
//!
//! **The build.** For a level that world generation builds whole (Sierra:
//! terrain, road, sky and every scenery module ported, D472), the client
//! runs `level_jobs` itself: natively on a thread, on the web a few jobs a
//! frame behind the loading screen. What it keeps is the [`WorldBuild`]'s
//! animators and sky (`world.updaters`, `world.sky`); the scene goes to the
//! loader with `?world=gen` (no download), and is dropped otherwise, the
//! loader drawing the level's `.mrscene` as before. The two scenes number
//! their nodes, meshes, materials and textures alike (D472), so the edits
//! address either.
//!
//! **Each frame** (after the camera), as `World.update` does: the sky at
//! the player's s ([`WorldBuild::update_sky`]: its edits are the dome's
//! uniforms and the lights, which the client's `Lighting` already holds,
//! so only its night factor is used), the night parameters (in the shader,
//! D455), then every animator with the camera ([`WorldBuild::update`]),
//! and the edits applied by handle:
//!
//! - a node's transform, visibility, instance matrices, colours and count:
//!   entity transforms and visibility, and a new instance stream for an
//!   `InstancedMesh` whose instances moved;
//! - a geometry attribute (the flag's cloth): the Bevy mesh's positions,
//!   kept on the CPU for meshes small enough to be animated;
//! - a material's colour, emissive colour, `emissiveIntensity`, sprite
//!   rotation, texture offsets and a kind's uniforms: never the Bevy
//!   material (re-preparing one is a GPU stall on the web, D455), but the
//!   material's *animation block* in the globals texture, five texels the
//!   shader reads instead of its parameters. A material gets its block the
//!   first time an animator touches it, which edits the material once.

use crate::loader::{AppState, SceneEntity};
use crate::render::ThreeMaterial;
use crate::render::instancing::{self, InstanceStream, Instances};
use crate::render::lighting::{
    BLOCK_TEXELS, G_BLOCK_MAP, G_BLOCKS, Globals, MAX_BLOCKS, MAX_MAPPED,
};
use crate::status::Status;
use crate::{Opts, SkyRes, inbox};
use bevy::camera::Projection;
use bevy::math::{DMat4, DQuat, DVec3, Mat4, Vec3};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use mr_scene::{MaterialKind, Scene};
use mr_worldgen::world::{
    Build, CameraView, Change, SceneEdit, SceneRef, Scenery, SceneryInfo, UpdateCtx, World,
    WorldBuild, level_jobs,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Whether `mr_worldgen` builds this level whole, numbered as its export,
/// in the client (D472 holds Sierra to it, D591 Seaside Raceway, D630 the
/// Night City Cruise, D551 Desert Run, D613 Downtown Streets, D536 the
/// Coast Highway; the client's builds of them: `crate::levels`, D680,
/// D700, D720).
pub fn generated(level: &str) -> bool {
    matches!(
        level,
        "sierra" | "seaside" | "cruise" | "desert" | "streets" | "coast"
    )
}

/// Draw the level as the client builds it, without the `.mrscene`
/// download: the default for every level the client builds (D678; D492
/// made it `?world=gen`). `?world=export` draws the exported scene instead
/// (a parity tool now, D439), as do `?world=off` and an explicit `?scene=`.
pub fn draws_generated(o: &crate::options::Options) -> bool {
    generated(&o.level)
        && !matches!(o.param("world"), Some("export" | "off"))
        && o.scene.is_none()
        && o.materials.is_none()
}

/// Whether the client builds the level's world (for its animators, and,
/// unless `?world=export`, its scene).
fn wants_world(o: &crate::options::Options) -> bool {
    generated(&o.level)
        // The menu's sections bring their own worlds (`crate::preview`).
        && !crate::preview::active()
        && o.materials.is_none()
        && o.scene.is_none()
        && o.param("world") != Some("off")
}

/// A world build in progress or done blocks `ready` (`status::tick`), so the
/// loading screen and the stations wait for the animators.
static PENDING: AtomicBool = AtomicBool::new(false);

pub fn pending() -> bool {
    PENDING.load(Ordering::Relaxed)
}

/// The native build thread's result, with the generation it was asked for.
#[cfg(not(target_arch = "wasm32"))]
static THREAD_OUT: Mutex<Option<(u64, Result<WorldBuild, String>)>> = Mutex::new(None);

/// The scene counts a build numbers by, to check the drawn scene against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shape {
    nodes: usize,
    meshes: usize,
    materials: usize,
    textures: usize,
}

impl Shape {
    fn of(s: &Scene) -> Shape {
        Shape {
            nodes: s.nodes.len(),
            meshes: s.meshes.len(),
            materials: s.materials.len(),
            textures: s.textures.len(),
        }
    }
}

enum Gen {
    Idle,
    /// On a thread (native), generation `.0`.
    #[cfg(not(target_arch = "wasm32"))]
    Thread,
    /// Stepped a few jobs a frame (web).
    #[cfg(target_arch = "wasm32")]
    Stepping(Box<Mutex<Build>>),
    Ready {
        wb: Box<Mutex<WorldBuild>>,
        shape: Shape,
    },
    Off,
}

/// The client's world build (`mr_worldgen`) and its animators.
#[derive(Resource)]
pub struct WorldGen {
    state: Gen,
    generation: u64,
    started: Option<bevy::platform::time::Instant>,
}

impl Default for WorldGen {
    fn default() -> Self {
        WorldGen {
            state: Gen::Idle,
            generation: 0,
            started: None,
        }
    }
}

/// The level's world jobs, as `tests/level1.rs` builds Sierra (or as
/// `crate::levels` builds the level, when it has inputs of its own).
fn new_build(level: &str, draws: bool) -> Build {
    if let Some(b) = crate::levels::new_build(level, draws) {
        return b;
    }
    use mr_worldgen::stages::{LevelSetup, level_stages};
    use mr_worldgen::terrain_mesh::TerrainSetup;
    let setup = LevelSetup {
        terrain: TerrainSetup {
            plan: None,
            ..TerrainSetup::default()
        },
        road: None,
    };
    Build::new(
        World::new(mr_levels::level_by_id(level)),
        match level {
            "desert" => level_jobs(level_stages(setup), crate::levels::desert::scenery),
            "streets" => level_jobs(level_stages(setup), crate::levels::streets::scenery),
            "coast" => level_jobs(level_stages(setup), crate::levels::coast::scenery),
            _ => level_jobs(level_stages(setup), level1_scenery),
        },
    )
}

/// A menu section of a level that world generation builds (D741): the
/// level's own build, restricted to `section` (`mr_worldgen::section`).
pub fn section_build(level: &str, section: mr_worldgen::section::Section) -> Build {
    let mut b = new_build(level, true);
    b.world.section = Some(section);
    b
}

/// One job of a section's build, as [`drive_build`] steps a level's.
pub fn section_step(b: &mut Build, t: Option<f64>) -> Result<(), String> {
    step(b, t)
}

/// Puts a menu section's world in place of the level's: its animators
/// run on the drawn scene from the next frame (`crate::preview`). Returns
/// what was there (a level's world, kept by the caller or dropped).
pub fn install_world(wg: &mut WorldGen, wb: WorldBuild, index: &SceneIndex) -> Option<WorldBuild> {
    let old = take_world(wg);
    wg.state = Gen::Ready {
        wb: Box::new(Mutex::new(wb)),
        shape: index.shape,
    };
    PENDING.store(false, Ordering::Relaxed);
    old
}

/// Takes the world whose animators run, leaving none (`Off`: nothing is
/// built in its place).
pub fn take_world(wg: &mut WorldGen) -> Option<WorldBuild> {
    match std::mem::replace(&mut wg.state, Gen::Off) {
        Gen::Ready { wb, .. } => Some(wb.into_inner().unwrap_or_else(|e| e.into_inner())),
        other => {
            wg.state = other;
            None
        }
    }
}

/// Level 1's scenery modules, named one by one rather than through
/// `scenery::PORTED`: the table would link every level's scenery into the
/// client, over the wasm budget (SPEC 6.6; D498). Every other module is
/// left out, as `scenery_factory` leaves out an unported one.
fn level1_scenery(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    match info.name {
        "Mountain" => Some(Box::new(mr_worldgen::mountain::Mountain::new(info))),
        "Valley" => Some(Box::new(mr_worldgen::valley::Valley::new(info))),
        "City" => Some(Box::new(mr_worldgen::city::City::new(info))),
        _ => None,
    }
}

/// One job, then the `?t=` override on the sky as soon as it exists (the
/// valley's water reads the sky's time of day as `Sky.js` would).
fn step(b: &mut Build, t: Option<f64>) -> Result<(), String> {
    b.step()?;
    if let Some(sky) = b.world.sky.as_mut()
        && sky.override_p.is_none()
    {
        sky.override_p = t;
    }
    Ok(())
}

/// A scene the client can draw: the build's, with an environment (the
/// export's camera place is the JS menu's; the cameras take over at once).
fn drawable(mut scene: Scene) -> Scene {
    if scene.environment.is_none() {
        scene.environment = Some(mr_scene::Environment {
            fog: None,
            tone_mapping: 4,
            tone_mapping_exposure: 1.0,
            environment_intensity: crate::ENV_INTENSITY,
            night: 0.0,
            camera: mr_scene::CameraDesc {
                position: [0.0, 125.0, 0.0],
                quaternion: [0.0, 0.0, 0.0, 1.0],
                fov: 62.0,
                near: 0.3,
                far: 9000.0,
                aspect: 1.6,
            },
        });
    }
    scene
}

/// Starts, steps and finishes the build; hands the scene to the loader
/// with `?world=gen`.
pub fn drive_build(mut wg: ResMut<WorldGen>, opts: Res<Opts>, state: Res<State<AppState>>) {
    #[cfg(target_arch = "wasm32")]
    const BUDGET_MS: u128 = 30;
    let o = &opts.o;
    if let Gen::Idle = wg.state {
        if !wants_world(o) || *state.get() != AppState::Waiting {
            wg.state = Gen::Off;
            PENDING.store(false, Ordering::Relaxed);
            return;
        }
        if !crate::levels::inputs_ready(&o.level, draws_generated(o)) {
            return; // Seaside's survey and photo
        }
        wg.generation += 1;
        wg.started = Some(bevy::platform::time::Instant::now());
        PENDING.store(true, Ordering::Relaxed);
        let level = o.level.clone();
        let draws = draws_generated(o);
        let t = o.t;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let generation = wg.generation;
            std::thread::spawn(move || {
                let mut b = new_build(&level, draws);
                let mut r = Ok(());
                while !b.is_done() && r.is_ok() {
                    r = step(&mut b, t);
                }
                let out = r.map(|()| b.finish());
                *THREAD_OUT.lock().unwrap_or_else(|e| e.into_inner()) = Some((generation, out));
            });
            wg.state = Gen::Thread;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = t;
            wg.state = Gen::Stepping(Box::new(Mutex::new(new_build(&level, draws))));
        }
        return;
    }
    let done: Option<Result<WorldBuild, String>> = match &mut wg.state {
        #[cfg(not(target_arch = "wasm32"))]
        Gen::Thread => {
            let mut out = THREAD_OUT.lock().unwrap_or_else(|e| e.into_inner());
            match out.take() {
                Some((g, r)) if g == wg.generation => Some(r),
                Some(_) | None => None,
            }
        }
        #[cfg(target_arch = "wasm32")]
        Gen::Stepping(b) => {
            let b = b.get_mut().unwrap_or_else(|e| e.into_inner());
            let t0 = bevy::platform::time::Instant::now();
            let mut r = Ok(());
            while !b.is_done() && r.is_ok() && t0.elapsed().as_millis() < BUDGET_MS {
                r = step(b, o.t);
            }
            match r {
                Err(e) => Some(Err(e)),
                Ok(()) if b.is_done() => match std::mem::replace(&mut wg.state, Gen::Idle) {
                    Gen::Stepping(b) => Some(Ok(b
                        .into_inner()
                        .unwrap_or_else(|e| e.into_inner())
                        .finish())),
                    _ => None,
                },
                Ok(()) => None,
            }
        }
        _ => None,
    };
    let Some(done) = done else { return };
    match done {
        Err(e) => {
            if draws_generated(o) {
                inbox().scene = Some(Err(format!("world build: {e}")));
            }
            warn!("world build: {e}; the scenery's animators do not run");
            wg.state = Gen::Off;
            PENDING.store(false, Ordering::Relaxed);
        }
        Ok(mut wb) => {
            let scene = std::mem::take(&mut wb.scene);
            let shape = Shape::of(&scene);
            info!(
                "world built in {:.2} s: {} animators, {} nodes",
                wg.started.map_or(0.0, |t| t.elapsed().as_secs_f64()),
                wb.animators.len(),
                shape.nodes
            );
            if draws_generated(o) {
                inbox().scene = Some(Ok(drawable(scene)));
            }
            wg.state = Gen::Ready {
                wb: Box::new(Mutex::new(wb)),
                shape,
            };
            PENDING.store(false, Ordering::Relaxed);
        }
    }
}

/// A new scene is on its way (start, or a reload): the old world goes, and
/// the next frame builds the level's again (the JS builds a new world).
pub fn reset_world(
    mut commands: Commands,
    mut wg: ResMut<WorldGen>,
    mut blocks: ResMut<AnimBlocks>,
    mesh_writes: Res<MeshWrites>,
    opts: Res<Opts>,
) {
    let generation = wg.generation;
    *wg = WorldGen {
        generation,
        ..WorldGen::default()
    };
    blocks.texels.clear();
    blocks.map.clear();
    if let Ok(mut w) = mesh_writes.0.lock() {
        w.clear();
    }
    commands.remove_resource::<SceneIndex>();
    // Pending from now, so the page holds the downloaded scene back until
    // the build is done (`web::world_pending`).
    PENDING.store(wants_world(&opts.o), Ordering::Relaxed);
}

// ── The drawn scene, indexed for the edits ──────────────────────────────

/// The scene node an entity was spawned from (`loader::spawn_node`).
#[derive(Component, Clone, Copy, Debug)]
pub struct NodeRef(pub u32);

/// A material's texture roles, for texture offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TexRole {
    /// `map` or `alphaMap`: the block's texel 3, xy.
    Map,
    /// `normalMap`: texel 3, zw.
    Normal,
}

struct MatInfo {
    kind: MaterialKind,
    /// The emissive colour and intensity as exported, and whether the
    /// intensity follows nightfall (D455).
    emissive: [f64; 3],
    emissive_intensity: f64,
    night: bool,
    /// The kind's uniforms as exported, in their block slots.
    uniforms: [f32; 4],
    block: Option<usize>,
}

struct InstInfo {
    matrices: Vec<[f32; 16]>,
    colors: Option<Vec<[f32; 3]>>,
    count: u32,
    receive: bool,
    /// The instance-rate attributes, if the geometry has any (`loader`).
    extras: Option<Vec<[f32; 4]>>,
    dirty: bool,
}

struct NodeInfo {
    parent: Option<u32>,
    children: Vec<u32>,
    /// three's `matrix` (local), and the world matrix as now drawn.
    local: DMat4,
    world: DMat4,
    /// The models' grid offset (zero for a level).
    offset: Vec3,
    visible: bool,
    instances: Option<InstInfo>,
}

/// What the edits need of the scene the loader drew, taken before it drops
/// the scene's CPU copy (`loader::build_step`).
#[derive(Resource)]
pub struct SceneIndex {
    shape: Shape,
    nodes: Vec<NodeInfo>,
    materials: Vec<MatInfo>,
    /// Per texture, the materials using it and how.
    textures: Vec<Vec<(u32, TexRole, [f64; 2])>>,
    /// Per scene mesh, the Bevy meshes drawn from it that keep a CPU copy
    /// (small triangle meshes, [`convert::KEEP_VERTICES`]; small `Points`,
    /// [`convert::KEEP_POINTS`]), and how its vertices map to the
    /// geometry's.
    meshes: Vec<Vec<KeptMesh>>,
    /// Entities per node, gathered from [`NodeRef`] at the first frame.
    entities: Option<Vec<Vec<Entity>>>,
    /// Edits of kinds nothing here applies, reported once each.
    unknown: Vec<String>,
    /// The spot light the loader takes into `Lighting` (the first in the
    /// scene) and its target, the next sibling (Desert's train, D720).
    spot: Option<(u32, Option<u32>)>,
    /// The scene's top-level groups, for the level viewer's toggles
    /// (`crate::viewer::groups`).
    pub groups: crate::viewer::groups::Groups,
}

/// The uniform slot (block texel 4) of a kind's animated uniform.
fn uniform_slot(kind: MaterialKind, prop: &str) -> Option<usize> {
    use MaterialKind::*;
    Some(match (kind, prop) {
        (TrafficStreams, "uTime") => 0,
        (TrafficStreams, "uNight") => 1,
        (TrafficStreams, "uFogK") => 2,
        (TrafficStreams, "uHalfH") => 3,
        (SkyGlow, "uK") => 0,
        (Asphalt, "uWet") => 0,
        (GlowPoints, "uFogK") => 0,
        (Surf, "uTime") => 0,
        (Surf, "uBright") => 1,
        (LighthouseBeam, "uStrength") => 0,
        (GroundPool, "uTime") => 0,
        (FlickerPoints, "uTime") => 0,
        (GroundPool, "opacity") => 1,
        (FloodBeam, "opacity") => 0,
        (Neon, "uNTime") => 0,
        (Steam, "uTime") => 0,
        (Steam, "uScale") => 1,
        // three's own materials: the opacity an updater moves (Level 2's
        // lamp pools, the lighthouse's glow, the beach surf, the boats'
        // lights; D701), read by the shader under `PLAIN_ANIM`.
        (Standard | Physical | Lambert | Basic | Line | Sprite | Points, "opacity") => 0,
        // A plain Points' size (Streets' phone flashes; D721).
        (Points, "size") => 1,
        _ => return None,
    })
}

/// Fixed values for a kind's animated uniforms (the material test scenes'
/// `overrides.uniforms`): into `kind0` at their slots, which the shader
/// then reads (`slots.y`) instead of following the scene-wide state.
pub fn fix_uniforms(mat: &mut ThreeMaterial, kind: MaterialKind, overrides: &serde_json::Value) {
    let Some(o) = overrides.as_object() else {
        return;
    };
    let mut any = false;
    for (name, v) in o {
        if let (Some(slot), Some(x)) = (uniform_slot(kind, name), v.as_f64()) {
            mat.params.kind0[slot] = x as f32;
            any = true;
        }
    }
    if any {
        mat.params.slots.y = 1.0;
    }
}

/// Every animated uniform of [`uniform_slot`].
const UNIFORMS: [&str; 12] = [
    "uTime",
    "uNight",
    "uFogK",
    "uHalfH",
    "uK",
    "uWet",
    "uBright",
    "uStrength",
    "opacity",
    "uNTime",
    "uScale",
    "size",
];

impl SceneIndex {
    pub fn new(
        scene: &Scene,
        meshes: &HashMap<crate::convert::MeshKey, Option<Handle<Mesh>>>,
        offset: &[Vec3],
    ) -> SceneIndex {
        let nodes = scene
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| NodeInfo {
                parent: n.parent,
                children: n.children.clone(),
                local: DMat4::from_cols_array(&n.matrix),
                world: DMat4::from_cols_array(&n.matrix_world),
                offset: offset.get(i).copied().unwrap_or(Vec3::ZERO),
                visible: n.visible,
                instances: n.instances.and_then(|k| {
                    let d = scene.instances.get(k as usize)?;
                    let m = scene.buffers[d.matrices as usize].data.as_f32()?;
                    let matrices = m
                        .as_chunks::<16>()
                        .0
                        .iter()
                        .take(d.capacity.max(d.count) as usize)
                        .copied()
                        .collect();
                    let colors = d.colors.map(|c| {
                        let b = &scene.buffers[c as usize].data;
                        (0..b.len() / 3)
                            .map(|j| {
                                [
                                    b.get(j * 3) as f32,
                                    b.get(j * 3 + 1) as f32,
                                    b.get(j * 3 + 2) as f32,
                                ]
                            })
                            .collect()
                    });
                    Some(InstInfo {
                        matrices,
                        colors,
                        count: d.count,
                        receive: n.receive_shadow,
                        extras: crate::loader::instance_extras(scene, d),
                        dirty: false,
                    })
                }),
            })
            .collect();
        let mut textures: Vec<Vec<(u32, TexRole, [f64; 2])>> =
            vec![Vec::new(); scene.textures.len()];
        let materials = scene
            .materials
            .iter()
            .enumerate()
            .map(|(i, m)| {
                for (name, role) in [
                    ("map", TexRole::Map),
                    ("alphaMap", TexRole::Map),
                    ("normalMap", TexRole::Normal),
                ] {
                    if let Some(t) = m.texture(name)
                        && let Some(users) = textures.get_mut(t as usize)
                        && !users.iter().any(|u| u.0 == i as u32 && u.1 == role)
                    {
                        users.push((i as u32, role, scene.textures[t as usize].offset));
                    }
                }
                let mut uniforms = [0f32; 4];
                for name in UNIFORMS {
                    if let Some(slot) = uniform_slot(m.kind, name) {
                        let unset = if matches!(name, "opacity" | "size") {
                            1.0
                        } else {
                            0.0
                        };
                        uniforms[slot] = m.number(name).unwrap_or(unset) as f32;
                    }
                }
                MatInfo {
                    kind: m.kind,
                    emissive: m.color("emissive").unwrap_or([0.0; 3]),
                    emissive_intensity: m.number("emissiveIntensity").unwrap_or(1.0),
                    night: scene
                        .night_params
                        .iter()
                        .any(|p| p.material == i as u32 && p.prop == "emissiveIntensity"),
                    uniforms,
                    block: None,
                }
            })
            .collect();
        let mut by_mesh: Vec<Vec<KeptMesh>> = (0..scene.meshes.len()).map(|_| Vec::new()).collect();
        for (key, h) in meshes {
            if let Some(h) = h
                && crate::convert::keeps_cpu_copy(scene, *key)
                && let Some(v) = by_mesh.get_mut(key.mesh as usize)
            {
                let m = &scene.meshes[key.mesh as usize];
                let size = |name: &str| {
                    m.attribute(name)
                        .map_or(3, |a| scene.buffers[a.accessor as usize].item_size as usize)
                };
                let map = VertexMap {
                    points: (key.draw == crate::convert::Draw::Points)
                        .then_some((key.start as usize, key.count as usize)),
                    color_size: size("color"),
                };
                v.push(KeptMesh {
                    handle: h.clone(),
                    map,
                    cpu: None,
                });
            }
        }
        let spot = scene
            .lights
            .iter()
            .find(|l| l.ty == mr_scene::NodeType::SpotLight)
            .map(|l| {
                let sib = scene.nodes[l.node as usize]
                    .parent
                    .map_or(&[][..], |p| scene.nodes[p as usize].children.as_slice());
                let next = sib
                    .iter()
                    .position(|&c| c == l.node)
                    .and_then(|k| sib.get(k + 1).copied());
                (l.node, next)
            });
        SceneIndex {
            shape: Shape::of(scene),
            nodes,
            materials,
            textures,
            meshes: by_mesh,
            entities: None,
            unknown: Vec::new(),
            spot,
            groups: crate::viewer::groups::scene_groups(scene),
        }
    }

    /// The entities drawn per node, for a scene that is not the only one
    /// drawn (a menu section's, under its root).
    pub fn set_entities(&mut self, by_node: Vec<Vec<Entity>>) {
        self.entities = Some(by_node);
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Forgets the materials' animation blocks (the blocks were cleared
    /// for another scene); they are made again on first use.
    pub fn reset_blocks(&mut self) {
        for m in &mut self.materials {
            m.block = None;
        }
    }

    fn report(&mut self, what: String) {
        if !self.unknown.contains(&what) {
            debug!("animators: {what} is not applied");
            self.unknown.push(what);
        }
    }

    /// The node and every node under it, depth first.
    fn subtree(&self, n: u32) -> Vec<u32> {
        let mut out = vec![n];
        let mut at = 0;
        while at < out.len() {
            if let Some(info) = self.nodes.get(out[at] as usize) {
                out.extend(info.children.iter().copied());
            }
            at += 1;
        }
        out
    }
}

// ── Animation blocks in the globals ─────────────────────────────────────

/// The materials' animation blocks (D490): [`BLOCK_TEXELS`] texels each
/// from texel [`G_BLOCKS`] of the globals, and the block map from
/// [`G_BLOCK_MAP`] (per scene material, its block's first texel), written
/// there every frame.
#[derive(Resource, Default)]
pub struct AnimBlocks {
    pub texels: Vec<[f32; 4]>,
    pub map: Vec<f32>,
}

/// Copies the blocks and their map into the globals row after
/// `lighting::pack_globals`.
pub fn pack_blocks(blocks: Res<AnimBlocks>, mut globals: ResMut<Globals>) {
    for (k, t) in blocks.texels.iter().enumerate() {
        if let Some(g) = globals.0.get_mut(G_BLOCKS + k) {
            *g = *t;
        }
    }
    for (i, b) in blocks.map.iter().enumerate() {
        if let Some(g) = globals.0.get_mut(G_BLOCK_MAP + i / 4) {
            g[i % 4] = *b;
        }
    }
}

/// The block of material `i`, made on first use: the colour and the
/// sprite's rotation unset (the material's own), the emissive colour as
/// exported and its intensity (unless it follows nightfall, which the
/// material's `night` does), the kind's uniforms as exported. The material
/// finds it through the block map by its index (`ThreeParams::slots.z`, set
/// by the loader), so the Bevy material is never edited.
fn block_of(index: &mut SceneIndex, i: u32, blocks: &mut AnimBlocks) -> Option<usize> {
    let m = index.materials.get_mut(i as usize)?;
    if let Some(b) = m.block {
        return Some(b);
    }
    let k = blocks.texels.len() / BLOCK_TEXELS;
    if k >= MAX_BLOCKS || i as usize >= MAX_MAPPED {
        return None;
    }
    let e = m.emissive;
    let mut t = [[0f32; 4]; BLOCK_TEXELS];
    t[1] = [e[0] as f32, e[1] as f32, e[2] as f32, 1.0];
    if !m.night {
        t[2] = [m.emissive_intensity as f32, 1.0, 0.0, 0.0];
    }
    t[4] = m.uniforms;
    blocks.texels.extend_from_slice(&t);
    m.block = Some(k);
    if blocks.map.len() <= i as usize {
        blocks.map.resize(i as usize + 1, 0.0);
    }
    blocks.map[i as usize] = (G_BLOCKS + k * BLOCK_TEXELS) as f32;
    Some(k)
}

fn set_texel(blocks: &mut AnimBlocks, k: usize, texel: usize, f: impl FnOnce(&mut [f32; 4])) {
    if let Some(t) = blocks.texels.get_mut(k * BLOCK_TEXELS + texel) {
        f(t);
    }
}

// ── Each frame ──────────────────────────────────────────────────────────

/// What `update_sky` saw this frame: the world's dt (summed), the
/// player's s and the focus (`World.update(dt, s, focus)`).
#[derive(Clone, Copy, Debug, Default)]
pub struct WorldFrame {
    pub dt: f64,
    pub s: f64,
    pub focus: DVec3,
}

/// The drawn entities an edit moves, shows or hides.
type Placed<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static mut Visibility),
    (With<NodeRef>, Without<Camera3d>),
>;

/// One frame of `World.update` on the drawn scene.
#[allow(clippy::too_many_arguments)]
/// The countdown's lamps (Seaside's start lights) the last frame of a
/// countdown set, and how many are lit: the test bridge's stand-in for the
/// JS's `scenery[0].lampMats` (`__mr.lamps`).
pub static LAMPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
pub static LAMPS_LIT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn count_lamps(edits: &[SceneEdit]) {
    let mut n = 0;
    let mut lit = 0;
    for e in edits {
        if let Change::Number {
            prop: "emissiveIntensity",
            value,
        } = &e.change
        {
            n += 1;
            if *value > 0.0 {
                lit += 1;
            }
        }
    }
    if n > 0 {
        LAMPS.store(n, Ordering::Relaxed);
        LAMPS_LIT.store(lit, Ordering::Relaxed);
    }
}

pub fn run_animators(
    mut commands: Commands,
    wg: ResMut<WorldGen>,
    index: Option<ResMut<SceneIndex>>,
    mut sky_res: ResMut<SkyRes>,
    mut blocks: ResMut<AnimBlocks>,
    meshes: Res<Assets<Mesh>>,
    mesh_writes: Res<MeshWrites>,
    camera: Query<(&Transform, &Projection), With<Camera3d>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    nodes: Query<(Entity, &NodeRef), With<SceneEntity>>,
    mut placed: Placed,
    streams: Query<&Instances>,
    status: Res<Status>,
    race: Res<crate::levels::RaceCountdown>,
    mut lighting: ResMut<crate::render::lighting::Lighting>,
) {
    let Some(frame) = sky_res.frame.take() else {
        return;
    };
    let Gen::Ready { wb, shape } = &wg.state else {
        return;
    };
    let Some(mut index) = index else { return };
    if index.shape != *shape {
        if index.entities.is_none() {
            warn!(
                "the drawn scene is not the built world ({:?} against {:?}); the animators do not run",
                index.shape, shape
            );
            index.entities = Some(Vec::new());
        }
        return;
    }
    if status.state == "waiting" || status.state == "building" {
        return;
    }
    let index = &mut *index;
    if index.entities.is_none() {
        let mut by_node = vec![Vec::new(); index.nodes.len()];
        for (e, n) in &nodes {
            if let Some(v) = by_node.get_mut(n.0 as usize) {
                v.push(e);
            }
        }
        index.entities = Some(by_node);
    }
    let mut wb = wb.lock().unwrap_or_else(|e| e.into_inner());
    let Some((sky_frame, _sky_edits)) =
        wb.update_sky(frame.dt, frame.s, Some(frame.focus.to_array()))
    else {
        return;
    };
    let view = camera.single().ok().map(|(t, p)| CameraView {
        position: t.translation.as_dvec3().to_array(),
        fov: match p {
            Projection::Perspective(p) => f64::from(p.fov.to_degrees()),
            _ => 62.0,
        },
        viewport_height: windows
            .single()
            .map_or(800.0, |w| f64::from(w.physical_height())),
    });
    let mut edits = wb.update(&UpdateCtx {
        dt: frame.dt,
        night: sky_frame.night,
        camera: view,
        s: frame.s,
    });
    // `Race.update`'s `world.onCountdown` (Seaside's start lights, D682).
    if let Some(cd) = race.0 {
        let lamps = wb.countdown(cd);
        count_lamps(&lamps);
        edits.extend(lamps);
    }
    drop(wb);
    let blocks_before = blocks.texels.len();
    let mut moved: Vec<u32> = Vec::new();
    let mut shown: Vec<u32> = Vec::new();
    let mut light: Option<crate::levels::desert::LightEdit> = None;
    for SceneEdit { target, change } in edits {
        match (target, change) {
            (
                SceneRef::Node(n),
                Change::Transform {
                    position,
                    quaternion,
                    scale,
                },
            ) => {
                if let Some(info) = index.nodes.get_mut(n as usize) {
                    let local = DMat4::from_scale_rotation_translation(
                        DVec3::from_array(scale),
                        DQuat::from_array(quaternion),
                        DVec3::from_array(position),
                    );
                    if info.local != local {
                        info.local = local;
                        moved.push(n);
                    }
                }
            }
            (SceneRef::Node(n), Change::Visible(v)) => {
                if let Some(info) = index.nodes.get_mut(n as usize)
                    && info.visible != v
                {
                    info.visible = v;
                    shown.push(n);
                }
            }
            (SceneRef::Node(n), Change::InstanceMatrix { index: k, matrix }) => {
                if let Some(inst) = index
                    .nodes
                    .get_mut(n as usize)
                    .and_then(|i| i.instances.as_mut())
                    && let Some(m) = inst.matrices.get_mut(k as usize)
                    && *m != matrix
                {
                    *m = matrix;
                    inst.dirty = true;
                }
            }
            (SceneRef::Node(n), Change::InstanceColor { index: k, rgb }) => {
                if let Some(inst) = index
                    .nodes
                    .get_mut(n as usize)
                    .and_then(|i| i.instances.as_mut())
                {
                    let len = inst.matrices.len();
                    let colors = inst.colors.get_or_insert_with(|| vec![[1.0; 3]; len]);
                    if let Some(c) = colors.get_mut(k as usize)
                        && *c != rgb
                    {
                        *c = rgb;
                        inst.dirty = true;
                    }
                }
            }
            (SceneRef::Node(n), Change::InstanceCount(c)) => {
                if let Some(inst) = index
                    .nodes
                    .get_mut(n as usize)
                    .and_then(|i| i.instances.as_mut())
                    && inst.count != c
                {
                    inst.count = c;
                    inst.dirty = true;
                }
            }
            (
                SceneRef::Node(n),
                Change::Light {
                    color, intensity, ..
                },
            ) => {
                // The sun and hemisphere light are the sky's (`Lighting`);
                // the spot light follows its animator (Desert's train).
                if index.spot.is_some_and(|(l, _)| l == n) {
                    light = Some((color, intensity));
                }
            }
            (
                SceneRef::Mesh(m),
                Change::Attribute {
                    name,
                    offset,
                    values,
                },
            ) => {
                if index.meshes.get(m as usize).is_none_or(|k| k.is_empty()) {
                    index.report(format!("attribute {name} of mesh {m}"));
                    continue;
                }
                let attr = match name {
                    "position" => Mesh::ATTRIBUTE_POSITION,
                    "normal" => Mesh::ATTRIBUTE_NORMAL,
                    "color" => Mesh::ATTRIBUTE_COLOR,
                    _ => {
                        index.report(format!("attribute {name}"));
                        continue;
                    }
                };
                for kept in &mut index.meshes[m as usize] {
                    let size = if name == "color" {
                        kept.map.color_size
                    } else {
                        3
                    };
                    write_attribute(&meshes, kept, attr, size, offset, &values, &mesh_writes);
                }
            }
            (SceneRef::Material(i), change) => {
                apply_material(index, i, change, &mut blocks);
            }
            (SceneRef::Texture(t), Change::TextureOffset(off)) => {
                let users = index.textures.get(t as usize).cloned().unwrap_or_default();
                for (i, role, base) in users {
                    let Some(k) = block_of(index, i, &mut blocks) else {
                        continue;
                    };
                    let d = [(off[0] - base[0]) as f32, (off[1] - base[1]) as f32];
                    set_texel(&mut blocks, k, 3, |t| match role {
                        TexRole::Map => {
                            t[0] = d[0];
                            t[1] = d[1];
                        }
                        TexRole::Normal => {
                            t[2] = d[0];
                            t[3] = d[1];
                        }
                    });
                }
            }
            (target, change) => index.report(format!("{target:?} {}", change_name(&change))),
        }
    }
    if blocks.texels.len() != blocks_before {
        info!(
            "animators: {} materials with an animation block",
            blocks.texels.len() / BLOCK_TEXELS
        );
    }
    // Moved nodes: their subtrees' world matrices, as three's
    // updateMatrixWorld, then the entities and instance streams.
    let mut dirty_world: Vec<u32> = Vec::new();
    for n in moved {
        for k in index.subtree(n) {
            let parent = index.nodes[k as usize]
                .parent
                .map_or(DMat4::IDENTITY, |p| index.nodes[p as usize].world);
            let info = &mut index.nodes[k as usize];
            info.world = parent * info.local;
            if !dirty_world.contains(&k) {
                dirty_world.push(k);
            }
        }
    }
    if let Some((l, t)) = index.spot {
        let at = |n: u32| {
            dirty_world.contains(&n).then(|| {
                let info = &index.nodes[n as usize];
                info.world.w_axis.truncate() + info.offset.as_dvec3()
            })
        };
        let (p, t) = (at(l), t.and_then(at));
        if light.is_some() || p.is_some() || t.is_some() {
            crate::levels::desert::follow_spot(&mut lighting, light, p, t);
        }
    }
    let entities = index.entities.take().unwrap_or_default();
    for &k in &dirty_world {
        let info = &mut index.nodes[k as usize];
        if let Some(inst) = info.instances.as_mut() {
            inst.dirty = true;
            continue;
        }
        let world = Mat4::from_translation(info.offset) * info.world.as_mat4();
        let t = Transform::from_matrix(world);
        for &e in entities.get(k as usize).map_or(&[][..], |v| v.as_slice()) {
            if let Ok((mut tr, _)) = placed.get_mut(e)
                && *tr != t
            {
                *tr = t;
            }
        }
    }
    // Visibility: a node shows when it and every ancestor do.
    for n in shown {
        for k in index.subtree(n) {
            let mut v = true;
            let mut at = Some(k);
            while let Some(a) = at {
                let info = &index.nodes[a as usize];
                v &= info.visible;
                at = info.parent;
            }
            let vis = if v {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            for &e in entities.get(k as usize).map_or(&[][..], |v| v.as_slice()) {
                if let Ok((_, mut cur)) = placed.get_mut(e)
                    && *cur != vis
                {
                    *cur = vis;
                }
            }
        }
    }
    // Instance streams: a new one for each InstancedMesh whose instances
    // changed (its entities, one per material group, share it).
    for (k, info) in index.nodes.iter_mut().enumerate() {
        let world = DMat4::from_translation(info.offset.as_dvec3()) * info.world;
        let Some(inst) = info.instances.as_mut() else {
            continue;
        };
        if !inst.dirty {
            continue;
        }
        inst.dirty = false;
        let mut data = Vec::with_capacity(inst.count as usize * instancing::INSTANCE_FLOATS);
        for j in 0..(inst.count as usize).min(inst.matrices.len()) {
            let cols = &inst.matrices[j];
            if Mat4::from_cols_slice(cols).determinant().abs() < 1e-12 {
                continue;
            }
            let local = DMat4::from_cols_array(&std::array::from_fn(|q| f64::from(cols[q])));
            let tint = inst
                .colors
                .as_ref()
                .and_then(|c| c.get(j).copied())
                .unwrap_or([1.0; 3]);
            instancing::push_instance(&mut data, &(world * local), tint, inst.receive);
            if let Some(x) = &inst.extras {
                instancing::set_instance_extra(&mut data, x.get(j).copied().unwrap_or([0.0; 4]));
            }
        }
        // The same count: written into the stream's buffer in place (no GPU
        // allocation per frame, D497); else a new stream.
        let first = entities.get(k).and_then(|v| v.first()).copied();
        if !data.is_empty()
            && let Some(cur) = first.and_then(|e| streams.get(e).ok())
            && cur.0.update(&data)
        {
            continue;
        }
        let stream = (!data.is_empty()).then(|| Instances(Arc::new(InstanceStream::new(&data))));
        for &e in entities.get(k).map_or(&[][..], |v| v.as_slice()) {
            match &stream {
                Some(s) => {
                    commands.entity(e).insert(s.clone());
                }
                None => {
                    if let Ok((_, mut cur)) = placed.get_mut(e) {
                        *cur = Visibility::Hidden;
                    }
                }
            }
        }
    }
    index.entities = Some(entities);
}

fn change_name(c: &Change) -> &'static str {
    match c {
        Change::Visible(_) => "visible",
        Change::Transform { .. } => "transform",
        Change::InstanceMatrix { .. } => "instance matrix",
        Change::InstanceColor { .. } => "instance colour",
        Change::InstanceCount(_) => "instance count",
        Change::Attribute { .. } => "attribute",
        Change::Number { .. } => "number",
        Change::Color { .. } => "colour",
        Change::TextureOffset(_) => "texture offset",
        Change::Vector { .. } => "vector",
        Change::Light { .. } => "light",
    }
}

/// A material's number or colour, into its block: a uniform of that name
/// if the kind has one animated (D411's rule), else the parameter.
fn apply_material(index: &mut SceneIndex, i: u32, change: Change, blocks: &mut AnimBlocks) {
    let Some(kind) = index.materials.get(i as usize).map(|m| m.kind) else {
        return;
    };
    if kind == MaterialKind::SkyDome {
        return; // the dome's uniforms are the client's sky (`Lighting`)
    }
    if kind == MaterialKind::Sea
        && matches!(
            change,
            Change::Number { prop: "uTime", .. } | Change::Vector { prop: "uOff2", .. }
        )
    {
        // `Sea.js`'s clock and second ripple offset: the client's
        // `lighting::Anim` computes the same from the same dt (D495, D701).
        return;
    }
    match change {
        Change::Number { prop, value } => {
            let slot = uniform_slot(kind, prop);
            let v = value as f32;
            // The texel, the component the value goes to, and the one
            // that says it is set (none for a uniform).
            let (texel, at, flag) = match (slot, prop) {
                (Some(s), _) => (4, s, None),
                (None, "emissiveIntensity") => (2, 0, Some(1)),
                (None, "rotation") => (2, 2, Some(3)),
                _ => {
                    index.report(format!("material {i} ({kind:?}) number {prop}"));
                    return;
                }
            };
            if let Some(k) = block_of(index, i, blocks) {
                set_texel(blocks, k, texel, |t| {
                    t[at] = v;
                    if let Some(f) = flag {
                        t[f] = 1.0;
                    }
                });
            }
        }
        Change::Color { prop, rgb } => {
            let texel = match prop {
                "color" => 0,
                "emissive" => 1,
                _ => {
                    index.report(format!("material {i} ({kind:?}) colour {prop}"));
                    return;
                }
            };
            if let Some(k) = block_of(index, i, blocks) {
                set_texel(blocks, k, texel, |t| {
                    *t = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.0];
                });
            }
        }
        other => index.report(format!("material {i} ({kind:?}) {}", change_name(&other))),
    }
}

/// How a kept Bevy mesh's vertices map to its geometry's.
#[derive(Clone, Copy, Debug)]
struct VertexMap {
    /// A `Points` geometry drawn as quads (`convert::build_points`, no
    /// index): geometry vertex `start + k` is quad vertices `4k..4k + 4`,
    /// for `k` below the count. `None`: the geometry's vertices in order.
    points: Option<(usize, usize)>,
    /// The geometry's `color` item size (3 or 4; the Bevy colour is four).
    color_size: usize,
}

/// A drawn Bevy mesh an animator's attribute edits are written into.
struct KeptMesh {
    handle: Handle<Mesh>,
    map: VertexMap,
    /// Its vertices as the edits leave them: a copy of the asset's (which
    /// the loader keeps on the CPU), made at the first edit. The asset is
    /// never modified: that would have Bevy re-extract and re-upload the
    /// mesh, which on the web left a GPU buffer behind each time (D701).
    cpu: Option<Mesh>,
}

/// Vertex data for meshes already on the GPU, written into their place in
/// Bevy's mesh slab at extraction (`write_mesh_updates`): the mesh's id,
/// its packed vertices and the vertex size in bytes.
#[derive(Resource, Clone, Default)]
pub struct MeshWrites(Arc<Mutex<Vec<MeshWrite>>>);

/// One pending write: the mesh, its packed vertices, the vertex size.
type MeshWrite = (AssetId<Mesh>, Vec<u8>, u64);

impl MeshWrites {
    /// Queues `cpu`'s vertices (the same layout and count as the mesh
    /// `handle` was made with) for the mesh's place in the slab, replacing
    /// a write still pending for it (the race's effects, WP 4.4).
    pub fn push(&self, handle: &Handle<Mesh>, cpu: &Mesh) {
        let id = handle.id();
        let bytes = cpu.create_packed_vertex_buffer_data();
        let stride = cpu.get_vertex_size();
        if let Ok(mut w) = self.0.lock() {
            w.retain(|(i, _, _)| *i != id);
            w.push((id, bytes, stride));
        }
    }
}

/// Writes the pending vertex data into the meshes' slab slices (render
/// world, at extraction, as `instancing::write_instance_updates`). A mesh
/// not allocated yet keeps its data for the next frame.
pub fn write_mesh_updates(
    queue: Res<bevy::render::renderer::RenderQueue>,
    allocator: Res<bevy::render::mesh::allocator::MeshAllocator>,
    writes: bevy::render::Extract<Res<MeshWrites>>,
) {
    let Ok(mut w) = writes.0.lock() else { return };
    let pending = std::mem::take(&mut *w);
    for (id, bytes, stride) in pending {
        match allocator.mesh_vertex_slice(&id) {
            Some(s) => {
                let at = u64::from(s.range.start) * stride;
                // wgpu's copy alignment (whole floats always are).
                if at % 4 == 0 && bytes.len() % 4 == 0 {
                    queue.write_buffer(s.buffer, at, &bytes);
                }
            }
            None => w.push((id, bytes, stride)),
        }
    }
}

/// Values written into a mesh attribute from float `offset` of the
/// geometry's array of `size` floats a vertex (an animator's edit): the
/// geometry's vertices in order (triangles), or each point's four quad
/// vertices (`Points`). A Bevy colour has four components, alpha kept.
/// The mesh's whole vertex data then goes to the GPU in place
/// ([`MeshWrites`]).
fn write_attribute(
    meshes: &Assets<Mesh>,
    kept: &mut KeptMesh,
    attr: bevy::mesh::MeshVertexAttribute,
    size: usize,
    offset: usize,
    values: &[f32],
    writes: &MeshWrites,
) {
    if size == 0 {
        return;
    }
    let map = kept.map;
    if kept.cpu.is_none() {
        kept.cpu = meshes.get(&kept.handle).cloned();
    }
    let Some(mesh) = kept.cpu.as_mut() else {
        return;
    };
    // (Bevy vertex, component, value) for each value written.
    let targets = |len: usize| {
        values.iter().enumerate().flat_map(move |(j, &x)| {
            let (v, c) = ((offset + j) / size, (offset + j) % size);
            let quads = match map.points {
                None => v..v + 1,
                Some((start, count)) if v >= start && v - start < count => {
                    (v - start) * 4..(v - start) * 4 + 4
                }
                Some(_) => 0..0,
            };
            quads.filter(move |&q| q < len).map(move |q| (q, c, x))
        })
    };
    // Unchanged (a frozen frame): nothing is sent to the GPU.
    let same = match mesh.attribute(attr) {
        Some(VertexAttributeValues::Float32x3(v)) => {
            targets(v.len()).all(|(q, c, x)| c >= 3 || v[q][c] == x)
        }
        Some(VertexAttributeValues::Float32x4(v)) => {
            targets(v.len()).all(|(q, c, x)| c >= 4 || v[q][c] == x)
        }
        _ => return,
    };
    if same {
        return;
    }
    match mesh.attribute_mut(attr) {
        Some(VertexAttributeValues::Float32x3(v)) => {
            for (q, c, x) in targets(v.len()) {
                if c < 3 {
                    v[q][c] = x;
                }
            }
        }
        Some(VertexAttributeValues::Float32x4(v)) => {
            for (q, c, x) in targets(v.len()) {
                if c < 4 {
                    v[q][c] = x;
                }
            }
        }
        _ => {}
    }
    let id = kept.handle.id();
    let bytes = mesh.create_packed_vertex_buffer_data();
    let stride = mesh.get_vertex_size();
    if let Ok(mut w) = writes.0.lock() {
        // The latest data for a mesh replaces any still pending.
        w.retain(|(i, _, _)| *i != id);
        w.push((id, bytes, stride));
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<WorldGen>()
        .init_resource::<AnimBlocks>()
        .init_resource::<MeshWrites>()
        .add_systems(OnEnter(AppState::Waiting), reset_world)
        .add_systems(Update, drive_build)
        .add_systems(
            PostUpdate,
            (
                run_animators.before(bevy::transform::TransformSystems::Propagate),
                pack_blocks.after(crate::render::lighting::pack_globals),
            ),
        );
    if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
        render_app.add_systems(
            bevy::render::ExtractSchedule,
            (instancing::write_instance_updates, write_mesh_updates),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_sit_where_the_shader_reads_them() {
        let wgsl = include_str!("render/three_globals.wgsl");
        assert!(wgsl.contains(&format!("const G_BLOCKS: i32 = {G_BLOCKS};")));
        // WebGL2's smallest maximum texture size.
        const { assert!(G_BLOCKS + MAX_BLOCKS * BLOCK_TEXELS <= 2048) };
    }

    #[test]
    fn each_kind_has_its_own_uniform_slots() {
        use MaterialKind::*;
        let slots: Vec<usize> = UNIFORMS
            .iter()
            .filter_map(|u| uniform_slot(TrafficStreams, u))
            .collect();
        assert_eq!(slots, vec![0, 1, 2, 3]);
        assert_eq!(uniform_slot(SkyGlow, "uK"), Some(0));
        assert_eq!(uniform_slot(Asphalt, "uWet"), Some(0));
        assert_eq!(uniform_slot(GlowPoints, "uFogK"), Some(0));
        assert_eq!(uniform_slot(Standard, "uTime"), None);
        assert!(generated("sierra") && generated("seaside") && generated("cruise"));
        assert!(generated("desert") && generated("streets") && generated("coast"));
        assert!(!generated("models"));
        for k in [Standard, Lambert, Basic, Sprite, Points] {
            assert_eq!(uniform_slot(k, "opacity"), Some(0));
        }
    }
}
