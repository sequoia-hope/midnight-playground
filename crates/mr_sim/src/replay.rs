//! A race from the client's run recording (`docs/rust-port/RECORDING.md`),
//! to replay: the options it was started with and every tick's input, as
//! the client's session fed them to [`crate::race::step`], with the state's
//! hash at checkpoints. The simulation is deterministic, so stepping a new
//! [`SimState`] with the same inputs gives the same race, tick for tick;
//! the hashes say whether it did.
//!
//! The recording is JSON lines; this reads only the two kinds a replay
//! needs (`race_start` and `ticks`), whose replay fields are flat and come
//! first, with a small reader of its own (the simulation has no JSON
//! dependency). The inputs are a run-length text ([`encode`]).

use crate::input::InputFrame;
use crate::race::{LevelRuntime, RaceOpts, SimState};

/// One recorded race (a restart is a race of its own).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recorded {
    /// The client's race number (`Race::starts`).
    pub race: u32,
    pub level: String,
    pub car: String,
    pub seed: u32,
    pub pursuit: bool,
    pub heat: f64,
    /// Hot Pursuit's cap on units (`?cops=`), set on the field as the client
    /// does (`flow::apply_pursuit_opts`).
    pub cops: f64,
    pub flash: bool,
    /// Every tick's input from tick 1, in order.
    pub inputs: Vec<InputFrame>,
    /// (tick, `race::hash` of the state after it).
    pub checks: Vec<(u32, u64)>,
    /// Ticks the recording says it missed (the inputs no longer line up).
    pub gaps: u32,
}

impl Recorded {
    /// The race's options, for [`SimState::new`].
    pub fn opts(&self) -> Result<RaceOpts, String> {
        let car = crate::physics::CAR_SPECS
            .iter()
            .map(|(k, _)| *k)
            .find(|k| *k == self.car)
            .ok_or_else(|| format!("race {}: no car {}", self.race, self.car))?;
        Ok(RaceOpts {
            car,
            seed: self.seed,
            pursuit: self.pursuit,
            heat: self.heat,
        })
    }

    /// The field on the grid, with the client's pursuit options.
    pub fn start(&self, lr: &LevelRuntime) -> Result<SimState, String> {
        let mut st = SimState::new(lr, self.opts()?);
        apply_pursuit_opts(&mut st, self.cops, self.flash);
        Ok(st)
    }
}

/// `new Pursuit({ maxUnits: cops, flash })` (the client's
/// `flow::apply_pursuit_opts`).
pub fn apply_pursuit_opts(st: &mut SimState, cops: f64, flash: bool) {
    if let Some(pv) = st.pv.as_mut() {
        pv.pursuit.max_units = mr_math::clamp(cops, 0.0, 6.0) as usize;
        pv.pursuit.flash = flash;
    }
}

/// Appends `frames` as text: `steer,throttle,brake,flags` per tick,
/// separated by spaces, a run of equal frames as one with `*n`.
pub fn encode(frames: &[InputFrame], out: &mut String) {
    use std::fmt::Write;
    let mut i = 0;
    while i < frames.len() {
        let f = frames[i];
        let mut n = 1;
        while i + n < frames.len() && frames[i + n] == f {
            n += 1;
        }
        if i > 0 {
            out.push(' ');
        }
        let _ = write!(out, "{},{},{},{}", f.steer, f.throttle, f.brake, f.flags);
        if n > 1 {
            let _ = write!(out, "*{n}");
        }
        i += n;
    }
}

/// [`encode`]'s text back into frames.
pub fn decode(s: &str, out: &mut Vec<InputFrame>) -> Result<(), String> {
    for item in s.split_whitespace() {
        let (f, n) = match item.split_once('*') {
            Some((f, n)) => (
                f,
                n.parse::<usize>().map_err(|_| format!("bad run {item}"))?,
            ),
            None => (item, 1),
        };
        let mut p = f.split(',');
        let mut next = || p.next().ok_or_else(|| format!("bad input {item}"));
        let bad = |_| format!("bad input {item}");
        let frame = InputFrame {
            steer: next()?.parse().map_err(bad)?,
            throttle: next()?.parse().map_err(bad)?,
            brake: next()?.parse().map_err(bad)?,
            flags: next()?.parse().map_err(bad)?,
        };
        out.extend(std::iter::repeat_n(frame, n));
    }
    Ok(())
}

/// The raw text of `"key":` 's value in a flat JSON line: a string's
/// contents (no escapes are expected in these fields), or a number's or
/// literal's text.
pub fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let pat = ["\"", key, "\":"].concat();
    let at = line.find(&pat)? + pat.len();
    let rest = line[at..].trim_start();
    if let Some(s) = rest.strip_prefix('"') {
        return Some(&s[..s.find('"')?]);
    }
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    Some(rest[..end].trim())
}

fn num<T: std::str::FromStr>(line: &str, key: &str) -> Result<T, String> {
    let v = field(line, key).ok_or_else(|| format!("no {key} in {line}"))?;
    v.parse().map_err(|_| format!("bad {key} {v}"))
}

/// The races of a recording, in order.
pub fn parse(text: &str) -> Result<Vec<Recorded>, String> {
    let mut races: Vec<Recorded> = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let at = |e: String| format!("line {}: {e}", i + 1);
        match field(line, "type") {
            Some("race_start") => races.push(Recorded {
                race: num(line, "race").map_err(at)?,
                level: field(line, "level").unwrap_or_default().to_owned(),
                car: field(line, "car").unwrap_or_default().to_owned(),
                seed: num(line, "seed").map_err(at)?,
                pursuit: field(line, "pursuit") == Some("true"),
                heat: num(line, "heat").map_err(at)?,
                cops: num(line, "cops").map_err(at)?,
                flash: field(line, "flash") != Some("false"),
                ..Recorded::default()
            }),
            Some("ticks") => {
                let race: u32 = num(line, "race").map_err(at)?;
                let Some(r) = races.iter_mut().rev().find(|r| r.race == race) else {
                    return Err(at(format!("ticks of race {race} before its start")));
                };
                let from: u32 = num(line, "from").map_err(at)?;
                if from as usize != r.inputs.len() + 1 {
                    r.gaps += 1;
                }
                decode(field(line, "in").unwrap_or_default(), &mut r.inputs).map_err(at)?;
                let tick: u32 = num(line, "tick").map_err(at)?;
                if let Some(h) = field(line, "hash") {
                    let h = u64::from_str_radix(h, 16).map_err(|_| at(format!("bad hash {h}")))?;
                    r.checks.push((tick, h));
                }
            }
            _ => {}
        }
    }
    Ok(races)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_round_trip_with_runs() {
        let a = InputFrame {
            steer: -1200,
            throttle: 255,
            brake: 0,
            flags: 4,
        };
        let frames = vec![InputFrame::default(), a, a, a, InputFrame::default()];
        // Two lines' worth: a run does not cross a line.
        let (mut a, mut b) = (String::new(), String::new());
        encode(&frames[..2], &mut a);
        encode(&frames[2..], &mut b);
        assert_eq!(a, "0,0,0,0 -1200,255,0,4");
        assert_eq!(b, "-1200,255,0,4*2 0,0,0,0");
        let mut back = Vec::new();
        decode(&a, &mut back).unwrap();
        decode(&b, &mut back).unwrap();
        assert_eq!(back, frames);
    }

    #[test]
    fn reads_the_flat_fields() {
        let text = concat!(
            "{\"t\":0.1,\"type\":\"start\"}\n",
            "{\"t\":1.0,\"type\":\"race_start\",\"race\":2,\"level\":\"coast\",\"car\":\"super\",",
            "\"seed\":7,\"pursuit\":false,\"heat\":1,\"cops\":6,\"flash\":true}\n",
            "{\"t\":2.0,\"type\":\"ticks\",\"race\":2,\"from\":1,\"tick\":3,\"hash\":\"00ff\",",
            "\"in\":\"0,0,0,0*3\",\"p\":{\"s\":1}}\n"
        );
        let r = parse(text).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].race, r[0].level.as_str(), r[0].seed), (2, "coast", 7));
        assert_eq!(r[0].inputs.len(), 3);
        assert_eq!(r[0].checks, vec![(3, 0xff)]);
        assert_eq!(r[0].gaps, 0);
    }
}
