//! The race's cars as `CarModel.js` builds them (WP 4.1's
//! `mr_worldgen::car_model`): every car of the field built into one scene
//! graph (so they share the kit's materials and geometry as the JS page
//! does), assembled into an `mr_scene::Scene` and spawned as an entity tree
//! per car with the nodes' local transforms, so `Vehicle.sync` can move the
//! root, tip the body, spin the wheels and turn the steer pivots, and the
//! light setters' edits and the traffic's far-model switch
//! (`Traffic.farLod`) reach what is drawn.
//!
//! Every material × mesh-layout combination the field can draw (the
//! hidden far models and lights included) joins the pipeline warm-up when
//! the cars are built (D458), so a race compiles nothing after `ready`.

use crate::convert::{self, Draw, MeshKey, StandIn};
use crate::loader::SceneEntity;
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::{ThreeKey, ThreeMaterial, three_material};
use crate::warmup::{Combos, Layout};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DQuat, EulerRot, Vec4};
use bevy::prelude::*;
use mr_scene::Scene;
use mr_worldgen::car_model::{BuildOpts, Lod, VehicleModel, build_vehicle};
use mr_worldgen::object::{HandleMap, SceneGraph};
use mr_worldgen::textures::TextureCache;
use mr_worldgen::world::{Change, Edit, Handle as WHandle};
use std::collections::HashMap;

/// How a slot's car is built (`buildVehicle(kind, { color, seed, lod, far })`).
#[derive(Clone, Copy, Debug)]
pub struct Want {
    pub kind: &'static str,
    pub color: u32,
    pub seed: u32,
    pub lod: Lod,
    pub far: bool,
    /// Race.js casts shadows from every mesh of a racer.
    pub racer: bool,
}

/// A wheel: its node's entity, its rest transform, radius and spin.
struct Wheel {
    e: Entity,
    base: Transform,
    radius: f64,
    spin: f64,
}

/// One drawn car.
pub struct Car {
    pub root: Entity,
    pub model: VehicleModel,
    /// `headlightAnchor`'s entity (the player's headlight spot hangs on it).
    pub headlight: Option<Entity>,
    body: Option<(Entity, Transform)>,
    wheels: Vec<Wheel>,
    pivots: Vec<(Entity, Transform)>,
}

/// Every car of the race, drawn.
pub struct Cars {
    pub cars: Vec<Car>,
    handles: HandleMap,
    /// Scene node index → its entity.
    nodes: Vec<Option<Entity>>,
    /// Scene material index → the materials drawn for it and its emissive
    /// colour (three's emissive uniform is colour × intensity).
    materials: Vec<(Vec<Handle<ThreeMaterial>>, [f64; 3])>,
    /// The last emissive intensity set per scene material.
    last: HashMap<u32, f64>,
    /// Scene material index → its slot in the globals' material lights
    /// (D456), once a setter has touched it.
    slots: HashMap<u32, usize>,
}

/// Builds the field's models and spawns them, hidden until placed.
pub fn spawn(
    wants: &[Want],
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut Assets<ThreeMaterial>,
    shared: &SharedImages,
) -> Cars {
    let mut graph = SceneGraph::new();
    let mut textures = TextureCache::new();
    let mut models = Vec::new();
    for w in wants {
        let opts = BuildOpts {
            lod: Some(w.lod),
            far: w.far,
            color: Some(w.color),
            seed: w.seed,
            ..BuildOpts::default()
        };
        let m = build_vehicle(&mut graph, &mut textures, w.kind, &opts)
            .or_else(|| build_vehicle(&mut graph, &mut textures, "sedan", &opts))
            .expect("the sedan builds");
        graph.roots.push(m.root);
        models.push(m);
    }
    let (scene, handles) = graph.finish();
    let image_handles: Vec<Option<Handle<Image>>> = scene
        .textures
        .iter()
        .map(|t| convert::build_image(&scene, t).map(|i| images.add(i)))
        .collect();
    let mut s = Spawner {
        scene: &scene,
        images: &image_handles,
        shared,
        meshes: HashMap::new(),
        materials: vec![(Vec::new(), [0.0; 3]); scene.materials.len()],
        mat_cache: HashMap::new(),
        combos: Combos::default(),
        nodes: vec![None; scene.nodes.len()],
    };
    let mut cars = Vec::new();
    for (m, w) in models.into_iter().zip(wants) {
        let Some(ri) = handles.node(m.root) else {
            continue;
        };
        let root = s.node(ri as usize, None, commands, meshes, mats, w.racer);
        commands
            .entity(root)
            .insert((Transform::default(), Visibility::Hidden, SceneEntity));
        let ent = |id| handles.node(id).and_then(|i| s.nodes[i as usize]);
        let base = |i: u32| Transform::from_matrix(convert::mat4(&scene.nodes[i as usize].matrix));
        let body = handles
            .node(m.body)
            .and_then(|i| Some((s.nodes[i as usize]?, base(i))));
        let wr = if m.dims.wheel_radius > 0.0 {
            m.dims.wheel_radius
        } else {
            0.34
        };
        let wheels = m
            .wheels
            .iter()
            .filter_map(|&id| {
                let i = handles.node(id)?;
                let radius = scene.nodes[i as usize]
                    .user_data
                    .get("radius")
                    .and_then(|v| v.as_f64())
                    .filter(|r| *r != 0.0)
                    .unwrap_or(wr);
                Some(Wheel {
                    e: ent(id)?,
                    base: base(i),
                    radius,
                    spin: 0.0,
                })
            })
            .collect();
        let pivots = m
            .steer_pivots
            .iter()
            .filter_map(|&id| Some((ent(id)?, base(handles.node(id)?))))
            .collect();
        let headlight = ent(m.headlight_anchor);
        cars.push(Car {
            root,
            headlight,
            model: m,
            body,
            wheels,
            pivots,
        });
    }
    // D458: the field's combinations join the warm-up (`warmup`): one
    // off-screen stand-in each, despawned once every pipeline is ready.
    let n = s.combos.spawn(commands, meshes);
    // D459: and the HUD's glyphs.
    crate::warmup::spawn_glyphs(commands);
    info!("race cars: {n} material × mesh-layout combinations warmed up");
    Cars {
        cars,
        materials: s.materials,
        nodes: s.nodes,
        handles,
        last: HashMap::new(),
        slots: HashMap::new(),
    }
}

struct Spawner<'a> {
    scene: &'a Scene,
    images: &'a [Option<Handle<Image>>],
    shared: &'a SharedImages,
    /// Each mesh and its vertex layout (for the warm-up).
    meshes: HashMap<MeshKey, Option<(Handle<Mesh>, Layout)>>,
    materials: Vec<(Vec<Handle<ThreeMaterial>>, [f64; 3])>,
    mat_cache: HashMap<u32, Option<(Handle<ThreeMaterial>, ThreeKey)>>,
    /// The combinations the field draws (`warmup`).
    combos: Combos,
    nodes: Vec<Option<Entity>>,
}

impl Spawner<'_> {
    fn material(
        &mut self,
        i: u32,
        assets: &mut Assets<ThreeMaterial>,
    ) -> Option<(Handle<ThreeMaterial>, ThreeKey)> {
        if let Some(h) = self.mat_cache.get(&i) {
            return h.clone();
        }
        let m = &self.scene.materials[i as usize];
        let h = three_material(self.scene, m, self.images, self.shared, false).map(|t| {
            let key = t.key;
            (assets.add(t), key)
        });
        if let Some((h, _)) = &h {
            let e = m.color("emissive").unwrap_or([0.0; 3]);
            self.materials[i as usize] = (vec![h.clone()], e);
        }
        self.mat_cache.insert(i, h.clone());
        h
    }

    /// The node `i` and its subtree as entities with their local
    /// transforms; returns the node's entity.
    fn node(
        &mut self,
        i: usize,
        parent: Option<Entity>,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        mats: &mut Assets<ThreeMaterial>,
        racer: bool,
    ) -> Entity {
        let n = &self.scene.nodes[i];
        let mut e = commands.spawn((
            Transform::from_matrix(convert::mat4(&n.matrix)),
            if n.visible {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
        ));
        if let Some(p) = parent {
            e.insert(ChildOf(p));
        }
        let id = e.id();
        self.nodes[i] = Some(id);
        if let (Some(mesh), Some(draw)) = (n.mesh, Draw::of(n.ty)) {
            let desc = &self.scene.meshes[mesh as usize];
            let parts: Vec<(u32, (u32, u32))> = if n.multi_material == Some(true) {
                desc.groups
                    .iter()
                    .enumerate()
                    .filter_map(|(g, grp)| {
                        let m = *n.materials.get(grp.material_index as usize)?;
                        Some((m, convert::draw_span(self.scene, desc, Some(g))))
                    })
                    .collect()
            } else {
                n.materials
                    .first()
                    .map(|&m| vec![(m, convert::draw_span(self.scene, desc, None))])
                    .unwrap_or_default()
            };
            let cast = n.cast_shadow || racer;
            let receive = n.receive_shadow;
            for (mi, (start, count)) in parts {
                let mdesc = &self.scene.materials[mi as usize];
                let mode = convert::stand_in(mdesc);
                if mode == StandIn::Hidden || mode == StandIn::Sky {
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
                let scene = self.scene;
                let Some((mh, layout)) = self
                    .meshes
                    .entry(key)
                    .or_insert_with(|| {
                        convert::build_mesh(scene, key).map(|m| {
                            let layout = Layout::of(&m);
                            (meshes.add(m), layout)
                        })
                    })
                    .clone()
                else {
                    continue;
                };
                let Some((mat, mkey)) = self.material(mi, mats) else {
                    continue;
                };
                self.combos.note(mkey, &mat, &layout, cast);
                let mut d = commands.spawn((
                    Mesh3d(mh),
                    MeshMaterial3d(mat),
                    Transform::default(),
                    ChildOf(id),
                ));
                if !cast {
                    d.insert(NotShadowCaster);
                }
                if !receive {
                    d.insert(NotShadowReceiver);
                }
            }
        }
        let children = self.scene.nodes[i].children.clone();
        for c in children {
            self.node(c as usize, Some(id), commands, meshes, mats, racer);
        }
        id
    }
}

impl Cars {
    /// The body's pitch and roll (`body.rotation.x/z`), the wheels' spin
    /// (`rotation.x += speed / radius × dt`) and the steer pivots
    /// (`rotation.y = −steerAngle`) of car `i`.
    pub fn sync_parts(
        &mut self,
        i: usize,
        springs: &super::pose::Springs,
        speed: f64,
        steer_angle: f64,
        dt: f64,
        q: &mut Query<&mut Transform, super::BodyFilter>,
    ) {
        let Some(car) = self.cars.get_mut(i) else {
            return;
        };
        if let Some((e, base)) = car.body
            && let Ok(mut t) = q.get_mut(e)
        {
            let (_, y, _) = base.rotation.to_euler(EulerRot::XYZ);
            t.rotation =
                DQuat::from_euler(EulerRot::XYZ, springs.pitch, f64::from(y), springs.roll)
                    .as_quat();
        }
        for w in &mut car.wheels {
            w.spin += speed / w.radius * dt;
            w.spin %= std::f64::consts::TAU;
            if let Ok(mut t) = q.get_mut(w.e) {
                // three's XYZ order: Rx(x + a) Ry Rz = Rx(a) · (Rx(x) Ry Rz).
                t.rotation = Quat::from_rotation_x(w.spin as f32) * w.base.rotation;
            }
        }
        for (e, base) in &car.pivots {
            if let Ok(mut t) = q.get_mut(*e) {
                let (x, _, z) = base.rotation.to_euler(EulerRot::XYZ);
                t.rotation = Quat::from_euler(EulerRot::XYZ, x, -steer_angle as f32, z);
            }
        }
    }

    /// Applies a light setter's or `setFar`'s edits to what is drawn.
    pub fn apply(
        &mut self,
        edits: Vec<Edit>,
        vis: &mut Query<&mut Visibility, Without<super::RaceCar>>,
        assets: &mut Assets<ThreeMaterial>,
        lights: &mut MaterialLights,
    ) {
        for e in edits {
            match (e.target, e.change) {
                (WHandle::Node(n), Change::Visible(v)) => {
                    let Some(ent) = self.handles.node(n).and_then(|i| self.nodes[i as usize])
                    else {
                        continue;
                    };
                    if let Ok(mut vv) = vis.get_mut(ent) {
                        let want = if v {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        };
                        if *vv != want {
                            *vv = want;
                        }
                    }
                }
                (
                    WHandle::Material(m),
                    Change::Number {
                        prop: "emissiveIntensity",
                        value,
                    },
                ) => {
                    let Some(i) = self.handles.material(m) else {
                        continue;
                    };
                    if self.last.get(&i) == Some(&value) {
                        continue;
                    }
                    self.last.insert(i, value);
                    // The value goes to the material's light slot, which
                    // the shader multiplies the emissive colour by (D456):
                    // the material itself changes once, when it gets its
                    // slot, not every frame the value moves.
                    if let Some(&k) = self.slots.get(&i) {
                        lights.values[k] = value as f32;
                        continue;
                    }
                    let (hs, c) = &self.materials[i as usize];
                    if let Some(k) = lights.slot(value as f32) {
                        self.slots.insert(i, k);
                        for h in hs {
                            if let Some(mut mat) = assets.get_mut(h) {
                                let w = mat.params.emissive.w;
                                mat.params.emissive =
                                    Vec4::new(c[0] as f32, c[1] as f32, c[2] as f32, w);
                                mat.params.night = Vec4::new(0.0, 0.0, 2.0, k as f32);
                            }
                        }
                        continue;
                    }
                    for h in hs {
                        if let Some(mut mat) = assets.get_mut(h) {
                            let w = mat.params.emissive.w;
                            mat.params.emissive = Vec4::new(
                                (c[0] * value) as f32,
                                (c[1] * value) as f32,
                                (c[2] * value) as f32,
                                w,
                            );
                        }
                    }
                }
                // The siren's glow colours are M8's.
                _ => {}
            }
        }
    }
}
