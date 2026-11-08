#!/usr/bin/env bash
# Removes Claude Session Widget for the current user (Linux and macOS).
#
# Stops the widget, removes the autostart entry, the launcher entry and the installed files.
# Settings are kept unless --purge is given. Nothing is deleted if the install dir does not
# look like an installation.
#
# Usage: scripts/uninstall.sh [--install-dir DIR] [--purge]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=common.sh
. "$ROOT/scripts/common.sh"

INSTALL_DIR="$DEFAULT_INSTALL_DIR"
PURGE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    --purge) PURGE=1; shift ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

if [ ! -f "$INSTALL_DIR/$BIN_NAME" ]; then
  echo "No installation found in $INSTALL_DIR ($BIN_NAME is missing); nothing was removed." >&2
  exit 1
fi

stop_widget "$INSTALL_DIR"

# Autostart entries are written by the app itself, so match them by the path they launch.
for entry in "${XDG_CONFIG_HOME:-$HOME/.config}"/autostart/*.desktop "$HOME"/Library/LaunchAgents/*.plist; do
  [ -f "$entry" ] && grep -qF "$INSTALL_DIR/$BIN_NAME" "$entry" && rm -f "$entry"
done
rm -f "$APPLICATIONS_DIR/$DESKTOP_FILE"

rm -rf "$INSTALL_DIR"
rmdir "$(dirname "$INSTALL_DIR")" 2>/dev/null || true

if [ "$PURGE" = 1 ]; then
  rm -rf "$DATA_DIR"
  echo "Settings removed."
else
  echo "Settings kept (use --purge to remove them)."
fi

echo "Claude Session Widget uninstalled."
