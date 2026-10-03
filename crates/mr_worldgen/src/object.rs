//! The object tree a world build makes, as three.js holds it (`Object3D`,
//! `Group`, `Mesh`, `InstancedMesh`, `Points`, ...), and its assembly into
//! an `mr_scene::Scene` (SPEC 5.1; DECISIONS D191).
//!
//! The JS scenery creates objects, geometries, materials and textures, adds
//! them to groups and the groups to `world.scene`; the exporter then walks
//! the tree from its roots. Here a [`SceneGraph`] holds all four as arenas
//! addressed by handle ([`NodeId`], [`GeoId`], [`MaterialId`],
//! [`TextureId`]): a builder or a scenery module makes things and keeps
//! their handles, as the JS keeps references, and an [`Animator`] addresses
//! its edits by them.
//!
//! [`SceneGraph::finish`] walks the tree as the exporter walks it (each
//! root, depth first, children in the order added) and numbers meshes,
//! materials and textures in the order it meets them, so the scene has the
//! layout of a JS export. Whatever no root reaches is left out, as an
//! object never added to the scene is not drawn in the JS. The returned
//! [`HandleMap`] turns a handle into its index in the scene.
//!
//! [`Animator`]: crate::world::Animator

use std::sync::Arc;

use mr_scene::{
    Buffer, BufferData, InstanceDesc, LightDesc, NightParam, NodeDesc, NodeType, Scene, TextureDesc,
};
use serde_json::{Map, Value};

use crate::color::Color;
use crate::material::Material;
use crate::textures::{Cached, Texture};
use crate::three_geom::{BufferGeometry, Matrix4, Quaternion, Sphere, Vector3};

/// An object in a [`SceneGraph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

/// A geometry in a [`SceneGraph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeoId(pub u32);

/// A material in a [`SceneGraph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MaterialId(pub u32);

/// A texture in a [`SceneGraph`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(pub u32);

/// An `InstancedMesh`'s per-instance data.
#[derive(Clone, Debug, PartialEq)]
pub struct Instances {
    /// Instances drawn (`mesh.count`).
    pub count: u32,
    /// `instanceMatrix.array`: `capacity × 16` floats, column-major.
    pub matrices: Vec<f32>,
    /// `instanceColor.array`, once `setColorAt` has made it.
    pub colors: Option<Vec<f32>>,
    /// `mesh.boundingSphere`, once computed.
    pub bounding_sphere: Option<Sphere>,
}

impl Instances {
    /// `new InstancedMesh(geo, mat, count)`'s matrices: every one the
    /// identity.
    pub fn new(count: u32) -> Instances {
        let mut matrices = Vec::with_capacity(count as usize * 16);
        for _ in 0..count {
            matrices.extend(Matrix4::IDENTITY.elements.iter().map(|&v| v as f32));
        }
        Instances {
            count,
            matrices,
            colors: None,
            bounding_sphere: None,
        }
    }

    /// Instances allocated (`instanceMatrix.count`).
    pub fn capacity(&self) -> u32 {
        (self.matrices.len() / 16) as u32
    }

    /// `setMatrixAt(i, m)`: stored as `f32`.
    pub fn set_matrix_at(&mut self, i: usize, m: &Matrix4) {
        for (k, v) in m.elements.iter().enumerate() {
            self.matrices[i * 16 + k] = *v as f32;
        }
    }

    /// `getMatrixAt(i)`.
    pub fn get_matrix_at(&self, i: usize) -> Matrix4 {
        let mut elements = [0.0; 16];
        for (k, e) in elements.iter_mut().enumerate() {
            *e = f64::from(self.matrices[i * 16 + k]);
        }
        Matrix4 { elements }
    }

    /// `setColorAt(i, c)`: the first call makes the colour array, white.
    pub fn set_color_at(&mut self, i: usize, c: Color) {
        let cap = self.capacity() as usize;
        let colors = self.colors.get_or_insert_with(|| vec![1.0; cap * 3]);
        colors[i * 3] = c.r as f32;
        colors[i * 3 + 1] = c.g as f32;
        colors[i * 3 + 2] = c.b as f32;
    }
}

/// An `Object3D` and what its subclass adds.
#[derive(Clone, Debug, PartialEq)]
pub struct Object3D {
    pub name: String,
    pub ty: NodeType,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub position: Vector3,
    /// The rotation (three keeps `rotation` and `quaternion` in step; set
    /// it with [`Object3D::set_rotation`] for an Euler angle).
    pub quaternion: Quaternion,
    pub scale: Vector3,
    /// The local matrix as last updated: what `matrixAutoUpdate = false`
    /// objects keep. Auto-updated objects are composed afresh at assembly.
    pub matrix: Matrix4,
    pub matrix_auto_update: bool,
    pub visible: bool,
    pub frustum_culled: bool,
    pub render_order: f64,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
    /// `layers.mask`.
    pub layers: u32,
    /// The plain values of `userData`.
    pub user_data: Map<String, Value>,
    /// Meshes, points, lines and sprites.
    pub geometry: Option<GeoId>,
    pub materials: Vec<MaterialId>,
    /// `Array.isArray(material)`.
    pub multi_material: bool,
    pub instances: Option<Instances>,
    /// A sprite's centre.
    pub center: Option<[f64; 2]>,
    /// A light (`node` is filled in at assembly).
    pub light: Option<LightDesc>,
}

impl Object3D {
    /// `new Object3D()` of the given type, with three's defaults.
    pub fn new(ty: NodeType) -> Object3D {
        Object3D {
            name: String::new(),
            ty,
            parent: None,
            children: Vec::new(),
            position: Vector3::new(0.0, 0.0, 0.0),
            quaternion: Quaternion::IDENTITY,
            scale: Vector3::splat(1.0),
            matrix: Matrix4::IDENTITY,
            matrix_auto_update: true,
            visible: true,
            frustum_culled: true,
            render_order: 0.0,
            cast_shadow: false,
            receive_shadow: false,
            layers: 1,
            user_data: Map::new(),
            geometry: None,
            materials: Vec::new(),
            multi_material: false,
            instances: None,
            center: None,
            light: None,
        }
    }

    /// `rotation.set(x, y, z, order)`: the quaternion follows.
    pub fn set_rotation(&mut self, e: &crate::three_geom::Euler) {
        self.quaternion = Quaternion::from_euler(e);
    }

    /// `updateMatrix()`.
    pub fn update_matrix(&mut self) {
        self.matrix = Matrix4::compose(self.position, self.quaternion, self.scale);
    }
}

/// Which picture of a cached texture a [`TextureEntry`] shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The texture (`Cached::texture`, a façade's `map`).
    Main,
    /// A façade's `emissive`.
    Emissive,
}

/// Where a texture's pixels come from. Textures that share pixels (a JS
/// `texture.clone()` with another `repeat`) share one buffer in the scene.
#[derive(Clone)]
pub enum Image {
    Cached(Arc<Cached>, Layer),
    Own(Arc<Texture>),
}

impl Image {
    fn texture(&self) -> &Texture {
        match self {
            Image::Own(t) => t,
            Image::Cached(c, Layer::Main) => c.texture(),
            Image::Cached(c, Layer::Emissive) => match &**c {
                Cached::Facade { emissive, .. } => emissive,
                other => other.texture(),
            },
        }
    }

    fn same(&self, other: &Image) -> bool {
        match (self, other) {
            (Image::Own(a), Image::Own(b)) => Arc::ptr_eq(a, b),
            (Image::Cached(a, la), Image::Cached(b, lb)) => Arc::ptr_eq(a, b) && la == lb,
            _ => false,
        }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let t = self.texture();
        write!(f, "Image({}×{})", t.width, t.height)
    }
}

/// A texture: its pixels and its sampler (the `TextureDesc`, whose
/// `pixels` the assembly fills in).
#[derive(Clone, Debug)]
pub struct TextureEntry {
    pub image: Image,
    pub desc: TextureDesc,
}

/// A material property that follows nightfall (`world.addNight`).
#[derive(Clone, Debug, PartialEq)]
pub struct NightMaterial {
    pub material: MaterialId,
    pub prop: String,
    pub day: f64,
    pub night: f64,
}

/// Handle → index in the assembled scene; `None` for what no root
/// reaches.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HandleMap {
    pub nodes: Vec<Option<u32>>,
    pub meshes: Vec<Option<u32>>,
    pub materials: Vec<Option<u32>>,
    pub textures: Vec<Option<u32>>,
}

impl HandleMap {
    pub fn node(&self, id: NodeId) -> Option<u32> {
        self.nodes.get(id.0 as usize).copied().flatten()
    }
    pub fn mesh(&self, id: GeoId) -> Option<u32> {
        self.meshes.get(id.0 as usize).copied().flatten()
    }
    pub fn material(&self, id: MaterialId) -> Option<u32> {
        self.materials.get(id.0 as usize).copied().flatten()
    }
    pub fn texture(&self, id: TextureId) -> Option<u32> {
        self.textures.get(id.0 as usize).copied().flatten()
    }
}

/// How far the handles of a graph appended to another moved
/// ([`SceneGraph::append`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bases {
    pub nodes: u32,
    pub geometries: u32,
    pub materials: u32,
    pub textures: u32,
}

impl Bases {
    pub fn node(&self, id: NodeId) -> NodeId {
        NodeId(id.0 + self.nodes)
    }
    pub fn geometry(&self, id: GeoId) -> GeoId {
        GeoId(id.0 + self.geometries)
    }
    pub fn material(&self, id: MaterialId) -> MaterialId {
        MaterialId(id.0 + self.materials)
    }
    pub fn texture(&self, id: TextureId) -> TextureId {
        TextureId(id.0 + self.textures)
    }
}

/// Objects, geometries, materials and textures being built.
#[derive(Clone, Debug, Default)]
pub struct SceneGraph {
    pub objects: Vec<Object3D>,
    pub geometries: Vec<BufferGeometry>,
    pub materials: Vec<Material>,
    pub textures: Vec<TextureEntry>,
    /// Objects added to the scene itself, in order.
    pub roots: Vec<NodeId>,
    pub night: Vec<NightMaterial>,
    /// What the JS would `console.warn` or `console.error` while building
    /// (a missing material, a scenery module that failed).
    pub log: Vec<String>,
}

impl SceneGraph {
    pub fn new() -> SceneGraph {
        SceneGraph::default()
    }

    // ── Making things ───────────────────────────────────────────────────

    /// Adds an object (not yet in the tree; see [`SceneGraph::add`]).
    pub fn object(&mut self, o: Object3D) -> NodeId {
        self.objects.push(o);
        NodeId(self.objects.len() as u32 - 1)
    }

    /// `new THREE.Group()`, named.
    pub fn group(&mut self, name: &str) -> NodeId {
        let mut o = Object3D::new(NodeType::Group);
        o.name = name.to_string();
        self.object(o)
    }

    /// `new THREE.Mesh(geo, mat)`.
    pub fn mesh(&mut self, geo: GeoId, mat: MaterialId) -> NodeId {
        self.drawable(NodeType::Mesh, geo, mat)
    }

    /// `new THREE.Points(geo, mat)`, `LineSegments`, `Sprite` and the
    /// rest: an object drawing one geometry with one material.
    pub fn drawable(&mut self, ty: NodeType, geo: GeoId, mat: MaterialId) -> NodeId {
        let mut o = Object3D::new(ty);
        o.geometry = Some(geo);
        o.materials = vec![mat];
        if ty == NodeType::Sprite {
            o.center = Some([0.5, 0.5]);
        }
        self.object(o)
    }

    /// `new THREE.Mesh(geo, [mat, ...])`: one material per group.
    pub fn multi_mesh(&mut self, geo: GeoId, mats: &[MaterialId]) -> NodeId {
        let mut o = Object3D::new(NodeType::Mesh);
        o.geometry = Some(geo);
        o.materials = mats.to_vec();
        o.multi_material = true;
        self.object(o)
    }

    /// `new THREE.InstancedMesh(geo, mat, count)`.
    pub fn instanced_mesh(&mut self, geo: GeoId, mat: MaterialId, count: u32) -> NodeId {
        let mut o = Object3D::new(NodeType::InstancedMesh);
        o.geometry = Some(geo);
        o.materials = vec![mat];
        o.instances = Some(Instances::new(count));
        self.object(o)
    }

    pub fn add_geometry(&mut self, g: BufferGeometry) -> GeoId {
        self.geometries.push(g);
        GeoId(self.geometries.len() as u32 - 1)
    }

    pub fn add_material(&mut self, m: Material) -> MaterialId {
        self.materials.push(m);
        MaterialId(self.materials.len() as u32 - 1)
    }

    /// A texture showing `image` with the sampler `desc` (whose `pixels`
    /// is ignored).
    pub fn add_texture(&mut self, image: Image, desc: TextureDesc) -> TextureId {
        self.textures.push(TextureEntry { image, desc });
        TextureId(self.textures.len() as u32 - 1)
    }

    /// A texture from the cache as the JS module hands it out (its own
    /// sampler, named `name`).
    pub fn cached_texture(&mut self, c: &Arc<Cached>, layer: Layer, name: &str) -> TextureId {
        let image = Image::Cached(c.clone(), layer);
        let desc = image.texture().desc(name, 0);
        self.add_texture(image, desc)
    }

    /// `texture.clone()`: the same pixels, a sampler of its own.
    pub fn clone_texture(&mut self, t: TextureId) -> TextureId {
        let e = self.textures[t.0 as usize].clone();
        self.textures.push(e);
        TextureId(self.textures.len() as u32 - 1)
    }

    // ── The tree ────────────────────────────────────────────────────────

    pub fn get(&self, id: NodeId) -> &Object3D {
        &self.objects[id.0 as usize]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut Object3D {
        &mut self.objects[id.0 as usize]
    }

    pub fn geometry(&self, id: GeoId) -> &BufferGeometry {
        &self.geometries[id.0 as usize]
    }

    pub fn geometry_mut(&mut self, id: GeoId) -> &mut BufferGeometry {
        &mut self.geometries[id.0 as usize]
    }

    pub fn material(&self, id: MaterialId) -> &Material {
        &self.materials[id.0 as usize]
    }

    pub fn material_mut(&mut self, id: MaterialId) -> &mut Material {
        &mut self.materials[id.0 as usize]
    }

    pub fn texture_mut(&mut self, id: TextureId) -> &mut TextureEntry {
        &mut self.textures[id.0 as usize]
    }

    /// `parent.add(child)`: a child that has a parent leaves it first.
    pub fn add(&mut self, parent: NodeId, child: NodeId) {
        assert_ne!(
            parent, child,
            "an object can't be added as a child of itself"
        );
        self.detach(child);
        self.objects[child.0 as usize].parent = Some(parent);
        self.objects[parent.0 as usize].children.push(child);
    }

    /// `scene.add(object)`: a root of the scene.
    pub fn add_root(&mut self, id: NodeId) {
        self.detach(id);
        self.roots.push(id);
    }

    /// `object.removeFromParent()` (or `scene.remove(object)`).
    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.objects[id.0 as usize].parent.take() {
            self.objects[p.0 as usize].children.retain(|&c| c != id);
        }
        self.roots.retain(|&r| r != id);
    }

    /// `world.addNight(material, prop, day, night)`.
    pub fn add_night(&mut self, material: Option<MaterialId>, prop: &str, day: f64, night: f64) {
        if let Some(material) = material {
            self.night.push(NightMaterial {
                material,
                prop: prop.to_string(),
                day,
                night,
            });
        }
    }

    // ── Bounds ──────────────────────────────────────────────────────────

    /// `InstancedMesh.computeBoundingSphere()`: the geometry's sphere
    /// (computed first if it has none) through each drawn instance's
    /// matrix, merged.
    pub fn compute_instance_bounding_sphere(&mut self, id: NodeId) {
        let geo = self.objects[id.0 as usize]
            .geometry
            .expect("an instanced mesh has a geometry");
        let g = &mut self.geometries[geo.0 as usize];
        if g.bounding_sphere.is_none() {
            g.compute_bounding_sphere();
        }
        let gs = g.bounding_sphere.expect("computed");
        let inst = self.objects[id.0 as usize]
            .instances
            .as_mut()
            .expect("an instanced mesh");
        let mut bs = Sphere::default();
        for i in 0..inst.count as usize {
            let m = inst.get_matrix_at(i);
            let s = sphere_apply_matrix4(gs, &m);
            bs = sphere_union(bs, s);
        }
        inst.bounding_sphere = Some(bs);
    }

    /// `InstancedMesh.computeBoundingBox()`. The instance box itself is not
    /// part of a scene; what shows is its side effect, the geometry's box
    /// computed if it had none.
    pub fn compute_instance_bounding_box(&mut self, id: NodeId) {
        let geo = self.objects[id.0 as usize]
            .geometry
            .expect("an instanced mesh has a geometry");
        let g = &mut self.geometries[geo.0 as usize];
        if g.bounding_box.is_none() {
            g.compute_bounding_box();
        }
    }

    // ── Merging and assembly ────────────────────────────────────────────

    /// Appends `other`'s contents, renumbering its handles; its roots are
    /// added to `parent` (or become roots). Returns the offsets.
    pub fn append(&mut self, other: SceneGraph, parent: Option<NodeId>) -> Bases {
        let b = Bases {
            nodes: self.objects.len() as u32,
            geometries: self.geometries.len() as u32,
            materials: self.materials.len() as u32,
            textures: self.textures.len() as u32,
        };
        for mut o in other.objects {
            o.parent = o.parent.map(|p| b.node(p));
            o.children = o.children.iter().map(|&c| b.node(c)).collect();
            o.geometry = o.geometry.map(|g| b.geometry(g));
            o.materials = o.materials.iter().map(|&m| b.material(m)).collect();
            self.objects.push(o);
        }
        self.geometries.extend(other.geometries);
        for mut m in other.materials {
            for v in m.desc.params.values_mut() {
                shift_textures(v, b.textures);
            }
            if let Some(u) = &mut m.desc.uniforms {
                for v in u.values_mut() {
                    shift_textures(v, b.textures);
                }
            }
            self.materials.push(m);
        }
        self.textures.extend(other.textures);
        for r in other.roots {
            match parent {
                Some(p) => self.add(p, b.node(r)),
                None => self.roots.push(b.node(r)),
            }
        }
        self.night.extend(other.night.into_iter().map(|mut n| {
            n.material = b.material(n.material);
            n
        }));
        self.log.extend(other.log);
        b
    }

    /// The scene, walked from the roots as the exporter walks the JS
    /// scene, and the map from handles to its indices.
    pub fn finish(&self) -> (Scene, HandleMap) {
        let mut a = Assembly {
            g: self,
            scene: Scene::default(),
            map: HandleMap {
                nodes: vec![None; self.objects.len()],
                meshes: vec![None; self.geometries.len()],
                materials: vec![None; self.materials.len()],
                textures: vec![None; self.textures.len()],
            },
            pixels: Vec::new(),
        };
        for &r in &self.roots {
            let i = a.node(r, None, &Matrix4::IDENTITY);
            a.scene.roots.push(i);
        }
        for n in &self.night {
            // Night parameters of materials nothing draws are left out
            // (D24).
            if let Some(material) = a.map.material(n.material) {
                a.scene.night_params.push(NightParam {
                    material,
                    prop: n.prop.clone(),
                    day: n.day,
                    night: n.night,
                });
            }
        }
        (a.scene, a.map)
    }
}

fn shift_textures(v: &mut Value, by: u32) {
    match v {
        Value::Object(o) => {
            if o.len() == 1
                && let Some(t) = o.get("texture").and_then(Value::as_u64)
            {
                o.insert("texture".into(), Value::from(t + u64::from(by)));
                return;
            }
            o.values_mut().for_each(|x| shift_textures(x, by));
        }
        Value::Array(a) => a.iter_mut().for_each(|x| shift_textures(x, by)),
        _ => {}
    }
}

struct Assembly<'a> {
    g: &'a SceneGraph,
    scene: Scene,
    map: HandleMap,
    /// Pixel buffers by image, shared.
    pixels: Vec<(Image, u32)>,
}

impl Assembly<'_> {
    fn buffer(&mut self, item_size: u32, data: BufferData) -> u32 {
        self.scene.buffers.push(Buffer {
            item_size,
            normalized: false,
            data,
        });
        self.scene.buffers.len() as u32 - 1
    }

    fn node(&mut self, id: NodeId, parent: Option<u32>, parent_world: &Matrix4) -> u32 {
        let o = &self.g.objects[id.0 as usize];
        let matrix = if o.matrix_auto_update {
            Matrix4::compose(o.position, o.quaternion, o.scale)
        } else {
            o.matrix
        };
        let world = if parent.is_some() {
            Matrix4::multiply_matrices(parent_world, &matrix)
        } else {
            matrix
        };
        let idx = self.scene.nodes.len() as u32;
        self.map.nodes[id.0 as usize] = Some(idx);
        self.scene.nodes.push(NodeDesc {
            name: o.name.clone(),
            ty: o.ty,
            parent,
            children: Vec::new(),
            matrix: matrix.elements,
            matrix_world: world.elements,
            visible: o.visible,
            matrix_auto_update: o.matrix_auto_update,
            frustum_culled: o.frustum_culled,
            render_order: o.render_order,
            cast_shadow: o.cast_shadow,
            receive_shadow: o.receive_shadow,
            layers: o.layers,
            user_data: o.user_data.clone(),
            mesh: None,
            materials: Vec::new(),
            multi_material: None,
            instances: None,
            center: None,
            light: None,
        });
        if let Some(p) = parent {
            self.scene.nodes[p as usize].children.push(idx);
        }
        if let Some(geo) = o.geometry {
            let mesh = self.mesh(geo);
            let materials: Vec<u32> = o.materials.iter().map(|&m| self.material(m)).collect();
            let n = &mut self.scene.nodes[idx as usize];
            n.mesh = Some(mesh);
            n.materials = materials;
            n.multi_material = Some(o.multi_material);
        }
        if let Some(inst) = &o.instances {
            let matrices = self.buffer(16, BufferData::F32(inst.matrices.clone()));
            let colors = inst
                .colors
                .as_ref()
                .map(|c| self.buffer(3, BufferData::F32(c.clone())));
            self.scene.instances.push(InstanceDesc {
                node: idx,
                count: inst.count,
                capacity: inst.capacity(),
                matrices,
                colors,
                bounding_sphere: inst
                    .bounding_sphere
                    .map(|s| vec![s.center.x, s.center.y, s.center.z, s.radius]),
            });
            self.scene.nodes[idx as usize].instances = Some(self.scene.instances.len() as u32 - 1);
        }
        if o.ty == NodeType::Sprite {
            self.scene.nodes[idx as usize].center = o.center;
        }
        if let Some(light) = &o.light {
            let mut l = light.clone();
            l.node = idx;
            self.scene.lights.push(l);
            self.scene.nodes[idx as usize].light = Some(self.scene.lights.len() as u32 - 1);
        }
        for &c in &o.children {
            self.node(c, Some(idx), &world);
        }
        idx
    }

    fn mesh(&mut self, id: GeoId) -> u32 {
        if let Some(i) = self.map.meshes[id.0 as usize] {
            return i;
        }
        let i = self.g.geometries[id.0 as usize].add_to_scene(&mut self.scene, "");
        self.map.meshes[id.0 as usize] = Some(i);
        i
    }

    fn material(&mut self, id: MaterialId) -> u32 {
        if let Some(i) = self.map.materials[id.0 as usize] {
            return i;
        }
        let mut desc = self.g.materials[id.0 as usize].desc.clone();
        // Textures are numbered as the exporter meets them: parameters in
        // order, then uniforms.
        for v in desc.params.values_mut() {
            self.renumber(v);
        }
        if let Some(u) = &mut desc.uniforms {
            for v in u.values_mut() {
                self.renumber(v);
            }
        }
        self.scene.materials.push(desc);
        let i = self.scene.materials.len() as u32 - 1;
        self.map.materials[id.0 as usize] = Some(i);
        i
    }

    fn renumber(&mut self, v: &mut Value) {
        match v {
            Value::Object(o) => {
                if o.len() == 1
                    && let Some(t) = o.get("texture").and_then(Value::as_u64)
                {
                    let i = self.texture(TextureId(t as u32));
                    o.insert("texture".into(), Value::from(i));
                    return;
                }
                o.values_mut().for_each(|x| self.renumber(x));
            }
            Value::Array(a) => a.iter_mut().for_each(|x| self.renumber(x)),
            _ => {}
        }
    }

    fn texture(&mut self, id: TextureId) -> u32 {
        if let Some(i) = self.map.textures[id.0 as usize] {
            return i;
        }
        let e = &self.g.textures[id.0 as usize];
        let pixels = match self.pixels.iter().find(|(im, _)| im.same(&e.image)) {
            Some((_, p)) => *p,
            None => {
                let t = e.image.texture();
                // One component per channel: RGBA, or R8 (Seaside's loose
                // ground mask).
                let p = self.buffer(e.desc.channels, BufferData::U8(t.rgba.clone()));
                self.pixels.push((e.image.clone(), p));
                p
            }
        };
        let mut desc = e.desc.clone();
        desc.pixels = pixels;
        self.scene.textures.push(desc);
        let i = self.scene.textures.len() as u32 - 1;
        self.map.textures[id.0 as usize] = Some(i);
        i
    }
}

// three's `Sphere` operations the instanced bounds use.

/// `Matrix4.getMaxScaleOnAxis()`.
pub fn max_scale_on_axis(m: &Matrix4) -> f64 {
    let te = &m.elements;
    let scale_x_sq = te[0] * te[0] + te[1] * te[1] + te[2] * te[2];
    let scale_y_sq = te[4] * te[4] + te[5] * te[5] + te[6] * te[6];
    let scale_z_sq = te[8] * te[8] + te[9] * te[9] + te[10] * te[10];
    f64::sqrt(mr_math::js::max_n(&[scale_x_sq, scale_y_sq, scale_z_sq]))
}

/// `Sphere.applyMatrix4(m)`.
pub fn sphere_apply_matrix4(s: Sphere, m: &Matrix4) -> Sphere {
    Sphere {
        center: s.center.apply_matrix4(m),
        radius: s.radius * max_scale_on_axis(m),
    }
}

/// `Sphere.expandByPoint(p)`.
pub fn sphere_expand_by_point(mut s: Sphere, point: Vector3) -> Sphere {
    if s.radius < 0.0 {
        s.center = point;
        s.radius = 0.0;
        return s;
    }
    let v1 = point - s.center;
    let length_sq = v1.length_sq();
    if length_sq > (s.radius * s.radius) {
        // calculate the minimal sphere
        let length = f64::sqrt(length_sq);
        let delta = (length - s.radius) * 0.5;
        s.center = s.center.add_scaled_vector(v1, delta / length);
        s.radius += delta;
    }
    s
}

/// `Sphere.union(other)`.
pub fn sphere_union(s: Sphere, other: Sphere) -> Sphere {
    if other.radius < 0.0 {
        return s;
    }
    if s.radius < 0.0 {
        return other;
    }
    if s.center.equals(other.center) {
        Sphere {
            center: s.center,
            radius: mr_math::js::max(s.radius, other.radius),
        }
    } else {
        let v2 = (other.center - s.center).set_length(other.radius);
        let s = sphere_expand_by_point(s, other.center + v2);
        sphere_expand_by_point(s, other.center - v2)
    }
}
