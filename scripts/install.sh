#!/usr/bin/env bash
# Builds Claude Session Widget from source and installs it for the current user (Linux and macOS).
#
# Safe to run repeatedly: it is also the update step. The new build is staged in a temporary
# directory first, and the previous installation is only replaced once the build succeeded
# (and restored if the copy fails). Settings are never touched.
#
# Usage: scripts/install.sh [--install-dir DIR] [--no-launch]
#
# Autostart is not handled here: use "Start at login" in the widget's tray menu.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=common.sh
. "$ROOT/scripts/common.sh"

INSTALL_DIR="$DEFAULT_INSTALL_DIR"
LAUNCH=1
while [ $# -gt 0 ]; do
  case "$1" in
    --install-dir) INSTALL_DIR="$2"; shift 2 ;;
    --no-launch) LAUNCH=0; shift ;;
    -h|--help) sed -n '2,10p' "$0"; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
done

command -v cargo >/dev/null 2>&1 || { echo "The Rust toolchain was not found. Install it from https://rustup.rs and retry." >&2; exit 1; }

STAGE="$(mktemp -d)"
BACKUP="$INSTALL_DIR.old"
trap 'rm -rf "$STAGE"' EXIT

echo "Building (the first build downloads and compiles dependencies and can take several minutes)..."
cargo build --release --manifest-path "$ROOT/src-tauri/Cargo.toml"

TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target}"
BUILT="$TARGET_DIR/release/$BIN_NAME"
[ -f "$BUILT" ] || BUILT="$BUILT.exe"
[ -f "$BUILT" ] || { echo "Build output does not contain $BIN_NAME." >&2; exit 1; }
cp "$BUILT" "$STAGE/$BIN_NAME"
chmod +x "$STAGE/$BIN_NAME"

stop_widget "$INSTALL_DIR"

rm -rf "$BACKUP"
[ -d "$INSTALL_DIR" ] && mv "$INSTALL_DIR" "$BACKUP"
if ! { mkdir -p "$(dirname "$INSTALL_DIR")" && mkdir "$INSTALL_DIR" && cp "$STAGE/$BIN_NAME" "$INSTALL_DIR/"; }; then
  rm -rf "$INSTALL_DIR"
  [ -d "$BACKUP" ] && mv "$BACKUP" "$INSTALL_DIR"
  echo "Install failed; the previous installation was restored." >&2
  exit 1
fi
rm -rf "$BACKUP"

if [ "$PLATFORM" = "linux" ]; then
  mkdir -p "$APPLICATIONS_DIR"
  cat > "$APPLICATIONS_DIR/$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=Claude Session Widget
Comment=Always-on-top widget that tracks your Claude Code sessions
Exec="$INSTALL_DIR/$BIN_NAME"
Terminal=false
Categories=Utility;
EOF
fi

echo "Installed Claude Session Widget to $INSTALL_DIR"

if [ "$LAUNCH" = 1 ]; then
  nohup "$INSTALL_DIR/$BIN_NAME" >/dev/null 2>&1 &
fi
