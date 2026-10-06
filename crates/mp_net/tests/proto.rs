//! The wire format from outside (SPEC 9.3): every message at its edges
//! round-trips, the tag bytes and layout are pinned (a change needs a
//! VERSION bump), decoding is canonical (whatever decodes re-encodes to the
//! same bytes) and nothing malformed panics or allocates on trust.

use mp_math::Mulberry32;
use mp_net::proto::{
    AiFill, DecodeError, GridRule, MAX_PLAYERS, Msg, PlayerInfo, PointsRow, RaceStart, Settings,
    VERSION,
};
use mp_sim::input::InputFrame;

fn player(slot: u8, name: &str) -> PlayerInfo {
    PlayerInfo {
        slot,
        name: name.into(),
        car: "electric".into(),
        color: u32::MAX,
        ready: slot.is_multiple_of(2),
        connected: !slot.is_multiple_of(3),
    }
}

fn frame(k: u32) -> InputFrame {
    InputFrame {
        steer: (k as i32 * 7919 % 65535 - 32767) as i16,
        throttle: (k * 31) as u8,
        brake: (k * 17) as u8,
        flags: (k * 5) as u8,
    }
}

/// Messages at the edges of what each field holds.
fn edge_cases() -> Vec<Msg> {
    let long = "x".repeat(255);
    let unicode = "Zoë 🏁 ドライバー";
    let full: Vec<PlayerInfo> = (0..MAX_PLAYERS as u8).map(|s| player(s, unicode)).collect();
    let all_settings = [
        (AiFill::None, GridRule::Random),
        (AiFill::To6, GridRule::Reverse),
        (AiFill::To8, GridRule::Same),
    ]
    .map(|(ai, grid)| Settings {
        level: "streets".into(),
        ai,
        rubber_band: false,
        ghost: true,
        grid,
        races: u8::MAX,
    });
    let mut v = vec![
        Msg::Hello {
            version: u16::MAX,
            name: String::new(),
            car: String::new(),
            color: 0,
        },
        Msg::Hello {
            version: 0,
            name: long.clone(),
            car: unicode.into(),
            color: u32::MAX,
        },
        Msg::SetMe {
            name: unicode.into(),
            car: long.clone(),
            color: 0x80000000,
        },
        Msg::Ready(false),
        Msg::Input {
            first: 0,
            frames: vec![],
        },
        Msg::Input {
            first: u32::MAX,
            frames: (0..u16::MAX as u32).map(frame).collect(),
        },
        Msg::Ping {
            id: u32::MAX,
            t: 0.0,
        },
        Msg::Ping { id: 0, t: -0.0 },
        Msg::Ping { id: 1, t: f64::MAX },
        Msg::Go(false),
        Msg::Welcome { slot: u8::MAX },
        Msg::Reject {
            reason: String::new(),
        },
        Msg::Lobby {
            settings: Settings::default(),
            players: vec![],
            points: vec![],
            raced: 0,
            racing: false,
        },
        Msg::Lobby {
            settings: all_settings[2].clone(),
            players: full.clone(),
            points: vec![
                PointsRow {
                    slot: Some(0),
                    name: long.clone(),
                    points: u32::MAX,
                    moved: i8::MIN,
                },
                PointsRow {
                    slot: Some(254),
                    name: unicode.into(),
                    points: 0,
                    moved: i8::MAX,
                },
                PointsRow {
                    slot: None,
                    name: String::new(),
                    points: 1,
                    moved: 0,
                },
            ],
            raced: u32::MAX,
            racing: true,
        },
        Msg::Start(RaceStart {
            race: u32::MAX,
            settings: all_settings[0].clone(),
            seed: 0,
            humans: full.clone(),
            grid: (0..MAX_PLAYERS as u8).rev().collect(),
        }),
        Msg::Start(RaceStart {
            race: 0,
            settings: all_settings[1].clone(),
            seed: u32::MAX,
            humans: vec![],
            grid: vec![],
        }),
        Msg::Inputs {
            first: u32::MAX,
            humans: MAX_PLAYERS as u8,
            frames: (0..MAX_PLAYERS as u32 * 600).map(frame).collect(),
        },
        Msg::Inputs {
            first: 1,
            humans: 3,
            frames: vec![],
        },
        Msg::Hash {
            tick: u32::MAX,
            hash: 0,
        },
        Msg::Pong {
            id: 0,
            t: f64::MIN_POSITIVE,
            host_ms: -1e300,
        },
        Msg::Begin { at: f64::INFINITY },
        Msg::Begin {
            at: f64::NEG_INFINITY,
        },
    ];
    v.extend(all_settings.into_iter().map(Msg::Configure));
    v
}

#[test]
fn every_message_round_trips_at_its_edges() {
    for m in edge_cases() {
        let b = m.encode();
        let back = Msg::decode(&b).unwrap_or_else(|e| panic!("{e:?}: {m:?}"));
        assert_eq!(back, m);
        assert_eq!(back.encode(), b, "re-encodes to the same bytes");
    }
}

#[test]
fn floats_keep_their_bits_nan_and_negative_zero_included() {
    for x in [f64::NAN, -f64::NAN, -0.0, 5e-324, 1234.5678] {
        let b = Msg::Begin { at: x }.encode();
        let Ok(Msg::Begin { at }) = Msg::decode(&b) else {
            panic!()
        };
        assert_eq!(at.to_bits(), x.to_bits());
    }
}

#[test]
fn strings_longer_than_255_bytes_are_cut_on_a_char_boundary() {
    for (s, want) in [
        ("a".repeat(300), 255),
        // 3-byte chars: 85 fit in 255 bytes exactly.
        ("ド".repeat(100), 255),
        // 4-byte chars: 63 fit (252 bytes), the 64th would cross 255.
        ("🏁".repeat(100), 252),
        // 2-byte chars after one ASCII byte: 1 + 2 * 127 = 255.
        (format!("x{}", "é".repeat(200)), 255),
    ] {
        let m = Msg::Reject { reason: s.clone() };
        let Ok(Msg::Reject { reason }) = Msg::decode(&m.encode()) else {
            panic!()
        };
        assert_eq!(reason.len(), want);
        assert!(s.starts_with(&reason));
    }
}

/// The bytes on the wire, pinned: if one of these changes, so must
/// [`VERSION`].
#[test]
fn the_wire_format_is_pinned() {
    assert_eq!(VERSION, 1);
    let cases: Vec<(Msg, Vec<u8>)> = vec![
        (
            Msg::Hello {
                version: 1,
                name: "Ab".into(),
                car: "c".into(),
                color: 0x01020304,
            },
            vec![1, 1, 0, 2, b'A', b'b', 1, b'c', 4, 3, 2, 1],
        ),
        (
            Msg::SetMe {
                name: "".into(),
                car: "x".into(),
                color: 5,
            },
            vec![2, 0, 1, b'x', 5, 0, 0, 0],
        ),
        (Msg::Ready(true), vec![3, 1]),
        (
            Msg::Input {
                first: 258,
                frames: vec![InputFrame {
                    steer: -2,
                    throttle: 3,
                    brake: 4,
                    flags: 5,
                }],
            },
            vec![4, 2, 1, 0, 0, 1, 0, 0xfe, 0xff, 3, 4, 5],
        ),
        (
            Msg::Ping { id: 1, t: 1.0 },
            [vec![5, 1, 0, 0, 0], 1.0f64.to_le_bytes().to_vec()].concat(),
        ),
        (Msg::Leave, vec![6]),
        (
            Msg::Configure(Settings {
                level: "co".into(),
                ai: AiFill::To8,
                rubber_band: true,
                ghost: false,
                grid: GridRule::Same,
                races: 6,
            }),
            vec![7, 2, b'c', b'o', 2, 1, 0, 2, 6],
        ),
        (Msg::Go(false), vec![8, 0]),
        (Msg::Loaded, vec![9]),
        (Msg::Welcome { slot: 7 }, vec![20, 7]),
        (
            Msg::Reject {
                reason: "no".into(),
            },
            vec![21, 2, b'n', b'o'],
        ),
        (
            Msg::Lobby {
                settings: Settings {
                    level: "".into(),
                    ai: AiFill::None,
                    rubber_band: false,
                    ghost: true,
                    grid: GridRule::Random,
                    races: 0,
                },
                players: vec![PlayerInfo {
                    slot: 1,
                    name: "P".into(),
                    car: "".into(),
                    color: 2,
                    ready: true,
                    connected: false,
                }],
                points: vec![PointsRow {
                    slot: None,
                    name: "R".into(),
                    points: 3,
                    moved: -1,
                }],
                raced: 4,
                racing: true,
            },
            vec![
                22, 0, 0, 0, 1, 0, 0, // settings
                1, 1, 1, b'P', 0, 2, 0, 0, 0, 1, 0, // one player
                1, 255, 1, b'R', 3, 0, 0, 0, 0xff, // one points row
                4, 0, 0, 0, 1,
            ],
        ),
        (
            Msg::Start(RaceStart {
                race: 1,
                settings: Settings {
                    level: "".into(),
                    ai: AiFill::To6,
                    rubber_band: true,
                    ghost: true,
                    grid: GridRule::Reverse,
                    races: 3,
                },
                seed: 0xaabbccdd,
                humans: vec![],
                grid: vec![2, 0],
            }),
            vec![
                23, 1, 0, 0, 0, 0, 1, 1, 1, 1, 3, 0xdd, 0xcc, 0xbb, 0xaa, 0, 2, 2, 0,
            ],
        ),
        (
            Msg::Inputs {
                first: 1,
                humans: 1,
                frames: vec![InputFrame::default()],
            },
            vec![24, 1, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0],
        ),
        (
            Msg::Hash { tick: 30, hash: 1 },
            vec![25, 30, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
        ),
        (
            Msg::Pong {
                id: 2,
                t: 0.0,
                host_ms: 0.0,
            },
            [vec![26, 2, 0, 0, 0], vec![0; 16]].concat(),
        ),
        (Msg::End, vec![27]),
        (
            Msg::Begin { at: 2.0 },
            [vec![28], 2.0f64.to_le_bytes().to_vec()].concat(),
        ),
    ];
    for (m, bytes) in cases {
        assert_eq!(m.encode(), bytes, "{m:?}");
        assert_eq!(Msg::decode(&bytes), Ok(m));
    }
}

#[test]
fn malformed_fields_are_named_errors() {
    let e = |b: &[u8]| Msg::decode(b).unwrap_err();
    assert_eq!(e(&[]), DecodeError("message too short"));
    assert_eq!(e(&[0]), DecodeError("unknown message"));
    assert_eq!(e(&[10]), DecodeError("unknown message"));
    assert_eq!(e(&[29]), DecodeError("unknown message"));
    assert_eq!(e(&[255]), DecodeError("unknown message"));
    assert_eq!(e(&[3, 2]), DecodeError("bad bool"));
    assert_eq!(e(&[21, 2, 0xc3, 0x28]), DecodeError("bad utf-8"));
    assert_eq!(e(&[21, 1, 0xff]), DecodeError("bad utf-8"));
    assert_eq!(e(&[7, 0, 3, 0, 0, 0, 0]), DecodeError("bad ai fill"));
    assert_eq!(e(&[7, 0, 0, 0, 0, 3, 0]), DecodeError("bad grid rule"));
    assert_eq!(e(&[6, 0]), DecodeError("trailing bytes"));
    // Inputs must be whole ticks of `humans` frames, and humans > 0.
    let inputs = |humans: u8, n: u16| {
        let mut b = vec![24, 1, 0, 0, 0, humans];
        b.extend_from_slice(&n.to_le_bytes());
        b.extend(std::iter::repeat_n(0, n as usize * 5));
        b
    };
    let whole = DecodeError("inputs not a whole number of ticks");
    assert_eq!(e(&inputs(0, 0)), whole);
    assert_eq!(e(&inputs(0, 2)), whole);
    assert_eq!(e(&inputs(2, 3)), whole);
    assert!(Msg::decode(&inputs(2, 4)).is_ok());
    assert!(Msg::decode(&inputs(3, 0)).is_ok());
}

#[test]
fn a_forged_count_is_an_error_not_an_allocation() {
    // 65535 frames claimed, none sent.
    assert_eq!(
        Msg::decode(&[4, 0, 0, 0, 0, 0xff, 0xff]),
        Err(DecodeError("message too short"))
    );
    // 255 players claimed in a lobby with one.
    let mut b = Msg::Lobby {
        settings: Settings::default(),
        players: vec![player(0, "a")],
        points: vec![],
        raced: 0,
        racing: false,
    }
    .encode();
    let at = 1 + 1 + "coast".len() + 5;
    assert_eq!(b[at], 1);
    b[at] = 255;
    assert!(Msg::decode(&b).is_err());
}

/// Mutated valid messages (flipped bits, cut, spliced) never panic, and any
/// that still decode re-encode to exactly their bytes: the encoding is
/// canonical, so the host and a client can't read one message two ways.
#[test]
fn mutated_messages_never_panic_and_decoding_is_canonical() {
    let seeds: Vec<Vec<u8>> = edge_cases()
        .into_iter()
        .filter(|m| !matches!(m, Msg::Input { frames, .. } if frames.len() > 100))
        .filter(|m| !matches!(m, Msg::Inputs { frames, .. } if frames.len() > 100))
        .map(|m| m.encode())
        .collect();
    let mut rng = Mulberry32::new(77);
    let mut r = |n: usize| (rng.next_f64() * n as f64) as usize;
    let mut decoded = 0;
    for _ in 0..30_000 {
        let mut b = seeds[r(seeds.len())].clone();
        match r(4) {
            0 => {
                let i = r(b.len());
                b[i] ^= 1 << r(8);
            }
            1 => {
                let i = r(b.len());
                b[i] = r(256) as u8;
            }
            2 => b.truncate(r(b.len() + 1)),
            _ => {
                let other = &seeds[r(seeds.len())];
                let cut = r(b.len() + 1);
                b.truncate(cut);
                b.extend_from_slice(&other[r(other.len())..]);
            }
        }
        if let Ok(m) = Msg::decode(&b) {
            decoded += 1;
            assert_eq!(m.encode(), b, "{m:?}");
        }
    }
    assert!(decoded > 1000, "the mutations reach the decoder: {decoded}");
}

#[test]
fn settings_default_to_a_coast_race_filled_to_six_on_the_reverse_grid() {
    let s = Settings::default();
    assert_eq!(s.level, "coast");
    assert_eq!((s.ai, s.grid, s.races), (AiFill::To6, GridRule::Reverse, 0));
    assert!(s.rubber_band && !s.ghost);
    assert_eq!(
        [AiFill::None, AiFill::To6, AiFill::To8].map(AiFill::field),
        [0, 6, 8]
    );
    assert_eq!(AiFill::To8.field(), MAX_PLAYERS);
}
