//! `mr_worldgen::flora` against `src/world/valley/flora.js` run under Node
//! with the parity kernel (`parity/golden/flora/flora.json`, written by
//! `tools/parity/flora.mjs`): the 3-D noise, every rock, conifer, canopy,
//! grass, flower and shrub template the scenery makes and the foliage
//! material. The cases are made here as the tool makes them; the
//! requirement is bit-identical. The golden is compiled in, so the test
//! runs in wasm too.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

mod geocheck;

use geocheck::{check_geo, check_material, check_seq};
use mr_math::kernel;
use mr_scene::three;
use mr_worldgen::flora::*;
use mr_worldgen::material::{Material, Param};
use mr_worldgen::three_geom::BufferGeometry;
use serde_json::Value;

const GOLDEN: &str = include_str!("../../../parity/golden/flora/flora.json");

enum Out {
    G(BufferGeometry),
    S(Vec<f64>),
    M(Material),
}

fn cases() -> Vec<(String, Out)> {
    let mut c: Vec<(String, Out)> = Vec::new();
    let mut g = |name: &str, x: BufferGeometry| c.push((name.to_string(), Out::G(x)));
    let mut seqs: Vec<(String, Out)> = Vec::new();
    for seed in [1, 3, 9, 21, 44] {
        let n = make_noise3d(seed);
        let mut out = Vec::new();
        for k in 0..400 {
            let t = k as f64 * 0.6180339887498949;
            out.push(n.noise(
                t * 3.7 - 40.0,
                (k % 23) as f64 * 0.41 - 4.7,
                (k % 17) as f64 * 1.37 - 5.5,
            ));
        }
        out.push(n.noise(0.0, 0.0, 0.0));
        out.push(n.noise(-0.5, 255.5, 256.25));
        out.push(n.noise(1e10 + 0.3, -3e9, kernel::pow(2.0, 31.0) + 0.7));
        seqs.push((format!("noise3d/{seed}"), Out::S(out)));
    }

    let d = RockOpts::default();
    g("rock/default", rock_geometry(7, 2.0, d));
    let crag = RockOpts { crag: true, ..d };
    g("rock/mtn-big-0", rock_geometry(3, 2.0, crag));
    g(
        "rock/mtn-big-1",
        rock_geometry(
            17,
            2.0,
            RockOpts {
                squash: 0.8,
                ..crag
            },
        ),
    );
    g("rock/mtn-far", rock_geometry(5, 1.0, crag));
    g(
        "rock/mtn-small",
        rock_geometry(
            9,
            0.0,
            RockOpts {
                squash: 0.8,
                lichen: 0.4,
                ..d
            },
        ),
    );
    g(
        "rock/mtn-pool",
        rock_geometry(31, 1.0, RockOpts { lichen: 1.6, ..d }),
    );
    g(
        "rock/tint",
        rock_geometry(
            12,
            1.0,
            RockOpts {
                tint: [0.7, 0.5, 0.4],
                lichen: 0.0,
                ..d
            },
        ),
    );

    g("conifer/default", conifer_geometry("spruce", 0, 1));
    for kind in ["spruce", "fir"] {
        for lod in 0..3 {
            g(
                &format!("conifer/{kind}-{lod}"),
                conifer_geometry(kind, lod, 11 + lod as u32),
            );
        }
    }
    g("conifer/mtn-fir", conifer_geometry("fir", 0, 12));
    g("conifer/mtn-mid", conifer_geometry("spruce", 1, 13));
    g("conifer/mtn-far", conifer_geometry("spruce", 2, 14));

    g("canopy/default", canopy_geometry("shade", 3, 0));
    for kind in ["orchard", "poplar", "shade", "willow"] {
        for lod in 0..2 {
            g(
                &format!("canopy/{kind}-{lod}"),
                canopy_geometry(kind, 5, lod),
            );
        }
    }
    g("canopy/valley-orchard", canopy_geometry("orchard", 5, 0));
    g("canopy/valley-poplar", canopy_geometry("poplar", 6, 0));
    g("canopy/valley-shade", canopy_geometry("shade", 7, 0));
    g("canopy/valley-willow", canopy_geometry("willow", 8, 0));
    g("canopy/valley-shade-far", canopy_geometry("shade", 7, 1));

    g("grass/default", grass_clump_geometry(9, 5));
    g("grass/mtn", grass_clump_geometry(11, 5));
    g("grass/valley", grass_clump_geometry(11, 15));
    g("flower/default", flower_geometry(5, 9));
    g("flower/mtn", flower_geometry(5, 9));
    g("flower/valley", flower_geometry(6, 19));
    g("shrub/default", shrub_geometry(21, 1.0));
    g("shrub/hedge", shrub_geometry(44, 0.0));
    g("shrub/detail-2", shrub_geometry(5, 2.0));

    let front = || ("side", Param::Num(three::FRONT_SIDE as f64));
    let rough = || ("roughness", Param::Num(0.95));
    let mut m = |name: &str, x: Material| c.push((name.to_string(), Out::M(x)));
    m("foliage/default", foliage_material(&[]));
    m("foliage/front", foliage_material(&[front()]));
    m("foliage/front-rough", foliage_material(&[front(), rough()]));
    m("foliage/rough", foliage_material(&[rough()]));
    seqs.extend(c);
    seqs
}

#[test]
fn flora_matches_the_js() {
    let golden: Value = serde_json::from_str(GOLDEN).unwrap();
    let want = golden["cases"].as_object().unwrap();
    let mut errors = Vec::new();
    let made = cases();
    for (name, out) in &made {
        let Some(w) = want.get(name) else {
            errors.push(format!("{name}: not in the golden"));
            continue;
        };
        match out {
            Out::G(g) => check_geo(name, Some(g), w, &mut errors),
            Out::S(s) => check_seq(name, s, w, &mut errors),
            Out::M(m) => check_material(name, m, w, &mut errors),
        }
    }
    assert_eq!(made.len(), want.len(), "every golden case is made");
    assert!(errors.is_empty(), "{}", errors.join("\n"));
}
