//! `SeasideData::parse` on small survey files built here in the
//! `survey.bin` container format (`tools/seaside/build.py`): the documented
//! fixed-point scales and the int16 delta decoding of heights (wrapping as
//! `<< 16 >> 16` does in `load.js`), grid sampling and blending at and past
//! the edges, and damaged files (truncated, a section missing or of the
//! wrong type, a bad grid) refused with an error, never a panic.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_levels::SeasideData;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// One section: name, type tag, element count and payload bytes.
#[derive(Clone)]
struct Sec {
    name: String,
    kind: u8,
    n: u32,
    payload: Vec<u8>,
}

fn f64s(name: &str, v: &[f64]) -> Sec {
    Sec {
        name: name.into(),
        kind: 1,
        n: v.len() as u32,
        payload: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}

fn i32s(name: &str, v: &[i32]) -> Sec {
    Sec {
        name: name.into(),
        kind: 2,
        n: v.len() as u32,
        payload: v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}

fn bytes(name: &str, v: &[u8]) -> Sec {
    Sec {
        name: name.into(),
        kind: 3,
        n: v.len() as u32,
        payload: v.to_vec(),
    }
}

fn strs(name: &str, v: &[&str]) -> Sec {
    let mut payload = Vec::new();
    for s in v {
        payload.extend((s.len() as u32).to_le_bytes());
        payload.extend(s.as_bytes());
    }
    Sec {
        name: name.into(),
        kind: 4,
        n: v.len() as u32,
        payload,
    }
}

fn lists(name: &str, v: &[&[i32]]) -> Sec {
    let mut payload = Vec::new();
    for l in v {
        payload.extend((l.len() as u32).to_le_bytes());
        payload.extend(l.iter().flat_map(|x| x.to_le_bytes()));
    }
    Sec {
        name: name.into(),
        kind: 5,
        n: v.len() as u32,
        payload,
    }
}

fn zlib(b: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(b, 6)
}

/// A grid's three sections: kind, `[x0, z0, step, w, h, q]`, data.
fn grid(name: &str, kind: &str, meta: [f64; 6], raw: &[u8]) -> Vec<Sec> {
    vec![
        strs(&format!("{name}.kind"), &[kind]),
        f64s(name, &meta),
        bytes(&format!("{name}.data"), &zlib(raw)),
    ]
}

fn heights(deltas: &[i16]) -> Vec<u8> {
    deltas.iter().flat_map(|d| d.to_le_bytes()).collect()
}

const BASE: f64 = 10.0;

/// A complete survey: a four-point line, every list, and 2×2 grids except
/// the fine height grid (3×2) and a 5×5 fine/wide pair for blending.
fn sections() -> Vec<Sec> {
    let mut s = vec![
        f64s("origin", &[600000.0, 4000000.0, 10.0]),
        f64s("lap", &[4.0]),
        f64s("startS", &[0.0]),
        f64s("line.step", &[1.0]),
        f64s("line.y0", &[50.0]),
        i32s("line.x", &[0, 100, 200, -150]),
        i32s("line.z", &[0, 0, 100, 250]),
        i32s("line.y", &[0, 25, -50, 0]),
        i32s("line.bank", &[0, 300, -300, 0]),
        i32s("line.hw", &[600, 600, 650, 600]),
        i32s("line.wallL", &[100, 120, 130, 100]),
        i32s("line.wallR", &[100, 110, 90, 100]),
        i32s("line.runL", &[0, 50, -20, 0]),
        i32s("line.runR", &[0, 10, 20, 0]),
        i32s("pitLane", &[10, 20, 30, 40]),
        lists("walls", &[&[0, 0, 10, 10], &[5, 5]]),
        lists("grandstands", &[&[1, 2, 3, 4, 5, 6]]),
        strs("buildings.name", &["pits", ""]),
        lists("buildings.pts", &[&[0, 0, 10, 0], &[20, 20]]),
        strs("bridges.kind", &["footway"]),
        lists("bridges.pts", &[&[0, 0, 0, 10]]),
        lists("water", &[]),
        lists("parking", &[&[100, 100, 200, 100, 200, 200]]),
        strs("paths.kind", &["track"]),
        lists("paths.pts", &[&[1, 1, 2, 2]]),
        strs("photo.file", &["photo.jpg"]),
        f64s("photo", &[-100.0, -100.0, 100.0, 100.0, 512.0, 512.0]),
        f64s("base", &[BASE]),
    ];
    // Deltas down each column, then along each row: rows of 3.
    s.extend(grid(
        "HEIGHT_FINE",
        "height",
        [0.0, 0.0, 100.0, 3.0, 2.0, 0.5],
        &heights(&[2, 4, -6, 1, 1, 1]),
    ));
    s.extend(grid(
        "HEIGHT_WIDE",
        "height",
        [-1000.0, -1000.0, 1000.0, 3.0, 3.0, 1.0],
        &heights(&[0; 9]),
    ));
    s.extend(grid(
        "COLOR_FINE",
        "rgb5",
        [0.0, 0.0, 10.0, 2.0, 1.0, 1.0],
        &[0, 31, 10, 20, 31, 0],
    ));
    s.extend(grid(
        "COLOR_WIDE",
        "rgb5",
        [0.0, 0.0, 10.0, 2.0, 1.0, 1.0],
        &[0; 6],
    ));
    // Trees: fine grid 400 m square all 1.0, wide grid all 0.2.
    s.extend(grid(
        "TREES_FINE",
        "cover",
        [0.0, 0.0, 100.0, 5.0, 5.0, 1.0],
        &[255; 25],
    ));
    s.extend(grid(
        "TREES_WIDE",
        "cover",
        [-5000.0, -5000.0, 5000.0, 3.0, 3.0, 1.0],
        &[51; 9],
    ));
    s.extend(grid(
        "LOOSE",
        "cover",
        [0.0, 0.0, 1.0, 2.0, 2.0, 1.0],
        &[0, 255, 0, 255],
    ));
    s
}

fn file(secs: &[Sec]) -> Vec<u8> {
    let mut out = b"MRSURVEY".to_vec();
    out.extend(1u32.to_le_bytes());
    out.extend((secs.len() as u32).to_le_bytes());
    for s in secs {
        out.push(s.name.len() as u8);
        out.extend(s.name.as_bytes());
        out.push(s.kind);
        out.extend(s.n.to_le_bytes());
        out.extend(&s.payload);
    }
    out
}

fn parse_no_panic(b: &[u8]) -> Result<SeasideData, String> {
    catch_unwind(AssertUnwindSafe(|| SeasideData::parse(b)))
        .unwrap_or_else(|_| panic!("parse panicked on {} bytes", b.len()))
}

fn with(name: &str, sec: Sec) -> Vec<u8> {
    let mut s = sections();
    let k = s.iter().position(|x| x.name == name).unwrap();
    s[k] = sec;
    file(&s)
}

#[test]
fn a_complete_file_decodes_with_the_documented_scales() {
    let d = SeasideData::parse(&file(&sections())).unwrap();
    assert_eq!(d.origin, [600000.0, 4000000.0, 10.0]);
    assert_eq!((d.lap, d.start_s, d.base), (4.0, 0.0, BASE));
    let l = &d.line;
    assert_eq!((l.n, l.step), (4, 1.0));
    assert_eq!(l.x, [0.0, 1.0, 2.0, -1.5], "centimetres");
    assert_eq!(l.y, [50.0, 50.25, 49.5, 50.0], "y0 + centimetres");
    assert_eq!(l.bank, [0.0, 0.03, -0.03, 0.0], "1/10000");
    assert_eq!(l.hw, [6.0, 6.0, 6.5, 6.0]);
    assert_eq!(l.wall_l, [10.0, 12.0, 13.0, 10.0], "decimetres");
    assert_eq!(l.run_l, [0.0, 0.05, -0.02, 0.0], "1/1000");
    assert_eq!(d.pit_lane.len(), 2);
    assert_eq!((d.pit_lane[1].x, d.pit_lane[1].z), (3.0, 4.0), "decimetres");
    assert_eq!(d.walls.len(), 2);
    assert_eq!(d.walls[1].len(), 1);
    assert_eq!(d.buildings[0].name, "pits");
    assert_eq!(d.buildings[1].pts.len(), 1);
    assert_eq!(d.bridges[0].name, "footway");
    assert!(d.water.is_empty());
    assert_eq!(d.photo.file, "photo.jpg");
    assert_eq!((d.photo.x1, d.photo.w), (100.0, 512.0));
    assert_eq!(d.color_fine.ch, 3);
    // Planar 5-bit channels, interleaved.
    let c: Vec<f64> = d.color_fine.values.iter().map(|&v| v as f64).collect();
    let want = [0.0, 10.0 / 31.0, 31.0 / 31.0, 1.0, 20.0 / 31.0, 0.0];
    for (a, b) in c.iter().zip(want) {
        assert_eq!(*a, f64::from(b as f32));
    }
}

/// Column sums first, then row sums: deltas `[2, 4, -6; 1, 1, 1]` give
/// columns `[2, 4, -6; 3, 5, -5]`, then rows `[2, 6, 0; 3, 8, 3]`.
#[test]
fn heights_undo_the_two_dimensional_deltas() {
    let d = SeasideData::parse(&file(&sections())).unwrap();
    let g = &d.height_fine;
    assert_eq!((g.w, g.h, g.ch), (3, 2, 1));
    let want: Vec<f32> = [2, 6, 0, 3, 8, 3]
        .iter()
        .map(|&a| (BASE + a as f64 * 0.5) as f32)
        .collect();
    assert_eq!(g.values, want);
}

#[test]
fn height_sums_wrap_as_int16() {
    let mut s = sections();
    let k = s.iter().position(|x| x.name == "HEIGHT_FINE").unwrap();
    s.splice(
        k - 1..k + 2,
        grid(
            "HEIGHT_FINE",
            "height",
            [0.0, 0.0, 1.0, 2.0, 1.0, 1.0],
            &heights(&[i16::MAX, 1]),
        ),
    );
    let d = SeasideData::parse(&file(&s)).unwrap();
    assert_eq!(
        d.height_fine.values,
        [
            (BASE + i16::MAX as f64) as f32,
            (BASE + i16::MIN as f64) as f32
        ]
    );
}

#[test]
fn sampling_is_exact_at_nodes_bilinear_between_and_clamped_outside() {
    let d = SeasideData::parse(&file(&sections())).unwrap();
    let g = &d.height_fine; // 3×2, step 100, values 11 13 10 / 11.5 14 11.5
    let mut out = [0.0];
    let inside = g.sample(100.0, 0.0, &mut out);
    assert_eq!(out[0], 13.0);
    assert_eq!(inside, 0.0, "on the edge");
    g.sample(50.0, 50.0, &mut out);
    assert_eq!(out[0], (11.0 + 13.0 + 11.5 + 14.0) / 4.0);
    // Far outside: clamped to the nearest edge, and the distance negative.
    let inside = g.sample(-500.0, 50.0, &mut out);
    assert_eq!(inside, -500.0);
    assert!((out[0] - (11.0 + 11.5) / 2.0).abs() < 1e-9, "{}", out[0]);
    let inside = g.sample(1e9, 1e9, &mut out);
    assert!(inside < 0.0 && out[0].is_finite());
    // NaN in, no panic.
    let _ = g.sample(f64::NAN, f64::NAN, &mut out);
}

#[test]
fn a_fine_grid_blends_into_the_wide_one_over_its_last_120_m() {
    let d = SeasideData::parse(&file(&sections())).unwrap();
    let (fine, wide) = (1.0, f64::from((51.0 / 255.0) as f32));
    // The fine trees grid spans 0..400 m: its centre is 200 m inside.
    assert_eq!(d.trees(200.0, 200.0), fine);
    assert_eq!(d.trees(-50.0, 200.0), wide, "outside the fine grid");
    assert_eq!(d.trees(0.0, 200.0), wide, "on its edge");
    // 60 m inside: half way, smoothstep(0.5) = 0.5.
    assert!((d.trees(60.0, 200.0) - (wide + fine) / 2.0).abs() < 1e-7);
    // Monotone from the edge in.
    let mut last = wide;
    for x in 0..=130 {
        let v = d.trees(x as f64, 200.0);
        assert!(v >= last - 1e-12, "at {x} m");
        last = v;
    }
    assert_eq!(last, fine);
}

#[test]
fn ground_is_loose_beyond_the_loose_grid() {
    let d = SeasideData::parse(&file(&sections())).unwrap();
    assert_eq!(d.loose(0.0, 0.0), 0.0);
    // The far edge samples just inside the last cell (`w - 1.001`), as
    // load.js does.
    assert!((d.loose(1.0, 0.0) - 0.999).abs() < 1e-12);
    assert_eq!(d.loose(0.5, 0.0), 0.5);
    assert_eq!(d.loose(-0.1, 0.5), 1.0, "outside");
    assert_eq!(d.loose(0.5, 1.5), 1.0, "outside");
}

#[test]
fn every_truncation_is_refused() {
    let b = file(&sections());
    for n in 0..b.len() {
        assert!(parse_no_panic(&b[..n]).is_err(), "prefix of {n}");
    }
}

#[test]
fn every_missing_section_is_named() {
    let all = sections();
    for k in 0..all.len() {
        let mut s = all.clone();
        let gone = s.remove(k);
        let e = parse_no_panic(&file(&s)).expect_err(&gone.name);
        assert!(e.contains(&gone.name), "{}: {e}", gone.name);
    }
}

#[test]
fn a_section_of_the_wrong_type_is_refused() {
    for (name, sec) in [
        ("line.x", f64s("line.x", &[0.0; 4])),
        ("lap", i32s("lap", &[4])),
        ("lap", f64s("lap", &[])),
        ("walls", i32s("walls", &[0, 0])),
        ("photo.file", bytes("photo.file", b"photo.jpg")),
        ("HEIGHT_FINE.data", strs("HEIGHT_FINE.data", &["x"])),
        ("HEIGHT_FINE.kind", strs("HEIGHT_FINE.kind", &["lidar"])),
        ("HEIGHT_FINE.kind", strs("HEIGHT_FINE.kind", &[])),
        (
            "HEIGHT_FINE",
            f64s("HEIGHT_FINE", &[0.0, 0.0, 100.0, 3.0, 2.0]),
        ),
        ("buildings.name", lists("buildings.name", &[])),
    ] {
        assert!(parse_no_panic(&with(name, sec)).is_err(), "{name}");
    }
}

#[test]
fn a_damaged_container_is_refused() {
    let good = file(&sections());
    let mut b = good.clone();
    b[0] = b'X';
    assert!(SeasideData::parse(&b).unwrap_err().contains("not a survey"));
    let mut b = good.clone();
    b[8] = 2;
    assert!(SeasideData::parse(&b).unwrap_err().contains("version 2"));
    // An unknown section type.
    let mut s = sections();
    s[0].kind = 9;
    assert!(
        SeasideData::parse(&file(&s))
            .unwrap_err()
            .contains("unknown section type 9")
    );
    // A section claiming more than the file holds.
    let mut s = sections();
    s.last_mut().unwrap().n = u32::MAX;
    assert!(parse_no_panic(&file(&s)).is_err());
    // A name that is not UTF-8.
    let mut s = sections();
    s[0].name = String::from_utf8_lossy(&[0xff]).into_owned();
    let mut b = file(&s);
    let at = b.iter().position(|&x| x == 0xef).unwrap();
    b[at] = 0xff;
    assert!(parse_no_panic(&b).is_err());
}

#[test]
fn bad_grid_data_is_refused() {
    // Not zlib.
    assert!(
        parse_no_panic(&with(
            "HEIGHT_FINE.data",
            bytes("HEIGHT_FINE.data", b"not zlib")
        ))
        .is_err()
    );
    // Too few heights.
    let short = zlib(&heights(&[1, 2, 3]));
    assert!(
        parse_no_panic(&with("HEIGHT_FINE.data", bytes("HEIGHT_FINE.data", &short)))
            .unwrap_err()
            .contains("too short")
    );
}

/// Colour and cover grids check their inflated length, as heights do.
#[test]
fn short_colour_and_cover_grids_are_refused() {
    let short = zlib(&[1, 2, 3]);
    for name in ["COLOR_FINE.data", "TREES_FINE.data", "LOOSE.data"] {
        assert!(
            parse_no_panic(&with(name, bytes(name, &short))).is_err(),
            "{name}"
        );
    }
}

/// `origin` and `photo` need three and six numbers.
#[test]
fn short_fixed_size_sections_are_refused() {
    assert!(parse_no_panic(&with("origin", f64s("origin", &[1.0, 2.0]))).is_err());
    assert!(parse_no_panic(&with("photo", f64s("photo", &[0.0; 5]))).is_err());
}

/// A strings or lists section whose count is absurd: refused when the file
/// runs out, without first reserving room for every element (about 100 GB,
/// an allocation failure that would abort the process).
#[test]
fn a_huge_element_count_is_refused_without_reserving_it() {
    let mut s = sections();
    let k = s.iter().position(|x| x.name == "walls").unwrap();
    s[k].n = u32::MAX;
    assert!(parse_no_panic(&file(&s)).is_err());
}
