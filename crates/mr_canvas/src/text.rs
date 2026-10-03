//! Text layout as Chrome's canvas does it: per-character font fallback
//! through the family list, HarfBuzz shaping of each run (harfrust, the
//! HarfBuzz project's Rust port: GPOS kerning with variable-font deltas,
//! ligatures), baselines from the primary font's metrics, and glyph
//! outlines (skrifa) placed as Skia places them (DECISIONS D152).

use harfrust::{Direction, ShapeOptions, ShaperData, ShaperInstance, UnicodeBuffer, Variation};
use skrifa::instance::{Location, Size};
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider, Tag};

use crate::Matrix;
use crate::fonts::{Face, FontBook, FontSpec};

/// A shaped glyph: which face, which glyph, and its pen position from the
/// start of the text in CSS pixels (y down).
#[derive(Clone, Debug)]
pub struct Glyph {
    pub face: usize,
    pub id: u32,
    pub x: f64,
    pub y: f64,
}

/// A face as drawn: the face, its `wght` (if variable) and whether it is
/// slanted synthetically (italic asked of an upright face).
pub struct UsedFace<'a> {
    pub face: &'a Face,
    pub weight: Option<f32>,
    pub oblique: bool,
}

/// Shaped text.
pub struct Layout<'a> {
    pub faces: Vec<UsedFace<'a>>,
    pub glyphs: Vec<Glyph>,
    /// Advance width, CSS pixels.
    pub width: f64,
    pub size: f64,
    /// Baseline offsets of the primary font (see [`Layout::baseline`]).
    pub metrics: Metrics,
}

/// The primary font's vertical metrics at the font size, as Blink keeps
/// them: ascent and descent rounded to whole pixels, and the OS/2 typo
/// ascent and descent normalised to sum to the font size, in 1/64 px.
#[derive(Clone, Copy, Debug, Default)]
pub struct Metrics {
    pub ascent: f32,
    pub descent: f32,
    pub typo_ascent: f32,
    pub typo_descent: f32,
}

fn round64(v: f32) -> f32 {
    (v * 64.0).round() / 64.0
}

impl Metrics {
    fn of(font: &FontRef, loc: &Location, size: f32) -> Metrics {
        // FreeType's choice of ascent and descent (skrifa follows it).
        let m = font.metrics(Size::new(size), loc);
        let ascent = m.ascent.round();
        let descent = (-m.descent).round();
        let norm = |a: f32, d: f32| -> Option<(f32, f32)> {
            let h = a + d;
            (h > 0.0 && a >= 0.0 && a <= h).then(|| (round64(a * size / h), round64(d * size / h)))
        };
        let upem = m.units_per_em as f32;
        let typo = font.os2().ok().and_then(|os2| {
            norm(
                os2.s_typo_ascender() as f32 * size / upem,
                -(os2.s_typo_descender() as f32) * size / upem,
            )
        });
        let (typo_ascent, typo_descent) = typo
            .or_else(|| norm(ascent, descent))
            .unwrap_or((size, 0.0));
        Metrics {
            ascent,
            descent,
            typo_ascent,
            typo_descent,
        }
    }
}

/// `textBaseline` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Baseline {
    Alphabetic,
    Top,
    Hanging,
    Middle,
    Ideographic,
    Bottom,
}

impl Baseline {
    pub fn parse(s: &str) -> Option<Baseline> {
        Some(match s {
            "alphabetic" => Baseline::Alphabetic,
            "top" => Baseline::Top,
            "hanging" => Baseline::Hanging,
            "middle" => Baseline::Middle,
            "ideographic" => Baseline::Ideographic,
            "bottom" => Baseline::Bottom,
            _ => return None,
        })
    }
}

/// `textAlign` values (the text direction is always left to right).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Start,
    End,
    Left,
    Right,
    Center,
}

impl Align {
    pub fn parse(s: &str) -> Option<Align> {
        Some(match s {
            "start" => Align::Start,
            "end" => Align::End,
            "left" => Align::Left,
            "right" => Align::Right,
            "center" => Align::Center,
            _ => return None,
        })
    }
}

const WGHT: Tag = Tag::new(b"wght");

fn font_ref(face: &Face) -> FontRef<'_> {
    FontRef::new(&face.data).expect("checked when added")
}

/// The `wght` a face is drawn at: the requested weight within the face's
/// range and its axis (Chrome sets the axis from `font-weight`).
fn axis_weight(face: &Face, weight: f32) -> Option<f32> {
    let f = font_ref(face);
    let axis = f.axes().get_by_tag(WGHT)?;
    Some(
        weight
            .clamp(face.weight.0, face.weight.1)
            .clamp(axis.min_value(), axis.max_value()),
    )
}

fn location(font: &FontRef, weight: Option<f32>) -> Location {
    match weight {
        Some(w) => font.axes().location([(WGHT, w)]),
        None => Location::default(),
    }
}

/// Shapes `text` in `spec` with the faces of `book`.
pub fn layout<'a>(book: &'a FontBook, spec: &FontSpec, text: &str) -> Layout<'a> {
    // The faces of the family list, in order.
    let mut list: Vec<&Face> = Vec::new();
    for fam in &spec.families {
        if let Some(f) = book.select(fam, spec.weight, spec.italic)
            && !list.iter().any(|g| std::ptr::eq(*g, f))
        {
            list.push(f);
        }
    }
    let size = spec.size as f64;
    let mut out = Layout {
        faces: Vec::new(),
        glyphs: Vec::new(),
        width: 0.0,
        size,
        metrics: Metrics::default(),
    };
    if list.is_empty() {
        return out;
    }
    for f in &list {
        out.faces.push(UsedFace {
            face: f,
            weight: axis_weight(f, spec.weight),
            oblique: spec.italic && !f.italic,
        });
    }
    let fonts: Vec<FontRef> = out.faces.iter().map(|u| font_ref(u.face)).collect();
    out.metrics = Metrics::of(
        &fonts[0],
        &location(&fonts[0], out.faces[0].weight),
        spec.size,
    );

    // Runs of characters by the first face that has them (else the first).
    // Blink shapes word by word (CachingWordShaper): each space is a run of
    // its own, so nothing kerns across a space.
    let maps: Vec<_> = fonts.iter().map(|f| f.charmap()).collect();
    let mut runs: Vec<(usize, String)> = Vec::new();
    let mut after_space = false;
    for ch in text.chars() {
        // Canvas text replaces tabs and newlines by spaces.
        let ch = if matches!(ch, '\t' | '\n' | '\r' | '\x0c') {
            ' '
        } else {
            ch
        };
        let k = maps.iter().position(|m| m.map(ch).is_some()).unwrap_or(0);
        let space = ch == ' ';
        match runs.last_mut() {
            Some((rk, s)) if *rk == k && !space && !after_space => s.push(ch),
            _ => runs.push((k, ch.to_string())),
        }
        after_space = space;
    }
    // HarfBuzz at the font size in 16.16 fixed point, as Blink sets it up.
    let scale = (size * 65536.0) as i32;
    let unit = 1.0 / 65536.0;
    let mut pen = 0.0f64;
    for (k, s) in runs {
        let font = &fonts[k];
        let data = ShaperData::new(font);
        let inst = out.faces[k].weight.map(|w| {
            ShaperInstance::from_variations(
                font,
                [Variation {
                    tag: WGHT,
                    value: w,
                }],
            )
        });
        let shaper = data.shaper(font).instance(inst.as_ref()).build();
        let mut buf = UnicodeBuffer::new();
        buf.push_str(&s);
        buf.set_direction(Direction::LeftToRight);
        buf.guess_segment_properties();
        let shaped = shaper.shape(buf, ShapeOptions::new().scale(Some(scale)));
        for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            out.glyphs.push(Glyph {
                face: k,
                id: info.glyph_id,
                x: pen + pos.x_offset as f64 * unit,
                y: -(pos.y_offset as f64) * unit,
            });
            pen += pos.x_advance as f64 * unit;
        }
    }
    out.width = pen;
    out
}

impl Layout<'_> {
    /// How far below `y` the alphabetic baseline sits for a `textBaseline`
    /// (Blink's `TextMetrics::GetFontBaseline`).
    pub fn baseline(&self, b: Baseline) -> f64 {
        let m = &self.metrics;
        (match b {
            Baseline::Alphabetic => 0.0,
            Baseline::Top => m.typo_ascent,
            Baseline::Middle => (m.typo_ascent - m.typo_descent) / 2.0,
            Baseline::Bottom => -m.typo_descent,
            Baseline::Hanging => m.ascent * 0.8,
            Baseline::Ideographic => -m.descent,
        }) as f64
    }

    /// The glyph outlines as one device-space path per glyph, the text's
    /// origin (alphabetic baseline, left end) at user `(x, y)` under `m`.
    ///
    /// Under a transform without rotation or skew, Chrome's Skia positions
    /// glyphs on a quarter-pixel grid across and a whole-pixel grid down
    /// (subpixel positioning in x) and hints them with FreeType's light
    /// automatic hinter at the device size (skrifa's port of it), which
    /// moves points only vertically. Otherwise the outlines are unhinted
    /// and go where they fall.
    pub fn glyph_paths(&self, m: &Matrix, x: f64, y: f64) -> Vec<tiny_skia::Path> {
        let fonts: Vec<FontRef> = self.faces.iter().map(|u| font_ref(u.face)).collect();
        let locs: Vec<Location> = fonts
            .iter()
            .zip(&self.faces)
            .map(|(f, u)| location(f, u.weight))
            .collect();
        let outlines: Vec<_> = fonts.iter().map(|f| f.outline_glyphs()).collect();
        // Skia's axis alignment for horizontal text: where the font's x axis
        // goes. Along it glyphs sit on a quarter-pixel grid, across it on
        // whole pixels, and the outline is hinted (in its own frame, so
        // vertically for the font) at the device size across the baseline.
        // (Nearly zero as SkScalarNearlyZero has it: a rotation by a
        // quarter turn leaves 6e-17 for its cosine.)
        let zero = |v: f64| v.abs() <= 1.0 / 4096.0;
        let align = if zero(m.b) && zero(m.c) && !zero(m.a) && !zero(m.d) {
            Some(true)
        } else if zero(m.a) && zero(m.d) && !zero(m.b) && !zero(m.c) {
            Some(false)
        } else {
            None
        };
        let k = (m.c * m.c + m.d * m.d).sqrt();
        let ppem = (self.size * k) as f32;
        let hinters: Vec<Option<HintingInstance>> = if align.is_some() && ppem > 0.0 {
            outlines
                .iter()
                .zip(&locs)
                .map(|(o, l)| HintingInstance::new(o, Size::new(ppem), l, light_autohint()).ok())
                .collect()
        } else {
            outlines.iter().map(|_| None).collect()
        };
        let mut out = Vec::new();
        for g in &self.glyphs {
            let skew = if self.faces[g.face].oblique {
                0.25
            } else {
                0.0
            };
            let (mut ox, mut oy) = m.apply(x + g.x, y + g.y);
            match align {
                Some(true) => {
                    ox = ((ox + 0.125) * 4.0).floor() / 4.0;
                    oy = (oy + 0.5).floor();
                }
                Some(false) => {
                    ox = (ox + 0.5).floor();
                    oy = ((oy + 0.125) * 4.0).floor() / 4.0;
                }
                None => {}
            }
            let Some(glyph) = outlines[g.face].get(GlyphId::new(g.id)) else {
                continue;
            };
            let (settings, a, b, c, d) = match &hinters[g.face] {
                // Hinted pixels (y up) to device: m without its scale across
                // the baseline.
                Some(h) => (
                    DrawSettings::hinted(h, false),
                    m.a / k,
                    m.b / k,
                    m.c / k,
                    m.d / k,
                ),
                None => {
                    let upem = fonts[g.face]
                        .head()
                        .map(|h| h.units_per_em())
                        .unwrap_or(1000) as f64;
                    let s = self.size / upem;
                    (
                        DrawSettings::unhinted(Size::unscaled(), &locs[g.face]),
                        m.a * s,
                        m.b * s,
                        m.c * s,
                        m.d * s,
                    )
                }
            };
            let mut pen = Outline {
                pb: tiny_skia::PathBuilder::new(),
                a,
                b,
                c,
                d,
                ox,
                oy,
                skew,
            };
            if glyph.draw(settings, &mut pen).is_ok()
                && let Some(p) = pen.pb.finish()
            {
                out.push(p);
            }
        }
        out
    }
}

/// Skia's Fontations backend at Chrome's slight hinting: the automatic
/// hinter in light mode for every face (measured: it is used even for faces
/// with their own TrueType hints), keeping linear advances.
fn light_autohint() -> HintingOptions {
    HintingOptions {
        engine: Engine::Auto(None),
        target: Target::Smooth {
            mode: SmoothMode::Light,
            symmetric_rendering: true,
            preserve_linear_metrics: true,
        },
    }
}

struct Outline {
    pb: tiny_skia::PathBuilder,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    ox: f64,
    oy: f64,
    skew: f32,
}

impl Outline {
    fn map(&self, x: f32, y: f32) -> (f32, f32) {
        // Glyph space is y up: flip, then skew for a synthetic oblique.
        let (ux, uy) = ((x + self.skew * y) as f64, -(y as f64));
        (
            (self.ox + self.a * ux + self.c * uy) as f32,
            (self.oy + self.b * ux + self.d * uy) as f32,
        )
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.pb.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.map(x, y);
        self.pb.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.map(x1, y1);
        let (x, y) = self.map(x, y);
        self.pb.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.map(x1, y1);
        let (x2, y2) = self.map(x2, y2);
        let (x, y) = self.map(x, y);
        self.pb.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// Chrome's text gamma on Linux (Skia's `SK_GAMMA_EXPONENT`,
/// `SK_GAMMA_CONTRAST`), confirmed against its canvas: white text's
/// coverage comes out as `c^(1/1.2)`, black text's as the contrast-boosted
/// curve below.
const GAMMA: f32 = 1.2;
const CONTRAST: f32 = 0.2;

/// The table Skia applies to an A8 glyph mask drawn in `style`
/// (`SkMaskGamma` preblend): the colour's luminance, quantised to the
/// three bits Skia keys its tables by, picks a curve that undoes what
/// blending in gamma space does to the stem weight.
pub fn mask_preblend(style: &crate::Style) -> [u8; 256] {
    let (r, g, b) = match style {
        crate::Style::Color(c) => {
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
            (q(c.r), q(c.g), q(c.b))
        }
        // SkPaintPriv::ComputeLuminanceColor for anything but a colour.
        crate::Style::Gradient(_) => (0x7f, 0x80, 0x7f),
    };
    // SkComputeLuminance, then the 3-bit table index scaled back to 8 bits.
    let lum = (r * 54 + g * 183 + b * 19) >> 8;
    let i = lum >> 5;
    let src_i = (i << 5) | (i << 2) | (i >> 1);
    correcting_lut(src_i as f32 / 255.0)
}

/// `SkTMaskGamma_build_correcting_lut` with gamma luminance on both sides.
fn correcting_lut(src: f32) -> [u8; 256] {
    let to_luma = |v: f32| mr_math::kernel::pow(v as f64, GAMMA as f64) as f32;
    let from_luma = |v: f32| mr_math::kernel::pow(v as f64, 1.0 / GAMMA as f64) as f32;
    let lin_src = to_luma(src);
    let dst = 1.0 - src;
    let lin_dst = to_luma(dst);
    // Contrast tapers off to 0 as the source luminance becomes white.
    let contrast = CONTRAST * lin_dst;
    let apply_contrast = |a: f32| a + (1.0 - a) * contrast * a;
    let mut t = [0u8; 256];
    for (i, out) in t.iter_mut().enumerate() {
        let raw = i as f32 / 255.0;
        let srca = apply_contrast(raw);
        *out = if (src - dst).abs() < 1.0 / 256.0 {
            (255.0 * srca).round() as u8
        } else {
            let lin_out = lin_src * srca + (1.0 - srca) * lin_dst;
            let o = from_luma(lin_out);
            // Undo what the blend will do.
            (255.0 * ((o - dst) / (src - dst)))
                .round()
                .clamp(0.0, 255.0) as u8
        };
    }
    t
}
