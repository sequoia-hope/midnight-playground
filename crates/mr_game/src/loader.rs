//! The scene loader (roadmap WP 2.2): a `.mrscene` becomes Bevy meshes,
//! materials, images, lights and entities, a slice per frame so the page
//! keeps drawing its progress bar. When it is done the `Scene` (every CPU
//! copy of the data) is dropped; Bevy keeps only the GPU copies.
//!
//! Transforms are the export's world matrices (`matrix_world`), so the node
//! tree is flattened: nothing in a scene moves yet (animators arrive with
//! world generation). An `InstancedMesh` becomes one entity per instance,
//! which Bevy batches back into instanced draws.

use crate::convert::{self, Draw, MeshKey, StandIn};
use crate::render::lighting::{Lighting, Point, Spot};
use crate::render::material::{ThreeKey, three_material};
use crate::render::pmrem::EnvRequest;
use crate::render::{SharedImages, SkyMaterial, ThreeMaterial};
use crate::status::Status;
use crate::warmup::{Combos, Layout};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DVec3, Mat4, Vec3, Vec4};
use bevy::mesh::MeshTag;
use bevy::platform::time::Instant;
use bevy::prelude::*;
use mr_scene::{MaterialKind, NodeType, Scene};
use std::collections::HashMap;

/// Everything spawned from a scene, for teardown.
#[derive(Component)]
pub struct SceneEntity;

/// The sky dome, which follows the focus as `Sky.update` moves it.
#[derive(Component)]
pub struct SkyDome;

/// `world.nightMaterials` (`World.js` `addNight`): material properties that
/// follow nightfall, `value = day + (night - day) × n`. Every one in the
/// exports is an `emissiveIntensity`.
#[derive(Resource, Default)]
pub struct NightMaterials {
    pub entries: Vec<NightEntry>,
    /// The night factor last applied.
    pub last: Option<f64>,
}

pub struct NightEntry {
    pub materials: Vec<Handle<ThreeMaterial>>,
    /// The material's `emissive` colour (three's uniform is colour ×
    /// intensity).
    pub emissive: [f64; 3],
    pub day: f64,
    pub night: f64,
}

/// Sets the night-following properties for the sky's night factor (`World
/// .update`), when it has changed.
pub fn apply_night(
    sky: Res<crate::SkyRes>,
    mut night: ResMut<NightMaterials>,
    mut assets: ResMut<Assets<ThreeMaterial>>,
) {
    let Some(n) = sky.sky.as_ref().map(|s| s.night) else {
        return;
    };
    if night.last == Some(n) {
        return;
    }
    night.last = Some(n);
    for e in &night.entries {
        let k = e.day + (e.night - e.day) * n;
        for h in &e.materials {
            if let Some(mut m) = assets.get_mut(h) {
                let w = m.params.emissive.w;
                m.params.emissive = Vec4::new(
                    (e.emissive[0] * k) as f32,
                    (e.emissive[1] * k) as f32,
                    (e.emissive[2] * k) as f32,
                    w,
                );
            }
        }
    }
}

/// What a finished build leaves behind for the camera and the HUD.
#[derive(Resource, Default, Clone, Debug)]
pub struct Loaded {
    /// The camera the scene was exported with (models: a computed view).
    pub camera: Option<Transform>,
    /// Bounds over every drawn node's origin (for the models view).
    pub min: Vec3,
    pub max: Vec3,
    /// The bounds were set by the models' layout, not gathered.
    pub fixed_bounds: bool,
    pub counts: Counts,
}

#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub entities: usize,
    pub meshes: usize,
    pub materials: usize,
    pub images: usize,
    /// Drawables skipped because their material kind is not drawn yet.
    pub hidden_kinds: Vec<(MaterialKind, usize)>,
    /// Sprites and other node types not drawn yet.
    pub skipped_nodes: usize,
    /// Nodes exported with `visible: false` (or under one).
    pub invisible: usize,
}

/// A linear colour in 0..2 as the tag `three_material.wgsl` unpacks (three's
/// instanceColor): 10 bits a channel.
pub fn pack_tint(c: [f32; 3]) -> u32 {
    let q = |x: f32| ((x * 511.5).round().clamp(0.0, 1023.0)) as u32;
    q(c[0]) | (q(c[1]) << 10) | (q(c[2]) << 20)
}

#[derive(Default)]
enum Step {
    #[default]
    Textures,
    Nodes,
    Lights,
    Done,
}

/// A build in progress.
#[derive(Resource)]
pub struct Build {
    scene: Scene,
    step: Step,
    cursor: usize,
    images: Vec<Option<Handle<Image>>>,
    meshes: HashMap<MeshKey, Option<Handle<Mesh>>>,
    /// Each mesh's vertex layout, for the warm-up.
    layouts: HashMap<AssetId<Mesh>, Layout>,
    /// Per JS material and whether its instances carry colours.
    materials: HashMap<(u32, bool), Option<Handle<ThreeMaterial>>>,
    /// Each material's pipeline key, for the warm-up.
    keys: HashMap<AssetId<ThreeMaterial>, ThreeKey>,
    /// The material × mesh-layout combinations drawn (SPEC 6.3).
    combos: Combos,
    shared: SharedImages,
    visible: Vec<bool>,
    /// Per node, a translation added to its world matrix: zero for a level;
    /// for the models scene, where every model sits at the origin, each
    /// model's place in a grid so they can be seen side by side.
    offset: Vec<Vec3>,
    out: Loaded,
    hidden: Vec<(MaterialKind, usize)>,
}

impl Build {
    pub fn new(scene: Scene, shared: SharedImages) -> Build {
        // three hides a node when it or any ancestor is invisible.
        let mut visible = vec![true; scene.nodes.len()];
        let mut stack: Vec<(u32, bool)> = scene.roots.iter().map(|&r| (r, true)).collect();
        while let Some((i, parent)) = stack.pop() {
            let n = &scene.nodes[i as usize];
            let v = parent && n.visible;
            visible[i as usize] = v;
            stack.extend(n.children.iter().map(|&c| (c, v)));
        }
        let offset = layout_models(&scene);
        let grid = scene.environment.is_none() && scene.roots.len() == 1;
        let rows = scene
            .roots
            .first()
            .map_or(0, |&r| scene.nodes[r as usize].children.len().div_ceil(8));
        Build {
            offset,
            images: Vec::new(),
            meshes: HashMap::new(),
            layouts: HashMap::new(),
            materials: HashMap::new(),
            keys: HashMap::new(),
            combos: Combos::default(),
            shared,
            visible,
            out: if grid {
                // The models' grid frames the view.
                Loaded {
                    min: Vec3::new(-3.0, 0.0, -5.0),
                    max: Vec3::new(52.0, 2.0, rows as f32 * 12.0 - 7.0),
                    fixed_bounds: true,
                    ..Loaded::default()
                }
            } else {
                Loaded {
                    min: Vec3::splat(f32::INFINITY),
                    max: Vec3::splat(f32::NEG_INFINITY),
                    ..Loaded::default()
                }
            },
            hidden: Vec::new(),
            step: Step::Textures,
            cursor: 0,
            scene,
        }
    }

    /// Fraction done, for the loading bar.
    pub fn progress(&self) -> f32 {
        let t = self.scene.textures.len().max(1) as f32;
        let n = self.scene.nodes.len().max(1) as f32;
        match self.step {
            Step::Textures => 0.3 * self.cursor as f32 / t,
            Step::Nodes => 0.3 + 0.7 * self.cursor as f32 / n,
            Step::Lights | Step::Done => 1.0,
        }
    }

    fn mesh(&mut self, key: MeshKey, assets: &mut Assets<Mesh>) -> Option<Handle<Mesh>> {
        if let Some(h) = self.meshes.get(&key) {
            return h.clone();
        }
        let h = convert::build_mesh(&self.scene, key).map(|m| {
            let layout = Layout::of(&m);
            let h = assets.add(m);
            self.layouts.insert(h.id(), layout);
            h
        });
        self.meshes.insert(key, h.clone());
        h
    }

    fn material(
        &mut self,
        index: u32,
        instance_color: bool,
        assets: &mut Assets<ThreeMaterial>,
    ) -> Option<Handle<ThreeMaterial>> {
        if let Some(h) = self.materials.get(&(index, instance_color)) {
            return h.clone();
        }
        let (scene, images, shared) = (&self.scene, &self.images, &self.shared);
        let m = &scene.materials[index as usize];
        let h = three_material(scene, m, images, shared, instance_color).map(|m| {
            let key = m.key;
            let h = assets.add(m);
            self.keys.insert(h.id(), key);
            h
        });
        self.materials.insert((index, instance_color), h.clone());
        h
    }

    /// Notes a material drawn on a mesh, for the warm-up.
    fn note_combo(&mut self, mesh: &Handle<Mesh>, material: &Handle<ThreeMaterial>, casts: bool) {
        if let (Some(layout), Some(key)) =
            (self.layouts.get(&mesh.id()), self.keys.get(&material.id()))
        {
            self.combos.note(*key, material, layout, casts);
        }
    }

    fn note_hidden(&mut self, kind: MaterialKind) {
        match self.hidden.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => self.hidden.push((kind, 1)),
        }
    }

    /// The sky dome's material: `tNoise` is its one texture.
    fn sky_material(
        &self,
        material: u32,
        assets: &mut Assets<SkyMaterial>,
    ) -> Option<Handle<SkyMaterial>> {
        let m = &self.scene.materials[material as usize];
        let noise = m.texture("tNoise")?;
        let noise = self.images.get(noise as usize).cloned().flatten()?;
        Some(assets.add(SkyMaterial {
            globals: self.shared.globals.clone(),
            noise,
        }))
    }

    /// The sky dome's noise texture, for the environment map's sky.
    pub fn sky_noise(&self) -> Option<Handle<Image>> {
        let m = self
            .scene
            .materials
            .iter()
            .find(|m| m.kind == MaterialKind::SkyDome)?;
        let i = m.texture("tNoise")?;
        self.images.get(i as usize).cloned().flatten()
    }
}

/// For a scene without an environment whose one root holds many models at
/// the origin (`models.mrscene`): a grid place for each of the root's
/// children, eight to a row, centred on its drawn nodes.
fn layout_models(scene: &Scene) -> Vec<Vec3> {
    let mut offset = vec![Vec3::ZERO; scene.nodes.len()];
    if scene.environment.is_some() || scene.roots.len() != 1 {
        return offset;
    }
    let groups = &scene.nodes[scene.roots[0] as usize].children;
    for (k, &g) in groups.iter().enumerate() {
        let mut subtree = vec![g];
        let mut at = 0;
        while at < subtree.len() {
            subtree.extend(scene.nodes[subtree[at] as usize].children.iter().copied());
            at += 1;
        }
        let pts: Vec<Vec3> = subtree
            .iter()
            .map(|&n| &scene.nodes[n as usize])
            .filter(|n| n.mesh.is_some())
            .map(|n| convert::mat4(&n.matrix_world).w_axis.truncate())
            .collect();
        let centre = pts.iter().copied().sum::<Vec3>() / pts.len().max(1) as f32;
        let cell = Vec3::new((k % 8) as f32 * 7.0, 0.0, (k / 8) as f32 * 12.0);
        let d = Vec3::new(cell.x - centre.x, 0.0, cell.z - centre.z);
        for &n in &subtree {
            offset[n as usize] = d;
        }
    }
    offset
}

/// Spawns one node's drawables. Returns the number of entities.
fn spawn_node(
    b: &mut Build,
    i: usize,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: (&mut Assets<ThreeMaterial>, &mut Assets<SkyMaterial>),
) -> usize {
    let node = b.scene.nodes[i].clone();
    let Some(mesh) = node.mesh else { return 0 };
    let Some(draw) = Draw::of(node.ty) else {
        b.out.counts.skipped_nodes += 1;
        return 0;
    };
    if !b.visible[i] {
        b.out.counts.invisible += 1;
        return 0;
    }
    let world = Mat4::from_translation(b.offset[i]) * convert::mat4(&node.matrix_world);
    let desc = b.scene.meshes[mesh as usize].clone();
    // (material, element range) per draw: one per group with multi_material.
    let parts: Vec<(u32, (u32, u32))> = if node.multi_material == Some(true) {
        desc.groups
            .iter()
            .enumerate()
            .filter_map(|(g, grp)| {
                let m = *node.materials.get(grp.material_index as usize)?;
                Some((m, convert::draw_span(&b.scene, &desc, Some(g))))
            })
            .collect()
    } else {
        node.materials
            .first()
            .map(|&m| vec![(m, convert::draw_span(&b.scene, &desc, None))])
            .unwrap_or_default()
    };
    let instances = node
        .instances
        .map(|k| b.scene.instances[k as usize].clone());
    let mut spawned = 0;
    for (mi, (start, count)) in parts {
        let mdesc = &b.scene.materials[mi as usize];
        let mode = convert::stand_in(mdesc);
        if mode == StandIn::Hidden {
            let kind = mdesc.kind;
            b.note_hidden(kind);
            continue;
        }
        let key = MeshKey {
            mesh,
            start,
            count,
            draw,
            colors: convert::vertex_colors(mdesc),
            lit: mode == StandIn::Lit,
            extra: convert::extra_attribute(mdesc),
        };
        let shadows = |e: &mut EntityCommands| {
            if !node.cast_shadow {
                e.insert(NotShadowCaster);
            }
            if !node.receive_shadow {
                e.insert(NotShadowReceiver);
            }
            if !node.frustum_culled {
                e.insert(NoFrustumCulling);
            }
        };
        if mode == StandIn::Sky {
            let Some(m) = b.mesh(key, meshes) else {
                continue;
            };
            let Some(mat) = b.sky_material(mi, materials.1) else {
                continue;
            };
            let mut e = commands.spawn((
                Mesh3d(m),
                MeshMaterial3d(mat),
                Transform::from_matrix(world),
                SceneEntity,
                SkyDome,
                NotShadowCaster,
                NotShadowReceiver,
                NoFrustumCulling,
            ));
            e.insert(Name::new("sky dome"));
            spawned += 1;
            continue;
        }
        let Some(mesh_h) = b.mesh(key, meshes) else {
            continue;
        };
        match &instances {
            None => {
                let Some(mat) = b.material(mi, false, materials.0) else {
                    continue;
                };
                b.note_combo(&mesh_h, &mat, node.cast_shadow);
                let t = Transform::from_matrix(world);
                if !b.out.fixed_bounds {
                    b.out.min = b.out.min.min(t.translation);
                    b.out.max = b.out.max.max(t.translation);
                }
                let mut e = commands.spawn((Mesh3d(mesh_h), MeshMaterial3d(mat), t, SceneEntity));
                shadows(&mut e);
                spawned += 1;
            }
            Some(inst) => {
                // Gather first (the scene is borrowed), then spawn.
                let list: Vec<(Mat4, Option<[f32; 3]>)> = {
                    let scene = &b.scene;
                    let Some(mats) = scene.buffers[inst.matrices as usize].data.as_f32() else {
                        continue;
                    };
                    let colors = inst.colors.map(|c| &scene.buffers[c as usize].data);
                    (0..inst.count as usize)
                        .filter_map(|k| {
                            let local = Mat4::from_cols_slice(mats.get(k * 16..k * 16 + 16)?);
                            // Zero-scale instances (tumbleweeds not yet
                            // launched) draw nothing and have no rotation.
                            if local.determinant().abs() < 1e-12 {
                                return None;
                            }
                            let tint = colors.map(|c| {
                                let v = |j: usize| c.get(k * 3 + j) as f32;
                                [v(0), v(1), v(2)]
                            });
                            Some((world * local, tint))
                        })
                        .collect()
                };
                let tinted = inst.colors.is_some();
                let Some(mat) = b.material(mi, tinted, materials.0) else {
                    continue;
                };
                if !list.is_empty() {
                    b.note_combo(&mesh_h, &mat, node.cast_shadow);
                }
                for (m, tint) in list {
                    let t = Transform::from_matrix(m);
                    let mut e = commands.spawn((
                        Mesh3d(mesh_h.clone()),
                        MeshMaterial3d(mat.clone()),
                        t,
                        SceneEntity,
                    ));
                    if let Some(c) = tint {
                        e.insert(MeshTag(pack_tint(c)));
                    }
                    shadows(&mut e);
                    spawned += 1;
                }
            }
        }
    }
    spawned
}

/// The scene's local lights: its first spot light (the desert train's) and
/// point light, as three holds them. The sun, hemisphere light and fog follow
/// the sky (`render::sky`), not the export.
fn scene_lights(b: &Build, lighting: &mut Lighting) {
    lighting.spot = None;
    lighting.point = None;
    for l in &b.scene.lights {
        let node = &b.scene.nodes[l.node as usize];
        if !b.visible[l.node as usize] {
            continue;
        }
        let p = convert::mat4(&node.matrix_world)
            .w_axis
            .truncate()
            .as_dvec3();
        match l.ty {
            NodeType::SpotLight if lighting.spot.is_none() => {
                let t = l
                    .target
                    .map_or(p - DVec3::Y, |t| DVec3::new(t[0], t[1], t[2]));
                lighting.spot = Some(Spot {
                    color: l.color,
                    intensity: l.intensity,
                    position: p,
                    target: t,
                    distance: l.distance.unwrap_or(0.0),
                    decay: l.decay.unwrap_or(2.0),
                    angle: l.angle.unwrap_or(std::f64::consts::FRAC_PI_3),
                    penumbra: l.penumbra.unwrap_or(0.0),
                });
            }
            NodeType::PointLight if lighting.point.is_none() => {
                lighting.point = Some(Point {
                    color: l.color,
                    intensity: l.intensity,
                    position: p,
                    distance: l.distance.unwrap_or(0.0),
                    decay: l.decay.unwrap_or(2.0),
                });
            }
            _ => {}
        }
    }
}

/// The client's state machine (WP 2.1).
#[derive(States, Default, Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum AppState {
    /// Waiting for the scene file (download natively or by the page).
    #[default]
    Waiting,
    /// Turning the scene into Bevy assets, a slice per frame.
    Building,
    /// The scene is up; the camera flies.
    Running,
    /// Something went wrong; the message is in `Status`.
    Failed,
}

/// Per-frame build budget: long enough to finish big scenes in a few
/// seconds, short enough for the page to repaint its bar.
const BUDGET_MS: u128 = 60;

#[allow(clippy::too_many_arguments)]
pub fn build_step(
    mut commands: Commands,
    build: Option<ResMut<Build>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ThreeMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut status: ResMut<Status>,
    mut next: ResMut<NextState<AppState>>,
    mut lighting: ResMut<Lighting>,
    mut env: ResMut<EnvRequest>,
) {
    let Some(mut build) = build else { return };
    let b = &mut *build;
    let t0 = Instant::now();
    while t0.elapsed().as_millis() < BUDGET_MS {
        match b.step {
            Step::Textures => {
                if let Some(t) = b.scene.textures.get(b.cursor) {
                    let img = convert::build_image(&b.scene, t).map(|i| images.add(i));
                    b.images.push(img);
                    b.cursor += 1;
                } else {
                    b.step = Step::Nodes;
                    b.cursor = 0;
                }
            }
            Step::Nodes => {
                if b.cursor < b.scene.nodes.len() {
                    let i = b.cursor;
                    b.out.counts.entities += spawn_node(
                        b,
                        i,
                        &mut commands,
                        &mut meshes,
                        (&mut materials, &mut sky_materials),
                    );
                    b.cursor += 1;
                } else {
                    b.step = Step::Lights;
                }
            }
            Step::Lights => {
                scene_lights(b, &mut lighting);
                // A scene without a dome (the models) still gets the sky's
                // noise for its environment: `terrainDetailTexture`, made by
                // the ported generator.
                env.noise = b
                    .sky_noise()
                    .or_else(|| crate::render::sky::noise_image().map(|i| images.add(i)));
                b.step = Step::Done;
            }
            Step::Done => break,
        }
    }
    status.progress = b.progress();
    if matches!(b.step, Step::Done) {
        let c = &mut b.out.counts;
        c.meshes = b.meshes.values().filter(|m| m.is_some()).count();
        c.materials = b.materials.values().filter(|m| m.is_some()).count();
        c.images = b.images.iter().filter(|m| m.is_some()).count();
        c.hidden_kinds = std::mem::take(&mut b.hidden);
        b.out.camera = b.scene.environment.as_ref().map(|e| {
            let p = e.camera.position;
            let q = e.camera.quaternion;
            Transform::from_xyz(p[0] as f32, p[1] as f32, p[2] as f32).with_rotation(
                Quat::from_xyzw(q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32),
            )
        });
        let loaded = b.out.clone();
        info!(
            "scene built: {} entities, {} meshes, {} materials, {} images; not drawn yet: {:?}, {} other nodes, {} invisible",
            loaded.counts.entities,
            loaded.counts.meshes,
            loaded.counts.materials,
            loaded.counts.images,
            loaded.counts.hidden_kinds,
            loaded.counts.skipped_nodes,
            loaded.counts.invisible
        );
        status.counts = Some(loaded.counts.clone());
        commands.insert_resource(loaded);
        let entries = b
            .scene
            .night_params
            .iter()
            .filter(|p| p.prop == "emissiveIntensity")
            .map(|p| NightEntry {
                materials: [false, true]
                    .iter()
                    .filter_map(|&t| b.materials.get(&(p.material, t)).cloned().flatten())
                    .collect(),
                emissive: b.scene.materials[p.material as usize]
                    .color("emissive")
                    .unwrap_or([0.0; 3]),
                day: p.day,
                night: p.night,
            })
            .collect();
        commands.insert_resource(NightMaterials {
            entries,
            last: None,
        });
        // The warm-up (SPEC 6.3): one off-screen stand-in per material ×
        // mesh-layout combination, until every pipeline has compiled.
        let n = b.combos.spawn(&mut commands, &mut meshes);
        info!("warm-up: {n} material × mesh-layout combinations");
        status.warm_up = n;
        // Dropping the build drops the Scene: the CPU copies go here.
        commands.remove_resource::<Build>();
        next.set(AppState::Running);
    }
}

/// Moves the sky dome with the focus (`Sky.update`: `dome.position.copy(focus)`).
pub fn follow_focus(focus: DVec3, sky: &mut Query<&mut Transform, With<SkyDome>>) {
    for mut t in sky.iter_mut() {
        t.translation = focus.as_vec3();
    }
}
