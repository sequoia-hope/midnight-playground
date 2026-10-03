//! The material test scenes (SPEC 6.2 "Verification", roadmap WP 2.3,
//! DECISIONS D19): the scenes `tools/parity/materials.mjs` renders with
//! three.js, rendered here from the same definitions
//! (`parity/golden/materials/scenes.json`) so the two can be compared pixel
//! by pixel (`cargo xtask parity materials`).
//!
//! A scene is a camera, the fixed sun (with its shadow box) and hemisphere
//! light, the level's sky dome as the environment, and either plain
//! materials on simple shapes (the fixed scenes: bloom chart, fog ramp,
//! shadow edge, sphere grids) or one material kind taken from the level's
//! scene export by its path, on a sphere and a plane carrying the source
//! mesh's extra attributes set to its first vertex. The geometry is three's
//! (`mr_worldgen::three_geom`), so only shading differs.
//!
//! Natively (`--materials all --out <dir>`) the scenes are drawn one after
//! another in a 512 × 512 window and each is saved as
//! `<dir>/<group>/<name>.png`, then the client exits.

use crate::Opts;
use crate::convert::{self, Draw, MeshKey};
use crate::render::lighting::{Hemi, Lighting, ShadowParams, Sun};
use crate::render::material::three_material;
use crate::render::pmrem::{ENV_DONE, EnvRequest};
use crate::render::sky::hex_color;
use crate::render::{SharedImages, SkyMaterial, SkyState, ThreeMaterial};
use crate::status::PIPELINES_WAITING;
use bevy::app::AppExit;
use bevy::camera::{PerspectiveProjection, Projection};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::DVec3;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use mr_scene::{Buffer, BufferData, MaterialDesc, MaterialKind, Scene};
use mr_worldgen::three_geom::{BufferGeometry, box_geometry, plane_geometry, sphere_geometry};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The scene definitions, as `tools/parity/materials.mjs` wrote them.
pub const SCENES_JSON: &str = include_str!("../../../parity/golden/materials/scenes.json");

/// The kinds this client draws as the JS does: the plain ones and the sky
/// (WP 2.3), the terrain, road and sea (WP 2.4). `all` renders these and the fixed scenes; `every` adds the
/// other kinds drawn as their plain stand-ins (for the record, not the gate).
pub const PORTED_KINDS: [&str; 10] = [
    "Standard", "Physical", "Lambert", "Basic", "SkyDome", "Terrain", "Asphalt", "Shoulder",
    "Markings", "Sea",
];

/// The scene definitions and the common setup.
pub struct Defs {
    pub setup: Value,
    pub scenes: Vec<Value>,
}

pub fn defs() -> Defs {
    let v: Value = serde_json::from_str(SCENES_JSON).expect("scenes.json parses");
    Defs {
        setup: v["setup"].clone(),
        scenes: v["scenes"].as_array().cloned().unwrap_or_default(),
    }
}

/// The scenes a `--materials` value names: `all`, `every`, or names.
pub fn select(defs: &Defs, which: &str) -> Vec<Value> {
    let drawable = |s: &Value| {
        let kind = s["kind"].as_str().unwrap_or("");
        s["group"] == "fixed" || PORTED_KINDS.contains(&kind)
    };
    match which {
        "all" => defs
            .scenes
            .iter()
            .filter(|s| drawable(s))
            .cloned()
            .collect(),
        "every" => defs
            .scenes
            .iter()
            .filter(|s| {
                drawable(s)
                    || mr_scene_kind(s).is_some_and(|k| {
                        // Stand-ins: kinds on a built-in mesh material (the
                        // effect shaders draw nothing yet; lines and points
                        // need the JS tool's wireframe and point grid).
                        !matches!(
                            k,
                            MaterialKind::Line
                                | MaterialKind::Points
                                | MaterialKind::GlowPoints
                                | MaterialKind::FlickerPoints
                                | MaterialKind::TrafficStreams
                                | MaterialKind::SkyGlow
                                | MaterialKind::Surf
                                | MaterialKind::LighthouseBeam
                                | MaterialKind::Steam
                                | MaterialKind::Particles
                                | MaterialKind::SkidMarks
                                | MaterialKind::PoliceGlow
                                | MaterialKind::Sprite
                        )
                    })
            })
            .cloned()
            .collect(),
        names => {
            let names: Vec<&str> = names.split(',').collect();
            defs.scenes
                .iter()
                .filter(|s| names.contains(&s["name"].as_str().unwrap_or("")))
                .cloned()
                .collect()
        }
    }
}

fn mr_scene_kind(s: &Value) -> Option<MaterialKind> {
    serde_json::from_value(s.get("kind")?.clone()).ok()
}

/// The source scene a definition needs: a level's export or the models.
pub fn source_of(def: &Value) -> String {
    if def["source"]["models"] == json!(true) {
        "models".into()
    } else {
        def["source"]["level"].as_str().unwrap_or("sierra").into()
    }
}

/// The level whose sky lights a scene (the page it was rendered in).
fn sky_level_of(def: &Value) -> String {
    def["source"]["level"].as_str().unwrap_or("sierra").into()
}

// ── Loading ─────────────────────────────────────────────────────────────

/// Scenes loaded on a thread, by name.
/// A source scene, or why it could not be read.
type Loaded = (String, Result<Arc<Scene>, String>);

static LOADED: std::sync::Mutex<Vec<Loaded>> = std::sync::Mutex::new(Vec::new());

#[cfg(not(target_arch = "wasm32"))]
fn load_sources(names: Vec<String>) {
    std::thread::spawn(move || {
        let root = crate::native::repo_root();
        for name in names {
            let r = mr_scene::cache::scenes_dir(&root)
                .map_err(|e| e.to_string())
                .and_then(|dir| {
                    let p = dir.join(format!("{name}.mrscene"));
                    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                    mr_scene::read(&bytes).map_err(|e| format!("{}: {e}", p.display()))
                })
                .map(Arc::new);
            LOADED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((name, r));
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn load_sources(_names: Vec<String>) {
    LOADED.lock().unwrap_or_else(|e| e.into_inner()).push((
        "sierra".into(),
        Err("the material test scenes run natively for now".into()),
    ));
}

// ── The run ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Waiting for the source scenes.
    Loading,
    /// Spawn the next scene.
    Setup,
    /// Drawing until everything is compiled and the environment is built.
    Settling,
    /// The screenshot is on its way.
    Capturing,
    Done,
}

/// A source scene with the Bevy images made from it so far.
struct Source {
    scene: Arc<Scene>,
    images: Vec<Option<Handle<Image>>>,
}

#[derive(Resource)]
struct MatRun {
    setup: Value,
    scenes: Vec<Value>,
    index: usize,
    phase: Phase,
    frames: u32,
    quiet: u32,
    sources: HashMap<String, Source>,
    out: std::path::PathBuf,
    env_level: Option<String>,
}

/// Marks what belongs to the current test scene.
#[derive(Component)]
struct MatEntity;

static CAPTURED: AtomicBool = AtomicBool::new(false);

pub fn plugin(app: &mut App, which: &str, out: Option<String>) {
    let d = defs();
    let scenes = select(&d, which);
    if scenes.is_empty() {
        error!("no material scene matches `{which}`");
    }
    let mut needed: Vec<String> = Vec::new();
    for s in &scenes {
        for n in [source_of(s), sky_level_of(s)] {
            if !needed.contains(&n) {
                needed.push(n);
            }
        }
    }
    load_sources(needed);
    app.insert_resource(MatRun {
        setup: d.setup,
        scenes,
        index: 0,
        phase: Phase::Loading,
        frames: 0,
        quiet: 0,
        sources: HashMap::new(),
        out: out.unwrap_or_else(|| "materials-rust".into()).into(),
        env_level: None,
    })
    .add_systems(Update, run);
}

fn vec3(v: &Value) -> Option<Vec3> {
    let a = v.as_array()?;
    Some(Vec3::new(
        a.first()?.as_f64()? as f32,
        a.get(1)?.as_f64()? as f32,
        a.get(2)?.as_f64()? as f32,
    ))
}

fn dvec3(v: &Value) -> Option<DVec3> {
    let a = v.as_array()?;
    Some(DVec3::new(
        a.first()?.as_f64()?,
        a.get(1)?.as_f64()?,
        a.get(2)?.as_f64()?,
    ))
}

fn hex(v: &Value) -> [f64; 3] {
    hex_color(v.as_u64().unwrap_or(0) as u32)
}

/// A built-in material of the fixed scenes (`pageMaterial`) as the export
/// would have described it.
fn plain_material(d: &Value) -> MaterialDesc {
    let ty = if d["type"] == "Physical" {
        "MeshPhysicalMaterial"
    } else {
        "MeshStandardMaterial"
    };
    let mut params = Map::new();
    for (k, v) in d.as_object().into_iter().flatten() {
        match k.as_str() {
            "type" => {}
            "color" => {
                params.insert("color".into(), json!({ "color": hex(v) }));
            }
            _ => {
                params.insert(k.clone(), v.clone());
            }
        }
    }
    MaterialDesc {
        kind: if ty == "MeshPhysicalMaterial" {
            MaterialKind::Physical
        } else {
            MaterialKind::Standard
        },
        kind_opts: None,
        ty: ty.into(),
        name: String::new(),
        program_key: None,
        params,
        uniforms: None,
        shader: None,
    }
}

fn basic_material(value: f64) -> MaterialDesc {
    let mut params = Map::new();
    params.insert("color".into(), json!({ "color": [value, value, value] }));
    MaterialDesc {
        kind: MaterialKind::Basic,
        kind_opts: None,
        ty: "MeshBasicMaterial".into(),
        name: String::new(),
        program_key: None,
        params,
        uniforms: None,
        shader: None,
    }
}

/// The node at a child-index path from a root.
fn node_at(scene: &Scene, root: u32, path: &[Value]) -> Option<u32> {
    let mut n = root;
    for i in path {
        let i = i.as_u64()? as usize;
        n = *scene.nodes[n as usize].children.get(i)?;
    }
    Some(n)
}

/// One object to draw: a geometry in the scratch scene, its material, how
/// it sits, its shadows and its instance colour.
struct Object {
    mesh: u32,
    material: Mat,
    translation: Vec3,
    cast: bool,
    receive: bool,
    tint: Option<[f32; 3]>,
}

enum Mat {
    Three(Box<MaterialDesc>),
    Sky(u32),
}

/// `pageWithAttrs`: the source mesh's extra attributes, each set to its
/// first vertex's (or first instance's) value everywhere. Only `color`
/// matters to the plain kinds.
fn with_attrs(scratch: &mut Scene, mesh: u32, src: &Scene, src_mesh: u32) {
    let sm = &src.meshes[src_mesh as usize];
    let n = {
        let m = &scratch.meshes[mesh as usize];
        let p = m.attribute("position").expect("position");
        scratch.buffers[p.accessor as usize].count()
    };
    for a in &sm.attributes {
        if matches!(a.name.as_str(), "position" | "normal")
            || (a.name == "uv" && scratch.meshes[mesh as usize].attribute("uv").is_some())
        {
            continue;
        }
        let b = &src.buffers[a.accessor as usize];
        let k = b.item_size as usize;
        let first: Vec<f64> = (0..k).map(|j| b.data.get(j)).collect();
        let data = match &b.data {
            BufferData::F32(_) => {
                BufferData::F32((0..n * k).map(|i| first[i % k] as f32).collect())
            }
            BufferData::U8(_) => BufferData::U8((0..n * k).map(|i| first[i % k] as u8).collect()),
            BufferData::U16(_) => {
                BufferData::U16((0..n * k).map(|i| first[i % k] as u16).collect())
            }
            BufferData::I8(_) => BufferData::I8((0..n * k).map(|i| first[i % k] as i8).collect()),
            BufferData::I16(_) => {
                BufferData::I16((0..n * k).map(|i| first[i % k] as i16).collect())
            }
            _ => BufferData::F32((0..n * k).map(|i| first[i % k] as f32).collect()),
        };
        scratch.buffers.push(Buffer {
            item_size: b.item_size,
            normalized: b.normalized,
            data,
        });
        let accessor = (scratch.buffers.len() - 1) as u32;
        scratch.meshes[mesh as usize]
            .attributes
            .push(mr_scene::AttributeRef {
                name: a.name.clone(),
                accessor,
                instanced: false,
                mesh_per_attribute: None,
            });
    }
}

fn add(scratch: &mut Scene, g: BufferGeometry) -> u32 {
    g.add_to_scene(scratch, "")
}

/// The objects of one scene, the source scene they take a material from.
fn objects(
    def: &Value,
    setup: &Value,
    scratch: &mut Scene,
    src: &Scene,
) -> Result<Vec<Object>, String> {
    let sp = &setup["sphere"];
    let pl = &setup["plane"];
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    let mut out = Vec::new();
    if let Some(quads) = def["quads"].as_array() {
        for q in quads {
            let mesh = add(
                scratch,
                plane_geometry(f(&q["size"][0]), f(&q["size"][1]), 1.0, 1.0),
            );
            out.push(Object {
                mesh,
                material: Mat::Three(Box::new(basic_material(f(&q["value"])))),
                translation: Vec3::new(f(&q["x"]) as f32, 0.0, 0.0),
                cast: false,
                receive: false,
                tint: None,
            });
        }
    }
    if let Some(boxes) = def["boxes"].as_array() {
        for b in boxes {
            let s = f(&b["size"]);
            let mesh = add(scratch, box_geometry(s, s, s, 1.0, 1.0, 1.0));
            out.push(Object {
                mesh,
                material: Mat::Three(Box::new(plain_material(
                    &json!({ "type": "Standard", "color": 0xffffff, "roughness": 0.9 }),
                ))),
                translation: Vec3::new(f(&b["x"]) as f32, f(&b["y"]) as f32, f(&b["z"]) as f32),
                cast: false,
                receive: false,
                tint: None,
            });
        }
    }
    if !def["material"].is_null() || !def["grid"].is_null() {
        let mut g = plane_geometry(
            f(&pl["size"]) * 2.0,
            f(&pl["size"]),
            f(&pl["segments"]),
            f(&pl["segments"]),
        );
        g.rotate_x(-std::f64::consts::PI / 2.0)
            .translate(0.0, f(&pl["y"]), 0.0);
        let mesh = add(scratch, g);
        let plane_mat = if def["material"].is_null() {
            json!({ "type": "Standard", "color": 0x808080, "roughness": 0.9 })
        } else {
            def["material"].clone()
        };
        out.push(Object {
            mesh,
            material: Mat::Three(Box::new(plain_material(&plane_mat))),
            translation: Vec3::ZERO,
            cast: false,
            receive: true,
            tint: None,
        });
        let grid = def["grid"]
            .as_array()
            .cloned()
            .unwrap_or_else(|| vec![json!({ "x": 0, "material": def["material"] })]);
        let radius = if def["grid"].is_null() {
            f(&sp["radius"])
        } else {
            0.7
        };
        for s in grid {
            let mesh = add(
                scratch,
                sphere_geometry(
                    radius,
                    f(&sp["widthSegments"]),
                    f(&sp["heightSegments"]),
                    0.0,
                    std::f64::consts::TAU,
                    0.0,
                    std::f64::consts::PI,
                ),
            );
            out.push(Object {
                mesh,
                material: Mat::Three(Box::new(plain_material(&s["material"]))),
                translation: Vec3::new(f(&s["x"]) as f32, 0.0, 0.0),
                cast: true,
                receive: true,
                tint: None,
            });
        }
    }
    if def["group"] == "kinds" {
        // The source object: by path from the world root (or the dome).
        let root = if def["source"]["sky"] == json!(true) {
            src.roots
                .iter()
                .copied()
                .find(|&r| {
                    src.nodes[r as usize]
                        .materials
                        .first()
                        .is_some_and(|&m| src.materials[m as usize].kind == MaterialKind::SkyDome)
                })
                .ok_or("no sky dome in the source")?
        } else {
            *src.roots.first().ok_or("no root")?
        };
        let path = def["path"].as_array().cloned().unwrap_or_default();
        let node_i = node_at(src, root, &path).ok_or("the source path is not in the export")?;
        let node = &src.nodes[node_i as usize];
        let mi = *node
            .materials
            .get(def["materialIndex"].as_u64().unwrap_or(0) as usize)
            .ok_or("no material")?;
        let src_mesh = node.mesh.ok_or("no mesh")?;
        let mdesc = &src.materials[mi as usize];
        let tint = node.instances.and_then(|k| {
            let inst = &src.instances[k as usize];
            let c = &src.buffers[inst.colors? as usize].data;
            Some([c.get(0) as f32, c.get(1) as f32, c.get(2) as f32])
        });
        let sphere = add(
            scratch,
            sphere_geometry(
                f(&sp["radius"]),
                f(&sp["widthSegments"]),
                f(&sp["heightSegments"]),
                0.0,
                std::f64::consts::TAU,
                0.0,
                std::f64::consts::PI,
            ),
        );
        let mut g = plane_geometry(
            f(&pl["size"]),
            f(&pl["size"]),
            f(&pl["segments"]),
            f(&pl["segments"]),
        );
        g.rotate_x(-std::f64::consts::PI / 2.0)
            .translate(0.0, f(&pl["y"]), 0.0);
        let plane = add(scratch, g);
        for mesh in [sphere, plane] {
            with_attrs(scratch, mesh, src, src_mesh);
            out.push(Object {
                mesh,
                material: if mdesc.kind == MaterialKind::SkyDome {
                    Mat::Sky(mi)
                } else {
                    Mat::Three(Box::new(mdesc.clone()))
                },
                translation: Vec3::ZERO,
                cast: true,
                receive: true,
                tint,
            });
        }
    }
    Ok(out)
}

/// The scene's light, fog and environment (`pageRender`, `pageLights`).
fn lighting_of(def: &Value, setup: &Value, sky: &SkyState) -> Lighting {
    let mut l = Lighting::default();
    let f = |v: &Value| v.as_f64().unwrap_or(0.0);
    if def["lights"] != json!(false) {
        let s = &setup["sun"];
        let sh = &s["shadow"];
        let b = f(&sh["box"]);
        l.sun = Some(Sun {
            color: hex(&s["color"]),
            intensity: f(&s["intensity"]),
            position: dvec3(&def["sunPosition"])
                .or_else(|| dvec3(&s["position"]))
                .unwrap_or(DVec3::Y),
            target: dvec3(&s["target"]).unwrap_or(DVec3::ZERO),
            shadow: Some(ShadowParams {
                left: -b,
                right: b,
                top: b,
                bottom: -b,
                near: f(&sh["near"]),
                far: f(&sh["far"]),
                bias: f(&sh["bias"]),
                normal_bias: f(&sh["normalBias"]),
                map_size: f(&sh["mapSize"]) as u32,
            }),
        });
        let h = &setup["hemi"];
        l.hemi = Some(Hemi {
            sky: hex(&h["sky"]),
            ground: hex(&h["ground"]),
            intensity: f(&h["intensity"]),
            position: DVec3::Y,
        });
    }
    if !def["fog"].is_null() {
        l.fog = Some((hex(&def["fog"]["color"]), f(&def["fog"]["density"])));
    }
    l.env_intensity = if def["environment"].is_null() && def.get("environment").is_some() {
        0.0
    } else {
        f(&setup["environment"]["intensity"])
    };
    l.exposure = f(&setup["renderer"]["exposure"]);
    // The dome's uniforms at the capture point (s = 0, the clock frozen).
    let mut sky = sky.clone();
    let mut tmp = Lighting::default();
    sky.update(0.0, 0.0, DVec3::ZERO, &mut tmp);
    l.sky = tmp.sky;
    l.shadows = true;
    l
}

/// The level's sky keys as `Sky` holds them (its Track's loop flag and
/// length decide the progress; at s = 0 only the loop flag matters).
fn sky_for(level: &str) -> SkyState {
    let lv = mr_levels::level_by_id(level);
    let is_loop = lv.is_loop();
    SkyState::new(&lv, is_loop, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn run(
    mut commands: Commands,
    mut mr: ResMut<MatRun>,
    shared: Res<SharedImages>,
    mut lighting: ResMut<Lighting>,
    mut env: ResMut<EnvRequest>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut three_mats: ResMut<Assets<ThreeMaterial>>,
    mut sky_mats: ResMut<Assets<SkyMaterial>>,
    mut clear: ResMut<ClearColor>,
    mut cam: Query<(&mut Transform, &mut Projection), With<Camera3d>>,
    old: Query<Entity, With<MatEntity>>,
    mut exit: MessageWriter<AppExit>,
    opts: Res<Opts>,
) {
    let mr = &mut *mr;
    match mr.phase {
        Phase::Loading => {
            let mut loaded = LOADED.lock().unwrap_or_else(|e| e.into_inner());
            for (name, r) in loaded.drain(..) {
                match r {
                    Ok(scene) => {
                        info!("material scenes: {name} loaded");
                        let n = scene.textures.len();
                        mr.sources.insert(
                            name,
                            Source {
                                scene,
                                images: vec![None; n],
                            },
                        );
                    }
                    Err(e) => {
                        error!("material scenes: {name}: {e}");
                        exit.write(AppExit::error());
                        mr.phase = Phase::Done;
                        return;
                    }
                }
            }
            let mut needed: Vec<String> = Vec::new();
            for s in &mr.scenes {
                needed.push(source_of(s));
                needed.push(sky_level_of(s));
            }
            if needed.iter().all(|n| mr.sources.contains_key(n)) {
                mr.phase = Phase::Setup;
            }
        }
        Phase::Setup => {
            for e in &old {
                commands.entity(e).despawn();
            }
            let Some(def) = mr.scenes.get(mr.index).cloned() else {
                info!("material scenes: done");
                mr.phase = Phase::Done;
                exit.write(AppExit::Success);
                return;
            };
            let name = def["name"].as_str().unwrap_or("?").to_string();
            let level = sky_level_of(&def);
            let src_name = source_of(&def);
            // The environment: the level's dome, built once per level as each
            // page of the JS tool builds it (a fresh generator each time).
            if mr.env_level.as_deref() != Some(level.as_str()) {
                let noise = {
                    let s = mr.sources.get_mut(&level).expect("loaded");
                    dome_noise(s, &mut images)
                };
                env.noise = noise;
                env.fresh = true;
                env.generation += 1;
                mr.env_level = Some(level.clone());
            }
            *lighting = lighting_of(&def, &mr.setup, &sky_for(&level));
            lighting.shadows = opts.hq;
            let bg = def
                .get("background")
                .filter(|v| !v.is_null())
                .unwrap_or(&mr.setup["background"]);
            let c = hex(bg);
            *clear = ClearColor(Color::linear_rgb(c[0] as f32, c[1] as f32, c[2] as f32));
            // The camera (fov 45, near 0.1, far 2000), placed and aimed.
            let camdef = def
                .get("camera")
                .filter(|v| !v.is_null())
                .unwrap_or(&mr.setup["camera"]);
            let pos = vec3(&camdef["position"])
                .or_else(|| vec3(&mr.setup["camera"]["position"]))
                .unwrap_or(Vec3::Z);
            let at = vec3(&camdef["lookAt"])
                .or_else(|| vec3(&mr.setup["camera"]["lookAt"]))
                .unwrap_or(Vec3::ZERO);
            if let Ok((mut t, mut p)) = cam.single_mut() {
                *t = Transform::from_translation(pos).looking_at(at, Vec3::Y);
                let sc = &mr.setup["camera"];
                *p = Projection::Perspective(PerspectiveProjection {
                    fov: (sc["fov"].as_f64().unwrap_or(45.0) as f32).to_radians(),
                    near: sc["near"].as_f64().unwrap_or(0.1) as f32,
                    far: sc["far"].as_f64().unwrap_or(2000.0) as f32,
                    ..default()
                });
            }
            // The objects.
            let src = mr.sources.get_mut(&src_name).expect("loaded");
            let mut scratch = Scene::default();
            let objs = match objects(&def, &mr.setup, &mut scratch, &src.scene) {
                Ok(o) => o,
                Err(e) => {
                    error!("material scene {name}: {e}");
                    mr.index += 1;
                    return;
                }
            };
            for o in objs {
                // The patch uniforms an updater moves in the game are
                // scene-wide here (`render::lighting::Anim`): take the
                // values the export captured.
                if let Mat::Three(m) = &o.material {
                    let a = &mut lighting.anim;
                    if let Some(w) = m.number("uWet") {
                        a.wet = w;
                    }
                    if let Some(t) = m.number("uTime") {
                        a.sea_time = t;
                        a.glow_time = t;
                    }
                    if let Some(v) = m.get("uOff2").and_then(|v| v.get("vec")) {
                        a.sea_off2 = [v[0].as_f64().unwrap_or(0.0), v[1].as_f64().unwrap_or(0.0)];
                    }
                }
                let (lit, colors, extra, material): (
                    bool,
                    bool,
                    Option<&'static str>,
                    Option<Handle<ThreeMaterial>>,
                ) = match &o.material {
                    Mat::Three(m) => {
                        // The textures this material refers to.
                        for t in m.textures() {
                            ensure_image(src, t, &mut images);
                        }
                        let tm =
                            three_material(&src.scene, m, &src.images, &shared, o.tint.is_some());
                        (
                            crate::convert::stand_in(m) == crate::convert::StandIn::Lit,
                            convert::vertex_colors(m),
                            convert::extra_attribute(m),
                            tm.map(|m| three_mats.add(m)),
                        )
                    }
                    Mat::Sky(_) => (false, false, None, None),
                };
                let key = MeshKey {
                    mesh: o.mesh,
                    start: 0,
                    count: u32::MAX,
                    draw: Draw::Triangles,
                    colors,
                    lit,
                    extra,
                };
                let (start, count) =
                    convert::draw_span(&scratch, &scratch.meshes[o.mesh as usize], None);
                let Some(m) = convert::build_mesh(
                    &scratch,
                    MeshKey {
                        start,
                        count,
                        ..key
                    },
                ) else {
                    continue;
                };
                let mut e = commands.spawn((
                    Mesh3d(meshes.add(m)),
                    Transform::from_translation(o.translation),
                    MatEntity,
                ));
                match (&o.material, material) {
                    (Mat::Sky(mi), _) => {
                        let noise = src.scene.materials[*mi as usize].texture("tNoise");
                        let noise = noise.and_then(|t| ensure_image(src, t, &mut images));
                        if let Some(noise) = noise {
                            e.insert(MeshMaterial3d(sky_mats.add(SkyMaterial {
                                globals: shared.globals.clone(),
                                noise,
                            })));
                        }
                    }
                    (_, Some(h)) => {
                        e.insert(MeshMaterial3d(h));
                    }
                    _ => {}
                }
                if !o.cast {
                    e.insert(NotShadowCaster);
                }
                if !o.receive {
                    e.insert(NotShadowReceiver);
                }
                if let Some(c) = o.tint {
                    e.insert(MeshTag(crate::loader::pack_tint(c)));
                }
            }
            info!("material scene {name}");
            mr.frames = 0;
            mr.quiet = 0;
            mr.phase = Phase::Settling;
        }
        Phase::Settling => {
            mr.frames += 1;
            let waiting = PIPELINES_WAITING.load(Ordering::Relaxed);
            let env_ok = ENV_DONE.load(Ordering::Relaxed) == env.generation;
            if waiting == 0 && env_ok && mr.frames > 4 {
                mr.quiet += 1;
            } else {
                mr.quiet = 0;
            }
            if mr.quiet >= 4 {
                let def = &mr.scenes[mr.index];
                let dir = mr.out.join(def["group"].as_str().unwrap_or("misc"));
                let _ = std::fs::create_dir_all(&dir);
                let path = dir.join(format!("{}.png", def["name"].as_str().unwrap_or("scene")));
                CAPTURED.store(false, Ordering::Relaxed);
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path))
                    .observe(|_: On<ScreenshotCaptured>| {
                        CAPTURED.store(true, Ordering::Relaxed);
                    });
                mr.phase = Phase::Capturing;
            } else if mr.frames > 600 {
                error!(
                    "material scene {}: never settled",
                    mr.scenes[mr.index]["name"]
                );
                mr.index += 1;
                mr.phase = Phase::Setup;
            }
        }
        Phase::Capturing => {
            if CAPTURED.load(Ordering::Relaxed) {
                mr.index += 1;
                mr.phase = Phase::Setup;
            }
        }
        Phase::Done => {}
    }
}

/// The Bevy image for one of a source scene's textures, made on first use.
fn ensure_image(src: &mut Source, t: u32, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let slot = src.images.get_mut(t as usize)?;
    if slot.is_none() {
        let desc = src.scene.textures.get(t as usize)?;
        *slot = convert::build_image(&src.scene, desc).map(|i| images.add(i));
    }
    slot.clone()
}

/// The sky dome's noise texture of a level's export.
fn dome_noise(src: &mut Source, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let t = src
        .scene
        .materials
        .iter()
        .find(|m| m.kind == MaterialKind::SkyDome)?
        .texture("tNoise")?;
    ensure_image(src, t, images)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_definitions_parse_and_select() {
        let d = defs();
        assert_eq!(d.setup["size"][0], 512);
        let all = select(&d, "all");
        let names: Vec<&str> = all.iter().map(|s| s["name"].as_str().unwrap()).collect();
        for n in [
            "bloom-chart",
            "fog-ramp",
            "shadow-edge",
            "standard-metal-0",
            "physical-clearcoat-sheen",
            "kind-Standard",
            "kind-SkyDome",
            "kind-Physical",
        ] {
            assert!(names.contains(&n), "{n} in {names:?}");
        }
        assert!(names.contains(&"kind-Terrain") && names.contains(&"kind-Sea"));
        assert!(!names.contains(&"kind-CityFacade"));
        assert!(select(&d, "every").len() > all.len());
        assert_eq!(select(&d, "fog-ramp,kind-Basic").len(), 2);
        assert_eq!(
            source_of(&all[names.iter().position(|n| *n == "kind-Physical").unwrap()]),
            "models"
        );
    }
}
