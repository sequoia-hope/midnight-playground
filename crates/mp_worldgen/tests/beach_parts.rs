//! WP 7.1: `beach/parts.js` against the game's own (`tools/parity/
//! beach-parts.mjs`): each building drawn into a ColorBuilder with Beach's
//! palette, in a placed frame, from a seeded stream; the buckets, every
//! attribute of the merged pieces, what the function returns and the
//! stream's next draw must be the same bits. The golden is compiled in, so
//! this runs in CI and in wasm.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::wasm_bindgen_test as test;

use mp_math::Mulberry32;
use mp_scene::digest::sha256_hex;
use mp_worldgen::beach::atlas::Rect;
use mp_worldgen::beach::parts::{self as p, GasOpts, HotelOpts, HouseOpts, MotelOpts, ShopOpts};
use mp_worldgen::builder::Builder;
use mp_worldgen::three_geom::{BufferGeometry, merge_geometries};
use serde_json::Value;

const PALETTE: &[(&str, u32)] = &[
    ("wPink", 0xe9aea8),
    ("wMint", 0xa9d8c1),
    ("wYellow", 0xf1d690),
    ("wBlue", 0x9fc3de),
    ("wWhite", 0xefebe1),
    ("wPeach", 0xf1bf95),
    ("wLilac", 0xc6b6de),
    ("wTeal", 0x6db8b1),
    ("wSand", 0xdcc9a2),
    ("trim", 0xf6f3ea),
    ("concrete", 0xc4bdb0),
    ("white", 0xf2f2ee),
    ("wood", 0x9c7651),
    ("woodDark", 0x5a3f2b),
    ("roofTar", 0x55524d),
    ("roofTile", 0xbf6a4c),
    ("black", 0x1b1b1d),
    ("boardB", 0xff8a3c),
    ("paintRed", 0xc83a34),
    ("lawn", 0x6f9a45),
    ("hedge", 0x3f6a32),
    ("sailBlue", 0x2b4f86),
    ("hullWhite", 0xf1f2ee),
];

const RECT: Rect = Rect {
    u0: 0.1,
    u1: 0.35,
    v0: 0.6,
    v1: 0.72,
    aspect: 4.0,
};
const RECT2: Rect = Rect {
    u0: 0.5,
    u1: 0.55,
    v0: 0.2,
    v1: 0.3,
    aspect: 0.5,
};

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

/// What a case returned, as the golden writes it.
enum Ret {
    None,
    Num(&'static str, f64),
    House(p::House),
}

fn draw(name: &str, b: &mut Builder, r: &mut Mulberry32) -> Ret {
    let base = name.split('-').next().expect("a name");
    match name {
        "shop" => Ret::Num("H", p::shop(b, r, ShopOpts::default())),
        "shop-rect-blade" => Ret::Num(
            "H",
            p::shop(
                b,
                r,
                ShopOpts {
                    w: 11.5,
                    d: 14.0,
                    rect: Some(RECT),
                    blade: Some(RECT2),
                    ..ShopOpts::default()
                },
            ),
        ),
        "shop-two-story" => Ret::Num(
            "H",
            p::shop(
                b,
                r,
                ShopOpts {
                    two_story: Some(true),
                    rect: Some(RECT),
                    blade: Some(RECT2),
                    ..ShopOpts::default()
                },
            ),
        ),
        "shop-wall" => Ret::Num(
            "H",
            p::shop(
                b,
                r,
                ShopOpts {
                    two_story: Some(true),
                    wall: Some("wLilac"),
                    ..ShopOpts::default()
                },
            ),
        ),
        "surfShop" => {
            p::surf_shop(b, r, &RECT);
            Ret::None
        }
        "diner" => {
            p::diner(b, r, &RECT);
            Ret::None
        }
        "tacoStand" => {
            p::taco_stand(b, r, Some(&RECT));
            Ret::None
        }
        "tacoStand-plain" => {
            p::taco_stand(b, r, None);
            Ret::None
        }
        "picnicTable" => {
            p::picnic_table(b, 1.5, -2.0);
            Ret::None
        }
        "motel" => Ret::Num(
            "d",
            p::motel(
                b,
                r,
                MotelOpts {
                    w: 42.0,
                    sign_rect: Some(RECT),
                    neon_rect: Some(RECT2),
                    wall: None,
                },
            ),
        ),
        "motel-default" => Ret::Num("d", p::motel(b, r, MotelOpts::default())),
        "gasStation" => {
            p::gas_station(
                b,
                r,
                GasOpts {
                    sign_rect: Some(RECT),
                    price_rect: Some(RECT2),
                },
            );
            Ret::None
        }
        "gasStation-plain" => {
            p::gas_station(b, r, GasOpts::default());
            Ret::None
        }
        "beachHouse-opts" => Ret::House(p::beach_house(
            b,
            r,
            HouseOpts {
                w: Some(10.5),
                d: Some(12.0),
                floors: Some(3.0),
                ..HouseOpts::default()
            },
        )),
        "beachHouse-tile" => Ret::House(p::beach_house(
            b,
            r,
            HouseOpts {
                w: Some(11.0),
                d: Some(11.0),
                floors: Some(2.0),
                tile: Some(true),
                ..HouseOpts::default()
            },
        )),
        _ if base == "beachHouse" => Ret::House(p::beach_house(b, r, HouseOpts::default())),
        "hotel" => {
            p::hotel(
                b,
                r,
                HotelOpts {
                    w: 36.0,
                    rect: Some(RECT),
                    ..HotelOpts::default()
                },
            );
            Ret::None
        }
        "hotel-default" => {
            p::hotel(b, r, HotelOpts::default());
            Ret::None
        }
        "bench" => {
            p::bench(b, 0.5, 1.0, 0.3);
            Ret::None
        }
        "promLamp" => {
            p::prom_lamp(b, -1.0, 2.0);
            Ret::None
        }
        "streetLight" => {
            p::street_light(b, 0.0, 0.0, 0.0, 3.4, 8.6);
            Ret::None
        }
        "streetLight-tall" => {
            p::street_light(b, 1.0, -2.0, 0.4, 3.2, 9.5);
            Ret::None
        }
        "trashCan" => {
            p::trash_can(b, 1.6, 0.2);
            Ret::None
        }
        "lifeguardTower" => {
            p::lifeguard_tower(b, r, Some(&RECT2));
            Ret::None
        }
        "umbrellaSet-open" => {
            p::umbrella_set(b, r, "awnBlue", Some(true));
            Ret::None
        }
        _ if base == "umbrellaSet" => {
            p::umbrella_set(b, r, "awnRed", None);
            Ret::None
        }
        "volleyballNet" => {
            p::volleyball_net(b);
            Ret::None
        }
        "surfboardsInSand" => {
            p::surfboards_in_sand(b, r, 4.0);
            Ret::None
        }
        _ if base == "sailboat" => Ret::Num("", p::sailboat(b, r)),
        _ if base == "motorboat" => {
            p::motorboat(b, r);
            Ret::None
        }
        _ => panic!("no case {name}"),
    }
}

fn ret_json(r: Ret) -> Value {
    match r {
        Ret::None => Value::Null,
        Ret::Num("", v) => Value::from(bits(v)),
        Ret::Num(k, v) => serde_json::json!({ k: bits(v) }),
        Ret::House(h) => serde_json::json!({ "w": bits(h.w), "d": bits(h.d), "H": bits(h.h) }),
    }
}

fn attrs(g: &BufferGeometry) -> Vec<String> {
    g.attributes
        .iter()
        .map(|(n, a)| format!("{n}={}", sha256_hex(&a.array.to_le_bytes())))
        .collect()
}

#[test]
fn beach_parts_are_the_games() {
    let golden: Value =
        serde_json::from_str(include_str!("../../../parity/golden/beach/parts.json"))
            .expect("golden parses");
    let mut problems = Vec::new();
    let cases = golden["cases"].as_array().expect("cases");
    for c in cases {
        let name = c["name"].as_str().expect("name");
        let seed = c["seed"].as_u64().expect("seed") as u32;
        let mut b = Builder::new_color(PALETTE);
        b.set_frame(12.5, 1.25, -7.75, 0.7);
        let mut rng = Mulberry32::new(seed);
        let value = ret_json(draw(name, &mut b, &mut rng));
        if value != c["value"] {
            problems.push(format!("{name}: returned {value}, the JS {}", c["value"]));
        }
        let next = bits(rng.next_f64());
        if next != c["next"].as_str().expect("next") {
            problems.push(format!("{name}: a different number of draws"));
        }
        let want = c["buckets"].as_array().expect("buckets");
        if want.len() != b.buckets.len() {
            problems.push(format!(
                "{name}: {} buckets, the JS {}",
                b.buckets.len(),
                want.len()
            ));
        }
        for ((key, geos), w) in b.buckets.iter().zip(want) {
            if key != w["key"].as_str().expect("key") || geos.len() as u64 != w["pieces"] {
                problems.push(format!(
                    "{name}: bucket {key} × {}, the JS {} × {}",
                    geos.len(),
                    w["key"],
                    w["pieces"]
                ));
                continue;
            }
            let refs: Vec<&BufferGeometry> = geos.iter().collect();
            let g = merge_geometries(&refs, false).expect("the pieces merge");
            let ours = attrs(&g);
            let theirs: Vec<String> = w["attrs"]
                .as_array()
                .expect("attrs")
                .iter()
                .map(|a| a.as_str().expect("attr").to_string())
                .collect();
            for (o, t) in ours.iter().zip(&theirs) {
                if o != t {
                    problems.push(format!("{name}: {key}: {o} (the JS {t})"));
                }
            }
            if ours.len() != theirs.len() {
                problems.push(format!("{name}: {key}: attributes differ"));
            }
        }
    }
    println!("{} cases", cases.len());
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
