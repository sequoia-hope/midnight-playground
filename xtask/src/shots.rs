//! `cargo xtask parity shots --a <dir> --b <dir> [--label name]`: compare two
//! sets of screenshots station by station (SPEC 12, roadmap WP 0.6).
//!
//! The metric: both images box-filtered to quarter resolution (half width,
//! half height, averaged in linear light), converted to CIELAB (D65), and
//! compared pixel by pixel with CIEDE2000. A station passes when the mean
//! difference is under 3 and 95 % of its 16-pixel blocks (mean difference per
//! 16×16 block of the quarter-resolution image) are under 6. The JS game
//! compared with itself gives the noise floor; a threshold below twice the
//! floor is raised to twice the floor.
//!
//! `<dir>` holds one directory per level of PNGs, as `tools/parity/shots.mjs`
//! writes them. The report (`parity/report/shots-<label>/`, git-ignored) shows
//! each pair with its difference map, worst first, and is viewed through the
//! registered server at `/parity/report/`.

use crate::{Result, root};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub const MEAN_LIMIT: f64 = 3.0;
pub const BLOCK_LIMIT: f64 = 6.0;
const BLOCK: usize = 16;

pub fn run(args: &[String]) -> Result {
    let opt = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let a = PathBuf::from(opt("--a").ok_or("--a <dir> is required")?);
    let b = PathBuf::from(opt("--b").ok_or("--b <dir> is required")?);
    let label = opt("--label").unwrap_or_else(|| "compare".into());
    let root = root();
    let a = if a.is_absolute() { a } else { root.join(a) };
    let b = if b.is_absolute() { b } else { root.join(b) };
    let out = root.join("parity/report").join(format!("shots-{label}"));
    std::fs::create_dir_all(out.join("diff")).map_err(|e| e.to_string())?;

    let mut rows = Vec::new();
    for level in subdirs(&a)? {
        for name in pngs(&a.join(&level))? {
            let pb = b.join(&level).join(&name);
            if !pb.exists() {
                return Err(format!("{level}/{name} is in --a but not in --b"));
            }
            let ia = Image::load(&a.join(&level).join(&name))?;
            let ib = Image::load(&pb)?;
            let (score, diff) = compare(&ia, &ib)?;
            let diff_name = format!("{level}-{}", name);
            diff.save(&out.join("diff").join(&diff_name))?;
            rows.push(Row {
                level: level.clone(),
                name,
                score,
                diff: diff_name,
            });
        }
    }
    if rows.is_empty() {
        return Err(format!("no screenshots under {}", a.display()));
    }
    rows.sort_by(|x, y| y.score.mean.total_cmp(&x.score.mean));

    let worst_mean = rows.iter().map(|r| r.score.mean).fold(0.0, f64::max);
    let worst_block = rows.iter().map(|r| r.score.block95).fold(0.0, f64::max);
    let failing = rows.iter().filter(|r| !r.score.passes()).count();
    let rel = |p: &Path| relative(&out, p);
    write_report(&out, &label, &rows, &a, &b, &rel)?;
    let summary = serde_json::json!({
        "label": label, "a": a.strip_prefix(&root).unwrap_or(&a), "b": b.strip_prefix(&root).unwrap_or(&b),
        "stations": rows.len(), "failing": failing,
        "worstMean": worst_mean, "worstBlock95": worst_block,
        "limits": { "mean": MEAN_LIMIT, "block95": BLOCK_LIMIT },
        "perStation": rows.iter().map(|r| serde_json::json!({
            "station": format!("{}/{}", r.level, r.name), "mean": r.score.mean, "block95": r.score.block95, "max": r.score.max
        })).collect::<Vec<_>>(),
    });
    std::fs::write(
        out.join("summary.json"),
        serde_json::to_string_pretty(&summary).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    write_index(&root.join("parity/report"))?;

    println!(
        "shots {label}: {} stations, {failing} over the limits; worst mean ΔE00 {worst_mean:.3}, worst 95th-percentile block {worst_block:.3}",
        rows.len()
    );
    println!(
        "report: parity/report/shots-{label}/index.html (on the registered server at /parity/report/)"
    );
    Ok(())
}

struct Row {
    level: String,
    name: String,
    score: Score,
    diff: String,
}

#[derive(Clone, Copy, Debug)]
pub struct Score {
    pub mean: f64,
    pub block95: f64,
    pub max: f64,
}

impl Score {
    pub fn passes(&self) -> bool {
        self.mean < MEAN_LIMIT && self.block95 < BLOCK_LIMIT
    }
}

/// An RGB image, 8 bits per channel, sRGB.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
}

impl Image {
    pub fn load(path: &Path) -> Result<Image> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut dec = png::Decoder::new(std::io::BufReader::new(file));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = dec
            .read_info()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        let (w, h) = (info.width as usize, info.height as usize);
        let ch = match info.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            other => return Err(format!("{}: colour type {other:?}", path.display())),
        };
        let mut rgb = Vec::with_capacity(w * h * 3);
        for px in buf[..w * h * ch].chunks(ch) {
            match ch {
                1 | 2 => rgb.extend_from_slice(&[px[0], px[0], px[0]]),
                _ => rgb.extend_from_slice(&px[..3]),
            }
        }
        Ok(Image { w, h, rgb })
    }

    pub fn save(&self, path: &Path) -> Result {
        let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut enc =
            png::Encoder::new(std::io::BufWriter::new(file), self.w as u32, self.h as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut wr = enc.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(&self.rgb).map_err(|e| e.to_string())
    }
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Quarter resolution (2×2 box in linear light), as CIELAB.
fn lab_quarter(img: &Image) -> (usize, usize, Vec<[f64; 3]>) {
    let lut: Vec<f64> = (0..=255u8).map(srgb_to_linear).collect();
    let (w, h) = (img.w / 2, img.h / 2);
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let i = ((y * 2 + dy) * img.w + x * 2 + dx) * 3;
                for c in 0..3 {
                    acc[c] += lut[img.rgb[i + c] as usize];
                }
            }
            out.push(linear_to_lab(acc[0] / 4.0, acc[1] / 4.0, acc[2] / 4.0));
        }
    }
    (w, h, out)
}

pub fn linear_to_lab(r: f64, g: f64, b: f64) -> [f64; 3] {
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    let f = |t: f64| {
        let d: f64 = 6.0 / 29.0;
        if t > d * d * d {
            t.cbrt()
        } else {
            t / (3.0 * d * d) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 colour difference (Sharma, Wu and Dalal 2005).
pub fn ciede2000(l1: [f64; 3], l2: [f64; 3]) -> f64 {
    let (lp1, a1, b1) = (l1[0], l1[1], l1[2]);
    let (lp2, a2, b2) = (l2[0], l2[1], l2[2]);
    let deg = std::f64::consts::PI / 180.0;
    let c1 = a1.hypot(b1);
    let c2 = a2.hypot(b2);
    let cbar = (c1 + c2) / 2.0;
    let c7 = cbar.powi(7);
    let g = 0.5 * (1.0 - (c7 / (c7 + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = (a1p.hypot(b1), a2p.hypot(b2));
    let hue = |b: f64, a: f64| {
        if b == 0.0 && a == 0.0 {
            0.0
        } else {
            let h = b.atan2(a) / deg;
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dl = lp2 - lp1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else {
        let d = h2p - h1p;
        if d.abs() <= 180.0 {
            d
        } else if d > 180.0 {
            d - 360.0
        } else {
            d + 360.0
        }
    };
    let dhh = 2.0 * (c1p * c2p).sqrt() * (dh * deg / 2.0).sin();
    let lbar = (lp1 + lp2) / 2.0;
    let cbarp = (c1p + c2p) / 2.0;
    let hbar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * ((hbar - 30.0) * deg).cos()
        + 0.24 * ((2.0 * hbar) * deg).cos()
        + 0.32 * ((3.0 * hbar + 6.0) * deg).cos()
        - 0.20 * ((4.0 * hbar - 63.0) * deg).cos();
    let dtheta = 30.0 * (-((hbar - 275.0) / 25.0).powi(2)).exp();
    let cp7 = cbarp.powi(7);
    let rc = 2.0 * (cp7 / (cp7 + 25f64.powi(7))).sqrt();
    let l50 = (lbar - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l50 / (20.0 + l50).sqrt();
    let sc = 1.0 + 0.045 * cbarp;
    let sh = 1.0 + 0.015 * cbarp * t;
    let rt = -(2.0 * dtheta * deg).sin() * rc;
    let (x, y, z) = (dl / sl, dc / sc, dhh / sh);
    (x * x + y * y + z * z + rt * y * z).sqrt()
}

/// The score of b against a, and a difference map (quarter resolution;
/// black is no difference, white is ΔE00 of 12 or more).
pub fn compare(a: &Image, b: &Image) -> Result<(Score, Image)> {
    if a.w != b.w || a.h != b.h {
        return Err(format!("sizes differ: {}×{} and {}×{}", a.w, a.h, b.w, b.h));
    }
    let (w, h, la) = lab_quarter(a);
    let (_, _, lb) = lab_quarter(b);
    let de: Vec<f64> = la.iter().zip(&lb).map(|(p, q)| ciede2000(*p, *q)).collect();
    let mean = de.iter().sum::<f64>() / de.len() as f64;
    let max = de.iter().copied().fold(0.0, f64::max);
    let mut blocks = Vec::new();
    for by in (0..h).step_by(BLOCK) {
        for bx in (0..w).step_by(BLOCK) {
            let (mut s, mut n) = (0.0, 0);
            for y in by..(by + BLOCK).min(h) {
                for x in bx..(bx + BLOCK).min(w) {
                    s += de[y * w + x];
                    n += 1;
                }
            }
            blocks.push(s / n as f64);
        }
    }
    blocks.sort_by(f64::total_cmp);
    let block95 = blocks[((blocks.len() as f64 * 0.95).ceil() as usize).saturating_sub(1)];
    let mut map = Image {
        w,
        h,
        rgb: Vec::with_capacity(w * h * 3),
    };
    for d in &de {
        let v = (d / 12.0 * 255.0).round().clamp(0.0, 255.0) as u8;
        map.rgb.extend_from_slice(&[v, v, v]);
    }
    Ok((Score { mean, block95, max }, map))
}

fn subdirs(dir: &Path) -> Result<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    Ok(v)
}

fn pngs(dir: &Path) -> Result<Vec<String>> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".png"))
        .collect();
    v.sort();
    Ok(v)
}

/// `to` as a URL path relative to the directory `from` (both under the repo).
fn relative(from: &Path, to: &Path) -> String {
    let root = root();
    let f = from.strip_prefix(&root).unwrap_or(from);
    let t = to.strip_prefix(&root).unwrap_or(to);
    let up = f.components().count();
    let mut s = "../".repeat(up);
    s += &t.to_string_lossy();
    s
}

fn write_report(
    out: &Path,
    label: &str,
    rows: &[Row],
    a: &Path,
    b: &Path,
    rel: &dyn Fn(&Path) -> String,
) -> Result {
    let mut levels: BTreeMap<&str, (usize, usize, f64, f64)> = BTreeMap::new();
    for r in rows {
        let e = levels.entry(&r.level).or_default();
        e.0 += 1;
        e.1 += (!r.score.passes()) as usize;
        e.2 = e.2.max(r.score.mean);
        e.3 = e.3.max(r.score.block95);
    }
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Shots: {label}</title><style>
:root {{ color-scheme: dark; }} body {{ margin: 0; padding: 16px; background: #0d1017; color: #e6e9ef; font: 14px/1.45 system-ui, sans-serif; }}
table {{ border-collapse: collapse; }} td, th {{ padding: 4px 10px; border-bottom: 1px solid #262c3a; text-align: right; }} th:first-child, td:first-child {{ text-align: left; }}
.row {{ display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 6px; margin: 18px 0 4px; }} .row img {{ width: 100%; display: block; border-radius: 4px; }}
.bad {{ color: #ff7a6b; }} .ok {{ color: #7be0a0; }} h2 {{ margin-top: 28px; }} small {{ color: #9aa3b5; }}
</style></head><body><h1>Screenshots: {label}</h1>
<p><small>A: {} · B: {} · CIEDE2000 on quarter-resolution images · pass: mean &lt; {MEAN_LIMIT} and 95 % of 16-px blocks &lt; {BLOCK_LIMIT} · difference maps: white is ΔE 12 or more</small></p>
<table><tr><th>Level</th><th>Stations</th><th>Over the limits</th><th>Worst mean</th><th>Worst 95 % block</th></tr>"#,
        rel(a),
        rel(b)
    );
    for (lv, (n, bad, m, b95)) in &levels {
        let _ = write!(
            h,
            "<tr><td>{lv}</td><td>{n}</td><td class=\"{}\">{bad}</td><td>{m:.3}</td><td>{b95:.3}</td></tr>",
            if *bad > 0 { "bad" } else { "ok" }
        );
    }
    h += "</table><h2>Every station, worst first</h2>";
    for r in rows {
        let pa = rel(&a.join(&r.level).join(&r.name));
        let pb = rel(&b.join(&r.level).join(&r.name));
        let _ = write!(
            h,
            r#"<div class="row"><img loading="lazy" src="{pa}" alt="A"><img loading="lazy" src="{pb}" alt="B"><img loading="lazy" src="diff/{}" alt="difference"></div>
<div class="{}">{}/{}: mean {:.3}, 95 % block {:.3}, max {:.2}</div>"#,
            r.diff,
            if r.score.passes() { "ok" } else { "bad" },
            r.level,
            r.name,
            r.score.mean,
            r.score.block95,
            r.score.max
        );
    }
    h += "</body></html>\n";
    std::fs::write(out.join("index.html"), h).map_err(|e| e.to_string())
}

/// parity/report/index.html: a link to every report there.
pub fn write_index(dir: &Path) -> Result {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("index.html").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut h = String::from(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Parity reports</title>\
         <style>:root{color-scheme:dark}body{margin:0;padding:16px;background:#0d1017;color:#e6e9ef;font:15px/1.5 system-ui,sans-serif}a{color:#8ab4ff}</style></head>\
         <body><h1>Parity reports</h1><ul>",
    );
    for n in names {
        let _ = write!(h, "<li><a href=\"{n}/\">{n}</a></li>");
    }
    h += "</ul></body></html>\n";
    std::fs::write(dir.join("index.html"), h).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Sharma, Wu and Dalal's test data, pairs 1, 7 and 17.
    #[test]
    fn ciede2000_matches_the_published_pairs() {
        let cases = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
        ];
        for (a, b, want) in cases {
            let got = ciede2000(a, b);
            assert!((got - want).abs() < 1e-4, "{a:?} {b:?}: {got} vs {want}");
        }
    }

    #[test]
    fn white_is_l100() {
        let w = linear_to_lab(1.0, 1.0, 1.0);
        assert!(
            (w[0] - 100.0).abs() < 1e-3 && w[1].abs() < 1e-2 && w[2].abs() < 1e-2,
            "{w:?}"
        );
    }

    #[test]
    fn identical_images_score_zero() {
        let img = Image {
            w: 64,
            h: 32,
            rgb: (0..64 * 32 * 3).map(|i| (i % 251) as u8).collect(),
        };
        let (s, _) = compare(&img, &img).unwrap();
        assert_eq!((s.mean, s.block95, s.max), (0.0, 0.0, 0.0));
    }
}
