//! Port of `src/world/textures.js`: the textures every level shares
//! (roadmap WP 3.2). Canvas-generated textures. Nothing is loaded from
//! disk, so the game has no asset pipeline and starts instantly from a
//! static server.
//!
//! Each generator draws on an [`mr_canvas::Canvas`] exactly as the JS draws
//! on its 2D context, drawing from the same seeded `mulberry32` streams in
//! the same order. The JS module caches by key in a `Map`; here the cache is
//! a [`TextureCache`] the world build owns, with the JS keys (including
//! their quirk: a sign's key leaves out its font and border).

// Index loops stay index loops (DECISIONS D52).
#![allow(clippy::needless_range_loop)]

use std::sync::Arc;

use mr_canvas::Canvas;
use mr_math::{Mulberry32, js, kernel};
use mr_scene::{TextureDesc, TextureSource, three};

const PI: f64 = std::f64::consts::PI;

/// A texture as three.js would upload it.
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// Unpremultiplied RGBA rows, top first: a canvas's `getImageData`, or
    /// a `DataTexture`'s array.
    pub rgba: Vec<u8>,
    pub source: TextureSource,
    pub repeat: bool,
    pub srgb: bool,
    pub anisotropy: f64,
}

impl Texture {
    /// `toTexture(c, { repeat = true, srgb = true, aniso = 8 })`.
    fn canvas(c: &Canvas, repeat: bool, srgb: bool, aniso: f64) -> Texture {
        Texture {
            width: c.width,
            height: c.height,
            rgba: c.to_rgba(),
            source: TextureSource::Canvas,
            repeat,
            srgb,
            anisotropy: aniso,
        }
    }

    /// A canvas as `new THREE.CanvasTexture(c)` uploads it (`repeat`:
    /// `RepeatWrapping` both ways; `srgb`: `SRGBColorSpace`, else none).
    pub fn from_canvas(c: &Canvas, repeat: bool, srgb: bool, aniso: f64) -> Texture {
        Texture::canvas(c, repeat, srgb, aniso)
    }

    /// The scene description, with the pixels in buffer `pixels`.
    pub fn desc(&self, name: &str, pixels: u32) -> TextureDesc {
        let canvas = self.source == TextureSource::Canvas;
        let wrap = if self.repeat {
            three::REPEAT_WRAPPING
        } else {
            three::CLAMP_TO_EDGE_WRAPPING
        };
        TextureDesc {
            name: name.to_string(),
            source: self.source,
            url: None,
            width: self.width,
            height: self.height,
            channels: 4,
            pixels,
            format: three::RGBA_FORMAT,
            ty: three::UNSIGNED_BYTE_TYPE,
            // A CanvasTexture is uploaded flipped (SPEC 5.2); a DataTexture
            // is not.
            flip_y: canvas,
            color_space: if self.srgb { "srgb" } else { "" }.to_string(),
            premultiply_alpha: false,
            // three's DataTexture sets unpackAlignment 1; a canvas texture
            // keeps Texture's 4.
            unpack_alignment: if canvas { 4 } else { 1 },
            generate_mipmaps: true,
            wrap_s: wrap,
            wrap_t: wrap,
            mag_filter: three::LINEAR_FILTER,
            min_filter: three::LINEAR_MIPMAP_LINEAR_FILTER,
            anisotropy: self.anisotropy,
            offset: [0.0, 0.0],
            repeat: [1.0, 1.0],
            rotation: 0.0,
            center: [0.0, 0.0],
            matrix_auto_update: true,
            channel: 0,
        }
    }
}

/// The module's cache: one texture per key, in the order made.
#[derive(Default)]
pub struct TextureCache {
    entries: Vec<(String, Arc<Cached>)>,
}

/// What a generator returns.
pub enum Cached {
    One(Texture),
    /// `signTexture`: `{ texture, aspect }`.
    Sign {
        texture: Texture,
        aspect: f64,
    },
    /// `facadeTextures`: `{ map, emissive, cols, rows }`.
    Facade {
        map: Texture,
        emissive: Texture,
        cols: u32,
        rows: u32,
    },
}

impl Cached {
    pub fn texture(&self) -> &Texture {
        match self {
            Cached::One(t) | Cached::Sign { texture: t, .. } | Cached::Facade { map: t, .. } => t,
        }
    }
}

impl TextureCache {
    pub fn new() -> TextureCache {
        TextureCache::default()
    }

    /// The cached picture under `key`, made by `make` the first time: for
    /// the caches other texture modules keep (`city/cityTextures.js`), under
    /// keys of their own.
    pub fn cached_with(&mut self, key: &str, make: impl FnOnce() -> Cached) -> Arc<Cached> {
        self.cached(key.to_string(), make)
    }

    /// The cached picture under `key`, if made.
    pub fn lookup(&self, key: &str) -> Option<Arc<Cached>> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }

    fn cached(&mut self, key: String, make: impl FnOnce() -> Cached) -> Arc<Cached> {
        if let Some((_, v)) = self.entries.iter().find(|(k, _)| *k == key) {
            return v.clone();
        }
        let v = Arc::new(make());
        self.entries.push((key, v.clone()));
        v
    }

    pub fn detail_texture(&mut self) -> Arc<Cached> {
        self.cached("detail".into(), || Cached::One(detail_texture()))
    }

    pub fn terrain_detail_texture(&mut self) -> Arc<Cached> {
        self.cached("terrainDetail".into(), || {
            Cached::One(terrain_detail_texture())
        })
    }

    pub fn rock_texture(&mut self) -> Arc<Cached> {
        self.cached("rock".into(), || Cached::One(rock_texture()))
    }

    pub fn asphalt_texture(&mut self, tone: u32) -> Arc<Cached> {
        self.cached(format!("asphalt{tone}"), || {
            Cached::One(asphalt_texture(tone))
        })
    }

    pub fn gravel_texture(&mut self) -> Arc<Cached> {
        self.cached("gravel".into(), || Cached::One(gravel_texture()))
    }

    pub fn concrete_texture(&mut self) -> Arc<Cached> {
        self.cached("concrete".into(), || Cached::One(concrete_texture()))
    }

    pub fn chevron_texture(&mut self) -> Arc<Cached> {
        self.cached("chevron".into(), || Cached::One(chevron_texture()))
    }

    pub fn checker_texture(&mut self, n: u32) -> Arc<Cached> {
        self.cached(format!("checker{n}"), || Cached::One(checker_texture(n)))
    }

    pub fn sign_texture(&mut self, lines: &[&str], o: &SignOpts) -> Arc<Cached> {
        // `'sign:' + lines.join('|') + bg + fg + w + h + arrow`.
        let key = format!(
            "sign:{}{}{}{}{}{}",
            lines.join("|"),
            o.bg,
            o.fg,
            o.w,
            o.h,
            o.arrow.unwrap_or("null")
        );
        self.cached(key, || {
            let (texture, aspect) = sign_texture(lines, o);
            Cached::Sign { texture, aspect }
        })
    }

    pub fn glow_texture(&mut self) -> Arc<Cached> {
        self.cached("glow".into(), || Cached::One(glow_texture()))
    }

    pub fn smoke_texture(&mut self) -> Arc<Cached> {
        self.cached("smoke".into(), || Cached::One(smoke_texture()))
    }

    pub fn facade_textures(&mut self, variant: u32) -> Arc<Cached> {
        self.cached(format!("facade{variant}"), || facade_textures(variant))
    }
}

fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::new(w, h)
}

/// A `Float32Array` element read back.
fn f32r(v: f64) -> f64 {
    js::fround(v)
}

/// `((i % n + n) % n)` on integers held as doubles.
fn wrap(i: f64, n: usize) -> usize {
    let n = n as f64;
    ((i % n + n) % n) as usize
}

/// `layers.map((n) => Float32Array of n*n draws)`.
fn lattice(rng: &mut Mulberry32, layers: &[usize]) -> Vec<Vec<f64>> {
    layers
        .iter()
        .map(|&n| (0..n * n).map(|_| f32r(rng.next_f64())).collect())
        .collect()
}

/// The `sample` closure of `detailTexture` and `terrainDetailTexture`.
fn sample(a: &[f64], n: usize, s: usize, x: usize, y: usize) -> f64 {
    let fx = (x * n) as f64 / s as f64;
    let fy = (y * n) as f64 / s as f64;
    let (x0, y0) = (fx.floor(), fy.floor());
    let (tx, ty) = (fx - x0, fy - y0);
    let at = |i: f64, j: f64| a[wrap(j, n) * n + wrap(i, n)];
    let sx = tx * tx * (3.0 - 2.0 * tx);
    let sy = ty * ty * (3.0 - 2.0 * ty);
    (at(x0, y0) * (1.0 - sx) + at(x0 + 1.0, y0) * sx) * (1.0 - sy)
        + (at(x0, y0 + 1.0) * (1.0 - sx) + at(x0 + 1.0, y0 + 1.0) * sx) * sy
}

/// `Math.max(0, Math.min(255, v))`.
fn clamp255(v: f64) -> f64 {
    js::max(0.0, js::min(255.0, v))
}

/// A store into a `Uint8Array` (ToUint8: truncate, modulo 256).
fn to_uint8(v: f64) -> u8 {
    js::to_uint32(v) as u8
}

/// Soft grey noise used to break up flat vertex colours (multiplies).
pub fn detail_texture() -> Texture {
    const S: usize = 256;
    let mut c = canvas(S as u32, S as u32);
    let mut img = c.create_image_data(S as u32, S as u32);
    let mut rng = Mulberry32::new(11);
    // Value noise at a few scales, tileable by wrapping lattice.
    let layers = [8, 16, 32, 64];
    let lat = lattice(&mut rng, &layers);
    for y in 0..S {
        for x in 0..S {
            let mut v = 0.0;
            let mut amp = 0.5;
            for (k, &n) in layers.iter().enumerate() {
                v += sample(&lat[k], n, S, x, y) * amp;
                amp *= 0.6;
            }
            v += (rng.next_f64() - 0.5) * 0.12;
            let g8 = clamp255(150.0 + v * 110.0);
            let i = (y * S + x) * 4;
            img.set(i, g8);
            img.set(i + 1, g8);
            img.set(i + 2, g8);
            img.data[i + 3] = 255;
        }
    }
    c.put_image_data(&img, 0, 0);
    Texture::canvas(&c, true, true, 8.0)
}

/// Tileable value noise on an S×S grid: sum of lattice layers (cells per
/// side) with falling amplitude. Shared by the procedural data textures.
fn tile_noise(s: usize, rng: &mut Mulberry32, layers: &[usize], gain: f64) -> Vec<f64> {
    let lat = lattice(rng, layers);
    let mut out = vec![0.0; s * s];
    let mut norm = 0.0;
    let mut amp = 0.5;
    for _ in layers {
        norm += amp;
        amp *= gain;
    }
    for y in 0..s {
        for x in 0..s {
            let mut v = 0.0;
            amp = 0.5;
            for (k, &n) in layers.iter().enumerate() {
                v += sample(&lat[k], n, s, x, y) * amp;
                amp *= gain;
            }
            out[y * s + x] = f32r(v / norm);
        }
    }
    out
}

/// Tileable cellular noise: stones. Each jittered cell gets its own tone,
/// darkening toward the gap with its neighbours (F2 - F1 small). ~0 in the
/// gaps, 0.3..1 on the stones.
fn tile_cells(s: usize, rng: &mut Mulberry32, n: usize) -> Vec<f64> {
    let mut pts = vec![0.0; n * n * 3];
    for i in 0..n * n {
        pts[i * 3] = f32r(rng.next_f64());
        pts[i * 3 + 1] = f32r(rng.next_f64());
        pts[i * 3 + 2] = f32r(rng.next_f64());
    }
    let mut out = vec![0.0; s * s];
    for y in 0..s {
        for x in 0..s {
            let fx = (x * n) as f64 / s as f64;
            let fy = (y * n) as f64 / s as f64;
            let (cx, cy) = (fx.floor(), fy.floor());
            let (mut f1, mut f2, mut tone) = (9.0, 9.0, 0.0);
            for j in -1..=1 {
                for i in -1..=1 {
                    let gx = cx + i as f64;
                    let gy = cy + j as f64;
                    let k = wrap(gy, n) * n + wrap(gx, n);
                    let d = kernel::hypot(gx + pts[k * 3] - fx, gy + pts[k * 3 + 1] - fy);
                    if d < f1 {
                        f2 = f1;
                        f1 = d;
                        tone = pts[k * 3 + 2];
                    } else if d < f2 {
                        f2 = d;
                    }
                }
            }
            let edge = js::min(1.0, (f2 - f1) * 5.0);
            out[y * s + x] = f32r(edge * (0.3 + 0.7 * tone) * (1.0 - f1 * 0.35));
        }
    }
    out
}

/// Terrain detail, four independent tileable channels in one texture so the
/// ground shader gets several scales of variation from a few fetches:
///   R  soft multi-octave noise (identical to detailTexture's grey)
///   G  fine grain: grass tufts / sand grain, high frequency
///   B  cellular: pebbles and dry cracks
///   A  broad smooth noise, for macro variation when sampled very large
/// Linear data (not sRGB) — the shader interprets the values itself.
pub fn terrain_detail_texture() -> Texture {
    const S: usize = 256;
    let mut data = vec![0u8; S * S * 4];
    // R: same generator and seed as detailTexture so the base look is kept.
    let mut rng = Mulberry32::new(11);
    let layers = [8, 16, 32, 64];
    let lat = lattice(&mut rng, &layers);
    for y in 0..S {
        for x in 0..S {
            let mut v = 0.0;
            let mut amp = 0.5;
            for (k, &n) in layers.iter().enumerate() {
                v += sample(&lat[k], n, S, x, y) * amp;
                amp *= 0.6;
            }
            v += (rng.next_f64() - 0.5) * 0.12;
            data[(y * S + x) * 4] = to_uint8(clamp255(150.0 + v * 110.0));
        }
    }
    let mut r2 = Mulberry32::new(77);
    let grain = tile_noise(S, &mut r2, &[64, 128], 0.7);
    let cells = tile_cells(S, &mut r2, 24);
    let broad = tile_noise(S, &mut r2, &[3, 6, 12], 0.55);
    for i in 0..S * S {
        let g = (grain[i] - 0.5) * 1.6 + (r2.next_f64() - 0.5) * 0.35 + 0.5;
        data[i * 4 + 1] = to_uint8(clamp255(g * 255.0));
        data[i * 4 + 2] = to_uint8(clamp255(cells[i] * 255.0));
        data[i * 4 + 3] = to_uint8(clamp255(((broad[i] - 0.5) * 1.8 + 0.5) * 255.0));
    }
    Texture {
        width: S as u32,
        height: S as u32,
        rgba: data,
        source: TextureSource::Data,
        repeat: true,
        srgb: false,
        anisotropy: 8.0,
    }
}

// Asphalt: dark aggregate with speckles and faint tyre-wear bands.
// Layered rock: horizontal strata with cracks, sampled triplanar on cliffs.
pub fn rock_texture() -> Texture {
    const S: usize = 256;
    let mut c = canvas(S as u32, S as u32);
    let mut img = c.create_image_data(S as u32, S as u32);
    let mut rng = Mulberry32::new(31);
    let mut bands = vec![0.0; S];
    let mut v = 0.8;
    for b in bands.iter_mut() {
        if rng.next_f64() < 0.08 {
            v = 0.62 + rng.next_f64() * 0.45;
        }
        *b = f32r(v);
    }
    let mut jitter = vec![0.0; S];
    for (x, j) in jitter.iter_mut().enumerate() {
        let x = x as f64;
        *j = f32r(
            kernel::sin(x / S as f64 * PI * 2.0 * 3.0) * 4.0
                + kernel::sin(x / S as f64 * PI * 2.0 * 7.0 + 1.0) * 2.0,
        );
    }
    for y in 0..S {
        for x in 0..S {
            let yy = wrap(y as f64 + js::round(jitter[x]), S);
            let mut b = bands[yy] + (rng.next_f64() - 0.5) * 0.16;
            if rng.next_f64() < 0.004 {
                b *= 0.5;
            }
            let g8 = clamp255(b * 200.0);
            let i = (y * S + x) * 4;
            img.set(i, g8);
            img.set(i + 1, g8 * 0.97);
            img.set(i + 2, g8 * 0.93);
            img.data[i + 3] = 255;
        }
    }
    c.put_image_data(&img, 0, 0);
    // Vertical cracks.
    c.set_stroke_style("rgba(20,18,16,0.5)");
    let s = S as f64;
    for _ in 0..26 {
        let mut x = rng.next_f64() * s;
        let mut y = rng.next_f64() * s;
        c.set_line_width(0.6 + rng.next_f64() * 1.4);
        c.begin_path();
        c.move_to(x, y);
        for _ in 0..5 {
            x += (rng.next_f64() - 0.5) * 10.0;
            y += rng.next_f64() * 22.0;
            c.line_to(x, y);
        }
        c.stroke();
    }
    Texture::canvas(&c, true, true, 8.0)
}

pub fn asphalt_texture(tone: u32) -> Texture {
    const S: usize = 512;
    let s = S as f64;
    let mut c = canvas(S as u32, S as u32);
    let base = match tone {
        1 => [70, 70, 72],
        2 => [58, 58, 62],
        _ => [66, 64, 62],
    };
    c.set_fill_style(format!("rgb({},{},{})", base[0], base[1], base[2]));
    c.fill_rect(0.0, 0.0, s, s);
    let mut rng = Mulberry32::new(5 + tone);
    let mut img = c.get_image_data(0, 0, S as u32, S as u32);
    for i in 0..S * S {
        let speck = (rng.next_f64() - 0.5) * 34.0;
        let light = if rng.next_f64() < 0.03 { 40.0 } else { 0.0 };
        let dark = if rng.next_f64() < 0.02 { 30.0 } else { 0.0 };
        let n = speck + light - dark;
        for k in 0..3 {
            let v = img.data[i * 4 + k] as f64 + n;
            img.set(i * 4 + k, v);
        }
    }
    c.put_image_data(&img, 0, 0);
    // Patches and cracks.
    for _ in 0..18 {
        let rgb = if rng.next_f64() < 0.5 {
            "20,20,22"
        } else {
            "110,108,104"
        };
        c.set_fill_style(format!("rgba({rgb},{})", 0.05 + rng.next_f64() * 0.08));
        c.begin_path();
        let (x, y) = (rng.next_f64() * s, rng.next_f64() * s);
        let rx = 20.0 + rng.next_f64() * 80.0;
        let ry = 10.0 + rng.next_f64() * 40.0;
        let rot = rng.next_f64() * 3.0;
        c.ellipse(x, y, rx, ry, rot, 0.0, 7.0, false);
        c.fill();
    }
    c.set_stroke_style("rgba(15,15,15,0.35)");
    c.set_line_width(1.2);
    for _ in 0..10 {
        let mut x = rng.next_f64() * s;
        let mut y = rng.next_f64() * s;
        c.begin_path();
        c.move_to(x, y);
        for _ in 0..6 {
            x += (rng.next_f64() - 0.5) * 40.0;
            y += (rng.next_f64() - 0.5) * 40.0;
            c.line_to(x, y);
        }
        c.stroke();
    }
    Texture::canvas(&c, true, true, 8.0)
}

/// Gravel verge: fines with a soft mottle, then stones of mixed size and
/// tone, each lit from one side with a contact shadow so they read as lumps.
pub fn gravel_texture() -> Texture {
    const S: usize = 256;
    let s = S as f64;
    let mut c = canvas(S as u32, S as u32);
    let mut rng = Mulberry32::new(19);
    let mott = tile_noise(S, &mut rng, &[6, 12, 24, 48], 0.6);
    let mut img = c.create_image_data(S as u32, S as u32);
    for (i, m) in mott.iter().enumerate() {
        let v = 124.0 + (m - 0.5) * 60.0 + (rng.next_f64() - 0.5) * 26.0;
        img.set(i * 4, v * 1.03);
        img.set(i * 4 + 1, v * 0.98);
        img.set(i * 4 + 2, v * 0.9);
        img.data[i * 4 + 3] = 255;
    }
    c.put_image_data(&img, 0, 0);
    let stone = |c: &mut Canvas, rng: &mut Mulberry32, x: f64, y: f64, r: f64, v: f64| {
        // Draw with wrap so the texture tiles.
        for ox in [-s, 0.0, s] {
            for oy in [-s, 0.0, s] {
                let (xx, yy) = (x + ox, y + oy);
                if xx < -r * 2.0 || xx > s + r * 2.0 || yy < -r * 2.0 || yy > s + r * 2.0 {
                    continue;
                }
                c.set_fill_style("rgba(20,18,15,0.45)");
                c.begin_path();
                c.ellipse(
                    xx + r * 0.35,
                    yy + r * 0.35,
                    r * 1.05,
                    r * 0.85,
                    0.0,
                    0.0,
                    7.0,
                    false,
                );
                c.fill();
                c.set_fill_style(format!(
                    "rgb({},{},{})",
                    js::to_int32(v * 1.02),
                    js::to_int32(v * 0.97),
                    js::to_int32(v * 0.9)
                ));
                c.begin_path();
                c.ellipse(xx, yy, r, r * 0.8, rng.next_f64() * 3.0, 0.0, 7.0, false);
                c.fill();
                c.set_fill_style("rgba(255,250,240,0.22)");
                c.begin_path();
                c.ellipse(
                    xx - r * 0.3,
                    yy - r * 0.3,
                    r * 0.45,
                    r * 0.35,
                    0.0,
                    0.0,
                    7.0,
                    false,
                );
                c.fill();
            }
        }
    };
    for _ in 0..1700 {
        let r = 0.7 + kernel::pow(rng.next_f64(), 3.0) * 3.2;
        let x = rng.next_f64() * s;
        let y = rng.next_f64() * s;
        let v = 70.0 + rng.next_f64() * 120.0;
        stone(&mut c, &mut rng, x, y, r, v);
    }
    Texture::canvas(&c, true, true, 8.0)
}

pub fn concrete_texture() -> Texture {
    const S: usize = 256;
    let s = S as f64;
    let mut c = canvas(S as u32, S as u32);
    c.set_fill_style("#b9b6ae");
    c.fill_rect(0.0, 0.0, s, s);
    let mut rng = Mulberry32::new(23);
    let mut img = c.get_image_data(0, 0, S as u32, S as u32);
    for i in 0..S * S {
        let n = (rng.next_f64() - 0.5) * 22.0;
        for k in 0..3 {
            let v = img.data[i * 4 + k] as f64 + n;
            img.set(i * 4 + k, v);
        }
    }
    c.put_image_data(&img, 0, 0);
    c.set_fill_style("rgba(60,55,50,0.18)");
    for _ in 0..20 {
        let x = rng.next_f64() * s;
        let y = rng.next_f64() * s;
        let w = 2.0 + rng.next_f64() * 30.0;
        let h = 1.0 + rng.next_f64() * 60.0;
        c.fill_rect(x, y, w, h);
    }
    c.set_fill_style("rgba(0,0,0,0.25)");
    c.fill_rect(0.0, 0.0, s, 2.0);
    Texture::canvas(&c, true, true, 8.0)
}

/// Chevron sign (yellow with black arrow). dir: 1 = points right.
pub fn chevron_texture() -> Texture {
    let mut g = canvas(128, 128);
    g.set_fill_style("#f2c230");
    g.fill_rect(0.0, 0.0, 128.0, 128.0);
    g.set_fill_style("#111");
    g.begin_path();
    g.move_to(30.0, 14.0);
    g.line_to(62.0, 14.0);
    g.line_to(100.0, 64.0);
    g.line_to(62.0, 114.0);
    g.line_to(30.0, 114.0);
    g.line_to(68.0, 64.0);
    g.close_path();
    g.fill();
    g.set_stroke_style("#111");
    g.set_line_width(6.0);
    g.stroke_rect(3.0, 3.0, 122.0, 122.0);
    Texture::canvas(&g, false, true, 8.0)
}

/// Checkerboard banner/start line.
pub fn checker_texture(n: u32) -> Texture {
    let mut g = canvas(256, 64);
    let w = 256.0 / (n * 2) as f64;
    let h = 64.0 / 4.0;
    for y in 0..4 {
        for x in 0..n * 2 {
            g.set_fill_style(if (x + y) % 2 != 0 { "#111" } else { "#f4f4f4" });
            g.fill_rect(x as f64 * w, y as f64 * h, w, h);
        }
    }
    Texture::canvas(&g, true, true, 8.0)
}

/// `signTexture`'s options, with the JS defaults.
#[derive(Clone, Debug)]
pub struct SignOpts<'a> {
    pub bg: &'a str,
    pub fg: &'a str,
    /// `null` for none.
    pub border: Option<&'a str>,
    pub w: u32,
    pub h: u32,
    pub font: &'a str,
    /// `'up'`, `'left'`, `'right'`, or `null`.
    pub arrow: Option<&'a str>,
}

impl Default for SignOpts<'_> {
    fn default() -> Self {
        SignOpts {
            bg: "#0b6b3a",
            fg: "#fff",
            border: Some("#fff"),
            w: 512,
            h: 256,
            font: "bold 64px \"Arial Narrow\", Arial, sans-serif",
            arrow: None,
        }
    }
}

/// Generic text sign. Returns `(texture, aspect)`.
pub fn sign_texture(lines: &[&str], o: &SignOpts) -> (Texture, f64) {
    let (w, h) = (o.w as f64, o.h as f64);
    let mut g = canvas(o.w, o.h);
    g.set_fill_style(o.bg);
    g.fill_rect(0.0, 0.0, w, h);
    if let Some(border) = o.border {
        g.set_stroke_style(border);
        g.set_line_width(js::max(4.0, h * 0.03));
        let r = h * 0.06;
        g.begin_path();
        g.round_rect(10.0, 10.0, w - 20.0, h - 20.0, r);
        g.stroke();
    }
    g.set_fill_style(o.fg);
    g.set_text_align("center");
    g.set_text_baseline("middle");
    g.set_font(o.font);
    let lh = h / (lines.len() as f64 + if o.arrow.is_some() { 1.0 } else { 0.0 } + 0.4);
    for (i, l) in lines.iter().enumerate() {
        g.fill_text(l, w / 2.0, lh * (i as f64 + 0.7 + 0.2));
    }
    if let Some(arrow) = o.arrow {
        let y = lh * (lines.len() as f64 + 0.7 + 0.1);
        g.save();
        g.translate(w / 2.0, y);
        g.rotate(match arrow {
            "right" => PI / 4.0,
            "left" => -PI / 4.0,
            _ => 0.0,
        });
        g.begin_path();
        g.move_to(0.0, -lh * 0.4);
        g.line_to(lh * 0.3, -lh * 0.05);
        g.line_to(lh * 0.1, -lh * 0.05);
        g.line_to(lh * 0.1, lh * 0.4);
        g.line_to(-lh * 0.1, lh * 0.4);
        g.line_to(-lh * 0.1, -lh * 0.05);
        g.line_to(-lh * 0.3, -lh * 0.05);
        g.close_path();
        g.fill();
        g.restore();
    }
    (Texture::canvas(&g, false, true, 8.0), w / h)
}

/// Radial glow, used for light pools on the road and sprite halos.
pub fn glow_texture() -> Texture {
    const S: f64 = 128.0;
    let mut g = canvas(S as u32, S as u32);
    let mut grd = g.create_radial_gradient(S / 2.0, S / 2.0, 0.0, S / 2.0, S / 2.0, S / 2.0);
    grd.add_color_stop(0.0, "rgba(255,255,255,1)");
    grd.add_color_stop(0.35, "rgba(255,255,255,0.45)");
    grd.add_color_stop(1.0, "rgba(255,255,255,0)");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, S, S);
    Texture::canvas(&g, false, true, 8.0)
}

pub fn smoke_texture() -> Texture {
    const S: f64 = 64.0;
    let mut g = canvas(S as u32, S as u32);
    let mut grd = g.create_radial_gradient(S / 2.0, S / 2.0, 0.0, S / 2.0, S / 2.0, S / 2.0);
    grd.add_color_stop(0.0, "rgba(255,255,255,0.9)");
    grd.add_color_stop(0.5, "rgba(255,255,255,0.35)");
    grd.add_color_stop(1.0, "rgba(255,255,255,0)");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, S, S);
    Texture::canvas(&g, false, true, 8.0)
}

struct FacadeStyle {
    wall: &'static str,
    win: &'static str,
    cols: u32,
    rows: u32,
    gap: f64,
    lit: f64,
}

/// Building façade with a grid of windows. `lit` fraction are warm/cool lit.
/// Returns {map, emissive} — the emissive map holds only the lit windows so
/// the city can light up as night falls by raising emissiveIntensity.
pub fn facade_textures(variant: u32) -> Cached {
    const W: f64 = 256.0;
    const H: f64 = 512.0;
    let mut rng = Mulberry32::new(100 + variant);
    let styles = [
        FacadeStyle {
            wall: "#4a4f58",
            win: "#1c2430",
            cols: 8,
            rows: 20,
            gap: 0.28,
            lit: 0.42,
        },
        FacadeStyle {
            wall: "#6b6258",
            win: "#20242a",
            cols: 6,
            rows: 16,
            gap: 0.35,
            lit: 0.36,
        },
        // glass tower
        FacadeStyle {
            wall: "#2b3440",
            win: "#15202c",
            cols: 10,
            rows: 26,
            gap: 0.16,
            lit: 0.5,
        },
        FacadeStyle {
            wall: "#7a6f64",
            win: "#262626",
            cols: 5,
            rows: 14,
            gap: 0.4,
            lit: 0.3,
        },
        // curtain wall
        FacadeStyle {
            wall: "#3a3d44",
            win: "#10161e",
            cols: 12,
            rows: 30,
            gap: 0.12,
            lit: 0.55,
        },
        // low-rise brick-ish
        FacadeStyle {
            wall: "#8b8074",
            win: "#2a2622",
            cols: 4,
            rows: 10,
            gap: 0.42,
            lit: 0.45,
        },
    ];
    let st = &styles[variant as usize % styles.len()];
    let mut g = canvas(W as u32, H as u32);
    let mut ge = canvas(W as u32, H as u32);
    g.set_fill_style(st.wall);
    g.fill_rect(0.0, 0.0, W, H);
    ge.set_fill_style("#000");
    ge.fill_rect(0.0, 0.0, W, H);
    let cw = W / st.cols as f64;
    let rh = H / st.rows as f64;
    let warm = ["#ffd99a", "#ffe7b8", "#fff2d6", "#ffcf80"];
    let cool = ["#cfe6ff", "#b8d4ff", "#e8f4ff"];
    for r in 0..st.rows {
        // Whole floors tend to be lit together (offices).
        let floor_lit = if rng.next_f64() < 0.35 {
            0.85
        } else {
            st.lit * 0.6
        };
        for q in 0..st.cols {
            let x = q as f64 * cw + cw * st.gap * 0.5;
            let y = r as f64 * rh + rh * st.gap * 0.5;
            let w = cw * (1.0 - st.gap);
            let h = rh * (1.0 - st.gap);
            g.set_fill_style(st.win);
            g.fill_rect(x, y, w, h);
            g.set_fill_style("rgba(255,255,255,0.06)");
            g.fill_rect(x, y, w, h * 0.3);
            if rng.next_f64() < floor_lit {
                let col = if rng.next_f64() < 0.7 {
                    warm[(rng.next_f64() * warm.len() as f64).floor() as usize]
                } else {
                    cool[(rng.next_f64() * cool.len() as f64).floor() as usize]
                };
                ge.set_fill_style(col);
                ge.set_global_alpha(0.55 + rng.next_f64() * 0.45);
                ge.fill_rect(x, y, w, h);
                ge.set_global_alpha(1.0);
                g.set_fill_style(col);
                g.set_global_alpha(0.25);
                g.fill_rect(x, y, w, h);
                g.set_global_alpha(1.0);
            }
        }
    }
    Cached::Facade {
        map: Texture::canvas(&g, true, true, 8.0),
        emissive: Texture::canvas(&ge, true, true, 8.0),
        cols: st.cols,
        rows: st.rows,
    }
}
