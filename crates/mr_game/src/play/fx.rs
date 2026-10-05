//! `Effects.js` drawn (roadmap WP 4.4): the state of [`super::effects`]
//! on Bevy entities, made with the race's cars and updated once a rendered
//! frame after the cars are placed, as `Race.update` runs `effects.update`
//! after its visual sync.
//!
//! - **Smoke and sparks**: one mesh each of camera-facing quads, four
//!   vertices a particle (SPEC 6.2 "Points"), at the origin and never
//!   culled, as the JS's `Points` (`frustumCulled = false`); the
//!   `Particles` material kind (`render::material::Patch::Particles`).
//!   Their vertices are written each frame into the mesh's place in Bevy's
//!   vertex slab (`animate::MeshWrites`, D701), never into the asset, while
//!   a particle lives.
//! - **Skid marks**: the 2,400-quad mesh, written when a mark is added
//!   (`SkidMarks`, `Patch::Skid`).
//! - **Flames**: two additive cones per exhaust on the car's body, shown
//!   with the nitro, stretched at random each frame.
//! - **Headlight pools**: one additive glow quad per car, on the road
//!   ahead after dusk. The JS's pools share one material whose opacity the
//!   last line of `update` sets to 0.3 × night (D800); here that opacity
//!   sits in a light slot of the globals (D803), so no material is edited
//!   per frame.
//!
//! The flames, pools and both particle kinds join the pipeline warm-up
//! with the cars (D458), so nothing compiles once the race runs.

use super::effects::{CarIn, Effects, Extras, Particles};
use super::flow::{Mode, Race};
use super::models::Cars;
use crate::animate::MeshWrites;
use crate::convert::{self, Draw, MeshKey};
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::{Model, Patch, ThreeKey, ThreeMaterial, ThreeParams, three_material};
use crate::warmup::{Combos, Layout};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Vec4;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use mr_scene::three;
use mr_sim::physics::PhysEvent;
use mr_sim::race::{RaceStateKind, SimEvent, SimState};
use mr_track::Track;
use mr_worldgen::material::Material;
use mr_worldgen::object::{Layer, SceneGraph};
use mr_worldgen::textures::TextureCache;
use mr_worldgen::three_geom::{cone_geometry, plane_geometry};
use std::f64::consts::PI;

/// An entity the effects place each frame.
#[derive(Component)]
pub struct FxPart;

/// One particle ring drawn: its mesh, the CPU copy its vertices are packed
/// from, and whether the last frame wrote it.
struct PointsMesh {
    mesh: Handle<Mesh>,
    cpu: Mesh,
    material: Handle<ThreeMaterial>,
    was_live: bool,
}

/// The race's effects and what draws them.
pub struct Fx {
    pub fx: Effects,
    smoke: PointsMesh,
    sparks: PointsMesh,
    skids: (Handle<Mesh>, Mesh),
    /// The smoke's, sparks' and skids' entities, and the points three sorts
    /// them by: their geometry's bounding-sphere centre, computed the first
    /// time three renders them and kept (D808). The entity sits there and its
    /// vertices are written relative to it, so Bevy sorts it by that point.
    rings: [Entity; 3],
    centres: [Vec3; 3],
    /// Per car slot: its pool, its flames (the outer cones).
    pools: Vec<Entity>,
    flames: Vec<Vec<Entity>>,
    /// The entities to take away with the race.
    pub entities: Vec<Entity>,
    /// The pools' opacity's light slot (D803).
    pool_slot: Option<usize>,
    /// The drawing buffer's height and the field of view `resize` saw.
    resized: (f32, f32),
    /// `Race::starts` the effects belong to (a restart is a new `Race` in
    /// the JS, with new effects).
    starts: u32,
    seed: u32,
    /// The player's throttle as the last tick read it (`inp.throttle`).
    throttle: f64,
}

/// Each effect's place among transparent objects at the same distance
/// (`ThreeKey::sort_rank`, D808): three breaks such ties by object id, the
/// order `Effects` makes them in: the smoke, the sparks, the skids, then per
/// car its flames and its pool.
const RANK_SMOKE: u8 = 1;
const RANK_SPARKS: u8 = 2;
const RANK_SKIDS: u8 = 3;
const RANK_FLAMES: u8 = 4;
const RANK_POOLS: u8 = 5;

/// three's `computeBoundingSphere` centre of a position array: the middle of
/// its bounding box, in `f32` as three keeps it.
fn box_centre(pos: &[f32]) -> Vec3 {
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for p in pos.as_chunks::<3>().0 {
        let v = Vec3::new(p[0], p[1], p[2]);
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if lo.x > hi.x {
        return Vec3::ZERO;
    }
    (lo + hi) * 0.5
}

/// The effects' random stream for a race (D801): from the race's seed, so
/// a seeded race's effects are the same each time.
fn fx_seed(race_seed: u32) -> u32 {
    race_seed ^ 0x5eed_0e44
}

/// A mesh of `n` points as quads: positions, colours, and (size, alpha,
/// corner) in the extra attribute.
fn points_mesh(n: usize) -> Mesh {
    const CORNERS: [[f32; 2]; 4] = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]];
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32; 3]; n * 4]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0f32, 0.0, 0.0, 1.0]; n * 4]);
    let extra: Vec<[f32; 4]> = (0..n)
        .flat_map(|_| CORNERS.map(|c| [0.0, 0.0, c[0], c[1]]))
        .collect();
    m.insert_attribute(convert::ATTRIBUTE_EXTRA, extra);
    let idx: Vec<u32> = (0..n as u32)
        .flat_map(|k| {
            let b = k * 4;
            [b, b + 1, b + 2, b, b + 2, b + 3]
        })
        .collect();
    m.insert_indices(Indices::U32(idx));
    m
}

/// The skid marks' mesh: four corners a quad (`b, b+2, b+1, b+1, b+2, b+3`,
/// wound to face up as the JS now is, D807) with the alpha in the extra
/// attribute.
fn skid_mesh(n: usize) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0f32; 3]; n * 4]);
    m.insert_attribute(convert::ATTRIBUTE_EXTRA, vec![[0f32; 4]; n * 4]);
    let idx: Vec<u32> = (0..n as u32)
        .flat_map(|i| {
            let b = i * 4;
            [b, b + 2, b + 1, b + 1, b + 2, b + 3]
        })
        .collect();
    m.insert_indices(Indices::U32(idx));
    m
}

/// `Particles`' `ShaderMaterial`: transparent, no depth write, fog,
/// normal or additive blending, `uTex`, `uScale`.
fn particles_material(
    map: Option<Handle<Image>>,
    additive: bool,
    rank: u8,
    scale: f64,
    shared: &SharedImages,
) -> ThreeMaterial {
    ThreeMaterial {
        params: ThreeParams {
            diffuse: Vec4::ONE,
            kind0: Vec4::new(scale as f32, 0.0, 0.0, 0.0),
            ..ThreeParams::default()
        },
        map,
        emissive_map: None,
        detail: None,
        aux: None,
        photo: None,
        loose: None,
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key: ThreeKey {
            model: Model::Basic,
            map: true,
            fog: true,
            // Points are never culled.
            side: three::DOUBLE_SIDE,
            shadow_side: three::DOUBLE_SIDE,
            blending: if additive {
                three::ADDITIVE_BLENDING
            } else {
                three::NORMAL_BLENDING
            },
            depth_write: false,
            depth_test: true,
            patch: Patch::Particles,
            sort_rank: rank,
            ..ThreeKey::default()
        },
    }
}

/// `SkidMarks`' `ShaderMaterial`: transparent, no depth write, polygon
/// offset (-4, -4), front faces only (three's default side), no fog.
fn skid_material(shared: &SharedImages) -> ThreeMaterial {
    ThreeMaterial {
        params: ThreeParams {
            diffuse: Vec4::ONE,
            ..ThreeParams::default()
        },
        map: None,
        emissive_map: None,
        detail: None,
        aux: None,
        photo: None,
        loose: None,
        globals: shared.globals.clone(),
        env: shared.env.clone(),
        key: ThreeKey {
            model: Model::Basic,
            side: three::FRONT_SIDE,
            shadow_side: three::BACK_SIDE,
            blending: three::NORMAL_BLENDING,
            depth_write: false,
            depth_test: true,
            depth_bias: 256,
            // polygonOffsetFactor -4 (D803).
            depth_slope: 4,
            patch: Patch::Skid,
            sort_rank: RANK_SKIDS,
            ..ThreeKey::default()
        },
    }
}

/// What `Effects`' constructor builds with three's own classes: the
/// flame cone and its two materials, the pool's plane and material, and
/// the particles' textures, made as `mr_worldgen` makes scenery so the
/// geometry and the materials are three's.
struct Kit {
    flame: (Handle<Mesh>, Layout),
    flame_mat: (Handle<ThreeMaterial>, ThreeKey),
    flame_core: (Handle<ThreeMaterial>, ThreeKey),
    pool: (Handle<Mesh>, Layout),
    pool_mat: (Handle<ThreeMaterial>, ThreeKey),
    smoke_tex: Option<Handle<Image>>,
    glow_tex: Option<Handle<Image>>,
}

fn kit(
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut Assets<ThreeMaterial>,
    shared: &SharedImages,
) -> Option<Kit> {
    const ADDITIVE: f64 = three::ADDITIVE_BLENDING as f64;
    let mut graph = SceneGraph::new();
    let mut textures = TextureCache::new();
    let flame_mat = graph.add_material(
        Material::basic()
            .set("color", 0x66aaff)
            .set("transparent", true)
            .set("opacity", 0.85)
            .set("blending", ADDITIVE)
            .set("depthWrite", false),
    );
    let flame_core = graph.add_material(
        Material::basic()
            .set("color", 0xffffff)
            .set("transparent", true)
            .set("opacity", 0.9)
            .set("blending", ADDITIVE)
            .set("depthWrite", false),
    );
    let mut cone = cone_geometry(0.13, 1.0, 10.0, 1.0, true, 0.0, PI * 2.0);
    cone.rotate_x(-PI / 2.0); // point down −Z (backwards)
    cone.translate(0.0, 0.0, -0.5);
    let flame_geo = graph.add_geometry(cone);
    let glow = graph.cached_texture(&textures.glow_texture(), Layer::Main, "");
    let smoke = graph.cached_texture(&textures.smoke_texture(), Layer::Main, "");
    let mut plane = plane_geometry(1.0, 1.0, 1.0, 1.0);
    plane.rotate_x(-PI / 2.0);
    let pool_geo = graph.add_geometry(plane);
    let pool_mat = graph.add_material(
        Material::basic()
            .set("map", glow)
            .set("color", 0xfff1d0)
            .set("transparent", true)
            .set("opacity", 0.0)
            .set("blending", ADDITIVE)
            .set("depthWrite", false)
            .set("polygonOffset", true)
            .set("polygonOffsetFactor", -6.0)
            .set("polygonOffsetUnits", -6.0),
    );
    // The smoke texture rides on a material nothing draws, to be exported.
    let smoke_mat = graph.add_material(Material::basic().set("map", smoke));
    let nodes = [
        graph.mesh(flame_geo, flame_mat),
        graph.mesh(flame_geo, flame_core),
        graph.mesh(pool_geo, pool_mat),
        graph.mesh(pool_geo, smoke_mat),
    ];
    for n in nodes {
        graph.roots.push(n);
    }
    let (scene, handles) = graph.finish();
    let image_handles: Vec<Option<Handle<Image>>> = scene
        .textures
        .iter()
        .map(|t| convert::build_image(&scene, t).map(|i| images.add(i)))
        .collect();
    let tex = |id| {
        handles
            .texture(id)
            .and_then(|i| image_handles.get(i as usize).cloned().flatten())
    };
    let mesh = |meshes: &mut Assets<Mesh>, node: usize| {
        let n = &scene.nodes[node];
        let m = n.mesh?;
        let (start, count) = convert::draw_span(&scene, &scene.meshes[m as usize], None);
        let key = MeshKey {
            mesh: m,
            start,
            count,
            draw: Draw::Triangles,
            colors: false,
            lit: false,
            extra: None,
        };
        let mesh = convert::build_mesh(&scene, key)?;
        let layout = Layout::of(&mesh);
        Some((meshes.add(mesh), layout))
    };
    let material = |mats: &mut Assets<ThreeMaterial>, id, rank: u8| {
        let i = handles.material(id)?;
        let mut t = three_material(
            &scene,
            &scene.materials[i as usize],
            &image_handles,
            shared,
            false,
        )?;
        // three's polygon offset grows with the slope (the pool's factor,
        // -6); `three_material` gives the constant part (D803).
        if scene.materials[i as usize].boolean("polygonOffset") == Some(true) {
            let f = scene.materials[i as usize]
                .number("polygonOffsetFactor")
                .unwrap_or(0.0);
            t.key.depth_slope = (-f) as i32;
        }
        t.key.sort_rank = rank;
        let key = t.key;
        Some((mats.add(t), key))
    };
    let node = |id| handles.node(id).map(|i| i as usize);
    Some(Kit {
        flame: mesh(meshes, node(nodes[0])?)?,
        pool: mesh(meshes, node(nodes[2])?)?,
        flame_mat: material(mats, flame_mat, RANK_FLAMES)?,
        flame_core: material(mats, flame_core, RANK_FLAMES)?,
        pool_mat: material(mats, pool_mat, RANK_POOLS)?,
        smoke_tex: tex(smoke),
        glow_tex: tex(glow),
    })
}

/// A car the effects are given (`addCar`): its exhausts in its body's
/// frame and the body's entity, which the flames hang on.
pub struct CarSpec {
    pub player: bool,
    pub exhausts: Vec<[f64; 3]>,
    pub body: Option<Entity>,
}

impl CarSpec {
    /// The race's cars, the player first (`{ player: true }`).
    pub fn of(cars: &Cars, players: usize) -> Vec<CarSpec> {
        cars.cars
            .iter()
            .enumerate()
            .map(|(i, c)| CarSpec {
                player: i < players,
                exhausts: c.model.exhausts.iter().map(|e| [e.x, e.y, e.z]).collect(),
                body: c.body_entity(),
            })
            .collect()
    }
}

/// What a race's effects are made from.
pub struct Assets3<'a> {
    pub meshes: &'a mut Assets<Mesh>,
    pub images: &'a mut Assets<Image>,
    pub mats: &'a mut Assets<ThreeMaterial>,
    pub shared: &'a SharedImages,
    pub lights: &'a mut MaterialLights,
}

impl Fx {
    /// `new Effects(scene, renderer, camera)`, then `addCar` for every car
    /// in order, and `resize` with the drawing buffer's height and the
    /// camera's field of view now (`startRace`).
    pub fn spawn(
        commands: &mut Commands,
        a: Assets3,
        cars: &[CarSpec],
        race_seed: u32,
        (height_px, fov_deg): (f32, f32),
    ) -> Option<Fx> {
        let kit = kit(a.meshes, a.images, a.mats, a.shared)?;
        let mut fx = Effects::new(fx_seed(race_seed));
        fx.resize(f64::from(height_px), f64::from(fov_deg));
        let mut combos = Combos::default();
        let mut entities = Vec::new();
        let mut part = |commands: &mut Commands,
                        name: &str,
                        mesh: Handle<Mesh>,
                        mat: Handle<ThreeMaterial>| {
            let e = commands
                .spawn((
                    Mesh3d(mesh),
                    MeshMaterial3d(mat),
                    Transform::IDENTITY,
                    Visibility::Inherited,
                    NoFrustumCulling,
                    NotShadowCaster,
                    NotShadowReceiver,
                    FxPart,
                    crate::loader::SceneEntity,
                    Name::new(name.to_owned()),
                ))
                .id();
            entities.push(e);
            e
        };
        // `scene.add(this.smoke.points, this.sparks.points)`, then the
        // skids.
        let mut points = |commands: &mut Commands,
                          n: usize,
                          tex: Option<Handle<Image>>,
                          (additive, rank): (bool, u8),
                          name: &str,
                          combos: &mut Combos| {
            let cpu = points_mesh(n);
            let layout = Layout::of(&cpu);
            let mesh = a.meshes.add(cpu.clone());
            let m = particles_material(tex, additive, rank, fx.scale, a.shared);
            let key = m.key;
            let material = a.mats.add(m);
            combos.note(key, &material, &layout, false);
            let e = part(commands, name, mesh.clone(), material.clone());
            (
                PointsMesh {
                    mesh,
                    cpu,
                    material,
                    was_live: false,
                },
                e,
            )
        };
        let (smoke, smoke_e) = points(
            commands,
            fx.smoke.max,
            kit.smoke_tex.clone(),
            (false, RANK_SMOKE),
            "fx smoke",
            &mut combos,
        );
        let (sparks, sparks_e) = points(
            commands,
            fx.sparks.max,
            kit.glow_tex.clone(),
            (true, RANK_SPARKS),
            "fx sparks",
            &mut combos,
        );
        let skid_cpu = skid_mesh(fx.skids.max);
        let skid_layout = Layout::of(&skid_cpu);
        let skid_handle = a.meshes.add(skid_cpu.clone());
        let sm = skid_material(a.shared);
        let skid_key = sm.key;
        let skid_mat = a.mats.add(sm);
        combos.note(skid_key, &skid_mat, &skid_layout, false);
        let skids_e = part(commands, "fx skids", skid_handle.clone(), skid_mat);
        // The pools' shared opacity (D803).
        let pool_slot = a.lights.slot(0.0);
        if let Some(k) = pool_slot
            && let Some(mut m) = a.mats.get_mut(&kit.pool_mat.0)
        {
            m.params.night = Vec4::new(0.0, 0.0, 3.0, k as f32);
        }
        combos.note(kit.pool_mat.1, &kit.pool_mat.0, &kit.pool.1, false);
        combos.note(kit.flame_mat.1, &kit.flame_mat.0, &kit.flame.1, false);
        combos.note(kit.flame_core.1, &kit.flame_core.0, &kit.flame.1, false);
        let mut pools = Vec::new();
        let mut flames = Vec::new();
        for car in cars {
            fx.add_car(car.player, car.exhausts.len());
            let mut fl = Vec::new();
            if let Some(body) = car.body {
                for e in &car.exhausts {
                    let outer = commands
                        .spawn((
                            Mesh3d(kit.flame.0.clone()),
                            MeshMaterial3d(kit.flame_mat.0.clone()),
                            Transform::from_xyz(e[0] as f32, e[1] as f32, e[2] as f32),
                            Visibility::Hidden,
                            NotShadowCaster,
                            NotShadowReceiver,
                            FxPart,
                            ChildOf(body),
                        ))
                        .id();
                    commands.spawn((
                        Mesh3d(kit.flame.0.clone()),
                        MeshMaterial3d(kit.flame_core.0.clone()),
                        Transform::from_scale(Vec3::new(0.45, 0.45, 0.6)),
                        Visibility::Inherited,
                        NotShadowCaster,
                        NotShadowReceiver,
                        ChildOf(outer),
                    ));
                    fl.push(outer);
                }
            }
            flames.push(fl);
            let pool = commands
                .spawn((
                    Mesh3d(kit.pool.0.clone()),
                    MeshMaterial3d(kit.pool_mat.0.clone()),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    NotShadowCaster,
                    NotShadowReceiver,
                    FxPart,
                    crate::loader::SceneEntity,
                    Name::new("fx pool"),
                ))
                .id();
            entities.push(pool);
            pools.push(pool);
        }
        let n = combos.spawn(commands, a.meshes);
        info!("race effects: {n} material × mesh-layout combinations warmed up");
        let mut fx = Fx {
            fx,
            smoke,
            sparks,
            skids: (skid_handle, skid_cpu),
            rings: [smoke_e, sparks_e, skids_e],
            centres: [Vec3::ZERO; 3],
            pools,
            flames,
            entities,
            pool_slot,
            resized: (height_px, fov_deg),
            starts: 0,
            seed: race_seed,
            throttle: 0.0,
        };
        // A race's first render comes before its effects' first update:
        // three's spheres are of empty buffers, centred on the origin.
        fx.fix_sort_centres();
        Some(fx)
    }

    /// Fixes the points the smoke, sparks and skids sort by from what their
    /// buffers hold now, as three's first render of them does (D808): the
    /// race calls it at once, the staged scenes after their frames.
    pub fn fix_sort_centres(&mut self) {
        self.centres = [
            box_centre(&self.fx.smoke.pos),
            box_centre(&self.fx.sparks.pos),
            box_centre(&self.fx.skids.pos),
        ];
    }

    /// A restart: new effects for the same cars (the JS's new `Race`
    /// builds new ones), the marks and particles gone.
    fn reset(&mut self, race_seed: u32) {
        let mut fresh = Effects::new(fx_seed(race_seed));
        fresh.scale = self.fx.scale;
        for c in &self.fx.cars {
            fresh.add_car(c.player, c.flames.len());
        }
        self.fx = fresh;
        self.seed = race_seed;
        // Everything drawn so far is cleared on the next write.
        self.smoke.was_live = true;
        self.sparks.was_live = true;
        self.fx.skids.dirty = true;
    }

    /// `resize(heightPx, fovDeg)` when the drawing buffer's height changes
    /// (`main.js`'s resize handler, with the camera's field of view then).
    pub fn resize(&mut self, height_px: f32, fov_deg: f32, mats: &mut Assets<ThreeMaterial>) {
        if (height_px - self.resized.0).abs() < 0.5 {
            return;
        }
        self.resized = (height_px, fov_deg);
        self.fx.resize(f64::from(height_px), f64::from(fov_deg));
        for h in [&self.smoke.material, &self.sparks.material] {
            if let Some(mut m) = mats.get_mut(h) {
                m.params.kind0.x = self.fx.scale as f32;
            }
        }
    }
}

/// A particle ring's vertices into its mesh, then into the slab.
fn write_points(p: &Particles, pm: &mut PointsMesh, c: Vec3, writes: &MeshWrites) {
    if !p.live && !pm.was_live {
        return;
    }
    pm.was_live = p.live;
    if let Some(VertexAttributeValues::Float32x3(v)) =
        pm.cpu.attribute_mut(Mesh::ATTRIBUTE_POSITION)
    {
        for i in 0..p.max {
            let q = [
                p.pos[i * 3] - c.x,
                p.pos[i * 3 + 1] - c.y,
                p.pos[i * 3 + 2] - c.z,
            ];
            v[i * 4..i * 4 + 4].fill(q);
        }
    }
    if let Some(VertexAttributeValues::Float32x4(v)) = pm.cpu.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
        for i in 0..p.max {
            let c = [p.col[i * 3], p.col[i * 3 + 1], p.col[i * 3 + 2], 1.0];
            v[i * 4..i * 4 + 4].fill(c);
        }
    }
    if let Some(VertexAttributeValues::Float32x4(v)) =
        pm.cpu.attribute_mut(convert::ATTRIBUTE_EXTRA)
    {
        for i in 0..p.max {
            for q in &mut v[i * 4..i * 4 + 4] {
                q[0] = p.size[i];
                q[1] = p.alpha[i];
            }
        }
    }
    writes.push(&pm.mesh, &pm.cpu);
}

/// The skid marks' buffers into their mesh, then into the slab.
fn write_skids(
    fx: &Effects,
    (handle, cpu): &mut (Handle<Mesh>, Mesh),
    c: Vec3,
    writes: &MeshWrites,
) {
    let s = &fx.skids;
    if let Some(VertexAttributeValues::Float32x3(v)) = cpu.attribute_mut(Mesh::ATTRIBUTE_POSITION) {
        for (k, q) in v.iter_mut().enumerate() {
            *q = [
                s.pos[k * 3] - c.x,
                s.pos[k * 3 + 1] - c.y,
                s.pos[k * 3 + 2] - c.z,
            ];
        }
    }
    if let Some(VertexAttributeValues::Float32x4(v)) = cpu.attribute_mut(convert::ATTRIBUTE_EXTRA) {
        for (k, q) in v.iter_mut().enumerate() {
            q[0] = s.alpha[k];
        }
    }
    writes.push(handle, cpu);
}

/// A car as the effects see it this frame: between the last two ticks as
/// it is drawn (SPEC 6.5).
fn car_in(a: &mr_sim::vehicle::Vehicle, b: &mr_sim::vehicle::Vehicle, t: f64) -> CarIn {
    let l = |p: f64, q: f64| p + (q - p) * t;
    CarIn {
        x: l(a.x, b.x),
        y: l(a.y, b.y),
        z: l(a.z, b.z),
        yaw: a.yaw + mr_math::wrap_angle(b.yaw - a.yaw) * t,
        visual_yaw: l(a.visual_yaw, b.visual_yaw),
        vx: l(a.vx, b.vx),
        vz: l(a.vz, b.vz),
        on_ground: if t < 0.5 { a.on_ground } else { b.on_ground },
        visible: true,
        wheel_base: 0.0,
        track: 0.0,
    }
}

/// The frame's sparks from the ticks' contacts (`Race.update`: a car hit
/// with the player, a wall impact), then a scrape's, in the JS's order.
fn frame_sparks(fx: &mut Effects, log: &[SimEvent], st: &SimState, track: &Track, dt: f64) {
    let p = &st.players[0];
    let (vx, vz) = (p.v.vx, p.v.vz);
    for e in log {
        match e {
            SimEvent::CarHit {
                hit,
                player: Some(0),
            } => fx.sparks_at(
                hit.x,
                hit.y,
                hit.z,
                mr_math::js::round(8.0 + hit.strength * 30.0),
                vx,
                vz,
            ),
            SimEvent::Phys {
                player: 0,
                e: PhysEvent::Impact {
                    strength, x, y, z, ..
                },
            } => fx.sparks_at(
                *x,
                *y,
                *z,
                mr_math::js::round(6.0 + strength * 40.0),
                vx,
                vz,
            ),
            _ => {}
        }
    }
    // `if (this.phys.scrape > 0.3 && Math.random() < 0.6)`, at its 60 Hz
    // rate (D802).
    if p.phys.scrape > 0.3 && fx.random() < super::effects::rate60(0.6, dt) {
        let v = &p.v;
        let f = track.frame(v.s);
        let side = f64::from(p.phys.scrape_side.filter(|s| *s != 0).unwrap_or(1));
        let x = v.x + f.rx * side * v.half_w;
        let z = v.z + f.rz * side * v.half_w;
        fx.sparks_at(x, v.y + 0.4, z, 2.0, v.vx, v.vz);
    }
}

/// One frame of the effects (`Race.update`'s "Effects." block), after the
/// cars are drawn: the extras, `effects.update`, and what it moved onto
/// the entities.
#[allow(clippy::too_many_arguments)]
pub fn frame(
    fx: &mut Fx,
    race: &Race,
    cars: &Cars,
    dt: f64,
    night: f64,
    parts: &mut Query<(&mut Transform, &mut Visibility), With<FxPart>>,
    lights: &mut MaterialLights,
    writes: &MeshWrites,
) {
    let s = &race.session;
    if fx.starts != race.starts {
        if fx.starts != 0 {
            fx.reset(race.setup.opts.seed);
        }
        fx.starts = race.starts;
    }
    if let Some(t) = race.audio_ticks.last() {
        fx.throttle = t.state.throttle.unwrap_or(0.0);
    }
    // Paused (or held before the countdown): `race.update` does not run.
    let running = race.mode != Mode::Paused && dt > 0.0;
    if running {
        let track = &*s.lr.track;
        frame_sparks(&mut fx.fx, &race.log, &s.curr, track, dt);
        let alpha = s.alpha();
        let prev: Vec<_> = super::flow::slots(&s.prev).collect();
        let mut ins = Vec::with_capacity(cars.cars.len());
        for (i, ((v, active), car)) in super::flow::slots(&s.curr).zip(&cars.cars).enumerate() {
            let a = match prev.get(i) {
                Some((pv, true)) => *pv,
                _ => v,
            };
            let mut c = car_in(a, v, alpha);
            c.visible = active;
            c.wheel_base = car.model.dims.wheel_base;
            c.track = car.model.dims.track;
            ins.push(c);
        }
        // Hot Pursuit: the police cars follow the field in `addCar`'s
        // order, and PursuitView.sync's smoke and sparks come before the
        // update (WP 8.2).
        if let Some(pv) = &s.curr.pv {
            let pu = &pv.pursuit;
            let pp = s.prev.pv.as_ref().map_or(pu, |p| &p.pursuit);
            let n = pu.units.len() + pu.block_cars.len();
            for (k, car) in (0..n).zip(cars.cars.iter().skip(cars.police_base)) {
                let (u, a) = (pu.police(k), pp.police(k));
                let a = if a.active { &a.k.v } else { &u.k.v };
                let mut c = car_in(a, &u.k.v, alpha);
                c.visible = u.active;
                c.wheel_base = car.model.dims.wheel_base;
                c.track = car.model.dims.track;
                ins.push(c);
            }
            if let Some(player) = ins.first() {
                let player = *player;
                super::police::emit(
                    &mut fx.fx,
                    &super::police::EmitIn {
                        log: &race.log,
                        pursuit: pu,
                        damage: pv.damage,
                        spiked: s.curr.players[0].phys.spiked,
                        player: &player,
                        dt,
                        night,
                    },
                );
            }
        }
        let st = &s.curr;
        // `this.extras`: the players and the rivals only; the traffic and
        // the police have no entry (D945).
        let mut extras =
            vec![Extras::default(); (st.players.len() + st.rivals.len()).min(ins.len())];
        if let Some(p) = st.players.first() {
            extras[0] = Extras {
                nitro: p.phys.nitro_active,
                skid: p.phys.skid,
                launch: st.race.state == RaceStateKind::Countdown && fx.throttle > 0.5,
            };
        }
        let np = st.players.len();
        for (k, r) in st.rivals.iter().enumerate() {
            if let Some(e) = extras.get_mut(np + k) {
                *e = Extras {
                    nitro: r.nitro_active,
                    skid: 0.0,
                    launch: false,
                };
            }
        }
        fx.fx.update(dt, night, &ins, &extras);
        fx.write(writes);
    }
    fx.place(parts, lights);
}

impl Fx {
    /// What `update` changed into the meshes' vertex data.
    pub fn write(&mut self, writes: &MeshWrites) {
        let [cs, cp, ck] = self.centres;
        write_points(&self.fx.smoke, &mut self.smoke, cs, writes);
        write_points(&self.fx.sparks, &mut self.sparks, cp, writes);
        if self.fx.skids.flush() {
            write_skids(&self.fx, &mut self.skids, ck, writes);
        }
    }

    /// The pools, the flames and the pools' opacity onto what is drawn.
    pub fn place(
        &self,
        parts: &mut Query<(&mut Transform, &mut Visibility), With<FxPart>>,
        lights: &mut MaterialLights,
    ) {
        let fx = self;
        for (&e, &c) in fx.rings.iter().zip(&fx.centres) {
            if let Ok((mut t, _)) = parts.get_mut(e)
                && t.translation != c
            {
                t.translation = c;
            }
        }
        if let Some(k) = fx.pool_slot
            && let Some(v) = lights.values.get_mut(k)
        {
            *v = fx.fx.pool_opacity as f32;
        }
        let vis = |on: bool| {
            if on {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        };
        for (c, &e) in fx.fx.cars.iter().zip(&fx.pools) {
            if let Ok((mut t, mut v)) = parts.get_mut(e) {
                let p = c.pool;
                if *v != vis(p.visible) {
                    *v = vis(p.visible);
                }
                if p.visible {
                    *t = Transform {
                        translation: Vec3::new(p.x as f32, p.y as f32, p.z as f32),
                        rotation: Quat::from_rotation_y(p.rot_y as f32),
                        scale: Vec3::new(p.sx as f32, 1.0, p.sz as f32),
                    };
                }
            }
        }
        for (c, fl) in fx.fx.cars.iter().zip(&fx.flames) {
            for (&e, &len) in fl.iter().zip(&c.flames) {
                if let Ok((mut t, mut v)) = parts.get_mut(e) {
                    if *v != vis(c.flames_on) {
                        *v = vis(c.flames_on);
                    }
                    if c.flames_on {
                        t.scale = Vec3::new(1.0, 1.0, len as f32);
                    }
                }
            }
        }
    }
}
