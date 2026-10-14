//! A road network (docs/vision/peninsula.md, stage 3): a branch leaves the
//! level's road at a junction, starts on its centreline, shares its surface
//! over the parent's pavement, and the projection picks the road under the
//! car.

use std::sync::Arc;

use mp_track::{
    BRANCH_FIT, Branch, Level, LoopPath, LoopSpec, Mode, Route, Track, Zone, road_type,
};

fn path(pts: &[(f64, f64, f64)]) -> LoopPath {
    LoopPath {
        x: pts.iter().map(|p| p.0).collect(),
        z: pts.iter().map(|p| p.1).collect(),
        y: Some(pts.iter().map(|p| p.2).collect()),
        ..Default::default()
    }
}

fn spec(p: LoopPath, road: &'static str) -> LoopSpec {
    LoopSpec {
        path: Arc::new(move || Ok(p.clone())),
        base_y: None,
        tags: Vec::new(),
        road: Some(road),
        roads: Vec::new(),
        start_s: Some(60.0),
        elevation_smooth: Some(4.0),
        zones: None,
    }
}

/// A straight road east along x at 10 m up, and a branch leaving it at
/// x = 500 to the south (+z), climbing 5 cm a metre, whose own first
/// point sits a little off the parent's centreline.
fn level() -> Level {
    let main: Vec<_> = (0..=20).map(|k| (k as f64 * 50.0, 0.0, 10.0)).collect();
    let branch: Vec<_> = (0..=40)
        .map(|k| (500.0, 1.5 + k as f64 * 10.0, 10.0 + 3.0 + k as f64 * 0.5))
        .collect();
    Level {
        id: "net",
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
        elevation_smooth: Some(4.0),
        route: Route::Path(spec(path(&main), "coastal")),
        elevation: None,
        ground: Some(Arc::new(|_, _| 0.0)),
        loose_ground: None,
        open: true,
        branches: vec![Branch {
            id: "b",
            name: "The branch",
            path: {
                let p = path(&branch);
                Arc::new(move || Ok(p.clone()))
            },
            road: "mountain",
            zone: 0,
            elevation_smooth: None,
        }],
        sea_y: None,
        zones: vec![Zone {
            key: "coast",
            name: "",
            sub: "",
            landform: "coast",
            scenery: "Coast",
            color: "#000000",
            blend: None,
            blend_offset: None,
        }],
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
fn a_branch_starts_on_its_parent_and_shares_its_surface() {
    let t = Track::new(&level()).unwrap();
    assert_eq!(t.branches.len(), 1);
    let b = &t.branches[0];
    assert!((b.s - 500.0).abs() < 1.0, "leaves at {}", b.s);
    assert_eq!(b.side, 1.0, "to the right (south) of a road heading east");
    let bt = &b.track;
    assert!((f64::from(bt.px[0]) - 500.0).abs() < 0.01);
    assert!(f64::from(bt.pz[0]).abs() < 0.01, "starts on the centreline");
    let coastal = road_type("coastal").unwrap();
    assert!((b.under - coastal.hw).abs() < 0.2);
    assert!((b.clear - (coastal.hw + coastal.margin + 1.0)).abs() < 0.2);
    // Over the parent the branch is the parent's surface (plus a hair's
    // lift over the shoulder); beyond the fit it is its own road again.
    let mut s = 0.0;
    while s <= b.clear {
        let parent = t.surface_y(b.s, s);
        let own = bt.surface_y(s, 0.0);
        assert!(
            (own - parent).abs() < 0.06,
            "at {s} m the branch is {own} over the parent's {parent}"
        );
        s += 1.0;
    }
    let far = b.clear + BRANCH_FIT + 40.0;
    let expect = 13.0 + (far - 1.5) * 0.05;
    assert!(
        (bt.surface_y(far, 0.0) - expect).abs() < 0.6,
        "{} against its own {expect}",
        bt.surface_y(far, 0.0)
    );
    // The parent's fence opens at the junction, on the branch's side.
    assert!(
        t.fence_gaps
            .iter()
            .any(|g| g.side == 1.0 && g.s0 < 500.0 && g.s1 > 500.0)
    );
}

#[test]
fn the_projection_picks_the_road_under_the_car() {
    let t = Track::new(&level()).unwrap();
    // Along the branch.
    let (r, p) = t.project_road(500.0, 200.0, 0, 480.0);
    assert_eq!(r, 1);
    assert!((p.s - 200.0).abs() < 1.5 && p.lat.abs() < 0.1, "{p:?}");
    // Along the parent.
    let (r, p) = t.project_road(200.0, 1.0, 1, 100.0);
    assert_eq!(r, 0);
    assert!(
        (p.s - 200.0).abs() < 0.1 && (p.lat - 1.0).abs() < 0.1,
        "{p:?}"
    );
    // In the junction both roads hold the car: the one it was on keeps it.
    assert_eq!(t.project_road(500.0, 3.0, 0, 500.0).0, 0);
    assert_eq!(t.project_road(500.0, 3.0, 1, 3.0).0, 1);
    // Off both roads, the one it is least far off.
    assert_eq!(t.project_road(530.0, 60.0, 0, 500.0).0, 1);
    assert_eq!(t.project_road(300.0, 40.0, 1, 100.0).0, 0);
    // The branch's own zone and road type the whole way.
    let bt = &t.branches[0].track;
    assert!(bt.zone.iter().all(|&z| z == 0));
    assert_eq!(bt.zones.len(), 1);
    assert_eq!(bt.hw[200], road_type("mountain").unwrap().hw as f32);
}
