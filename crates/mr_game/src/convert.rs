//! `mr_scene` data to Bevy assets: meshes and images (roadmap WP 2.2), and
//! how each material kind draws for now ([`stand_in`]). Pure conversions, no
//! ECS; `loader` spawns the results, `render::material` makes the materials.
//!
//! Every asset is created with `RenderAssetUsages::RENDER_WORLD`, so Bevy
//! drops the CPU copy once it is on the GPU.
//!
//! The plain kinds draw with three_std (`render::material`); a patched kind
//! draws as the plain version of its built-in type, as a stand-in, until its
//! patch is ported (SPEC 6.2, roadmap WP 2.4). The `ShaderMaterial` effects
//! other than the sky draw nothing for now ([`StandIn::Hidden`]).

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::{Affine2, Mat4};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use mr_scene::{
    Buffer, BufferData, MaterialDesc, MaterialKind, MeshDesc, NodeType, Scene, TextureDesc, three,
};

// ── Buffers ────────────────────────────────────────────────────────────

/// Component `j` of element `i` as the GPU reads it: normalised integers to
/// 0..1 or -1..1 (WebGL's rules, which three relies on).
fn component(b: &Buffer, i: usize, j: usize) -> f32 {
    let k = i * b.item_size as usize + j;
    if !b.normalized {
        return b.data.get(k) as f32;
    }
    let x = b.data.get(k);
    (match b.data {
        BufferData::U8(_) => x / 255.0,
        BufferData::U16(_) => x / 65535.0,
        BufferData::U32(_) => x / 4294967295.0,
        BufferData::I8(_) => (x / 127.0).max(-1.0),
        BufferData::I16(_) => (x / 32767.0).max(-1.0),
        BufferData::I32(_) => (x / 2147483647.0).max(-1.0),
        BufferData::F32(_) | BufferData::F64(_) => x,
    }) as f32
}

/// The buffer as `N`-component items; missing components are `pad`.
fn items<const N: usize>(b: &Buffer, pad: f32) -> Vec<[f32; N]> {
    let n = b.count();
    let have = (b.item_size as usize).min(N);
    if let (BufferData::F32(v), 3, false) = (&b.data, b.item_size, b.normalized)
        && N == 3
    {
        // The common case, without the per-element dispatch.
        return v
            .as_chunks::<3>()
            .0
            .iter()
            .map(|c| {
                let mut o = [pad; N];
                o[..3].copy_from_slice(c);
                o
            })
            .collect();
    }
    (0..n)
        .map(|i| {
            let mut o = [pad; N];
            for (j, slot) in o.iter_mut().enumerate().take(have) {
                *slot = component(b, i, j);
            }
            o
        })
        .collect()
}

fn index_values(b: &Buffer) -> Vec<u32> {
    match &b.data {
        BufferData::U16(v) => v.iter().map(|&x| u32::from(x)).collect(),
        BufferData::U32(v) => v.clone(),
        other => (0..other.len()).map(|i| other.get(i) as u32).collect(),
    }
}

// ── Meshes ─────────────────────────────────────────────────────────────

/// How a node draws its geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Draw {
    Triangles,
    /// `LineSegments`.
    Lines,
    /// `Line`: a strip, drawn as a list.
    LineStrip,
    /// `LineLoop`: a closed strip, drawn as a list.
    LineLoop,
    /// `Points`: one pixel each for now (WP 2.4 makes them quads).
    Points,
}

impl Draw {
    pub fn of(ty: NodeType) -> Option<Draw> {
        match ty {
            NodeType::Mesh | NodeType::InstancedMesh => Some(Draw::Triangles),
            NodeType::LineSegments => Some(Draw::Lines),
            NodeType::Line => Some(Draw::LineStrip),
            NodeType::LineLoop => Some(Draw::LineLoop),
            NodeType::Points => Some(Draw::Points),
            _ => None,
        }
    }
}

/// One Bevy mesh made from a geometry: which part, and which attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshKey {
    pub mesh: u32,
    /// Element range (indices if indexed, vertices if not): start, count.
    pub start: u32,
    pub count: u32,
    pub draw: Draw,
    /// The material reads vertex colours (`vertexColors: true`).
    pub colors: bool,
    /// The material is lit, so normals are computed if the geometry has none.
    pub lit: bool,
}

/// The element range three draws for a node's geometry, or one group of it:
/// the draw range intersected with the group (`WebGLRenderer.renderBufferDirect`).
pub fn draw_span(scene: &Scene, m: &MeshDesc, group: Option<usize>) -> (u32, u32) {
    let total = match m.index {
        Some(i) => scene.buffers[i as usize].count(),
        None => m
            .attribute("position")
            .map_or(0, |a| scene.buffers[a.accessor as usize].count()),
    } as u64;
    let mut start = u64::from(m.draw_range.start);
    let mut end = m
        .draw_range
        .count
        .map_or(total, |c| start + u64::from(c))
        .min(total);
    if let Some(g) = group.and_then(|g| m.groups.get(g)) {
        let gs = u64::from(g.start);
        let ge = if g.count.is_finite() {
            gs + g.count as u64
        } else {
            total
        };
        start = start.max(gs);
        end = end.min(ge);
    }
    (start as u32, end.saturating_sub(start) as u32)
}

/// Builds the Bevy mesh for `key`, or none if it draws nothing.
pub fn build_mesh(scene: &Scene, key: MeshKey) -> Option<Mesh> {
    let m = &scene.meshes[key.mesh as usize];
    let pos = &scene.buffers[m.attribute("position")?.accessor as usize];
    let n = pos.count();
    if n == 0 || key.count == 0 {
        return None;
    }
    let topology = match key.draw {
        Draw::Triangles => PrimitiveTopology::TriangleList,
        Draw::Lines | Draw::LineStrip | Draw::LineLoop => PrimitiveTopology::LineList,
        Draw::Points => PrimitiveTopology::PointList,
    };
    let mut mesh = Mesh::new(topology, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, items::<3>(pos, 0.0));
    let same = |b: &Buffer| b.count() == n;
    let mut has_normals = false;
    if key.draw == Draw::Triangles
        && let Some(a) = m.attribute("normal")
    {
        let b = &scene.buffers[a.accessor as usize];
        if same(b) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, items::<3>(b, 0.0));
            has_normals = true;
        }
    }
    if let Some(a) = m.attribute("uv") {
        let b = &scene.buffers[a.accessor as usize];
        if same(b) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, items::<2>(b, 0.0));
        }
    }
    if key.colors
        && let Some(a) = m.attribute("color")
    {
        let b = &scene.buffers[a.accessor as usize];
        if same(b) {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, items::<4>(b, 1.0));
        }
    }

    // The elements drawn: three's index (or vertex) range, as Bevy indices.
    let (s, c) = (key.start as usize, key.count as usize);
    let base: Option<Vec<u32>> = m.index.map(|i| {
        let all = index_values(&scene.buffers[i as usize]);
        let e = (s + c).min(all.len());
        all[s.min(e)..e].to_vec()
    });
    let whole = m.index.is_none() && s == 0 && c >= n;
    let seq =
        |v: Option<Vec<u32>>| v.unwrap_or_else(|| (s as u32..(s + c).min(n) as u32).collect());
    let indices: Option<Vec<u32>> = match key.draw {
        Draw::Triangles | Draw::Lines | Draw::Points => {
            if whole {
                None
            } else {
                Some(seq(base))
            }
        }
        Draw::LineStrip | Draw::LineLoop => {
            let v = seq(base);
            let mut out = Vec::with_capacity(v.len() * 2);
            for w in v.windows(2) {
                out.extend_from_slice(w);
            }
            if key.draw == Draw::LineLoop && v.len() > 2 {
                out.push(v[v.len() - 1]);
                out.push(v[0]);
            }
            Some(out)
        }
    };
    if let Some(ix) = indices {
        if ix.is_empty() || ix.iter().any(|&i| i as usize >= n) {
            return None;
        }
        mesh.insert_indices(if n <= 65536 {
            Indices::U16(ix.into_iter().map(|i| i as u16).collect())
        } else {
            Indices::U32(ix)
        });
    }
    if key.lit && key.draw == Draw::Triangles && !has_normals {
        // MeshStandardMaterial without normals: three would light it with
        // whatever the shader finds; flat or smooth normals are the closest.
        if mesh.indices().is_some() {
            mesh.compute_smooth_normals();
        } else {
            mesh.compute_flat_normals();
        }
    }
    Some(mesh)
}

/// A column-major three.js matrix.
pub fn mat4(m: &[f64; 16]) -> Mat4 {
    Mat4::from_cols_array(&m.map(|x| x as f32))
}

// ── Textures ───────────────────────────────────────────────────────────

/// sRGB to linear, for averaging sRGB texels.
fn srgb_to_linear(c: u8) -> f32 {
    let x = f32::from(c) / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(x: f32) -> u8 {
    let y = if x <= 0.003_130_8 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    };
    (y * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

/// The next mip level by a 2×2 box filter (odd edges clamp), averaging
/// colour in linear light when the texture is sRGB (as the GPU's own mip
/// generation does for sRGB formats). Alpha and non-colour data average
/// as stored.
fn downsample(src: &[u8], w: usize, h: usize, ch: usize, srgb: bool, lut: &[f32; 256]) -> Vec<u8> {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0u8; nw * nh * ch];
    for y in 0..nh {
        let (y0, y1) = ((2 * y).min(h - 1), (2 * y + 1).min(h - 1));
        for x in 0..nw {
            let (x0, x1) = ((2 * x).min(w - 1), (2 * x + 1).min(w - 1));
            for k in 0..ch {
                let at = |xx: usize, yy: usize| src[(yy * w + xx) * ch + k];
                let px = [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)];
                out[(y * nw + x) * ch + k] = if srgb && k < 3 {
                    linear_to_srgb(px.iter().map(|&p| lut[p as usize]).sum::<f32>() / 4.0)
                } else {
                    ((px.iter().map(|&p| u32::from(p)).sum::<u32>() + 2) / 4) as u8
                };
            }
        }
    }
    out
}

fn wrap(mode: u32) -> ImageAddressMode {
    match mode {
        three::CLAMP_TO_EDGE_WRAPPING => ImageAddressMode::ClampToEdge,
        three::MIRRORED_REPEAT_WRAPPING => ImageAddressMode::MirrorRepeat,
        _ => ImageAddressMode::Repeat,
    }
}

/// three's filters: (filter, whether it uses mipmaps, mipmap filter).
fn filter(f: u32) -> (ImageFilterMode, bool, ImageFilterMode) {
    use ImageFilterMode::{Linear, Nearest};
    match f {
        1003 => (Nearest, false, Nearest), // Nearest
        1004 => (Nearest, true, Nearest),  // NearestMipmapNearest
        1005 => (Nearest, true, Linear),   // NearestMipmapLinear
        1006 => (Linear, false, Nearest),  // Linear
        1007 => (Linear, true, Nearest),   // LinearMipmapNearest
        _ => (Linear, true, Linear),       // LinearMipmapLinear (1008)
    }
}

/// A texture as a Bevy image: rows flipped where three flips them on upload
/// (SPEC 5.2), a mip chain where three generates one, and three's sampler.
pub fn build_image(scene: &Scene, t: &TextureDesc) -> Option<Image> {
    let BufferData::U8(px) = &scene.buffers[t.pixels as usize].data else {
        return None;
    };
    let (w, h, ch) = (t.width as usize, t.height as usize, t.channels as usize);
    if w == 0 || h == 0 || px.len() < w * h * ch || !(ch == 1 || ch == 4) {
        return None;
    }
    let row = w * ch;
    let mut level: Vec<u8> = if t.flip_y {
        let mut v = Vec::with_capacity(w * h * ch);
        for y in (0..h).rev() {
            v.extend_from_slice(&px[y * row..(y + 1) * row]);
        }
        v
    } else {
        px[..w * h * ch].to_vec()
    };
    let srgb = t.color_space == "srgb";
    let format = match (ch, srgb) {
        (4, true) => TextureFormat::Rgba8UnormSrgb,
        (4, false) => TextureFormat::Rgba8Unorm,
        _ => TextureFormat::R8Unorm,
    };
    let (min, min_mips, mip_filter) = filter(t.min_filter);
    let (mag, _, _) = filter(t.mag_filter);
    let mips = t.generate_mipmaps && min_mips;
    let size = Extent3d {
        width: t.width,
        height: t.height,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new(
        size,
        TextureDimension::D2,
        level.clone(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    if mips {
        let lut: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as u8));
        let mut data = level.clone();
        let (mut lw, mut lh, mut count) = (w, h, 1u32);
        while lw > 1 || lh > 1 {
            level = downsample(&level, lw, lh, ch, srgb && ch == 4, &lut);
            lw = (lw / 2).max(1);
            lh = (lh / 2).max(1);
            data.extend_from_slice(&level);
            count += 1;
        }
        image.data = Some(data);
        image.texture_descriptor.mip_level_count = count;
    }
    // WebGPU allows anisotropy only with linear filtering throughout.
    let linear = min == ImageFilterMode::Linear
        && mag == ImageFilterMode::Linear
        && (!mips || mip_filter == ImageFilterMode::Linear);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        label: None,
        address_mode_u: wrap(t.wrap_s),
        address_mode_v: wrap(t.wrap_t),
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: mag,
        min_filter: min,
        mipmap_filter: if mips {
            mip_filter
        } else {
            ImageFilterMode::Nearest
        },
        anisotropy_clamp: if linear && mips {
            (t.anisotropy as u16).clamp(1, 16)
        } else {
            1
        },
        ..ImageSamplerDescriptor::default()
    });
    Some(image)
}

/// three's `Matrix3.setUvTransform` for a texture's offset, repeat, rotation
/// and centre, as the affine map Bevy applies to UVs.
pub fn uv_transform(t: &TextureDesc) -> Affine2 {
    let [tx, ty] = t.offset;
    let [sx, sy] = t.repeat;
    let [cx, cy] = t.center;
    let (s, c) = t.rotation.sin_cos();
    Affine2::from_cols_array(&[
        (sx * c) as f32,
        (-sy * s) as f32,
        (sx * s) as f32,
        (sy * c) as f32,
        (-sx * (c * cx + s * cy) + cx + tx) as f32,
        (-sy * (-s * cx + c * cy) + cy + ty) as f32,
    ])
}

// ── Materials ──────────────────────────────────────────────────────────

/// How a material kind draws for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandIn {
    /// Lit (three's standard, physical or Lambert model).
    Lit,
    /// Unlit (three's basic model: basic, line and points materials).
    Unlit,
    /// The sky dome (`render::sky`).
    Sky,
    /// An effect shader not ported yet: not drawn.
    Hidden,
}

pub fn stand_in(m: &MaterialDesc) -> StandIn {
    use MaterialKind::*;
    match m.kind {
        SkyDome => StandIn::Sky,
        // ShaderMaterials (and sprites) whose look is all shader: additive
        // glows, beams, foam, steam, particles, skid marks. Drawn as plain
        // quads they would be white sheets, so they wait for their kinds.
        TrafficStreams | SkyGlow | Surf | LighthouseBeam | Steam | Particles | SkidMarks
        | PoliceGlow | Sprite => StandIn::Hidden,
        _ => match m.ty.as_str() {
            "MeshStandardMaterial" | "MeshPhysicalMaterial" | "MeshLambertMaterial" => StandIn::Lit,
            "MeshBasicMaterial" | "LineBasicMaterial" | "PointsMaterial" => StandIn::Unlit,
            _ => StandIn::Hidden,
        },
    }
}

/// Whether the material reads the geometry's vertex colours.
pub fn vertex_colors(m: &MaterialDesc) -> bool {
    m.boolean("vertexColors").unwrap_or(false) && stand_in(m) != StandIn::Sky
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::math::Vec2;

    #[test]
    fn uv_transform_matches_three() {
        // repeat (4, 2), offset (0.25, 0.5), no rotation: u' = 4u + 0.25.
        let t = TextureDesc {
            name: String::new(),
            source: mr_scene::TextureSource::Canvas,
            url: None,
            width: 1,
            height: 1,
            channels: 4,
            pixels: 0,
            format: three::RGBA_FORMAT,
            ty: three::UNSIGNED_BYTE_TYPE,
            flip_y: true,
            color_space: "srgb".into(),
            premultiply_alpha: false,
            unpack_alignment: 4,
            generate_mipmaps: true,
            wrap_s: 1000,
            wrap_t: 1000,
            mag_filter: 1006,
            min_filter: 1008,
            anisotropy: 8.0,
            offset: [0.25, 0.5],
            repeat: [4.0, 2.0],
            rotation: 0.0,
            center: [0.0, 0.0],
            matrix_auto_update: true,
            channel: 0,
        };
        let a = uv_transform(&t);
        assert_eq!(
            a.transform_point2(Vec2::new(1.0, 1.0)),
            Vec2::new(4.25, 2.5)
        );
    }

    /// A square of four vertices, indexed as two triangles, with two groups.
    fn square() -> Scene {
        let buf = |item_size, data| Buffer {
            item_size,
            normalized: false,
            data,
        };
        let attr = |name: &str, accessor| mr_scene::AttributeRef {
            name: name.into(),
            accessor,
            instanced: false,
            mesh_per_attribute: None,
        };
        Scene {
            buffers: vec![
                buf(
                    3,
                    BufferData::F32(vec![0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.]),
                ),
                buf(1, BufferData::U16(vec![0, 1, 2, 0, 2, 3])),
            ],
            meshes: vec![MeshDesc {
                name: String::new(),
                attributes: vec![attr("position", 0)],
                index: Some(1),
                groups: vec![
                    mr_scene::GroupDesc {
                        start: 0,
                        count: 3.0,
                        material_index: 0,
                    },
                    mr_scene::GroupDesc {
                        start: 3,
                        count: f64::INFINITY,
                        material_index: 1,
                    },
                ],
                draw_range: mr_scene::DrawRange {
                    start: 0,
                    count: None,
                },
                bounding_box: None,
                bounding_sphere: None,
            }],
            ..Scene::default()
        }
    }

    #[test]
    fn spans_and_line_loops() {
        let s = square();
        let m = &s.meshes[0];
        assert_eq!(draw_span(&s, m, None), (0, 6));
        assert_eq!(draw_span(&s, m, Some(0)), (0, 3));
        assert_eq!(draw_span(&s, m, Some(1)), (3, 3));
        let key = |draw, start, count| MeshKey {
            mesh: 0,
            start,
            count,
            draw,
            colors: false,
            lit: true,
        };
        let tri = build_mesh(&s, key(Draw::Triangles, 3, 3)).unwrap();
        let idx: Vec<usize> = tri.indices().unwrap().iter().collect();
        assert_eq!(idx, vec![0, 2, 3]);
        // Lit without normals: normals are made.
        assert!(tri.attribute(Mesh::ATTRIBUTE_NORMAL).is_some());
        // A line loop over the first three indices closes back to the first.
        let ll = build_mesh(&s, key(Draw::LineLoop, 0, 3)).unwrap();
        let idx: Vec<usize> = ll.indices().unwrap().iter().collect();
        assert_eq!(idx, vec![0, 1, 1, 2, 2, 0]);
        assert!(build_mesh(&s, key(Draw::Triangles, 0, 0)).is_none());
    }

    #[test]
    fn tint_packs_ten_bits_a_channel() {
        assert_eq!(crate::loader::pack_tint([0.0, 0.0, 0.0]), 0);
        let p = crate::loader::pack_tint([1.0, 0.5, 2.0]);
        let ch = |k: u32| ((p >> (10 * k)) & 1023) as f32 / 511.5;
        assert!(
            (ch(0) - 1.0).abs() < 1e-3 && (ch(1) - 0.5).abs() < 1e-3 && (ch(2) - 2.0).abs() < 1e-3
        );
    }

    #[test]
    fn box_filter_in_linear_light() {
        let lut: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as u8));
        // Black and white average to linear 0.5, which is sRGB 188.
        let src = [0, 0, 0, 255, 255, 255, 255, 255];
        let out = downsample(&src, 2, 1, 4, true, &lut);
        assert_eq!(out, vec![188, 188, 188, 255]);
        assert_eq!(linear_to_srgb(srgb_to_linear(77)), 77);
    }
}
