//! The wire protocol (SPEC 9.3): binary, little-endian, versioned messages,
//! hand-encoded so the format is exactly what is written here and any
//! malformed or truncated message decodes to an error, never a panic.
//!
//! Every message starts with its tag byte. `Hello` carries the protocol
//! [`VERSION`]; a host answers a different version with `Reject`.

use mp_sim::input::InputFrame;

/// Bumped whenever a message changes shape.
pub const VERSION: u16 = 1;

/// The most players in a race (four rows of two; MULTIPLAYER 2.2).
pub const MAX_PLAYERS: usize = 8;

/// A player's place in the lobby, and their index in the race's players.
pub type Slot = u8;

/// How rivals fill the grid (MULTIPLAYER 2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiFill {
    None,
    To6,
    To8,
}

impl AiFill {
    /// The field size AI rivals fill to (humans count).
    pub fn field(self) -> usize {
        match self {
            AiFill::None => 0,
            AiFill::To6 => 6,
            AiFill::To8 => 8,
        }
    }
}

/// Where the humans start (MULTIPLAYER 2.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridRule {
    /// A random order every race.
    Random,
    /// Random for the first race, then the reverse of the last result.
    Reverse,
    /// The order of the last result.
    Same,
}

/// The host's lobby settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub level: String,
    pub ai: AiFill,
    pub rubber_band: bool,
    pub ghost: bool,
    pub grid: GridRule,
    /// Races in the session, 0 for open-ended.
    pub races: u8,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            level: "coast".into(),
            ai: AiFill::To6,
            rubber_band: true,
            ghost: false,
            grid: GridRule::Reverse,
            races: 0,
        }
    }
}

/// A player as the lobby shows them.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub slot: Slot,
    pub name: String,
    pub car: String,
    pub color: u32,
    pub ready: bool,
    pub connected: bool,
}

/// One row of the session's points table (MULTIPLAYER 4.5). AI rivals have
/// no slot.
#[derive(Clone, Debug, PartialEq)]
pub struct PointsRow {
    pub slot: Option<Slot>,
    pub name: String,
    pub points: u32,
    /// Places gained (+) or lost (-) since the last race.
    pub moved: i8,
}

/// Everything a device needs to build the same race as the host.
#[derive(Clone, Debug, PartialEq)]
pub struct RaceStart {
    /// The race's number in the session, from 0.
    pub race: u32,
    pub settings: Settings,
    pub seed: u32,
    /// The humans in the race, in slot order: their index here is their
    /// index in the simulation's players.
    pub humans: Vec<PlayerInfo>,
    /// Indexes into `humans`, in grid order.
    pub grid: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Msg {
    // ── Client to host ──
    Hello {
        version: u16,
        name: String,
        car: String,
        color: u32,
    },
    /// A change of name, car or colour in the lobby.
    SetMe {
        name: String,
        car: String,
        color: u32,
    },
    Ready(bool),
    /// The client's inputs for ticks `first..first + frames.len()` (the last
    /// few repeated, so a lost packet costs nothing).
    Input {
        first: u32,
        frames: Vec<InputFrame>,
    },
    Ping {
        id: u32,
        t: f64,
    },
    Leave,

    // ── Host to client ──
    Welcome {
        slot: Slot,
    },
    Reject {
        reason: String,
    },
    Lobby {
        settings: Settings,
        players: Vec<PlayerInfo>,
        points: Vec<PointsRow>,
        /// Races run in this session.
        raced: u32,
    },
    Start(RaceStart),
    /// The authoritative inputs for ticks `first..first + n`, each tick's
    /// frames for every human in order (`frames.len() == n * humans`).
    Inputs {
        first: u32,
        humans: u8,
        frames: Vec<InputFrame>,
    },
    /// The host's state hash after tick `tick`.
    Hash {
        tick: u32,
        hash: u64,
    },
    /// The host's tick (with the fraction of the next) when it answered.
    Pong {
        id: u32,
        t: f64,
        host_tick: f64,
    },
    /// The race is over; back to the lobby.
    End,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DecodeError(pub &'static str);

// ── Encoding ─────────────────────────────────────────────────────

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, x: u8) {
        self.0.push(x);
    }
    fn u16(&mut self, x: u16) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn u64(&mut self, x: u64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn f64(&mut self, x: f64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn bool(&mut self, x: bool) {
        self.u8(x as u8);
    }
    fn str(&mut self, s: &str) {
        // Names and ids are short; anything longer is cut at a char boundary.
        let mut end = s.len().min(255);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        self.u8(end as u8);
        self.0.extend_from_slice(&s.as_bytes()[..end]);
    }
    fn frame(&mut self, f: &InputFrame) {
        self.0.extend_from_slice(&f.steer.to_le_bytes());
        self.u8(f.throttle);
        self.u8(f.brake);
        self.u8(f.flags);
    }
    fn frames(&mut self, fs: &[InputFrame]) {
        self.u16(fs.len() as u16);
        for f in fs {
            self.frame(f);
        }
    }
    fn settings(&mut self, s: &Settings) {
        self.str(&s.level);
        self.u8(match s.ai {
            AiFill::None => 0,
            AiFill::To6 => 1,
            AiFill::To8 => 2,
        });
        self.bool(s.rubber_band);
        self.bool(s.ghost);
        self.u8(match s.grid {
            GridRule::Random => 0,
            GridRule::Reverse => 1,
            GridRule::Same => 2,
        });
        self.u8(s.races);
    }
    fn player(&mut self, p: &PlayerInfo) {
        self.u8(p.slot);
        self.str(&p.name);
        self.str(&p.car);
        self.u32(p.color);
        self.bool(p.ready);
        self.bool(p.connected);
    }
}

struct R<'a> {
    b: &'a [u8],
    at: usize,
}

const SHORT: DecodeError = DecodeError("message too short");

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], DecodeError> {
        let end = self.at.checked_add(n).ok_or(SHORT)?;
        let s = self.b.get(self.at..end).ok_or(SHORT)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError("bad bool")),
        }
    }
    fn str(&mut self) -> Result<String, DecodeError> {
        let n = self.u8()? as usize;
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| DecodeError("bad utf-8"))
    }
    fn frame(&mut self) -> Result<InputFrame, DecodeError> {
        let b = self.take(5)?;
        Ok(InputFrame {
            steer: i16::from_le_bytes([b[0], b[1]]),
            throttle: b[2],
            brake: b[3],
            flags: b[4],
        })
    }
    fn frames(&mut self) -> Result<Vec<InputFrame>, DecodeError> {
        let n = self.u16()? as usize;
        // Checked before allocating: a forged count can't ask for more than
        // the message holds.
        if n * 5 > self.b.len() - self.at {
            return Err(SHORT);
        }
        (0..n).map(|_| self.frame()).collect()
    }
    fn settings(&mut self) -> Result<Settings, DecodeError> {
        Ok(Settings {
            level: self.str()?,
            ai: match self.u8()? {
                0 => AiFill::None,
                1 => AiFill::To6,
                2 => AiFill::To8,
                _ => return Err(DecodeError("bad ai fill")),
            },
            rubber_band: self.bool()?,
            ghost: self.bool()?,
            grid: match self.u8()? {
                0 => GridRule::Random,
                1 => GridRule::Reverse,
                2 => GridRule::Same,
                _ => return Err(DecodeError("bad grid rule")),
            },
            races: self.u8()?,
        })
    }
    fn player(&mut self) -> Result<PlayerInfo, DecodeError> {
        Ok(PlayerInfo {
            slot: self.u8()?,
            name: self.str()?,
            car: self.str()?,
            color: self.u32()?,
            ready: self.bool()?,
            connected: self.bool()?,
        })
    }
    fn list<T>(
        &mut self,
        f: impl Fn(&mut Self) -> Result<T, DecodeError>,
    ) -> Result<Vec<T>, DecodeError> {
        let n = self.u8()? as usize;
        (0..n).map(|_| f(self)).collect()
    }
}

impl Msg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W(Vec::with_capacity(32));
        match self {
            Msg::Hello {
                version,
                name,
                car,
                color,
            } => {
                w.u8(1);
                w.u16(*version);
                w.str(name);
                w.str(car);
                w.u32(*color);
            }
            Msg::SetMe { name, car, color } => {
                w.u8(2);
                w.str(name);
                w.str(car);
                w.u32(*color);
            }
            Msg::Ready(r) => {
                w.u8(3);
                w.bool(*r);
            }
            Msg::Input { first, frames } => {
                w.u8(4);
                w.u32(*first);
                w.frames(frames);
            }
            Msg::Ping { id, t } => {
                w.u8(5);
                w.u32(*id);
                w.f64(*t);
            }
            Msg::Leave => w.u8(6),
            Msg::Welcome { slot } => {
                w.u8(20);
                w.u8(*slot);
            }
            Msg::Reject { reason } => {
                w.u8(21);
                w.str(reason);
            }
            Msg::Lobby {
                settings,
                players,
                points,
                raced,
            } => {
                w.u8(22);
                w.settings(settings);
                w.u8(players.len() as u8);
                for p in players {
                    w.player(p);
                }
                w.u8(points.len() as u8);
                for r in points {
                    w.u8(r.slot.unwrap_or(255));
                    w.str(&r.name);
                    w.u32(r.points);
                    w.u8(r.moved as u8);
                }
                w.u32(*raced);
            }
            Msg::Start(s) => {
                w.u8(23);
                w.u32(s.race);
                w.settings(&s.settings);
                w.u32(s.seed);
                w.u8(s.humans.len() as u8);
                for p in &s.humans {
                    w.player(p);
                }
                w.u8(s.grid.len() as u8);
                for g in &s.grid {
                    w.u8(*g);
                }
            }
            Msg::Inputs {
                first,
                humans,
                frames,
            } => {
                w.u8(24);
                w.u32(*first);
                w.u8(*humans);
                w.frames(frames);
            }
            Msg::Hash { tick, hash } => {
                w.u8(25);
                w.u32(*tick);
                w.u64(*hash);
            }
            Msg::Pong { id, t, host_tick } => {
                w.u8(26);
                w.u32(*id);
                w.f64(*t);
                w.f64(*host_tick);
            }
            Msg::End => w.u8(27),
        }
        w.0
    }

    pub fn decode(b: &[u8]) -> Result<Msg, DecodeError> {
        let mut r = R { b, at: 0 };
        let m = match r.u8()? {
            1 => Msg::Hello {
                version: r.u16()?,
                name: r.str()?,
                car: r.str()?,
                color: r.u32()?,
            },
            2 => Msg::SetMe {
                name: r.str()?,
                car: r.str()?,
                color: r.u32()?,
            },
            3 => Msg::Ready(r.bool()?),
            4 => Msg::Input {
                first: r.u32()?,
                frames: r.frames()?,
            },
            5 => Msg::Ping {
                id: r.u32()?,
                t: r.f64()?,
            },
            6 => Msg::Leave,
            20 => Msg::Welcome { slot: r.u8()? },
            21 => Msg::Reject { reason: r.str()? },
            22 => Msg::Lobby {
                settings: r.settings()?,
                players: r.list(R::player)?,
                points: r.list(|r| {
                    let slot = r.u8()?;
                    Ok(PointsRow {
                        slot: (slot != 255).then_some(slot),
                        name: r.str()?,
                        points: r.u32()?,
                        moved: r.u8()? as i8,
                    })
                })?,
                raced: r.u32()?,
            },
            23 => Msg::Start(RaceStart {
                race: r.u32()?,
                settings: r.settings()?,
                seed: r.u32()?,
                humans: r.list(R::player)?,
                grid: r.list(R::u8)?,
            }),
            24 => {
                let first = r.u32()?;
                let humans = r.u8()?;
                let frames = r.frames()?;
                if humans == 0 || frames.len() % humans as usize != 0 {
                    return Err(DecodeError("inputs not a whole number of ticks"));
                }
                Msg::Inputs {
                    first,
                    humans,
                    frames,
                }
            }
            25 => Msg::Hash {
                tick: r.u32()?,
                hash: r.u64()?,
            },
            26 => Msg::Pong {
                id: r.u32()?,
                t: r.f64()?,
                host_tick: r.f64()?,
            },
            27 => Msg::End,
            _ => return Err(DecodeError("unknown message")),
        };
        if r.at != b.len() {
            return Err(DecodeError("trailing bytes"));
        }
        Ok(m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp_math::Mulberry32;

    fn f(s: i16, t: u8) -> InputFrame {
        InputFrame {
            steer: s,
            throttle: t,
            brake: 3,
            flags: 5,
        }
    }

    fn samples() -> Vec<Msg> {
        let p = PlayerInfo {
            slot: 2,
            name: "Kit ✿".into(),
            car: "rally".into(),
            color: 0xee5a12,
            ready: true,
            connected: false,
        };
        vec![
            Msg::Hello {
                version: VERSION,
                name: "Marisol".into(),
                car: "sports".into(),
                color: 0xd81e36,
            },
            Msg::SetMe {
                name: "x".into(),
                car: "super".into(),
                color: 1,
            },
            Msg::Ready(true),
            Msg::Input {
                first: 77,
                frames: vec![f(-32767, 255), f(12, 0)],
            },
            Msg::Ping { id: 9, t: 1234.5 },
            Msg::Leave,
            Msg::Welcome { slot: 3 },
            Msg::Reject {
                reason: "full".into(),
            },
            Msg::Lobby {
                settings: Settings::default(),
                players: vec![p.clone()],
                points: vec![
                    PointsRow {
                        slot: Some(2),
                        name: "Kit".into(),
                        points: 18,
                        moved: -2,
                    },
                    PointsRow {
                        slot: None,
                        name: "Raven".into(),
                        points: 10,
                        moved: 1,
                    },
                ],
                raced: 2,
            },
            Msg::Start(RaceStart {
                race: 1,
                settings: Settings {
                    level: "seaside".into(),
                    ai: AiFill::To8,
                    rubber_band: false,
                    ghost: true,
                    grid: GridRule::Same,
                    races: 4,
                },
                seed: 0xdeadbeef,
                humans: vec![p.clone(), p],
                grid: vec![1, 0],
            }),
            Msg::Inputs {
                first: 5,
                humans: 2,
                frames: vec![f(1, 2), f(3, 4), f(5, 6), f(7, 8)],
            },
            Msg::Hash {
                tick: 120,
                hash: u64::MAX - 3,
            },
            Msg::Pong {
                id: 9,
                t: 1234.5,
                host_tick: 600.25,
            },
            Msg::End,
        ]
    }

    #[test]
    fn every_message_round_trips() {
        for m in samples() {
            let b = m.encode();
            assert_eq!(Msg::decode(&b).as_ref(), Ok(&m), "{m:?}");
        }
    }

    #[test]
    fn truncated_and_padded_messages_are_errors() {
        for m in samples() {
            let b = m.encode();
            for n in 0..b.len() {
                assert!(Msg::decode(&b[..n]).is_err(), "{m:?} cut at {n}");
            }
            let mut long = b.clone();
            long.push(0);
            assert!(Msg::decode(&long).is_err());
        }
    }

    /// Random bytes never panic the decoder (and the forged frame count of
    /// a short message can't make it allocate).
    #[test]
    fn garbage_never_panics() {
        let mut rng = Mulberry32::new(4);
        for _ in 0..20_000 {
            let n = (rng.next_f64() * 40.0) as usize;
            let b: Vec<u8> = (0..n).map(|_| (rng.next_f64() * 256.0) as u8).collect();
            let _ = Msg::decode(&b);
        }
    }

    #[test]
    fn long_names_are_cut_on_a_char_boundary() {
        let name = "é".repeat(200); // 400 bytes
        let b = Msg::Ready(true).encode();
        assert_eq!(b.len(), 2);
        let m = Msg::SetMe {
            name,
            car: "sports".into(),
            color: 0,
        };
        match Msg::decode(&m.encode()).unwrap() {
            Msg::SetMe { name, .. } => {
                assert!(name.len() <= 255);
                assert!(name.chars().all(|c| c == 'é'));
            }
            _ => unreachable!(),
        }
    }
}
