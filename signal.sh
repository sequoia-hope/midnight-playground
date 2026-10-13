#!/usr/bin/env bash
# signal.sh — run mp-signal, the WebRTC signalling server for joining by
# link (docs/rust-port/MULTIPLAYER.md 8.3), on this project's registered
# "signal" port.
#
# The number lives in the registry (~/scripts/launcher.toml), as the
# project's `web_extra` entry named "signal"; never in this file. In order:
# --port, $SIGNAL_PORT, the registry; there is no fallback default. (Not
# $PORT: the hub exports that for the game's own server, serve.sh.)
#
# It binds 127.0.0.1 only. The public front is Tailscale Funnel, set up
# once, which proxies https://<machine>.<tailnet>.ts.net:<funnel port>/<path>
# to it; the game is then given wss://<that>/ as its signalling server.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

PORT_ARG=""
if [ "${1:-}" = "--port" ]; then PORT_ARG="${2:?--port needs a number}"; fi
# The registry library, with the interpreter `proj` itself runs on.
PY="$HOME/scripts/.venv/bin/python3"
SIGNAL_PORT="${PORT_ARG:-${SIGNAL_PORT:-$("$PY" - "$PWD" <<'PY'
import os, sys
sys.path.insert(0, os.path.expanduser("~/scripts"))
import registry
cfg = registry.load()
for p in registry.projects_for_dir(cfg, sys.argv[1]):
    for e in p.get("web_extra", []):
        if e.get("name") == "signal" and e.get("port"):
            print(e["port"]); sys.exit(0)
sys.exit("signal.sh: no web_extra entry named \"signal\" for this directory in the registry")
PY
)}}"

cargo build --release -q -p mp_host
exec target/release/mp-signal --port "$SIGNAL_PORT" --bind 127.0.0.1
