#!/usr/bin/env bash
# Adds Midnight Racer to the desktop's application menu by writing
# ~/.local/share/applications/midnight-racer.desktop, which launches
# play.sh. Remove that file to undo.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
FILE="$DIR/midnight-racer.desktop"
mkdir -p "$DIR"
cat > "$FILE" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Midnight Racer
Comment=Street racing from Sierra Pass to downtown Meridian
Exec="$ROOT/play.sh"
Path=$ROOT
Icon=$ROOT/desktop/icon.svg
Terminal=false
Categories=Game;RacingGame;
StartupWMClass=midnight-racer
DESKTOP
chmod +x "$FILE"
command -v update-desktop-database >/dev/null && update-desktop-database "$DIR" >/dev/null 2>&1 || true
echo "Installed $FILE"
