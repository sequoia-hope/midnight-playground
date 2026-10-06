# Where the game is going: worlds, activities and range

Status: **a vision and a parking lot, not a plan.** Nothing here is
scheduled. The Rust port finishes first (ROADMAP M0–M9), with no new levels
or modes before cutover. This document exists so ideas are written down
once and not lost, and so work done in the meantime doesn't close doors.
When something here becomes work, it gets its own design document (as
ROADMAP "M12 and beyond" asks) and a pointer from here.

Written from a conversation with the owner on 2026-10-06. The vehicle side
of the same conversation is `docs/vehicle-dynamics/SPEC.md`.

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

## 2. The principle: the world is the menu

- **One easy way in.** Press Drive and you are on the coast highway in a
  good car, with arcade handling. That is the whole game for someone who
  wants a fun drive.
- **Every feature lives at a place.** A new idea must answer "where in the
  world is this?" before it is built. Examples:
  - a race starts at an intersection;
  - a crawler waits at a trailhead in the hills;
  - a crash junction is a junction;
  - the Skyline time trial starts at the summit;
  - a shortcut is a fence you can break through.
  A feature that cannot answer gets parked, not built.
- **Depth is opt-in.** The extra depth is there to find, never in the way:
  - arcade handling by default, sim handling as a toggle;
  - soft-body tyres only on the vehicles and terrain that need them;
  - a Ferrari stuck in the sand is a joke, and a hint that something else
    belongs there.
- **No mode explosion.** Single levels stay, as quick races from the menu.
  The open world is where everything else lives.

## 3. The first world: the Peninsula

A video-game compression of the San Mateo coast and peninsula: it should
feel like the place, not map it. Real roads are inspiration for character,
not surveys. Seaside Raceway is the exception that proves the rule.

| Region | Character | Real inspiration |
|---|---|---|
| **The coast** | A beautiful coastal highway, a big sweeping beach, farms and fields, fog in the mornings | Highway 1 at Half Moon Bay |
| **The coastal city** | The sleepy town grown into a real city by the beach, with harbour, downtown and surface streets | Half Moon Bay, stretched toward a San Francisco |
| **Over the hill** | The climb inland, a reservoir on the far side | Highway 92, Crystal Springs |
| **The summit** | A ridge road at the top, fog rolling over the crest | Skyline Boulevard |
| **The redwoods** | A twisty, turning descent through redwood forest, looping back down to the coast | Highway 84 with the character of Highway 9 in the Santa Cruz Mountains |
| **The bay side** | Highways and surface streets down the other side of the hills | Redwood City |
| **Crawler hills** | Steep, rocky hills north of the coastal city, trails and trailheads | The hills around Pacifica |

**Later, the same world grows:**

- a big city to the north, in the role of San Francisco;
- a bridge across the bay;
- the East Bay, with BART-style trains and the owner's fantasy surface
  tram system;
- a crossing over the bay from the end of 92.

The first connected piece worth building (the "vertical slice") is the
loop **coast → 92 → Skyline → the redwoods → back to the coast**, with the
coastal town.

### 3.1 A second world, later

A **Pearl River Delta** world: dense cities, bridges and water. The owner
has never been there, so Claude leads its design. It comes after the
Peninsula has proved the format.

## 4. How the levels relate to the world

**Every new level is a stretch of the future map.**

1. Keep a coarse world plan: where the regions are and which roads
   connect them. It starts as the table above; later it becomes a map
   file.
2. Build each new level as a corridor of that plan. The **redwood level**
   the owner wants is the Skyline-and-redwoods stretch, not a level that
   stands on its own.
3. Each level stays playable on its own, as now.
4. The open world is made later by stitching corridors into a road
   network, with world streaming (ROADMAP expansion tracks b and d). The
   levels become its content; nothing is thrown away.

The current levels (Coast Highway, Sierra to the City, Downtown Streets,
Night City Cruise, Desert Run, Seaside Raceway) predate the plan. They stay
as they are; future corridors can borrow their generators and look.

## 5. Things to do in the world

A catalogue to choose from. Burnout Paradise's model: stop at any marked
intersection and hold gas and brake to start an event.

- **Races** from point to point and around loops, starting at
  intersections. They already exist as levels.
- **Pursuits:** Hot Pursuit (built) and Most Wanted style free-roam heat
  (`docs/hot-pursuit.md`).
- **Time trials** on the great roads: Skyline at dawn, the redwood
  descent, the coast at sunset.
- **Crash junctions:** Burnout's Crash Mode. Launch into a busy junction
  and score the damage in dollars. Needs car damage and destructible props
  (section 6).
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

## 6. Damage and destruction

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

## 7. Vehicles in the world

- **Road cars:** the current garage, with arcade or sim handling.
- **Off-road and crawlers:** found at trailheads or garages in the world,
  not only picked in a menu.
- **Old trucks, electric cars and other kinds** as the vehicle definitions
  grow (`VehicleDef`, vehicle-dynamics SPEC 8.2).
- **Robots** for RL, sharing the world's terrain.

Surfaces tell you which vehicle belongs where: sand, mud and rock that
strand a sports car are part of the design, not a bug.

### 7.1 Car classes and the stable

Balance comes from **matching cars, not slowing them down.** Today the
Ion Arc is the most fun partly because it is faster than the rest, which
makes races easy. Racing games solve this with classes of a similar era
and performance, and a larger stable to fill them:

- **Classes** such as classic, 90s Japanese, modern sports, supercar,
  electric and off-road. A race is run within a class, so every car in
  it can be as strong as it really would be.
- **A performance index** per car, computed from its specs (power,
  weight, grip), as a check that a class is fair. Within a class, cars
  differ in character (grip against power, launch against top speed),
  not in pace.
- **Cars are inspired, never copied:** recognisable through their
  proportions and era, with original bodywork and names. The usual trick
  of one car's front and another's rear is a starting point, not the
  rule.
- **Owner's favourites** to start the classic and modern classes: a 1991
  3000GT VR-4 type (black, white leather interior), and a 2008 Saturn
  Sky Redline type (black, red and black leather interior).

## 8. How levels get designed together

A collaborative level design process:

- **The owner sets the feel** in plain words, with photos and real places:
  "fog over Skyline", "the reservoir on the far side", "Highway 9 twists".
- **Claude (and Fable for design passes) drafts:** a region's road plan,
  its landmarks, its events and its scenery kit. The drafts are written into
  the world plan and level data, never only in a chat.
- **The level viewer ("god mode", port SPEC 8.6) is the shared table:**
  both sides look at the same generated level and annotate it.
- **Real geography is compressed,** as games do: shorter distances, more
  landmarks per kilometre, the character kept and the tedium dropped.

## 9. Order, when the time comes

1. Finish the port to cutover. **Nothing in this document starts first.**
2. Vehicle-dynamics seams, then sim handling (vehicle-dynamics SPEC
   milestones V0–V4).
3. The world plan as a map file, and the Peninsula vertical slice: coast,
   town, 92, Skyline, the redwoods. Start-anywhere events, one breakable
   shortcut.
4. Then crawler hills (with the vehicle-dynamics V5–V6), crash junctions,
   the big city, the bridge, the East Bay and its trams, and the Pearl
   River Delta.

## 10. Parking lot

Ideas that came up and have no home yet. Add to this list rather than
starting them.

- An H-pattern gearbox and clutch for the owner's wheel and pedals.
- Weather and time of day as world state: fog rolling over Skyline.
- Trains as moving obstacles and as scenery (BART, the fantasy trams).
- Ferries, or the bay crossing as a bridge drive.
- Multiplayer free roam in the open world (ROADMAP M10–M11 are races
  first).
