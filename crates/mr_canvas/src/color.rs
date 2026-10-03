//! CSS colour strings as canvas `fillStyle`, `strokeStyle`, `shadowColor`
//! and gradient stops take them: `#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`,
//! `rgb()`/`rgba()` (comma or space separated, numbers or percentages),
//! `hsl()`/`hsla()`, `transparent` and the CSS named colours.
//!
//! A string that does not parse is ignored by the setter, as the browser
//! ignores it (the previous value stays).

use mr_math::js;

/// An unpremultiplied sRGB colour, components in 0..=1. Like Blink's
/// `Color`, the channels are kept as parsed (an `rgb()` component rounds to
/// a whole 8-bit value; alpha stays a float).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };
    pub const BLACK: Rgba = Rgba {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    pub fn from_u8(r: u8, g: u8, b: u8, a: f32) -> Rgba {
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a,
        }
    }

    /// Premultiplied components.
    pub fn premul(self) -> [f32; 4] {
        [self.r * self.a, self.g * self.a, self.b * self.a, self.a]
    }
}

/// Parses a CSS colour, or `None` for anything the browser would reject.
pub fn parse(s: &str) -> Option<Rgba> {
    let s = s.trim();
    let lower = s.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(open) = lower.find('(') {
        let name = lower[..open].trim();
        let inner = lower[open + 1..].strip_suffix(')')?;
        return match name {
            "rgb" | "rgba" => parse_rgb(inner),
            "hsl" | "hsla" => parse_hsl(inner),
            _ => None,
        };
    }
    if lower == "transparent" {
        return Some(Rgba::TRANSPARENT);
    }
    NAMED
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|&(_, v)| Rgba::from_u8((v >> 16) as u8, (v >> 8) as u8, v as u8, 1.0))
}

fn hex_digit(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

fn parse_hex(h: &str) -> Option<Rgba> {
    let d: Vec<u8> = h.bytes().map(hex_digit).collect::<Option<_>>()?;
    let (r, g, b, a) = match d.len() {
        3 => (d[0] * 17, d[1] * 17, d[2] * 17, 255),
        4 => (d[0] * 17, d[1] * 17, d[2] * 17, d[3] * 17),
        6 => (d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], 255),
        8 => (
            d[0] * 16 + d[1],
            d[2] * 16 + d[3],
            d[4] * 16 + d[5],
            d[6] * 16 + d[7],
        ),
        _ => return None,
    };
    Some(Rgba::from_u8(r, g, b, a as f32 / 255.0))
}

/// The arguments of a functional notation: comma separated, or space
/// separated with an optional `/ alpha`.
fn args(inner: &str) -> Option<Vec<&str>> {
    let inner = inner.trim();
    if inner.contains(',') {
        let v: Vec<&str> = inner.split(',').map(str::trim).collect();
        if v.iter().any(|a| a.is_empty()) {
            return None;
        }
        return Some(v);
    }
    let (main, alpha) = match inner.split_once('/') {
        Some((m, a)) => (m, Some(a.trim())),
        None => (inner, None),
    };
    let mut v: Vec<&str> = main.split_whitespace().collect();
    if let Some(a) = alpha {
        v.push(a);
    }
    Some(v)
}

/// A CSS `<number>`, as JS writes numbers into template strings.
fn number(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() || s.ends_with('.') {
        return None;
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

fn alpha(s: &str) -> Option<f32> {
    let v = match s.strip_suffix('%') {
        Some(p) => number(p)? / 100.0,
        None => number(s)?,
    };
    Some(v.clamp(0.0, 1.0) as f32)
}

fn parse_rgb(inner: &str) -> Option<Rgba> {
    let a = args(inner)?;
    if a.len() != 3 && a.len() != 4 {
        return None;
    }
    let mut c = [0.0f32; 3];
    for (k, s) in a[..3].iter().enumerate() {
        let v = match s.strip_suffix('%') {
            Some(p) => number(p)? * 255.0 / 100.0,
            None => number(s)?,
        };
        // Blink rounds each component to a whole 8-bit value.
        c[k] = js::round(v.clamp(0.0, 255.0)) as f32 / 255.0;
    }
    let al = if a.len() == 4 { alpha(a[3])? } else { 1.0 };
    Some(Rgba {
        r: c[0],
        g: c[1],
        b: c[2],
        a: al,
    })
}

fn parse_hsl(inner: &str) -> Option<Rgba> {
    let a = args(inner)?;
    if a.len() != 3 && a.len() != 4 {
        return None;
    }
    let h = number(a[0].trim_end_matches("deg"))?;
    let s = number(a[1].strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let l = number(a[2].strip_suffix('%')?)?.clamp(0.0, 100.0) / 100.0;
    let al = if a.len() == 4 { alpha(a[3])? } else { 1.0 };
    // CSS Color 4, hsl to rgb.
    let h = ((h % 360.0) + 360.0) % 360.0;
    let f = |n: f64| {
        let k = (n + h / 30.0) % 12.0;
        let t = s * l.min(1.0 - l);
        l - t * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    // Blink stores hsl() colours as 8-bit sRGB like rgb().
    let q = |v: f64| js::round((v * 255.0).clamp(0.0, 255.0)) as f32 / 255.0;
    Some(Rgba {
        r: q(f(0.0)),
        g: q(f(8.0)),
        b: q(f(4.0)),
        a: al,
    })
}

/// CSS named colours (CSS Color 4).
const NAMED: &[(&str, u32)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xa9a9a9),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("grey", 0x808080),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightgrey", 0xd3d3d3),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn u8s(c: Rgba) -> [u32; 4] {
        [
            (c.r * 255.0).round() as u32,
            (c.g * 255.0).round() as u32,
            (c.b * 255.0).round() as u32,
            (c.a * 255.0).round() as u32,
        ]
    }

    #[test]
    fn forms_in_use() {
        assert_eq!(u8s(parse("#111").unwrap()), [17, 17, 17, 255]);
        assert_eq!(u8s(parse("#f2c230").unwrap()), [242, 194, 48, 255]);
        assert_eq!(u8s(parse("rgb(66,64,62)").unwrap()), [66, 64, 62, 255]);
        assert_eq!(parse("rgba(20,18,16,0.5)").unwrap().a, 0.5);
        assert_eq!(u8s(parse("white").unwrap()), [255, 255, 255, 255]);
        assert_eq!(u8s(parse("hsl(0,100%,50%)").unwrap()), [255, 0, 0, 255]);
        assert_eq!(u8s(parse("hsl(120, 100%, 25%)").unwrap()), [0, 128, 0, 255]);
        assert_eq!(u8s(parse("rgba(255,255,255,0)").unwrap())[3], 0);
        assert!(parse("rgba(1,2)").is_none());
        assert!(parse("#12").is_none());
        assert!(parse("nonsense").is_none());
    }
}
