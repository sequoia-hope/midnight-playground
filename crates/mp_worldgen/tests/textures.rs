//! L3 gate of WP 3.2 (SPEC 5.7): the shared textures of
//! `src/world/textures.js`, drawn by `mp_worldgen::textures` on
//! `mp_canvas`, against Chrome's canvas (`tools/parity/textures.mjs`,
//! kernel on, bundled fonts).
//!
//! Always checked, from the committed summary
//! (`parity/golden/textures/textures.json`): sizes, the textures made
//! without drawing (bit for bit, by SHA-256), and the 8×8 block means of the
//! rest. When the JS images are in the cache (`parity/cache/<key>/textures/`,
//! regenerated on demand) every pixel is compared: mean absolute difference
//! under 3/255 per channel, and a side-by-side sheet is written to
//! `parity/report/textures/`.

use std::path::PathBuf;

use mp_canvas::compare::{block_diff, block_means, mean_abs_diff, sheet};
use mp_worldgen::textures::{self, Cached, SignOpts, Texture};
use serde_json::Value;
use sha2::{Digest, Sha256};

const GOLDEN: &str = include_str!("../../../parity/golden/textures/textures.json");
/// SPEC 5.7: mean absolute difference per channel, in 0..255 levels.
const LIMIT: f64 = 3.0;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// The cached JS image of a case, if its pixels are the committed ones.
fn cached(name: &str, sha: &str) -> Option<Vec<u8>> {
    let dirs = std::fs::read_dir(root().join("parity/cache")).ok()?;
    for d in dirs.flatten() {
        let p = d.path().join("textures").join(format!("{name}.png"));
        let Ok(f) = std::fs::File::open(&p) else {
            continue;
        };
        let mut r = png::Decoder::new(std::io::BufReader::new(f))
            .read_info()
            .ok()?;
        let mut buf = vec![0; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        if hex(&buf) == sha {
            return Some(buf);
        }
    }
    None
}

fn write_png(path: &PathBuf, w: usize, h: usize, rgba: &[u8]) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// The Rust texture for a case of the summary.
fn render(c: &Value) -> Texture {
    let args = c["args"].as_array().unwrap();
    let num = |i: usize| args[i].as_u64().unwrap() as u32;
    let part = c["part"].as_str();
    let pick = |r: Cached| match (r, part) {
        (Cached::One(t), None) => t,
        (Cached::Sign { texture, .. }, Some("texture")) => texture,
        (Cached::Facade { map, .. }, Some("map")) => map,
        (Cached::Facade { emissive, .. }, Some("emissive")) => emissive,
        _ => panic!("{}: unexpected part", c["name"]),
    };
    match c["fn"].as_str().unwrap() {
        "detailTexture" => textures::detail_texture(),
        "terrainDetailTexture" => textures::terrain_detail_texture(),
        "rockTexture" => textures::rock_texture(),
        "asphaltTexture" => textures::asphalt_texture(num(0)),
        "gravelTexture" => textures::gravel_texture(),
        "concreteTexture" => textures::concrete_texture(),
        "chevronTexture" => textures::chevron_texture(),
        "checkerTexture" => textures::checker_texture(num(0)),
        "glowTexture" => textures::glow_texture(),
        "smokeTexture" => textures::smoke_texture(),
        "facadeTextures" => pick(textures::facade_textures(num(0))),
        "signTexture" => {
            let lines: Vec<&str> = args[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|l| l.as_str().unwrap())
                .collect();
            let o = &args[1];
            let d = SignOpts::default();
            let s = |k: &str, dflt: &'static str| -> String {
                o[k].as_str().unwrap_or(dflt).to_string()
            };
            let (bg, fg, font) = (s("bg", d.bg), s("fg", d.fg), s("font", d.font));
            let border = match o.get("border") {
                None => d.border.map(str::to_string),
                Some(Value::Null) => None,
                Some(v) => Some(v.as_str().unwrap().to_string()),
            };
            let arrow = o["arrow"].as_str().map(str::to_string);
            let opts = SignOpts {
                bg: &bg,
                fg: &fg,
                border: border.as_deref(),
                w: o["w"].as_u64().map_or(d.w, |v| v as u32),
                h: o["h"].as_u64().map_or(d.h, |v| v as u32),
                font: &font,
                arrow: arrow.as_deref(),
            };
            textures::sign_texture(&lines, &opts).0
        }
        f => panic!("no port of {f}"),
    }
}

#[test]
fn shared_textures_match_chrome() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let report = root().join("parity/report/textures");
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    let mut any_cached = false;
    for c in golden["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let t = render(c);
        let (w, h) = (
            c["width"].as_u64().unwrap() as usize,
            c["height"].as_u64().unwrap() as usize,
        );
        assert_eq!(
            (t.width as usize, t.height as usize),
            (w, h),
            "{name}: size"
        );
        let sha = c["sha256"].as_str().unwrap();
        if c["exact"].as_bool().unwrap() && hex(&t.rgba) != sha {
            failures.push(format!("{name}: not bit-identical to the JS"));
        }
        let want: Vec<[f64; 4]> = c["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                let v: Vec<f64> = b
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap())
                    .collect();
                [v[0], v[1], v[2], v[3]]
            })
            .collect();
        let bd = block_diff(&block_means(w, h, &t.rgba), &want);
        if bd.iter().any(|&v| v >= LIMIT) {
            failures.push(format!("{name}: block means differ by {bd:.2?}"));
        }
        let mad = cached(name, sha).map(|js| {
            any_cached = true;
            let mad = mean_abs_diff(&js, &t.rgba);
            if mad.iter().any(|&v| v >= LIMIT) {
                failures.push(format!("{name}: mean abs diff {mad:.2?} (limit {LIMIT})"));
            }
            std::fs::create_dir_all(&report).unwrap();
            let (sw, sh, px) = sheet(w, h, &js, &t.rgba);
            write_png(&report.join(format!("{name}.png")), sw, sh, &px);
            mad
        });
        rows.push((name.to_string(), w, h, bd, mad, hex(&t.rgba) == sha));
    }
    println!(
        "{:<28} {:>9} {:>30} {:>30}",
        "texture", "size", "mean abs diff (R G B A)", "block means (R G B A)"
    );
    for (name, w, h, bd, mad, same) in &rows {
        let m = mad.map_or("(no JS image cached)".to_string(), |m| {
            format!("{:.3} {:.3} {:.3} {:.3}", m[0], m[1], m[2], m[3])
        });
        let tag = if *same { "  identical" } else { "" };
        println!(
            "{name:<28} {:>9} {m:>30} {:>30}{tag}",
            format!("{w}x{h}"),
            format!("{:.3} {:.3} {:.3} {:.3}", bd[0], bd[1], bd[2], bd[3])
        );
    }
    if any_cached {
        let mut html = String::from(
            "<!doctype html><meta charset=utf-8><title>Texture parity</title><style>body{font:14px system-ui;background:#1b1d22;color:#ddd;margin:16px}table{border-collapse:collapse}td,th{padding:4px 10px;text-align:right}td:first-child,th:first-child{text-align:left}.bad{color:#f66}img{max-width:100%;image-rendering:pixelated;background:#fff}</style>\n<h1>Shared textures: JS (left), Rust (middle), difference ×8 (right)</h1>\n<p>SPEC 5.7: mean absolute difference under 3/255 per channel. Generated by <code>cargo test -p mp_worldgen --test textures</code> from <code>tools/parity/textures.mjs</code>'s capture.</p>\n<table><tr><th>texture</th><th>size</th><th>R</th><th>G</th><th>B</th><th>A</th><th></th></tr>\n",
        );
        for (name, w, h, _, mad, same) in &rows {
            let Some(m) = mad else { continue };
            let bad = m.iter().any(|&v| v >= LIMIT);
            html += &format!(
                "<tr class={}><td><a href=\"#{name}\">{name}</a></td><td>{w}×{h}</td><td>{:.3}</td><td>{:.3}</td><td>{:.3}</td><td>{:.3}</td><td>{}</td></tr>\n",
                if bad { "bad" } else { "ok" },
                m[0],
                m[1],
                m[2],
                m[3],
                if *same { "bit-identical" } else { "" }
            );
        }
        html += "</table>\n";
        for (name, ..) in rows.iter().filter(|r| r.4.is_some()) {
            html +=
                &format!("<h2 id=\"{name}\">{name}</h2><img src=\"{name}.png\" alt=\"{name}\">\n");
        }
        std::fs::write(report.join("index.html"), html).unwrap();
        println!("report: parity/report/textures/index.html");
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
