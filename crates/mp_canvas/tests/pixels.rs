//! Pixel-level checks of the Canvas 2D primitives against what the HTML
//! canvas specifies and Chrome draws: exact-area rectangles (negative and
//! fractional sizes), source-over and the other composite operations,
//! `clearRect`, clips (nested, fractional, restored), transforms, linear
//! and radial gradients (padding, hard stops, unpremultiplied
//! interpolation, the degenerate cases that paint nothing), the nonzero
//! rule, arcs and ellipses, `putImageData`/`getImageData`/`drawImage` at and
//! past the edges, shadows and the blur filter, and that nothing drawn off
//! the canvas, at absurd coordinates or with non-finite arguments panics.

use mp_canvas::{Canvas, Matrix};
use std::f64::consts::PI;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Unpremultiplied RGBA of one pixel (`getImageData`).
fn px(g: &Canvas, x: u32, y: u32) -> [u8; 4] {
    let d = g.get_image_data(x as i32, y as i32, 1, 1).data;
    [d[0], d[1], d[2], d[3]]
}

fn alpha(g: &Canvas, x: u32, y: u32) -> u8 {
    px(g, x, y)[3]
}

/// Total coverage: the sum of alpha over the canvas, in pixels.
fn area(g: &Canvas) -> f64 {
    g.premultiplied()
        .chunks(4)
        .map(|p| p[3] as f64 / 255.0)
        .sum()
}

fn near(a: u8, b: u8, tol: u8) -> bool {
    a.abs_diff(b) <= tol
}

// ── Rectangles ────────────────────────────────────────────────────────

#[test]
fn a_fractional_rect_covers_each_pixel_by_its_area() {
    let mut g = Canvas::new(3, 3);
    g.set_fill_style("#fff");
    g.fill_rect(0.5, 0.5, 1.0, 1.0);
    for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        assert_eq!(px(&g, x, y), [255, 255, 255, 64], "({x}, {y})");
    }
    for (x, y) in [(2, 0), (2, 2), (0, 2)] {
        assert_eq!(px(&g, x, y), [0, 0, 0, 0]);
    }
    assert!((area(&g) - 1.0).abs() < 0.01);
}

#[test]
fn a_negative_size_rect_is_the_same_rect() {
    let draw = |x: f64, y: f64, w: f64, h: f64| {
        let mut g = Canvas::new(8, 8);
        g.set_fill_style("#0f0");
        g.fill_rect(x, y, w, h);
        g.to_rgba()
    };
    let want = draw(1.25, 2.0, 3.5, 4.0);
    assert_eq!(draw(4.75, 2.0, -3.5, 4.0), want);
    assert_eq!(draw(1.25, 6.0, 3.5, -4.0), want);
    assert_eq!(draw(4.75, 6.0, -3.5, -4.0), want);
}

#[test]
fn empty_and_non_finite_rects_draw_nothing() {
    for (x, y, w, h) in [
        (1.0, 1.0, 0.0, 0.0),
        (f64::NAN, 0.0, 4.0, 4.0),
        (0.0, 0.0, f64::INFINITY, 4.0),
        (0.0, 0.0, 4.0, f64::NEG_INFINITY),
    ] {
        let mut g = Canvas::new(8, 8);
        g.fill_rect(x, y, w, h);
        g.stroke_rect(x, y, w, h);
        assert_eq!(area(&g), 0.0, "{x} {y} {w} {h}");
    }
}

/// HTML: a rectangle with one side zero has no area to fill, but its
/// stroke is a line.
#[test]
fn a_rect_with_one_zero_side_fills_nothing_but_strokes_a_line() {
    for (w, h) in [(0.0, 5.0), (5.0, 0.0)] {
        let mut g = Canvas::new(8, 8);
        g.fill_rect(1.0, 1.0, w, h);
        assert_eq!(area(&g), 0.0, "fill {w}×{h}");
        g.stroke_rect(1.0, 1.0, w, h);
        assert!(area(&g) > 0.0, "stroke {w}×{h}");
        // Only within half the line width (and the joins) of the segment.
        for y in 0..8 {
            for x in 0..8 {
                let near_line = if w == 0.0 {
                    x <= 1 && (1..=5).contains(&y)
                } else {
                    y <= 1 && (1..=5).contains(&x)
                };
                if !near_line {
                    assert_eq!(alpha(&g, x, y), 0, "{w}×{h} at ({x}, {y})");
                }
            }
        }
    }
}

#[test]
fn a_stroked_rect_straddles_its_edges() {
    let mut g = Canvas::new(8, 8);
    g.set_line_width(2.0);
    g.stroke_rect(1.0, 1.0, 4.0, 4.0);
    // Edges at 1 and 5, two wide: columns 0-1 and 4-5 on a middle row.
    let row: Vec<u8> = (0..8).map(|x| alpha(&g, x, 3)).collect();
    assert_eq!(row, [255, 255, 0, 0, 255, 255, 0, 0]);
    let top: Vec<u8> = (0..8).map(|x| alpha(&g, x, 0)).collect();
    assert_eq!(top, [255, 255, 255, 255, 255, 255, 0, 0]);
}

#[test]
fn invalid_line_widths_are_ignored() {
    let draw = |bad: Option<f64>| {
        let mut g = Canvas::new(8, 8);
        g.set_line_width(2.0);
        if let Some(b) = bad {
            g.set_line_width(b);
        }
        g.set_line_cap("nonsense");
        g.set_line_join("nonsense");
        g.stroke_rect(1.0, 1.0, 4.0, 4.0);
        g.to_rgba()
    };
    let want = draw(None);
    for b in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(draw(Some(b)), want, "lineWidth {b}");
    }
}

// ── Colour and compositing ────────────────────────────────────────────

#[test]
fn an_unparsable_fill_style_keeps_the_previous_one() {
    let mut g = Canvas::new(1, 1);
    g.set_fill_style("#00f");
    for bad in ["", "#12", "rgb(1,2)", "blurple", "rgba(0,0,0", "#ggg"] {
        g.set_fill_style(bad);
    }
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(px(&g, 0, 0), [0, 0, 255, 255]);
}

#[test]
fn source_over_blends_premultiplied_without_linearising() {
    // Half-transparent red on opaque blue: (0.5, 0, 0.5) in sRGB bytes.
    for how in ["rgba", "globalAlpha", "#rrggbbaa"] {
        let mut g = Canvas::new(1, 1);
        g.set_fill_style("#0000ff");
        g.fill_rect(0.0, 0.0, 1.0, 1.0);
        match how {
            "rgba" => g.set_fill_style("rgba(255, 0, 0, 0.5)"),
            "globalAlpha" => {
                g.set_fill_style("#f00");
                g.set_global_alpha(0.5);
            }
            _ => g.set_fill_style("#ff000080"),
        }
        g.fill_rect(0.0, 0.0, 1.0, 1.0);
        let p = px(&g, 0, 0);
        assert!(
            near(p[0], 128, 1) && p[1] == 0 && near(p[2], 127, 1) && p[3] == 255,
            "{how}: {p:?}"
        );
    }
}

#[test]
fn translucent_layers_accumulate_alpha() {
    let mut g = Canvas::new(1, 1);
    g.set_fill_style("rgba(255,255,255,0.5)");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    // 1 - 0.5² = 0.75.
    let p = px(&g, 0, 0);
    assert!(near(p[3], 191, 1), "{p:?}");
    assert_eq!(&p[..3], &[255, 255, 255]);
}

#[test]
fn global_alpha_outside_0_to_1_is_ignored() {
    for bad in [-0.1, 1.5, f64::NAN, f64::INFINITY] {
        let mut g = Canvas::new(1, 1);
        g.set_global_alpha(0.5);
        g.set_global_alpha(bad);
        g.fill_rect(0.0, 0.0, 1.0, 1.0);
        assert!(near(alpha(&g, 0, 0), 128, 1), "globalAlpha {bad}");
    }
    let mut g = Canvas::new(1, 1);
    g.set_global_alpha(0.0);
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(alpha(&g, 0, 0), 0);
}

#[test]
fn lighter_adds_and_saturates() {
    let mut g = Canvas::new(2, 1);
    g.set_fill_style("#f00");
    g.fill_rect(0.0, 0.0, 2.0, 1.0);
    g.set_global_composite_operation("lighter");
    g.set_fill_style("#0f0");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    g.set_fill_style("#800000");
    g.fill_rect(1.0, 0.0, 1.0, 1.0);
    assert_eq!(px(&g, 0, 0), [255, 255, 0, 255]);
    assert_eq!(px(&g, 1, 0), [255, 0, 0, 255]);
}

#[test]
fn destination_out_erases_by_the_source_alpha_whatever_its_colour() {
    let mut g = Canvas::new(2, 1);
    g.set_fill_style("#fff");
    g.fill_rect(0.0, 0.0, 2.0, 1.0);
    g.set_global_composite_operation("destination-out");
    g.set_fill_style("rgba(0, 255, 0, 0.5)");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    g.set_fill_style("#123456");
    g.fill_rect(1.0, 0.0, 1.0, 1.0);
    let p = px(&g, 0, 0);
    assert!(near(p[3], 128, 1) && p[..3] == [255, 255, 255], "{p:?}");
    assert_eq!(px(&g, 1, 0), [0, 0, 0, 0]);
}

#[test]
fn lighten_keeps_the_lighter_channel() {
    let mut g = Canvas::new(1, 1);
    g.set_fill_style("rgb(200, 10, 100)");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    g.set_global_composite_operation("lighten");
    g.set_fill_style("rgb(50, 220, 100)");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(px(&g, 0, 0), [200, 220, 100, 255]);
}

#[test]
fn an_unknown_composite_operation_is_ignored() {
    let mut g = Canvas::new(1, 1);
    g.set_fill_style("#f00");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    g.set_global_composite_operation("lighter");
    g.set_global_composite_operation("no-such-op");
    g.set_fill_style("#0f0");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(px(&g, 0, 0), [255, 255, 0, 255], "still lighter");
}

#[test]
#[should_panic(expected = "not implemented")]
fn a_valid_but_unimplemented_composite_operation_panics() {
    let mut g = Canvas::new(1, 1);
    g.set_global_composite_operation("multiply");
}

#[test]
fn clear_rect_ignores_alpha_and_compositing_but_follows_transform_and_clip() {
    let mut g = Canvas::new(8, 1);
    g.set_fill_style("#fff");
    g.fill_rect(0.0, 0.0, 8.0, 1.0);
    g.set_global_alpha(0.25);
    g.set_global_composite_operation("lighter");
    g.translate(2.0, 0.0);
    g.clear_rect(0.0, 0.0, 2.0, 1.0);
    g.begin_path();
    g.rect(4.0, 0.0, 1.0, 1.0);
    g.clip();
    g.clear_rect(-2.0, 0.0, 10.0, 1.0);
    let a: Vec<u8> = (0..8).map(|x| alpha(&g, x, 0)).collect();
    assert_eq!(a, [255, 255, 0, 0, 255, 255, 0, 255]);
}

// ── Clips and state ───────────────────────────────────────────────────

#[test]
fn clips_intersect_and_restore_removes_them() {
    let mut g = Canvas::new(8, 1);
    g.save();
    g.begin_path();
    g.rect(2.0, 0.0, 4.0, 1.0);
    g.clip();
    g.begin_path();
    g.rect(4.0, 0.0, 4.0, 1.0);
    g.clip();
    g.fill_rect(0.0, 0.0, 8.0, 1.0);
    let a: Vec<u8> = (0..8).map(|x| alpha(&g, x, 0)).collect();
    assert_eq!(a, [0, 0, 0, 0, 255, 255, 0, 0]);
    g.restore();
    g.fill_rect(0.0, 0.0, 8.0, 1.0);
    assert!((0..8).all(|x| alpha(&g, x, 0) == 255));
}

#[test]
fn a_fractional_clip_scales_coverage() {
    let mut g = Canvas::new(4, 1);
    g.begin_path();
    g.rect(0.0, 0.0, 2.5, 1.0);
    g.clip();
    g.fill_rect(0.0, 0.0, 4.0, 1.0);
    let a: Vec<u8> = (0..4).map(|x| alpha(&g, x, 0)).collect();
    assert_eq!(a, [255, 255, 128, 0]);
}

#[test]
fn clipping_to_an_empty_path_hides_everything() {
    let mut g = Canvas::new(4, 4);
    g.begin_path();
    g.clip();
    g.fill_rect(0.0, 0.0, 4.0, 4.0);
    assert_eq!(area(&g), 0.0);
}

#[test]
fn restore_brings_back_every_setting_and_an_extra_restore_is_harmless() {
    let mut g = Canvas::new(4, 1);
    g.restore();
    g.restore();
    g.save();
    g.set_fill_style("#f00");
    g.set_global_alpha(0.1);
    g.set_global_composite_operation("destination-out");
    g.translate(2.0, 0.0);
    g.set_filter("blur(3px)");
    g.restore();
    assert_eq!(g.transform_matrix(), Matrix::IDENTITY);
    assert_eq!(g.filter(), "none");
    g.fill_rect(0.0, 0.0, 1.0, 1.0);
    assert_eq!(px(&g, 0, 0), [0, 0, 0, 255]);
    assert_eq!(alpha(&g, 1, 0), 0);
}

// ── Transforms ────────────────────────────────────────────────────────

#[test]
fn a_quarter_turn_maps_a_rect_onto_whole_pixels() {
    let mut g = Canvas::new(5, 3);
    g.translate(4.0, 0.0);
    g.rotate(PI / 2.0);
    // User (0..2, 0..1) → device x = 4 - y, y = x.
    g.fill_rect(0.0, 0.0, 2.0, 1.0);
    let filled: Vec<(u32, u32)> = (0..3)
        .flat_map(|y| (0..5).map(move |x| (x, y)))
        .filter(|&(x, y)| alpha(&g, x, y) > 0)
        .collect();
    assert_eq!(filled, [(3, 0), (3, 1)]);
    assert_eq!(alpha(&g, 3, 1), 255);
}

#[test]
fn a_mirror_transform_flips_drawing() {
    let mut g = Canvas::new(4, 1);
    g.scale(-1.0, 1.0);
    g.fill_rect(-2.0, 0.0, 1.0, 1.0);
    let a: Vec<u8> = (0..4).map(|x| alpha(&g, x, 0)).collect();
    assert_eq!(a, [0, 255, 0, 0]);
}

#[test]
fn non_finite_transforms_are_ignored() {
    let mut g = Canvas::new(4, 4);
    g.translate(1.0, 1.0);
    let m = g.transform_matrix();
    g.translate(f64::NAN, 0.0);
    g.scale(f64::INFINITY, 1.0);
    g.rotate(f64::NAN);
    assert_eq!(g.transform_matrix(), m);
}

#[test]
fn a_singular_transform_draws_nothing_and_does_not_panic() {
    let mut g = Canvas::new(4, 4);
    g.scale(0.0, 1.0);
    g.fill_rect(0.0, 0.0, 4.0, 4.0);
    g.set_line_width(3.0);
    g.stroke_rect(0.0, 0.0, 4.0, 4.0);
    g.begin_path();
    g.move_to(0.0, 0.0);
    g.line_to(4.0, 4.0);
    g.stroke();
    let mut grd = g.create_linear_gradient(0.0, 0.0, 4.0, 0.0);
    grd.add_color_stop(0.0, "#f00");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 4.0, 4.0);
    assert_eq!(area(&g), 0.0);
}

// ── Paths ─────────────────────────────────────────────────────────────

#[test]
fn nonzero_fills_overlaps_once_and_leaves_reversed_holes() {
    // Same direction: the union, the overlap no darker.
    let mut g = Canvas::new(8, 8);
    g.set_fill_style("rgba(0,0,0,0.5)");
    g.begin_path();
    g.rect(0.0, 0.0, 4.0, 4.0);
    g.rect(2.0, 2.0, 4.0, 4.0);
    g.fill();
    assert_eq!(alpha(&g, 3, 3), alpha(&g, 0, 0));
    assert!((area(&g) - 28.0 * 128.0 / 255.0).abs() < 0.01);
    // Opposite direction: a hole.
    let mut g = Canvas::new(8, 8);
    g.begin_path();
    g.rect(0.0, 0.0, 8.0, 8.0);
    g.move_to(2.0, 2.0);
    g.line_to(2.0, 6.0);
    g.line_to(6.0, 6.0);
    g.line_to(6.0, 2.0);
    g.close_path();
    g.fill();
    assert_eq!(alpha(&g, 4, 4), 0);
    assert_eq!(alpha(&g, 0, 4), 255);
    assert!((area(&g) - 48.0).abs() < 0.01);
}

#[test]
fn an_open_subpath_is_closed_for_filling() {
    let mut g = Canvas::new(8, 8);
    g.begin_path();
    g.move_to(0.0, 0.0);
    g.line_to(8.0, 0.0);
    g.line_to(0.0, 8.0);
    g.fill();
    // 32 px², the diagonal's half pixels stored as 128/255.
    assert!((area(&g) - 32.0).abs() < 0.05, "{}", area(&g));
}

#[test]
fn non_finite_path_points_are_skipped() {
    let draw = |junk: bool| {
        let mut g = Canvas::new(8, 8);
        g.begin_path();
        g.move_to(0.0, 0.0);
        if junk {
            g.line_to(f64::NAN, 3.0);
            g.quadratic_curve_to(1.0, f64::INFINITY, 2.0, 2.0);
            g.bezier_curve_to(0.0, 0.0, 1.0, 1.0, f64::NAN, 0.0);
            g.rect(f64::NAN, 0.0, 1.0, 1.0);
            g.arc(4.0, 4.0, f64::INFINITY, 0.0, 1.0, false);
        }
        g.line_to(8.0, 0.0);
        g.line_to(0.0, 8.0);
        g.fill();
        g.to_rgba()
    };
    assert_eq!(draw(true), draw(false));
}

#[test]
fn a_circle_and_an_ellipse_cover_their_area() {
    let mut g = Canvas::new(64, 64);
    g.begin_path();
    g.arc(32.0, 32.0, 20.0, 0.0, 2.0 * PI, false);
    g.fill();
    let want = PI * 400.0;
    assert!((area(&g) - want).abs() / want < 0.005, "{}", area(&g));

    let mut g = Canvas::new(64, 64);
    g.begin_path();
    g.ellipse(32.0, 32.0, 20.0, 10.0, 0.3, 0.0, 2.0 * PI, false);
    g.fill();
    let want = PI * 200.0;
    assert!((area(&g) - want).abs() / want < 0.005, "{}", area(&g));
}

/// Angles run clockwise on screen (y down): anticlockwise from 0 to π goes
/// through -π/2, the top.
#[test]
fn an_anticlockwise_half_arc_is_the_upper_half() {
    let mut g = Canvas::new(64, 64);
    g.begin_path();
    g.arc(32.0, 32.0, 20.0, 0.0, PI, true);
    g.fill();
    assert_eq!(alpha(&g, 32, 20), 255);
    assert_eq!(alpha(&g, 32, 44), 0);
    let want = PI * 200.0;
    assert!((area(&g) - want).abs() / want < 0.01, "{}", area(&g));
    // And clockwise, the lower half.
    let mut g = Canvas::new(64, 64);
    g.begin_path();
    g.arc(32.0, 32.0, 20.0, 0.0, PI, false);
    g.fill();
    assert_eq!(alpha(&g, 32, 20), 0);
    assert_eq!(alpha(&g, 32, 44), 255);
}

#[test]
fn a_new_path_forgets_the_old_one() {
    let mut g = Canvas::new(8, 8);
    g.rect(0.0, 0.0, 4.0, 4.0);
    g.begin_path();
    g.rect(4.0, 4.0, 4.0, 4.0);
    g.fill();
    assert_eq!(alpha(&g, 1, 1), 0);
    assert_eq!(alpha(&g, 5, 5), 255);
    // Filling an empty path draws nothing.
    let mut g = Canvas::new(8, 8);
    g.begin_path();
    g.fill();
    g.stroke();
    assert_eq!(area(&g), 0.0);
}

// ── Gradients ─────────────────────────────────────────────────────────

#[test]
fn a_linear_gradient_ramps_within_dither_and_pads_beyond_its_ends() {
    let mut g = Canvas::new(256, 1);
    let mut grd = g.create_linear_gradient(64.0, 0.0, 192.0, 0.0);
    grd.add_color_stop(0.0, "#000");
    grd.add_color_stop(1.0, "#fff");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 256.0, 1.0);
    for x in 0..256u32 {
        let p = px(&g, x, 0);
        assert_eq!(p[3], 255);
        assert!(p[0] == p[1] && p[1] == p[2], "grey at {x}: {p:?}");
        let t = ((x as f64 + 0.5 - 64.0) / 128.0).clamp(0.0, 1.0);
        let want = t * 255.0;
        assert!(
            (p[0] as f64 - want).abs() <= 1.0,
            "at {x}: {} vs {want}",
            p[0]
        );
        if x < 64 {
            assert_eq!(p[0], 0, "padded at {x}");
        }
        if x >= 192 {
            assert_eq!(p[0], 255, "padded at {x}");
        }
    }
}

#[test]
fn gradients_live_in_user_space() {
    let mut g = Canvas::new(8, 1);
    let mut grd = g.create_linear_gradient(0.0, 0.0, 4.0, 0.0);
    grd.add_color_stop(0.0, "#f00");
    grd.add_color_stop(1.0, "#00f");
    g.translate(4.0, 0.0);
    g.set_fill_style(&grd);
    g.fill_rect(-4.0, 0.0, 8.0, 1.0);
    // Device 0..4 is user -4..0: padded red.
    for x in 0..4 {
        assert_eq!(px(&g, x, 0), [255, 0, 0, 255], "at {x}");
    }
    assert!(px(&g, 7, 0)[2] > 200);
}

#[test]
fn equal_offsets_make_a_hard_edge_and_stops_sort_by_offset() {
    let mut g = Canvas::new(100, 1);
    let mut grd = g.create_linear_gradient(0.0, 0.0, 100.0, 0.0);
    // Added out of order; the two at 0.5 keep their order.
    grd.add_color_stop(1.0, "#00f");
    grd.add_color_stop(0.5, "#f00");
    grd.add_color_stop(0.0, "#f00");
    grd.add_color_stop(0.5, "#00f");
    let offsets: Vec<f64> = grd.stops.iter().map(|s| s.0).collect();
    assert_eq!(offsets, [0.0, 0.5, 0.5, 1.0]);
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 100.0, 1.0);
    for x in 0..49 {
        assert_eq!(px(&g, x, 0), [255, 0, 0, 255], "at {x}");
    }
    for x in 51..100 {
        assert_eq!(px(&g, x, 0), [0, 0, 255, 255], "at {x}");
    }
}

#[test]
fn stops_interpolate_unpremultiplied() {
    // Transparent red to red: the colour stays red all the way; only the
    // alpha ramps (no darkening towards transparent black).
    let mut g = Canvas::new(64, 1);
    let mut grd = g.create_linear_gradient(0.0, 0.0, 64.0, 0.0);
    grd.add_color_stop(0.0, "rgba(255,0,0,0)");
    grd.add_color_stop(1.0, "rgba(255,0,0,1)");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 64.0, 1.0);
    let mid = px(&g, 32, 0);
    assert!(near(mid[3], 130, 2), "{mid:?}");
    assert!(mid[0] >= 250 && mid[1] == 0 && mid[2] == 0, "{mid:?}");
}

#[test]
fn degenerate_gradients_paint_nothing() {
    let base = |g: &mut Canvas| {
        g.set_fill_style("#0f0");
        g.fill_rect(0.0, 0.0, 4.0, 4.0);
    };
    // A zero-length linear gradient (HTML: "must paint nothing").
    let mut g = Canvas::new(4, 4);
    base(&mut g);
    let mut grd = g.create_linear_gradient(2.0, 2.0, 2.0, 2.0);
    grd.add_color_stop(0.0, "#f00");
    grd.add_color_stop(1.0, "#f00");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 4.0, 4.0);
    assert_eq!(px(&g, 1, 1), [0, 255, 0, 255]);
    // No stops: transparent black.
    let mut g = Canvas::new(4, 4);
    base(&mut g);
    let grd = g.create_linear_gradient(0.0, 0.0, 4.0, 0.0);
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 4.0, 4.0);
    assert_eq!(px(&g, 1, 1), [0, 255, 0, 255]);
}

#[test]
fn a_radial_gradient_is_the_first_stop_at_the_centre_and_pads_past_the_rim() {
    let mut g = Canvas::new(100, 100);
    let mut grd = g.create_radial_gradient(50.0, 50.0, 0.0, 50.0, 50.0, 40.0);
    grd.add_color_stop(0.0, "#f00");
    grd.add_color_stop(1.0, "#00f");
    g.set_fill_style(&grd);
    g.fill_rect(0.0, 0.0, 100.0, 100.0);
    let c = px(&g, 50, 50);
    assert!(c[0] >= 250 && c[2] <= 5, "centre {c:?}");
    for (x, y) in [(95, 50), (0, 0), (50, 99), (99, 99)] {
        assert_eq!(px(&g, x, y), [0, 0, 255, 255], "({x}, {y})");
    }
    // Symmetric about the centre (within the ordered dither).
    let (l, r) = (px(&g, 30, 50), px(&g, 69, 50));
    assert!((0..4).all(|k| near(l[k], r[k], 1)), "{l:?} vs {r:?}");
    // Monotone outwards in blue.
    let blues: Vec<u8> = (50..95).map(|x| px(&g, x, 50)[2]).collect();
    assert!(
        blues
            .windows(2)
            .all(|w| u16::from(w[1]) + 1 >= u16::from(w[0])),
        "{blues:?}"
    );
}

#[test]
#[should_panic(expected = "outside 0..1")]
fn a_stop_offset_outside_0_to_1_panics_as_the_browser_throws() {
    let g = Canvas::new(1, 1);
    let mut grd = g.create_linear_gradient(0.0, 0.0, 1.0, 0.0);
    grd.add_color_stop(1.5, "#fff");
}

// ── Pixels in and out ─────────────────────────────────────────────────

#[test]
fn put_image_data_ignores_state_and_clips_to_the_canvas() {
    let mut g = Canvas::new(4, 4);
    g.begin_path();
    g.rect(0.0, 0.0, 1.0, 1.0);
    g.clip();
    g.set_global_alpha(0.1);
    g.set_global_composite_operation("destination-out");
    g.translate(1.0, 1.0);
    let mut img = g.create_image_data(4, 4);
    for (i, v) in img.data.chunks_mut(4).enumerate() {
        v.copy_from_slice(&[i as u8 * 10, 0, 0, 255]);
    }
    g.put_image_data(&img, -2, -2);
    // Image pixel (2, 2) = index 10 lands at (0, 0); (3, 3) = 15 at (1, 1).
    assert_eq!(px(&g, 0, 0), [100, 0, 0, 255]);
    assert_eq!(px(&g, 1, 1), [150, 0, 0, 255]);
    assert_eq!(px(&g, 2, 2), [0, 0, 0, 0]);
    // Wholly outside, both ways: nothing, no panic.
    g.put_image_data(&img, 100, 0);
    g.put_image_data(&img, i32::MIN / 2, i32::MAX / 2);
    g.put_image_data(&g.create_image_data(0, 0), 0, 0);
    assert_eq!(px(&g, 3, 3), [0, 0, 0, 0]);
}

#[test]
fn get_image_data_past_the_edges_reads_transparent_black() {
    let mut g = Canvas::new(2, 2);
    g.set_fill_style("#fff");
    g.fill_rect(0.0, 0.0, 2.0, 2.0);
    let d = g.get_image_data(-1, -1, 3, 3);
    assert_eq!((d.width, d.height, d.data.len()), (3, 3, 36));
    let alphas: Vec<u8> = d.data.chunks(4).map(|p| p[3]).collect();
    assert_eq!(alphas, [0, 0, 0, 0, 255, 255, 0, 255, 255]);
    let far = g.get_image_data(i32::MAX - 1, i32::MIN, 2, 2);
    assert!(far.data.iter().all(|&v| v == 0));
    assert!(g.get_image_data(0, 0, 0, 5).data.is_empty());
}

#[test]
fn translucent_pixels_lose_only_premultiplied_precision() {
    let mut g = Canvas::new(256, 1);
    let mut img = g.create_image_data(256, 1);
    for a in 0..256usize {
        img.data[a * 4..a * 4 + 4].copy_from_slice(&[200, 100, 7, a as u8]);
    }
    g.put_image_data(&img, 0, 0);
    let back = g.get_image_data(0, 0, 256, 1);
    for a in 1..256usize {
        let p = &back.data[a * 4..a * 4 + 4];
        assert_eq!(p[3] as usize, a);
        // One premultiplied step is 255/a unpremultiplied levels.
        let tol = (255.0 / a as f64).ceil() as u8;
        assert!(
            near(p[0], 200, tol) && near(p[1], 100, tol) && near(p[2], 7, tol),
            "a {a}: {p:?}"
        );
    }
    assert_eq!(&back.data[..4], &[0, 0, 0, 0]);
}

fn checker(w: u32, h: u32) -> Canvas {
    let mut src = Canvas::new(w, h);
    let mut img = src.create_image_data(w, h);
    for (i, p) in img.data.chunks_mut(4).enumerate() {
        let v = (i * 37 % 256) as u8;
        p.copy_from_slice(&[v, 255 - v, (i * 11 % 256) as u8, 255]);
    }
    src.put_image_data(&img, 0, 0);
    src
}

#[test]
fn draw_image_at_one_to_one_copies_pixels_exactly() {
    let src = checker(6, 5);
    let mut g = Canvas::new(6, 5);
    g.draw_image(&src, 0.0, 0.0, 6.0, 5.0);
    assert_eq!(g.to_rgba(), src.to_rgba());
    // Shifted a whole pixel: shifted exactly.
    let mut g = Canvas::new(7, 6);
    g.translate(1.0, 1.0);
    g.draw_image(&src, 0.0, 0.0, 6.0, 5.0);
    assert_eq!(px(&g, 1, 1), px(&src, 0, 0));
    assert_eq!(px(&g, 6, 5), px(&src, 5, 4));
    assert_eq!(alpha(&g, 0, 0), 0);
}

#[test]
fn draw_image_takes_global_alpha_and_a_source_rectangle() {
    let src = checker(6, 5);
    let mut g = Canvas::new(2, 2);
    g.set_global_alpha(0.5);
    g.draw_image_sub(&src, 3.0, 2.0, 2.0, 2.0, 0.0, 0.0, 2.0, 2.0);
    let p = px(&g, 1, 1);
    assert!(near(p[3], 128, 1), "{p:?}");
    let s = px(&src, 4, 3);
    for k in 0..3 {
        assert!(near(p[k], s[k], 2), "{p:?} vs {s:?}");
    }
}

#[test]
fn draw_image_of_nothing_draws_nothing() {
    let src = checker(4, 4);
    let mut g = Canvas::new(4, 4);
    g.draw_image(&src, 0.0, 0.0, 0.0, 4.0);
    g.draw_image_sub(&src, 0.0, 0.0, 0.0, 4.0, 0.0, 0.0, 4.0, 4.0);
    g.draw_image(&src, f64::NAN, 0.0, 4.0, 4.0);
    g.draw_image(&src, 10.0, 10.0, 4.0, 4.0);
    assert_eq!(area(&g), 0.0);
}

/// HTML: a source rectangle is clipped to the source; one wholly outside
/// it draws nothing.
#[test]
fn a_source_rectangle_outside_the_source_draws_nothing() {
    let src = checker(4, 4);
    let empty = Canvas::new(0, 0);
    let mut panicked = Vec::new();
    for (what, sx, s) in [
        ("rect past the source", 10.0, &src),
        ("0×0 source", 0.0, &empty),
    ] {
        let mut g = Canvas::new(4, 4);
        let r = catch_unwind(AssertUnwindSafe(|| {
            g.draw_image_sub(s, sx, 0.0, 2.0, 2.0, 0.0, 0.0, 4.0, 4.0)
        }));
        match r {
            Ok(()) => assert_eq!(area(&g), 0.0, "{what}"),
            Err(_) => panicked.push(what),
        }
    }
    assert!(panicked.is_empty(), "panicked: {panicked:?}");
}

// ── Shadows and filters ───────────────────────────────────────────────

#[test]
fn a_shadow_spills_onto_the_canvas_from_a_shape_just_off_it() {
    let draw = |color: &str, blur: f64| {
        let mut g = Canvas::new(16, 8);
        g.set_shadow_color(color);
        g.set_shadow_blur(blur);
        g.fill_rect(-6.0, 0.0, 5.0, 8.0);
        g
    };
    let g = draw("#000", 8.0);
    assert!(alpha(&g, 0, 4) > 0, "the shadow reaches in");
    assert!(alpha(&g, 0, 4) > alpha(&g, 3, 4));
    assert_eq!(alpha(&g, 15, 4), 0);
    // No shadow without blur or with a transparent colour.
    assert_eq!(area(&draw("#000", 0.0)), 0.0);
    assert_eq!(area(&draw("rgba(0,0,0,0)", 8.0)), 0.0);
}

#[test]
fn a_shadow_is_drawn_under_its_shape() {
    let mut g = Canvas::new(16, 16);
    g.set_shadow_color("#f00");
    g.set_shadow_blur(4.0);
    g.set_fill_style("#00f");
    g.fill_rect(4.0, 4.0, 8.0, 8.0);
    assert_eq!(px(&g, 8, 8), [0, 0, 255, 255], "the shape covers it");
    let edge = px(&g, 2, 8);
    assert!(edge[0] == 255 && edge[2] == 0 && edge[3] > 0, "{edge:?}");
}

#[test]
fn the_blur_filter_spreads_a_shape_and_keeps_its_mass() {
    let mut g = Canvas::new(32, 32);
    g.set_filter("blur(2px)");
    g.fill_rect(12.0, 12.0, 8.0, 8.0);
    assert!(
        (area(&g) - 64.0).abs() < 1.0,
        "mass {} (8-bit rounding aside)",
        area(&g)
    );
    assert!(alpha(&g, 16, 16) > 200, "the middle stays nearly solid");
    assert!(alpha(&g, 11, 16) > 0, "spread");
    assert_eq!(alpha(&g, 0, 16), 0, "past 3σ");
    // "none" turns it off; a bad blur keeps the last good value.
    g.set_filter("blur(-1px)");
    assert_eq!(g.filter(), "blur(2px)");
    g.set_filter("none");
    assert_eq!(g.filter(), "none");
}

#[test]
#[should_panic(expected = "not implemented")]
fn an_unimplemented_filter_panics() {
    let mut g = Canvas::new(1, 1);
    g.set_filter("grayscale(1)");
}

// ── Robustness ────────────────────────────────────────────────────────

#[test]
fn an_empty_canvas_takes_every_call() {
    let mut g = Canvas::new(0, 0);
    assert!(g.to_rgba().is_empty());
    g.fill_rect(0.0, 0.0, 10.0, 10.0);
    g.set_shadow_color("#000");
    g.set_shadow_blur(4.0);
    g.fill_rect(0.0, 0.0, 10.0, 10.0);
    g.begin_path();
    g.arc(0.0, 0.0, 5.0, 0.0, 6.0, false);
    g.stroke();
    g.clear_rect(0.0, 0.0, 1.0, 1.0);
    g.fill_text("x", 0.0, 0.0);
    assert_eq!(g.get_image_data(0, 0, 1, 1).data, [0, 0, 0, 0]);
}

#[test]
fn drawing_far_off_the_canvas_or_at_huge_sizes_never_panics() {
    type Draw = Box<dyn Fn(&mut Canvas)>;
    let shapes: Vec<(&str, Draw)> = vec![
        ("huge rect", Box::new(|g| g.fill_rect(-1e9, -1e9, 2e9, 2e9))),
        (
            "f32 overflow",
            Box::new(|g| g.fill_rect(-1e300, -1e300, 2e300, 2e300)),
        ),
        ("far rect", Box::new(|g| g.fill_rect(1e7, 1e7, 5.0, 5.0))),
        (
            "negative far",
            Box::new(|g| g.fill_rect(-1e7, 3.0, 2.0, 2.0)),
        ),
        (
            "huge arc",
            Box::new(|g| {
                g.begin_path();
                g.arc(8.0, 8.0, 1e7, 0.0, 2.0 * PI, false);
                g.fill();
            }),
        ),
        (
            "tiny arc",
            Box::new(|g| {
                g.begin_path();
                g.arc(8.0, 8.0, 1e-12, 0.0, 2.0 * PI, false);
                g.fill();
                g.stroke();
            }),
        ),
        (
            "long line",
            Box::new(|g| {
                g.set_line_width(3.0);
                g.begin_path();
                g.move_to(-1e8, -1e8);
                g.line_to(1e8, 1e8);
                g.stroke();
            }),
        ),
        (
            "long curve",
            Box::new(|g| {
                g.begin_path();
                g.move_to(-1e6, 0.0);
                g.quadratic_curve_to(8.0, 1e6, 1e6, 0.0);
                g.stroke();
                g.fill();
            }),
        ),
        (
            "degenerate stroke",
            Box::new(|g| {
                g.set_line_cap("round");
                g.begin_path();
                g.move_to(4.0, 4.0);
                g.line_to(4.0, 4.0);
                g.stroke();
                g.close_path();
                g.stroke();
            }),
        ),
        (
            "tiny scale",
            Box::new(|g| {
                g.scale(1e-30, 1e-30);
                g.fill_rect(0.0, 0.0, 1e30, 1e30);
                g.stroke_rect(0.0, 0.0, 1e30, 1e30);
            }),
        ),
        (
            "huge scale",
            Box::new(|g| {
                g.scale(1e20, 1e20);
                g.fill_rect(0.0, 0.0, 1.0, 1.0);
            }),
        ),
        (
            "shadow off canvas",
            Box::new(|g| {
                g.set_shadow_color("#000");
                g.set_shadow_blur(10.0);
                g.fill_rect(-1e6, -1e6, 10.0, 10.0);
                g.fill_rect(30.0, 30.0, 4.0, 4.0);
            }),
        ),
        (
            "clip off canvas",
            Box::new(|g| {
                g.begin_path();
                g.rect(-100.0, -100.0, 50.0, 50.0);
                g.clip();
                g.fill_rect(0.0, 0.0, 16.0, 16.0);
            }),
        ),
        (
            "gradient far away",
            Box::new(|g| {
                let mut grd = g.create_radial_gradient(1e9, 1e9, 0.0, 1e9, 1e9, 1.0);
                grd.add_color_stop(0.0, "#fff");
                grd.add_color_stop(1.0, "#000");
                g.set_fill_style(&grd);
                g.fill_rect(0.0, 0.0, 16.0, 16.0);
            }),
        ),
        (
            "text far away",
            Box::new(|g| {
                g.set_font("bold 40px sans-serif");
                g.fill_text("Midnight", -1e6, 1e6);
                g.stroke_text_max("Racer", 0.0, 10.0, 1e-9);
                g.fill_text_max("Racer", 0.0, 10.0, f64::NAN);
            }),
        ),
    ];
    for (what, f) in &shapes {
        let mut g = Canvas::new(16, 16);
        let r = catch_unwind(AssertUnwindSafe(|| f(&mut g)));
        assert!(r.is_ok(), "{what} panicked");
        if *what == "huge rect" {
            assert!((area(&g) - 256.0).abs() < 1e-9, "{what}: the whole canvas");
        }
        if matches!(*what, "far rect" | "negative far" | "clip off canvas") {
            assert_eq!(area(&g), 0.0, "{what}");
        }
    }
}

#[test]
fn the_same_drawing_gives_the_same_bytes() {
    let draw = || {
        let mut g = Canvas::new(48, 48);
        g.set_line_width(2.5);
        g.set_stroke_style("rgba(255, 128, 0, 0.7)");
        for i in 0..8 {
            g.begin_path();
            g.move_to(2.0, 4.0 + i as f64 * 5.0);
            g.quadratic_curve_to(24.0, 40.0 - i as f64 * 3.0, 46.0, 10.0 + i as f64);
            g.stroke();
        }
        g.set_shadow_color("#000");
        g.set_shadow_blur(3.0);
        g.set_fill_style("hsl(200, 50%, 50%)");
        g.begin_path();
        g.ellipse(24.0, 24.0, 10.0, 6.0, 0.4, 0.0, 2.0 * PI, false);
        g.fill();
        g.to_rgba()
    };
    assert_eq!(draw(), draw());
}

#[test]
fn measure_text_grows_with_the_text_and_the_font_size() {
    let mut g = Canvas::new(1, 1);
    g.set_font("20px sans-serif");
    assert_eq!(g.measure_text("").width, 0.0);
    let a = g.measure_text("M").width;
    let ab = g.measure_text("MM").width;
    assert!(a > 0.0 && (ab - 2.0 * a).abs() < 1e-9, "{a} {ab}");
    g.set_font("40px sans-serif");
    let big = g.measure_text("M").width;
    assert!((big / a - 2.0).abs() < 0.02, "{big} vs {a}");
    // A font string that does not parse leaves the font as it was.
    g.set_font("not a font");
    assert_eq!(g.measure_text("M").width, big);
}
