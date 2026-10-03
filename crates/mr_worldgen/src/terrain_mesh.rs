//! Turns the Terrain height function into render tiles: 4 m near the road,
//! coarser further out. Each tile hangs a skirt off its edges so differing
//! resolutions never show a crack. (Port of `src/world/TerrainMesh.js`
//! without its colouriser, which is [`crate::colorizer`]; roadmap WP 3.4.)
//!
//! [`build_tile_geometry`] is `buildTileGeometry`, [`terrain_material`] the
//! `MeshStandardMaterial` with `patchTriplanar`'s tag, and
//! [`build_terrain_meshes`] is `buildTerrainMeshes` in one call. In a level
//! build the same work runs as jobs ([`terrain_stages`]): the tiles in
//! batches of 24, reporting `0.1 + f × 0.55` as the JS does between its
//! yields, then the merge into meshes. The tiles are pure functions of the
//! terrain, so natively (feature `parallel`) a batch's tiles are built on
//! threads; the meshes are assembled in the JS order either way.
//!
//! The ground shader itself (the GLSL `patchTriplanar` injects) is the
//! renderer's: the scene carries the material kind `Terrain`, its options
//! and uniforms (SPEC 6.2).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mr_levels::survey::SeasideData;
use mr_math::{clamp, hash2, js, kernel};
use mr_scene::{MaterialKind, TextureSource, three};
use serde_json::{Value, json};

use crate::colorizer::Colorizer;
use crate::material::{Material, num, texture_value};
use crate::object::{Image, Layer, MaterialId, NodeId, SceneGraph, TextureId};
use crate::terrain::{ColorFn, TERRAIN_TILE, Terrain, TerrainOpts, TerrainPlan, Tile};
use crate::textures::{Texture, TextureCache};
use crate::three_geom::{BufferAttribute, BufferGeometry, merge_geometries};
use crate::world::{Job, StageFn, World};

/// A tile's vertex arrays as `buildTileGeometry` fills them.
struct Verts {
    pos: Vec<f32>,
    nor: Vec<f32>,
    col: Vec<f32>,
    uv: Vec<f32>,
    surf: Vec<f32>,
    v: usize,
}

impl Verts {
    #[allow(clippy::too_many_arguments)]
    fn put(
        &mut self,
        x: f64,
        y: f64,
        z: f64,
        nx: f64,
        ny: f64,
        nz: f64,
        c: [f64; 3],
        pv: f64,
    ) -> usize {
        let v = self.v;
        self.surf[v] = pv as f32;
        self.pos[v * 3] = x as f32;
        self.pos[v * 3 + 1] = y as f32;
        self.pos[v * 3 + 2] = z as f32;
        self.nor[v * 3] = nx as f32;
        self.nor[v * 3 + 1] = ny as f32;
        self.nor[v * 3 + 2] = nz as f32;
        self.col[v * 3] = c[0] as f32;
        self.col[v * 3 + 1] = c[1] as f32;
        self.col[v * 3 + 2] = c[2] as f32;
        self.uv[v * 2] = (x / 9.0) as f32;
        self.uv[v * 2 + 1] = (z / 9.0) as f32;
        self.v += 1;
        v
    }
}

/// `buildTileGeometry(terrain, colorizer, tile)`.
pub fn build_tile_geometry(
    terrain: &Terrain,
    colorizer: &Colorizer,
    tile: &Tile,
) -> BufferGeometry {
    let (x0, z0, size, step) = (tile.x0, tile.z0, tile.size, tile.step);
    let n = js::round(size / step) as usize;
    let vn = n + 1;
    // Heights on a padded grid so normals at edges are correct.
    let p1 = vn + 2;
    let mut hh = vec![0f32; p1 * p1];
    for j in 0..p1 {
        for i in 0..p1 {
            hh[j * p1 + i] = terrain
                .height_at(x0 + (i as f64 - 1.0) * step, z0 + (j as f64 - 1.0) * step)
                as f32;
        }
    }
    // Stitch: along an edge shared with a coarser tile, use that tile's
    // linear interpolation so the two surfaces meet exactly.
    let at = |i: usize, j: usize| (j + 1) * p1 + (i + 1);
    let edge_idx = |e: usize, k: usize| match e {
        0 => at(k, 0),
        1 => at(k, vn - 1),
        2 => at(0, k),
        _ => at(vn - 1, k),
    };
    for (e, &ns) in tile.nsteps.iter().enumerate() {
        if ns <= step {
            continue;
        }
        let ratio = js::round(ns / step) as usize;
        let src: Vec<f32> = (0..vn).map(|k| hh[edge_idx(e, k)]).collect();
        for k in 0..vn {
            let k0 = (k / ratio) * ratio;
            let k1 = (vn - 1).min(k0 + ratio);
            if k0 == k {
                continue;
            }
            let (s0, s1) = (f64::from(src[k0]), f64::from(src[k1]));
            hh[edge_idx(e, k)] = (s0 + (s1 - s0) * ((k - k0) as f64 / (k1 - k0) as f64)) as f32;
        }
    }
    let skirt_depth = step * 1.5 + 3.0;
    let v_count = vn * vn + 4 * vn;
    let mut vt = Verts {
        pos: vec![0f32; v_count * 3],
        nor: vec![0f32; v_count * 3],
        col: vec![0f32; v_count * 3],
        uv: vec![0f32; v_count * 2],
        surf: vec![0f32; v_count],
        v: 0,
    };
    let hf = |k: usize| f64::from(hh[k]);
    for j in 0..vn {
        for i in 0..vn {
            let h = hf((j + 1) * p1 + (i + 1));
            let hl = hf((j + 1) * p1 + i);
            let hr = hf((j + 1) * p1 + i + 2);
            let hd = hf(j * p1 + i + 1);
            let hu = hf((j + 2) * p1 + i + 1);
            let (mut nx, mut ny, mut nz) = (hl - hr, 2.0 * step, hd - hu);
            let l = kernel::hypot3(nx, ny, nz);
            nx /= l;
            ny /= l;
            nz /= l;
            let x = x0 + i as f64 * step;
            let z = z0 + j as f64 * step;
            let (mut tmp, paved) = colorizer.color(x, h, z, ny);
            // Crease shading: ground lower than its neighbours (gullies, the foot of
            // a cutting) darkens, crests lighten a touch. Measured in grade over one
            // node step so it reads alike on fine and coarse tiles.
            let cav = ((hl + hr + hd + hu) * 0.25 - h) / step;
            let ao = clamp(1.0 - cav * 0.9, 0.7, 1.06);
            // Mottling a few nodes across so neighbouring vertices aren't identical.
            let mot = 1.0
                + colorizer.noise(x / 23.0 + 11.3, z / 23.0 - 4.1) * 0.06
                + (hash2(js::round(x), js::round(z), 5.0) - 0.5) * 0.035;
            tmp[0] *= ao * mot;
            tmp[1] *= ao * mot;
            tmp[2] *= ao * mot;
            vt.put(x, h, z, nx, ny, nz, tmp, paved);
        }
    }
    let mut idx: Vec<u32> = Vec::with_capacity(n * n * 6 + 4 * (vn - 1) * 12);
    for j in 0..n {
        for i in 0..n {
            let a = j * vn + i;
            let b = a + 1;
            let c = a + vn;
            let d = c + 1;
            // Split each quad along the diagonal whose ends are closest in height,
            // so terrace edges and cliff lips follow the ground instead of zigzagging
            // across it; on even ground alternate for a less directional look.
            let py = |k: usize| f64::from(vt.pos[k * 3 + 1]);
            let dad = (py(a) - py(d)).abs();
            let dbc = (py(b) - py(c)).abs();
            let use_bc = if (dad - dbc).abs() > 0.25 {
                dbc < dad
            } else {
                (i + j) & 1 != 0
            };
            let q = if use_bc {
                [a, c, b, b, c, d]
            } else {
                [a, c, d, a, d, b]
            };
            idx.extend(q.iter().map(|&k| k as u32));
        }
    }
    // Skirts: four edges.
    let edges = |e: usize, k: usize| -> usize {
        match e {
            0 => k,                 // z0 edge  (j=0)
            1 => (vn - 1) * vn + k, // z1 edge  (j=V-1)
            2 => k * vn,            // x0 edge  (i=0)
            _ => k * vn + vn - 1,   // x1 edge  (i=V-1)
        }
    };
    for e in 0..4 {
        let base = vt.v;
        for k in 0..vn {
            let src = edges(e, k);
            let p = |a: &[f32], o: usize| f64::from(a[src * 3 + o]);
            let tmp = [p(&vt.col, 0), p(&vt.col, 1), p(&vt.col, 2)];
            let (x, y, z) = (p(&vt.pos, 0), p(&vt.pos, 1) - skirt_depth, p(&vt.pos, 2));
            let (nx, ny, nz) = (p(&vt.nor, 0), p(&vt.nor, 1), p(&vt.nor, 2));
            let pv = f64::from(vt.surf[src]);
            vt.put(x, y, z, nx, ny, nz, tmp, pv);
        }
        for k in 0..vn - 1 {
            let (a, b, c, d) = (edges(e, k), edges(e, k + 1), base + k, base + k + 1);
            // Double-sided by emitting both windings — skirts are thin and cheap.
            idx.extend(
                [a, b, c, b, d, c, a, c, b, b, c, d]
                    .iter()
                    .map(|&k| k as u32),
            );
        }
    }
    let mut g = BufferGeometry::new();
    g.set_attribute("position", BufferAttribute::from_f32(vt.pos, 3));
    g.set_attribute("normal", BufferAttribute::from_f32(vt.nor, 3));
    g.set_attribute("color", BufferAttribute::from_f32(vt.col, 3));
    g.set_attribute("uv", BufferAttribute::from_f32(vt.uv, 2));
    g.set_attribute("aSurf", BufferAttribute::from_f32(vt.surf, 1));
    g.set_index_attribute(Some(if v_count > 65535 {
        BufferAttribute::from_u32(idx, 1)
    } else {
        BufferAttribute::from_u16(&idx, 1)
    }));
    g
}

// ── The ground photo ───────────────────────────────────────────────────

/// A level's draped aerial photo (`level.groundPhoto`) as textures for the
/// ground shader: `{ tex, loose, box: [x0, z0, x1, z1] }`. Inside the box
/// it is the ground's colour, fading back to the vertex colours over its
/// last 80 m; `loose` is a one-channel texture over the same box, 0 where
/// the ground is paved.
#[derive(Clone)]
pub struct GroundPhoto {
    /// The decoded photo (RGBA rows, row 0 the north edge at z0): decoding
    /// the JPEG is the caller's, as fetching the survey is (DECISIONS D234).
    pub tex: Arc<Texture>,
    /// Where the photo file is, for the scene (`url`).
    pub url: String,
    /// `loose`: one byte per texel, `Math.round(values[k] * 255)`.
    pub loose: Arc<Texture>,
    pub bbox: [f64; 4],
}

impl GroundPhoto {
    /// Seaside's photo (`level.data.photo`): `photo` is `photo.jpg`
    /// decoded, `url` where it was read from.
    pub fn seaside(data: &SeasideData, photo: Texture, url: &str) -> GroundPhoto {
        let g = &data.loose_grid;
        let bytes: Vec<u8> = g
            .values
            .iter()
            .map(|&v| js::round(f64::from(v) * 255.0) as u8)
            .collect();
        GroundPhoto {
            tex: Arc::new(photo),
            url: url.to_string(),
            loose: Arc::new(Texture {
                width: g.w as u32,
                height: g.h as u32,
                rgba: bytes,
                source: TextureSource::Data,
                repeat: false,
                srgb: false,
                anisotropy: 1.0,
            }),
            bbox: [data.photo.x0, data.photo.z0, data.photo.x1, data.photo.z1],
        }
    }
}

/// `level.groundColor` for Seaside: the survey's colour grids.
pub fn seaside_ground_color(data: Arc<SeasideData>) -> ColorFn {
    Arc::new(move |x, z| data.color(x, z))
}

/// The photo's texture as `groundPhoto` sets it up: sRGB, not flipped
/// (row 0 is the north edge, at z0), anisotropy 8, clamped.
fn photo_texture(graph: &mut SceneGraph, p: &GroundPhoto) -> TextureId {
    let mut desc = p.tex.desc("", 0);
    desc.source = TextureSource::Image;
    desc.url = Some(p.url.clone());
    desc.flip_y = false;
    desc.unpack_alignment = 4;
    desc.color_space = "srgb".into();
    desc.anisotropy = 8.0;
    desc.wrap_s = three::CLAMP_TO_EDGE_WRAPPING;
    desc.wrap_t = three::CLAMP_TO_EDGE_WRAPPING;
    graph.add_texture(Image::Own(p.tex.clone()), desc)
}

/// `new THREE.DataTexture(bytes, w, h, RedFormat, UnsignedByteType)` with
/// linear filtering.
fn loose_texture(graph: &mut SceneGraph, p: &GroundPhoto) -> TextureId {
    let mut desc = p.loose.desc("", 0);
    desc.channels = 1;
    desc.format = three::RED_FORMAT;
    desc.flip_y = false;
    desc.color_space = String::new();
    desc.unpack_alignment = 1;
    desc.generate_mipmaps = false;
    desc.wrap_s = three::CLAMP_TO_EDGE_WRAPPING;
    desc.wrap_t = three::CLAMP_TO_EDGE_WRAPPING;
    desc.mag_filter = three::LINEAR_FILTER;
    desc.min_filter = three::LINEAR_FILTER;
    desc.anisotropy = 1.0;
    graph.add_texture(Image::Own(p.loose.clone()), desc)
}

// ── The material ───────────────────────────────────────────────────────

/// The terrain's material: `new MeshStandardMaterial({ vertexColors: true,
/// map: detailTexture(), roughness: 0.96, metalness: 0 })` patched by
/// `patchTriplanar(material, rockTexture(), terrainDetailTexture(), photo)`.
pub fn terrain_material(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    photo: Option<&GroundPhoto>,
) -> MaterialId {
    let detail = graph.cached_texture(&textures.detail_texture(), Layer::Main, "");
    let rock = graph.cached_texture(&textures.rock_texture(), Layer::Main, "");
    let tdetail = graph.cached_texture(&textures.terrain_detail_texture(), Layer::Main, "");
    let has_photo = photo.is_some();
    // three caches programs by this function's source, which is the same
    // with or without a photo.
    let mut m = Material::standard()
        .set("vertexColors", true)
        .set("map", detail)
        .set("roughness", 0.96)
        .set("metalness", 0.0)
        .kind(
            MaterialKind::Terrain,
            Some(json!({ "packed": true, "photo": has_photo })),
        )
        .program_key(&format!("triplanar:true:{has_photo}"))
        .uniform("tRock", texture_value(rock))
        .uniform("tDetail", texture_value(tdetail));
    if let Some(p) = photo {
        let tp = photo_texture(graph, p);
        let tl = loose_texture(graph, p);
        let [x0, z0, x1, z1] = p.bbox;
        m = m
            .uniform("tPhoto", texture_value(tp))
            .uniform("tLoose", texture_value(tl))
            .uniform(
                "uPhotoBox",
                json!({ "vec": [num(x0), num(z0), num(x1), num(z1)] }),
            );
    }
    // three's own uniform for the material's clipping planes, which the
    // export lists with the patch's.
    m = m.uniform("clippingPlanes", Value::Null);
    graph.add_material(m)
}

// ── Meshes ─────────────────────────────────────────────────────────────

/// Builds the tiles' geometries, on threads natively with `parallel`.
pub fn build_tiles(terrain: &Terrain, tiles: &[Tile]) -> Vec<BufferGeometry> {
    let one = |ts: &[Tile]| -> Vec<BufferGeometry> {
        let c = Colorizer::new(terrain);
        ts.iter()
            .map(|t| build_tile_geometry(terrain, &c, t))
            .collect()
    };
    #[cfg(all(feature = "parallel", not(target_arch = "wasm32")))]
    {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get());
        if threads > 1 && tiles.len() > 1 {
            let per = tiles.len().div_ceil(threads);
            return std::thread::scope(|sc| {
                let hs: Vec<_> = tiles
                    .chunks(per)
                    .map(|ch| sc.spawn(move || one(ch)))
                    .collect();
                hs.into_iter()
                    .flat_map(|h| h.join().expect("a terrain tile panicked"))
                    .collect()
            });
        }
    }
    one(tiles)
}

/// Group coarse tiles into larger meshes to keep draw calls down: the key
/// of the group a tile goes into (`${step}:${gx}:${gz}`).
fn group_key(terrain: &Terrain, tile: &Tile) -> (u64, i64, i64) {
    let g = if tile.step == 4.0 {
        2.0
    } else if tile.step == 16.0 {
        3.0
    } else {
        5.0
    };
    let gx = ((tile.x0 - terrain.min_x) / (TERRAIN_TILE * g)).floor() as i64;
    let gz = ((tile.z0 - terrain.min_z) / (TERRAIN_TILE * g)).floor() as i64;
    (tile.step.to_bits(), gx, gz)
}

/// The meshes from the tiles' geometries (in `tileList` order): merged by
/// group in the order the groups were first met, each with its bounding
/// sphere, receiving shadows, its matrix left alone, under a group named
/// `terrain`.
pub fn assemble(
    graph: &mut SceneGraph,
    terrain: &Terrain,
    tiles: &[Tile],
    geos: Vec<BufferGeometry>,
    material: MaterialId,
) -> NodeId {
    let group = graph.group("terrain");
    let mut index: BTreeMap<(u64, i64, i64), usize> = BTreeMap::new();
    let mut groups: Vec<Vec<BufferGeometry>> = Vec::new();
    for (tile, geo) in tiles.iter().zip(geos) {
        let key = group_key(terrain, tile);
        let k = *index.entry(key).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[k].push(geo);
    }
    for mut geos in groups {
        let mut geo = if geos.len() == 1 {
            geos.pop().expect("one")
        } else {
            let refs: Vec<&BufferGeometry> = geos.iter().collect();
            merge_geometries(&refs, false).expect("terrain tiles merge")
        };
        geo.compute_bounding_sphere();
        let gid = graph.add_geometry(geo);
        let mesh = graph.mesh(gid, material);
        let o = graph.get_mut(mesh);
        o.receive_shadow = true;
        o.matrix_auto_update = false;
        graph.add(group, mesh);
    }
    group
}

/// What `buildTerrainMeshes` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerrainMeshes {
    pub group: NodeId,
    pub material: MaterialId,
}

/// `buildTerrainMeshes(terrain)` in one call: the group `terrain` (not yet
/// added anywhere) and its material.
pub fn build_terrain_meshes(
    graph: &mut SceneGraph,
    textures: &mut TextureCache,
    terrain: &Terrain,
    photo: Option<&GroundPhoto>,
) -> TerrainMeshes {
    let material = terrain_material(graph, textures, photo);
    let tiles = terrain.tile_list();
    let geos = build_tiles(terrain, &tiles);
    let group = assemble(graph, terrain, &tiles, geos, material);
    TerrainMeshes { group, material }
}

// ── The stages of a level build ────────────────────────────────────────

/// What a level's terrain needs besides the track: the options, the plan
/// the scenery would register (until the scenery is ported, DECISIONS
/// D232), and the level's surveyed ground colour and photo (Seaside).
#[derive(Clone, Default)]
pub struct TerrainSetup {
    pub opts: TerrainOpts,
    pub plan: Option<TerrainPlan>,
    pub ground_color: Option<ColorFn>,
    pub photo: Option<GroundPhoto>,
}

/// Tiles per job: the JS yields to the browser every 24 tiles.
const YIELD_EVERY: usize = 24;

/// The terrain's three stages of `World.build` (`Stages::terrain`,
/// `fields`, `terrain_meshes`): `new Terrain` (with the recorded plan, if
/// any), `buildFields` and `resolveFlattens`, and the meshes, as jobs of
/// 24 tiles labelled "Sculpting terrain" at `0.1 + f × 0.55`, then the
/// merge, whose group is added to the world's root.
pub fn terrain_stages(setup: TerrainSetup) -> (StageFn, StageFn, StageFn) {
    let TerrainSetup {
        opts,
        plan,
        ground_color,
        photo,
    } = setup;
    let make: StageFn = Box::new(move |w: &mut World| {
        let mut t = Terrain::new(w.track(), &w.level, &opts)?;
        t.ground_color = ground_color;
        if let Some(p) = &plan {
            p.apply(&mut t);
        }
        w.terrain = Some(t);
        Ok(Vec::new())
    });
    let fields: StageFn = Box::new(|w: &mut World| {
        let track = w.track.as_ref().ok_or("the route is surveyed first")?;
        let t = w.terrain.as_mut().ok_or("no terrain")?;
        t.build_fields(track);
        t.resolve_flattens();
        Ok(Vec::new())
    });
    let meshes: StageFn = Box::new(move |w: &mut World| {
        let t = w.terrain.as_ref().ok_or("no terrain")?;
        let tiles = Arc::new(t.tile_list());
        let built: Arc<Mutex<Vec<BufferGeometry>>> = Arc::new(Mutex::new(Vec::new()));
        let mut jobs = Vec::new();
        let total = tiles.len();
        let mut done = 0;
        while done < total {
            let (from, to) = (done, (done + YIELD_EVERY).min(total));
            let (tiles, built) = (tiles.clone(), built.clone());
            let run = move |w: &mut World| -> Result<Vec<Job>, String> {
                let t = w.terrain.as_ref().ok_or("no terrain")?;
                let geos = build_tiles(t, &tiles[from..to]);
                built.lock().map_err(|e| e.to_string())?.extend(geos);
                Ok(Vec::new())
            };
            // The first batch runs in the job that reported 0.1; each later
            // one shows what the JS reported after the batch before it.
            if from == 0 {
                run(w)?;
            } else {
                jobs.push(Job::serial(
                    "Sculpting terrain",
                    0.1 + (from as f64 / total as f64) * 0.55,
                    run,
                ));
            }
            done = to;
        }
        jobs.push(Job::serial(
            "Sculpting terrain",
            0.1 + (done as f64 / total.max(1) as f64) * 0.55,
            move |w: &mut World| {
                let geos = std::mem::take(&mut *built.lock().map_err(|e| e.to_string())?);
                let World {
                    graph,
                    textures,
                    terrain,
                    root,
                    ..
                } = w;
                let t = terrain.as_ref().ok_or("no terrain")?;
                let material = terrain_material(graph, textures, photo.as_ref());
                let group = assemble(graph, t, &tiles, geos, material);
                graph.add(*root, group);
                w.terrain_material = Some(material);
                Ok(Vec::new())
            },
        ));
        Ok(jobs)
    });
    (make, fields, meshes)
}
