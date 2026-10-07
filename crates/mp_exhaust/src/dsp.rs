//! The model's building blocks (`worklet.js` `Biquad`, `OnePole`, `Delay`,
//! `Guide`, `Rng`), line for line.

pub const TAU: f64 = std::f64::consts::PI * 2.0;

pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Lowpass,
    Highpass,
    Bandpass,
    Peaking,
    Lowshelf,
    Highshelf,
}

/// A biquad from the RBJ cookbook (`bandpass` is the 0 dB peak one).
#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Default for Biquad {
    fn default() -> Self {
        Biquad {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            z1: 0.0,
            z2: 0.0,
        }
    }
}

impl Biquad {
    pub fn new(kind: Kind, f: f64, q: f64, db: f64, sr: f64) -> Biquad {
        let mut b = Biquad::default();
        b.set(kind, f, q, db, sr);
        b
    }

    pub fn set(&mut self, kind: Kind, f: f64, q: f64, db: f64, sr: f64) {
        let f = clamp(f, 5.0, sr * 0.45);
        let w = (TAU * f) / sr;
        let (cs, sn) = (w.cos(), w.sin());
        let al = sn / (2.0 * q);
        let a = 10f64.powf(db / 40.0);
        let (b0, b1, b2, a0, a1, a2);
        match kind {
            Kind::Lowpass => {
                b0 = (1.0 - cs) / 2.0;
                b1 = 1.0 - cs;
                b2 = b0;
                a0 = 1.0 + al;
                a1 = -2.0 * cs;
                a2 = 1.0 - al;
            }
            Kind::Highpass => {
                b0 = (1.0 + cs) / 2.0;
                b1 = -(1.0 + cs);
                b2 = b0;
                a0 = 1.0 + al;
                a1 = -2.0 * cs;
                a2 = 1.0 - al;
            }
            Kind::Bandpass => {
                b0 = al;
                b1 = 0.0;
                b2 = -al;
                a0 = 1.0 + al;
                a1 = -2.0 * cs;
                a2 = 1.0 - al;
            }
            Kind::Peaking => {
                b0 = 1.0 + al * a;
                b1 = -2.0 * cs;
                b2 = 1.0 - al * a;
                a0 = 1.0 + al / a;
                a1 = -2.0 * cs;
                a2 = 1.0 - al / a;
            }
            Kind::Lowshelf | Kind::Highshelf => {
                let s = if kind == Kind::Lowshelf { 1.0 } else { -1.0 };
                let r = 2.0 * a.sqrt() * (sn / 2.0) * std::f64::consts::SQRT_2;
                b0 = a * ((a + 1.0) - s * (a - 1.0) * cs + r);
                b1 = 2.0 * s * a * ((a - 1.0) - s * (a + 1.0) * cs);
                b2 = a * ((a + 1.0) - s * (a - 1.0) * cs - r);
                a0 = (a + 1.0) + s * (a - 1.0) * cs + r;
                a1 = -2.0 * s * ((a - 1.0) + s * (a + 1.0) * cs);
                a2 = (a + 1.0) + s * (a - 1.0) * cs - r;
            }
        }
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn run(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

#[derive(Clone, Debug)]
pub struct OnePole {
    pub y: f64,
    a: f64,
}

impl OnePole {
    pub fn new(hz: f64, sr: f64) -> OnePole {
        let mut p = OnePole { y: 0.0, a: 0.0 };
        p.set_hz(hz, sr);
        p
    }

    pub fn set_hz(&mut self, hz: f64, sr: f64) {
        self.a = 1.0 - ((-TAU * hz) / sr).exp();
    }

    #[inline]
    pub fn run(&mut self, x: f64) -> f64 {
        self.y += self.a * (x - self.y);
        self.y
    }
}

/// A circular delay line read at a fractional distance (linear
/// interpolation). Its buffer is a `Float32Array` in the JS: stores round.
#[derive(Clone, Debug)]
pub struct Delay {
    buf: Vec<f32>,
    mask: usize,
    w: usize,
}

impl Delay {
    pub fn new(n: usize) -> Delay {
        Delay {
            buf: vec![0.0; n],
            mask: n - 1,
            w: 0,
        }
    }

    #[inline]
    pub fn push(&mut self, x: f64) {
        self.buf[self.w] = x as f32;
        self.w = (self.w + 1) & self.mask;
    }

    /// `d = 0` is the sample pushed last.
    #[inline]
    pub fn read(&self, d: f64) -> f64 {
        let p = self.w as f64 - 1.0 - d;
        let i = p.floor();
        let fr = p - i;
        let i = i as i64;
        let a = self.buf[(i & self.mask as i64) as usize] as f64;
        let b = self.buf[((i + 1) & self.mask as i64) as usize] as f64;
        a + (b - a) * fr
    }
}

/// A one-dimensional waveguide: the wave enters at the near end and arrives
/// at the far end `len` samples later; the far end reflects `r_far` of it
/// (through the loop loss), which travels back and reflects `r_near` at the
/// near end. Returns the wave arriving at the far end.
#[derive(Clone, Debug)]
pub struct Guide {
    d: Delay,
    pub loss: OnePole,
    pub len: f64,
}

impl Guide {
    pub fn new(sr: f64) -> Guide {
        Guide {
            d: Delay::new(8192),
            loss: OnePole::new(3000.0, sr),
            len: 40.0,
        }
    }

    #[inline]
    pub fn step(&mut self, x: f64, r_far: f64, r_near: f64) -> f64 {
        let l = self.len;
        let arrive = self.d.read(l - 1.0);
        let back = self.d.read(2.0 * l - 1.0);
        let fb = self.loss.run(r_far * back);
        self.d.push(x + r_near * fb);
        arrive
    }
}

/// mulberry32, as the lab draws it.
#[derive(Clone, Debug)]
pub struct Rng {
    s: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng {
            s: if seed == 0 { 1 } else { seed },
        }
    }

    // The lab's name (`r.next()`); it is not an iterator.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.s = self.s.wrapping_add(0x6d2b_79f5);
        let mut t = self.s;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        (t ^ (t >> 14)) as f64 / 4_294_967_296.0
    }

    pub fn bi(&mut self) -> f64 {
        self.next() * 2.0 - 1.0
    }

    /// About N(0, 1).
    pub fn gauss(&mut self) -> f64 {
        let s = self.next() + self.next() + self.next() + self.next();
        (s - 2.0) * 1.732
    }
}
