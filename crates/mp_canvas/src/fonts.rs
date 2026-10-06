//! Fonts: the faces text is drawn with, the CSS `font` shorthand as canvas
//! code writes it, and CSS font matching (DECISIONS D150, D152).
//!
//! The game names system fonts ("Arial Narrow", "Arial Black", Georgia...);
//! the port bundles open-licence substitutes under those names
//! (`assets/fonts/fonts.json`, [`FontBook::bundled`]), so every platform
//! draws the same signs. The JS reference capture registers the same files
//! under the same names. The faces are the owner's choice (DECISIONS D370:
//! the Roboto family for the Arials), subsets of the google/fonts files
//! (D371).

use std::sync::{Arc, OnceLock};

/// One face: a font file and the descriptors it is registered with.
#[derive(Clone)]
pub struct Face {
    /// The family it answers to, lower case (CSS family names match without
    /// regard to case).
    pub family: String,
    /// The weight range it covers. A variable font's `wght` axis follows the
    /// requested weight within it.
    pub weight: (f32, f32),
    pub italic: bool,
    pub data: Arc<[u8]>,
    /// The file it came from, for reports.
    pub file: String,
}

/// The faces text may use, and the generic families.
#[derive(Clone, Default)]
pub struct FontBook {
    pub faces: Vec<Face>,
    /// `(generic, family)`: `sans-serif` → `arial`, and so on.
    pub generic: Vec<(String, String)>,
    /// Families tried, after the font's own, for a character none of those
    /// has: what a browser's system font fallback is to the port (DECISIONS
    /// D372). Lower case.
    pub fallback: Vec<String>,
}

// `BUNDLED`, `BUNDLED_GENERIC`, `BUNDLED_FALLBACK` and the files' bytes,
// generated from `assets/fonts/fonts.json` by `build.rs`: the manifest is
// the one place the mapping is written (DECISIONS D373).
include!(concat!(env!("OUT_DIR"), "/bundled_fonts.rs"));

impl FontBook {
    /// An empty book: text draws nothing until faces are added.
    pub fn new() -> FontBook {
        FontBook::default()
    }

    /// The bundled substitutes, built once.
    pub fn bundled() -> Arc<FontBook> {
        static BOOK: OnceLock<Arc<FontBook>> = OnceLock::new();
        BOOK.get_or_init(|| {
            let mut b = FontBook::new();
            // One copy of each file, shared by the faces registered from it.
            let data: Vec<Arc<[u8]>> = BUNDLED_FILES.iter().map(|(_, d)| Arc::from(*d)).collect();
            for &(family, file, weight, style) in BUNDLED {
                let k = BUNDLED_FILES.iter().position(|(f, _)| *f == file).unwrap();
                b.add(family, data[k].clone(), weight, style == "italic", file);
            }
            for &(g, f) in BUNDLED_GENERIC {
                b.set_generic(g, f);
            }
            for &f in BUNDLED_FALLBACK {
                b.add_fallback(f);
            }
            Arc::new(b)
        })
        .clone()
    }

    /// Registers a face, as `new FontFace(family, data, {weight, style})`.
    pub fn add(
        &mut self,
        family: &str,
        data: Arc<[u8]>,
        weight: (f32, f32),
        italic: bool,
        file: &str,
    ) {
        assert!(
            skrifa::FontRef::new(&data).is_ok(),
            "mp_canvas: {file} is not a font"
        );
        self.faces.push(Face {
            family: family.to_ascii_lowercase(),
            weight,
            italic,
            data,
            file: file.to_string(),
        });
    }

    pub fn set_generic(&mut self, generic: &str, family: &str) {
        let g = generic.to_ascii_lowercase();
        self.generic.retain(|(k, _)| *k != g);
        self.generic.push((g, family.to_ascii_lowercase()));
    }

    /// Adds a family to the fallback list.
    pub fn add_fallback(&mut self, family: &str) {
        self.fallback.push(family.to_ascii_lowercase());
    }

    /// The face CSS font matching picks within a family, or `None` when the
    /// family has no faces (CSS Fonts 4, 5.2 step 4: style first, then
    /// weight).
    pub fn select(&self, family: &str, weight: f32, italic: bool) -> Option<&Face> {
        let mut fam = family.to_ascii_lowercase();
        if let Some((_, f)) = self.generic.iter().find(|(g, _)| *g == fam) {
            fam = f.clone();
        }
        let all: Vec<&Face> = self.faces.iter().filter(|f| f.family == fam).collect();
        if all.is_empty() {
            return None;
        }
        // Italic wants italic faces (an oblique would come next: none are
        // registered), else normal ones; normal wants normal, else italic.
        let pref: Vec<&Face> = all.iter().copied().filter(|f| f.italic == italic).collect();
        let set = if pref.is_empty() { all } else { pref };
        if let Some(f) = set
            .iter()
            .find(|f| f.weight.0 <= weight && weight <= f.weight.1)
        {
            return Some(f);
        }
        // Heavier first above 500, lighter first below 400; 400 to 500 look
        // up to 500, then down, then above 500.
        let below = || {
            set.iter()
                .copied()
                .filter(|f| f.weight.1 < weight)
                .max_by(|a, b| a.weight.1.total_cmp(&b.weight.1))
        };
        let above = |lim: f32| {
            set.iter()
                .copied()
                .filter(|f| f.weight.0 > weight && f.weight.0 <= lim)
                .min_by(|a, b| a.weight.0.total_cmp(&b.weight.0))
        };
        if weight > 500.0 {
            above(f32::INFINITY).or_else(below)
        } else if weight < 400.0 {
            below().or_else(|| above(f32::INFINITY))
        } else {
            above(500.0).or_else(below).or_else(|| above(f32::INFINITY))
        }
    }
}

/// A parsed CSS `font` value.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub italic: bool,
    pub weight: f32,
    /// CSS pixels.
    pub size: f32,
    /// Family names in order, unquoted.
    pub families: Vec<String>,
}

impl Default for FontSpec {
    /// The canvas default, `10px sans-serif`.
    fn default() -> FontSpec {
        FontSpec {
            italic: false,
            weight: 400.0,
            size: 10.0,
            families: vec!["sans-serif".to_string()],
        }
    }
}

/// Parses the `font` shorthand the way a canvas setter does: `[style]
/// [variant] [weight] [stretch] size[/line-height] family[, family]*`.
/// `None` for a value the browser would ignore.
pub fn parse_font(s: &str) -> Option<FontSpec> {
    let s = s.trim();
    let mut spec = FontSpec {
        families: Vec::new(),
        ..FontSpec::default()
    };
    let mut rest = s;
    loop {
        let tok_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let tok = &rest[..tok_end];
        if tok.is_empty() {
            return None;
        }
        let lower = tok.to_ascii_lowercase();
        if let Some(size) = parse_size(&lower) {
            spec.size = size;
            rest = rest[tok_end..].trim_start();
            break;
        }
        match lower.as_str() {
            "normal" | "small-caps" => {}
            "italic" | "oblique" => spec.italic = true,
            "bold" | "bolder" => spec.weight = 700.0,
            "lighter" => spec.weight = 100.0,
            "ultra-condensed" | "extra-condensed" | "condensed" | "semi-condensed"
            | "semi-expanded" | "expanded" | "extra-expanded" | "ultra-expanded" => {}
            _ => {
                let w: f32 = lower.parse().ok()?;
                if !(1.0..=1000.0).contains(&w) {
                    return None;
                }
                spec.weight = w;
            }
        }
        rest = rest[tok_end..].trim_start();
    }
    for part in split_families(rest)? {
        spec.families.push(part);
    }
    if spec.families.is_empty() {
        return None;
    }
    Some(spec)
}

fn parse_size(tok: &str) -> Option<f32> {
    let tok = tok.split('/').next()?;
    let (num, scale) = match tok.strip_suffix("px") {
        Some(n) => (n, 1.0),
        None => (tok.strip_suffix("pt")?, 4.0 / 3.0),
    };
    let v: f32 = num.parse().ok()?;
    (v >= 0.0 && v.is_finite()).then_some(v * scale)
}

fn split_families(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let p = part.trim();
        let name = if (p.starts_with('"') && p.ends_with('"')
            || p.starts_with('\'') && p.ends_with('\''))
            && p.len() >= 2
        {
            &p[1..p.len() - 1]
        } else {
            p
        };
        if name.is_empty() {
            return None;
        }
        out.push(name.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorthand() {
        let f = parse_font("bold 64px \"Arial Narrow\", Arial, sans-serif").unwrap();
        assert_eq!((f.weight, f.size, f.italic), (700.0, 64.0, false));
        assert_eq!(f.families, ["Arial Narrow", "Arial", "sans-serif"]);
        let f = parse_font("900 italic 64px \"Arial Narrow\", Arial").unwrap();
        assert_eq!((f.weight, f.italic), (900.0, true));
        let f = parse_font("italic bold 30px Georgia, serif").unwrap();
        assert_eq!((f.weight, f.italic, f.size), (700.0, true, 30.0));
        assert!(parse_font("bold Arial").is_none());
        assert!(parse_font("").is_none());
    }

    #[test]
    fn matching() {
        let b = FontBook::bundled();
        let f = |fam: &str, w: f32, i: bool| b.select(fam, w, i).map(|f| f.file.as_str());
        assert_eq!(
            f("Arial Narrow", 900.0, true),
            Some("robotocondensed/RobotoCondensed-Italic[wght].ttf")
        );
        assert_eq!(
            f("arial narrow", 700.0, false),
            Some("robotocondensed/RobotoCondensed[wght].ttf")
        );
        assert_eq!(
            f("Arial", 700.0, false),
            Some("roboto/Roboto[wdth,wght].ttf")
        );
        // Arial Black is Roboto at its Black weight, whatever is asked, from
        // the same bytes as Arial.
        let black = b.select("Arial Black", 700.0, false).unwrap();
        assert_eq!(black.weight, (900.0, 900.0));
        assert!(Arc::ptr_eq(
            &black.data,
            &b.select("Arial", 400.0, false).unwrap().data
        ));
        assert_eq!(
            f("Courier New", 900.0, false),
            Some("courierprime/CourierPrime-Bold.ttf")
        );
        assert_eq!(
            f("Courier New", 500.0, false),
            Some("courierprime/CourierPrime-Regular.ttf")
        );
        assert_eq!(
            f("Courier New", 300.0, false),
            Some("courierprime/CourierPrime-Regular.ttf")
        );
        assert_eq!(
            f("Rajdhani", 650.0, false),
            Some("rajdhani/Rajdhani-Bold.ttf")
        );
        assert_eq!(f("serif", 700.0, false), Some("gelasio/Gelasio[wght].ttf"));
        assert_eq!(f("Helvetica Neue", 700.0, false), None);
        assert_eq!(b.fallback, ["arimo"]);
    }

    #[test]
    fn bundled_table_is_the_manifest() {
        let m = include_str!("../../../assets/fonts/fonts.json");
        assert_eq!(m.matches("\"family\"").count(), BUNDLED.len());
        for &(family, file, _, _) in BUNDLED {
            assert!(
                m.contains(&format!("\"family\": \"{family}\", \"file\": \"{file}\"")),
                "{family} {file}"
            );
        }
        for &(g, f) in BUNDLED_GENERIC {
            assert!(m.contains(&format!("\"{g}\": \"{f}\"")), "generic {g}");
        }
        assert_eq!(BUNDLED_FALLBACK, ["Arimo"]);
    }
}
