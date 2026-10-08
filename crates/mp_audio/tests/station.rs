//! The car radio in `GameAudio` (DECISIONS D1151) on the null backend:
//! tuning makes one radio node into the music bus and drives its params;
//! the playlist stops while a station plays and comes back when the radio
//! is turned off; a platform without AudioWorklet keeps the old music.
//! With `--features native`, a tuned `GameAudio` rendered offline is heard.
//!
//!   cargo test -p mp_audio --test station
//!   cargo test -p mp_audio --features native --test station

use mp_audio::game::{GameAudio, InitOptions, Platform};
use mp_audio::radio::{Bytes, Fetch};
use mp_audio::wa::null::{self, ClipInfo, NullHandle, NullOptions};
use mp_audio::wa::{AudioContext, ContextOptions, ContextState, Pending};
use std::cell::RefCell;
use std::rc::Rc;

/// A wall time: day 20543 since the Unix epoch, 84800 s into it.
const WALL: f64 = 1_775_000_000.0;

/// A DJ directory with one clip, `marisol-ident-3`, in two takes.
struct DjFetch;

impl Fetch for DjFetch {
    fn fetch(&self, file: &str) -> Pending<Bytes> {
        let p = Pending::new();
        let body = match file {
            "index.json" => Some(Rc::new(
                br#"{"clips":{"marisol-ident-3":{"takes":2,"text":"The Tide.","voice":"marisol"}}}"#
                    .to_vec(),
            )),
            "marisol-ident-3.mp3" | "marisol-ident-3.2.mp3" => Some(Rc::new(file.as_bytes().to_vec())),
            _ => None,
        };
        p.resolve(Ok(body));
        p
    }
}

type Handle = Rc<RefCell<Option<(AudioContext, NullHandle)>>>;

/// A `GameAudio` on a logged null context, built, with the music on (as
/// the game has it); the log up to there is returned apart.
fn game(radio_available: bool, dj: bool) -> (GameAudio, Handle, Vec<String>) {
    let handle: Handle = Rc::new(RefCell::new(None));
    let hc = handle.clone();
    let platform = Platform {
        new_context: Some(Box::new(move |opts: &ContextOptions| {
            let (ctx, h) = null::context(
                NullOptions {
                    sample_rate: 48000.0,
                    log: true,
                    state: ContextState::Suspended,
                },
                opts.clone(),
            );
            h.set_radio_available(radio_available);
            // Every DJ clip decodes to a second of mono.
            h.set_decoder(|bytes| {
                Some(ClipInfo {
                    name: String::from_utf8_lossy(bytes).into_owned(),
                    channels: 1,
                    length: 48000,
                })
            });
            *hc.borrow_mut() = Some((ctx.clone(), h));
            Some(ctx)
        })),
        dj: dj.then(|| Rc::new(DjFetch) as Rc<dyn Fetch>),
        ..Platform::headless(1)
    };
    let mut a = GameAudio::new(platform);
    a.init(InitOptions::default());
    a.play_track("neon");
    a.set_music(true);
    assert!(a.ready());
    let init_log = take(&handle);
    (a, handle, init_log)
}

fn take(h: &Handle) -> Vec<String> {
    h.borrow().as_ref().expect("a context").1.take_log()
}

fn ctx(h: &Handle) -> AudioContext {
    h.borrow().as_ref().expect("a context").0.clone()
}

/// The lines of `log` about node `n` (its creation, params and connections).
fn about<'a>(log: &'a [String], n: &str) -> Vec<&'a str> {
    log.iter()
        .map(String::as_str)
        .filter(|l| l.contains(&format!("\"{n}\"")) || l.contains(&format!("\"{n}.")))
        .collect()
}

/// The node a `new` line made (`["new","nX",kind]`).
fn made(line: &str) -> &str {
    line.split('"').nth(3).expect("a node id")
}

/// `music_in`: the high-pass at 36 Hz the music plays into.
fn music_in(init_log: &[String]) -> String {
    let line = init_log
        .iter()
        .find(|l| l.contains(".frequency\",36]"))
        .expect("music_in's cutoff");
    line.split('"')
        .nth(3)
        .unwrap()
        .split('.')
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn tuning_makes_one_node_into_the_music_bus() {
    let (mut a, h, init_log) = game(true, false);
    let min = music_in(&init_log);
    assert!(a.music.as_ref().unwrap().on, "the playlist plays");

    a.set_station(Some(0), WALL);
    assert_eq!(a.station(), Some(0));
    assert!(!a.radio_ready(), "the node loads first");
    assert!(!a.music.as_ref().unwrap().on, "the playlist stops at once");
    a.poll();
    assert!(a.radio_ready());
    assert_eq!(a.radio_nodes(), 1);
    let log = take(&h);
    let radio = made(log.iter().find(|l| l.contains("\"Radio\"")).unwrap());
    let out = made(log.iter().find(|l| l.contains("\"Gain\"")).unwrap());
    let want = [
        format!(r#"[0,"new","{radio}","Radio"]"#),
        format!(r#"[0,"new","{out}","Gain"]"#),
        format!(r#"[0,"connect","{radio}","{out}"]"#),
        format!(r#"[0,"connect","{out}","{min}"]"#),
        format!(r#"[0,"setValue","{radio}.station",0,0]"#),
        format!(r#"[0,"setValue","{radio}.wallDay",20543,0]"#),
        format!(r#"[0,"setValue","{radio}.wallSec",84800,0]"#),
        format!(r#"[0,"setValue","{radio}.tune",1,0]"#),
    ];
    assert_eq!(log, want);

    // Re-tuning, even to another station, moves the params and bumps the
    // serial; no second node.
    a.set_station(Some(2), WALL + 100.0);
    let log = take(&h);
    assert_eq!(
        log,
        [
            format!(r#"[0,"setValue","{radio}.station",2,0]"#),
            format!(r#"[0,"setValue","{radio}.wallDay",20543,0]"#),
            format!(r#"[0,"setValue","{radio}.wallSec",84900,0]"#),
            format!(r#"[0,"setValue","{radio}.tune",2,0]"#),
        ]
    );
    a.set_station(Some(2), WALL + 200.0);
    let log = take(&h);
    assert!(log.contains(&format!(r#"[0,"setValue","{radio}.tune",3,0]"#)));
    assert_eq!(a.radio_nodes(), 1);

    // The music gate toggled with a station on: the playlist stays stopped.
    a.set_music(false);
    a.set_music(true);
    assert!(!a.music.as_ref().unwrap().on);
    take(&h);

    // Off: the station param goes to -1 and the playlist comes back.
    a.set_station(None, WALL + 300.0);
    assert_eq!(a.station(), None);
    assert!(a.music.as_ref().unwrap().on, "the playlist resumes");
    let log = take(&h);
    assert_eq!(
        about(&log, radio),
        [format!(r#"[0,"setValue","{radio}.station",-1,0]"#)]
    );
    assert!(
        log.iter().any(|l| l.contains("\"start\"")),
        "the song starts again"
    );
    assert_eq!(a.radio_nodes(), 1);
    assert!(ctx(&h).problems().is_empty(), "{:?}", ctx(&h).problems());
}

#[test]
fn energy_asked_before_the_node_applies_at_build() {
    let (mut a, h, _) = game(true, false);
    a.set_energy(0.5);
    a.set_station(Some(1), WALL);
    a.poll();
    let log = take(&h);
    let radio = made(log.iter().find(|l| l.contains("\"Radio\"")).unwrap());
    let i = log
        .iter()
        .position(|l| l == &format!(r#"[0,"setValue","{radio}.energy",0.5,0]"#))
        .expect("the energy set at build");
    let j = log
        .iter()
        .position(|l| l == &format!(r#"[0,"setValue","{radio}.station",1,0]"#))
        .unwrap();
    assert!(i < j, "before the tune");
    // From then on it glides.
    a.set_energy(0.9);
    assert_eq!(
        take(&h),
        [format!(r#"[0,"setTarget","{radio}.energy",0.9,0,0.5]"#)]
    );
}

#[test]
fn a_station_asked_before_init_is_tuned_at_init() {
    let mut a = GameAudio::new(Platform::headless(1));
    a.set_station(Some(0), WALL);
    assert_eq!(a.station(), Some(0));
    assert_eq!(a.radio_nodes(), 0);
    // No context ever: nothing happens, and nothing panics.
    a.init(InitOptions::default());
    a.poll();
    assert_eq!(a.radio_nodes(), 0);

    let handle: Handle = Rc::new(RefCell::new(None));
    let hc = handle.clone();
    let platform = Platform {
        new_context: Some(Box::new(move |opts: &ContextOptions| {
            let (ctx, h) = null::context(NullOptions::default(), opts.clone());
            *hc.borrow_mut() = Some((ctx.clone(), h));
            Some(ctx)
        })),
        ..Platform::headless(1)
    };
    let mut a = GameAudio::new(platform);
    a.set_station(Some(0), WALL);
    a.init(InitOptions::default());
    a.set_music(true);
    assert!(!a.music.as_ref().unwrap().on, "tuned at init: no playlist");
    a.poll();
    assert!(a.radio_ready());
    assert_eq!(a.radio_nodes(), 1);
}

#[test]
fn without_audioworklet_the_old_music_plays() {
    let (mut a, h, _) = game(false, false);
    a.set_station(Some(0), WALL);
    assert!(!a.music.as_ref().unwrap().on);
    a.poll();
    assert_eq!(a.station(), None, "not available: off");
    assert_eq!(a.radio_nodes(), 0);
    assert!(!a.radio_ready());
    assert!(a.music.as_ref().unwrap().on, "the playlist is back");
    let log = take(&h);
    assert!(!log.iter().any(|l| l.contains("\"Radio\"")));
    // Later requests change nothing.
    a.set_station(Some(1), WALL);
    a.poll();
    assert_eq!(a.station(), None);
    assert!(a.music.as_ref().unwrap().on);
    assert!(!take(&h).iter().any(|l| l.contains("\"Radio\"")));
}

#[test]
fn the_dj_talks_over_a_ducked_station() {
    let (mut a, h, _) = game(true, true);
    a.set_station(Some(0), WALL);
    a.poll();
    let log = take(&h);
    let out = made(log.iter().find(|l| l.contains("\"Gain\"")).unwrap());

    a.dj_say("marisol-ident-3", 2);
    a.poll();
    let log = take(&h);
    assert!(
        log.iter()
            .any(|l| l.contains(r#""decode""#) && l.contains("marisol-ident-3.2.mp3")),
        "{log:#?}"
    );
    let src = made(log.iter().find(|l| l.contains("\"BufferSource\"")).unwrap());
    assert!(
        log.iter()
            .any(|l| l == &format!(r#"[0,"start","{src}",0.01]"#)),
        "{log:#?}"
    );
    // The station comes down while the clip (a second) plays, and back.
    assert_eq!(
        about(&log, out),
        [
            format!(r#"[0,"cancel","{out}.gain",0.01]"#),
            format!(r#"[0,"setTarget","{out}.gain",0.4,0.01,0.05]"#),
            format!(r#"[0,"setTarget","{out}.gain",1,1.01,0.15]"#),
        ]
    );

    // A clip that is not recorded plays nothing, silently.
    a.dj_say("kit-ident-1", 1);
    a.poll();
    assert!(take(&h).is_empty());
    assert!(ctx(&h).problems().is_empty(), "{:?}", ctx(&h).problems());
}

#[test]
fn no_dj_fetcher_no_dj() {
    let (mut a, h, _) = game(true, false);
    assert!(a.dj().is_none());
    a.dj_say("marisol-ident-3", 1);
    a.poll();
    assert!(take(&h).is_empty());
}

#[cfg(all(feature = "native", not(target_arch = "wasm32")))]
mod native {
    //! A tuned `GameAudio` rendered offline: the station is heard through
    //! the music bus (needs `mp_music::radio::Player`).
    use super::WALL;
    use mp_audio::game::{CarState, GameAudio, InitOptions, Platform, Volume};
    use mp_audio::wa::native;
    use std::cell::RefCell;
    use std::rc::Rc;

    const SR: f64 = 48000.0;
    const FRAME: usize = 384;

    fn db(x: &[f32]) -> f64 {
        let e: f64 = x.iter().map(|&v| v as f64 * v as f64).sum::<f64>() / x.len() as f64;
        10.0 * (e + 1e-20).log10()
    }

    #[test]
    fn a_tuned_station_is_heard() {
        let secs = 4.0;
        let frames = (secs * SR / FRAME as f64).round() as usize * FRAME;
        let ctx = native::offline_context(2, frames, SR as f32);
        let a = Rc::new(RefCell::new(GameAudio::new(Platform::headless(1))));
        {
            let mut a = a.borrow_mut();
            a.init(InitOptions {
                context: Some(ctx.clone()),
                latency_hint: None,
            });
            a.set_volume(Volume {
                master: Some(1.0),
                sfx: Some(0.0),
                music: Some(0.7),
            });
            a.set_music(true);
            a.set_station(Some(0), WALL);
        }
        let dt = FRAME as f64 / SR;
        let aa = a.clone();
        let step = move |_k: usize| {
            let mut a = aa.borrow_mut();
            a.settle();
            a.poll();
            a.update(dt, &CarState::default());
        };
        step(0);
        let out = ctx
            .start_rendering_steered(FRAME, Box::new(step))
            .expect("an offline native context");
        assert!(ctx.problems().is_empty(), "{:?}", ctx.problems());
        let a = a.borrow();
        assert!(a.radio_ready());
        assert_eq!(a.radio_nodes(), 1);
        assert!(!a.music.as_ref().unwrap().on, "the playlist is stopped");
        let late = db(&out[0][(2.5 * SR) as usize..]);
        assert!(late > -50.0, "the station is heard: {late:.1} dB");
    }
}
