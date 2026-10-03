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

# tools/serve.py is http.server with cache headers, so phones never mix old
# and new modules.
exec python3 tools/serve.py --port "$PORT" --bind 0.0.0.0
