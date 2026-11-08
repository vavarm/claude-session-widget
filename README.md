# Claude Session Widget

A small always-on-top widget that shows your running [Claude Code](https://claude.com/claude-code) sessions: which ones are working, which are idle, and which are waiting for you.

![The widget showing four sessions](docs/screenshot.png)

- One row per live session: name, project folder, state, time spent in that state.
- Sessions that need you come first. The tray icon takes the colour of the most urgent one.
- The tray icon is the app icon with a dot in the colour of the most urgent state. Its menu covers show/hide, always on top, compact mode, opacity and start at login.
- Double-click a session to bring its terminal or editor window to the front (the project folder opens if no window is found).
- Placement: lock the position, exact position in pixels (nudge buttons), click-through.
- Optional context bar per session, with its model on hover.
- Optional agents and background tasks: the running ones are listed under their session.
- Optional notifications when a session needs you or finishes.
- Optional layout helpers: fold sessions idle for an hour, collapse to the header while nothing runs, and a global show/hide shortcut.
- Only reads the files Claude Code already writes under `~/.claude`: it changes nothing in your Claude Code setup and makes no network access.
- Windows, macOS and Linux (see [platform status](#platform-status)).

Unofficial project, not affiliated with Anthropic.

| Dot | State |
| --- | --- |
| Red | Needs you (waiting for input or a permission) |
| Orange | Busy |
| Green | Idle |
| Grey | Unknown status |

## Install

### From a release

Download the archive for your platform from the Releases page, extract it and run `ClaudeSessionWidget`. Enable **Start at login** in the tray menu if you want it to launch with your session.

The binaries are not code-signed:

- Windows: SmartScreen may warn on first launch; choose "More info", then "Run anyway".
- macOS: remove the quarantine flag once with `xattr -d com.apple.quarantine ClaudeSessionWidget`.

Release archives come with `SHA256SUMS.txt`.

### From source

Requires [Rust](https://rustup.rs) (stable) and the platform dependencies of [Tauri](https://tauri.app/start/prerequisites/): the Visual Studio C++ Build Tools on Windows (WebView2 ships with Windows 11), WebKitGTK and libayatana-appindicator on Linux, the Xcode command line tools on macOS.

```sh
git clone <repository-url>
cd claude-session-widget
scripts/install.sh                                                          # Linux, macOS
powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install.ps1     # Windows
```

The first build compiles all dependencies and takes several minutes. The new build is staged first, so a failed build leaves the previous installation untouched.

| | Windows | Linux / macOS |
| --- | --- | --- |
| Installed to | `%LOCALAPPDATA%\ClaudeSessionWidget\app` | `~/.local/share/ClaudeSessionWidget/app` (Linux), `~/Library/Application Support/ClaudeSessionWidget/app` (macOS) |
| Launcher | Start Menu shortcut | Desktop entry (Linux only) |

Options: `-InstallDir`, `-StartMenuDir`, `-Autostart`, `-NoLaunch` on Windows; `--install-dir`, `--no-launch` on Linux and macOS (use the tray menu for start at login there).

**Update** with `scripts/update.sh` or `scripts\update.ps1`: it runs `git pull --ff-only`, then reinstalls. Settings and the start-at-login choice are kept.

**Uninstall** with `scripts/uninstall.sh` or `scripts\uninstall.ps1`. It stops the widget and removes the installed files, launcher and autostart entry. Settings are kept unless you pass `--purge` (`-Purge` on Windows). Nothing is deleted if the install folder does not hold an installation.

## Usage

- Drag the widget to move it; the position is remembered.
- Left-click the tray icon to show or hide it.
- Right-click the widget or the tray icon for the menu.

Only one instance runs at a time.

Double-clicking a session looks for the window of the terminal or editor that started it, and prefers the one whose title mentions the project. On Linux and macOS, and when no window is found, it opens the project folder instead.

Layout options in the tray menu:

- **Fold sessions idle for an hour** replaces them with a single "+N idle" line.
- **Collapse when nothing is running** shrinks the widget to its header while no session is busy or waiting; hovering it expands it again.
- **Show/hide shortcut** registers a global shortcut (Ctrl+Alt+C, Ctrl+Alt+W or Ctrl+Alt+Space; Cmd on macOS). If another application already owns it, the widget says so.

Placement options in the tray menu:

- **Lock position** disables dragging.
- **Click-through** lets mouse clicks pass through the widget to the window below. The widget then cannot be right-clicked: use the tray icon to change settings or turn it off again.
- **Position** shows the current coordinates and moves the widget by 1 or 10 screen pixels in any direction. The position is saved after every move; **Reset position** returns to the top-right corner.

## Optional features

Both are off by default and switched on from the tray menu.

- **Show session context** draws a thin context bar at the bottom of each session (orange from 80 %, red from 95 %); hovering a session shows its model and context.
- **Show agents** lists the subagents a session has started and that have not reported back yet (up to three lines, the rest folded into "+N more"). In compact mode a small counter replaces the lines.

Both read the transcripts Claude Code keeps in `~/.claude/projects` (`$CLAUDE_CONFIG_DIR/projects`), which the terminal, the VS Code extension and the desktop app all write. Notes:

- The context is the size of the last request of the conversation. The transcript does not say how large the model's window is, so it is measured against 200k tokens, or against 1M once the conversation has gone past 200k. Treat it as an estimate.
- A subagent counts as running until its record says it finished; one that has written nothing for 15 minutes is taken for a crashed one and dropped.
- Background shell commands are not shown: Claude Code does not record them with the subagents.

**Notifications** (tray menu only) can be switched on separately for a session that needs you and for a session that finishes (busy to idle). They are system notifications and never replay what was already true when you turn them on. Whether "needs you" ever fires depends on the status values Claude Code writes, of which only `busy` and `idle` have been observed.

## Configuration

Everything is set from the menu and stored in `settings.json` in the data folder:

| Platform | Data folder |
| --- | --- |
| Windows | `%APPDATA%\ClaudeSessionWidget` |
| Linux | `~/.config/ClaudeSessionWidget` |
| macOS | `~/Library/Application Support/ClaudeSessionWidget` |

Environment variables override locations, mostly for tests: `CLAUDE_WIDGET_SESSIONS_DIR` (default `~/.claude/sessions`), `CLAUDE_WIDGET_DATA_DIR`, and `CLAUDE_CONFIG_DIR` (the Claude Code configuration folder, default `~/.claude`).

## Log

The widget records what it does in `widget.log` in the data folder (tray menu: *Open log folder*), one JSON object per line with a UTC timestamp (`t`), the writing process (`pid`) and an `event` name. It covers startup and shutdown with the settings in force, session starts, ends and state changes (with the raw status), context changes, running agents, notifications, menu actions, shortcut registration and presses, window visibility, position and height, panics, and a heartbeat every five minutes. Session names and folders are recorded; tokens and prompts never are. The file is rotated at 4 MB, keeping one previous `widget.log.1`.

## How it works

Claude Code writes one JSON file per live session (pid, name, working directory, status, timestamps) to `~/.claude/sessions`. The Rust backend reads that folder every second and sends the result to the page.

A session file can outlive a crashed process, so a session is only listed if its pid is alive. On Windows its process start time must also match the file, which rules out recycled pids.

Status mapping: `busy` and `idle` are known values; a status containing `wait`, `input`, `permission` or `approval` means the session needs you; anything else is shown as unknown. Only `busy` and `idle` have been observed so far, so the "needs you" detection is a best guess.

## Development

```sh
cargo test --manifest-path src-tauri/Cargo.toml      # backend
node --test tests/format.test.mjs                    # UI formatting
tests/scripts.tests.sh                               # Linux/macOS installer scripts
```

Windows-only tests, after `cargo build --release --manifest-path src-tauri/Cargo.toml`:

```powershell
powershell -NoProfile -File tests\e2e.ps1            # drives the real exe
powershell -NoProfile -File tests\scripts.tests.ps1  # installer scripts (rebuilds, takes a few minutes)
```

The end-to-end test needs no mouse and no focus: it runs the executable against a fake sessions folder with its own settings and WebView2 profile, reads the page through the WebView2 DevTools protocol, and runs menu entries through the app's `menu_action` command.

```
src/         UI (HTML, CSS, JS, no build step)
src-tauri/   Rust backend: sessions, settings, tray, menu, window
tests/       UI tests, Windows end-to-end test, installer script tests
scripts/     install, update, uninstall (PowerShell and shell)
```

CI runs formatting, clippy and the tests on all three platforms, plus the shell installer test on Linux.

**Releasing**: run the *Prepare release* workflow from the Actions tab on `main` and pick `patch`, `minor` or `major`. It requires a green CI on the commit, raises the version in `src-tauri/Cargo.toml` and `Cargo.lock`, commits `chore(release): X.Y.Z`, tags it, then builds the four binaries and publishes the release with generated notes and `SHA256SUMS.txt`. Pushing a `v*` tag by hand also publishes a release; the tag must then match the version in `src-tauri/Cargo.toml`.

## License

[MIT](LICENSE)

## Platform status

| Platform | Status |
| --- | --- |
| Windows 10/11 | Developed and tested, including the end-to-end and installer tests |
| Linux | Built and unit-tested in CI; never run by hand |
| macOS | Built and unit-tested in CI; never run by hand |

Known limits on Linux and macOS:

- Wayland compositors may ignore "always on top" for normal windows; X11 works.
- GNOME needs an AppIndicator extension to show tray icons.
- On macOS the widget is a bare binary, not an app bundle, and lives in the menu bar.
