//! Text measurement against numbers read from Chrome 151 with the bundled
//! faces registered (tools/parity/textures.html): `measureText().width`,
//! and `fontBoundingBoxAscent` under each `textBaseline`, which gives the
//! baseline offset Blink applies.

use mr_canvas::Canvas;
use mr_canvas::fonts::{FontBook, parse_font};
use mr_canvas::text::{Baseline, layout};

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
                ("i", 14.591979980),
                ("W", 49.535919189),
                ("AV", 69.055877686),
                ("PORT MERIDIAN", 419.839294434),
            ],
        ),
        // Arimo's kerning of RT and T-space is zero at wght 700 by a
        // variation delta: a shaper that ignores GPOS deltas gets 509.69.
        (
            "bold 64px Arial",
            &[
                ("i", 17.78125),
                ("W", 60.40625),
                ("AV", 84.15625),
                ("RT", 85.3125),
                ("PORT MERIDIAN", 512.0),
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
                ("i", 31.967987061),
                ("AV", 144.095947266),
                ("PORT MERIDIAN", 863.039611816),
            ],
        ),
        (
            "italic 900 84px \"Arial Narrow\", Arial",
            &[
                ("i", 19.152023315),
                ("AV", 89.460128784),
                ("PORT MERIDIAN", 551.040771484),
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
            66.0,
            [48.828125, 16.828125, 80.828125, 13.200000763, 86.0],
        ),
        (
            "bold 64px Arial",
            58.0,
            [38.140625, 6.140625, 70.140625, 11.599998474, 72.0],
        ),
        (
            "bold 96px \"Arial Black\", Arial",
            84.0,
            [54.53125, 6.53125, 102.53125, 16.800003052, 104.0],
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
