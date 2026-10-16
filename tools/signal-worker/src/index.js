// The signalling relay as a Cloudflare Worker (DECISIONS D1222): the same
// relay as `mp-signal` (crates/mp_host/src/signal.rs, D1123), one Durable
// Object per room, so a game can be introduced from anywhere without a
// machine of the owner's to keep up.
//
// It speaks matchbox 0.14's JSON protocol on `/<room>` (any path's last
// segment): `IdAssigned` to a newcomer, `NewPeer` to everyone already in
// the room, `Signal` passed to its receiver with the sender filled in,
// `PeerLeft` when someone goes. Full mesh, as matchbox_server is; the game
// makes a star of it inside the sealed payloads (D1121), and seals every
// payload with the link's secret (D1122), so all this sees is a room id,
// peer ids it made up, and ciphertext. It writes nothing down: no
// console output, and observability is off in wrangler.toml.
//
// The room uses the WebSocket Hibernation API: between messages the
// object can be evicted and costs nothing, so everything it knows lives
// on the sockets themselves (each one's attachment holds its peer id and
// when it was last heard), never in a field of the class.

import { DurableObject } from 'cloudflare:workers';

// The same limits as mp-signal (signal.rs), bar the count of rooms: each
// room is its own object, so there is no one place to count them.
const MAX_PEERS = 16;
const MAX_MESSAGE = 64 * 1024;
// A peer that says nothing for this long is dropped (matchbox sends a
// keep-alive every 10 s).
const IDLE_MS = 60_000;

const ROOM = /^[A-Za-z0-9_-]{1,64}$/;

/** A room id as matchbox clients send it: the path's last segment. */
function roomOf(pathname) {
  const room = pathname.replace(/\/+$/, '').split('/').pop();
  return ROOM.test(room) ? room : null;
}

/** Whether a page at `origin` may use the relay: any, unless the
 * ALLOWED_ORIGINS variable lists some (comma-separated). A native client
 * sends no Origin and is always let in. */
function allowed(origin, list) {
  if (!origin || !list) return true;
  return list.split(',').map(s => s.trim()).filter(Boolean).includes(origin);
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const room = roomOf(url.pathname);
    if (request.headers.get('Upgrade') !== 'websocket') {
      return new Response('A signalling relay for WebRTC games: connect a WebSocket to /<room>.\n', {
        status: room ? 426 : 200,
        headers: { 'content-type': 'text/plain; charset=utf-8' },
      });
    }
    if (!room) return new Response('no such room\n', { status: 404 });
    if (!allowed(request.headers.get('Origin'), env.ALLOWED_ORIGINS)) {
      return new Response('not from here\n', { status: 403 });
    }
    return env.ROOMS.get(env.ROOMS.idFromName(room)).fetch(request);
  },
};

const text = e => JSON.stringify(e);

/** The size of a message in bytes, counting a string as UTF-8. */
function bytes(m) {
  if (typeof m !== 'string') return m.byteLength;
  // Every UTF-16 unit is at most three bytes: skip encoding the small ones.
  return m.length * 3 <= MAX_MESSAGE ? m.length : new TextEncoder().encode(m).length;
}

/** A matchbox PeerRequest, or null for anything else. */
function request(m) {
  let v;
  try {
    v = JSON.parse(m);
  } catch {
    return null;
  }
  if (v === 'KeepAlive') return { keepAlive: true };
  const s = v && typeof v === 'object' && !Array.isArray(v) && Object.keys(v).length === 1 ? v.Signal : undefined;
  if (s && typeof s === 'object' && typeof s.receiver === 'string' && 'data' in s) {
    return { receiver: s.receiver, data: s.data };
  }
  return null;
}

export class Room extends DurableObject {
  /** The peers here now, in the order they came: [socket, attachment]. */
  peers() {
    const out = [];
    for (const ws of this.ctx.getWebSockets()) {
      const a = ws.deserializeAttachment();
      if (a && !a.gone) out.push([ws, a]);
    }
    return out.sort((x, y) => x[1].n - y[1].n);
  }

  async fetch() {
    const [client, server] = Object.values(new WebSocketPair());
    const here = this.peers();
    if (here.length >= MAX_PEERS) {
      // A full room says no, as mp-signal does: the handshake succeeds and
      // the socket is closed straight away.
      server.accept();
      server.close(1008, 'room full');
      return new Response(null, { status: 101, webSocket: client });
    }
    this.ctx.acceptWebSocket(server);
    const id = crypto.randomUUID();
    const n = here.reduce((m, [, a]) => Math.max(m, a.n), 0) + 1;
    server.serializeAttachment({ id, n, heard: Date.now() });
    server.send(text({ IdAssigned: id }));
    for (const [ws] of here) this.send(ws, { NewPeer: id });
    await this.watch();
    return new Response(null, { status: 101, webSocket: client });
  }

  send(ws, e) {
    try {
      ws.send(text(e));
    } catch {
      // Closing already; its webSocketClose tells the others.
    }
  }

  async webSocketMessage(ws, m) {
    const a = ws.deserializeAttachment();
    if (!a || a.gone) return;
    if (bytes(m) > MAX_MESSAGE) return this.drop(ws, 1009, 'too big');
    a.heard = Date.now();
    ws.serializeAttachment(a);
    if (typeof m !== 'string') return; // matchbox speaks text; anything else is only a sign of life
    const r = request(m);
    // Anything else is a client that isn't ours.
    if (!r) return this.drop(ws, 1003, 'not matchbox');
    if (r.keepAlive) return;
    const to = this.peers().find(([, p]) => p.id === r.receiver);
    if (to) this.send(to[0], { Signal: { sender: a.id, data: r.data } });
  }

  async webSocketClose(ws, code) {
    this.leave(ws);
    try {
      ws.close(code === 1005 || code === 1006 ? 1000 : code, 'bye');
    } catch {
      // Already closed.
    }
  }

  async webSocketError(ws) {
    this.leave(ws);
  }

  /** Closes `ws` from this side and tells the others it has gone. */
  drop(ws, code, why) {
    this.leave(ws);
    try {
      ws.close(code, why);
    } catch {
      // Already closed.
    }
  }

  /** Marks `ws` gone (once) and says `PeerLeft` to everyone still here. */
  leave(ws) {
    const a = ws.deserializeAttachment();
    if (!a || a.gone) return;
    ws.serializeAttachment({ ...a, gone: true });
    for (const [other] of this.peers()) this.send(other, { PeerLeft: a.id });
  }

  /** Keeps an alarm set while anyone is here, to drop the silent. */
  async watch() {
    if ((await this.ctx.storage.getAlarm()) === null) {
      await this.ctx.storage.setAlarm(Date.now() + IDLE_MS / 2);
    }
  }

  async alarm() {
    const now = Date.now();
    for (const [ws, a] of this.peers()) {
      if (now - a.heard > IDLE_MS) this.drop(ws, 1001, 'idle');
    }
    if (this.peers().length) await this.ctx.storage.setAlarm(now + IDLE_MS / 2);
    else await this.ctx.storage.deleteAll();
  }
}
