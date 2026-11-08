# Claude Session Widget

A small always-on-top widget that shows your running [Claude Code](https://claude.com/claude-code) sessions: which ones are working, which are idle, and which are waiting for you.

![The widget showing four sessions](docs/screenshot.png)

- One row per live session: name, project folder, state, time spent in that state.
- Sessions that need you come first. The tray icon takes the colour of the most urgent one.
- Tray icon and menu: show/hide, always on top, compact mode, opacity, start at login.
- Reads the session files Claude Code already writes. No hooks, no changes to your Claude Code setup, no network access.
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

## Configuration

Everything is set from the menu and stored in `settings.json` in the data folder:

| Platform | Data folder |
| --- | --- |
| Windows | `%APPDATA%\ClaudeSessionWidget` |
| Linux | `~/.config/ClaudeSessionWidget` |
| macOS | `~/Library/Application Support/ClaudeSessionWidget` |

Two environment variables override locations, mostly for tests: `CLAUDE_WIDGET_SESSIONS_DIR` (default `~/.claude/sessions`) and `CLAUDE_WIDGET_DATA_DIR`.

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

CI runs formatting, clippy and the tests on all three platforms, plus the shell installer test on Linux. Pushing a `v*` tag builds the binaries and publishes a release; the tag must match the version in `src-tauri/Cargo.toml`.

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
