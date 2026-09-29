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

# tools/serve.py is http.server with cache headers, so phones never mix old
# and new modules.
exec python3 tools/serve.py --port "$PORT" --bind 0.0.0.0
