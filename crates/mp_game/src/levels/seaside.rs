//! Seaside Raceway built by `mp_worldgen` in the client (roadmap WP 7.4;
//! DECISIONS D595's list, D680 onwards).
//!
//! The build is `tests/seaside_animators.rs`'s: the level prepared with
//! the survey (`seaside::prepare`), `World::with_level_data` with the same
//! `Arc<SeasideData>` (Raceway's `world.level.data`), the terrain with the
//! survey's ground colour and the draped photo (`GroundPhoto::seaside`),
//! and Raceway as the scenery.
//!
//! **The survey** is the Track's: `make_track` parses it when it arrives
//! (the page's download, or the file natively) and hands a copy here
//! ([`survey_parsed`]), so it is read once.
//!
//! **The photo** (`src/levels/seaside/photo.jpg`, 1843 × 2160) is decoded
//! as the JS game's texture is: on the web by the browser (the page draws
//! the image on a canvas and hands in `getImageData`'s bytes,
//! [`load_photo`], as the scene exporter reads it), natively by the `image`
//! crate's JPEG decoder (D681), once, and kept for every later build. It
//! is needed only when the build is the drawn scene (`?world=gen`): with
//! the export drawn, the build is for the animators alone, which never read
//! the photo, and a 1 × 1 blank stands in (the texture keeps its place, so
//! the build is numbered as the export).

use mp_levels::SeasideData;
use mp_worldgen::stages::{LevelSetup, level_stages};
use mp_worldgen::terrain_mesh::{GroundPhoto, TerrainSetup, seaside_ground_color};
use mp_worldgen::textures::Texture;
use mp_worldgen::world::{Build, Scenery, SceneryInfo, World, level_jobs};
use std::sync::{Arc, Mutex};

/// Where the photo is, as the scene export names it (the texture's `url`).
pub const PHOTO_URL: &str = "src/levels/seaside/photo.jpg";

/// The survey (`Ok`), or why there is none; `None` until it arrives.
type Survey = Option<Result<Arc<SeasideData>, String>>;

static SURVEY: Mutex<Survey> = Mutex::new(None);

/// The decoded photo (the page's on the web, the file's natively), shared
/// by every drawn build (the race's and the menu's sections) and kept, so
/// it is decoded once.
static PHOTO: Mutex<Option<Result<Arc<Texture>, String>>> = Mutex::new(None);

/// `make_track` parsed the survey (or could not get it).
pub fn survey_parsed(s: Result<Arc<SeasideData>, String>) {
    *SURVEY.lock().unwrap_or_else(|e| e.into_inner()) = Some(s);
}

/// The survey and, for a drawn build on the web, the photo are in.
pub fn inputs_ready(draws: bool) -> bool {
    let survey = SURVEY.lock().unwrap_or_else(|e| e.into_inner()).is_some();
    #[cfg(target_arch = "wasm32")]
    {
        survey && (!draws || PHOTO.lock().unwrap_or_else(|e| e.into_inner()).is_some())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = draws;
        survey
    }
}

/// The photo as `groundPhoto` gets it: the decoded image's RGBA rows, top
/// first (row 0 the north edge, `flipY` false), sRGB, anisotropy 8.
fn photo_texture(width: u32, height: u32, rgba: Vec<u8>) -> Texture {
    Texture {
        width,
        height,
        rgba,
        source: mp_scene::TextureSource::Image,
        repeat: false,
        srgb: true,
        anisotropy: 8.0,
    }
}

/// The photo for a build: decoded (natively from the file, once; on the
/// web the page's) when the build is drawn, else a 1 × 1 blank.
fn photo(draws: bool) -> Arc<Texture> {
    let blank = || Arc::new(photo_texture(1, 1, vec![0; 4]));
    if !draws {
        return blank();
    }
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut stash = PHOTO.lock().unwrap_or_else(|e| e.into_inner());
    #[cfg(not(target_arch = "wasm32"))]
    if stash.is_none() {
        *stash = Some(decode_file().map(Arc::new));
    }
    match stash.as_ref() {
        Some(Ok(t)) => t.clone(),
        Some(Err(e)) => {
            bevy::log::warn!("seaside photo: {e}; the ground is drawn without it");
            blank()
        }
        None => {
            bevy::log::warn!("seaside photo: not handed in; the ground is drawn without it");
            blank()
        }
    }
}

/// `photo.jpg` decoded natively (D681).
#[cfg(not(target_arch = "wasm32"))]
fn decode_file() -> Result<Texture, String> {
    let p = crate::native::repo_root().join(PHOTO_URL);
    let t0 = std::time::Instant::now();
    let img = image::ImageReader::open(&p)
        .map_err(|e| format!("{}: {e}", p.display()))?
        .with_guessed_format()
        .map_err(|e| format!("{}: {e}", p.display()))?
        .decode()
        .map_err(|e| format!("{}: {e}", p.display()))?
        .into_rgba8();
    let (w, h) = img.dimensions();
    bevy::log::info!(
        "seaside photo: {w} × {h} decoded in {:.2} s",
        t0.elapsed().as_secs_f64()
    );
    Ok(photo_texture(w, h, img.into_raw()))
}

/// Seaside's scenery: Raceway alone, named as `animate::level1_scenery`
/// names Level 1's (D498: `scenery::PORTED` would link every level's).
fn seaside_scenery(info: &SceneryInfo) -> Option<Box<dyn Scenery>> {
    match info.name {
        "Raceway" => Some(Box::new(mp_worldgen::raceway::Raceway::new(info))),
        _ => None,
    }
}

/// Seaside Raceway's world jobs. Without the survey the level is not
/// prepared, and the first job (the Track) fails, which `animate` reports.
pub fn new_build(draws: bool) -> Build {
    let survey = SURVEY
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .and_then(Result::ok);
    let mut level = mp_levels::level_by_id("seaside");
    let mut terrain = TerrainSetup {
        plan: None,
        ..TerrainSetup::default()
    };
    let world = match survey {
        Some(d) => {
            mp_levels::seaside::prepare(&mut level, d.clone());
            terrain.ground_color = Some(seaside_ground_color(d.clone()));
            // `GroundPhoto::seaside` takes the picture by value; the shared
            // one goes in its place, uncopied.
            let mut p = GroundPhoto::seaside(&d, photo_texture(1, 1, vec![0; 4]), PHOTO_URL);
            p.tex = photo(draws);
            terrain.photo = Some(p);
            World::new(level).with_level_data(d)
        }
        None => World::new(level),
    };
    Build::new(
        world,
        level_jobs(
            level_stages(LevelSetup {
                terrain,
                road: None,
            }),
            seaside_scenery,
        ),
    )
}

/// The page decoded `photo.jpg` (an image drawn on a canvas, then
/// `getImageData`: the bytes the scene exporter reads, D680).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn load_photo(width: u32, height: u32, rgba: Vec<u8>) {
    let r = if rgba.len() == (width * height * 4) as usize {
        Ok(Arc::new(photo_texture(width, height, rgba)))
    } else {
        Err(format!("{} bytes for {width} × {height}", rgba.len()))
    };
    *PHOTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(r);
}

/// The photo is in (the page fetches it only once).
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn has_photo() -> bool {
    matches!(
        *PHOTO.lock().unwrap_or_else(|e| e.into_inner()),
        Some(Ok(_))
    )
}

/// The page could not fetch or decode the photo.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn photo_failed(message: String) {
    *PHOTO.lock().unwrap_or_else(|e| e.into_inner()) = Some(Err(message));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// The native decode against Chrome's, the export's `photo.jpg` texture
    /// (`parity/golden/seaside/seaside.json`, `data`, the image).
    #[test]
    fn the_photo_decodes_as_chrome_does() {
        let golden: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../parity/golden/seaside/seaside.json"
        ))
        .expect("golden parses");
        let want = golden["data"]
            .as_array()
            .and_then(|a| a.iter().find(|t| t["source"] == "image"))
            .expect("the photo's entry");
        let t = decode_file().expect("photo.jpg decodes");
        assert_eq!(
            (u64::from(t.width), u64::from(t.height)),
            (
                want["width"].as_u64().unwrap_or(0),
                want["height"].as_u64().unwrap_or(0)
            )
        );
        if mp_scene::digest::sha256_hex(&t.rgba) == want["sha256"].as_str().unwrap_or("") {
            return; // byte for byte
        }
        // Not byte for byte (D681: two JPEG decoders' IDCT and chroma
        // upsampling): held to Chrome's pixels in the cached export.
        let root = crate::native::repo_root();
        let Some(js) = mp_scene::cache::scenes_dir(&root)
            .ok()
            .map(|d| d.join("seaside.mrscene"))
            .filter(|p| p.exists())
            .and_then(|p| mp_scene::read_file(&p).ok())
        else {
            println!("seaside photo: no cached export to compare the decode with");
            return;
        };
        let tex = js
            .textures
            .iter()
            .find(|t| t.source == mp_scene::TextureSource::Image)
            .expect("the export has the photo");
        let mp_scene::BufferData::U8(chrome) = &js.buffers[tex.pixels as usize].data else {
            panic!("photo pixels are bytes")
        };
        assert_eq!(chrome.len(), t.rgba.len());
        let mut sum = 0u64;
        let mut max = 0u8;
        let mut over2 = 0usize;
        for (a, b) in chrome.iter().zip(&t.rgba) {
            let d = a.abs_diff(*b);
            sum += u64::from(d);
            max = max.max(d);
            over2 += usize::from(d > 2);
        }
        let mean = sum as f64 / chrome.len() as f64;
        println!(
            "seaside photo against Chrome's decode: mean {mean:.3} levels, max {max}, {:.3} % over 2",
            100.0 * over2 as f64 / chrome.len() as f64
        );
        assert!(mean < 1.0, "mean {mean}");
    }
}
