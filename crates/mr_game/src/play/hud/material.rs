//! The HUD's one UI material (`hud.wgsl`): the dial, the minimap and the
//! speed lines, told apart by `head.x`. One type, so Bevy builds one
//! pipeline and the wasm carries one copy of the material plugin.
//!
//! The uniforms change most frames (the dial with the revs, the minimap
//! with the car), and an edited Bevy material is prepared again: a new
//! uniform buffer and bind group each time (D455's cost). So each of the
//! three materials is prepared once over a uniform buffer of its own,
//! kept by the render world, and new uniforms are written into it in place
//! ([`HudWrites`], DECISIONS D865): the same bytes in the same frame as
//! the edit would have given.

use super::dials::{DialDraw, Fill};
use super::minimap::{MAX_ROAD, MAX_SHAPES, Scene, Shape};
use bevy::ecs::system::SystemParamItem;
use bevy::ecs::system::lifetimeless::SRes;
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, AsBindGroupError, BindGroupLayout, BindGroupLayoutEntry, BindingResources, Buffer,
    BufferInitDescriptor, BufferUsages, OwnedBindingResource, ShaderType, UnpreparedBindGroup,
    encase,
};
use bevy::render::renderer::{RenderDevice, RenderQueue};
use bevy::render::{MainWorld, Render, RenderApp, RenderSystems};
use bevy::shader::ShaderRef;
use bevy::ui_render::prelude::UiMaterial;
use std::sync::Mutex;

pub const DIAL: f32 = 1.0;
pub const MINIMAP: f32 = 2.0;
pub const SPEEDLINES: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub struct HudParams {
    pub head: Vec4,
    pub v: [Vec4; 8],
    pub road: [Vec4; MAX_ROAD / 2],
    pub shapes: [Vec4; MAX_SHAPES * 4],
}

impl HudParams {
    pub fn new(kind: f32) -> HudParams {
        HudParams {
            head: Vec4::new(kind, 0.0, 0.0, 0.0),
            v: [Vec4::ZERO; 8],
            road: [Vec4::ZERO; MAX_ROAD / 2],
            shapes: [Vec4::ZERO; MAX_SHAPES * 4],
        }
    }
}

/// One of the three materials: its first uniforms (`p.head.x` names which
/// one); later ones go through [`HudWrites`], not by editing the asset.
#[derive(Asset, TypePath, Clone, Debug)]
pub struct HudMaterial {
    pub p: HudParams,
}

/// The layout the derive gives one `#[uniform(0)]` field, which
/// [`HudMaterial`] keeps.
#[derive(AsBindGroup)]
struct HudLayout {
    #[uniform(0)]
    _p: HudParams,
}

/// The material's slot (0 dial, 1 minimap, 2 speed lines) from its kind.
fn slot(p: &HudParams) -> usize {
    (p.head.x as usize).clamp(1, 3) - 1
}

/// The uniforms' bytes, as the derive writes them.
fn bytes(p: &HudParams) -> Vec<u8> {
    let mut b = encase::UniformBuffer::new(Vec::new());
    b.write(p).expect("HUD uniforms");
    b.into_inner()
}

/// Render world: the materials' uniform buffers, one per slot, made when
/// the material is first prepared.
#[derive(Resource, Default)]
pub struct HudBuffers(Mutex<[Option<Buffer>; 3]>);

impl AsBindGroup for HudMaterial {
    type Data = ();
    type Param = SRes<HudBuffers>;

    fn label() -> &'static str {
        "hud_material"
    }

    fn bind_group_data(&self) -> Self::Data {}

    fn unprepared_bind_group(
        &self,
        _layout: &BindGroupLayout,
        render_device: &RenderDevice,
        buffers: &mut SystemParamItem<'_, '_, Self::Param>,
        _force_no_bindless: bool,
    ) -> Result<UnpreparedBindGroup, AsBindGroupError> {
        let mut all = buffers.0.lock().unwrap_or_else(|e| e.into_inner());
        let buffer = all[slot(&self.p)]
            .get_or_insert_with(|| {
                render_device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("hud_material"),
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    contents: &bytes(&self.p),
                })
            })
            .clone();
        Ok(UnpreparedBindGroup {
            bindings: BindingResources(vec![(0, OwnedBindingResource::Buffer(buffer))]),
        })
    }

    fn bind_group_layout_entries(
        render_device: &RenderDevice,
        force_no_bindless: bool,
    ) -> Vec<BindGroupLayoutEntry> {
        HudLayout::bind_group_layout_entries(render_device, force_no_bindless)
    }
}

impl UiMaterial for HudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://mr_game/play/hud/hud.wgsl".into()
    }
}

/// Main world: new uniforms for the materials this frame, by the
/// material's first uniforms' slot.
#[derive(Resource, Default)]
pub struct HudWrites(Vec<(usize, HudParams)>);

impl HudWrites {
    /// New uniforms for `material` (as it was made), `p` with its kind.
    pub fn set(&mut self, material: &HudMaterial, p: HudParams) {
        let s = slot(&material.p);
        self.0.retain(|(k, _)| *k != s);
        self.0.push((s, p));
    }
}

/// Render world: writes waiting for their buffer.
#[derive(Resource, Default)]
struct PendingHud(Vec<(usize, HudParams)>);

fn extract_writes(mut main: ResMut<MainWorld>, mut pending: ResMut<PendingHud>) {
    let Some(mut w) = main.get_resource_mut::<HudWrites>() else {
        return;
    };
    for (s, p) in w.0.drain(..) {
        pending.0.retain(|(k, _)| *k != s);
        pending.0.push((s, p));
    }
}

/// After the materials are prepared (`PrepareAssets`), so a buffer made
/// this frame takes this frame's uniforms too.
fn write_uniforms(
    buffers: Res<HudBuffers>,
    mut pending: ResMut<PendingHud>,
    queue: Res<RenderQueue>,
) {
    if pending.0.is_empty() {
        return;
    }
    let all = buffers.0.lock().unwrap_or_else(|e| e.into_inner());
    pending.0.retain(|(s, p)| match &all[*s] {
        Some(b) => {
            queue.write_buffer(b, 0, &bytes(p));
            false
        }
        None => true,
    });
}

pub fn plugin(app: &mut App) {
    bevy::asset::embedded_asset!(app, "hud.wgsl");
    app.init_resource::<HudWrites>()
        .add_plugins(bevy::ui_render::UiMaterialPlugin::<HudMaterial>::default());
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app
            .init_resource::<HudBuffers>()
            .init_resource::<PendingHud>()
            .add_systems(bevy::render::ExtractSchedule, extract_writes)
            .add_systems(
                Render,
                write_uniforms.in_set(RenderSystems::PrepareResources),
            );
    }
}

fn srgb(c: u32) -> Vec3 {
    Vec3::new(
        ((c >> 16) & 255) as f32 / 255.0,
        ((c >> 8) & 255) as f32 / 255.0,
        (c & 255) as f32 / 255.0,
    )
}

/// The dial's uniforms.
pub fn dial(d: &DialDraw) -> HudParams {
    let mut p = HudParams::new(DIAL);
    let (solid, mid, stops) = match d.paint {
        Fill::Grad(g) => (0.0, g.mid as f32, g.c),
        Fill::Solid(c) => (1.0, 0.5, [c, c, c]),
    };
    p.v[0] = Vec4::new(d.fill.0 as f32, d.fill.1 as f32, solid, mid);
    p.v[1] = Vec4::new(d.zone.0 as f32, d.zone.1 as f32, 0.0, d.zone.3 as f32);
    p.v[2] = srgb(d.zone.2).extend(1.0);
    for (i, c) in stops.iter().enumerate() {
        p.v[3 + i] = srgb(*c).extend(1.0);
    }
    p.v[6] = Vec4::new(d.ticks.0 as f32, d.ticks.1 as f32, d.ticks.2 as f32, 0.0);
    p
}

/// The minimap's uniforms.
pub fn minimap(sc: &Scene) -> HudParams {
    let mut p = HudParams::new(MINIMAP);
    let n = sc.road.len().min(MAX_ROAD);
    for (i, &(x, y)) in sc.road.iter().take(n).enumerate() {
        let v = &mut p.road[i / 2];
        if i % 2 == 0 {
            v.x = x as f32;
            v.y = y as f32;
        } else {
            v.z = x as f32;
            v.w = y as f32;
        }
    }
    let m = sc.shapes.len().min(MAX_SHAPES);
    for (i, s) in sc.shapes.iter().take(m).enumerate() {
        let o = i * 4;
        match *s {
            Shape::Dot {
                x,
                y,
                r,
                fill,
                stroke,
            } => {
                p.shapes[o] = Vec4::new(x as f32, y as f32, r as f32, 0.0);
                let (sw, sc) = stroke.unwrap_or((0.0, [0.0; 4]));
                p.shapes[o + 1] = Vec4::new(0.0, sw as f32, 0.0, 0.0);
                p.shapes[o + 2] = Vec4::from_array(fill);
                p.shapes[o + 3] = Vec4::from_array(sc);
            }
            Shape::Bar { a, b, w, color } => {
                p.shapes[o] = Vec4::new(a.0 as f32, a.1 as f32, b.0 as f32, b.1 as f32);
                p.shapes[o + 1] = Vec4::new(1.0, w as f32, 0.0, 0.0);
                p.shapes[o + 2] = Vec4::from_array(color);
            }
        }
    }
    p.head = Vec4::new(MINIMAP, n as f32, m as f32, 0.0);
    p
}

/// The speed lines' uniforms.
pub fn speedlines(opacity: f64) -> HudParams {
    let mut p = HudParams::new(SPEEDLINES);
    p.head.y = opacity as f32;
    p
}
