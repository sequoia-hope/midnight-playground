#!/usr/bin/env python3
"""Streaming probe: can a browser on a weak device play a WebRTC stream from
this machine, and send its keys and gamepad back, fast enough for a game?

Sends a 1280x720 test pattern (a moving bar, a frame counter, and a marker
square that turns white for a few frames after the viewer's "ping") and a
440 Hz tone, over WebRTC (aiortc). The viewer page (`viewer.html`) shows the
decode stats, the keys and pads it sees, and the press-to-picture time, and
sends all of it back here, where it is printed and appended to
`probe-log.jsonl` beside this file.

    python probe.py [--port N] [--fps 60] [--codec h264|vp8]

The port: --port, then $PORT, then the registry's "stream" listener for this
project (`~/scripts/launcher.toml`, web_extra), then failure.
"""

import argparse
import asyncio
import fractions
import json
import os
import sys
import time
from pathlib import Path

import numpy as np
from aiohttp import web
from aiortc import (MediaStreamTrack, RTCPeerConnection, RTCRtpSender,
                    RTCSessionDescription)
from av import AudioFrame, VideoFrame

HERE = Path(__file__).resolve().parent
LOG = HERE / "probe-log.jsonl"
W, H = 1280, 720


def registry_port():
    """The "stream" listener registered for the project holding this file."""
    import tomllib
    reg = Path.home() / "scripts" / "launcher.toml"
    if not reg.exists():
        return None
    data = tomllib.loads(reg.read_text())
    best = None
    for p in data.get("projects", []):
        d = Path(p.get("dir", "")).expanduser()
        if HERE == d or d in HERE.parents:
            if best is None or len(str(d)) > len(str(best[0])):
                best = (d, p)
    if not best:
        return None
    for e in best[1].get("web_extra", []):
        if e.get("name") == "stream":
            return int(e["port"])
    return None


def log(kind, **kw):
    rec = {"t": round(time.time(), 3), "kind": kind, **kw}
    line = json.dumps(rec)
    print(line, flush=True)
    with LOG.open("a") as f:
        f.write(line + "\n")


# A 3x5 digit font, scaled up for the frame counter.
DIGITS = {
    "0": "111101101101111", "1": "010110010010111", "2": "111001111100111",
    "3": "111001111001111", "4": "101101111001001", "5": "111100111001111",
    "6": "111100111101111", "7": "111001001001001", "8": "111101111101111",
    "9": "111101111001111",
}


def draw_text(img, text, x, y, s):
    for ch in text:
        bits = DIGITS.get(ch)
        if bits:
            for i, b in enumerate(bits):
                if b == "1":
                    r, c = divmod(i, 3)
                    img[y + r * s:y + (r + 1) * s, x + c * s:x + (c + 1) * s] = 255
        x += 4 * s


class Pattern(MediaStreamTrack):
    kind = "video"

    def __init__(self, fps):
        super().__init__()
        self.fps = fps
        self.n = 0
        self.t0 = None
        self.flash = 0
        base = np.zeros((H, W, 3), np.uint8)
        base[:, :, 0] = np.linspace(20, 60, W, dtype=np.uint8)[None, :]
        base[:, :, 2] = np.linspace(40, 90, H, dtype=np.uint8)[:, None]
        self.base = base

    async def recv(self):
        if self.t0 is None:
            self.t0 = time.monotonic()
        target = self.t0 + self.n / self.fps
        delay = target - time.monotonic()
        if delay > 0:
            await asyncio.sleep(delay)
        img = self.base.copy()
        bx = int((self.n * 12) % W)
        img[:, bx:bx + 40, 1] = 220  # the moving bar: motion to judge smoothness
        draw_text(img, str(self.n), 40, 520, 24)
        if self.flash > 0:
            img[40:240, 40:240] = 255  # the marker the viewer samples
            self.flash -= 1
        frame = VideoFrame.from_ndarray(img, format="rgb24")
        frame.pts = self.n
        frame.time_base = fractions.Fraction(1, self.fps)
        self.n += 1
        return frame


class Tone(MediaStreamTrack):
    kind = "audio"

    def __init__(self):
        super().__init__()
        self.sr, self.n, self.t0 = 48000, 0, None

    async def recv(self):
        if self.t0 is None:
            self.t0 = time.monotonic()
        samples = 960  # 20 ms
        target = self.t0 + self.n / self.sr
        delay = target - time.monotonic()
        if delay > 0:
            await asyncio.sleep(delay)
        t = (np.arange(samples) + self.n) / self.sr
        pcm = (np.sin(2 * np.pi * 440 * t) * 3000).astype(np.int16)
        frame = AudioFrame.from_ndarray(pcm.reshape(1, -1), format="s16", layout="mono")
        frame.sample_rate = self.sr
        frame.pts = self.n
        frame.time_base = fractions.Fraction(1, self.sr)
        self.n += samples
        return frame


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int)
    ap.add_argument("--fps", type=int, default=60)
    ap.add_argument("--codec", choices=["h264", "vp8"], default="h264")
    a = ap.parse_args()
    port = a.port or (int(os.environ["PORT"]) if os.environ.get("PORT") else None) or registry_port()
    if not port:
        sys.exit("probe: no port (--port, $PORT, or a 'stream' web_extra for this project in ~/scripts/launcher.toml)")

    pcs = set()

    async def index(_req):
        return web.FileResponse(HERE / "viewer.html", headers={"Cache-Control": "no-store"})

    async def offer(req):
        params = await req.json()
        pc = RTCPeerConnection()
        pcs.add(pc)
        video = Pattern(a.fps)
        log("connect", ua=req.headers.get("User-Agent"), peer=req.remote, fps=a.fps, codec=a.codec)

        @pc.on("datachannel")
        def on_dc(ch):
            @ch.on("message")
            def on_msg(m):
                try:
                    ev = json.loads(m)
                except ValueError:
                    return
                if ev.get("type") == "ping":
                    video.flash = 6
                log("viewer", **ev)

        @pc.on("connectionstatechange")
        async def on_state():
            log("state", state=pc.connectionState)
            if pc.connectionState in ("failed", "closed"):
                await pc.close()
                pcs.discard(pc)

        await pc.setRemoteDescription(RTCSessionDescription(sdp=params["sdp"], type=params["type"]))
        for t in pc.getTransceivers():
            if t.kind == "video":
                pc.addTrack(video)
                caps = RTCRtpSender.getCapabilities("video").codecs
                want = "video/H264" if a.codec == "h264" else "video/VP8"
                pref = [c for c in caps if c.mimeType == want] + [c for c in caps if c.mimeType != want]
                t.setCodecPreferences(pref)
            elif t.kind == "audio":
                pc.addTrack(Tone())
        await pc.setLocalDescription(await pc.createAnswer())
        return web.json_response({"sdp": pc.localDescription.sdp, "type": pc.localDescription.type})

    async def on_shutdown(_app):
        await asyncio.gather(*[pc.close() for pc in pcs])

    app = web.Application()
    app.router.add_get("/", index)
    app.router.add_post("/offer", offer)
    app.on_shutdown.append(on_shutdown)
    print(f"probe: serving on 0.0.0.0:{port} (fps {a.fps}, {a.codec})", flush=True)
    web.run_app(app, host="0.0.0.0", port=port, print=None)


if __name__ == "__main__":
    main()
