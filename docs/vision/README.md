# Vision: where Midnight Racer goes after the port

| File | What |
|---|---|
| `WORLD.md` | The vision: what the game becomes |
| `ROADMAP.md` | The order of work after the port, as milestones M12 onward |
| `sound.md` | Engines and music: the sound milestone (draft) |
| `../vehicle-dynamics/SPEC.md` | Sim handling, soft-body tyres, the force-feedback wheel |

## How this is kept

The owner thinks out loud, often in voice sessions. Claude turns that into
these documents:

- **New ideas go into WORLD.md,** in the section where they belong, or in
  its "Parked on purpose" list. They never go straight into work.
- **Order changes go into ROADMAP.md.**
- **Each session's notes are logged below,** in short, so a later session
  can see what was said and when, and what changed because of it.
- Nothing here starts before the port's cutover.

## Notes log

### 2026-10-06 (voice session)

- **Handling:** wants an Assetto Corsa class sim option beside the
  arcade handling, as a toggle on every car with a racing tune of assists;
  player-only first, Tier 1 rivals tried on the desktop. Soft-body tyres
  on a rigid chassis for rock crawlers, BeamNG-inspired, mindful of
  phones. → `docs/vehicle-dynamics/SPEC.md`.
- **Wheel:** has a home-built force-feedback wheel: a motor on their own
  controller, an RP2350. Wants it made a proper HID force-feedback device;
  the firmware goes in a separate repository. → vehicle-dynamics SPEC 7.3.
- **Tyre model** for the crawler is in Onshape.
- **Open world:** Burnout Paradise's structure and BeamNG's range,
  cross-platform and open source. The Peninsula as the first world: Half
  Moon Bay grown into a city, the coast and its beach, 92, Skyline's fog,
  a Highway 9-like redwood descent, Highway 1 north to Pacifica, crawler
  hills, a state-park rallycross. Later the bay, the East Bay with trams,
  and a Pearl River Delta world led by Claude. → WORLD 3–4.
- **Focus:** finish the port first; multiplayer next, gated on stability
  (audio bugs and frame stutters are being fixed now). → ROADMAP.
- **Car balance:** the Ion Arc is fun partly because it is the fastest;
  wants classes of similar cars and a bigger stable, inspired and not
  copied. Owner's cars: '91 3000GT VR-4 (black, white leather), '08
  Saturn Sky Redline (black, red and black leather), '04 Audi S4, '88
  Firebird Formula (red, black interior, T-tops; a better trim wanted,
  name not remembered). Wishes: a Lamborghini Diablo, a Porsche GT.
  → WORLD 8.
- **A living world:** crowds, fans, camera operators and drones at the
  raceway; birds, pelicans on the coast; reacting traffic; pedestrians
  and wildlife that get out of the way; dripping fog on Skyline; rain,
  headlights. → WORLD 5.
- **Radio:** generated stations with DJ chatter, voiced like the police
  radio (Qwen voice design); the music system needs depth; no "bring your
  own music". → WORLD 5.2.
- **Liked:** garage and tuning, more cameras, replays, photo mode, ghosts,
  deliveries, rallycross, telemetry, accessibility, leaderboards (perhaps
  on AT Protocol), spectating and streaming to the projector, RL with
  dash camera and lidar models.
- **Parked:** trucking (Euro Truck Simulator 2), walking on foot, a player
  track editor, extra sim hardware until requested. Level tools for the
  owner and Claude come first.
- **Process:** push straight to main.
- **Later the same session:** Interstate 76 (open world, missions, a
  garage, vehicle combat; bought with a Sidewinder Force Feedback
  joystick) is a favourite, but the owner doesn't like violence. Wants
  combat that is adult without being violent, and a higher-level
  narrative: who the player is and why they drive. The current menu stays
  until there is an open world. Thinking of renaming the game **Midnight
  Playground**. → WORLD 1.1, 1.2, 7.1.
- **Then:** "who you are" should be someone still becoming who they
  are, with no set backstory, drawn from the owner's own years of driving
  these roads. Worried that radio chatter stretched too far falls flat:
  keep it sparse and reactive, and test it. New gadget idea: glitching
  the game world itself. → WORLD 1.1, 7.1.
- **Then, on story and sound:** reluctant to write detailed narrative, but
  wants something there; chatter that tries to be deep may show no depth.
  → WORLD 1.1 (depth from memory). **Sound is high priority, ahead of the
  open world:** engines are whiny and should have a deep rumble (a V8, a
  V12); music has a nice vibe but sounds like generic generated
  synthwave and needs richer, more real sound and more range. Loves
  electronic dance music, house and techno. Suggested Mutable
  Instruments' open-source modules (deterministic, the code is the
  instrument) and classic machines like the 808. No "bring your own
  music". → `sound.md`, ROADMAP M12.
- **DJ chatter side quest:** two DJs, both women, both kind, both plugged
  into the street racing scene (you listen because you race).
  **Marisol** (The Tide, 88.1): mid-forties, warm, understated, into
  cycling and film photography; goes to the forest rave and still runs
  Monday; tired from other people's nonsense. **Kit** (Ridgeline Radio, a
  pirate van on Skyline): late twenties, creative, into plants, hiking and
  camping; brash, sometimes combative, wants to be kind and doesn't always
  manage it; wrecked after a rave. The classic long "Goooood morning"
  opener, fake local ads, raves, camping. Scripts and the listening page:
  `tools/dj-voice/`, `tools/dj-voice.html`; the owner's local agent
  records them.
