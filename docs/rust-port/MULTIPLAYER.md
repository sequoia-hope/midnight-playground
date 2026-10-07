# Multiplayer: race together (design note, ROADMAP WP 10.1)

Status: **approved by the owner (2026-10-06)** as proposed; the open points
are in section 7. SPEC section 9 fixes the
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
   the next race.
5. **The points table** (approved, on by default): a running score across
   the session's races, like a cup. Points by finishing position: 10, 8,
   6, 5, 4, 3, 2, 1, for humans and AI alike; DNF scores nothing; no
   fastest-lap bonus at first. After each race's results, the table shows
   each player's total, places gained or lost, and the gap to the leader.
   The host sets **races per session** in the lobby (open-ended, 3, 4 or
   6); a set number ends with final standings and a winner. The lobby
   then shows the final standings; the leader's next Start begins a new
   session with an empty table. With the
   reverse grid (2.2) the leader starts at the back, which keeps it close.
   The table lives in the host's session state, not the simulation.
6. **Late joiners** wait in the lobby until the current race ends (a
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

## 6.1 Direction for M11: join by link, own signalling server

Decided with the owner (2026-10-06). The first test is with coworkers on
one LAN and tailnet, which M10's native host covers as specified. M11 then
targets the owner's main interest, **a browser tab as host, joined by a
link** shared in Slack, Signal, Discord or a stream's chat:

- **The link is an invitation, not the handshake.** It opens the game from
  GitHub Pages with a room code and a secret after the `#`
  (`…/#join=<room>.<secret>`). Host and guests swap their WebRTC offer
  and answer through a signalling server, automatically; the guest sees
  only "Joining…".
- **The secret encrypts and authenticates every signalling message**, so
  the signalling server sees only ciphertext under an opaque room id, and
  nobody without the link can join or tamper. Browsers never send the `#`
  part to a server.
- **The owner runs the signalling server**, rather than using public
  relays (Nostr, MQTT, BitTorrent trackers), for control over logs and
  reliability. First choice: `matchbox_server` behind Caddy (automatic
  https) on the owner's existing cloud Linux machine, keeping no logs.
  Alternative: a Cloudflare Worker with a Durable Object per room.
- **Players see each other's network addresses**, as in any peer-to-peer
  game (guests see the host's, the host sees each guest's). The lobby says
  so in one line. A TURN relay that hides addresses, and that rescues
  connections across strict networks, is measured before it is decided.
- **A public link means strangers can join:** the lobby gets a player cap,
  kick, and an "approve each join" option. The game has no text or voice
  chat, which keeps moderation small.
- **Guests can only send their own controls**, which the host validates;
  the host is the authority and could cheat, which is acceptable among
  friends.

## 6.2 What is built (2026-10-06)

- **WP 10.2** `mp_net::proto` (messages, encoding, fuzzed decoding) and
  `mp_net::transport` (the `Transport` trait; `SimNet`, an in-process
  network with latency, jitter, loss, reordering and cuts).
- **WP 10.6 (the simulation's part)** `SimState::new_multi` and a
  per-player `step` (section 2's rules). One human is single-player
  exactly, tick for tick, on every level (`mp_sim/tests/multi.rs`).
- **WP 10.3, 10.4** `mp_net::host` (lobby, leader, authoritative race,
  input relay, hashes, rejoin, grid rules, points) and `mp_net::client`
  (prediction, rollback, clock, hash checks, rebuild). Soak:
  `cargo test --release -p mp_net -- --ignored` races eight players on
  mixed links to the points with no desync.
- **WP 10.5** `mp-host` (files, WebSocket at `…/ws`, the session).
  `MP_HOST=1 ./serve.sh` runs it on the project's port.
- **WP 10.7** the game client (`mp_game::net`): a WebSocket transport in
  the browser (`ws` beside the page) and natively (`?join=ws://…/ws`), the
  lobby screen (name, car, colour, ready; the leader's level, AI fill,
  contact, rubber-banding, grid, races and Start; the points table), the
  race driven by the network client (`Race::online`, `frame_online`), the
  other humans' cars, engines and minimap dots, name tags over their cars,
  the results with every human's name, and Back to the lobby. The menu
  does not stop an online race: it takes the player's hands off the wheel.
  `net::tests` races two game clients on the in-process network from the
  lobby to the results.
- **To try it:** `MP_HOST=1 ./serve.sh`, open the project's address plus
  `dist/next/` on each machine, press Multiplayer. The first player in
  leads and starts the race.
- **Not yet:** a browser end-to-end suite with two tabs, and a real test
  across machines.
- **Found in the owner's first try (2026-10-07), two desktop windows:**
  - **No way to end a race from the game.** The host can abort
    (`Msg::Go(false)`, `Host::abort`), but no control sends it. And the
    host keeps a race running when every player has left
    (`Host::drop_peer` only marks the player gone), so pressing
    Multiplayer again rejoins that race instead of opening the lobby.
    Wanted: the leader can end the race from the pause menu, and a race
    with no one connected ends by itself, back to the lobby.
    **Done (2026-10-07):** the pause menu's "End the race for everyone"
    (the leader only) sends the abort, and every client whose race ended
    before its results goes back to the lobby; the host ends a race (no
    points) when its last connected player goes; and Leave, unlike a
    dropped connection, doesn't take your car back in the race you left:
    pressing Multiplayer again waits in the lobby for the next one
    (`host_rules.rs`: `when_everyone_has_gone_…`,
    `a_player_who_leaves_on_purpose_…`).
  - **Frame stutter.** Not multiplayer's: the web build stuttered in
    single player too, and the machine's GPU was at 98% from a leaked
    headless Chrome. Two headless clients on localhost held 60 fps with no
    desyncs, a rollback most frames that re-ran about two ticks, and a
    steady drawn clock after the first second. `window.__mp.net` now shows
    the connection's counters (rollbacks, re-run ticks, desyncs, rebuilds,
    ticks ahead, stalls) and the drawn camera and cars, for the next look.

Two choices differ from SPEC 9.1 and 9.2 (DECISIONS records them):

- **No state snapshots on the wire.** The host sends a state hash every 30
  ticks; a client that disagrees rebuilds the race from its start and the
  confirmed inputs, which it already has. The simulation is deterministic
  across devices by construction (the math kernel), so a mismatch can only
  come from a bug, which the rebuild then also shows (it is counted). This
  avoids serialising the whole simulation state, and a rejoining player is
  sent the inputs so far instead of a snapshot.
- **The host's relayed inputs travel on the reliable channel.** Clients'
  inputs go unreliable with the last eight ticks repeated; the host's
  final inputs must all arrive, in order, and WebSocket is reliable
  anyway. WebRTC (M11) may move them to unreliable with redundancy.

## 7. Open points

The rules in section 2 are approved as proposed. Still open:

1. **Guests outside the tailnet** (SPEC 9.5): default for the first
   release is to invite them to the tailnet; a public name with a
   certificate, or waiting for M11's WebRTC, are the alternatives.
2. **The 45-second finish countdown** (2.8): length.

## 8. M11 plan: WebRTC, the signalling server, a tab as host

Written 2026-10-07 at the start of M11. The owner has said the Rust game
no longer needs to match the JS one, so nothing here is held to parity.
DECISIONS D1120 to D1127 record the choices below.

### 8.1 The pieces

1. **`mp_net::rtc`, the WebRTC transport** (feature `rtc`, D1120). A
   `Transport` over `matchbox_socket` 0.14.0, the same code natively
   (webrtc-rs) and in the browser (the page's `RTCPeerConnection`). Two
   data channels: 0 reliable and ordered, 1 unreliable and unordered (no
   retransmits), so `Channel::Unreliable` is finally a real one. As with
   WebSocket, a guest sees the host as peer 0; the host numbers guests
   from 1. The host's relayed inputs stay on the reliable channel for now
   (section 6.2's note).
2. **`mp_net::signal`, the sealed signalling** (D1121, D1122). No I/O,
   so every rule is a unit test: the join link
   (`#join=<room>.<secret>`), the room key, sealing and opening each
   signalling payload, and the small state machine that turns the
   signalling server's full mesh into a star (only the host makes WebRTC
   offers; guests never connect to each other). `rtc` runs it inside a
   matchbox `Signaller`.
3. **The signalling server** (D1123): `mp-signal`, a second binary of
   `mp_host`, speaking matchbox's protocol (so a stock `matchbox_server`
   works too), relaying only sealed payloads, keeping no logs. `mp-host`
   also answers it at `…/signal/<room>`, so a LAN or a test needs nothing
   from the internet. The port rule is `mp-host`'s.
4. **`transport::Mux`** (D1125): several transports as one, renumbering
   peers. The host tab's `Host` listens on the in-process loopback (its
   own player) and on WebRTC (everyone else); `mp-host --room` listens on
   WebSocket and WebRTC at once.
5. **The tab as host** (`mp_game::net`, D1125): the menu's Multiplayer
   offers "Host a game" when the page has a signalling server. The tab
   runs `mp_net::host::Host` (the same session `mp-host` runs) on its own
   clock, its own player joins it through the loopback, and the lobby
   shows the invitation link with a Copy button and the line "Players see
   each other's network addresses".
6. **Join by link** (D1124): a page opened with `#join=<room>.<secret>`
   goes straight to the lobby and joins over WebRTC; natively,
   `--join '<link>'` does the same. The `#` part never reaches a server.
7. **The native host as a peer** (D1126): `mp-host --room` opens a room
   on the signalling server, prints the link, and takes WebRTC guests as
   well as its WebSocket ones.

### 8.2 Order and tests

| Step | Test |
|---|---|
| `signal`: link, key, seal/open, the star | Unit tests: round trips; a wrong secret, a tampered byte, a redirected or re-attributed message are all refused; every join order (host first, guests first, guest after guest) ends with exactly the host-guest pairs offering |
| `Mux` | Unit tests over two `SimNet`s: renumbering, sends and closes reach the right inner peer, disconnects |
| The signalling server | `mp_host` tests over real WebSockets on an ephemeral port: ids, new peers, relaying, rooms kept apart, peers leaving, limits |
| `rtc` natively | An `mp_host` test: `mp-signal` on an ephemeral port, a `Host` and two `Client`s on `RtcNet` in-process, lobby to race start, inputs and hashes flowing with no desync; a guest with the wrong secret never connects |
| The tab as host, join by link | `mp_game` unit tests: the link parsed from the page, the commands, the host-and-loopback session racing a remote client to the results on `SimNet` (the WebRTC part is covered above); `cargo xtask web` builds |
| By hand (owner) | Two browsers on different networks, one hosting from Pages, the other joining by the link |

Lossy-network behaviour is already covered by `SimNet` (the session code
is the same over any transport); WebRTC adds only the real channels.

### 8.3 What the owner sets up

- **The signalling server.** `cargo build --release -p mp_host` gives
  `target/release/mp-signal`; run it on the cloud machine with
  `--port` or `$PORT`, behind Caddy for https (`reverse_proxy` to it).
  Then give the address to the Pages build: the workflow's
  `MP_SIGNAL_URL` (a repository variable, e.g.
  `wss://signal.example.com/`), or for a test the page's
  `?signal=wss://…/` parameter.
- **TURN** (optional, later): matchbox's default ICE server is Google's
  public STUN. Players behind strict NATs will need a TURN relay; it is
  measured before it is decided (section 6.1).

## 9. Hot Pursuit and cruise with several players (design, for the owner)

Draft, 2026-10-07; nothing below is built. Today `SimState::new_multi`
turns pursuit off and treats the cruise loop as a race level, and the
pursuit holds only player 0 (`mp_sim::race`, "Hot Pursuit is
single-player"). The machinery (every device simulating everything, the
host's inputs final) carries over unchanged: police, heat and roadblocks
come from the shared seed and the confirmed inputs, so every device sees
the same chase. What needs deciding is the rules. Each **Proposed** is a
default the owner can change.

### 9.1 Hot Pursuit race (sprint levels)

- **Who the police chase.** They already chase "whichever racer they're
  closest to" (hot-pursuit.md 1.1), rivals included. **Proposed:** humans
  and AI alike, by the same rule, with no preference for humans. A
  human who is being chased pulls units off everyone else, which is the
  fun of it.
- **Heat.** **Proposed: one shared heat** for the race, raised by
  anyone's events (hot-pursuit.md 3), capped per zone as now. Per-player
  heat would need per-player unit pools and roadblocks, which the
  pursuit system doesn't have. The HUD's stars show the shared heat.
- **Busts and wrecks.** As single-player since the 2026-09-29 change:
  the car is held for the penalty, then put back ahead of the police
  with grace. **Proposed:** the same for every human; the held player
  sees the hold card, everyone else sees the parked car with the cruiser
  behind it.
- **Damage** per car, as now.
- **The radio line and the music's mood** are each player's own, from
  the units chasing them (or the nearest pursuit when none is).
- **Finishing** as section 2.8.

### 9.2 Cruise (Night City Cruise)

- **Scoring.** Each player has their own score, multiplier and chain, as
  single-player. **Proposed:** a session runs for a set time (lobby: 3,
  5 or 10 minutes) and ranks by score; the points table (4.5) awards by
  that rank.
- **Near misses** count traffic only (as 2.5): passing close to another
  human scores nothing, so nobody is rewarded for bumping.
- **Crashes** cost the multiplier as now, including into another human.
  **Proposed:** ghost mode (2.4) defaults on in cruise, since the loop
  has no grid and players meet head-on on the freeway.
- **Start.** No grid: players start spread along the loop (by slot,
  evenly), so they don't all pile into the same traffic.

### 9.3 Most Wanted (pursuit on the cruise loop)

Not built in single-player yet (hot-pursuit.md 1.2). **Proposed:** wait
for the single-player mode, then a shared heat and a bounty per player:
each player's own escape banks their own bounty; a bust costs only the
busted player.

### 9.4 Open questions for the owner

1. Police: chase humans and AI alike (proposed), or prefer humans?
2. One shared heat (proposed), or per-player heat with its own units?
3. Cruise: a timed session ranked by score (proposed), or open-ended
   with a live leaderboard and no end?
4. Ghost mode on by default in cruise (proposed)?
5. Should a human be able to play as the police (hot-pursuit.md 1.3's
   v2) once several humans are in a pursuit? It changes the input frame
   (a police car's controls) and the results.
6. Most Wanted: wait for single-player first (proposed)?
