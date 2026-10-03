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
use crate::status::Status;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DVec3, Mat4, Vec3};
use bevy::mesh::MeshTag;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::platform::time::Instant;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;
use mr_scene::{MaterialKind, NodeType, Scene};
use std::collections::HashMap;

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

/// A stand-in material with three's per-instance colour: `StandardMaterial`
/// whose base colour is multiplied by a colour carried in the instance's
/// [`MeshTag`] (`tint.wgsl`). One material per JS material, so instances
/// keep batching whatever their colours.
pub type TintMaterial = ExtendedMaterial<StandardMaterial, InstanceTint>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct InstanceTint {
    /// Unused by the shader; a bind group needs a binding.
    #[uniform(100)]
    pub reserved: u32,
}

impl MaterialExtension for InstanceTint {
    fn fragment_shader() -> ShaderRef {
        "embedded://mr_game/tint.wgsl".into()
    }
}

/// Registers the tint material and its shader.
pub fn tint_plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "tint.wgsl");
    app.add_plugins(MaterialPlugin::<TintMaterial>::default());
}

/// A linear colour in 0..2 as the tag `tint.wgsl` unpacks: 10 bits a channel.
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
    /// three's pre-ACES exposure factor (`toneMappingExposure / 0.6`).
    exposure: f32,
    images: Vec<Option<Handle<Image>>>,
    meshes: HashMap<MeshKey, Option<Handle<Mesh>>>,
    materials: HashMap<u32, Option<Handle<StandardMaterial>>>,
    tinted: HashMap<u32, Option<Handle<TintMaterial>>>,
    visible: Vec<bool>,
    /// Per node, a translation added to its world matrix: zero for a level;
    /// for the models scene, where every model sits at the origin, each
    /// model's place in a grid so they can be seen side by side.
    offset: Vec<Vec3>,
    out: Loaded,
    hidden: Vec<(MaterialKind, usize)>,
}

impl Build {
    pub fn new(scene: Scene) -> Build {
        let exposure = scene
            .environment
            .as_ref()
            .map_or(1.0, |e| e.tone_mapping_exposure) as f32
            / 0.6;
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
            exposure,
            images: Vec::new(),
            meshes: HashMap::new(),
            materials: HashMap::new(),
            tinted: HashMap::new(),
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
        let scene = &self.scene;
        self.meshes
            .entry(key)
            .or_insert_with(|| convert::build_mesh(scene, key).map(|m| assets.add(m)))
            .clone()
    }

    fn material(
        &mut self,
        index: u32,
        assets: &mut Assets<StandardMaterial>,
    ) -> Option<Handle<StandardMaterial>> {
        let (scene, exposure, images) = (&self.scene, self.exposure, &self.images);
        self.materials
            .entry(index)
            .or_insert_with(|| {
                let m = &scene.materials[index as usize];
                convert::build_material(scene, m, None, exposure, images).map(|m| assets.add(m))
            })
            .clone()
    }

    fn tinted_material(
        &mut self,
        index: u32,
        assets: &mut Assets<TintMaterial>,
    ) -> Option<Handle<TintMaterial>> {
        let (scene, exposure, images) = (&self.scene, self.exposure, &self.images);
        self.tinted
            .entry(index)
            .or_insert_with(|| {
                let m = &scene.materials[index as usize];
                convert::build_material(scene, m, None, exposure, images).map(|base| {
                    assets.add(TintMaterial {
                        base,
                        extension: InstanceTint::default(),
                    })
                })
            })
            .clone()
    }

    fn note_hidden(&mut self, kind: MaterialKind) {
        match self.hidden.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => self.hidden.push((kind, 1)),
        }
    }

    /// The sky dome's mesh: its sphere with the stand-in sky colours baked
    /// into vertex colours (times the exposure: it is unlit).
    fn sky_mesh(&self, mesh: u32, material: u32) -> Option<Mesh> {
        let key = MeshKey {
            mesh,
            start: 0,
            count: u32::MAX,
            draw: Draw::Triangles,
            colors: false,
            lit: false,
        };
        let m = &self.scene.meshes[mesh as usize];
        let (start, count) = convert::draw_span(&self.scene, m, None);
        let mut out = convert::build_mesh(
            &self.scene,
            MeshKey {
                start,
                count,
                ..key
            },
        )?;
        let mat = &self.scene.materials[material as usize];
        let pos = match out.attribute(Mesh::ATTRIBUTE_POSITION)? {
            bevy::mesh::VertexAttributeValues::Float32x3(p) => p.clone(),
            _ => return None,
        };
        let k = self.exposure;
        let colors: Vec<[f32; 4]> = pos
            .iter()
            .map(|p| {
                let c = convert::sky_color(mat, *p);
                [c[0] * k, c[1] * k, c[2] * k, 1.0]
            })
            .collect();
        out.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        Some(out)
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
    materials: (&mut Assets<StandardMaterial>, &mut Assets<TintMaterial>),
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
            let Some(m) = b.sky_mesh(mesh, mi) else {
                continue;
            };
            let Some(mat) = b.material(mi, materials.0) else {
                continue;
            };
            let mut e = commands.spawn((
                Mesh3d(meshes.add(m)),
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
                let Some(mat) = b.material(mi, materials.0) else {
                    continue;
                };
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
                for (m, tint) in list {
                    let t = Transform::from_matrix(m);
                    let mut e = commands.spawn((Mesh3d(mesh_h.clone()), t, SceneEntity));
                    match tint {
                        Some(c) => {
                            let Some(mat) = b.tinted_material(mi, materials.1) else {
                                continue;
                            };
                            e.insert((MeshMaterial3d(mat), MeshTag(pack_tint(c))));
                        }
                        None => {
                            let Some(mat) = b.material(mi, materials.0) else {
                                continue;
                            };
                            e.insert(MeshMaterial3d(mat));
                        }
                    }
                    shadows(&mut e);
                    spawned += 1;
                }
            }
        }
    }
    spawned
}

/// Spawns the scene's lights. The hemisphere light becomes the camera's
/// ambient light (Bevy has no hemisphere light); returns its colour times
/// brightness.
fn spawn_lights(b: &Build, commands: &mut Commands, hq: bool) -> Option<LinearRgba> {
    let mut ambient = None;
    let pi = std::f32::consts::PI;
    if b.scene.lights.is_empty() {
        // The models scene has no lights of its own: a sun from the front
        // left, high, at a level's usual intensity.
        commands.spawn((
            DirectionalLight {
                illuminance: 3.0,
                shadow_maps_enabled: hq,
                ..default()
            },
            Transform::from_xyz(-30.0, 60.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
            SceneEntity,
        ));
    }
    for l in &b.scene.lights {
        let node = &b.scene.nodes[l.node as usize];
        let pos = convert::mat4(&node.matrix_world).w_axis.truncate();
        let color = Color::linear_rgb(l.color[0] as f32, l.color[1] as f32, l.color[2] as f32);
        let target = l
            .target
            .map(|t| Vec3::new(t[0] as f32, t[1] as f32, t[2] as f32));
        match l.ty {
            NodeType::DirectionalLight => {
                let to = target.unwrap_or(Vec3::ZERO);
                let up = if (to - pos).normalize_or_zero().y.abs() > 0.999 {
                    Vec3::Z
                } else {
                    Vec3::Y
                };
                commands.spawn((
                    DirectionalLight {
                        color,
                        // three: radiance = colour × intensity, diffuse
                        // = albedo/π × that; Bevy's illuminance is the same
                        // quantity with Bevy's 1/π in its diffuse.
                        illuminance: l.intensity as f32,
                        shadow_maps_enabled: hq && l.cast_shadow,
                        ..default()
                    },
                    // One cascade over about the JS's ±70 m box (SPEC 6.1).
                    bevy::light::CascadeShadowConfigBuilder {
                        num_cascades: 1,
                        minimum_distance: 0.3,
                        maximum_distance: 140.0,
                        first_cascade_far_bound: 140.0,
                        overlap_proportion: 0.2,
                    }
                    .build(),
                    Transform::from_translation(pos).looking_at(to, up),
                    SceneEntity,
                ));
            }
            NodeType::HemisphereLight => {
                // The sky colour from above, the ground colour from below; an
                // ambient light averages the two, weighted to the sky as most
                // of what is seen faces up. three's diffuse divides by π,
                // Bevy's ambient does not.
                let g = l.ground_color.unwrap_or([0.0; 3]);
                let k = l.intensity as f32 / pi;
                let mix = |i: usize| (l.color[i] as f32 * 0.75 + g[i] as f32 * 0.25) * k;
                ambient = Some(LinearRgba::rgb(mix(0), mix(1), mix(2)));
            }
            NodeType::SpotLight => {
                let angle = l.angle.unwrap_or(std::f64::consts::FRAC_PI_3) as f32;
                let pen = l.penumbra.unwrap_or(0.0) as f32;
                commands.spawn((
                    SpotLight {
                        color,
                        // three's candela to Bevy's lumens.
                        intensity: l.intensity as f32 * 4.0 * pi,
                        range: l.distance.filter(|d| *d > 0.0).unwrap_or(1000.0) as f32,
                        outer_angle: angle,
                        inner_angle: angle * (1.0 - pen),
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(pos)
                        .looking_at(target.unwrap_or(pos - Vec3::Y), Vec3::Y),
                    SceneEntity,
                ));
            }
            NodeType::PointLight => {
                commands.spawn((
                    PointLight {
                        color,
                        intensity: l.intensity as f32 * 4.0 * pi,
                        range: l.distance.filter(|d| *d > 0.0).unwrap_or(1000.0) as f32,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_translation(pos),
                    SceneEntity,
                ));
            }
            _ => {}
        }
    }
    ambient
}

/// The camera settings a scene asks for: fog, clear colour, exposure and
/// ambient light (the scene's environment, or neutral ones for models).
pub fn apply_environment(
    b: &Build,
    ambient: Option<LinearRgba>,
    camera: &mut EntityCommands,
    clear: &mut ClearColor,
) {
    let k = b.exposure;
    // Bevy's exposure scales lit colour by 2^-ev100 / 1.2; three's ACES
    // scales it by exposure / 0.6 before the same fit.
    let ev100 = -(1.2 * k).log2();
    camera.insert(bevy::camera::Exposure { ev100 });
    // Without the sky's environment map (WP 2.3) three's image-based light is
    // missing; the stand-in adds the fog colour, which is the sky near the
    // horizon, at the environment intensity.
    let env = b.scene.environment.as_ref();
    let fog_c = env
        .and_then(|e| e.fog.as_ref())
        .map(|f| f.color.map(|x| x as f32));
    let env_k = env.map_or(0.0, |e| e.environment_intensity) as f32;
    let amb = ambient.unwrap_or(LinearRgba::rgb(0.25, 0.25, 0.27));
    let amb = match fog_c {
        Some(f) => LinearRgba::rgb(
            amb.red + f[0] * env_k * 0.5,
            amb.green + f[1] * env_k * 0.5,
            amb.blue + f[2] * env_k * 0.5,
        ),
        None => amb,
    };
    camera.insert(AmbientLight {
        color: Color::LinearRgba(amb),
        brightness: 1.0,
        ..default()
    });
    if let Some(f) = env.and_then(|e| e.fog.as_ref()) {
        let c = f.color.map(|x| x as f32 * k);
        let falloff = match (f.ty.as_str(), f.density) {
            ("FogExp2", Some(d)) => bevy::pbr::FogFalloff::ExponentialSquared { density: d as f32 },
            _ => bevy::pbr::FogFalloff::Linear {
                start: f.near.unwrap_or(1.0) as f32,
                end: f.far.unwrap_or(1000.0) as f32,
            },
        };
        // The JS fog picks up the sun's colour toward the sun (Sky.js's fog
        // patch: fogSun⁴ × 0.035 + fogSun²⁴ × 0.07); Bevy's directional
        // scattering is one power term, close enough for a stand-in.
        camera.insert(bevy::pbr::DistanceFog {
            color: Color::linear_rgb(c[0], c[1], c[2]),
            directional_light_color: Color::linear_rgba(0.05, 0.05, 0.05, 1.0),
            directional_light_exponent: 6.0,
            falloff,
        });
        *clear = ClearColor(Color::linear_rgb(c[0], c[1], c[2]));
    } else {
        *clear = ClearColor(Color::linear_rgb(0.08, 0.09, 0.11));
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
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut tinted: ResMut<Assets<TintMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut status: ResMut<Status>,
    mut next: ResMut<NextState<AppState>>,
    mut clear: ResMut<ClearColor>,
    camera: Query<Entity, With<Camera3d>>,
    opts: Res<crate::Opts>,
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
                        (&mut materials, &mut tinted),
                    );
                    b.cursor += 1;
                } else {
                    b.step = Step::Lights;
                }
            }
            Step::Lights => {
                let ambient = spawn_lights(b, &mut commands, opts.hq);
                if let Ok(cam) = camera.single() {
                    apply_environment(b, ambient, &mut commands.entity(cam), &mut clear);
                }
                b.step = Step::Done;
            }
            Step::Done => break,
        }
    }
    status.progress = b.progress();
    if matches!(b.step, Step::Done) {
        let c = &mut b.out.counts;
        c.meshes = b.meshes.values().filter(|m| m.is_some()).count();
        c.materials = b.materials.values().filter(|m| m.is_some()).count()
            + b.tinted.values().filter(|m| m.is_some()).count();
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
