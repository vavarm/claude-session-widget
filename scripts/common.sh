# Helpers shared by install.sh, update.sh and uninstall.sh. Sourced, not executed.

# CLAUDE_WIDGET_PLATFORM only exists so the scripts can be exercised from Git Bash on Windows.
PLATFORM="${CLAUDE_WIDGET_PLATFORM:-$(uname -s | tr '[:upper:]' '[:lower:]')}"
case "$PLATFORM" in
  linux)
    DEFAULT_INSTALL_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/ClaudeSessionWidget/app"
    DEFAULT_DATA_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/ClaudeSessionWidget"
    ;;
  darwin)
    DEFAULT_INSTALL_DIR="$HOME/Library/Application Support/ClaudeSessionWidget/app"
    DEFAULT_DATA_DIR="$HOME/Library/Application Support/ClaudeSessionWidget"
    ;;
  *)
    echo "Unsupported platform '$PLATFORM'. On Windows use scripts/install.ps1." >&2
    exit 1
    ;;
esac

BIN_NAME="ClaudeSessionWidget"
DATA_DIR="${CLAUDE_WIDGET_DATA_DIR:-$DEFAULT_DATA_DIR}"
APPLICATIONS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
DESKTOP_FILE="claude-session-widget.desktop"

# Only processes started from $1 are stopped, so another copy (for example a dev build) is left alone.
stop_widget() {
  local dir="$1"
  command -v pkill >/dev/null 2>&1 || return 0
  if pgrep -f "$dir/$BIN_NAME" >/dev/null 2>&1; then
    pkill -f "$dir/$BIN_NAME" || true
    for _ in 1 2 3 4 5 6 7 8 9 10; do
      pgrep -f "$dir/$BIN_NAME" >/dev/null 2>&1 || return 0
      sleep 0.3
    done
    pkill -9 -f "$dir/$BIN_NAME" || true
  fi
}
