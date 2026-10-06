//! The bodies on the road and the plumbing between them: the agent list
//! (`agents` in Race.update: players, rivals, active traffic, pursuit
//! bodies, in that order, which is the collision order), views of them for
//! the drivers, the collision pass over all pools, and the pursuit's access
//! to its racers. Shared by the race (`mp_sim::race`) and the module
//! oracle's staging (`mp_sim::staged`).

use mp_track::Track;

use crate::ai::AiDriver;
use crate::body::{AgentView, Body, BodyId, PhysicsBody, player_view};
use crate::collisions::{Hit, resolve_collisions_except};
use crate::pursuit::{PAgent, Pursuit, RacerBody, Racers};
use crate::race::PlayerCar;
use crate::traffic::{Agent, Traffic, TrafficCar};

/// Bodies taken out of a pool one at a time, for the collision list.
type Slots<'a> = Vec<Option<&'a mut dyn Body>>;

/// The pools, borrowed apart so the pursuit can be updated while its
/// racers are reached.
pub struct Field<'a> {
    pub players: &'a mut [PlayerCar],
    pub rivals: &'a mut [AiDriver],
    pub traffic: Option<&'a mut Traffic>,
    pub pursuit: Option<&'a mut Pursuit>,
}

impl Field<'_> {
    /// The agent list at the start of a tick.
    pub fn agents(&self) -> Vec<BodyId> {
        let mut out: Vec<BodyId> = (0..self.players.len()).map(BodyId::Player).collect();
        out.extend((0..self.rivals.len()).map(BodyId::Rival));
        if let Some(tr) = &self.traffic {
            out.extend(
                tr.cars
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.active)
                    .map(|(i, _)| BodyId::Traffic(i)),
            );
        }
        if let Some(pu) = &self.pursuit {
            out.extend(pu.bodies().iter().map(pagent_id));
        }
        out
    }

    pub fn view(&self, id: BodyId) -> AgentView {
        let v = match id {
            BodyId::Player(i) => player_view(&self.players[i].v),
            BodyId::Rival(i) => self.rivals[i].view(),
            BodyId::Traffic(i) => self.traffic.as_ref().unwrap().cars[i].view(),
            BodyId::Police(i) => self.pursuit.as_ref().unwrap().police(i).view(),
            BodyId::Sawhorse(i) => self.pursuit.as_ref().unwrap().sawhorses[i].view(),
            BodyId::Anon => unreachable!("no anonymous bodies in a field"),
        };
        AgentView { id, ..v }
    }

    pub fn views(&self, agents: &[BodyId]) -> Vec<AgentView> {
        agents.iter().map(|&id| self.view(id)).collect()
    }

    /// Traffic's list: its own cars read live, the others as they are.
    pub fn traffic_agents(&self, agents: &[BodyId]) -> Vec<Agent> {
        agents
            .iter()
            .map(|&id| match id {
                BodyId::Traffic(i) => Agent::Traffic(i),
                id => Agent::Other(self.view(id)),
            })
            .collect()
    }

    /// The pursuit's list: its own bodies read live, the others as they are.
    pub fn pursuit_agents(&self, agents: &[BodyId]) -> Vec<PAgent> {
        agents
            .iter()
            .map(|&id| match id {
                BodyId::Police(i) => PAgent::Police(i),
                BodyId::Sawhorse(i) => PAgent::Sawhorse(i),
                id => PAgent::Other(self.view(id)),
            })
            .collect()
    }

    /// A body's velocity (`velocity()`), its mass (`mass`) and the
    /// position of its vehicle.
    pub fn velocity(&self, id: BodyId) -> (f64, f64) {
        match id {
            BodyId::Player(i) => (self.players[i].v.vx, self.players[i].v.vz),
            BodyId::Rival(i) => self.rivals[i].velocity(),
            BodyId::Traffic(i) => self.traffic.as_ref().unwrap().cars[i].velocity(),
            BodyId::Police(i) => self.pursuit.as_ref().unwrap().police(i).velocity(),
            BodyId::Sawhorse(i) => self.pursuit.as_ref().unwrap().sawhorses[i].velocity(),
            BodyId::Anon => (0.0, 0.0),
        }
    }

    pub fn mass(&self, id: BodyId) -> f64 {
        match id {
            BodyId::Player(i) => self.players[i].v.mass,
            BodyId::Rival(i) => self.rivals[i].mass(),
            BodyId::Traffic(i) => self.traffic.as_ref().unwrap().cars[i].mass(),
            BodyId::Police(i) => self.pursuit.as_ref().unwrap().police(i).mass(),
            BodyId::Sawhorse(i) => self.pursuit.as_ref().unwrap().sawhorses[i].mass(),
            BodyId::Anon => f64::NAN,
        }
    }

    /// `resolveCollisions(agents, hits)` over the pools, in agent order.
    pub fn collide(&mut self, t: &Track, agents: &[BodyId]) -> Vec<Hit> {
        self.collide_except(t, agents, false)
    }

    /// [`Field::collide`]; with `ghost_players`, the players pass through
    /// each other (they lead the agent list).
    pub fn collide_except(
        &mut self,
        t: &Track,
        agents: &[BodyId],
        ghost_players: bool,
    ) -> Vec<Hit> {
        let ghosts = if ghost_players {
            agents
                .iter()
                .take_while(|id| matches!(id, BodyId::Player(_)))
                .count()
        } else {
            0
        };
        let mut hits = Vec::new();
        let mut players: Vec<Option<PhysicsBody>> = self
            .players
            .iter_mut()
            .map(|p| Some(PhysicsBody { v: &mut p.v }))
            .collect();
        let mut rivals: Vec<Option<&mut AiDriver>> = self.rivals.iter_mut().map(Some).collect();
        let mut cars: Vec<Option<&mut TrafficCar>> = match &mut self.traffic {
            Some(tr) => tr.cars.iter_mut().map(Some).collect(),
            None => Vec::new(),
        };
        let (mut units, mut saws): (Slots, Slots) = match &mut self.pursuit {
            Some(pu) => {
                let Pursuit {
                    units,
                    block_cars,
                    sawhorses,
                    ..
                } = &mut **pu;
                (
                    units
                        .iter_mut()
                        .chain(block_cars.iter_mut())
                        .map(|u| Some(u as &mut dyn Body))
                        .collect(),
                    sawhorses
                        .iter_mut()
                        .map(|b| Some(b as &mut dyn Body))
                        .collect(),
                )
            }
            None => (Vec::new(), Vec::new()),
        };
        let mut pbs: Vec<PhysicsBody> = Vec::new();
        let mut player_slots = Vec::new();
        for &id in agents {
            if let BodyId::Player(i) = id {
                pbs.push(players[i].take().unwrap());
                player_slots.push(pbs.len() - 1);
            }
        }
        let mut pb_refs: Vec<Option<&mut PhysicsBody>> = pbs.iter_mut().map(Some).collect();
        let mut next_player = player_slots.into_iter();
        let mut bodies: Vec<&mut dyn Body> = Vec::with_capacity(agents.len());
        for &id in agents {
            let b: &mut dyn Body = match id {
                BodyId::Player(_) => pb_refs[next_player.next().unwrap()].take().unwrap(),
                BodyId::Rival(i) => rivals[i].take().unwrap(),
                BodyId::Traffic(i) => cars[i].take().unwrap(),
                BodyId::Police(i) => units[i].take().unwrap(),
                BodyId::Sawhorse(i) => saws[i].take().unwrap(),
                BodyId::Anon => unreachable!(),
            };
            bodies.push(b);
        }
        resolve_collisions_except(&mut bodies[..], t, &mut hits, ghosts);
        hits
    }
}

/// The id of a pursuit body.
pub fn pagent_id(a: &PAgent) -> BodyId {
    match *a {
        PAgent::Police(i) => BodyId::Police(i),
        PAgent::Sawhorse(i) => BodyId::Sawhorse(i),
        PAgent::Other(v) => v.id,
    }
}

/// The pursuit's racers: the players' cars and the rivals.
pub struct RacerAccess<'a> {
    pub players: &'a mut [PlayerCar],
    pub rivals: &'a mut [AiDriver],
}

impl Racers for RacerAccess<'_> {
    fn body(&self, id: BodyId) -> RacerBody {
        match id {
            BodyId::Player(i) => {
                let v = &self.players[i].v;
                RacerBody {
                    s: v.s,
                    lat: v.lat,
                    speed_along: v.speed,
                    half_w: v.half_w,
                    half_l: v.half_l,
                    x: v.x,
                    z: v.z,
                    vx: v.vx,
                    vz: v.vz,
                }
            }
            BodyId::Rival(i) => {
                let a = &self.rivals[i];
                RacerBody {
                    s: a.k.s,
                    lat: a.k.lat,
                    speed_along: a.k.speed,
                    half_w: a.k.v.half_w,
                    half_l: a.k.v.half_l,
                    x: a.k.v.x,
                    z: a.k.v.z,
                    vx: a.k.v.vx,
                    vz: a.k.v.vz,
                }
            }
            _ => unreachable!("racers are players and rivals"),
        }
    }

    fn ai_finished(&self, id: BodyId) -> bool {
        match id {
            BodyId::Rival(i) => self.rivals[i].finished,
            _ => false,
        }
    }

    fn hold_ai(&mut self, id: BodyId, seconds: f64, hold_lat: f64) {
        if let BodyId::Rival(i) = id {
            self.rivals[i].hold = seconds;
            self.rivals[i].hold_lat = Some(hold_lat);
        }
    }

    fn spike(&mut self, id: BodyId) {
        match id {
            BodyId::Player(i) => self.players[i].phys.spiked = 10.0,
            BodyId::Rival(i) => self.rivals[i].spiked = 10.0,
            _ => {}
        }
    }
}
