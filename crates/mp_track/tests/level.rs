//! The level data types and the road-type table: the segment builders, the
//! route accessors, and `ROAD_TYPES` as tracks index it (the stored index is
//! the position in the list, so the order is part of the format).

use std::sync::Arc;

use mp_track::*;

#[test]
fn road_index_is_the_position_in_the_table_and_minus_one_when_missing() {
    for (i, rt) in ROAD_TYPES.iter().enumerate() {
        assert_eq!(road_index(rt.key), i as i32, "{}", rt.key);
        assert_eq!(road_type(rt.key), Some(rt));
    }
    assert_eq!(road_index("nope"), -1);
    assert_eq!(road_index(""), -1);
    assert_eq!(road_index("Mountain"), -1, "keys are case-sensitive");
    assert!(road_type("nope").is_none());
    // -1 stored into a Uint8Array is 255: off the end of the table.
    assert_eq!(road_index("nope") as u8, 255);
    assert!(ROAD_TYPES.len() < 255);
}

#[test]
fn road_type_order_is_the_stored_format() {
    // New types go on the end; reordering would change every saved track.
    let keys: Vec<_> = ROAD_TYPES.iter().map(|r| r.key).collect();
    assert_eq!(
        keys,
        [
            "mountain",
            "valley",
            "freeway",
            "coastal",
            "boulevard",
            "street",
            "desert",
            "playa",
            "circuit",
            "circuitWide",
        ]
    );
}

#[test]
fn road_types_are_well_formed() {
    let edges = [
        "terrain", "fence", "jersey", "rail", "curb", "circuit", "none",
    ];
    for rt in &ROAD_TYPES {
        assert!(rt.hw > 0.0 && rt.margin > 0.0, "{}", rt.key);
        assert!(rt.lanes == 2 || rt.lanes == 4, "{}", rt.key);
        assert!(edges.contains(&rt.edge), "{}: edge {}", rt.key, rt.edge);
        assert!(rt.tone <= 2, "{}", rt.key);
        // Run-off belongs to circuits, and only they have it.
        assert_eq!(rt.runoff, rt.edge == "circuit", "{}", rt.key);
    }
    assert_eq!(
        road_type("street").unwrap().bank,
        Some(0.0),
        "streets stay level"
    );
    assert!(
        ROAD_TYPES
            .iter()
            .filter(|r| r.key != "street")
            .all(|r| r.bank.is_none()),
        "every other type takes the default banking"
    );
    assert_eq!(road_type("circuitWide").unwrap().hw, 7.5);
}

#[test]
fn seg_builders_set_one_extra_each() {
    let s = seg(120.0, -35.0, 4.0);
    assert_eq!((s.len, s.turn, s.rise), (120.0, -35.0, 4.0));
    assert_eq!(s.extra, SegExtra::default());
    let s = seg(10.0, 0.0, 0.0)
        .zone(2)
        .road("freeway")
        .tag("tunnel")
        .elevated()
        .cross(-3, 4);
    assert_eq!(
        s.extra,
        SegExtra {
            zone: Some(2),
            road: Some("freeway"),
            tag: Some("tunnel"),
            elevated: true,
            cross: Some((-3, 4)),
        }
    );
    // Later calls overwrite.
    assert_eq!(seg(1.0, 0.0, 0.0).zone(1).zone(3).extra.zone, Some(3));
}

fn level(route: Route) -> Level {
    Level {
        id: "t",
        mode: Mode::Cruise,
        num: "",
        title: "",
        desc: "",
        laps: None,
        lap_length: None,
        start_height: None,
        start_heading: None,
        start_x: None,
        start_z: None,
        finish_runoff: None,
        elevation_smooth: None,
        route,
        elevation: None,
        ground: None,
        loose_ground: None,
        sea_y: None,
        zones: Vec::new(),
        sky: Vec::new(),
        sun_azimuth: 0.0,
        moon_dir: None,
        traffic_paint: None,
        traffic: Vec::new(),
        police: None,
        rivals: Vec::new(),
    }
}

#[test]
fn route_accessors_pick_the_matching_kind() {
    let segs = level(Route::Segments(vec![seg(5.0, 0.0, 0.0)]));
    assert!(!segs.is_loop());
    assert_eq!(segs.segments().map(<[_]>::len), Some(1));
    assert!(segs.loop_spec().is_none());

    let spec = LoopSpec {
        path: Arc::new(|| Ok(LoopPath::default())),
        base_y: Some(3.0),
        tags: Vec::new(),
        road: Some("circuit"),
        roads: Vec::new(),
        start_s: None,
        elevation_smooth: None,
        zones: None,
    };
    let lp = level(Route::Loop(spec));
    assert!(lp.is_loop());
    assert!(lp.segments().is_none());
    let l = lp.loop_spec().unwrap();
    assert_eq!((l.base_y, l.road), (Some(3.0), Some("circuit")));
    assert_eq!((l.path)().unwrap(), LoopPath::default());
    // Debug skips the closures but names the level.
    let dbg = format!("{lp:?}");
    assert!(dbg.contains("\"t\"") && dbg.contains("LoopSpec"), "{dbg}");
}

#[test]
fn zone_builders_set_their_blend() {
    let z = Zone {
        key: "k",
        name: "n",
        sub: "",
        landform: "",
        scenery: "",
        color: "",
        blend: None,
        blend_offset: None,
    };
    let b = z.clone().blend(120.0).blend_offset(-30.0);
    assert_eq!((b.blend, b.blend_offset), (Some(120.0), Some(-30.0)));
    assert_eq!(z.blend, None);
}
