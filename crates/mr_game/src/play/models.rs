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
//!
//! In Hot Pursuit the police cars and the sawhorses follow the field
//! ([`Extra`], PursuitView's `makeUnit`, WP 8.1 and 8.2) into the same
//! graph, as the JS builds them into the same page's caches; the siren's
//! glow billboards draw with `Patch::PoliceGlow` (D921).

use crate::convert::{self, Draw, MeshKey, StandIn};
use crate::loader::SceneEntity;
use crate::render::SharedImages;
use crate::render::lighting::MaterialLights;
use crate::render::material::{ThreeKey, ThreeMaterial, three_material};
use crate::warmup::{Combos, Layout};
use bevy::camera::primitives::Aabb;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::{DQuat, DVec3, EulerRot, Vec4};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use mr_scene::{MaterialKind, MeshDesc, Scene};
use mr_worldgen::car_model::{BuildOpts, Lod, VehicleModel, build_vehicle};
use mr_worldgen::object::{HandleMap, NodeId, SceneGraph};
use mr_worldgen::pursuit_props::{PropKit, sawhorse_model, spike_geometry, spike_material};
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

/// What the pursuit adds after the field (PursuitView's `makeUnit`, in its
/// order: the units, the roadblock cars, the sawhorses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extra {
    /// `buildVehicle(kind, { ...o, lod: 'low', far: true, seed })`, every
    /// mesh casting a shadow; `livery` is the interceptor's `{ livery:
    /// 'police' }`.
    Police {
        kind: &'static str,
        livery: bool,
        seed: u32,
    },
    /// `sawhorseModel()`.
    Sawhorse,
}

/// A prop drawn as a minimal vehicle model (a sawhorse): its root and its
/// body with the body's rest transform.
pub struct Prop {
    pub root: Entity,
    body: Option<(Entity, Transform)>,
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
    /// Every mesh entity of the model (not what the effects hang on it).
    meshes: Vec<Entity>,
    /// The player's model's box in its root's frame, for the bumper view
    /// (`incar`, D1040); `None` for the rest of the field.
    bounds: Option<(DVec3, DVec3)>,
    /// Whether the meshes are hidden from the camera inside the car.
    hidden: bool,
}

/// Every car of the race, drawn.
pub struct Cars {
    pub cars: Vec<Car>,
    /// Where the police cars start in `cars` (they follow the field).
    pub police_base: usize,
    /// The sawhorses.
    pub props: Vec<Prop>,
    /// The spike strips' material (`spikeMat`), with a pursuit.
    pub spike_material: Option<Handle<ThreeMaterial>>,
    /// Scene material index of a siren glow → its two light slots (`uRed`
    /// and `uBlue`'s levels, D921), once given.
    glow_slots: HashMap<u32, (usize, usize)>,
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

/// Builds the field's models and spawns them, hidden until placed, then
/// the pursuit's cars and props after the field.
pub fn spawn_field(
    wants: &[Want],
    extras: &[Extra],
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    mats: &mut Assets<ThreeMaterial>,
    shared: &SharedImages,
) -> Cars {
    let mut graph = SceneGraph::new();
    let mut textures = TextureCache::new();
    let mut models = Vec::new();
    // Per model: every mesh casts a shadow (racers and police).
    let mut racer: Vec<bool> = wants.iter().map(|w| w.racer).collect();
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
    let mut kit = PropKit::default();
    let mut saws = Vec::new();
    for e in extras {
        match *e {
            Extra::Police { kind, livery, seed } => {
                let opts = BuildOpts {
                    lod: Some(Lod::Low),
                    far: true,
                    seed,
                    police_livery: livery,
                    ..BuildOpts::default()
                };
                // PursuitView falls back to a dark sedan if a kind fails.
                let m = build_vehicle(&mut graph, &mut textures, kind, &opts)
                    .or_else(|| {
                        let o = BuildOpts {
                            color: Some(0x16181c),
                            police_livery: false,
                            ..opts
                        };
                        build_vehicle(&mut graph, &mut textures, "sedan", &o)
                    })
                    .expect("the sedan builds");
                graph.roots.push(m.root);
                models.push(m);
                racer.push(true);
            }
            Extra::Sawhorse => {
                let m = sawhorse_model(&mut graph, &mut kit);
                graph.roots.push(m.root);
                saws.push(m);
            }
        }
    }
    // A spike strip's material on a strip's geometry, never drawn: the
    // strips are made where the pursuit lays them (`play::police`), with
    // this material, and its pipeline joins the warm-up now.
    let spike = (!extras.is_empty()).then(|| {
        let mat = spike_material(&mut graph, &mut kit);
        let geo = graph.add_geometry(spike_geometry(8.0));
        let n = graph.mesh(geo, mat);
        graph.roots.push(n);
        n
    });
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
        spawned: Vec::new(),
    };
    let mut cars = Vec::new();
    for (k, (m, &racer)) in models.into_iter().zip(&racer).enumerate() {
        let Some(ri) = handles.node(m.root) else {
            continue;
        };
        s.spawned.clear();
        let root = s.node(ri as usize, None, commands, meshes, mats, racer);
        let car_meshes = std::mem::take(&mut s.spawned);
        // The player's car (the first wanted) hides from the camera inside
        // it (D1040).
        let bounds =
            (k == 0).then(|| super::incar::bounds(&super::incar::triangles(&scene, ri as usize)));
        commands
            .entity(root)
            .insert((Transform::default(), Visibility::Hidden, SceneEntity));
        if k >= wants.len() {
            // The police: drawn and taken away with the race's cars.
            commands
                .entity(root)
                .insert((super::RaceCar, Name::new(format!("police {}", m.kind))));
        }
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
            meshes: car_meshes,
            bounds,
            hidden: false,
        });
    }
    let mut props = Vec::new();
    for m in saws {
        let Some(ri) = handles.node(m.root) else {
            continue;
        };
        let root = s.node(ri as usize, None, commands, meshes, mats, false);
        commands
            .entity(root)
            .insert((Transform::default(), Visibility::Hidden, SceneEntity));
        let body = handles.node(m.body).and_then(|i| {
            Some((
                s.nodes[i as usize]?,
                Transform::from_matrix(convert::mat4(&scene.nodes[i as usize].matrix)),
            ))
        });
        commands.entity(root).insert(super::RaceCar);
        props.push(Prop { root, body });
    }
    let spike_material = spike.and_then(|n| {
        let ni = handles.node(n)?;
        let node = &scene.nodes[ni as usize];
        let mesh = node.mesh?;
        let mi = *node.materials.first()?;
        let (start, count) = convert::draw_span(&scene, &scene.meshes[mesh as usize], None);
        let m = convert::build_mesh(
            &scene,
            MeshKey {
                mesh,
                start,
                count,
                draw: Draw::Triangles,
                colors: false,
                lit: true,
                extra: None,
            },
        )?;
        let (mat, key) = s.material(mi, mats)?;
        s.combos.note(key, &mat, &Layout::of(&m), false);
        Some(mat)
    });
    // D458: the field's combinations join the warm-up (`warmup`): one
    // off-screen stand-in each, despawned once every pipeline is ready.
    let n = s.combos.spawn(commands, meshes);
    // D459: and the HUD's glyphs.
    crate::warmup::spawn_glyphs(commands);
    info!("race cars: {n} material × mesh-layout combinations warmed up");
    Cars {
        police_base: wants.len(),
        props,
        spike_material,
        glow_slots: HashMap::new(),
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
    /// The mesh entities spawned since last cleared.
    spawned: Vec<Entity>,
}

/// The siren glow's geometry: its positions, and the corner, `aBlue` and
/// `aSize` in the extra attribute (`Patch::PoliceGlow`).
fn glow_mesh(scene: &Scene, m: &MeshDesc) -> Option<Mesh> {
    let attr = |name: &str| {
        m.attribute(name)
            .map(|a| &scene.buffers[a.accessor as usize])
    };
    let pos = attr("position")?;
    let n = pos.count();
    let get = |b: Option<&mr_scene::Buffer>, i: usize, k: usize| -> f32 {
        b.map_or(0.0, |b| b.data.get(i * b.item_size as usize + k) as f32)
    };
    let (corner, blue, size) = (attr("corner"), attr("aBlue"), attr("aSize"));
    let p: Vec<[f32; 3]> = (0..n)
        .map(|i| [0, 1, 2].map(|k| get(Some(pos), i, k)))
        .collect();
    let extra: Vec<[f32; 4]> = (0..n)
        .map(|i| {
            [
                get(corner, i, 0),
                get(corner, i, 1),
                get(blue, i, 0),
                get(size, i, 0),
            ]
        })
        .collect();
    let idx: Vec<u32> = match m.index {
        Some(b) => {
            let b = &scene.buffers[b as usize];
            (0..b.data.len()).map(|i| b.data.get(i) as u32).collect()
        }
        None => (0..n as u32).collect(),
    };
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, p);
    mesh.insert_attribute(convert::ATTRIBUTE_EXTRA, extra);
    mesh.insert_indices(Indices::U32(idx));
    Some(mesh)
}

impl Spawner<'_> {
    /// The siren's glow billboards (`sirenGlow`, renderOrder 2): culled by
    /// its bounding sphere grown by the largest spot, as three culls it,
    /// and casting no shadow (its quads are points until the vertex shader
    /// opens them, so the JS's shadow pass draws nothing of them either).
    #[allow(clippy::too_many_arguments)]
    fn glow(
        &mut self,
        i: usize,
        mesh: u32,
        mi: u32,
        parent: Entity,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        mats: &mut Assets<ThreeMaterial>,
    ) {
        let desc = &self.scene.meshes[mesh as usize];
        let key = MeshKey {
            mesh,
            start: 0,
            count: 0,
            draw: Draw::Triangles,
            colors: false,
            lit: false,
            extra: Some("corner+aBlue+aSize"),
        };
        let scene = self.scene;
        let Some((mh, layout)) = self
            .meshes
            .entry(key)
            .or_insert_with(|| {
                glow_mesh(scene, desc).map(|m| {
                    let layout = Layout::of(&m);
                    (meshes.add(m), layout)
                })
            })
            .clone()
        else {
            return;
        };
        let Some((mat, mkey)) = self.material(mi, mats) else {
            return;
        };
        self.combos.note(mkey, &mat, &layout, false);
        let mut d = commands.spawn((
            Mesh3d(mh),
            MeshMaterial3d(mat),
            Transform::default(),
            ChildOf(parent),
            NotShadowCaster,
            NotShadowReceiver,
        ));
        self.spawned.push(d.id());
        if let Some(bs) = &desc.bounding_sphere
            && bs.len() == 4
        {
            d.insert(Aabb {
                center: Vec3::new(bs[0] as f32, bs[1] as f32, bs[2] as f32).into(),
                half_extents: Vec3::splat(bs[3] as f32).into(),
            });
        }
        if let Some(o) = crate::render::sort::RenderOrder::of(self.scene.nodes[i].render_order) {
            d.insert(o);
        }
    }

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
                if mdesc.kind == MaterialKind::PoliceGlow {
                    self.glow(i, mesh, mi, id, commands, meshes, mats);
                    continue;
                }
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
                self.spawned.push(d.id());
                if !cast {
                    d.insert(NotShadowCaster);
                }
                if !receive {
                    d.insert(NotShadowReceiver);
                }
                // The glass and the siren glow's `renderOrder` (D810).
                if let Some(o) = crate::render::sort::RenderOrder::of(n.render_order) {
                    d.insert(o);
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

impl Car {
    /// The body node's entity (the flames hang on it, `Effects.addCar`).
    pub fn body_entity(&self) -> Option<Entity> {
        self.body.map(|(e, _)| e)
    }
}

impl Cars {
    /// The entity drawn for a model's node (the siren's anchor, say).
    pub fn entity_of(&self, id: NodeId) -> Option<Entity> {
        self.handles.node(id).and_then(|i| self.nodes[i as usize])
    }

    /// The siren glow's `uRed` and `uBlue` levels of car `i` (`setSiren`'s
    /// levels times PursuitView's daylight dimming), into its two light
    /// slots, which it takes the first time (D921).
    pub fn set_glow(
        &mut self,
        i: usize,
        red: f64,
        blue: f64,
        assets: &mut Assets<ThreeMaterial>,
        lights: &mut MaterialLights,
    ) {
        let Some(m) = self
            .cars
            .get(i)
            .and_then(|c| c.model.siren)
            .and_then(|s| self.handles.material(s.glow_material))
        else {
            return;
        };
        let slots = match self.glow_slots.get(&m) {
            Some(&k) => k,
            None => {
                let (Some(r), Some(b)) = (lights.slot(red as f32), lights.slot(blue as f32)) else {
                    return;
                };
                for h in &self.materials[m as usize].0 {
                    if let Some(mut mat) = assets.get_mut(h) {
                        mat.params.kind0.x = r as f32;
                        mat.params.kind0.y = b as f32;
                    }
                }
                self.glow_slots.insert(m, (r, b));
                (r, b)
            }
        };
        lights.values[slots.0] = red as f32;
        lights.values[slots.1] = blue as f32;
    }

    /// A prop's body pitch and roll (`Vehicle.sync` on a sawhorse: no
    /// wheels, no steering, no lights).
    pub fn sync_prop(
        &self,
        k: usize,
        springs: &super::pose::Springs,
        q: &mut Query<&mut Transform, super::BodyFilter>,
    ) {
        if let Some((e, base)) = self.props.get(k).and_then(|p| p.body)
            && let Ok(mut t) = q.get_mut(e)
        {
            let (_, y, _) = base.rotation.to_euler(EulerRot::XYZ);
            t.rotation =
                DQuat::from_euler(EulerRot::XYZ, springs.pitch, f64::from(y), springs.roll)
                    .as_quat();
        }
    }

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

    /// Car `i`'s meshes are not drawn while the camera's eye is inside it
    /// (the bumper view, `incar`, D1040); its root, the anchors and what
    /// hangs on the body stay. Only a car with a box (the player's) hides.
    pub fn hide_round_eye(
        &mut self,
        i: usize,
        pos: DVec3,
        q: DQuat,
        eye: DVec3,
        vis: &mut Query<&mut Visibility, Without<super::RaceCar>>,
    ) {
        let Some(car) = self.cars.get_mut(i) else {
            return;
        };
        let Some(bounds) = car.bounds else {
            return;
        };
        let hide = super::incar::eye_inside(bounds, pos, q, eye);
        if hide == car.hidden {
            return;
        }
        car.hidden = hide;
        let want = if hide {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        for &e in &car.meshes {
            if let Ok(mut v) = vis.get_mut(e) {
                *v = want;
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
