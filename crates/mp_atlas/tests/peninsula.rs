//! The peninsula's atlas and plan: the real geography the owner described
//! holds in the data, every route resolves along its roads, and
//! `resolved.json` (what the planner page draws) is up to date.

use mp_atlas::profile::Profile;
use mp_atlas::report::{json, measure};
use mp_atlas::{Atlas, Cover};

fn atlas() -> Atlas {
    Atlas::load(&Atlas::dir("peninsula")).expect("the peninsula's atlas loads")
}

#[test]
fn the_origin_is_half_moon_bays_big_intersection() {
    let a = atlas();
    let o = a.place("hmb-1-92").unwrap();
    assert!(o.x.abs() < 1.0 && o.z.abs() < 1.0, "{o:?}");
    let near = |r: &str| {
        a.geo
            .roads
            .iter()
            .filter(|x| x.has_ref(r))
            .map(|x| mp_atlas::geo::nearest_on(o, &x.pts).0)
            .fold(f64::INFINITY, f64::min)
    };
    assert!(near("1") < 30.0 && near("92") < 30.0);
}

#[test]
fn the_first_race_starts_at_the_crest_above_san_gregorio() {
    // "Right at the intersection of 84 and Highway 1 it's almost at sea
    // level. Then it climbs up a hill just a little bit, and there's a
    // crest, and then there's this beautiful view."
    let a = atlas();
    let m = measure(&a).unwrap();
    let coast = m.routes.iter().find(|r| r.id == "coast-south").unwrap();
    let jct = &coast.profile.samples[0];
    assert!(jct.h < 30.0, "the junction is near sea level: {}", jct.h);
    let crest = coast.profile.first_crest_after(0.0).expect("a crest");
    assert!(crest.d < 4000.0, "a short climb: {} m", crest.d);
    assert!(crest.h - jct.h > 50.0, "up to a view: {} m", crest.h);
    let start = a.place("crest-san-gregorio").unwrap();
    assert!(start.dist(crest.at) < 40.0, "the start is the crest");
    // It finishes at the intersection, through farmland.
    let race = m.routes.iter().find(|r| r.id == "first-race").unwrap();
    let end = race.profile.samples.last().unwrap().at;
    assert!(end.dist(a.place("hmb-1-92").unwrap()) < 30.0);
    let crop = race
        .profile
        .cover
        .iter()
        .find(|c| c.0 == Cover::Crop)
        .map_or(0.0, |c| c.1);
    assert!(crop > 0.25, "farms beside the road: {crop}");
}

#[test]
fn the_loop_closes_and_every_route_resolves() {
    let a = atlas();
    let m = measure(&a).unwrap();
    let (_, _, len, closes) = m
        .circuits
        .iter()
        .find(|c| c.0 == "the-loop")
        .cloned()
        .unwrap();
    assert!(closes);
    assert!((60e3..80e3).contains(&len), "1, 92, 35 and 84: {len} m");
    for r in &m.routes {
        for (id, off) in &r.snaps {
            assert!(*off < 150.0, "{}: `{id}` is {off:.0} m off its roads", r.id);
        }
    }
    // Skyline runs through the redwoods, high on the ridge.
    let sky = m.routes.iter().find(|r| r.id == "skyline-south").unwrap();
    assert!(sky.profile.cover[0].0 == Cover::Forest);
    assert!(sky.profile.h_max > 600.0);
}

#[test]
fn tunnels_keep_the_road_level() {
    // Devil's Slide's tunnels run under the ridge: no 40 % grades.
    let a = atlas();
    let m = measure(&a).unwrap();
    let north = m.routes.iter().find(|r| r.id == "coast-north").unwrap();
    assert!(north.profile.max_grade < 0.2, "{}", north.profile.max_grade);
    let none = Profile::of(&north.line, &a.terrain, &[]);
    assert!(none.max_grade > north.profile.max_grade);
}

#[test]
fn resolved_json_is_up_to_date() {
    let dir = Atlas::dir("peninsula");
    let a = atlas();
    // Compared as JSON, not as text: the key order depends on whether the
    // build unifies serde_json's `preserve_order` (a workspace build does).
    let now = serde_json::to_string(&json(&a, &measure(&a).unwrap())).unwrap();
    let now: serde_json::Value = serde_json::from_str(&now).unwrap();
    let file = std::fs::read_to_string(dir.join("resolved.json")).unwrap_or_default();
    let file: serde_json::Value = serde_json::from_str(&file).unwrap_or_default();
    assert!(
        file == now,
        "assets/atlas/peninsula/resolved.json is stale: run `cargo run -p mp_atlas -- resolve`"
    );
}
