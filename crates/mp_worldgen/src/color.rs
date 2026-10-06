//! `THREE.Color` (three.js r180 `src/math/Color.js`) with three's colour
//! management as the game runs it: the working space is linear sRGB, so a
//! hex or CSS colour is converted from sRGB on the way in (`setHex`,
//! `setStyle`) and back on the way out (`getHex`), while `setRGB` and
//! `setHSL` take working-space values as they are.
//!
//! The arithmetic is three's, in its order; the transfer functions use
//! `mp_math::kernel::pow` (SPEC 4.2), so a colour matches the JS run with
//! the parity kernel bit for bit (`tests/builders.rs`).
//!
//! The setters return `&mut Self` so calls chain as in the JS; `Color` is
//! `Copy`, so `c.clone()` is a plain copy.

use mp_math::{js, kernel};

/// A linear RGB colour (three's `Color`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

/// `getHSL`'s target.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

impl Default for Color {
    /// `new Color()`: white.
    fn default() -> Self {
        Color::new(1.0, 1.0, 1.0)
    }
}

/// three's `SRGBToLinear`.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c < 0.04045 {
        c * 0.0773993808
    } else {
        kernel::pow(c * 0.9478672986 + 0.0521327014, 2.4)
    }
}

/// three's `LinearToSRGB`.
pub fn linear_to_srgb(c: f64) -> f64 {
    if c < 0.0031308 {
        c * 12.92
    } else {
        1.055 * (kernel::pow(c, 0.41666)) - 0.055
    }
}

/// three's `MathUtils.clamp`.
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    js::max(min, js::min(max, value))
}

/// three's `MathUtils.euclideanModulo`.
fn euclidean_modulo(n: f64, m: f64) -> f64 {
    ((n % m) + m) % m
}

/// three's `MathUtils.lerp`.
fn lerp(x: f64, y: f64, t: f64) -> f64 {
    (1.0 - t) * x + t * y
}

fn hue2rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * 6.0 * (2.0 / 3.0 - t);
    }
    p
}

impl Color {
    /// `new Color(r, g, b)`: working-space (linear) components.
    pub const fn new(r: f64, g: f64, b: f64) -> Self {
        Color { r, g, b }
    }

    /// `new Color(hex)`: an sRGB hex colour, converted to linear.
    pub fn hex(hex: u32) -> Self {
        let mut c = Color::default();
        c.set_hex(f64::from(hex));
        c
    }

    /// `new Color(style)`: a CSS colour string (`setStyle`).
    pub fn style(style: &str) -> Self {
        let mut c = Color::default();
        c.set_style(style);
        c
    }

    /// `setScalar(s)`.
    pub fn set_scalar(&mut self, scalar: f64) -> &mut Self {
        self.r = scalar;
        self.g = scalar;
        self.b = scalar;
        self
    }

    /// `setHex(hex)` in sRGB: `Math.floor`, then the bytes through ToInt32
    /// as the JS shifts do.
    pub fn set_hex(&mut self, hex: f64) -> &mut Self {
        let hex = js::to_int32(hex.floor());
        self.r = f64::from((hex >> 16) & 255) / 255.0;
        self.g = f64::from((hex >> 8) & 255) / 255.0;
        self.b = f64::from(hex & 255) / 255.0;
        self.srgb_to_working();
        self
    }

    /// `setRGB(r, g, b)` in the working space (no conversion).
    pub fn set_rgb(&mut self, r: f64, g: f64, b: f64) -> &mut Self {
        self.r = r;
        self.g = g;
        self.b = b;
        self
    }

    /// `setRGB(r, g, b, SRGBColorSpace)`.
    pub fn set_rgb_srgb(&mut self, r: f64, g: f64, b: f64) -> &mut Self {
        self.r = r;
        self.g = g;
        self.b = b;
        self.srgb_to_working();
        self
    }

    /// `setHSL(h, s, l)` in the working space.
    pub fn set_hsl(&mut self, h: f64, s: f64, l: f64) -> &mut Self {
        self.hsl(h, s, l);
        self
    }

    fn hsl(&mut self, h: f64, s: f64, l: f64) {
        // h,s,l ranges are in 0.0 - 1.0
        let h = euclidean_modulo(h, 1.0);
        let s = clamp(s, 0.0, 1.0);
        let l = clamp(l, 0.0, 1.0);
        if s == 0.0 {
            self.r = l;
            self.g = l;
            self.b = l;
        } else {
            let p = if l <= 0.5 {
                l * (1.0 + s)
            } else {
                l + s - (l * s)
            };
            let q = (2.0 * l) - p;
            self.r = hue2rgb(q, p, h + 1.0 / 3.0);
            self.g = hue2rgb(q, p, h);
            self.b = hue2rgb(q, p, h - 1.0 / 3.0);
        }
    }

    /// `setStyle(style)`: `rgb()`/`rgba()` (integers or percentages),
    /// `hsl()`/`hsla()`, `#rgb` and `#rrggbb`, all in sRGB. three's colour
    /// keywords (`'red'`) are not ported: the game never names one
    /// (DECISIONS D193); an unknown style leaves the colour as it was, as
    /// three does after its warning.
    pub fn set_style(&mut self, style: &str) -> &mut Self {
        if let Some(open) = style.find('(')
            && style.ends_with(')')
            && style[..open]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !style[..open].is_empty()
        {
            let name = &style[..open];
            let parts: Vec<&str> = style[open + 1..style.len() - 1]
                .split(',')
                .map(str::trim)
                .collect();
            if parts.len() == 3 || parts.len() == 4 {
                match name {
                    "rgb" | "rgba" => {
                        let int = |s: &str| -> Option<f64> {
                            (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                                .then(|| s.parse::<f64>().ok())
                                .flatten()
                        };
                        let pct = |s: &str| s.strip_suffix('%').and_then(int);
                        if let (Some(r), Some(g), Some(b)) =
                            (int(parts[0]), int(parts[1]), int(parts[2]))
                        {
                            let (r, g, b) = (
                                js::min(255.0, r) / 255.0,
                                js::min(255.0, g) / 255.0,
                                js::min(255.0, b) / 255.0,
                            );
                            return self.set_rgb_srgb(r, g, b);
                        }
                        if let (Some(r), Some(g), Some(b)) =
                            (pct(parts[0]), pct(parts[1]), pct(parts[2]))
                        {
                            let (r, g, b) = (
                                js::min(100.0, r) / 100.0,
                                js::min(100.0, g) / 100.0,
                                js::min(100.0, b) / 100.0,
                            );
                            return self.set_rgb_srgb(r, g, b);
                        }
                    }
                    "hsl" | "hsla" => {
                        let n = |s: &str| s.parse::<f64>().ok();
                        if let (Some(h), Some(s), Some(l)) = (
                            n(parts[0]),
                            parts[1].strip_suffix('%').and_then(n),
                            parts[2].strip_suffix('%').and_then(n),
                        ) {
                            self.hsl(h / 360.0, s / 100.0, l / 100.0);
                            self.srgb_to_working();
                            return self;
                        }
                    }
                    _ => {}
                }
            }
        } else if let Some(hex) = style.strip_prefix('#')
            && !hex.is_empty()
            && hex.bytes().all(|b| b.is_ascii_hexdigit())
        {
            let digit = |i: usize| f64::from(u8::from_str_radix(&hex[i..=i], 16).unwrap());
            if hex.len() == 3 {
                return self.set_rgb_srgb(digit(0) / 15.0, digit(1) / 15.0, digit(2) / 15.0);
            } else if hex.len() == 6 {
                return self.set_hex(f64::from(u32::from_str_radix(hex, 16).unwrap()));
            }
        }
        self
    }

    /// `ColorManagement.colorSpaceToWorking(this, SRGBColorSpace)`: the same
    /// primaries, so only the transfer function.
    fn srgb_to_working(&mut self) {
        self.r = srgb_to_linear(self.r);
        self.g = srgb_to_linear(self.g);
        self.b = srgb_to_linear(self.b);
    }

    /// `convertSRGBToLinear()`.
    pub fn convert_srgb_to_linear(&mut self) -> &mut Self {
        self.srgb_to_working();
        self
    }

    /// `convertLinearToSRGB()`.
    pub fn convert_linear_to_srgb(&mut self) -> &mut Self {
        self.r = linear_to_srgb(self.r);
        self.g = linear_to_srgb(self.g);
        self.b = linear_to_srgb(self.b);
        self
    }

    /// `getHex()`: the sRGB hex value.
    pub fn get_hex(&self) -> u32 {
        let mut c = *self;
        c.convert_linear_to_srgb();
        let byte = |v: f64| js::round(clamp(v * 255.0, 0.0, 255.0));
        (byte(c.r) * 65536.0 + byte(c.g) * 256.0 + byte(c.b)) as u32
    }

    /// `getHexString()`.
    pub fn get_hex_string(&self) -> String {
        format!("{:06x}", self.get_hex())
    }

    /// `getHSL(target)` in the working space.
    pub fn get_hsl(&self) -> Hsl {
        let (r, g, b) = (self.r, self.g, self.b);
        let max = js::max_n(&[r, g, b]);
        let min = js::min_n(&[r, g, b]);
        let hue;
        let saturation;
        let lightness = (min + max) / 2.0;
        if min == max {
            hue = 0.0;
            saturation = 0.0;
        } else {
            let delta = max - min;
            saturation = if lightness <= 0.5 {
                delta / (max + min)
            } else {
                delta / (2.0 - max - min)
            };
            // switch (max) { case r: ... case g: ... case b: ... }
            let h = if max == r {
                (g - b) / delta + if g < b { 6.0 } else { 0.0 }
            } else if max == g {
                (b - r) / delta + 2.0
            } else {
                (r - g) / delta + 4.0
            };
            hue = h / 6.0;
        }
        Hsl {
            h: hue,
            s: saturation,
            l: lightness,
        }
    }

    /// `offsetHSL(h, s, l)`.
    pub fn offset_hsl(&mut self, h: f64, s: f64, l: f64) -> &mut Self {
        let hsl = self.get_hsl();
        self.set_hsl(hsl.h + h, hsl.s + s, hsl.l + l)
    }

    pub fn add(&mut self, c: Color) -> &mut Self {
        self.r += c.r;
        self.g += c.g;
        self.b += c.b;
        self
    }

    pub fn add_scalar(&mut self, s: f64) -> &mut Self {
        self.r += s;
        self.g += s;
        self.b += s;
        self
    }

    pub fn multiply(&mut self, c: Color) -> &mut Self {
        self.r *= c.r;
        self.g *= c.g;
        self.b *= c.b;
        self
    }

    pub fn multiply_scalar(&mut self, s: f64) -> &mut Self {
        self.r *= s;
        self.g *= s;
        self.b *= s;
        self
    }

    pub fn lerp(&mut self, c: Color, alpha: f64) -> &mut Self {
        self.r += (c.r - self.r) * alpha;
        self.g += (c.g - self.g) * alpha;
        self.b += (c.b - self.b) * alpha;
        self
    }

    pub fn lerp_colors(&mut self, c1: Color, c2: Color, alpha: f64) -> &mut Self {
        self.r = c1.r + (c2.r - c1.r) * alpha;
        self.g = c1.g + (c2.g - c1.g) * alpha;
        self.b = c1.b + (c2.b - c1.b) * alpha;
        self
    }

    /// `lerpHSL(color, alpha)`.
    pub fn lerp_hsl(&mut self, c: Color, alpha: f64) -> &mut Self {
        let a = self.get_hsl();
        let b = c.get_hsl();
        let h = lerp(a.h, b.h, alpha);
        let s = lerp(a.s, b.s, alpha);
        let l = lerp(a.l, b.l, alpha);
        self.set_hsl(h, s, l)
    }

    /// `[r, g, b]`.
    pub fn to_array(self) -> [f64; 3] {
        [self.r, self.g, self.b]
    }
}
