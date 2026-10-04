//! The scene loader (roadmap WP 2.2): a `.mrscene` becomes Bevy meshes,
//! materials, images, lights and entities, a slice per frame so the page
//! keeps drawing its progress bar. When it is done the `Scene` (every CPU
//! copy of the data) is dropped; Bevy keeps only the GPU copies.
//!
//! Transforms are the export's world matrices (`matrix_world`), so the node
//! tree is flattened: nothing in a scene moves yet (animators arrive with
//! world generation). An `InstancedMesh` becomes one entity (per material
//! group) with its instances in a vertex buffer, drawn as three draws it:
//! one instanced draw (`render::instancing`, DECISIONS D450).

use crate::convert::{self, Draw, MeshKey, StandIn};
use crate::render::instancing::{self, InstanceStream, Instances};
use crate::render::lighting::{Lighting, Point, Spot};
use crate::render::material::{ThreeKey, three_material};
use crate::render::pmrem::EnvRequest;
use crate::render::{SharedImages, SkyMaterial, ThreeMaterial};
use crate::status::Status;
use crate::warmup::{Combos, Layout};
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::{NoAutoAabb, NoFrustumCulling};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DMat4, DVec3, Mat4, Vec3, Vec3A, Vec4};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::batching::NoAutomaticBatching;
use mr_scene::{MaterialKind, NodeType, Scene};
use std::collections::HashMap;
use std::sync::Arc;

/// Everything spawned from a scene, for teardown.
#[derive(Component)]
pub struct SceneEntity;

/// The sky dome, which follows the focus as `Sky.update` moves it.
#[derive(Component)]
pub struct SkyDome;

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
    /// Each mesh's bounding-box centre as Bevy computes it (the point
    /// Bevy sorts a transparent mesh by).
    centres: HashMap<AssetId<Mesh>, Vec3>,
    /// Per JS material, whether its instances carry colours, and whether
    /// it draws an InstancedMesh.
    materials: HashMap<(u32, bool, bool), Option<Handle<ThreeMaterial>>>,
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
    /// The entity every spawned entity goes under (a menu section's root,
    /// `crate::preview`); none for a level, whose entities have no parent.
    parent: Option<Entity>,
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
            centres: HashMap::new(),
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
            parent: None,
            step: Step::Textures,
            cursor: 0,
            scene,
        }
    }

    /// Spawns the scene under `parent` (a menu section's root, D742).
    pub fn under(mut self, parent: Entity) -> Build {
        self.parent = Some(parent);
        self
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
            let centre = m.compute_aabb().map_or(Vec3::ZERO, |b| b.center.into());
            let h = assets.add(m);
            self.layouts.insert(h.id(), layout);
            self.centres.insert(h.id(), centre);
            h
        });
        self.meshes.insert(key, h.clone());
        h
    }

    fn material(
        &mut self,
        index: u32,
        instance_color: bool,
        instanced: bool,
        assets: &mut Assets<ThreeMaterial>,
    ) -> Option<Handle<ThreeMaterial>> {
        let cache_key = (index, instance_color, instanced);
        if let Some(h) = self.materials.get(&cache_key) {
            return h.clone();
        }
        let (scene, images, shared) = (&self.scene, &self.images, &self.shared);
        let m = &scene.materials[index as usize];
        // `world.nightMaterials` (`World.js` `addNight`): every one in the
        // exports is an `emissiveIntensity`, `day + (night - day) × n`;
        // the shader follows the sky's night factor (D455).
        let night = scene
            .night_params
            .iter()
            .find(|p| p.material == index && p.prop == "emissiveIntensity");
        let emissive = m.color("emissive").unwrap_or([0.0; 3]);
        let h = three_material(scene, m, images, shared, instance_color).map(|mut m| {
            m.key.instanced = instanced;
            // Its index, for its animation block (`animate`, D490).
            m.params.slots.z = index as f32 + 1.0;
            if let Some(p) = night {
                let w = m.params.emissive.w;
                m.params.emissive = Vec4::new(
                    emissive[0] as f32,
                    emissive[1] as f32,
                    emissive[2] as f32,
                    w,
                );
                m.params.night = Vec4::new(p.day as f32, p.night as f32, 1.0, 0.0);
            }
            let key = m.key;
            let h = assets.add(m);
            self.keys.insert(h.id(), key);
            h
        });
        self.materials.insert(cache_key, h.clone());
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
    // A node exported invisible (or under one) is spawned hidden, as three
    // keeps it: an animator may show it (City's cut-off on the loop, D498).
    let hidden = !b.visible[i];
    if hidden {
        b.out.counts.invisible += 1;
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
    // An InstancedMesh's stream and world bounding sphere, made at its first
    // drawn part and shared by its material groups.
    let mut stream: Option<Option<Stream>> = None;
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
            if let Some(p) = b.parent {
                e.insert(ChildOf(p));
            }
            spawned += 1;
            continue;
        }
        let Some(mesh_h) = b.mesh(key, meshes) else {
            continue;
        };
        match &instances {
            None => {
                let Some(mat) = b.material(mi, false, false, materials.0) else {
                    continue;
                };
                b.note_combo(&mesh_h, &mat, node.cast_shadow);
                let t = Transform::from_matrix(world);
                if !b.out.fixed_bounds {
                    b.out.min = b.out.min.min(t.translation);
                    b.out.max = b.out.max.max(t.translation);
                }
                let mut e = commands.spawn((
                    Mesh3d(mesh_h),
                    MeshMaterial3d(mat),
                    t,
                    SceneEntity,
                    crate::animate::NodeRef(i as u32),
                ));
                shadows(&mut e);
                if hidden {
                    e.insert(Visibility::Hidden);
                }
                if let Some(p) = b.parent {
                    e.insert(ChildOf(p));
                }
                spawned += 1;
            }
            Some(inst) => {
                let made = stream.get_or_insert_with(|| {
                    instance_stream(
                        &b.scene,
                        inst,
                        b.offset[i],
                        &node.matrix_world,
                        node.receive_shadow,
                    )
                });
                let Some((stream, sphere)) = made.clone() else {
                    continue;
                };
                let Some(mat) = b.material(mi, inst.colors.is_some(), true, materials.0) else {
                    continue;
                };
                b.note_combo(&mesh_h, &mat, node.cast_shadow);
                let mut e = commands.spawn((
                    Mesh3d(mesh_h.clone()),
                    MeshMaterial3d(mat),
                    stream,
                    NoAutomaticBatching,
                    SceneEntity,
                    crate::animate::NodeRef(i as u32),
                ));
                match sphere {
                    // three culls the InstancedMesh as a whole by its
                    // bounding sphere (`Frustum.intersectsObject`). The
                    // instanced shader ignores the entity's transform, so it
                    // places the box around that sphere, centred where Bevy
                    // takes the mesh's centre for the transparent sort.
                    Some((centre, radius)) if node.frustum_culled => {
                        let c = b.centres.get(&mesh_h.id()).copied().unwrap_or(Vec3::ZERO);
                        e.insert((
                            Transform::from_translation(centre - c),
                            Aabb {
                                center: c.into(),
                                half_extents: Vec3A::splat(radius),
                            },
                            NoAutoAabb,
                        ));
                    }
                    _ => {
                        e.insert((Transform::IDENTITY, NoFrustumCulling));
                    }
                }
                shadows(&mut e);
                if hidden {
                    e.insert(Visibility::Hidden);
                }
                if let Some(p) = b.parent {
                    e.insert(ChildOf(p));
                }
                spawned += 1;
            }
        }
    }
    spawned
}

/// An InstancedMesh's stream, and its bounding sphere in world space (centre,
/// radius) when the export has one.
type Stream = (Instances, Option<(Vec3, f32)>);

/// An InstancedMesh's instance stream: each instance's world matrix (the
/// node's, with the models' grid offset, × `instanceMatrix`, in f64), its
/// `instanceColor` (white without) and the node's `receiveShadow`; and
/// three's bounding sphere over all the instances, in world space. Zero-scale
/// instances (the tumbleweeds not yet launched) draw nothing in three and
/// are left out. None when no instance is drawn.
fn instance_stream(
    scene: &Scene,
    inst: &mr_scene::InstanceDesc,
    offset: Vec3,
    matrix_world: &[f64; 16],
    receive: bool,
) -> Option<Stream> {
    let world = DMat4::from_translation(offset.as_dvec3()) * DMat4::from_cols_array(matrix_world);
    let mats = scene.buffers[inst.matrices as usize].data.as_f32()?;
    let colors = inst.colors.map(|c| &scene.buffers[c as usize].data);
    // The geometry's instance-rate attributes (the harbour containers'
    // `aVar`, the desert pools' `ph` and `fl`), in the stream (D499).
    let extras = instance_extras(scene, inst);
    let mut data = Vec::with_capacity(inst.count as usize * instancing::INSTANCE_FLOATS);
    for k in 0..inst.count as usize {
        let Some(cols) = mats.get(k * 16..k * 16 + 16) else {
            break;
        };
        if Mat4::from_cols_slice(cols).determinant().abs() < 1e-12 {
            continue;
        }
        let local = DMat4::from_cols_array(&std::array::from_fn(|j| f64::from(cols[j])));
        let tint = colors.map_or([1.0; 3], |c| {
            let v = |j: usize| c.get(k * 3 + j) as f32;
            [v(0), v(1), v(2)]
        });
        instancing::push_instance(&mut data, &(world * local), tint, receive);
        if let Some(x) = &extras {
            instancing::set_instance_extra(&mut data, x.get(k).copied().unwrap_or([0.0; 4]));
        }
    }
    if data.is_empty() {
        return None;
    }
    let sphere = inst
        .bounding_sphere
        .as_ref()
        .filter(|s| s.len() >= 4)
        .map(|s| {
            let (c, r) = instancing::sphere_to_world(&world, DVec3::new(s[0], s[1], s[2]), s[3]);
            (c.as_vec3(), r as f32)
        });
    Some((Instances(Arc::new(InstanceStream::new(&data))), sphere))
}

/// An InstancedMesh's instance-rate attributes, per instance: the first
/// component of each, in the geometry's order, up to four (none if the
/// geometry has none).
pub fn instance_extras(scene: &Scene, inst: &mr_scene::InstanceDesc) -> Option<Vec<[f32; 4]>> {
    let node = scene.nodes.get(inst.node as usize)?;
    let mesh = scene.meshes.get(node.mesh? as usize)?;
    let attrs: Vec<&mr_scene::Buffer> = mesh
        .attributes
        .iter()
        .filter(|a| a.instanced)
        .take(4)
        .map(|a| &scene.buffers[a.accessor as usize])
        .collect();
    if attrs.is_empty() {
        return None;
    }
    let n = attrs.iter().map(|b| b.count()).max().unwrap_or(0);
    Some(
        (0..n)
            .map(|i| {
                let mut x = [0f32; 4];
                for (j, b) in attrs.iter().enumerate() {
                    if i < b.count() {
                        x[j] = b.data.get(i * b.item_size as usize) as f32;
                    }
                }
                x
            })
            .collect(),
    )
}

/// The scene's local lights: its first spot light (the desert train's) and
/// point light, as three holds them. The sun, hemisphere light and fog follow
/// the sky (`render::sky`), not the export.
fn scene_lights(b: &Build, lighting: &mut Lighting) {
    (lighting.spot, lighting.point) = lights_of(b);
}

/// The scene's first spot light and point light.
fn lights_of(b: &Build) -> (Option<Spot>, Option<Point>) {
    let mut lighting = (None::<Spot>, None::<Point>);
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
            NodeType::SpotLight if lighting.0.is_none() => {
                let t = l
                    .target
                    .map_or(p - DVec3::Y, |t| DVec3::new(t[0], t[1], t[2]));
                lighting.0 = Some(Spot {
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
            NodeType::PointLight if lighting.1.is_none() => {
                lighting.1 = Some(Point {
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
    lighting
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
        // The warm-up (SPEC 6.3): one off-screen stand-in per material ×
        // mesh-layout combination, until every pipeline has compiled.
        let n = b.combos.spawn(&mut commands, &mut meshes);
        info!("warm-up: {n} material × mesh-layout combinations");
        status.warm_up = n;
        // What the scenery's animators address (`crate::animate`).
        commands.insert_resource(crate::animate::SceneIndex::new(
            &b.scene, &b.meshes, &b.offset,
        ));
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

// ── Menu sections (D742) ────────────────────────────────────────────────

/// What a menu section's scene leaves behind once spawned
/// (`crate::preview`): what a level's build puts in resources, kept by the
/// section until it is shown.
pub struct SectionScene {
    pub loaded: Loaded,
    pub index: crate::animate::SceneIndex,
    pub spot: Option<Spot>,
    pub point: Option<Point>,
    pub noise: Option<Handle<Image>>,
    /// The warm-up's stand-ins (SPEC 6.3), spawned by [`finish_section`].
    pub warm_up: Vec<Entity>,
}

/// One slice of a section's build: its textures, then its nodes, for at
/// most `budget_ms`. True once everything is spawned.
#[allow(clippy::too_many_arguments)]
pub fn step_section(
    b: &mut Build,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<ThreeMaterial>,
    sky_materials: &mut Assets<SkyMaterial>,
    images: &mut Assets<Image>,
    budget_ms: u128,
) -> bool {
    let t0 = Instant::now();
    while t0.elapsed().as_millis() < budget_ms {
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
                    b.out.counts.entities +=
                        spawn_node(b, i, commands, meshes, (materials, sky_materials));
                    b.cursor += 1;
                } else {
                    b.step = Step::Lights;
                }
            }
            Step::Lights | Step::Done => return true,
        }
    }
    matches!(b.step, Step::Lights | Step::Done)
}

/// The end of a section's build, as [`build_step`]'s for a level: the
/// counts, the warm-up's stand-ins, the animators' index, the local lights
/// and the sky's noise; the scene's CPU copy is dropped with the build.
pub fn finish_section(
    mut b: Build,
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
) -> SectionScene {
    let c = &mut b.out.counts;
    c.meshes = b.meshes.values().filter(|m| m.is_some()).count();
    c.materials = b.materials.values().filter(|m| m.is_some()).count();
    c.images = b.images.iter().filter(|m| m.is_some()).count();
    c.hidden_kinds = std::mem::take(&mut b.hidden);
    let (spot, point) = lights_of(&b);
    let noise = b
        .sky_noise()
        .or_else(|| crate::render::sky::noise_image().map(|i| images.add(i)));
    let warm_up = b.combos.spawn_each(commands, meshes);
    let index = crate::animate::SceneIndex::new(&b.scene, &b.meshes, &b.offset);
    SectionScene {
        loaded: b.out.clone(),
        index,
        spot,
        point,
        noise,
        warm_up,
    }
}
