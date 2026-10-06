//! The canvas probes (`parity/golden/textures/probes.json`, drawn by Chrome
//! in `tools/parity/textures.mjs`): small scenes for the parts of the
//! Canvas 2D subset that `textures.js` does not reach (clips, composite
//! operations, gradients, shadows, the blur filter, text alignment and
//! baselines, every bundled face, transformed text, drawImage, pixel
//! round trips). Each is interpreted here op by op and compared like the
//! textures (SPEC 5.7): block means always, every pixel when the cached PNG
//! is there, with sheets in `parity/report/textures/probes/`.

use std::path::PathBuf;

use mp_canvas::Canvas;
use mp_canvas::compare::{block_diff, block_means, mean_abs_diff, sheet};
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/textures/probes.json");
const LIMIT: f64 = 3.0;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read_png(p: &PathBuf) -> Option<Vec<u8>> {
    let f = std::fs::File::open(p).ok()?;
    let mut r = png::Decoder::new(std::io::BufReader::new(f))
        .read_info()
        .ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(buf)
}

fn write_png(path: &PathBuf, w: usize, h: usize, rgba: &[u8]) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// The cached JS image, if it is the one summarised (by block means: the
/// probes summary keeps no hash of its own beyond the file's).
fn cached(name: &str, w: usize, h: usize, blocks: &[[f64; 4]]) -> Option<Vec<u8>> {
    for d in std::fs::read_dir(root().join("parity/cache"))
        .ok()?
        .flatten()
    {
        let p = d.path().join("textures/probes").join(format!("{name}.png"));
        if let Some(px) = read_png(&p)
            && px.len() == w * h * 4
            && block_diff(&block_means(w, h, &px), blocks)
                .iter()
                .all(|&v| v < 0.006)
        {
            return Some(px);
        }
    }
    None
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// Runs a probe definition (`{w, h, ops}`) on a fresh canvas.
fn run(def: &Value) -> Canvas {
    let mut g = Canvas::new(
        def["w"].as_u64().unwrap() as u32,
        def["h"].as_u64().unwrap() as u32,
    );
    for op in def["ops"].as_array().unwrap() {
        let op = op.as_array().unwrap();
        let m = op[0].as_str().unwrap();
        let a = &op[1..];
        let n = |i: usize| f(&a[i]);
        let s = |i: usize| a[i].as_str().unwrap();
        match m {
            "=" => {
                let v = &a[1];
                match s(0) {
                    "fillStyle" => g.set_fill_style(v.as_str().unwrap()),
                    "strokeStyle" => g.set_stroke_style(v.as_str().unwrap()),
                    "lineWidth" => g.set_line_width(f(v)),
                    "lineCap" => g.set_line_cap(v.as_str().unwrap()),
                    "lineJoin" => g.set_line_join(v.as_str().unwrap()),
                    "font" => g.set_font(v.as_str().unwrap()),
                    "textAlign" => g.set_text_align(v.as_str().unwrap()),
                    "textBaseline" => g.set_text_baseline(v.as_str().unwrap()),
                    "globalCompositeOperation" => {
                        g.set_global_composite_operation(v.as_str().unwrap())
                    }
                    "globalAlpha" => g.set_global_alpha(f(v)),
                    "shadowColor" => g.set_shadow_color(v.as_str().unwrap()),
                    "shadowBlur" => g.set_shadow_blur(f(v)),
                    "filter" => g.set_filter(v.as_str().unwrap()),
                    p => panic!("probe sets {p}"),
                }
            }
            "grad" => {
                let c: Vec<f64> = a[2].as_array().unwrap().iter().map(f).collect();
                let mut grd = if s(1) == "linear" {
                    g.create_linear_gradient(c[0], c[1], c[2], c[3])
                } else {
                    g.create_radial_gradient(c[0], c[1], c[2], c[3], c[4], c[5])
                };
                for st in a[3].as_array().unwrap() {
                    grd.add_color_stop(f(&st[0]), st[1].as_str().unwrap());
                }
                if s(0) == "fillStyle" {
                    g.set_fill_style(&grd);
                } else {
                    g.set_stroke_style(&grd);
                }
            }
            "image" => {
                let src = run(&a[0]);
                let v: Vec<f64> = a[1..].iter().map(f).collect();
                if v.len() == 4 {
                    g.draw_image(&src, v[0], v[1], v[2], v[3]);
                } else {
                    g.draw_image_sub(&src, v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
                }
            }
            "putGradient" => {
                let (x, y, w, h) = (n(0) as i32, n(1) as i32, n(2) as u32, n(3) as u32);
                let mut img = g.create_image_data(w, h);
                for j in 0..h as usize {
                    for i in 0..w as usize {
                        let k = (j * w as usize + i) * 4;
                        img.set(k, (i * 8) as f64);
                        img.set(k + 1, (j * 8) as f64);
                        img.set(k + 2, 200.0);
                        img.set(k + 3, ((i + j) * 4) as f64);
                    }
                }
                g.put_image_data(&img, x, y);
                let back = g.get_image_data(x, y, w, h);
                g.put_image_data(&back, x + 24, y + 24);
            }
            "fillRect" => g.fill_rect(n(0), n(1), n(2), n(3)),
            "strokeRect" => g.stroke_rect(n(0), n(1), n(2), n(3)),
            "clearRect" => g.clear_rect(n(0), n(1), n(2), n(3)),
            "beginPath" => g.begin_path(),
            "closePath" => g.close_path(),
            "moveTo" => g.move_to(n(0), n(1)),
            "lineTo" => g.line_to(n(0), n(1)),
            "quadraticCurveTo" => g.quadratic_curve_to(n(0), n(1), n(2), n(3)),
            "rect" => g.rect(n(0), n(1), n(2), n(3)),
            "arc" => g.arc(
                n(0),
                n(1),
                n(2),
                n(3),
                n(4),
                a.get(5).and_then(Value::as_bool).unwrap_or(false),
            ),
            "ellipse" => g.ellipse(
                n(0),
                n(1),
                n(2),
                n(3),
                n(4),
                n(5),
                n(6),
                a.get(7).and_then(Value::as_bool).unwrap_or(false),
            ),
            "fill" => g.fill(),
            "stroke" => g.stroke(),
            "clip" => g.clip(),
            "save" => g.save(),
            "restore" => g.restore(),
            "translate" => g.translate(n(0), n(1)),
            "rotate" => g.rotate(n(0)),
            "scale" => g.scale(n(0), n(1)),
            "fillText" if a.len() == 4 => g.fill_text_max(s(0), n(1), n(2), n(3)),
            "fillText" => g.fill_text(s(0), n(1), n(2)),
            "strokeText" => g.stroke_text(s(0), n(1), n(2)),
            other => panic!("probe op {other}"),
        }
    }
    g
}

#[test]
fn probes_match_chrome() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let report = root().join("parity/report/textures/probes");
    let mut failures = Vec::new();
    let mut lines = Vec::new();
    for p in golden["probes"].as_array().unwrap() {
        let name = p["name"].as_str().unwrap();
        let (w, h) = (
            p["width"].as_u64().unwrap() as usize,
            p["height"].as_u64().unwrap() as usize,
        );
        let rgba = run(p).to_rgba();
        let want: Vec<[f64; 4]> = p["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| [f(&b[0]), f(&b[1]), f(&b[2]), f(&b[3])])
            .collect();
        let bd = block_diff(&block_means(w, h, &rgba), &want);
        if bd.iter().any(|&v| v >= LIMIT) {
            failures.push(format!("{name}: block means differ by {bd:.2?}"));
        }
        let mad = cached(name, w, h, &want).map(|js| {
            let mad = mean_abs_diff(&js, &rgba);
            if mad.iter().any(|&v| v >= LIMIT) {
                failures.push(format!("{name}: mean abs diff {mad:.2?} (limit {LIMIT})"));
            }
            std::fs::create_dir_all(&report).unwrap();
            let (sw, sh, px) = sheet(w, h, &js, &rgba);
            write_png(&report.join(format!("{name}.png")), sw, sh, &px);
            mad
        });
        let m = mad.map_or("(no JS image cached)".to_string(), |m| {
            format!("{:.3} {:.3} {:.3} {:.3}", m[0], m[1], m[2], m[3])
        });
        println!(
            "{name:<16} {m:>30}   blocks {:.3} {:.3} {:.3} {:.3}",
            bd[0], bd[1], bd[2], bd[3]
        );
        lines.push((name.to_string(), mad));
    }
    if lines.iter().any(|l| l.1.is_some()) {
        let mut html = String::from(
            "<!doctype html><meta charset=utf-8><title>Canvas probes</title><style>body{font:14px system-ui;background:#1b1d22;color:#ddd;margin:16px}td,th{padding:4px 10px;text-align:right}td:first-child{text-align:left}.bad{color:#f66}img{image-rendering:pixelated;width:min(100%,960px)}</style>\n<h1>Canvas probes: JS (left), Rust (middle), difference ×8 (right)</h1>\n<p>Generated by <code>cargo test -p mp_canvas --test probes</code>. <a href=\"../index.html\">Textures</a>.</p><table><tr><th>probe</th><th>R</th><th>G</th><th>B</th><th>A</th></tr>\n",
        );
        for (name, mad) in &lines {
            if let Some(m) = mad {
                let bad = m.iter().any(|&v| v >= LIMIT);
                html += &format!(
                    "<tr class={}><td><a href=\"#{name}\">{name}</a></td><td>{:.3}</td><td>{:.3}</td><td>{:.3}</td><td>{:.3}</td></tr>\n",
                    if bad { "bad" } else { "ok" },
                    m[0],
                    m[1],
                    m[2],
                    m[3]
                );
            }
        }
        html += "</table>\n";
        for (name, _) in lines.iter().filter(|l| l.1.is_some()) {
            html +=
                &format!("<h2 id=\"{name}\">{name}</h2><img src=\"{name}.png\" alt=\"{name}\">\n");
        }
        std::fs::write(report.join("index.html"), html).unwrap();
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
