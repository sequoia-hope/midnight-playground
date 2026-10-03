//! Graph construction (`Audio.js` `_build`, `_makeNoise`, `_loop` and the
//! `_build*` methods).

use super::{GATE_TAIL, GameAudio, GateId, SFX_GATE, SirenState, siren_pattern};
use crate::music::Music;
use crate::noise;
use crate::samples;
use crate::shapes::{distortion_curve, exhaust_curve, pulse_wave, soft_square_wave};
use crate::wa::{
    AudioBuffer, AudioBufferSourceNode, AudioContext, BiquadFilterNode, BiquadFilterType,
    Connectable, ConvolverNode, DynamicsCompressorNode, GainNode, Node, OscillatorNode,
    OscillatorType, OverSampleType, StereoPannerNode, WaveShaperNode,
};
use mr_math::Rng;
use std::rc::Rc;

/// The voice gates (`_vg`).
pub(crate) struct VoiceGates {
    pub tunnel: GateId,
    pub eng: GateId,
    pub whine: GateId,
    pub turbo: GateId,
    pub ev: GateId,
    pub road: GateId,
    pub squeal: GateId,
    pub gravel: GateId,
    pub scrape: GateId,
    pub nitro: GateId,
    pub tyres: GateId,
    pub dmg: GateId,
    pub radio: GateId,
}

/// One exhaust chain (`chain(pan, detune)`).
pub(crate) struct Chain {
    pub on: OscillatorNode,
    pub off: OscillatorNode,
    pub g_on: GainNode,
    pub g_off: GainNode,
    pub drive: GainNode,
    pub f: Vec<BiquadFilterNode>,
    pub lp: BiquadFilterNode,
}

/// `this.eng`.
pub(crate) struct Eng {
    pub out: GainNode,
    pub shift_g: GainNode,
    pub lim_g: GainNode,
    pub mis_g: GainNode,
    pub l: Chain,
    pub r: Chain,
    pub rum: OscillatorNode,
    pub rum_g: GainNode,
    pub in1: BiquadFilterNode,
    pub in2: BiquadFilterNode,
    pub in_g: GainNode,
    pub rasp_bp: BiquadFilterNode,
    pub rasp_depth: GainNode,
    pub lim_depth: GainNode,
    pub whine: OscillatorNode,
    pub whine2: OscillatorNode,
    pub whine_g: GainNode,
}

pub(crate) struct Turbo {
    pub out: GainNode,
    pub w1: OscillatorNode,
    pub w2: OscillatorNode,
    pub wg: GainNode,
    pub hiss_bp: BiquadFilterNode,
    pub hg: GainNode,
}

/// An electric tone (`tone(type, gain)`).
pub(crate) struct Tone {
    pub o: OscillatorNode,
    pub g: GainNode,
}

pub(crate) struct Ev {
    pub out: GainNode,
    pub f1: Tone,
    pub f2: Tone,
    pub f3: Tone,
    pub mesh: Tone,
    pub regen: Tone,
    pub saw: Tone,
    pub saw_lp: BiquadFilterNode,
    pub growl: Tone,
    pub growl_bp: BiquadFilterNode,
    pub hum: Tone,
    pub hum2: Tone,
    pub air_bp: BiquadFilterNode,
    pub air_g: GainNode,
}

/// A noise bed through a filter into a gain (`mk(...)`).
pub(crate) struct Bed {
    pub src: AudioBufferSourceNode,
    pub f: BiquadFilterNode,
    pub g: GainNode,
}

pub(crate) struct WindSide {
    pub bed: Bed,
    pub p: StereoPannerNode,
}

pub(crate) struct Squeal {
    pub out: GainNode,
    pub o1: OscillatorNode,
    pub o2: OscillatorNode,
    pub tone: GainNode,
    pub tbp: BiquadFilterNode,
    pub nbp: BiquadFilterNode,
    pub nbp2: BiquadFilterNode,
}

pub(crate) struct Scrape {
    pub pan: StereoPannerNode,
    pub g: GainNode,
    pub src: AudioBufferSourceNode,
    pub grit: Bed,
}

pub(crate) struct RivalVoice {
    pub osc: OscillatorNode,
    pub lp: BiquadFilterNode,
    pub g: GainNode,
    pub p: StereoPannerNode,
    pub gate: GateId,
}

pub(crate) struct SirenVoice {
    pub a: OscillatorNode,
    pub b: OscillatorNode,
    pub lp: BiquadFilterNode,
    pub g: GainNode,
    pub p: StereoPannerNode,
    pub gate: GateId,
    pub tri: OscillatorNode,
    pub sq: OscillatorNode,
    pub tri_d: GainNode,
    pub sq_d: GainNode,
}

pub(crate) struct Tyres {
    pub out: GainNode,
    pub pulse: OscillatorNode,
}

pub(crate) struct Dmg {
    pub out: GainNode,
    pub pulse: OscillatorNode,
    pub knock: GainNode,
    pub rattle: GainNode,
    pub steam: GainNode,
}

pub(crate) struct RadioBus {
    pub input: GainNode,
    pub p: StereoPannerNode,
}

/// Everything `_build` makes (the JS keeps all of it as properties, some
/// only for tools: the limiter is where the music player taps its analyser).
#[allow(dead_code)]
pub(crate) struct Graph {
    pub limiter: DynamicsCompressorNode,
    pub master: GainNode,
    pub sfx_bus: GainNode,
    pub sfx_comp: DynamicsCompressorNode,
    pub env_low: BiquadFilterNode,
    pub sfx_vol: GainNode,
    pub music_in: BiquadFilterNode,
    pub music_gate: GainNode,
    pub music_duck: GainNode,
    pub music_mood_lp: BiquadFilterNode,
    pub music_mood: GainNode,
    pub music_vol: GainNode,
    pub noise_white: AudioBuffer,
    pub noise_pink: AudioBuffer,
    pub noise_brown: AudioBuffer,
    pub sfx_buf: Vec<(&'static str, AudioBuffer)>,
    pub tunnel: ConvolverNode,
    pub env_send: GainNode,
    pub eng: Eng,
    pub turbo: Turbo,
    pub ev: Ev,
    pub wind: Bed,
    pub wind_hi: Vec<WindSide>,
    pub rumble: Bed,
    pub squeal: Squeal,
    pub gravel: Bed,
    pub scrape: Scrape,
    pub nitro_hiss: Bed,
    pub nitro_rumble: Bed,
    pub nitro_flame: Bed,
    pub nitro_flutter: GainNode,
    pub rivals: Vec<RivalVoice>,
    pub sirens: Vec<SirenVoice>,
    pub tyres: Tyres,
    pub dmg: Dmg,
    pub radio_bus: RadioBus,
    pub vg: VoiceGates,
    /// Nodes only the graph holds in the JS (local variables there).
    pub _keep: Vec<Node>,
}

impl Graph {
    /// `this.sfxBuf[name]`.
    pub fn sfx(&self, name: &str) -> Option<&AudioBuffer> {
        self.sfx_buf
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, b)| b)
    }
}

/// A `&mut dyn Rng` as an `impl Rng` (the generators take one).
struct DynRng<'a>(&'a mut dyn Rng);

impl Rng for DynRng<'_> {
    fn next_f64(&mut self) -> f64 {
        self.0.next_f64()
    }
}

fn biquad(ctx: &AudioContext, t: BiquadFilterType) -> BiquadFilterNode {
    let f = ctx.create_biquad_filter();
    f.set_type(t);
    f
}

fn gain(ctx: &AudioContext, v: f64) -> GainNode {
    let g = ctx.create_gain();
    g.gain.set_value(v);
    g
}

fn osc(ctx: &AudioContext, t: OscillatorType) -> OscillatorNode {
    let o = ctx.create_oscillator();
    o.set_type(t);
    o
}

fn n(x: &impl std::ops::Deref<Target = Node>) -> Node {
    (**x).clone()
}

fn link(a: &impl std::ops::Deref<Target = Node>, b: &impl Connectable) {
    let _ = a.connect(b);
}

use BiquadFilterType as B;

impl GameAudio {
    // ── Graph construction ───────────────────────────────────────────
    pub(super) fn build(&mut self) {
        let ctx = self.ctx.clone().expect("a context");
        let mut keep: Vec<Node> = Vec::new();
        let limiter = ctx.create_dynamics_compressor();
        limiter.threshold.set_value(-1.5);
        limiter.knee.set_value(0.0);
        limiter.ratio.set_value(20.0);
        limiter.attack.set_value(0.002);
        limiter.release.set_value(0.12);
        link(&limiter, &ctx.destination());
        let master = ctx.create_gain();
        link(&master, &limiter);

        // SFX bus: its own glue compressor, then the user volume.
        let sfx_bus = ctx.create_gain();
        let sfx_comp = ctx.create_dynamics_compressor();
        sfx_comp.threshold.set_value(-13.0);
        sfx_comp.knee.set_value(8.0);
        sfx_comp.ratio.set_value(3.0);
        sfx_comp.attack.set_value(0.005);
        sfx_comp.release.set_value(0.18);
        let env_low = biquad(&ctx, B::Lowshelf);
        env_low.frequency.set_value(180.0);
        env_low.gain.set_value(0.0);
        let sfx_vol = ctx.create_gain();
        link(&sfx_bus, &env_low);
        link(&env_low, &sfx_comp);
        link(&sfx_comp, &sfx_vol);
        link(&sfx_vol, &master);
        let sg = self.add_gate(vec![(n(&sfx_vol), n(&master))], 0.5, vec![]);
        debug_assert_eq!(sg, SFX_GATE);

        // Music bus: mix → gentle compressor → on/off gate → user volume.
        let music_in = biquad(&ctx, B::Highpass);
        music_in.frequency.set_value(36.0);
        let music_mix = ctx.create_gain();
        link(&music_in, &music_mix);
        let music_comp = ctx.create_dynamics_compressor();
        music_comp.threshold.set_value(-14.0);
        music_comp.knee.set_value(12.0);
        music_comp.ratio.set_value(2.0);
        music_comp.attack.set_value(0.004);
        music_comp.release.set_value(0.25);
        let music_gate = gain(&ctx, 0.0);
        let music_duck = ctx.create_gain(); // dips under the finish fanfare
        let music_vol = ctx.create_gain();
        link(&music_mix, &music_comp);
        link(&music_comp, &music_gate);
        // Pursuit mood: a low-pass (wide open unless cooling down) and a trim.
        let music_mood_lp = biquad(&ctx, B::Lowpass);
        music_mood_lp.frequency.set_value(20000.0);
        music_mood_lp.q.set_value(0.5);
        let music_mood = ctx.create_gain();
        link(&music_gate, &music_duck);
        link(&music_duck, &music_mood_lp);
        link(&music_mood_lp, &music_mood);
        link(&music_mood, &music_vol);
        link(&music_vol, &master);
        keep.extend([n(&music_mix), n(&music_comp)]);

        // _makeNoise
        let sr = ctx.sample_rate();
        let len = (sr * 2.0).floor() as u32;
        let white = ctx.create_buffer(1, len, sr);
        let pink = ctx.create_buffer(1, len, sr);
        let brown = ctx.create_buffer(1, len, sr);
        let beds = {
            let mut r = self.platform.random.borrow_mut();
            noise::make_noise(sr, &mut DynRng(&mut *r))
        };
        white.copy_to_channel(&beds.white, 0);
        pink.copy_to_channel(&beds.pink, 0);
        brown.copy_to_channel(&beds.brown, 0);
        let sfx_buf = samples::render_sfx(&ctx);

        // _buildTunnel: concrete tube, dense early reflections, ~1.3 s dark tail.
        let tlen = (sr * 1.4).floor() as u32;
        let ir = ctx.create_buffer(2, tlen, sr);
        let ird = {
            let mut r = self.platform.random.borrow_mut();
            noise::tunnel_ir(sr, &mut DynRng(&mut *r))
        };
        for (c, d) in ird.iter().enumerate() {
            ir.copy_to_channel(d, c as u32);
        }
        let tunnel = ctx.create_convolver();
        let _ = tunnel.set_buffer(Some(&ir));
        let env_send = gain(&ctx, 0.0);
        let tout = gain(&ctx, 0.55);
        link(&sfx_bus, &env_send);
        link(&env_send, &tunnel);
        link(&tunnel, &tout);
        link(&tout, &sfx_comp);
        // Rendered only in (and just out of) a tunnel: the fade and the 1.4 s tail.
        let g_tunnel = self.add_gate(vec![(n(&tout), n(&sfx_comp))], 4.0, vec![]);
        keep.push(n(&tout));

        let bufs = (white, pink, brown);
        let (eng, g_eng, g_whine) = self.build_engine(&ctx, &sfx_bus, &bufs, &mut keep);
        let (turbo, g_turbo) = self.build_turbo(&ctx, &sfx_bus, &bufs, &mut keep);
        let (ev, g_ev) = self.build_electric(&ctx, &sfx_bus, &bufs, &mut keep);
        let env = self.build_environment(&ctx, &sfx_bus, &bufs, &sfx_buf, &mut keep);
        let rivals = self.build_rivals(&ctx, &sfx_bus);
        let p = self.build_pursuit(&ctx, &sfx_bus, &bufs, &mut keep);

        let music = Music::build(&ctx, &music_in, self.realtime, self.platform.random.clone());
        self.music = Some(music);
        let cb = self.on_track_change.clone();
        self.music.as_mut().expect("music").on_track = Some(Rc::new(move |info| {
            if let Some(f) = cb.borrow().as_ref() {
                f(info);
            }
        }));

        let (white, pink, brown) = bufs;
        self.g = Some(Rc::new(Graph {
            limiter,
            master,
            sfx_bus,
            sfx_comp,
            env_low,
            sfx_vol,
            music_in,
            music_gate,
            music_duck,
            music_mood_lp,
            music_mood,
            music_vol,
            noise_white: white,
            noise_pink: pink,
            noise_brown: brown,
            sfx_buf,
            tunnel,
            env_send,
            eng,
            turbo,
            ev,
            wind: env.wind,
            wind_hi: env.wind_hi,
            rumble: env.rumble,
            squeal: env.squeal,
            gravel: env.gravel,
            scrape: env.scrape,
            nitro_hiss: env.nitro_hiss,
            nitro_rumble: env.nitro_rumble,
            nitro_flame: env.nitro_flame,
            nitro_flutter: env.nitro_flutter,
            rivals,
            sirens: p.sirens,
            tyres: p.tyres,
            dmg: p.dmg,
            radio_bus: p.radio_bus,
            vg: VoiceGates {
                tunnel: g_tunnel,
                eng: g_eng,
                whine: g_whine,
                turbo: g_turbo,
                ev: g_ev,
                road: env.g_road,
                squeal: env.g_squeal,
                gravel: env.g_gravel,
                scrape: env.g_scrape,
                nitro: env.g_nitro,
                tyres: p.g_tyres,
                dmg: p.g_dmg,
                radio: p.g_radio,
            },
            _keep: keep,
        }));
        // Everything starts silent: update() and the setters connect what they use.
        for g in self.gates.iter_mut().skip(1) {
            g.shut();
        }
        self.apply_volumes(0.001);
    }

    /// `_loop(buffer, rate = 1)`: a looped source started at a random offset.
    fn loop_src(
        &mut self,
        ctx: &AudioContext,
        buffer: &AudioBuffer,
        rate: f64,
    ) -> AudioBufferSourceNode {
        let src = ctx.create_buffer_source();
        let _ = src.set_buffer(Some(buffer));
        src.set_loop(true);
        src.playback_rate.set_value(rate);
        let off = self.random() * buffer.duration();
        let _ = src.start_with(0.0, off, None);
        src
    }

    fn build_engine(
        &mut self,
        ctx: &AudioContext,
        sfx_bus: &GainNode,
        (white, pink, brown): &(AudioBuffer, AudioBuffer, AudioBuffer),
        keep: &mut Vec<Node>,
    ) -> (Eng, GateId, GateId) {
        let sum = gain(ctx, 0.105);
        let hp = biquad(ctx, B::Highpass);
        hp.frequency.set_value(28.0);
        hp.q.set_value(0.6);
        let shift_g = gain(ctx, 1.0);
        let lim_g = gain(ctx, 1.0);
        let mis_g = gain(ctx, 1.0); // damage misfires
        let out = gain(ctx, 0.0);
        // Weight under the whole engine, exhaust chains and rumble alike.
        let body = biquad(ctx, B::Lowshelf);
        body.frequency.set_value(140.0);
        body.gain.set_value(6.0);
        link(&sum, &hp);
        link(&hp, &body);
        link(&body, &shift_g);
        link(&shift_g, &lim_g);
        link(&lim_g, &mis_g);
        link(&mis_g, &out);
        link(&out, sfx_bus);
        keep.extend([n(&sum), n(&hp), n(&body)]);

        // Slow combustion unsteadiness: low-passed noise wobbles amplitude and
        // pitch a touch, so the cycle never repeats exactly.
        let jit_src = self.loop_src(ctx, brown, 0.7);
        let jit_lp = biquad(ctx, B::Lowpass);
        jit_lp.frequency.set_value(16.0);
        link(&jit_src, &jit_lp);
        let jit_amp = gain(ctx, 0.35);
        let jit_pitch = gain(ctx, 14.0); // cents
        link(&jit_lp, &jit_amp);
        link(&jit_lp, &jit_pitch);
        keep.extend([n(&jit_src), n(&jit_lp), n(&jit_amp), n(&jit_pitch)]);

        let chain = |pan: f64, detune: f64, keep: &mut Vec<Node>| -> Chain {
            let on = ctx.create_oscillator();
            let off = ctx.create_oscillator();
            let g_on = ctx.create_gain();
            let g_off = ctx.create_gain();
            g_on.gain.set_value(0.5);
            g_off.gain.set_value(0.5);
            let am = gain(ctx, 1.0);
            link(&jit_amp, &am.gain);
            link(&on, &g_on);
            link(&off, &g_off);
            link(&g_on, &am);
            link(&g_off, &am);
            for o in [&on, &off] {
                o.detune.set_value(detune);
                link(&jit_pitch, &o.detune);
            }
            let drive = gain(ctx, 1.4);
            let shaper = ctx.create_wave_shaper();
            shaper.set_curve(Some(&exhaust_curve(2.2, 2048)));
            shaper.set_oversample(OverSampleType::X2);
            link(&am, &drive);
            link(&drive, &shaper);
            // The asymmetric shaper leaves a DC offset that wanders with the
            // envelope; block it before it turns into sub-bass rumble.
            let dc = biquad(ctx, B::Highpass);
            dc.frequency.set_value(28.0);
            dc.q.set_value(0.6);
            link(&shaper, &dc);
            let f: Vec<BiquadFilterNode> = (0..3).map(|_| biquad(ctx, B::Peaking)).collect();
            let lp = biquad(ctx, B::Lowpass);
            lp.q.set_value(0.9);
            lp.frequency.set_value(800.0);
            link(&dc, &f[0]);
            link(&f[0], &f[1]);
            link(&f[1], &f[2]);
            link(&f[2], &lp);
            let p = ctx.create_stereo_panner();
            p.pan.set_value(pan);
            link(&lp, &p);
            link(&p, &sum);
            on.frequency.set_value(7.0);
            off.frequency.set_value(7.0);
            let _ = on.start();
            let _ = off.start();
            keep.extend([n(&am), n(&shaper), n(&dc), n(&p)]);
            Chain {
                on,
                off,
                g_on,
                g_off,
                drive,
                f,
                lp,
            }
        };
        let l = chain(-0.38, -4.0, keep);
        let r = chain(0.38, 5.0, keep);

        // Rumble: the low end on its own oscillator, low-passed, then squashed a
        // little so its overtones still carry on small speakers.
        let rum = ctx.create_oscillator();
        rum.frequency.set_value(7.0);
        link(&jit_pitch, &rum.detune);
        let rum_hp = biquad(ctx, B::Highpass);
        rum_hp.frequency.set_value(26.0);
        rum_hp.q.set_value(0.7);
        let rum_lp = biquad(ctx, B::Lowpass);
        rum_lp.frequency.set_value(170.0);
        rum_lp.q.set_value(0.8);
        let rum_am = gain(ctx, 1.0);
        link(&jit_amp, &rum_am.gain);
        let rum_sh = ctx.create_wave_shaper();
        rum_sh.set_curve(Some(&distortion_curve(2.5, 1024)));
        rum_sh.set_oversample(OverSampleType::X2);
        let rum_lp2 = biquad(ctx, B::Lowpass);
        rum_lp2.frequency.set_value(520.0);
        rum_lp2.q.set_value(0.6);
        let rum_g = gain(ctx, 0.0);
        link(&rum, &rum_hp);
        link(&rum_hp, &rum_lp);
        link(&rum_lp, &rum_am);
        link(&rum_am, &rum_sh);
        link(&rum_sh, &rum_lp2);
        link(&rum_lp2, &rum_g);
        link(&rum_g, &sum);
        let _ = rum.start();
        keep.extend([n(&rum_hp), n(&rum_lp), n(&rum_am), n(&rum_sh), n(&rum_lp2)]);

        // Intake roar: noise band-passed at the firing rate and its double.
        let intake = self.loop_src(ctx, pink, 1.0);
        let in1 = biquad(ctx, B::Bandpass);
        in1.q.set_value(1.6);
        let in2 = biquad(ctx, B::Bandpass);
        in2.q.set_value(2.4);
        let in_g = gain(ctx, 0.0);
        let in2_g = gain(ctx, 0.6);
        link(&intake, &in1);
        link(&intake, &in2);
        link(&in1, &in_g);
        link(&in2, &in2_g);
        link(&in2_g, &in_g);
        link(&in_g, &sum);
        keep.extend([n(&intake), n(&in2_g)]);

        // Exhaust rasp: high noise gated by the engine's own pulse wave.
        let rasp = self.loop_src(ctx, white, 1.0);
        let rasp_bp = biquad(ctx, B::Bandpass);
        rasp_bp.frequency.set_value(2600.0);
        rasp_bp.q.set_value(0.8);
        let rasp_g = gain(ctx, 0.0);
        let rasp_depth = gain(ctx, 0.0);
        link(&rasp, &rasp_bp);
        link(&rasp_bp, &rasp_g);
        link(&l.on, &rasp_depth);
        link(&rasp_depth, &rasp_g.gain);
        link(&rasp_g, &sum);
        keep.extend([n(&rasp), n(&rasp_g)]);

        // Rev limiter: square LFO chops the engine when bouncing off redline.
        let lim_lfo = osc(ctx, OscillatorType::Square);
        lim_lfo.frequency.set_value(14.0);
        let lim_depth = gain(ctx, 0.0);
        link(&lim_lfo, &lim_depth);
        link(&lim_depth, &lim_g.gain);
        let _ = lim_lfo.start();
        keep.push(n(&lim_lfo));

        // Transmission / diff whine (rises with road speed) — straight into sfx.
        let whine = osc(ctx, OscillatorType::Sine);
        let whine2 = osc(ctx, OscillatorType::Triangle);
        let whine_g = gain(ctx, 0.0);
        let w2g = gain(ctx, 0.35);
        link(&whine, &whine_g);
        link(&whine2, &w2g);
        link(&w2g, &whine_g);
        link(&whine_g, sfx_bus);
        whine.frequency.set_value(200.0);
        whine2.frequency.set_value(300.0);
        let _ = whine.start();
        let _ = whine2.start();
        keep.push(n(&w2g));

        let g_eng = self.add_gate(
            vec![(n(&out), n(sfx_bus))],
            GATE_TAIL,
            vec![out.gain.clone()],
        );
        let g_whine = self.add_gate(
            vec![(n(&whine_g), n(sfx_bus))],
            GATE_TAIL,
            vec![whine_g.gain.clone()],
        );
        (
            Eng {
                out,
                shift_g,
                lim_g,
                mis_g,
                l,
                r,
                rum,
                rum_g,
                in1,
                in2,
                in_g,
                rasp_bp,
                rasp_depth,
                lim_depth,
                whine,
                whine2,
                whine_g,
            },
            g_eng,
            g_whine,
        )
    }

    // Turbo: a whistle that climbs with shaft speed and a breathy intake
    // hiss, both scaled by boost. The blow-off valve is a one-shot.
    fn build_turbo(
        &mut self,
        ctx: &AudioContext,
        sfx_bus: &GainNode,
        (white, _, _): &(AudioBuffer, AudioBuffer, AudioBuffer),
        keep: &mut Vec<Node>,
    ) -> (Turbo, GateId) {
        let out = gain(ctx, 0.0);
        link(&out, sfx_bus);
        let w1 = osc(ctx, OscillatorType::Sine);
        let w2 = osc(ctx, OscillatorType::Sine);
        let wg = gain(ctx, 0.0);
        let w2g = gain(ctx, 0.35);
        link(&w1, &wg);
        link(&w2, &w2g);
        link(&w2g, &wg);
        link(&wg, &out);
        let hiss_bp = biquad(ctx, B::Bandpass);
        hiss_bp.q.set_value(2.2);
        let hg = gain(ctx, 0.0);
        let src = self.loop_src(ctx, white, 1.0);
        link(&src, &hiss_bp);
        link(&hiss_bp, &hg);
        link(&hg, &out);
        w1.frequency.set_value(2000.0);
        w2.frequency.set_value(3000.0);
        let _ = w1.start();
        let _ = w2.start();
        keep.extend([n(&w2g), n(&src)]);
        let gid = self.add_gate(
            vec![(n(&out), n(sfx_bus))],
            GATE_TAIL,
            vec![out.gain.clone()],
        );
        self.boost = 0.0;
        (
            Turbo {
                out,
                w1,
                w2,
                wg,
                hiss_bp,
                hg,
            },
            gid,
        )
    }

    // Electric drive: the motor's whine (a fundamental that follows motor
    // speed plus a detuned upper partial), a resonant sawtooth "jet" layer, a
    // gear-mesh whine from the reduction gear, fan/air noise, a separate
    // higher whirr under regen and a growl under boost. A faint hum when
    // stopped says the car is on.
    fn build_electric(
        &mut self,
        ctx: &AudioContext,
        sfx_bus: &GainNode,
        (_, pink, _): &(AudioBuffer, AudioBuffer, AudioBuffer),
        keep: &mut Vec<Node>,
    ) -> (Ev, GateId) {
        let out = gain(ctx, 0.0);
        let hp = biquad(ctx, B::Highpass);
        hp.frequency.set_value(45.0);
        hp.q.set_value(0.7);
        let sum = gain(ctx, 1.0);
        link(&sum, &hp);
        link(&hp, &out);
        link(&out, sfx_bus);
        // A slow shared vibrato so the tones never sound perfectly static.
        let vib = ctx.create_oscillator();
        vib.frequency.set_value(4.3);
        let vib_g = gain(ctx, 4.0); // cents
        link(&vib, &vib_g);
        let _ = vib.start();
        let tone = |t: OscillatorType| -> Tone {
            let o = osc(ctx, t);
            let g = gain(ctx, 0.0);
            link(&vib_g, &o.detune);
            link(&o, &g);
            let _ = o.start();
            Tone { o, g }
        };
        let f1 = tone(OscillatorType::Sine);
        let f2 = tone(OscillatorType::Sine);
        let f3 = tone(OscillatorType::Triangle);
        let mesh = tone(OscillatorType::Sine);
        let regen = tone(OscillatorType::Triangle);
        for k in [&f1, &f2, &f3, &mesh] {
            link(&k.g, &sum);
        }
        let regen_lp = biquad(ctx, B::Lowpass);
        regen_lp.frequency.set_value(3000.0);
        link(&regen.g, &regen_lp);
        link(&regen_lp, &sum);
        let saw = tone(OscillatorType::Sawtooth);
        let saw_lp = biquad(ctx, B::Lowpass);
        saw_lp.q.set_value(7.0);
        saw_lp.frequency.set_value(400.0);
        link(&saw.g, &saw_lp);
        link(&saw_lp, &sum);
        let growl = tone(OscillatorType::Square);
        let growl_bp = biquad(ctx, B::Bandpass);
        growl_bp.q.set_value(1.8);
        link(&growl.g, &growl_bp);
        link(&growl_bp, &sum);
        let hum = tone(OscillatorType::Sine);
        hum.o.frequency.set_value(100.0);
        link(&hum.g, &sum);
        let hum2 = tone(OscillatorType::Sine);
        hum2.o.frequency.set_value(200.0);
        link(&hum2.g, &sum);
        let air_bp = biquad(ctx, B::Bandpass);
        air_bp.q.set_value(2.5);
        air_bp.frequency.set_value(800.0);
        let air_g = gain(ctx, 0.0);
        let src = self.loop_src(ctx, pink, 1.0);
        link(&src, &air_bp);
        link(&air_bp, &air_g);
        link(&air_g, &sum);
        keep.extend([n(&hp), n(&sum), n(&vib), n(&vib_g), n(&regen_lp), n(&src)]);
        let gid = self.add_gate(
            vec![(n(&out), n(sfx_bus))],
            GATE_TAIL,
            vec![out.gain.clone()],
        );
        (
            Ev {
                out,
                f1,
                f2,
                f3,
                mesh,
                regen,
                saw,
                saw_lp,
                growl,
                growl_bp,
                hum,
                hum2,
                air_bp,
                air_g,
            },
            gid,
        )
    }

    /// `mk(buf, type, freq, q, dest)`: a looped bed through a filter into a
    /// silent gain.
    #[allow(clippy::too_many_arguments)]
    fn bed(
        &mut self,
        ctx: &AudioContext,
        buf: &AudioBuffer,
        t: BiquadFilterType,
        freq: f64,
        q: Option<f64>,
        dest: &Node,
    ) -> Bed {
        let src = self.loop_src(ctx, buf, 1.0);
        let f = biquad(ctx, t);
        f.frequency.set_value(freq);
        if let Some(q) = q {
            f.q.set_value(q);
        }
        let g = gain(ctx, 0.0);
        link(&src, &f);
        link(&f, &g);
        link(&g, dest);
        Bed { src, f, g }
    }

    /// `wobble(hz, depth)`: slow random wobble (brown noise, low-passed) for
    /// gusts and chatter.
    fn wobble(
        &mut self,
        ctx: &AudioContext,
        brown: &AudioBuffer,
        hz: f64,
        depth: f64,
        keep: &mut Vec<Node>,
    ) -> GainNode {
        let lp = biquad(ctx, B::Lowpass);
        lp.frequency.set_value(hz);
        let g = gain(ctx, depth);
        let rate = 0.5 + self.random();
        let src = self.loop_src(ctx, brown, rate);
        link(&src, &lp);
        link(&lp, &g);
        keep.extend([n(&lp), n(&src), n(&g)]);
        g
    }

    fn build_environment(
        &mut self,
        ctx: &AudioContext,
        sfx_bus: &GainNode,
        (white, pink, brown): &(AudioBuffer, AudioBuffer, AudioBuffer),
        sfx_buf: &[(&'static str, AudioBuffer)],
        keep: &mut Vec<Node>,
    ) -> EnvParts {
        let bus = n(sfx_bus);
        let pan = |v: f64| {
            let p = ctx.create_stereo_panner();
            p.pan.set_value(v);
            p
        };
        // Wind: a low roar in the middle plus two gusting upper bands, one each
        // side, so the air moves around you at speed.
        let wind = self.bed(ctx, pink, B::Lowpass, 500.0, Some(0.5), &bus);
        let mut wind_hi = Vec::new();
        for pv in [-0.6, 0.6] {
            let p = pan(pv);
            link(&p, sfx_bus);
            let w = self.bed(ctx, pink, B::Bandpass, 1400.0, Some(0.9), &n(&p));
            let gust = gain(ctx, 1.0);
            w.g.disconnect();
            link(&w.g, &gust);
            link(&gust, &p);
            let wob = self.wobble(ctx, brown, 0.45, 0.45, keep);
            link(&wob, &gust.gain);
            keep.push(n(&gust));
            wind_hi.push(WindSide { bed: w, p });
        }
        let rumble = self.bed(ctx, brown, B::Lowpass, 130.0, Some(0.7), &bus);

        // Tyre squeal: a jittery tone (the rubber stick-slip) plus a narrow noise
        // band at the same pitch, chattering in level. Pitch climbs with slip.
        let out = gain(ctx, 0.0);
        let am = gain(ctx, 0.75);
        let wob = self.wobble(ctx, brown, 28.0, 0.5, keep);
        link(&wob, &am.gain);
        link(&am, &out);
        link(&out, sfx_bus);
        let o1 = osc(ctx, OscillatorType::Triangle);
        o1.frequency.set_value(950.0);
        let o2 = osc(ctx, OscillatorType::Sine);
        o2.frequency.set_value(1930.0);
        let jit = self.wobble(ctx, brown, 45.0, 70.0, keep); // cents
        link(&jit, &o1.detune);
        link(&jit, &o2.detune);
        let tone = gain(ctx, 0.5);
        let o2g = gain(ctx, 0.35);
        let tbp = biquad(ctx, B::Bandpass);
        tbp.q.set_value(2.0);
        tbp.frequency.set_value(1100.0);
        link(&o1, &tbp);
        link(&o2, &o2g);
        link(&o2g, &tbp);
        link(&tbp, &tone);
        link(&tone, &am);
        let _ = o1.start();
        let _ = o2.start();
        let nz = self.loop_src(ctx, white, 1.0);
        let nbp = biquad(ctx, B::Bandpass);
        nbp.q.set_value(9.0);
        nbp.frequency.set_value(1000.0);
        let nbp2 = biquad(ctx, B::Bandpass);
        nbp2.q.set_value(2.5);
        nbp2.frequency.set_value(2100.0);
        let noise_g = gain(ctx, 1.4);
        link(&nz, &nbp);
        link(&nz, &nbp2);
        link(&nbp, &noise_g);
        link(&nbp2, &noise_g);
        link(&noise_g, &am);
        keep.extend([n(&am), n(&o2g), n(&nz), n(&noise_g)]);
        let squeal = Squeal {
            out,
            o1,
            o2,
            tone,
            tbp,
            nbp,
            nbp2,
        };

        // Gravel off the tarmac: a looped bed of stone clicks, played faster
        // with speed, over extra low rumble.
        let gr = sfx_buf
            .iter()
            .find(|(k, _)| *k == "gravel")
            .map(|(_, b)| b.clone())
            .expect("the gravel loop");
        let gsrc = self.loop_src(ctx, &gr, 1.0);
        let gg = ctx.create_gain();
        let gf = ctx.create_biquad_filter();
        gg.gain.set_value(0.0);
        gf.set_type(B::Highshelf);
        gf.frequency.set_value(2500.0);
        gf.gain.set_value(-4.0);
        link(&gsrc, &gf);
        link(&gf, &gg);
        link(&gg, sfx_bus);
        let gravel = Bed {
            src: gsrc,
            f: gf,
            g: gg,
        };

        // Wall scrape: grinding metal (pre-rendered loop) plus a gritty band, panned to the wall side.
        let sc_pan = pan(0.0);
        link(&sc_pan, sfx_bus);
        let sc_g = gain(ctx, 0.0);
        link(&sc_g, &sc_pan);
        let scb = sfx_buf
            .iter()
            .find(|(k, _)| *k == "scrape")
            .map(|(_, b)| b.clone())
            .expect("the scrape loop");
        let sc_src = self.loop_src(ctx, &scb, 1.0);
        link(&sc_src, &sc_g);
        let grit = self.bed(ctx, white, B::Bandpass, 2600.0, Some(0.8), &n(&sc_pan));
        let scrape = Scrape {
            pan: sc_pan,
            g: sc_g,
            src: sc_src,
            grit,
        };

        // Nitro: hiss, a low rumble and a fluttering flame roar.
        let nitro_hiss = self.bed(ctx, white, B::Highpass, 3200.0, Some(0.7), &bus);
        let nitro_rumble = self.bed(ctx, brown, B::Lowpass, 90.0, Some(1.2), &bus);
        let nitro_flame = self.bed(ctx, pink, B::Bandpass, 420.0, Some(0.8), &bus);
        let fl = ctx.create_oscillator();
        fl.frequency.set_value(23.0);
        let flg = gain(ctx, 0.0);
        link(&fl, &flg);
        link(&flg, &nitro_flame.g.gain);
        let _ = fl.start();
        keep.push(n(&fl));

        // Road noise: wind, the gusting side bands and the tyre rumble.
        let mut links = vec![(n(&wind.g), bus.clone())];
        links.extend(wind_hi.iter().map(|w| (n(&w.p), bus.clone())));
        links.push((n(&rumble.g), bus.clone()));
        let mut levels = vec![wind.g.gain.clone()];
        levels.extend(wind_hi.iter().map(|w| w.bed.g.gain.clone()));
        levels.push(rumble.g.gain.clone());
        let g_road = self.add_gate(links, GATE_TAIL, levels);
        let g_squeal = self.add_gate(
            vec![(n(&squeal.out), bus.clone())],
            GATE_TAIL,
            vec![squeal.out.gain.clone()],
        );
        let g_gravel = self.add_gate(
            vec![(n(&gravel.g), bus.clone())],
            GATE_TAIL,
            vec![gravel.g.gain.clone()],
        );
        let g_scrape = self.add_gate(
            vec![(n(&scrape.pan), bus.clone())],
            GATE_TAIL,
            vec![scrape.g.gain.clone(), scrape.grit.g.gain.clone()],
        );
        let nitro = [&nitro_hiss, &nitro_rumble, &nitro_flame];
        let g_nitro = self.add_gate(
            nitro.iter().map(|b| (n(&b.g), bus.clone())).collect(),
            1.6,
            nitro.iter().map(|b| b.g.gain.clone()).collect(),
        );
        EnvParts {
            wind,
            wind_hi,
            rumble,
            squeal,
            gravel,
            scrape,
            nitro_hiss,
            nitro_rumble,
            nitro_flame,
            nitro_flutter: flg,
            g_road,
            g_squeal,
            g_gravel,
            g_scrape,
            g_nitro,
        }
    }

    fn build_rivals(&mut self, ctx: &AudioContext, sfx_bus: &GainNode) -> Vec<RivalVoice> {
        let mut out = Vec::new();
        for i in 0..3 {
            let o = ctx.create_oscillator();
            let lp = biquad(ctx, B::Lowpass);
            lp.frequency.set_value(800.0);
            lp.q.set_value(1.0);
            let g = gain(ctx, 0.0);
            link(&o, &lp);
            link(&lp, &g);
            let p = ctx.create_stereo_panner();
            link(&g, &p);
            link(&p, sfx_bus);
            o.frequency.set_value(30.0);
            o.detune.set_value((i as f64 - 1.0) * 13.0);
            let _ = o.start();
            let gate = self.add_gate(vec![(n(&p), n(sfx_bus))], GATE_TAIL, vec![g.gain.clone()]);
            out.push(RivalVoice {
                osc: o,
                lp,
                g,
                p,
                gate,
            });
        }
        out
    }

    // Hot Pursuit: three siren voices, the shredded-tyre flap, engine distress
    // for a damaged car and the police radio's bus. All of it idles at zero gain.
    fn build_pursuit(
        &mut self,
        ctx: &AudioContext,
        sfx_bus: &GainNode,
        (white, pink, _): &(AudioBuffer, AudioBuffer, AudioBuffer),
        keep: &mut Vec<Node>,
    ) -> PursuitParts {
        let pulse = pulse_wave(24);
        let pulse_w = ctx
            .create_periodic_wave(&pulse.wave.real, &pulse.wave.imag, Some(true))
            .expect("the pulse wave");
        let sqw = soft_square_wave(15);
        let square = ctx
            .create_periodic_wave(&sqw.real, &sqw.imag, None)
            .expect("the soft square");

        // Siren voice: square + detuned saw through a horn-speaker band-pass, a
        // distance low-pass, gain and pan. The pattern is two always-running
        // pitch LFOs on the oscillators' detune (a triangle for wail/yelp, a soft
        // square for hi-lo) crossfaded by depth, so a voice's pattern never
        // restarts: switching mode only changes LFO rates and depths. Each voice's
        // cycle runs a little long or short so several units drift apart.
        let mut sirens = Vec::new();
        self.sirens.clear();
        let wail = siren_pattern("wail").expect("wail");
        let hilo = siren_pattern("hilo").expect("hilo");
        for i in 0..3 {
            let drift = 1.0 + (i as f64 - 1.0) * 0.045;
            let a = osc(ctx, OscillatorType::Square);
            let b = osc(ctx, OscillatorType::Sawtooth);
            b.detune.set_value(18.0);
            let ag = gain(ctx, 0.55);
            let bg = gain(ctx, 0.45);
            let bp = biquad(ctx, B::Bandpass);
            bp.frequency.set_value(1150.0);
            bp.q.set_value(0.9);
            let lp = biquad(ctx, B::Lowpass);
            lp.frequency.set_value(6000.0);
            lp.q.set_value(0.6);
            let g = gain(ctx, 0.0);
            link(&a, &ag);
            link(&b, &bg);
            link(&ag, &bp);
            link(&bg, &bp);
            link(&bp, &lp);
            link(&lp, &g);
            let p = ctx.create_stereo_panner();
            link(&g, &p);
            link(&p, sfx_bus);
            let gate = self.add_gate(vec![(n(&p), n(sfx_bus))], GATE_TAIL, vec![g.gain.clone()]);
            let tri = osc(ctx, OscillatorType::Triangle);
            tri.frequency.set_value(1.0 / (wail.period * drift));
            let sq = ctx.create_oscillator();
            sq.set_periodic_wave(&square);
            sq.frequency.set_value(1.0 / (hilo.period * drift));
            let tri_d = gain(ctx, 0.0);
            let sq_d = gain(ctx, 0.0);
            link(&tri, &tri_d);
            link(&sq, &sq_d);
            for d in [&tri_d, &sq_d] {
                link(d, &a.detune);
                link(d, &b.detune);
            }
            // `v.a.frequency.value = v.b.frequency.value = x`: b first.
            let f = (wail.lo * wail.hi).sqrt();
            b.frequency.set_value(f);
            a.frequency.set_value(f);
            for o in [&a, &b, &tri, &sq] {
                let _ = o.start();
            }
            keep.extend([n(&ag), n(&bg), n(&bp)]);
            self.sirens.push(SirenState {
                id: None,
                mode: "off".into(),
                drift,
            });
            sirens.push(SirenVoice {
                a,
                b,
                lp,
                g,
                p,
                gate,
                tri,
                sq,
                tri_d,
                sq_d,
            });
        }

        // Spiked tyres: flapping rubber strips. A pulse train at the flap rate
        // gates a thwacking noise band and carries a low thump of its own.
        let tout = gain(ctx, 0.0);
        link(&tout, sfx_bus);
        let tpulse = ctx.create_oscillator();
        tpulse.set_periodic_wave(&pulse_w);
        tpulse.frequency.set_value(8.0);
        let tam = gain(ctx, -pulse.floor);
        link(&tpulse, &tam.gain);
        let ty_bp = biquad(ctx, B::Bandpass);
        ty_bp.frequency.set_value(340.0);
        ty_bp.q.set_value(1.2);
        let tsrc = self.loop_src(ctx, pink, 1.0);
        link(&tsrc, &ty_bp);
        link(&ty_bp, &tam);
        link(&tam, &tout);
        let th_lp = biquad(ctx, B::Lowpass);
        th_lp.frequency.set_value(160.0);
        let th_g = gain(ctx, 0.35);
        link(&tpulse, &th_lp);
        link(&th_lp, &th_g);
        link(&th_g, &tout);
        let _ = tpulse.start();
        keep.extend([n(&tam), n(&ty_bp), n(&tsrc), n(&th_lp), n(&th_g)]);
        let g_tyres = self.add_gate(
            vec![(n(&tout), n(sfx_bus))],
            GATE_TAIL,
            vec![tout.gain.clone()],
        );
        let tyres = Tyres {
            out: tout,
            pulse: tpulse,
        };

        // Damage: rod knock (a pulse at crank rate ringing a metallic band), a
        // loose-panel rattle gated by the same pulse, and a steam hiss. Misfires
        // dip eng.misG from update().
        let dout = gain(ctx, 0.0);
        link(&dout, sfx_bus);
        let dpulse = ctx.create_oscillator();
        dpulse.set_periodic_wave(&pulse_w);
        dpulse.frequency.set_value(12.0);
        let k_bp = biquad(ctx, B::Bandpass);
        k_bp.frequency.set_value(1300.0);
        k_bp.q.set_value(5.0);
        let k_bp2 = biquad(ctx, B::Bandpass);
        k_bp2.frequency.set_value(420.0);
        k_bp2.q.set_value(3.0);
        let knock = gain(ctx, 0.0);
        link(&dpulse, &k_bp);
        link(&dpulse, &k_bp2);
        link(&k_bp, &knock);
        link(&k_bp2, &knock);
        link(&knock, &dout);
        let r_am = gain(ctx, -pulse.floor);
        link(&dpulse, &r_am.gain);
        let r_bp = biquad(ctx, B::Bandpass);
        r_bp.frequency.set_value(2300.0);
        r_bp.q.set_value(1.6);
        let rattle = gain(ctx, 0.0);
        let rsrc = self.loop_src(ctx, white, 1.0);
        link(&rsrc, &r_bp);
        link(&r_bp, &r_am);
        link(&r_am, &rattle);
        link(&rattle, &dout);
        let s_hp = biquad(ctx, B::Highpass);
        s_hp.frequency.set_value(3800.0);
        s_hp.q.set_value(0.7);
        let steam = gain(ctx, 0.0);
        let ssrc = self.loop_src(ctx, white, 0.9);
        link(&ssrc, &s_hp);
        link(&s_hp, &steam);
        link(&steam, sfx_bus);
        let _ = dpulse.start();
        keep.extend([
            n(&k_bp),
            n(&k_bp2),
            n(&r_am),
            n(&r_bp),
            n(&rsrc),
            n(&s_hp),
            n(&ssrc),
        ]);
        let g_dmg = self.add_gate(
            vec![(n(&dout), n(sfx_bus)), (n(&steam), n(sfx_bus))],
            GATE_TAIL,
            vec![dout.gain.clone(), steam.gain.clone()],
        );
        self.damage = 0.0;
        self.misfire_t = 0.0;
        let dmg = Dmg {
            out: dout,
            pulse: dpulse,
            knock,
            rattle,
            steam,
        };

        // Radio bus: crunch, then the handset's 300–3000 Hz band.
        let rin = gain(ctx, 1.6);
        let sh: WaveShaperNode = ctx.create_wave_shaper();
        sh.set_curve(Some(&distortion_curve(2.6, 1024)));
        let mut band = Vec::new();
        for (t, f) in [
            (B::Highpass, 340.0),
            (B::Highpass, 340.0),
            (B::Lowpass, 3000.0),
            (B::Lowpass, 3000.0),
        ] {
            let b = biquad(ctx, t);
            b.frequency.set_value(f);
            b.q.set_value(0.7);
            band.push(b);
        }
        let rout = gain(ctx, 0.22);
        link(&rin, &sh);
        link(&sh, &band[0]);
        link(&band[0], &band[1]);
        link(&band[1], &band[2]);
        link(&band[2], &band[3]);
        link(&band[3], &rout);
        let rp = ctx.create_stereo_panner();
        link(&rout, &rp);
        link(&rp, sfx_bus);
        keep.push(n(&sh));
        keep.extend(band.iter().map(n));
        keep.push(n(&rout));
        let g_radio = self.add_gate(vec![(n(&rp), n(sfx_bus))], GATE_TAIL, vec![]);
        self.radio_cur = None;
        self.mood = "off";
        self.mood_to = 20000.0;
        PursuitParts {
            sirens,
            tyres,
            dmg,
            radio_bus: RadioBus { input: rin, p: rp },
            g_tyres,
            g_dmg,
            g_radio,
        }
    }
}

pub(crate) struct EnvParts {
    wind: Bed,
    wind_hi: Vec<WindSide>,
    rumble: Bed,
    squeal: Squeal,
    gravel: Bed,
    scrape: Scrape,
    nitro_hiss: Bed,
    nitro_rumble: Bed,
    nitro_flame: Bed,
    nitro_flutter: GainNode,
    g_road: GateId,
    g_squeal: GateId,
    g_gravel: GateId,
    g_scrape: GateId,
    g_nitro: GateId,
}

pub(crate) struct PursuitParts {
    sirens: Vec<SirenVoice>,
    tyres: Tyres,
    dmg: Dmg,
    radio_bus: RadioBus,
    g_tyres: GateId,
    g_dmg: GateId,
    g_radio: GateId,
}
