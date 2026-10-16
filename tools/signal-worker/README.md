# The signalling relay as a Cloudflare Worker

The relay that introduces WebRTC peers for joining a game by link
(MULTIPLAYER 6.1 and 8, DECISIONS D1222), as a Worker with one Durable
Object per room. It is the same relay as `mp-signal`
(`crates/mp_host/src/signal.rs`, D1123), so nothing of the owner's has to
stay up for people to find each other. The game's traffic never passes
through it: once introduced, the peers talk directly.

It speaks matchbox 0.14's protocol on `/<room>` (any path's last
segment), so it works for the game and for any other game built on
`matchbox_socket` the same way: point the client at
`wss://<worker>/` and use a room per game. The game seals every payload
with the link's secret (D1122), so the relay sees only room ids, peer ids
it made up, and ciphertext. It logs nothing.

| File | What |
|---|---|
| `src/index.js` | The Worker (routes `/<room>` to its room) and `Room`, the Durable Object |
| `wrangler.toml` | Name `mp-signal`, the `ROOMS` binding, observability off, the optional `ALLOWED_ORIGINS` |
| `package.json` | `wrangler` pinned, for `npm run dev` and `npm run deploy` |

## Testing it

`mp-signal`'s own test suites run against the Worker under `wrangler dev`
(each test starts one on a free port, with its own state):

```
cd tools/signal-worker && npm install && cd ../..
MP_SIGNAL_WORKER=1 cargo test -p mp_host --test signal --test rtc
```

`signal` holds the relay's conversation (introductions, relaying, rooms
apart, leaving), the full room and the refusals; `rtc` connects a host and
guests natively over WebRTC through it and races two players.

## Deploying it (the owner)

1. A Cloudflare account; the free plan is enough (SQLite-backed Durable
   Objects).
2. `cd tools/signal-worker && npm install && npx wrangler login`, then
   `npm run deploy`. Wrangler prints the address,
   `https://mp-signal.<account>.workers.dev`.
3. Try it before building it in: open the game with
   `?signal=wss://mp-signal.<account>.workers.dev/`, press Multiplayer,
   Host a game, and open the link on another device.
4. Build it in: set the repository variable `MP_SIGNAL_URL` to
   `wss://mp-signal.<account>.workers.dev/` (Settings, Secrets and
   variables, Actions, Variables); the Pages workflow passes it to the
   build (D1124).
5. Optionally, `ALLOWED_ORIGINS = "https://sequoia-hope.github.io"` under
   `[vars]` in `wrangler.toml` turns away pages from anywhere else (the
   local server's tailnet address would need adding to test from there).
   Native clients send no Origin and always get in.

A custom domain (a route on a zone of the owner's) works the same way;
only the address changes.

## Not yet

- **TURN.** Players behind strict NATs need a relay for the game's traffic
  itself. Cloudflare's TURN service would fit (the Worker mints
  short-lived credentials with an API token, the clients fetch them
  before connecting), but it is metered bandwidth on the owner's account,
  so it waits for a measurement that it is needed and a decision on who
  may ask for credentials (D1222).
