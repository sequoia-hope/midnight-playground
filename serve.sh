#!/usr/bin/env bash
# serve.sh — serve this project on its registered port.
#
# The number lives in the registry (~/scripts/launcher.toml), never in this
# file: $PORT when the hub started us, otherwise `proj port`, which resolves
# this directory to the project registered for it. There is deliberately no
# fallback default — a project that cannot find its port should say so rather
# than quietly bind whatever looked free.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

PROJ="$(command -v proj || echo "$HOME/scripts/proj")"
PORT="${PORT:-$("$PROJ" port)}"

# An https front on the tailnet, at https://<machine>.<tailnet>.ts.net/midnight-racer/
# (mind the trailing slash). Phones only hand the tilt sensor, and WebGPU, to
# https pages, and this server speaks plain http. Pointed at $PORT on every
# start, so the proxy follows the registry if the port is ever reallocated.
# Tailnet only, and best-effort: without tailscale the plain address still works.
tailscale serve --bg --set-path /midnight-racer "http://127.0.0.1:$PORT" >/dev/null 2>&1 \
  || echo "serve.sh: no tailnet https front (tailscale serve failed); plain http only" >&2

# MP_HOST=1 serves with mp-host instead: the same files and cache rules,
# plus the multiplayer lobby and races over a WebSocket at …/ws (roadmap
# WP 10.5). Built on demand; the Python server stays the default until it
# has been lived with.
if [ "${MP_HOST:-}" = 1 ]; then
  cargo build --release -q -p mp_host
  exec target/release/mp-host --port "$PORT" --bind 0.0.0.0
fi

# tools/serve.py is http.server with cache headers, so phones never mix old
# and new modules.
exec python3 tools/serve.py --port "$PORT" --bind 0.0.0.0
