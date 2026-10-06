//! WP 3.9: Level 1's animators, time step by time step, against the game's
//! own `world.update` (`tools/parity/animators.mjs`, DECISIONS D470).
//!
//! Sierra is built through `level_jobs` with every ported stage and module
//! (Mountain, Valley, City). Then, for each frame the JS ran (uneven steps
//! from 0 to 3.3 s along the whole route, then 360 ticks of 1/120 s), as the
//! client runs a frame: `WorldBuild::update_sky(dt, s, focus)`, the night
//! parameters at the frame's night factor, `WorldBuild::update` with the
//! JS camera. Every edit is folded into the state of what it addresses
//! (a node's transform, visibility or instances; a geometry attribute; a
//! material's parameter or uniform; a texture's offset), keyed as the
//! golden keys the JS values, and every value the JS changed must be the
//! same, bit for bit, at every frame: the flag, the waterfall's streaks,
//! foam and spray, the windpumps, waterwheel and sails, the creek, the
//! freeway's and the streets' lamps, the neon, the aircraft warning lights,
//! the glow points, the traffic streams, the sky glow and the road's dew.
//!
//! The golden is compiled in, so the gate runs in CI and in wasm.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod common;

use std::collections::BTreeMap;

use mp_scene::digest::sha256_hex;
use mp_scene::{BufferData, Scene};
use mp_worldgen::scenery::scenery_factory;
use mp_worldgen::stages::{LevelSetup, level_stages};
use mp_worldgen::world::{
    Build, CameraView, Change, SceneEdit, SceneRef, UpdateCtx, World, WorldBuild, level_jobs,
};
use serde_json::Value;

fn golden() -> Value {
    serde_json::from_str(include_str!("../../../parity/golden/animators/sierra.json"))
        .expect("animators golden parses")
}

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn join(v: &[f64]) -> String {
    v.iter().map(|&x| bits(x)).collect::<Vec<_>>().join(",")
}

fn hex(v: &Value) -> f64 {
    common::hex(v)
}

/// Sierra as `World.build` builds it.
fn build() -> WorldBuild {
    let setup = LevelSetup {
        terrain: common::terrain_setup("sierra"),
        road: None,
    };
    let b = Build::new(
        World::new(common::level("sierra")),
        level_jobs(
            level_stages(setup),
            scenery_factory(Some(common::recording("sierra"))),
        ),
    );
    let wb = b.run(|_, _| {}).expect("Sierra builds");
    assert!(wb.log.is_empty(), "the build logged {:?}", wb.log);
    wb
}

/// The state the edits have written so far, keyed as the golden keys it.
struct State<'a> {
    scene: &'a Scene,
    values: BTreeMap<String, String>,
    /// Instance matrices and colours, and attributes, as the edits leave
    /// them (from the scene's buffers).
    arrays: BTreeMap<String, Vec<f32>>,
    /// The texture → its key prefix (`m<j>.<param>`, first holder).
    tex_key: BTreeMap<u32, String>,
    /// The mesh → the first node drawing it.
    mesh_node: BTreeMap<u32, u32>,
}

impl<'a> State<'a> {
    fn new(scene: &'a Scene) -> State<'a> {
        let mut tex_key = BTreeMap::new();
        for (j, m) in scene.materials.iter().enumerate() {
            for (k, v) in &m.params {
                if let Some(t) = v.get("texture").and_then(Value::as_u64) {
                    tex_key.entry(t as u32).or_insert(format!("m{j}.{k}"));
                }
            }
            for (k, v) in m.uniforms.iter().flatten() {
                if let Some(t) = v.get("texture").and_then(Value::as_u64) {
                    tex_key.entry(t as u32).or_insert(format!("m{j}.u.{k}"));
                }
            }
        }
        let mut mesh_node = BTreeMap::new();
        for (i, n) in scene.nodes.iter().enumerate() {
            if let Some(m) = n.mesh {
                mesh_node.entry(m).or_insert(i as u32);
            }
        }
        State {
            scene,
            values: BTreeMap::new(),
            arrays: BTreeMap::new(),
            tex_key,
            mesh_node,
        }
    }

    /// A material's number or colour: a uniform of that name if it has one
    /// (as `car_model::apply_edits` and the client resolve it), else the
    /// parameter.
    fn material_key(&self, m: u32, prop: &str) -> String {
        let has_uniform = self.scene.materials[m as usize]
            .uniforms
            .as_ref()
            .is_some_and(|u| u.contains_key(prop));
        if has_uniform {
            format!("m{m}.u.{prop}")
        } else {
            format!("m{m}.{prop}")
        }
    }

    fn array(&mut self, key: String, accessor: u32) -> &mut Vec<f32> {
        let scene = self.scene;
        self.arrays
            .entry(key)
            .or_insert_with(|| match &scene.buffers[accessor as usize].data {
                BufferData::F32(v) => v.clone(),
                d => panic!("an animated array of {:?}", d.component()),
            })
    }

    fn instances(&self, n: u32) -> &mp_scene::InstanceDesc {
        let i = self.scene.nodes[n as usize]
            .instances
            .expect("an instanced mesh");
        &self.scene.instances[i as usize]
    }

    fn apply(&mut self, e: SceneEdit) {
        match (e.target, e.change) {
            (
                SceneRef::Node(n),
                Change::Transform {
                    position,
                    quaternion,
                    scale,
                },
            ) => {
                self.values
                    .insert(format!("n{n}.position"), join(&position));
                self.values
                    .insert(format!("n{n}.quaternion"), join(&quaternion));
                self.values.insert(format!("n{n}.scale"), join(&scale));
            }
            (SceneRef::Node(n), Change::Visible(v)) => {
                self.values.insert(format!("n{n}.visible"), v.to_string());
            }
            (SceneRef::Node(n), Change::InstanceCount(c)) => {
                self.values
                    .insert(format!("n{n}.count"), bits(f64::from(c)));
            }
            (SceneRef::Node(n), Change::InstanceMatrix { index, matrix }) => {
                let acc = self.instances(n).matrices;
                let a = self.array(format!("n{n}.instanceMatrix"), acc);
                a[index as usize * 16..][..16].copy_from_slice(&matrix);
            }
            (SceneRef::Node(n), Change::InstanceColor { index, rgb }) => {
                let acc = self.instances(n).colors.expect("instance colours");
                let a = self.array(format!("n{n}.instanceColor"), acc);
                a[index as usize * 3..][..3].copy_from_slice(&rgb);
            }
            (
                SceneRef::Node(n),
                Change::Light {
                    color, intensity, ..
                },
            ) => {
                self.values.insert(
                    format!("n{n}.light"),
                    join(&[color[0], color[1], color[2], intensity]),
                );
            }
            (
                SceneRef::Mesh(g),
                Change::Attribute {
                    name,
                    offset,
                    values,
                },
            ) => {
                let node = self.mesh_node[&g];
                let acc = self.scene.meshes[g as usize]
                    .attribute(name)
                    .expect("the attribute")
                    .accessor;
                let a = self.array(format!("n{node}.attr.{name}"), acc);
                a[offset..][..values.len()].copy_from_slice(&values);
            }
            (SceneRef::Material(m), Change::Number { prop, value }) => {
                let key = self.material_key(m, prop);
                self.values.insert(key, bits(value));
            }
            (SceneRef::Material(m), Change::Color { prop, rgb }) => {
                let key = self.material_key(m, prop);
                self.values.insert(key, join(&rgb));
            }
            (SceneRef::Material(m), Change::Vector { prop, value }) => {
                let key = self.material_key(m, prop);
                self.values.insert(key, join(&value));
            }
            (SceneRef::Texture(t), Change::TextureOffset(o)) => {
                if let Some(k) = self.tex_key.get(&t) {
                    self.values.insert(format!("{k}.offset"), join(&o));
                }
            }
            (t, c) => panic!("an edit the gate does not fold: {t:?} {c:?}"),
        }
    }

    /// The value of a key, if an edit has written it.
    fn get(&self, key: &str) -> Option<String> {
        if let Some(a) = self.arrays.get(key) {
            let bytes: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
            return Some(format!("sha256:{}", sha256_hex(&bytes)));
        }
        self.values.get(key).cloned()
    }
}

/// One frame as the client runs it: the sky, the night parameters, the
/// animators.
fn frame(wb: &mut WorldBuild, st: &mut State, dt: f64, s: f64, camera: CameraView) {
    let (sky, edits) = wb
        .update_sky(dt, s, Some(camera.position))
        .expect("Sierra has a sky");
    // The sky's dome and lights are not under world.root (the golden holds
    // what is), but a material there could share its uniforms.
    for e in edits {
        st.apply(e);
    }
    for p in &st.scene.night_params {
        let key = st.material_key(p.material, &p.prop);
        st.values
            .insert(key, bits(mp_worldgen::world::night_value(p, sky.night)));
    }
    let u = UpdateCtx {
        dt,
        night: sky.night,
        camera: Some(camera),
        s,
    };
    for e in wb.update(&u) {
        st.apply(e);
    }
}

/// Whether a key addresses something under `world.root` (the scene's
/// first root; the sky's dome and lights are roots of their own).
fn under_root(scene: &Scene) -> impl Fn(&str) -> bool + use<> {
    let mut nodes = vec![false; scene.nodes.len()];
    let mut stack = vec![scene.roots[0]];
    while let Some(n) = stack.pop() {
        nodes[n as usize] = true;
        stack.extend(scene.nodes[n as usize].children.iter().copied());
    }
    let mut mats = vec![false; scene.materials.len()];
    for (i, n) in scene.nodes.iter().enumerate() {
        if nodes[i] {
            for &m in &n.materials {
                mats[m as usize] = true;
            }
        }
    }
    move |key: &str| {
        let id = key.split('.').next().expect("a target");
        let k: usize = id[1..].parse().expect("an index");
        if id.starts_with('n') {
            nodes[k]
        } else {
            mats[k]
        }
    }
}

#[test]
fn sierra_animators() {
    let g = golden();
    let mut wb = build();
    let scene = wb.scene.clone();
    let mut st = State::new(&scene);

    // The targets are the JS's: the same node, the same material.
    let mut problems = Vec::new();
    for (id, what) in g["targets"].as_object().expect("targets") {
        let k: usize = id[1..].parse().expect("an index");
        let ours = if id.starts_with('n') {
            let n = &scene.nodes[k];
            let ty = format!("{:?}", n.ty);
            format!("{ty} {}", n.name).trim().to_string()
        } else {
            let m = &scene.materials[k];
            let kind = format!("{:?}", m.kind);
            let builtin = [
                "Standard", "Physical", "Lambert", "Basic", "Line", "Sprite", "Points",
            ];
            if builtin.contains(&kind.as_str()) {
                m.ty.clone()
            } else {
                format!("{} {kind}", m.ty)
            }
        };
        if ours != what.as_str().expect("a target") {
            problems.push(format!("{id}: ours is {ours:?}, the JS's {what}"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));

    let keys: Vec<&str> = g["keys"]
        .as_array()
        .expect("keys")
        .iter()
        .map(|k| k.as_str().expect("key"))
        .collect();
    let base: Vec<&str> = g["base"]
        .as_array()
        .expect("base")
        .iter()
        .map(|k| k.as_str().expect("value"))
        .collect();
    let cam = |at: &str| CameraView {
        position: [0, 1, 2].map(|i| hex(&g["camera"][at][i])),
        fov: hex(&g["fov"]),
        viewport_height: hex(&g["viewportHeight"]),
    };
    // A value the JS left alone so far and we have not written either is
    // the same; one we write must be the JS's.
    let compare = |st: &State, label: &str, want: &[&str], problems: &mut Vec<String>| {
        for (i, key) in keys.iter().enumerate() {
            match st.get(key) {
                Some(v) if v == want[i] => {}
                None if want[i] == base[i] => {}
                got => problems.push(format!("{label}: {key} = {got:?}, the JS {}", want[i])),
            }
        }
    };

    // What the JS holds still (written by an updater with an unchanging
    // value, so not in the golden) we must hold still too: the first value
    // we write under world.root is kept to the end.
    let under_root = under_root(&scene);
    let mut still: BTreeMap<String, String> = BTreeMap::new();
    let mut check_still = |st: &State, problems: &mut Vec<String>| {
        let written = st.values.keys().chain(st.arrays.keys());
        for k in written {
            if keys.contains(&k.as_str()) || !under_root(k) {
                continue;
            }
            let v = st.get(k).expect("written");
            let first = still.entry(k.clone()).or_insert_with(|| v.clone());
            if *first != v {
                problems.push(format!(
                    "{k}: we change it ({first} to {v}), the JS does not"
                ));
            }
        }
    };

    for (k, f) in g["frames"].as_array().expect("frames").iter().enumerate() {
        let (dt, s) = (hex(&f["dt"]), hex(&f["s"]));
        frame(&mut wb, &mut st, dt, s, cam(f["at"].as_str().expect("at")));
        let want: Vec<&str> = f["values"]
            .as_array()
            .expect("values")
            .iter()
            .map(|v| v.as_str().expect("value"))
            .collect();
        compare(&st, &format!("frame {k}"), &want, &mut problems);
        check_still(&st, &mut problems);
    }
    let unwritten: Vec<&&str> = keys.iter().filter(|k| st.get(k).is_none()).collect();
    assert!(
        unwritten.is_empty(),
        "values the JS animates and the port never writes: {unwritten:?}"
    );

    let t = &g["ticks"];
    let (s0, dt, speed) = (hex(&t["s0"]), hex(&t["dt"]), hex(&t["speed"]));
    for (k, d) in t["digests"].as_array().expect("digests").iter().enumerate() {
        let at = ["flag", "pool", "city"][(k / 40) % 3];
        let s = s0 + k as f64 * dt * speed;
        frame(&mut wb, &mut st, dt, s, cam(at));
        let lines: Vec<String> = keys
            .iter()
            .map(|key| format!("{key}={}", st.get(key).unwrap_or_default()))
            .collect();
        check_still(&st, &mut problems);
        let ours = sha256_hex(lines.join("\n").as_bytes());
        if ours != d.as_str().expect("digest") {
            problems.push(format!("tick {k} (s {s}, camera by the {at}): differs"));
            if problems.len() > 5 {
                break;
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
