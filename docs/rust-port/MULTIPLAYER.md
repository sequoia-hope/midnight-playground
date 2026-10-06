# Multiplayer: race together (design note, ROADMAP WP 10.1)

Status: **draft for the owner's approval.** SPEC section 9 fixes the
machinery (server-authoritative, every client simulating the whole world
with rollback, WebSocket first, `mr-host`). This note settles the game
rules SPEC 9.6 left open, plus the lobby and the results, so WPs 10.2 to
10.7 can be built against it. Each rule marked **Proposed** is the
default; the owner can change it.

Scope: M10, two to eight people on a LAN (or the tailnet), each in a
browser tab or the native client, racing the existing race levels. Cruise
and Hot Pursuit with several players are M11.

## 1. How it works, in one paragraph

Every player's device runs the whole race: all cars, the AI rivals, the
traffic, from the same seed. Each device sends only its player's controls,
120 times a second, stamped with the tick they belong to. The host runs
the official copy and relays everyone's controls. A device that guessed a
remote player's controls wrong (it assumes "same as last tick") rewinds to
that tick and replays to now, which takes well under a millisecond. Contact
between cars is worked out by the same code on both sides, so two players
trading paint see the same thing. A few times a second the host sends a
fingerprint of its state; a device that has drifted adopts the host's
snapshot. On a LAN the guesses are almost always right, so rewinds are
rare and invisible.

## 2. The rules

### 2.1 Rubber-banding: each rival bands to its nearest human

Today each rival speeds up when it is far behind the player and slows when
it is far ahead (`AIDriver.js:46`: up to 10 % either way, past 80 to 400 m
of gap). With several humans, "the player" has to mean someone.

**Proposed: each rival measures its gap to the human nearest to it in
race progress**, and applies today's formula to that gap.

- With one human it is exactly today's behaviour, so single-player is
  unchanged.
- A rival far ahead of every human slows for the leader; one far behind
  every human catches up to the last. The AI never runs away from the
  field or drops off it.
- Rivals spread themselves among the humans, so a player at the back still
  has someone to race, and so does the leader.

Lobby option: **rubber-banding on / off** (off for straight competition).
Humans never get catch-up help.

### 2.2 Grid

Today the grid is rows of two, ten metres apart, and the player starts
fourth among five rivals.

**Proposed:**
- The first race of a session: humans in a random order (from the session
  seed), placed among the AI from fourth place back, as the player is now.
  With few AI, humans fill from the front.
- Later races in the same session: **reverse of the last result** among the
  humans (the winner starts last), a party-racing staple. Lobby option:
  random / reverse / same as last.
- At most **eight cars** on the grid (four rows), humans first; AI fill the
  rest.

### 2.3 AI fill

**Proposed:** lobby option **AI rivals: none / fill to 6 / fill to 8**,
default fill to 6 (today's field size). The AI rivals keep their names,
cars and skills from the level, as now.

### 2.4 Contact between players

**Proposed:** collisions **on** by default, using the same contact code as
with rivals. Lobby option **ghost mode** (players pass through each other,
still collide with AI and traffic), for uneven connections or for a time
trial feel.

### 2.5 Bonuses and nitro

**Proposed: unchanged and per player.** Each player earns their own nitro
from drifts and near misses, and their own perfect start. Near misses count
traffic only, as now (a near miss on another human would reward bumping).

### 2.6 Traffic

On, as in single-player, from the shared seed, so everyone meets the same
cars. A traffic car hit by one player is hit for everyone.

### 2.7 Dropping out

As SPEC 9.6: **a player who disconnects is driven by the AI** (a rival's
driver, at a rival's skill) to the finish, so the race the others see
doesn't change shape. Their result is marked "finished by AI". **Proposed
extra:** if they reconnect before the finish, they take their car back
from the next tick.

### 2.8 Finishing

**Proposed:** the race ends when every connected human has finished, or
**45 seconds after the first human finishes**, whichever comes first, with
the countdown shown to the others. Anyone unfinished then is placed by
race progress and marked DNF. AI rivals still parking or racing are placed
as today.

### 2.9 Pausing

There is no pause. **Proposed:** opening the menu during a race takes the
player's hands off the wheel (no throttle, no steering) and shows them as
"away" on the others' screens. Leaving the race from the menu is a drop-out
(2.7).

### 2.10 Driving aids

The driving aids (the guide line and steering assist, D1080–D1084) are
each player's own setting. **Requirement:** whatever an aid does to the car
must reach the simulation through that player's input (the `InputFrame`),
or as a per-player flag in it, so every device simulates the same thing.
Purely visual aids (the guide line) stay local. To check in WP 10.6.

## 3. On screen

- **Name tags** above the other humans' cars (fading with distance), in
  their chosen colour.
- **Position and gaps** count humans and AI alike; the leaderboard marks
  humans.
- **The finish countdown** (2.8) and "away" or "AI driving" markers (2.7,
  2.9).
- Each player's own camera, HUD, sound and settings. Other humans' engines
  are heard like rivals' engines.
- **Colours:** each player picks a car and a colour; two players in the
  same car and colour get the second one's colour shifted, so cars stay
  told apart.

## 4. Lobby and session

1. **Host** (the person running `mr-host`, or in M11 a tab) opens a lobby:
   level, laps, AI fill, rubber-banding, collisions or ghost, grid rule.
2. **Players join** by address or by scanning a QR code on the host's
   screen; they pick a name, car and colour, and press ready.
3. **The host starts** when everyone is ready. The countdown runs on the
   shared tick, so it ends at the same moment everywhere.
4. **Results** list everyone. Then back to the lobby, where the host picks
   the next race; the session keeps a points table across races (10, 8, 6,
   5, 4, 3, 2, 1 for humans and AI alike).
5. **Late joiners** wait in the lobby until the current race ends (a
   spectator view is a later feature).

## 5. Parameters (from SPEC 9.2)

Tick 1/120 s; input delay one tick on a LAN; rollback window 32 ticks
(267 ms), beyond which a device resynchronises from a snapshot; snapshots
with a state hash every 30 ticks; inputs sent with the last few ticks
repeated so one lost packet costs nothing.

## 6. How it is tested

- **Headless soak** (WP 10.3): eight simulated clients for an hour over the
  lossy test transport (latency, jitter, loss, reordering): no desync a
  snapshot doesn't repair.
- **Contact** (WP 10.4): two cars side by side at 50 ms simulated latency
  look the same on both screens.
- **Rules** (WP 10.6): unit tests for each rule above: nearest-human
  rubber-banding (one human reproduces today's numbers exactly), grid
  orders, drop-out takeover and return, the finish countdown.
- **End to end** (WP 10.7): four browser tabs complete a race.
- **Owner:** a race with real people over the tailnet.

## 7. Questions for the owner

1. Rubber-banding to the nearest human (2.1): agreed?
2. Grid: reverse order after the first race (2.2): agreed, or random every
   time?
3. Collisions on by default, ghost as an option (2.4)?
4. The 45-second finish countdown (2.8): right length?
5. A points table across a session's races (4.4): wanted?
