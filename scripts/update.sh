#!/usr/bin/env bash
# Pulls the latest source and reinstalls Claude Session Widget (Linux and macOS).
# Fast-forwards the current branch, then runs install.sh with the same options.
#
# Usage: scripts/update.sh [--install-dir DIR] [--no-launch]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

command -v git >/dev/null 2>&1 || { echo "git was not found." >&2; exit 1; }
[ -e "$ROOT/.git" ] || { echo "$ROOT is not a git checkout; update it manually, then run install.sh." >&2; exit 1; }

git -C "$ROOT" pull --ff-only || { echo "git pull failed. Resolve it (local changes? diverged branch?) and retry." >&2; exit 1; }

exec "$ROOT/scripts/install.sh" "$@"
