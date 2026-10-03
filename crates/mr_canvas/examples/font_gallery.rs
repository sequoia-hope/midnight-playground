//! The font gallery (WP 3.2): draws the game's sign strings with each
//! candidate substitute, through mr_canvas exactly as the port will draw
//! them. Run by `node tools/parity/fonts-gallery.mjs`, which fetches the
//! candidates and writes the spec this reads.
//!
//!   cargo run --release -p mr_canvas --example font-gallery -- <spec.json>

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mr_canvas::{Canvas, FontBook};
use serde_json::Value;

fn slug(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

/// One sample sign: background, optional border (as `signTexture` draws
/// one), centred lines, optional neon glow.
fn sign(book: &Arc<FontBook>, s: &Value) -> Canvas {
    let (w, h) = (s["w"].as_f64().unwrap(), s["h"].as_f64().unwrap());
    let mut g = Canvas::with_fonts(w as u32, h as u32, book.clone());
    g.set_fill_style(s["bg"].as_str().unwrap());
    g.fill_rect(0.0, 0.0, w, h);
    if let Some(b) = s["border"].as_str() {
        g.set_stroke_style(b);
        g.set_line_width((h * 0.03).max(4.0));
        g.begin_path();
        g.round_rect(10.0, 10.0, w - 20.0, h - 20.0, h * 0.06);
        g.stroke();
    }
    let fg = s["fg"].as_str().unwrap();
    g.set_fill_style(fg);
    g.set_text_align("center");
    g.set_text_baseline("middle");
    if s["glow"].as_bool().unwrap_or(false) {
        g.set_shadow_color(fg);
        g.set_shadow_blur(16.0);
    }
    let lines = s["lines"].as_array().unwrap();
    let lh = h / (lines.len() as f64 + 0.4);
    for (i, l) in lines.iter().enumerate() {
        g.set_font(l[1].as_str().unwrap());
        g.fill_text(l[0].as_str().unwrap(), w / 2.0, lh * (i as f64 + 0.9));
    }
    g
}

fn alphabet(book: &Arc<FontBook>, family: &str) -> Canvas {
    let mut g = Canvas::with_fonts(900, 132, book.clone());
    g.set_fill_style("#f4f1ea");
    g.fill_rect(0.0, 0.0, 900.0, 132.0);
    g.set_fill_style("#16181d");
    g.set_text_baseline("middle");
    let f = format!("\"{family}\"");
    for (k, (font, text)) in [
        (
            format!("bold 34px {f}"),
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789",
        ),
        // The symbols the game's signs draw (· — → ●), so a missing one
        // shows here as a box; none of them is missing (DECISIONS D372).
        (
            format!("bold 34px {f}"),
            "abcdefghijklmnopqrstuvwxyz · & / → ●",
        ),
        (
            format!("italic bold 34px {f}"),
            "Italic Bold 4,210 — 1/2 MILE",
        ),
    ]
    .iter()
    .enumerate()
    {
        g.set_font(font);
        g.fill_text(text, 12.0, 22.0 + 44.0 * k as f64);
    }
    g
}

/// Canvases stacked with a gap, on the page colour.
fn stack(cs: &[Canvas]) -> (u32, u32, Vec<u8>) {
    let gap = 10u32;
    let w = cs.iter().map(|c| c.width).max().unwrap_or(1);
    let h = cs.iter().map(|c| c.height).sum::<u32>() + gap * (cs.len().saturating_sub(1)) as u32;
    let mut out = vec![0u8; (w * h * 4) as usize];
    for p in out.as_chunks_mut::<4>().0 {
        p.copy_from_slice(&[0x1b, 0x1d, 0x22, 255]);
    }
    let mut y0 = 0;
    for c in cs {
        let px = c.to_rgba();
        for y in 0..c.height {
            for x in 0..c.width {
                let s = ((y * c.width + x) * 4) as usize;
                let d = (((y0 + y) * w + x) * 4) as usize;
                out[d..d + 4].copy_from_slice(&px[s..s + 4]);
            }
        }
        y0 += c.height + gap;
    }
    (w, h, out)
}

fn main() {
    let spec_path = std::env::args()
        .nth(1)
        .expect("usage: font-gallery <spec.json>");
    let spec: Value = serde_json::from_str(&std::fs::read_to_string(&spec_path).unwrap()).unwrap();
    let out = PathBuf::from(spec["out"].as_str().unwrap());
    std::fs::create_dir_all(&out).unwrap();
    let mut html = String::from(include_str!("font_gallery_head.html"));
    for role in spec["roles"].as_array().unwrap() {
        let family = role["family"].as_str().unwrap();
        html += &format!(
            "<section><h2>“{family}”</h2><p class=why>{}</p><div class=grid>\n",
            role["why"].as_str().unwrap()
        );
        for c in role["candidates"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            let mut book = (*FontBook::bundled()).clone();
            let fam = family.to_ascii_lowercase();
            book.faces.retain(|f| f.family != fam);
            for f in c["faces"].as_array().unwrap() {
                let p = f["path"].as_str().unwrap();
                let data: Arc<[u8]> = std::fs::read(p).unwrap().into();
                let w = f["weight"].as_array().unwrap();
                let (w0, w1) = (w[0].as_f64().unwrap() as f32, w[1].as_f64().unwrap() as f32);
                book.add(
                    family,
                    data,
                    (w0, w1),
                    f["style"].as_str() == Some("italic"),
                    p,
                );
            }
            let book = Arc::new(book);
            let mut cs: Vec<Canvas> = role["samples"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| sign(&book, s))
                .collect();
            cs.push(alphabet(&book, family));
            let (w, h, px) = stack(&cs);
            let file = format!("{}-{}.png", slug(family), slug(name));
            write_png(&out.join(&file), w, h, &px);
            let tag = if c["bundled"].as_bool().unwrap() {
                "<span class=tag>bundled</span>"
            } else {
                ""
            };
            html += &format!(
                "<figure><figcaption><b>{name}</b> {tag} <a href=\"{}\">{}</a></figcaption><img src=\"{file}\" alt=\"{family} as {name}\" loading=lazy></figure>\n",
                c["source"]
                    .as_str()
                    .unwrap()
                    .replace("assets/fonts/", "../../../assets/fonts/"),
                c["licence"].as_str().unwrap()
            );
        }
        html += "</div></section>\n";
    }
    html += "</main></body></html>\n";
    std::fs::write(out.join("index.html"), html).unwrap();
}
