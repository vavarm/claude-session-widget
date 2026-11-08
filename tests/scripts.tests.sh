#!/usr/bin/env bash
# Tests for scripts/install.sh, update.sh and uninstall.sh using a throwaway HOME.
# Usage: tests/scripts.tests.sh   (on Windows, run it from Git Bash: CLAUDE_WIDGET_PLATFORM=linux is set below)
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"
# cargo and rustup locate themselves through HOME, so pin them before HOME is faked.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$WORK/home"
export CLAUDE_WIDGET_PLATFORM="${CLAUDE_WIDGET_PLATFORM:-linux}"
export CLAUDE_WIDGET_DATA_DIR="$WORK/data"
export CLAUDE_CONFIG_DIR="$WORK/claude"
unset XDG_DATA_HOME XDG_CONFIG_HOME
mkdir -p "$HOME" "$CLAUDE_WIDGET_DATA_DIR"
APP="$WORK/inst/app"
failures=0

cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

check() {
  if [ "$2" = 0 ]; then echo "PASS  $1"; else echo "FAIL  $1"; failures=$((failures + 1)); fi
}

"$ROOT/scripts/install.sh" --install-dir "$APP" --no-launch >"$WORK/install.log" 2>&1
check "install succeeds" $?
[ -f "$APP/ClaudeSessionWidget" ]; check "install puts the binary in the install dir" $?
[ -x "$APP/ClaudeSessionWidget" ]; check "installed binary is executable" $?
grep -qF "Exec=\"$APP/ClaudeSessionWidget\"" "$HOME/.local/share/applications/claude-session-widget.desktop"
check "install writes the launcher entry" $?
[ ! -e "$APP.old" ]; check "install leaves no backup dir" $?

touch "$APP/stale.txt"; echo '{}' > "$CLAUDE_WIDGET_DATA_DIR/settings.json"
"$ROOT/scripts/install.sh" --install-dir "$APP" --no-launch >"$WORK/reinstall.log" 2>&1
check "re-install (update path) succeeds" $?
[ ! -e "$APP/stale.txt" ]; check "re-install replaces old files" $?
[ -f "$CLAUDE_WIDGET_DATA_DIR/settings.json" ]; check "re-install keeps settings" $?

"$ROOT/scripts/install.sh" --install-dir "$APP" --no-launch --quotas --agents >"$WORK/quotas.log" 2>&1
check "install --quotas succeeds" $?
grep -q 'ClaudeSessionWidget.*--statusline' "$CLAUDE_CONFIG_DIR/settings.json"; check "install --quotas registers the status line" $?
grep -q '"ShowQuotas": *true' "$CLAUDE_WIDGET_DATA_DIR/settings.json"; check "install --quotas turns the setting on" $?
grep -q 'subagentStatusLine' "$CLAUDE_CONFIG_DIR/settings.json"; check "install --agents registers the subagent status line" $?

mkdir -p "$WORK/not-an-install"; touch "$WORK/not-an-install/precious.txt"
"$ROOT/scripts/uninstall.sh" --install-dir "$WORK/not-an-install" >/dev/null 2>&1
[ $? -ne 0 ] && [ -f "$WORK/not-an-install/precious.txt" ]
check "uninstall refuses a directory that is not an install" $?

mkdir -p "$HOME/.config/autostart"
echo "Exec=\"$APP/ClaudeSessionWidget\"" > "$HOME/.config/autostart/widget.desktop"
echo "Exec=/usr/bin/other" > "$HOME/.config/autostart/other.desktop"
"$ROOT/scripts/uninstall.sh" --install-dir "$APP" >/dev/null 2>&1
check "uninstall succeeds" $?
[ ! -e "$APP" ]; check "uninstall removes the files" $?
! grep -qi statusLine "$CLAUDE_CONFIG_DIR/settings.json"; check "uninstall removes both status lines" $?
[ ! -e "$HOME/.local/share/applications/claude-session-widget.desktop" ]; check "uninstall removes the launcher entry" $?
[ ! -e "$HOME/.config/autostart/widget.desktop" ] && [ -e "$HOME/.config/autostart/other.desktop" ]
check "uninstall removes only its own autostart entry" $?
[ -f "$CLAUDE_WIDGET_DATA_DIR/settings.json" ]; check "uninstall keeps settings by default" $?

"$ROOT/scripts/install.sh" --install-dir "$APP" --no-launch >/dev/null 2>&1
"$ROOT/scripts/uninstall.sh" --install-dir "$APP" --purge >/dev/null 2>&1
[ ! -e "$CLAUDE_WIDGET_DATA_DIR" ]; check "uninstall --purge removes settings" $?

if [ "$failures" -gt 0 ]; then echo "$failures check(s) failed"; exit 1; fi
echo "All script checks passed"
