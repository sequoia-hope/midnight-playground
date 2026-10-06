//! Text measurement against numbers read from Chrome 151 with the bundled
//! faces registered (tools/parity/textures.html): `measureText().width`,
//! and `fontBoundingBoxAscent` under each `textBaseline`, which gives the
//! baseline offset Blink applies.

use mp_canvas::Canvas;
use mp_canvas::fonts::{FontBook, parse_font};
use mp_canvas::text::{Baseline, layout};

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn widths_match_chrome() {
    let mut g = Canvas::new(8, 8);
    let cases: &[(&str, &[(&str, f64)])] = &[
        (
            "bold 64px \"Arial Narrow\", Arial, sans-serif",
            &[
                ("i", 16.125),
                ("W", 47.96875),
                ("AV", 72.772064209),
                ("PORT MERIDIAN", 422.316162109),
            ],
        ),
        // Roboto's kerning varies with wght by GPOS variation deltas (A-V
        // is -87 units at 400, -80 at 700): a shaper that ignores them gets
        // AV wrong in bold.
        (
            "bold 64px Arial",
            &[
                ("i", 16.9375),
                ("W", 56.0),
                ("AV", 82.305511475),
                ("RT", 78.791519165),
                ("PORT MERIDIAN", 480.854003906),
            ],
        ),
        (
            "bold 64px Georgia",
            &[
                ("PORT", 192.15625),
                ("MERIDIAN", 375.375),
                ("ME", 111.65625),
            ],
        ),
        (
            "bold 80px \"Brush Script MT\"",
            &[
                ("PORT", 212.7734375),
                ("MERIDIAN", 374.9609375),
                ("IA", 69.4140625),
            ],
        ),
        (
            "bold 40px \"Courier New\"",
            &[("PORT", 95.9375), (" ", 23.984375)],
        ),
        (
            "bold 96px \"Arial Black\", Arial",
            &[
                ("i", 26.390625),
                ("AV", 125.25),
                ("PORT MERIDIAN", 727.828125),
            ],
        ),
        (
            "italic 900 84px \"Arial Narrow\", Arial",
            &[
                ("i", 21.287109375),
                ("AV", 93.84375),
                ("PORT MERIDIAN", 543.45703125),
            ],
        ),
    ];
    for (font, words) in cases {
        g.set_font(font);
        for (t, want) in words.iter() {
            let w = g.measure_text(t).width;
            // Both shape at the font size in 16.16 fixed point.
            assert!(close(w, *want, 0.002), "{font} {t:?}: {w} vs Chrome {want}");
        }
    }
}

#[test]
fn baselines_match_chrome() {
    let book = FontBook::bundled();
    // (font, alphabetic ascent, [middle, top, bottom, hanging, ideographic]
    // fontBoundingBoxAscent), from Chrome.
    let cases: &[(&str, f64, [f64; 5])] = &[
        (
            "bold 64px \"Arial Narrow\", Arial, sans-serif",
            59.0,
            [43.0, 11.0, 75.0, 11.799999237, 75.0],
        ),
        (
            "bold 64px Arial",
            59.0,
            [43.0, 11.0, 75.0, 11.799999237, 75.0],
        ),
        (
            "bold 96px \"Arial Black\", Arial",
            89.0,
            [65.0, 17.0, 113.0, 17.800003052, 112.0],
        ),
    ];
    for (font, ascent, want) in cases {
        let spec = parse_font(font).unwrap();
        let lay = layout(&book, &spec, "H");
        assert!(
            close(lay.metrics.ascent as f64, *ascent, 1e-6),
            "{font}: ascent {}",
            lay.metrics.ascent
        );
        let bs = [
            Baseline::Middle,
            Baseline::Top,
            Baseline::Bottom,
            Baseline::Hanging,
            Baseline::Ideographic,
        ];
        for (b, w) in bs.iter().zip(want) {
            // fontBoundingBoxAscent = ascent - offset.
            let got = ascent - lay.baseline(*b);
            assert!(close(got, *w, 1e-4), "{font} {b:?}: {got} vs Chrome {w}");
        }
    }
}
