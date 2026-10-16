//! The game's fonts for canvas text: open-licence substitutes for the
//! system fonts the JS game names ("Arial Narrow", "Arial Black", Georgia,
//! "Brush Script MT", "Segoe Script", "Courier New"; SPEC 5.3), registered
//! under those names so every platform draws the same signs. The faces are
//! the owner's choice (DECISIONS D370: the Roboto family for the Arials),
//! subsets of the google/fonts files (D371), listed once in
//! `assets/fonts/fonts.json`, which `build.rs` turns into the table below
//! (D373). The JS reference capture registers the same files under the
//! same names (`tools/parity/textures.html`).
//!
//! The canvas itself is the kit's `pk_canvas`, which carries no fonts
//! (D1232): texture code draws on [`canvas`], a `Canvas::with_fonts` on the
//! one shared [`book`].

use std::sync::{Arc, OnceLock};

use pk_canvas::{Canvas, FontBook};

// `BUNDLED`, `BUNDLED_GENERIC`, `BUNDLED_FALLBACK` and the files' bytes,
// generated from `assets/fonts/fonts.json` by `build.rs`: the manifest is
// the one place the mapping is written (DECISIONS D373).
include!(concat!(env!("OUT_DIR"), "/bundled_fonts.rs"));

/// The bundled substitutes, built once and shared by every canvas.
pub fn book() -> Arc<FontBook> {
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

/// A transparent canvas that draws text with the game's fonts: what
/// `document.createElement('canvas')` is to the JS texture code.
pub fn canvas(width: u32, height: u32) -> Canvas {
    Canvas::with_fonts(width, height, book())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching() {
        let b = book();
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
