//! Random controls for the fuzz runs (SPEC 4.6 test 5): `fuzzer(seed)` in
//! `src/parity/sim.js`, from mulberry32(seed). Every 30 ticks (a quarter
//! second) a new set, held until the next. The draws, in order: steer, then
//! throttle (a 70 % chance of a random amount), brake (20 %), handbrake
//! (10 %), nitro (20 %).

use mp_math::Mulberry32;

use crate::input::Input;

#[derive(Clone, Debug, PartialEq)]
pub struct Fuzzer {
    rng: Mulberry32,
    k: u64,
    cur: Input,
}

impl Fuzzer {
    pub fn new(seed: u32) -> Fuzzer {
        Fuzzer {
            rng: Mulberry32::new(seed),
            k: 0,
            cur: Input::default(),
        }
    }

    /// The controls for the next tick, over `inp` (`{ ...inp, ...cur }`).
    pub fn next(&mut self, inp: Input) -> Input {
        if self.k.is_multiple_of(30) {
            let r = &mut self.rng;
            let steer = r.next_f64() * 2.0 - 1.0;
            let throttle = if r.next_f64() < 0.7 {
                r.next_f64()
            } else {
                0.0
            };
            let brake = if r.next_f64() < 0.2 {
                r.next_f64()
            } else {
                0.0
            };
            let handbrake = r.next_f64() < 0.1;
            let nitro = r.next_f64() < 0.2;
            self.cur = Input {
                steer,
                throttle,
                brake,
                handbrake,
                nitro,
                ..Input::default()
            };
        }
        self.k += 1;
        Input {
            steer: self.cur.steer,
            throttle: self.cur.throttle,
            brake: self.cur.brake,
            handbrake: self.cur.handbrake,
            nitro: self.cur.nitro,
            ..inp
        }
    }
}
