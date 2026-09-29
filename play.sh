#!/usr/bin/env bash
# play.sh — run Midnight Racer in its own window (Electron).
#
# No web server or port is involved: the desktop shell serves the project
# folder itself through a private app:// scheme. Extra arguments are passed
# to the shell, e.g. `./play.sh --dev` (F12 devtools) or
# `./play.sh --url-query stats=1`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

ELECTRON_DIR=node_modules/electron
if [ ! -d "$ELECTRON_DIR" ]; then
  echo "Installing the desktop runtime (first run only)…"
  npm install --no-audit --no-fund
fi
# npm sometimes skips Electron's binary download; fetch it if missing.
if [ ! -x "$ELECTRON_DIR/dist/electron" ]; then
  node "$ELECTRON_DIR/install.js"
fi

# Chromium's sandbox needs either a root-owned setuid chrome-sandbox helper
# or unprivileged user namespaces. Ubuntu's AppArmor blocks the latter for
# unconfined apps, so fall back to --no-sandbox there. The game only loads
# its own local files. Force either way with MR_SANDBOX=1 / MR_SANDBOX=0.
sandbox_ok() {
  local helper="$ELECTRON_DIR/dist/chrome-sandbox"
  if [ "$(stat -c '%u' "$helper" 2>/dev/null)" = "0" ] && [ -u "$helper" ]; then return 0; fi
  local restrict=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
  local clone=/proc/sys/kernel/unprivileged_userns_clone
  if [ -r "$clone" ] && [ "$(cat "$clone")" = "0" ]; then return 1; fi
  if [ -r "$restrict" ] && [ "$(cat "$restrict")" = "1" ]; then return 1; fi
  return 0
}
FLAGS=()
case "${MR_SANDBOX:-auto}" in
  0) FLAGS+=(--no-sandbox) ;;
  1) ;;
  *) sandbox_ok || FLAGS+=(--no-sandbox) ;;
esac

exec "$ELECTRON_DIR/dist/electron" . "${FLAGS[@]}" "$@"
