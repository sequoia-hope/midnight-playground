# A living world

Status: **started 2026-10-07** (vision ROADMAP M14). The owner asked for the
living world next, after multiplayer: birds first, then people on the
sidelines, the raceway and Seabright before anywhere else, and nothing in
the path of cars yet. This note records what is built, how, and what comes
next. The vision is WORLD.md section 5.

## 1. What is built

**Birds on every level** (`crates/mp_worldgen/src/life/birds.rs`).

- Flocks circle over anchors every ~380 m along the route (~300 m on the
  loops), 25–120 m off the road, on either side; on the coast three in four
  are over the sea.
- The species follow the zone's scenery:
  - gulls on the coast and at the raceway;
  - raptors, small birds and ravens over the mountains and the valley;
  - ravens and raptors in the desert;
  - pigeons, with a few gulls, in the city.
- Each species has its own size, colour, wingbeat, share of gliding, speed,
  flock size, height and circle.
- Small birds tighten and widen their circle; raptors soar high and wide,
  barely beating.
- **Pelicans along the Coast Highway:** a V of seven over the sea 34 m left
  of the road, 9 m up, flying along the coast at 13 m/s.
  - At a cruise they keep you company.
  - When you have left them 450 m behind (or haven't reached them), they
    set off again 650 m ahead, so a drive along the coast keeps meeting them.
  - This is the M14 gate's "drive down the Coast Highway at dusk, with
    pelicans".
- Birds roost at night: none are drawn past a night factor of 0.8.

**People on the sidelines** (`crates/mp_worldgen/src/life/people.rs`).

- **Seaside Raceway: fans along the catch fence.** They stand in the
  stretches where the raceway hangs its sponsor banners: the start/finish,
  the crest, the hairpins and the Corkscrew.
  - They come in clusters of one to three rows, facing the track, 1.7 m or
    more behind the fence.
  - Nobody stands in the pit lane, the grandstands, the buildings, the
    water, or near another stretch of the track. Nobody stands in a ditch
    or up a bank.
  - As the player's car comes within 170 m they raise their arms. Close by,
    they wave and bounce, more for a fast car than a crawl. Once the car has
    gone, they settle.
- **Seabright:**
  - people walk the promenade (two lanes) and the town's sidewalk, back and
    forth, with a stride, swinging arms and a little bounce;
  - others stand at the promenade's sea rail, alone or in pairs, looking out.
- The grandstands keep their painted crowd for now.

**How it is built.**

- `life::Life` is a scenery module the game adds after a level's own
  (`world::level_jobs_with`; `?life=0` leaves it out). The ported modules,
  and the parity digests their tests hold, are untouched.
- Everything is instanced:
  - birds are three instanced meshes for the whole level (body, two wings),
    filled each frame from the flocks near the camera, up to 160 birds;
  - people are six (two legs, torso, head, two arms), one instance each per
    person.
- One animator moves what is near: fans within 170 m of the player along
  the track, walkers within 260 m of the camera, flocks within 950 m. The
  rest keep their last pose.
- Flight and walking are functions of time, not integrated state, so a
  flock or a walker that comes back into range is where it should be.
- World generation's rules hold: `mp_math::kernel` for every inexact
  function, seeded `Mulberry32` for every choice.
- The menu's flyover sections leave it out.
- `crates/mp_worldgen/tests/life.rs` checks:
  - birds by day and roosting at night;
  - the pelicans staying with a car on the coast;
  - nobody on the road (the coast and the raceway: everyone more than a
    metre outside the road's half-width);
  - fans cheering and settling;
  - walkers walking about a metre a second.

## 2. Next

0. **What the owner saw (2026-10-08): nothing, at Seaside Raceway.**
   Looked at headless in the release build:
   - **The birds were never drawn, on any level.** The bird pool is
     built empty for its animator to fill, and the client's loader
     spawned nothing for an instanced mesh with no instances; the
     client also hid such a mesh for good once it ran empty. Fixed
     (port DECISIONS D1156). Checked headless in the release build: the
     gulls circle over the track about 450 m into the lap again.
   - **The fans are there but hidden.** They stand 1.7 m or more behind
     the catch fence, in the stretches with the sponsor boards, and the
     concrete wall and the boards hide everything but their heads and
     raised arms. From a car only a blur of colour shows above the
     boards. Options for the owner: stand them where the fence has no
     boards, on the banks and mounds where real fans watch from (the
     "not up a bank" rule dropped), on terraces stepped up behind the
     fence, or as figures in the grandstands near the camera.
   - Gulls are white and small against a pale golden-hour sky; once the
     owner has seen them, they may want to be larger or darker.
1. **Look at it with the owner** (on the phone too): the sizes, the colours,
   how many, how lively. Everything is a constant at the top of `birds.rs`
   and `people.rs`.
2. **More of Seaside Raceway:** people in the grandstands as figures (in
   place of the painted crowd, near the camera), camera operators on the
   bridge, a drone following the leader, marshals with flags at the
   corners.
3. **More places:**
   - the pier and the beach in Seabright (swimmers, surfers, sunbathers);
   - the harbour (dock workers);
   - the city's sidewalks at night;
   - the desert diner;
   - hikers at the mountain lookout.
4. **Wildlife and people near the road** (WORLD 5: they "always get out of
   the way"): deer at the valley's edge, cattle egrets in the fields,
   pedestrians at Seabright's crossings who wait, and who step back when a
   car comes. This needs the simulation to know them, so they get out of
   the way the same on every device (multiplayer), which is a design of its
   own.
5. **Traffic that reacts:** braking and pulling over for a pursuit.
6. **Weather and light as simulation state** (WORLD 5.1): rain that changes
   grip, wet roads, fog, headlights and high beams. This is the other half
   of M14.

## 3. Cost

- **Draw calls:** nine in all (three for birds, six for people), whatever
  the numbers.
- **Per frame:** a few hundred instance matrix edits near the action. A
  full pool of 160 birds is 480; a crowded grandstand stretch with the car
  going by is about 2 per fan plus 4 for the ones bouncing.
- **Phones:** to measure on the reference phones before raising the
  numbers.
