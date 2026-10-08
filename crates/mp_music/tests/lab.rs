//! The lab's composer, sequencer and grammar tests
//! (`tools/music-lab/test/lab.test.js`), ported. The ones that need the JS
//! game's `tracks.js` use a generated track instead.

use mp_music::compose::Item::{Deg as D, Ext as X};
use mp_music::compose::{
    MINOR, Motif, MotifOpts, R, Realise, SCALES, chord_on, chord_pcs, euclid, expand, motif,
    progression, realise, thin,
};
use mp_music::genres::{GENRES, chicha, genre, house, trance};
use mp_music::seq::{DRUM_LANES, Event, Seq, compile_track, is_drum_lane, note_to_midi};
use mp_music::track::{Part, Section, Track, lookup};

fn pc_of(tok: &str) -> i32 {
    note_to_midi(tok.trim_end_matches('!')).expect("a note") as i32 % 12
}

fn is_note(tok: &str) -> bool {
    tok.as_bytes()
        .first()
        .is_some_and(|c| (b'A'..=b'G').contains(c))
}

/// `T.sections.map(s => s.p.lead || s.p.blip)`: a section's lead pattern.
fn lead_key(s: &Section) -> Option<&str> {
    lookup(&s.p, "lead")
        .or_else(|| lookup(&s.p, "blip"))
        .map(String::as_str)
}

// ── Sequencer ────────────────────────────────────────────────────

#[test]
fn swing_delays_the_off_16ths_and_the_song_loops_with_an_end_event() {
    // The lab uses the game's 'seabright'; garage swings too.
    let t = (genre("garage").expect("garage").make)(1, None);
    let swing = t.swing.expect("garage swings");
    assert!(swing > 0.0);
    let mut s = Seq::new(t);
    s.seek_bar(16); // the groove: hats on the off 16ths
    let mut even = 0;
    let mut odd = 0;
    for i in 0..16 {
        for e in s.step() {
            if let Event::Drum { dt, .. } = e {
                if i % 2 == 0 {
                    assert_eq!(dt, 0.0);
                    even += 1;
                } else {
                    assert!((dt - swing * s.step_dur).abs() < 1e-12);
                    odd += 1;
                }
            }
        }
    }
    assert!(even > 0 && odd > 0, "{even} on the grid, {odd} swung");
    let last = s.bars - 1;
    s.seek_bar(last);
    let mut end = false;
    for _ in 0..16 {
        if s.step().iter().any(|e| matches!(e, Event::End)) {
            end = true;
        }
    }
    assert!(end);
    assert_eq!(s.bar_index(), 0);
}

#[test]
fn mute_solo_and_energy_layers_drop_what_they_should() {
    let t = house(3, None);
    let count = |setup: &dyn Fn(&mut Seq)| -> Vec<String> {
        let mut s = Seq::new(t.clone());
        setup(&mut s);
        s.seek_bar(16); // the full groove
        let mut lanes: Vec<String> = Vec::new();
        for _ in 0..64 {
            for e in s.step() {
                let name = match &e {
                    Event::Drum { lane, .. } => lane.clone(),
                    Event::Note { part, .. } => part.clone(),
                    _ => continue,
                };
                if !lanes.contains(&name) {
                    lanes.push(name);
                }
            }
        }
        lanes
    };
    let has = |l: &[String], n: &str| l.iter().any(|x| x == n);
    let all = count(&|_| {});
    assert!(has(&all, "kick") && has(&all, "shaker") && has(&all, "bass"));
    let low = count(&|s| s.energy = 0.1);
    assert!(has(&low, "kick") && !has(&low, "shaker") && !has(&low, "hat"));
    let solo = count(&|s| s.solo = Some("drums".to_owned()));
    assert!(has(&solo, "kick") && !has(&solo, "bass"));
    let muted = count(&|s| s.mute = vec!["kick".to_owned(), "bass".to_owned()]);
    assert!(!has(&muted, "kick") && !has(&muted, "bass") && has(&muted, "clap"));
    assert!(DRUM_LANES.contains(&"revCrash") && is_drum_lane("revCrash"));
}

// ── Grammars ─────────────────────────────────────────────────────

#[test]
fn the_grammars_are_seeded_same_seed_same_track_new_seed_new_track() {
    for g in GENRES {
        assert_eq!((g.make)(42, None), (g.make)(42, None), "{}", g.key);
        assert_ne!((g.make)(42, None), (g.make)(43, None), "{}", g.key);
        for seed in 1..=25 {
            let t = (g.make)(seed, None);
            compile_track(&t);
            for s in &t.sections {
                let drums = s.drums.as_deref().expect("a drum pattern");
                assert!(t.drum(drums).is_some(), "{} drums {drums}", t.id);
                for (part, pat) in &s.p {
                    let p = t.part(part).unwrap_or_else(|| panic!("{} {part}", t.id));
                    assert!(lookup(&p.pat, pat).is_some(), "{} {part}.{pat}", t.id);
                }
                for (target, _) in s.auto.iter().flatten() {
                    let part = target.split('.').next().expect("a target");
                    assert!(t.part(part).is_some(), "{target}");
                }
                assert!(s.v.is_none() && s.vd.is_none(), "expand drops v and vd");
            }
        }
    }
}

#[test]
fn deja_vu_1_plays_the_core_in_every_block_0_leaves_it() {
    let keys = |t: &Track| -> Vec<String> {
        t.sections
            .iter()
            .flat_map(|s| s.p.iter().map(|(_, k)| k.clone()))
            .filter(|k| {
                let b = k.as_bytes();
                b.len() == 2 && b[0].is_ascii_lowercase() && b[1].is_ascii_digit()
            })
            .collect()
    };
    let locked = trance(7, Some(1.0));
    let free = trance(7, Some(0.0));
    assert!(keys(&locked).iter().all(|k| k.ends_with('0')));
    assert!(keys(&free).iter().any(|k| !k.ends_with('0')));
    // The variants are made from the core once: each differs from it in a few steps.
    let bass = locked.part("bass").expect("bass");
    let core = lookup(&bass.pat, "b0").expect("b0");
    for k in ["b1", "b2"] {
        let v = lookup(&bass.pat, k).expect(k);
        let diff = v.chars().zip(core.chars()).filter(|(a, b)| a != b).count();
        assert!((1..=12).contains(&diff), "{k} differs in {diff} steps");
    }
}

#[test]
fn the_composer_euclid_chords_motifs_in_the_scale_the_hook_answered_and_thinned() {
    assert_eq!(euclid(3, 8, 0, 'x'), "x..x..x.");
    assert_eq!(euclid(5, 16, 0, 'x'), "x..x..x..x..x...");
    assert_eq!(euclid(5, 16, 1, 'x'), ".x..x..x..x..x..");
    assert_eq!(chord_on(MINOR, 9, 0, "7"), "Am7");
    assert_eq!(chord_on(MINOR, 9, 5, "7"), "Fmaj7");
    assert_eq!(chord_on(MINOR, 9, 6, ""), "G");
    assert_eq!(chord_on(SCALES[1], 2, 3, "9"), "G9");
    assert_eq!(
        progression(MINOR, 9, &[D(0), D(5), D(2), D(6)], ""),
        "Am F C G"
    );
    let mut r = R::new(11, 0);
    let m = motif(
        &mut r,
        MotifOpts {
            bars: 2,
            ..Default::default()
        },
    );
    assert_eq!(m.onsets[0], 0);
    assert_eq!(m.ivs.len(), m.onsets.len() - 1);
    let chords: Vec<String> = ["Am", "F", "C", "G", "Am", "F", "C", "G"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let lane = realise(
        &m,
        &Realise {
            scale: MINOR,
            tonic: 69,
            chords: &chords,
            lo: -2,
            hi: 9,
            gate: Some(8),
            ..Default::default()
        },
    );
    let toks: Vec<&str> = lane.split(' ').collect();
    assert_eq!(toks.len(), 128);
    let notes: Vec<(&str, usize)> = toks
        .iter()
        .enumerate()
        .filter(|(_, t)| is_note(t))
        .map(|(i, t)| (*t, i))
        .collect();
    assert!(notes.len() >= 8);
    for (t, _) in &notes {
        assert!([9, 11, 0, 2, 4, 5, 7].contains(&pc_of(t)), "{t} in A minor");
    }
    // Strong beats sit on chord tones; the eighth bar ends on the chord's root.
    let tones = |name: &str| -> Vec<i32> {
        match name {
            "Am" => vec![9, 0, 4],
            "F" => vec![5, 9, 0],
            "C" => vec![0, 4, 7],
            _ => vec![7, 11, 2],
        }
    };
    for (t, i) in &notes {
        if i % 4 == 0 {
            let ch = &chords[i / 16];
            assert!(tones(ch).contains(&pc_of(t)), "{t} at {i} on {ch}");
        }
    }
    let last = notes.iter().rfind(|(_, i)| *i >= 112).expect("a last note");
    assert_eq!(pc_of(last.0), 7, "closes on G, the last chord");
    // The thinned hook keeps the downbeat notes and loses some others.
    let th = thin(&m);
    assert!(th.onsets.len() < m.onsets.len() && th.onsets[0] == 0);
    assert_eq!(th.ivs.len(), th.onsets.len() - 1);
    assert_eq!(
        th.ivs.iter().sum::<i32>(),
        m.ivs.iter().sum::<i32>(),
        "same net contour"
    );
}

#[test]
fn every_genre_states_the_hook_sparse_before_the_drop_and_in_full_at_it_and_the_bass_picks_up() {
    for g in GENRES {
        let t = (g.make)(9, None);
        let lead = t
            .part("lead")
            .or_else(|| t.part("blip"))
            .unwrap_or_else(|| panic!("{} has a lead", g.key));
        for k in ["hook", "sparse", "answer"] {
            assert!(lookup(&lead.pat, k).is_some(), "{} has a hook ({k})", g.key);
        }
        let uses: Vec<&str> = t.sections.iter().filter_map(lead_key).collect();
        let drop = t
            .sections
            .iter()
            .position(|s| s.drop == Some(true))
            .unwrap_or(0);
        assert!(drop > 0, "{} has a drop", g.key);
        assert!(
            uses.contains(&"hook") && uses.contains(&"sparse"),
            "{} states the hook both ways",
            g.key
        );
        // Sparse before a drop (eurobeat's first chorus follows a verse, so its
        // sparse statement sits before the second), full from the first.
        let last_drop = t
            .sections
            .iter()
            .rposition(|s| s.drop == Some(true))
            .expect("a drop");
        assert!(
            t.sections[..last_drop]
                .iter()
                .any(|s| lead_key(s) == Some("sparse")),
            "{} sparse before a drop",
            g.key
        );
        assert_eq!(
            lead_key(&t.sections[drop]),
            Some("hook"),
            "{} full hook at the drop",
            g.key
        );
    }
    // The pickup: 'n' plays the next bar's chord root.
    let mut t = Track::skeleton("pickup");
    t.bpm = 120.0;
    t.prog = vec![("a".to_owned(), "Am F".to_owned())];
    t.parts = vec![(
        "bass".to_owned(),
        Part {
            kind: "bass".into(),
            lo: Some(33.0),
            pat: vec![("a".to_owned(), "r.............n.".to_owned())],
            ..Default::default()
        },
    )];
    t.sections = vec![Section {
        bars: 2,
        drums: None,
        p: vec![("bass".to_owned(), "a".to_owned())],
        ..Default::default()
    }];
    let mut s = Seq::new(t);
    let mut notes = Vec::new();
    for _ in 0..32 {
        for e in s.step() {
            if let Event::Note { midis, .. } = e {
                notes.push(midis[0]);
            }
        }
    }
    assert_eq!(notes, vec![33.0, 41.0, 41.0, 33.0]);
}

#[test]
fn chicha_the_cumbia_bass_the_guiros_stroke_a_pentatonic_lead_twin_guitars_and_the_organ_solo() {
    for seed in 1..=12 {
        let t = chicha(seed, None);
        assert_eq!(t.kit_name.as_deref(), Some("latin"));
        // The tumbao: the root on beats 1 and 3, the fifth (or a pickup) on the
        // off-beat before the next beat, in every bar of every pattern.
        for (_, pat) in &t.part("bass").expect("bass").pat {
            let b = pat.as_bytes();
            for bar in b.chunks(16) {
                let bar = std::str::from_utf8(bar).expect("ascii");
                assert_eq!(&bar[..1], "r", "{} starts on the root: {bar}", t.id);
                assert_eq!(&bar[8..9], "r", "{} root on 3: {bar}", t.id);
                assert!(
                    "fon".contains(&bar[6..7]) && "fon".contains(&bar[14..15]),
                    "{} fifths off the beat: {bar}",
                    t.id
                );
            }
        }
        // The güiro: long on every beat, shorts between.
        let v = t.drum("v0").expect("v0");
        assert_eq!(
            lookup(v, "guiroL").map(String::as_str),
            Some("x...x...x...x...")
        );
        assert!(
            lookup(v, "guiroS").is_some_and(|s| s.contains('x'))
                && lookup(v, "congaO").is_some()
                && lookup(v, "congaS").is_some()
                && lookup(v, "bongoH").is_some(),
            "the verse percussion"
        );
        let c0 = t.drum("c0").expect("c0");
        assert!(
            lookup(c0, "cascara").is_some() && lookup(c0, "cowbell").is_some(),
            "the chorus adds the timbales' cáscara and the bell"
        );
        // The lead is minor pentatonic off the chord tones: no 2nd or 6th of the key
        // except as a chord tone (the V7's 7th is the 4th; its 3rd is outside the scale).
        let style = t.style.as_deref().expect("style");
        let key_name = style
            .split(" · ")
            .nth(1)
            .expect("key")
            .split(' ')
            .next()
            .expect("note");
        let tonic = [
            "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
        ]
        .iter()
        .position(|n| *n == key_name)
        .expect("a tonic") as i32;
        let chords: Vec<&str> = lookup(&t.prog, "c").expect("c").split(' ').collect();
        let hook = lookup(&t.part("lead").expect("lead").pat, "hook").expect("hook");
        let mut off = 0;
        let mut n = 0;
        for (i, tok) in hook.split(' ').enumerate() {
            if !is_note(tok) {
                continue;
            }
            n += 1;
            let pc = (note_to_midi(tok.trim_end_matches('!')).expect("a note") as i32 - tonic
                + 12 * 10)
                % 12;
            let ch = chords[(i / 16) % chords.len()];
            let tones: Vec<i32> = chord_pcs(ch)
                .pcs
                .iter()
                .map(|p| (p - tonic + 12) % 12)
                .collect();
            if (pc == 2 || pc == 8) && !tones.contains(&pc) {
                off += 1;
            }
        }
        assert!(
            n >= 8 && off == 0,
            "{}: {off} of {n} lead notes off the pentatonic",
            t.id
        );
        // Twin guitars in the chorus, the organ solo in the break, the hook alone to open.
        let drop = t
            .sections
            .iter()
            .find(|s| s.drop == Some(true))
            .expect("a drop");
        assert_eq!(lookup(&drop.p, "lead").map(String::as_str), Some("hook"));
        assert_eq!(lookup(&drop.p, "lead2").map(String::as_str), Some("third"));
        assert!(
            t.sections.iter().any(|s| s.drums.as_deref() == Some("brk")
                && lookup(&s.p, "organLead").map(String::as_str) == Some("hook")),
            "organ solo"
        );
        assert_eq!(
            t.sections[0]
                .p
                .iter()
                .map(|(k, _)| k.as_str())
                .collect::<Vec<_>>(),
            vec!["lead"]
        );
        compile_track(&t);
    }
    // The dominant: 'dom' writes the V7 whatever the scale.
    assert_eq!(chord_on(MINOR, 0, 4, "dom"), "G7");
    assert_eq!(
        progression(MINOR, 9, &[D(0), D(3), X(4, "dom"), D(0)], ""),
        "Am Dm E7 Am"
    );
    // avoid: the pentatonic realisation steps over the 2nd and 6th.
    let m: Motif = motif(
        &mut R::new(3, 0),
        MotifOpts {
            bars: 2,
            density: mp_music::compose::Density::Dense,
            ..Default::default()
        },
    );
    let chords: Vec<String> = vec!["Am".to_owned(); 8];
    let lane = realise(
        &m,
        &Realise {
            scale: MINOR,
            tonic: 69,
            chords: &chords,
            lo: -3,
            hi: 9,
            avoid: &[1, 5],
            ..Default::default()
        },
    );
    for t in lane.split(' ') {
        if is_note(t) {
            assert!(![11, 5].contains(&pc_of(t)), "{t} is not pentatonic");
        }
    }
}

#[test]
fn expand_splits_long_sections_into_8_bar_blocks_and_keeps_the_seams_where_they_belong() {
    let s = vec![Section {
        bars: 16,
        drums: Some("g".to_owned()),
        vd: Some(2),
        crash: Some(true),
        fill: Some("clap".to_owned()),
        lp: Some([100.0, 1600.0]),
        auto: Some(vec![("pad.cutoff".to_owned(), [0.0, 100.0])]),
        p: vec![("bass".to_owned(), "b".to_owned())],
        v: Some(vec![("bass".to_owned(), 3)]),
        ..Default::default()
    }];
    let out = expand(&s, &mut R::new(1, 0), 1.0);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].crash, Some(true));
    assert_eq!(out[1].crash, None);
    assert_eq!(out[1].fill.as_deref(), Some("clap"));
    assert_eq!(out[0].fill, None);
    assert_eq!(
        lookup(out[0].auto.as_ref().expect("auto"), "pad.cutoff"),
        Some(&[0.0, 50.0])
    );
    assert_eq!(
        lookup(out[1].auto.as_ref().expect("auto"), "pad.cutoff"),
        Some(&[50.0, 100.0])
    );
    assert_eq!(out[0].lp, Some([100.0, 400.0]));
    assert_eq!(lookup(&out[0].p, "bass").map(String::as_str), Some("b0"));
    assert_eq!(lookup(&out[1].p, "bass").map(String::as_str), Some("b0"));
    assert_eq!(out[0].drums.as_deref(), Some("g0"));
    assert!(out[0].v.is_none() && out[0].vd.is_none());
}
