//! `Canvas`: a canvas element and its 2D context in one, with the context's
//! methods under their JS names in snake case.

use std::rc::Rc;
use std::sync::Arc;

use tiny_skia::{LineCap, LineJoin, Path, Pixmap, Stroke};

use crate::Matrix;
use crate::color::{self, Rgba};
use crate::fonts::{FontBook, FontSpec, parse_font};
use crate::paint::{Gradient, GradientKind, IntoStyle, Op, Source, Style, blur};
use crate::path::CanvasPath;
use crate::raster::{Coverage, IBox, fill_coverage};
use crate::text::{Align, Baseline, layout};

/// `ImageData`: unpremultiplied RGBA rows, top first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl ImageData {
    pub fn new(width: u32, height: u32) -> ImageData {
        ImageData {
            width,
            height,
            data: vec![0; width as usize * height as usize * 4],
        }
    }

    /// `img.data[i] = v` on the `Uint8ClampedArray`.
    #[inline]
    pub fn set(&mut self, i: usize, v: f64) {
        self.data[i] = to_uint8_clamp(v);
    }
}

/// ECMAScript ToUint8Clamp: what storing a number into a
/// `Uint8ClampedArray` does (NaN → 0, clamp, round half to even).
#[inline]
pub fn to_uint8_clamp(v: f64) -> u8 {
    if v.is_nan() || v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v.round_ties_even() as u8
    }
}

/// `measureText` result (the width is all the game reads).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextMetrics {
    pub width: f64,
}

#[derive(Clone)]
struct State {
    m: Matrix,
    fill: Style,
    stroke: Style,
    global_alpha: f64,
    op: Op,
    line_width: f64,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f64,
    shadow_blur: f64,
    shadow_color: Rgba,
    /// `filter`: the string as set, and the blur's standard deviation (0 for
    /// none).
    filter: String,
    filter_blur: f64,
    font: String,
    font_spec: FontSpec,
    align: Align,
    baseline: Baseline,
    clip: Option<Rc<Coverage>>,
}

impl Default for State {
    fn default() -> State {
        State {
            m: Matrix::IDENTITY,
            fill: Style::Color(Rgba::BLACK),
            stroke: Style::Color(Rgba::BLACK),
            global_alpha: 1.0,
            op: Op::SourceOver,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            shadow_blur: 0.0,
            shadow_color: Rgba::TRANSPARENT,
            filter: "none".to_string(),
            filter_blur: 0.0,
            font: "10px sans-serif".to_string(),
            font_spec: FontSpec::default(),
            align: Align::Start,
            baseline: Baseline::Alphabetic,
            clip: None,
        }
    }
}

/// A canvas of `width × height` premultiplied RGBA8 pixels (as Chrome keeps
/// it) and its 2D context state.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixmap: Pixmap,
    state: State,
    stack: Vec<State>,
    path: CanvasPath,
    fonts: Arc<FontBook>,
}

impl Canvas {
    /// A transparent canvas that draws text with the bundled fonts.
    pub fn new(width: u32, height: u32) -> Canvas {
        Canvas::with_fonts(width, height, FontBook::bundled())
    }

    pub fn with_fonts(width: u32, height: u32, fonts: Arc<FontBook>) -> Canvas {
        Canvas {
            width,
            height,
            pixmap: Pixmap::new(width.max(1), height.max(1)).expect("canvas size"),
            state: State::default(),
            stack: Vec::new(),
            path: CanvasPath::new(),
            fonts,
        }
    }

    fn bounds(&self) -> IBox {
        IBox {
            x0: 0,
            y0: 0,
            x1: self.width as i32,
            y1: self.height as i32,
        }
    }

    /// Premultiplied RGBA8 pixels.
    pub fn premultiplied(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// The whole canvas as `getImageData` returns it (unpremultiplied); this
    /// is what a `CanvasTexture` uploads.
    pub fn to_rgba(&self) -> Vec<u8> {
        self.get_image_data(0, 0, self.width, self.height).data
    }

    // ── State ─────────────────────────────────────────────────────────

    pub fn save(&mut self) {
        self.stack.push(self.state.clone());
    }

    pub fn restore(&mut self) {
        if let Some(s) = self.stack.pop() {
            self.state = s;
        }
    }

    pub fn translate(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.state.m = self.state.m.mul(&Matrix::translate(x, y));
        }
    }

    pub fn rotate(&mut self, angle: f64) {
        if angle.is_finite() {
            self.state.m = self.state.m.mul(&Matrix::rotate(angle));
        }
    }

    pub fn scale(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.state.m = self.state.m.mul(&Matrix::scale(x, y));
        }
    }

    /// The current transform (for tests and tools).
    pub fn transform_matrix(&self) -> Matrix {
        self.state.m
    }

    pub fn set_fill_style(&mut self, s: impl IntoStyle) {
        if let Some(s) = s.into_style() {
            self.state.fill = s;
        }
    }

    pub fn set_stroke_style(&mut self, s: impl IntoStyle) {
        if let Some(s) = s.into_style() {
            self.state.stroke = s;
        }
    }

    pub fn set_global_alpha(&mut self, a: f64) {
        if (0.0..=1.0).contains(&a) {
            self.state.global_alpha = a;
        }
    }

    pub fn set_global_composite_operation(&mut self, op: &str) {
        if let Some(op) = Op::parse(op) {
            self.state.op = op;
        }
    }

    pub fn set_line_width(&mut self, w: f64) {
        if w.is_finite() && w > 0.0 {
            self.state.line_width = w;
        }
    }

    pub fn set_line_cap(&mut self, cap: &str) {
        self.state.line_cap = match cap {
            "butt" => LineCap::Butt,
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => return,
        };
    }

    pub fn set_line_join(&mut self, join: &str) {
        self.state.line_join = match join {
            "miter" => LineJoin::Miter,
            "round" => LineJoin::Round,
            "bevel" => LineJoin::Bevel,
            _ => return,
        };
    }

    pub fn set_miter_limit(&mut self, v: f64) {
        if v.is_finite() && v > 0.0 {
            self.state.miter_limit = v;
        }
    }

    pub fn set_shadow_blur(&mut self, v: f64) {
        if v.is_finite() && v >= 0.0 {
            self.state.shadow_blur = v;
        }
    }

    pub fn set_shadow_color(&mut self, c: &str) {
        if let Some(c) = color::parse(c) {
            self.state.shadow_color = c;
        }
    }

    /// `filter`: `none` or `blur(<n>px)`, the values in use.
    pub fn set_filter(&mut self, f: &str) {
        let t = f.trim();
        if t == "none" {
            self.state.filter_blur = 0.0;
        } else if let Some(px) = t.strip_prefix("blur(").and_then(|r| r.strip_suffix("px)")) {
            match px.trim().parse::<f64>() {
                Ok(v) if v >= 0.0 => self.state.filter_blur = v,
                _ => return,
            }
        } else {
            panic!("mr_canvas: filter {f:?} is not implemented");
        }
        self.state.filter = t.to_string();
    }

    pub fn filter(&self) -> &str {
        &self.state.filter
    }

    pub fn set_font(&mut self, f: &str) {
        if let Some(spec) = parse_font(f) {
            self.state.font_spec = spec;
            self.state.font = f.trim().to_string();
        }
    }

    /// The `font` as set (Chrome serialises it; the game only reads the
    /// pixel size back out of it).
    pub fn font(&self) -> &str {
        &self.state.font
    }

    pub fn set_text_align(&mut self, a: &str) {
        if let Some(a) = Align::parse(a) {
            self.state.align = a;
        }
    }

    pub fn set_text_baseline(&mut self, b: &str) {
        if let Some(b) = Baseline::parse(b) {
            self.state.baseline = b;
        }
    }

    // ── Gradients ─────────────────────────────────────────────────────

    pub fn create_linear_gradient(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> Gradient {
        Gradient {
            kind: GradientKind::Linear { x0, y0, x1, y1 },
            stops: Vec::new(),
        }
    }

    pub fn create_radial_gradient(
        &self,
        x0: f64,
        y0: f64,
        r0: f64,
        x1: f64,
        y1: f64,
        r1: f64,
    ) -> Gradient {
        assert!(
            r0 >= 0.0 && r1 >= 0.0,
            "createRadialGradient: negative radius"
        );
        Gradient {
            kind: GradientKind::Radial {
                x0,
                y0,
                r0,
                x1,
                y1,
                r1,
            },
            stops: Vec::new(),
        }
    }

    // ── Paths ─────────────────────────────────────────────────────────

    pub fn begin_path(&mut self) {
        self.path = CanvasPath::new();
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.path.move_to(&self.state.m, x, y);
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        self.path.line_to(&self.state.m, x, y);
    }

    pub fn quadratic_curve_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.path.quadratic_curve_to(&self.state.m, cx, cy, x, y);
    }

    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.path
            .bezier_curve_to(&self.state.m, c1x, c1y, c2x, c2y, x, y);
    }

    pub fn close_path(&mut self) {
        self.path.close_path();
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.path.rect(&self.state.m, x, y, w, h);
    }

    pub fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.path.round_rect(&self.state.m, x, y, w, h, r);
    }

    /// `arc(x, y, r, start, end, anticlockwise)`; pass `false` where the JS
    /// leaves the last argument out.
    pub fn arc(&mut self, x: f64, y: f64, r: f64, a0: f64, a1: f64, anticlockwise: bool) {
        self.path.arc(&self.state.m, x, y, r, a0, a1, anticlockwise);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ellipse(
        &mut self,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        a0: f64,
        a1: f64,
        anticlockwise: bool,
    ) {
        self.path
            .ellipse(&self.state.m, x, y, rx, ry, rotation, a0, a1, anticlockwise);
    }

    pub fn fill(&mut self) {
        let Some(p) = self.path.finish() else { return };
        let style = self.state.fill.clone();
        self.draw(&style, |limit| fill_coverage(&p, limit));
    }

    pub fn stroke(&mut self) {
        let Some(p) = self.path.finish() else { return };
        let Some(outline) = self.stroke_outline(&p) else {
            return;
        };
        let style = self.state.stroke.clone();
        self.draw(&style, |limit| fill_coverage(&outline, limit));
    }

    /// `clip()` with the nonzero rule: intersects the clip region with the
    /// current path.
    pub fn clip(&mut self) {
        let b = self.bounds();
        let cov = match self.path.finish() {
            Some(p) => fill_coverage(&p, b),
            None => Coverage::empty(),
        };
        let cov = match &self.state.clip {
            None => cov,
            Some(old) => {
                let mut c = cov;
                for y in 0..c.h {
                    for x in 0..c.w {
                        c.data[y * c.w + x] *= old.at(c.x0 + x as i32, c.y0 + y as i32);
                    }
                }
                c
            }
        };
        self.state.clip = Some(Rc::new(cov));
    }

    /// The stroke of a device-space path: back to user space, stroked with
    /// the line settings there, then to device space again.
    fn stroke_outline(&self, p: &Path) -> Option<Path> {
        let m = self.state.m;
        let inv = m.invert()?;
        let user = p.clone().transform(inv.to_skia())?;
        let stroke = Stroke {
            width: self.state.line_width as f32,
            miter_limit: self.state.miter_limit as f32,
            line_cap: self.state.line_cap,
            line_join: self.state.line_join,
            dash: None,
        };
        let res = tiny_skia::PathStroker::compute_resolution_scale(&m.to_skia());
        user.stroke(&stroke, res)?.transform(m.to_skia())
    }

    // ── Rectangles ────────────────────────────────────────────────────

    pub fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let Some(p) = self.rect_path(x, y, w, h) else {
            return;
        };
        let style = self.state.fill.clone();
        self.draw(&style, |limit| fill_coverage(&p, limit));
    }

    pub fn stroke_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let Some(p) = self.rect_path(x, y, w, h) else {
            return;
        };
        let Some(outline) = self.stroke_outline(&p) else {
            return;
        };
        let style = self.state.stroke.clone();
        self.draw(&style, |limit| fill_coverage(&outline, limit));
    }

    /// Clears to transparent black through the transform and clip;
    /// unaffected by alpha, compositing, shadows and filters.
    pub fn clear_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let Some(p) = self.rect_path(x, y, w, h) else {
            return;
        };
        let cov = fill_coverage(&p, self.bounds());
        let clip = self.state.clip.clone();
        self.composite(
            &cov,
            &Source::Solid([0.0, 0.0, 0.0, 1.0]),
            Op::Clear,
            clip.as_deref(),
        );
    }

    fn rect_path(&self, x: f64, y: f64, w: f64, h: f64) -> Option<Path> {
        if ![x, y, w, h].iter().all(|v| v.is_finite()) || w == 0.0 && h == 0.0 {
            return None;
        }
        let mut p = CanvasPath::new();
        p.rect(&self.state.m, x, y, w, h);
        p.finish()
    }

    // ── Text ──────────────────────────────────────────────────────────

    pub fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.text(text, x, y, None, false);
    }

    /// `fillText(text, x, y, maxWidth)`.
    pub fn fill_text_max(&mut self, text: &str, x: f64, y: f64, max_width: f64) {
        self.text(text, x, y, Some(max_width), false);
    }

    pub fn stroke_text(&mut self, text: &str, x: f64, y: f64) {
        self.text(text, x, y, None, true);
    }

    /// `strokeText(text, x, y, maxWidth)`.
    pub fn stroke_text_max(&mut self, text: &str, x: f64, y: f64, max_width: f64) {
        self.text(text, x, y, Some(max_width), true);
    }

    pub fn measure_text(&self, text: &str) -> TextMetrics {
        TextMetrics {
            width: layout(&self.fonts, &self.state.font_spec, text).width,
        }
    }

    fn text(&mut self, text: &str, x: f64, y: f64, max_width: Option<f64>, stroke: bool) {
        if !(x.is_finite() && y.is_finite())
            || max_width.is_some_and(|w| !(w.is_finite() && w > 0.0))
        {
            return;
        }
        let fonts = self.fonts.clone();
        let lay = layout(&fonts, &self.state.font_spec, text);
        let width = lay.width;
        let squeeze = max_width.is_some_and(|w| w < width);
        let allowed = if squeeze { max_width.unwrap() } else { width };
        let ax = match self.state.align {
            Align::Center => x - allowed / 2.0,
            Align::Right | Align::End => x - allowed,
            Align::Left | Align::Start => x,
        };
        let by = y + lay.baseline(self.state.baseline);
        // Blink squeezes over-long text with a horizontal scale about its
        // left end.
        let (m, ox, oy) = if squeeze {
            let s = if width > 0.0 { allowed / width } else { 0.0 };
            (
                self.state
                    .m
                    .mul(&Matrix::translate(ax, by))
                    .mul(&Matrix::scale(s, 1.0)),
                0.0,
                0.0,
            )
        } else {
            (self.state.m, ax, by)
        };
        let glyphs = lay.glyph_paths(&m, ox, oy);
        let saved = self.state.m;
        self.state.m = m;
        let style = if stroke {
            self.state.stroke.clone()
        } else {
            self.state.fill.clone()
        };
        let outlines: Vec<Path> = if stroke {
            glyphs
                .iter()
                .filter_map(|g| self.stroke_outline(g))
                .collect()
        } else {
            glyphs
        };
        self.state.m = saved;
        // Filled glyphs are A8 masks that Skia corrects for gamma and
        // contrast by the text colour's luminance; strokes and glyphs too big
        // for the atlas are drawn as paths, uncorrected.
        let ppem = lay.size * m.d.abs().max(m.b.abs());
        let table = (!stroke && ppem <= 256.0).then(|| crate::text::mask_preblend(&style));
        // Each glyph is its own mask, composited as overlapping masks do.
        self.draw(&style, |limit| {
            outlines.iter().fold(Coverage::empty(), |acc, g| {
                let mut c = fill_coverage(g, limit);
                if let Some(t) = &table {
                    for v in &mut c.data {
                        *v = t[(*v * 255.0 + 0.5) as usize] as f32 / 255.0;
                    }
                }
                acc.union(&c)
            })
        });
    }

    // ── Pixels ────────────────────────────────────────────────────────

    pub fn create_image_data(&self, w: u32, h: u32) -> ImageData {
        ImageData::new(w, h)
    }

    /// Unpremultiplied pixels of a rectangle (outside the canvas reads as
    /// transparent black).
    pub fn get_image_data(&self, x: i32, y: i32, w: u32, h: u32) -> ImageData {
        let mut out = ImageData::new(w, h);
        let src = self.pixmap.data();
        for j in 0..h as i32 {
            let sy = y + j;
            if sy < 0 || sy >= self.height as i32 {
                continue;
            }
            for i in 0..w as i32 {
                let sx = x + i;
                if sx < 0 || sx >= self.width as i32 {
                    continue;
                }
                let s = (sy as usize * self.width as usize + sx as usize) * 4;
                let d = (j as usize * w as usize + i as usize) * 4;
                let a = src[s + 3];
                if a == 0 {
                    continue;
                }
                for c in 0..3 {
                    // Skia's unpremultiply: c / a in float, rounded.
                    out.data[d + c] =
                        ((src[s + c] as f32 * 255.0 / a as f32) + 0.5).min(255.0) as u8;
                }
                out.data[d + 3] = a;
            }
        }
        out
    }

    /// Writes pixels as they are (premultiplied on the way in), ignoring
    /// the transform, clip, alpha and compositing.
    pub fn put_image_data(&mut self, img: &ImageData, x: i32, y: i32) {
        let (cw, ch) = (self.width as i32, self.height as i32);
        let dst = self.pixmap.data_mut();
        for j in 0..img.height as i32 {
            let dy = y + j;
            if dy < 0 || dy >= ch {
                continue;
            }
            for i in 0..img.width as i32 {
                let dx = x + i;
                if dx < 0 || dx >= cw {
                    continue;
                }
                let s = (j as usize * img.width as usize + i as usize) * 4;
                let d = (dy as usize * cw as usize + dx as usize) * 4;
                let a = img.data[s + 3];
                for c in 0..3 {
                    dst[d + c] = if a == 255 {
                        img.data[s + c]
                    } else {
                        ((img.data[s + c] as f32 * a as f32 / 255.0) + 0.5) as u8
                    };
                }
                dst[d + 3] = a;
            }
        }
    }

    /// `drawImage(canvas, dx, dy, dw, dh)`.
    pub fn draw_image(&mut self, src: &Canvas, dx: f64, dy: f64, dw: f64, dh: f64) {
        self.draw_image_sub(
            src,
            0.0,
            0.0,
            src.width as f64,
            src.height as f64,
            dx,
            dy,
            dw,
            dh,
        );
    }

    /// `drawImage(canvas, sx, sy, sw, sh, dx, dy, dw, dh)`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_image_sub(
        &mut self,
        src: &Canvas,
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    ) {
        if sw == 0.0 || sh == 0.0 || dw == 0.0 || dh == 0.0 {
            return;
        }
        let Some(p) = self.rect_path(dx, dy, dw, dh) else {
            return;
        };
        // Device → user → source pixels.
        let Some(inv) = self.state.m.invert() else {
            return;
        };
        let to_src = Matrix::translate(sx, sy)
            .mul(&Matrix::scale(sw / dw, sh / dh))
            .mul(&Matrix::translate(-dx, -dy))
            .mul(&inv);
        let (x0, y0) = (sx.max(0.0).floor(), sy.max(0.0).floor());
        let x1 = (sx + sw).min(src.width as f64).ceil();
        let y1 = (sy + sh).min(src.height as f64).ceil();
        let source = Source::Image {
            data: src.pixmap.data(),
            width: src.width as usize,
            inv: to_src,
            rect: (x0, y0, x1, y1),
            alpha: self.state.global_alpha as f32,
        };
        let cov = fill_coverage(&p, self.pad_bounds());
        self.draw_source(cov, &source);
    }

    // ── Drawing ───────────────────────────────────────────────────────

    /// How far outside the canvas a shape still matters: a shadow or a blur
    /// filter spreads it.
    fn pad(&self) -> i32 {
        let mut sigma: f64 = 0.0;
        if self.shadow_on() {
            sigma = sigma.max(self.state.shadow_blur / 2.0);
        }
        sigma = sigma.max(self.state.filter_blur);
        (3.0 * sigma).ceil() as i32
    }

    fn pad_bounds(&self) -> IBox {
        let p = self.pad();
        let b = self.bounds();
        IBox {
            x0: b.x0 - p,
            y0: b.y0 - p,
            x1: b.x1 + p,
            y1: b.y1 + p,
        }
    }

    fn shadow_on(&self) -> bool {
        self.state.shadow_color.a > 0.0 && self.state.shadow_blur > 0.0
    }

    fn draw(&mut self, style: &Style, raster: impl FnOnce(IBox) -> Coverage) {
        let cov = raster(self.pad_bounds());
        let m = self.state.m;
        let Some(src) = Source::from_style(style, &m, self.state.global_alpha) else {
            return;
        };
        self.draw_source(cov, &src);
    }

    /// The canvas drawing model: shadow first, then the shape (through the
    /// filter if one is set), each composited with the current operation
    /// inside the clip.
    fn draw_source(&mut self, cov: Coverage, src: &Source) {
        if cov.is_empty() {
            return;
        }
        let clip = self.state.clip.clone();
        let op = self.state.op;
        if self.shadow_on() {
            let sigma = self.state.shadow_blur / 2.0;
            let r = (3.0 * sigma).ceil() as i32;
            let (x0, y0) = (cov.x0 - r, cov.y0 - r);
            let (w, h) = (cov.w + 2 * r as usize, cov.h + 2 * r as usize);
            let mut a = vec![0.0f32; w * h];
            for y in 0..cov.h {
                for x in 0..cov.w {
                    let c = cov.data[y * cov.w + x];
                    if c > 0.0 {
                        let s = src.sample(
                            (cov.x0 + x as i32) as f64 + 0.5,
                            (cov.y0 + y as i32) as f64 + 0.5,
                        );
                        a[(y + r as usize) * w + x + r as usize] = s[3] * c;
                    }
                }
            }
            blur(&mut a, w, h, 1, sigma);
            let sc = self.state.shadow_color.premul();
            let shadow = Coverage {
                x0,
                y0,
                w,
                h,
                data: a,
            };
            self.composite(&shadow, &Source::Solid(sc), op, clip.as_deref());
        }
        if self.state.filter_blur > 0.0 {
            let sigma = self.state.filter_blur;
            let r = (3.0 * sigma).ceil() as i32;
            let (x0, y0) = (cov.x0 - r, cov.y0 - r);
            let (w, h) = (cov.w + 2 * r as usize, cov.h + 2 * r as usize);
            let mut layer = vec![0.0f32; w * h * 4];
            for y in 0..cov.h {
                for x in 0..cov.w {
                    let c = cov.data[y * cov.w + x];
                    if c > 0.0 {
                        let s = src.sample(
                            (cov.x0 + x as i32) as f64 + 0.5,
                            (cov.y0 + y as i32) as f64 + 0.5,
                        );
                        let i = ((y + r as usize) * w + x + r as usize) * 4;
                        for k in 0..4 {
                            layer[i + k] = s[k] * c;
                        }
                    }
                }
            }
            blur(&mut layer, w, h, 4, sigma);
            self.composite_layer(x0, y0, w, h, &layer, op, clip.as_deref());
        } else {
            self.composite(&cov, src, op, clip.as_deref());
        }
    }

    /// Blends `src` through `cov` (and the clip) onto the pixels.
    fn composite(&mut self, cov: &Coverage, src: &Source, op: Op, clip: Option<&Coverage>) {
        if src.is_clear() && matches!(op, Op::SourceOver | Op::Lighter | Op::DestinationOut) {
            return;
        }
        let b = IBox {
            x0: cov.x0,
            y0: cov.y0,
            x1: cov.x0 + cov.w as i32,
            y1: cov.y0 + cov.h as i32,
        };
        let Some(b) = b.intersect(self.bounds()) else {
            return;
        };
        let cw = self.width as usize;
        let dst = self.pixmap.data_mut();
        for y in b.y0..b.y1 {
            for x in b.x0..b.x1 {
                let mut c = cov.data[(y - cov.y0) as usize * cov.w + (x - cov.x0) as usize];
                if let Some(cl) = clip {
                    c *= cl.at(x, y);
                }
                if c <= 0.0 {
                    continue;
                }
                let s = if op == Op::Clear {
                    [0.0, 0.0, 0.0, c]
                } else {
                    let s = src.sample(x as f64 + 0.5, y as f64 + 0.5);
                    [s[0] * c, s[1] * c, s[2] * c, s[3] * c]
                };
                let i = (y as usize * cw + x as usize) * 4;
                let d = load(&dst[i..i + 4]);
                store(&mut dst[i..i + 4], op.blend(s, d));
            }
        }
    }

    /// Blends a premultiplied float layer onto the pixels (through the clip).
    #[allow(clippy::too_many_arguments)]
    fn composite_layer(
        &mut self,
        x0: i32,
        y0: i32,
        w: usize,
        h: usize,
        layer: &[f32],
        op: Op,
        clip: Option<&Coverage>,
    ) {
        let b = IBox {
            x0,
            y0,
            x1: x0 + w as i32,
            y1: y0 + h as i32,
        };
        let Some(b) = b.intersect(self.bounds()) else {
            return;
        };
        let cw = self.width as usize;
        let dst = self.pixmap.data_mut();
        for y in b.y0..b.y1 {
            for x in b.x0..b.x1 {
                let li = ((y - y0) as usize * w + (x - x0) as usize) * 4;
                let mut s = [layer[li], layer[li + 1], layer[li + 2], layer[li + 3]];
                if let Some(cl) = clip {
                    let c = cl.at(x, y);
                    s = [s[0] * c, s[1] * c, s[2] * c, s[3] * c];
                }
                if s[3] <= 0.0 && s[0] <= 0.0 && s[1] <= 0.0 && s[2] <= 0.0 {
                    continue;
                }
                let i = (y as usize * cw + x as usize) * 4;
                let d = load(&dst[i..i + 4]);
                store(&mut dst[i..i + 4], op.blend(s, d));
            }
        }
    }
}

#[inline]
fn load(p: &[u8]) -> [f32; 4] {
    [
        p[0] as f32 / 255.0,
        p[1] as f32 / 255.0,
        p[2] as f32 / 255.0,
        p[3] as f32 / 255.0,
    ]
}

#[inline]
fn store(p: &mut [u8], v: [f32; 4]) {
    let a = (v[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    p[3] = a;
    for k in 0..3 {
        // Premultiplied: a channel never exceeds alpha.
        p[k] = ((v[k].clamp(0.0, 1.0) * 255.0 + 0.5) as u8).min(a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uint8_clamped_stores() {
        assert_eq!(to_uint8_clamp(0.5), 0);
        assert_eq!(to_uint8_clamp(1.5), 2);
        assert_eq!(to_uint8_clamp(2.5), 2);
        assert_eq!(to_uint8_clamp(254.6), 255);
        assert_eq!(to_uint8_clamp(-3.0), 0);
        assert_eq!(to_uint8_clamp(f64::NAN), 0);
        assert_eq!(to_uint8_clamp(300.0), 255);
    }

    #[test]
    fn image_data_round_trip() {
        let mut g = Canvas::new(4, 1);
        let mut img = g.create_image_data(4, 1);
        img.data.copy_from_slice(&[
            200, 100, 50, 255, 200, 100, 50, 128, 255, 255, 255, 3, 9, 9, 9, 0,
        ]);
        g.put_image_data(&img, 0, 0);
        let back = g.get_image_data(0, 0, 4, 1);
        // Opaque pixels survive; translucent ones lose precision in the
        // premultiplied store; transparent ones read back as black.
        assert_eq!(&back.data[..4], &[200, 100, 50, 255]);
        assert_eq!(back.data[7], 128);
        assert_eq!(&back.data[12..], &[0, 0, 0, 0]);
        // Outside the canvas reads as transparent black.
        assert_eq!(g.get_image_data(-1, 0, 1, 1).data, vec![0, 0, 0, 0]);
    }

    #[test]
    fn fill_rect_covers_exact_area() {
        let mut g = Canvas::new(8, 8);
        g.set_fill_style("#fff");
        g.fill_rect(1.5, 0.0, 5.0, 8.0);
        let d = g.to_rgba();
        assert_eq!(d[4 + 3], 128);
        assert_eq!(d[2 * 4 + 3], 255);
        assert_eq!(d[6 * 4 + 3], 128);
        assert_eq!(d[7 * 4 + 3], 0);
    }

    #[test]
    fn save_restore_and_transform() {
        let mut g = Canvas::new(8, 8);
        g.save();
        g.translate(4.0, 4.0);
        g.set_fill_style("#f00");
        g.fill_rect(0.0, 0.0, 2.0, 2.0);
        g.restore();
        g.fill_rect(0.0, 0.0, 1.0, 1.0);
        let d = g.to_rgba();
        let i = (4 * 8 + 4) * 4;
        assert_eq!(&d[i..i + 4], &[255, 0, 0, 255]);
        // The fill style was restored to black.
        assert_eq!(&d[..4], &[0, 0, 0, 255]);
    }
}
