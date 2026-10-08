# Where the game is going: worlds, activities and range

Status: **the vision: what the game becomes.** The order of work is in
`docs/vision/ROADMAP.md`, and the owner's notes that feed both are logged in
`docs/vision/README.md`. The Rust port finishes first (port ROADMAP M0–M9),
then multiplayer (M10–M11). No new levels or modes start before cutover.
When something here becomes work, it gets its own design document and a
pointer from the vision roadmap.

## 1. What kind of game

It began as a Need for Speed style night racer and is becoming a racing
game with range. The models are:

- **Burnout Paradise** for structure: one open world, with things to do
  everywhere and no menus between them.
- **BeamNG.drive** for range: road cars, old trucks, electric cars and rock
  crawlers in one world, physics deep enough to play with.
- **Need for Speed** for feel: night, speed, pursuits, and the secret dirt
  shortcut through a broken fence.

The difference from all three: this is **open source and cross-platform**,
in the browser on WebGPU and native on Linux, and it runs on phones.

The risk with range is an unapproachable mess, a "nerdy, weird simulator".
The answer is the principle in section 2.

### 1.1 Who you are

**Someone still becoming who they are.** The owner's own driving on these
roads was like that: working jobs, going from place to place, already
someone but not yet who they would become, with no idea yet what was
coming. So the player gets **no set backstory**. Who they are comes from
how and where they drive, and the world never tells them who to be.

- Endless roads, jobs and night drives with the radio on carry the theme.
  Becoming is the story; there is no plot to finish.
- A garage in the coastal town can still be a home base (jobs, cars,
  gadgets, the crawler project) without defining the player.
- **The radio carries the voices,** as Burnout Paradise's DJ did, voiced
  like the Hot Pursuit police radio (WORLD 5.2). No cutscenes.
- **Depth from memory, not from script.** Detailed narrative is not
  wanted, and chatter that tries to be deep can show no depth. What
  carries weight cheaply is the game remembering the player: the DJ
  mentions that someone just took the Skyline record in a red Firebird;
  a rival who lost to you last week remembers it; fans wear your colours
  once you are known. Lines are about what this player actually did.
- **The risk is chatter that falls flat:** the police radio's "I saw this
  driver at this time" wears thin if stretched. So: little and good.
  Short lines, mostly reacting to what the player just did, never on a
  loop, long silences between. Tested on the owner before more is built.
- Interstate 76 (open world, missions, a garage, the Sidewinder Force
  Feedback joystick in the box) is the spirit; its outlaw violence is not.

### 1.2 The name

**Midnight Playground** (proposed by the owner) fits the sandbox: a
literal playground for driving and simulation. One way to keep both
names: *Midnight Playground* is the game, and *Midnight Racer* is its
street-racing event series. The owner chose the name on 2026-10-06, and
the crates became `mp_` (port DECISIONS D1100).

## 2. The principle: the world is the menu

- **One easy way in.** Press Drive and you are on the coast highway in a
  good car, with arcade handling. That is the whole game for someone who
  wants a fun drive.
- **Every feature lives at a place.** A new idea must answer "where in the
  world is this?" before it is built. Examples:
  - a race starts at an intersection;
  - a crawler waits at a trailhead in the hills;
  - a crash junction is a junction;
  - a delivery starts at a garage;
  - the Skyline time trial starts at the summit;
  - a shortcut is a fence you can break through.
  A feature that cannot answer gets parked, not built.
- **Depth is opt-in.** The extra depth is there to find, never in the way:
  - arcade handling by default, sim handling as a toggle on every car;
  - soft-body tyres only on the vehicles and terrain that need them;
  - a Ferrari stuck in the sand is a joke, and a hint that something else
    belongs there.
- **No mode explosion.** Single levels stay, as quick races from the menu.
  The open world is where everything else lives.
- **Scope is reined in on purpose.** Some things the owner loves are
  deliberately parked (section 11): trucking in the spirit of Euro Truck
  Simulator 2, getting out of the car on foot, a track editor for players.

## 3. The first world: the Peninsula

A video-game compression of the San Mateo coast and peninsula: it should
feel like the place, not map it. Seaside Raceway is the exception that
proves the rule.

**Revised 2026-10-08:** the world is planned on the real peninsula, with
real elevation and roads (`docs/vision/atlas.md`). Half Moon Bay and
Pacifica become more like the real towns, and **over the hill the city is
the peninsula**: Redwood City all the way up to San Francisco. The first
piece is the loop of Highways 1, 92, 35 (Skyline) and 84, and the first
race runs from the crest above San Gregorio to Half Moon Bay's big
intersection. The table below is the first plan; the atlas's
`plan.json` now holds the regions.

| Region | Character | Real inspiration |
|---|---|---|
| **The coast** | A beautiful coastal highway, a big sweeping beach, farms and fields, fog in the mornings, pelicans flying alongside | Highway 1 at Half Moon Bay |
| **The coastal city** | The real Half Moon Bay: Main Street, the beaches, the harbour at Pillar Point (revised 2026-10-08: no longer grown into a city) | Half Moon Bay, El Granada, Princeton |
| **North up the coast** | Highway 1 on to the next town, the drive home | Half Moon Bay to Pacifica |
| **Over the hill** | The climb inland, a reservoir on the far side | Highway 92, Crystal Springs |
| **The summit** | A ridge road in fog so dense the redwoods drip with it, so it feels like rain | Skyline Boulevard |
| **The redwoods** | A twisty, turning descent through redwood forest, looping back down to the coast | Highway 84 with the character of Highway 9 in the Santa Cruz Mountains |
| **The state park** | Dirt roads and clearings in the mountains: a rallycross loop and a rally sandbox | The state parks between the coast and the peninsula |
| **The bay side** | The city: the peninsula's towns as one city along the bay, from Redwood City up to San Francisco (revised 2026-10-08) | Redwood City to San Francisco |
| **Crawler hills** | Steep, rocky hills north of the coastal city, trails and trailheads | The hills around Pacifica |

**Later, the same world grows:**

- a big city to the north, in the role of San Francisco;
- a bridge across the bay;
- the East Bay, with BART-style trains and the owner's fantasy surface
  tram system;
- a crossing over the bay from the end of 92.

The first connected piece worth building (the "vertical slice") is the
loop **coast → 92 → Skyline → the redwoods → back to the coast**, with the
coastal town and Highway 1 north.

### 3.1 A second world, later

A **Pearl River Delta** world: dense cities, bridges and water. The owner
has never been there, so Claude leads its design. It comes after the
Peninsula has proved the format.

## 4. How the levels relate to the world

**Every new level is a stretch of the future map.**

1. Keep a coarse world plan: where the regions are and which roads
   connect them. It starts as the table above; it becomes a map file in
   the level tools (vision ROADMAP M13).
2. Build each new level as a corridor of that plan. The **redwood level**
   the owner wants is the Skyline-and-redwoods stretch, not a level that
   stands on its own.
3. Each level stays playable on its own, as now.
4. The open world is made later by stitching corridors into a road
   network, with world streaming. The levels become its content; nothing
   is thrown away.

The current levels (Coast Highway, Sierra to the City, Downtown Streets,
Night City Cruise, Desert Run, Seaside Raceway) predate the plan. They stay
as they are; future corridors can borrow their generators and look.

## 5. A living world

Today the game feels too static. The world should be busy and react to the
player:

- **Crowds at Seaside Raceway:** people in the stands, fans leaning on the
  fence waving and cheering, camera operators on the bridge as the cars go
  by, drones flying over.
- **Birds everywhere,** above all pelicans flying alongside the car on the
  coast, like the ending of the first Jurassic Park.
- **Traffic that reacts:** pulls over for police, brakes, honks, swerves.
- **Pedestrians and wildlife** who always get out of the way. People step
  back or run; deer and other animals scamper off. Nobody is ever hit.
- **Places that make the world feel lived in:** gas stations, car washes and
  garages. Garages also anchor jobs (section 6) and could later carry
  GTA-like play.

### 5.1 Weather, light and atmosphere

- **Day and night** as a cycle, and **weather** as world state.
- **Rain changes grip,** wets the windscreen, and puts reflections on the
  road.
- **Headlights that matter at night,** with high beams.
- **Fog on Skyline:** so heavy in the redwoods that the trees drip and it
  feels like rain, without rain falling. This one the owner especially
  wants.

### 5.2 Sound and radio

- **Generated radio stations with DJ chatter,** voiced the way the Hot
  Pursuit police radio is (generated with a Qwen voice-design model), which
  the owner was pleased with.
- **Sound comes first** (vision ROADMAP M12, `docs/vision/sound.md`):
  engines with a deep rumble and music with real depth.
- **The generated music system needs much more depth** to carry stations.
  In-game generated audio is the focus; "bring your own music" is not
  needed, since players can mute the game and play their own.

## 6. Things to do in the world

A catalogue to choose from. Burnout Paradise's model: stop at any marked
intersection and hold gas and brake to start an event.

- **Races** from point to point and around loops, starting at
  intersections. They already exist as levels.
- **Pursuits:** Hot Pursuit (built) and Most Wanted style free-roam heat
  (`docs/hot-pursuit.md`).
- **Time trials** on the great roads: Skyline at dawn, the redwood
  descent, the coast at sunset.
- **Rallycross:** a dirt loop in the state park, wanted early.
- **Delivery jobs** from garages: anything from food to dodgy technology,
  against the clock or the police.
- **Crash junctions:** Burnout's Crash Mode. Launch into a busy junction
  and score the damage in dollars. Needs car damage and destructible props
  (section 7).
- **Takedowns and survival:** Burnout's Road Rage and Marked Man.
- **Stunt runs:** chain jumps, drifts, near misses and air (drift and
  near-miss bonuses exist already).
- **Discoveries:** breakable fences and gates that hide shortcuts, super
  jumps, billboards to smash. Each road keeps its own records (fastest
  time, biggest crash).
- **Crawler trails:** a trailhead with a crawler parked at it; trails
  graded by difficulty; airing the tyres down. Uses Tier 2
  (`docs/vehicle-dynamics/SPEC.md`).
- **Free roam,** including the wrong car in the wrong place.

Lower interest, not opposed: drift zones and Forza Horizon style speed
traps, a drag strip, a hill climb.

## 7. Damage and destruction

- **Shortcuts through things**, as in Need for Speed: the road winds, and
  a dirt path through an old wooden fence cuts the corner with some air.
  This works even in today's corridor levels once breakable props exist.
- **Environmental damage** for crash junctions and takedowns: fences,
  signs, barriers, parked cars, fruit stands.
- **Car damage** beyond Hot Pursuit's damage meter: visible dents, then
  (with sim handling) damage that changes how the car drives.

For the simulation this means **breakable props are simulation state**
(deterministic, in `SimState`, so replays, rollback and RL see them),
with debris that doesn't affect play left to the client.

### 7.1 Contact without violence

Vehicle combat in the spirit of Interstate 76 and Mario Kart, made adult
but not violent. Splatoon (aiming is ink, the score is ground covered) and
Rocket League (cars, contact and skill, no harm) prove it works. The rule:
**cars get disabled, people never get hurt.** The tools are gadgets:

- **Winch and tow hook:** yank a rival's bumper, or anchor to a tree on a
  crawler trail.
- **EMP and jamming:** for a few seconds a rival's engine cuts, lights die,
  or nitro won't fire.
- **Drones** that drop oil, smoke or a spike strip ahead (Hot Pursuit's
  spike strip, turned around).
- **Hacking the world:** lights to red ahead of a rival, a bridge raised,
  sprinklers on a roundabout.
- **Possession games:** carry the package; a bumper tap steals it.
  Keep-away with cars, and delivery jobs in multiplayer.
- **Paint tags:** tag a car and it's "it", or score by painting rivals.

- **Glitching the world:** a meta gadget that tears the game's own
  fabric. Textures corrupt, a stretch of road drops to wireframe, a
  rival's car stutters a few frames or clips out of the world for a
  moment. Hacking taken one level up: the player hacks the game itself.

## 8. Cars

### 8.1 Classes and the stable

Balance comes from **matching cars, not slowing them down.** Today the
Ion Arc is the most fun partly because it is faster than the rest, which
makes races easy. Racing games solve this with classes of a similar era
and performance, and a larger stable to fill them:

- **Classes** of a similar era and pace. A race is run within a class, so
  every car in it can be as strong as it really would be.
- **A performance index** per car, computed from its specs (power,
  weight, grip), as a check that a class is fair. Within a class, cars
  differ in character (grip against power, launch against top speed),
  not in pace.
- **Cars are inspired, never copied:** recognisable through their
  proportions and era, with original bodywork and names. The usual trick
  of one car's front and another's rear is a starting point, not the
  rule.

A first draft of the stable. The owner's own cars are marked ★; their
colours are the owner's, and red and black recur for a reason.

| Class (draft) | Cars |
|---|---|
| **American muscle and classics** | Brawler 69 (existing); ★ an '88 Firebird type in red with black interior and T-tops (the owner had the Formula; the hero version is probably the Trans Am GTA, to confirm) |
| **90s heroes** | ★ a '91 3000GT VR-4 type, black with white leather |
| **Modern sports** | Vento GT (existing); ★ an '04 Audi S4 type; ★ an '08 Saturn Sky Redline type, black with red and black leather |
| **Supercars** | Stiletto R (existing); a Lamborghini Diablo type (the Need for Speed cars the owner grew up on); a Porsche GT type |
| **Rally and off-road** | Kestrel RS (existing); later the crawlers |
| **Electric** | Ion Arc (existing), with electric rivals to race |

### 8.2 Garage, tuning and looks

- A **garage**: tuning (gear ratios, suspension, tyre pressure) and looks
  (paint, liveries, interior colour).
- With sim handling, a tune becomes a **setup sheet**: the car's settings
  saved as a file that can be shared.

### 8.3 Cameras, replays and photos

- More cameras: cockpit with a working dash, bonnet, chase, and free
  choice of view.
- **Replays** (determinism makes them nearly free), a **photo mode**, and
  **ghost cars** of your own or a friend's best run.

### 8.4 Vehicles in the world

- **Road cars:** the stable, with arcade or sim handling.
- **Off-road and crawlers:** found at trailheads or garages in the world,
  not only picked in a menu.
- **Robots** for RL, sharing the world's terrain.

Surfaces tell you which vehicle belongs where: sand, mud and rock that
strand a sports car are part of the design, not a bug.

## 9. People together

- **Multiplayer comes right after the port** (port ROADMAP M10–M11): the
  current game is already great to race together and needs no expansion
  first. Its gate is stability: the audio bugs and frame stutters being
  worked on now must be gone, and a friend's Android phone tried.
- **Car meets:** park up somewhere scenic with other players (the open
  world).
- **Spectating and streaming:** spectator mode, and the streaming support
  being added now, which sends the game to the owner's Android projector.
- **Leaderboards and sharing results** without the owner having to run a
  server, if possible: for example storing results and records in the
  player's own Bluesky (AT Protocol) repository, linked to friends. A
  small server stays an option.

## 10. Tools, telemetry, accessibility and robots

- **Level tools first:** the owner and Claude need tools to build levels
  together (section 10.1). A track editor for players comes much later.
- **Telemetry:** per-tick data (speed, inputs, tyre loads and slips with
  sim handling) viewable in the game and exportable. MoTeC is the
  analysis software real racing teams use; exporting to a format like it
  lets a lap be studied in the same way.
- **Accessibility:** one-handed controls, a colour-blind-safe HUD, and
  auto-steer for younger players and phones (auto-steer for mobile is
  being built now).
- **Sim-racing hardware** beyond the owner's wheel and pedals (shifters,
  handbrakes, button boxes) waits for requests.
- **RL and robotics:** environments on the simulation, the crawler RL of
  the vehicle-dynamics spec, and sensor models: a dash camera and a lidar.

### 10.1 How levels get designed together

- **The owner sets the feel** in plain words, with photos and real places:
  "fog over Skyline", "the reservoir on the far side", "Highway 9 twists".
- **Claude (and Fable for design passes) drafts:** a region's road plan,
  its landmarks, its events and its scenery kit. The drafts are written into
  the world plan and level data, never only in a chat.
- **The level viewer ("god mode", port SPEC 8.6) is the shared table:**
  both sides look at the same generated level and annotate it.
- **Real geography is compressed,** as games do: shorter distances, more
  landmarks per kilometre, the character kept and the tedium dropped.

## 11. Parked on purpose

Ideas the owner wants eventually, held back to keep the scope sane. Add to
this list rather than starting them.

- **Trucking,** in the spirit of Euro Truck Simulator 2.
- **On foot:** getting out of the car and walking around.
- **A track and route editor for players.**
- **Towing and recovery** with trucks.
- **The bay city, the bridge, the East Bay and its trams,** and the Pearl
  River Delta world, after the Peninsula slice.
- An H-pattern gearbox and clutch for the owner's wheel and pedals.
- Trains as moving obstacles; ferries.
- Multiplayer free roam in the open world.
