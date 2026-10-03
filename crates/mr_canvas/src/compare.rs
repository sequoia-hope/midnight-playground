//! Comparing a drawn image with the JS reference (SPEC 5.7): mean absolute
//! difference per channel, the 8×8 block means the committed summaries
//! hold, and a side-by-side sheet for review.

/// Mean absolute difference per channel (R, G, B, A), in 0..255 levels.
pub fn mean_abs_diff(a: &[u8], b: &[u8]) -> [f64; 4] {
    assert_eq!(a.len(), b.len(), "images of different sizes");
    let mut s = [0u64; 4];
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        s[i % 4] += (*x as i32 - *y as i32).unsigned_abs() as u64;
    }
    let n = (a.len() / 4).max(1) as f64;
    s.map(|v| v as f64 / n)
}

/// Per-channel means over an 8×8 grid of blocks, as
/// `tools/parity/textures.mjs` writes them (two decimals).
pub fn block_means(w: usize, h: usize, rgba: &[u8]) -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(64);
    for by in 0..8 {
        for bx in 0..8 {
            let (x0, x1) = (bx * w / 8, (bx + 1) * w / 8);
            let (y0, y1) = (by * h / 8, (by + 1) * h / 8);
            let mut s = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    for (k, v) in s.iter_mut().enumerate() {
                        *v += rgba[(y * w + x) * 4 + k] as u64;
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)).max(1) as f64;
            out.push(s.map(|v| (v as f64 / n * 100.0).round() / 100.0));
        }
    }
    out
}

/// Mean over the blocks of the absolute difference of block means, per
/// channel: a lower bound of [`mean_abs_diff`] that needs only the summary.
pub fn block_diff(a: &[[f64; 4]], b: &[[f64; 4]]) -> [f64; 4] {
    let mut s = [0.0; 4];
    for (x, y) in a.iter().zip(b) {
        for k in 0..4 {
            s[k] += (x[k] - y[k]).abs();
        }
    }
    s.map(|v| v / a.len().max(1) as f64)
}

/// JS, Rust and their difference (×8) side by side on a checkerboard, so
/// transparency shows. Returns `(width, height, rgba)`.
pub fn sheet(w: usize, h: usize, js: &[u8], rust: &[u8]) -> (usize, usize, Vec<u8>) {
    let gap = 8;
    let sw = w * 3 + gap * 2;
    let mut out = vec![0u8; sw * h * 4];
    for y in 0..h {
        for x in 0..sw {
            let o = (y * sw + x) * 4;
            let check = if ((x / 8) + (y / 8)) % 2 == 0 {
                200.0
            } else {
                150.0
            };
            let (panel, px) = (x / (w + gap), x % (w + gap));
            if px >= w {
                out[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
                continue;
            }
            let i = (y * w + px) * 4;
            let rgb = match panel {
                0 | 1 => {
                    let p = if panel == 0 {
                        &js[i..i + 4]
                    } else {
                        &rust[i..i + 4]
                    };
                    let a = p[3] as f64 / 255.0;
                    [0, 1, 2].map(|k| (p[k] as f64 * a + check * (1.0 - a)).round() as u8)
                }
                _ => [0, 1, 2].map(|k| {
                    let d = (js[i + k] as i32 - rust[i + k] as i32)
                        .unsigned_abs()
                        .max((js[i + 3] as i32 - rust[i + 3] as i32).unsigned_abs());
                    (d * 8).min(255) as u8
                }),
            };
            out[o..o + 3].copy_from_slice(&rgb);
            out[o + 3] = 255;
        }
    }
    (sw, h, out)
}
