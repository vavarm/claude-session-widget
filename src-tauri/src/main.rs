#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod focus;
mod hook;
mod info;
mod notify;
mod quota;
mod sessions;
mod settings;
mod tray;

use hook::Hook;
use quota::Quotas;
use serde::Serialize;
use sessions::{Kind, Session};
use settings::Settings;
use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{
        CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
        SubmenuBuilder,
    },
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const WIDTH: f64 = 280.0;
const TRAY_ID: &str = "main";
const OPACITIES: [u32; 5] = [100, 90, 80, 60, 40];
const NUDGE_STEPS: [i32; 2] = [1, 10];
const NOTICE_TTL: Duration = Duration::from_secs(8);
const SHORTCUTS: [&str; 3] = [
    "CommandOrControl+Alt+C",
    "CommandOrControl+Alt+W",
    "CommandOrControl+Alt+Space",
];
const KEEP_PER_SESSION_FILES: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Default)]
struct AppState {
    settings: Mutex<Settings>,
    sessions: Mutex<Vec<Session>>,
    notice: Mutex<Option<(String, Instant)>>,
    position_dirty: AtomicBool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Payload {
    sessions: Vec<Session>,
    compact: bool,
    opacity: f64,
    locked: bool,
    fold_idle: bool,
    auto_collapse: bool,
    quotas: Option<Quotas>,
    notice: Option<String>,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn payload(app: &AppHandle) -> Payload {
    let state = app.state::<AppState>();
    let sessions = state.sessions.lock().unwrap().clone();
    let notice = state
        .notice
        .lock()
        .unwrap()
        .as_ref()
        .filter(|(_, at)| at.elapsed() < NOTICE_TTL)
        .map(|(message, _)| message.clone());
    let settings = state.settings.lock().unwrap();
    let quotas = settings
        .show_quotas
        .then(|| quota::load(now_secs()))
        .filter(|q| !q.is_empty());
    Payload {
        sessions,
        compact: settings.compact,
        opacity: settings.opacity,
        locked: settings.locked,
        fold_idle: settings.fold_idle,
        auto_collapse: settings.auto_collapse,
        quotas,
        notice,
    }
}

fn set_notice(app: &AppHandle, message: String) {
    *app.state::<AppState>().notice.lock().unwrap() = Some((message, Instant::now()));
}

fn push_state(app: &AppHandle) {
    let _ = app.emit("state", payload(app));
}

#[tauri::command]
fn get_state(app: AppHandle) -> Payload {
    payload(&app)
}

#[tauri::command]
fn fit(window: WebviewWindow, height: f64) {
    let _ = window.set_size(LogicalSize::new(WIDTH, height));
}

#[tauri::command]
fn show_menu(app: AppHandle, window: WebviewWindow) {
    if let Ok(menu) = build_menu(&app) {
        let _ = window.popup_menu(&menu);
    }
}

// Runs a menu entry by id without going through the native popup, so tests can drive the menu.
#[tauri::command]
fn menu_action(app: AppHandle, id: String) {
    handle_menu(&app, &id);
}

#[tauri::command]
fn open_session(app: AppHandle, pid: u32) {
    let session = app
        .state::<AppState>()
        .sessions
        .lock()
        .unwrap()
        .iter()
        .find(|s| s.pid == pid)
        .cloned();
    if let Some(session) = session {
        let folder = std::path::Path::new(&session.cwd)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !focus::focus(pid, &folder) {
            open_folder(std::path::Path::new(&session.cwd));
        }
    }
}

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

fn set_visible(app: &AppHandle, visible: bool) {
    if let Some(window) = main_window(app) {
        let _ = if visible {
            window.show()
        } else {
            window.hide()
        };
    }
    let state = app.state::<AppState>();
    let mut settings = state.settings.lock().unwrap();
    settings.widget_visible = visible;
    settings.save();
}

fn is_visible(app: &AppHandle) -> bool {
    main_window(app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let settings = app.state::<AppState>().settings.lock().unwrap().clone();
    let autostart = app.autolaunch().is_enabled().unwrap_or(false);

    let show = MenuItemBuilder::with_id(
        "show",
        if is_visible(app) {
            "Hide widget"
        } else {
            "Show widget"
        },
    )
    .build(app)?;
    let top = CheckMenuItemBuilder::with_id("top", "Always on top")
        .checked(settings.always_on_top)
        .build(app)?;
    let compact = CheckMenuItemBuilder::with_id("compact", "Compact display")
        .checked(settings.compact)
        .build(app)?;
    let mut opacity = SubmenuBuilder::new(app, "Opacity");
    for pct in OPACITIES {
        let checked = (settings.opacity * 100.0).round() as u32 == pct;
        opacity = opacity.item(
            &CheckMenuItemBuilder::with_id(format!("opacity_{pct}"), format!("{pct} %"))
                .checked(checked)
                .build(app)?,
        );
    }
    let opacity = opacity.build()?;
    let check = |id: &str, label: &str, checked: bool| {
        CheckMenuItemBuilder::with_id(id, label)
            .checked(checked)
            .build(app)
    };
    let quotas = check(
        "quotas",
        "Show usage quotas and context",
        settings.show_quotas,
    )?;
    let agents = check(
        "agents",
        "Show agents and background tasks",
        settings.show_agents,
    )?;
    let fold_idle = check(
        "fold_idle",
        "Fold sessions idle for an hour",
        settings.fold_idle,
    )?;
    let auto_collapse = check(
        "auto_collapse",
        "Collapse when nothing is running",
        settings.auto_collapse,
    )?;
    let notifications = SubmenuBuilder::new(app, "Notifications")
        .item(&check(
            "notify_attention",
            "A session needs you",
            settings.notify_attention,
        )?)
        .item(&check(
            "notify_finished",
            "A session finishes",
            settings.notify_finished,
        )?)
        .item(&check(
            "notify_quota",
            "A quota is nearly used up",
            settings.notify_quota,
        )?)
        .build()?;
    let mut shortcut = SubmenuBuilder::new(app, "Show/hide shortcut").item(&check(
        "shortcut_off",
        "None",
        settings.shortcut.is_empty(),
    )?);
    for spec in SHORTCUTS {
        shortcut = shortcut.item(&check(
            &format!("shortcut_{spec}"),
            &spec.replace("CommandOrControl", "Ctrl"),
            settings.shortcut == spec,
        )?);
    }
    let shortcut = shortcut.build()?;
    let locked = CheckMenuItemBuilder::with_id("locked", "Lock position")
        .checked(settings.locked)
        .build(app)?;
    let click_through = CheckMenuItemBuilder::with_id("click_through", "Click-through")
        .checked(settings.click_through)
        .build(app)?;
    let position = build_position_menu(app)?;
    let startup = CheckMenuItemBuilder::with_id("startup", "Start at login")
        .checked(autostart)
        .build(app)?;
    let folder = MenuItemBuilder::with_id("folder", "Open sessions folder").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    MenuBuilder::new(app)
        .items(&[
            &show,
            &top,
            &compact,
            &opacity,
            &quotas,
            &agents,
            &fold_idle,
            &auto_collapse,
            &notifications,
            &PredefinedMenuItem::separator(app)?,
            &locked,
            &click_through,
            &position,
            &shortcut,
            &PredefinedMenuItem::separator(app)?,
            &startup,
            &PredefinedMenuItem::separator(app)?,
            &folder,
            &quit,
        ])
        .build()
}

fn build_position_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Submenu<Wry>> {
    let current = main_window(app)
        .and_then(|w| w.outer_position().ok())
        .map_or_else(
            || "Position: unknown".to_string(),
            |p| format!("Position: {}, {}", p.x, p.y),
        );
    let mut menu = SubmenuBuilder::new(app, "Position").item(
        &MenuItemBuilder::with_id("position_label", current)
            .enabled(false)
            .build(app)?,
    );
    menu = menu.separator();
    for (name, dx, dy) in [
        ("Left", -1, 0),
        ("Right", 1, 0),
        ("Up", 0, -1),
        ("Down", 0, 1),
    ] {
        for step in NUDGE_STEPS {
            menu = menu.item(
                &MenuItemBuilder::with_id(
                    format!("nudge_{}_{}", dx * step, dy * step),
                    format!("{name} {step} px"),
                )
                .build(app)?,
            );
        }
    }
    menu.separator()
        .item(&MenuItemBuilder::with_id("reset_position", "Reset position").build(app)?)
        .build()
}

fn nudge(app: &AppHandle, dx: i32, dy: i32) {
    if let Some(window) = main_window(app) {
        if let Ok(p) = window.outer_position() {
            let _ = window.set_position(PhysicalPosition::new(p.x + dx, p.y + dy));
        }
    }
}

fn refresh_menu(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn handle_menu(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    match id {
        "show" => set_visible(app, !is_visible(app)),
        "top" => {
            // The window call runs on the main thread, which may be waiting for this lock in its Moved handler.
            let on_top = {
                let mut settings = state.settings.lock().unwrap();
                settings.always_on_top = !settings.always_on_top;
                settings.save();
                settings.always_on_top
            };
            if let Some(window) = main_window(app) {
                let _ = window.set_always_on_top(on_top);
            }
        }
        "compact" => {
            let mut settings = state.settings.lock().unwrap();
            settings.compact = !settings.compact;
            settings.save();
        }
        "locked" => {
            let mut settings = state.settings.lock().unwrap();
            settings.locked = !settings.locked;
            settings.save();
        }
        "click_through" => {
            let enabled = {
                let mut settings = state.settings.lock().unwrap();
                settings.click_through = !settings.click_through;
                settings.save();
                settings.click_through
            };
            if let Some(window) = main_window(app) {
                let _ = window.set_ignore_cursor_events(enabled);
            }
        }
        "quotas" => toggle_feature(app, Feature::Quotas),
        "agents" => toggle_feature(app, Feature::Agents),
        "fold_idle" => toggle(&state, |s| &mut s.fold_idle),
        "auto_collapse" => toggle(&state, |s| &mut s.auto_collapse),
        "notify_attention" => toggle(&state, |s| &mut s.notify_attention),
        "notify_finished" => toggle(&state, |s| &mut s.notify_finished),
        "notify_quota" => toggle(&state, |s| &mut s.notify_quota),
        "reset_position" => {
            let settings = {
                let mut settings = state.settings.lock().unwrap();
                settings.x = None;
                settings.y = None;
                settings.save();
                settings.clone()
            };
            if let Some(window) = main_window(app) {
                place_window(&window, &settings);
            }
        }
        "startup" => {
            let autolaunch = app.autolaunch();
            let _ = if autolaunch.is_enabled().unwrap_or(false) {
                autolaunch.disable()
            } else {
                autolaunch.enable()
            };
        }
        "folder" => open_folder(&sessions::dir()),
        "quit" => app.exit(0),
        _ => {
            if let Some(pct) = id
                .strip_prefix("opacity_")
                .and_then(|p| p.parse::<u32>().ok())
            {
                let mut settings = state.settings.lock().unwrap();
                settings.opacity = f64::from(pct) / 100.0;
                settings.save();
            } else if let Some((dx, dy)) = id.strip_prefix("nudge_").and_then(|d| d.split_once('_'))
            {
                if let (Ok(dx), Ok(dy)) = (dx.parse(), dy.parse()) {
                    nudge(app, dx, dy);
                }
            } else if let Some(spec) = id.strip_prefix("shortcut_") {
                choose_shortcut(app, if spec == "off" { "" } else { spec });
            }
        }
    }
    push_state(app);
    refresh_menu(app);
}

fn toggle(state: &AppState, field: impl FnOnce(&mut Settings) -> &mut bool) {
    let mut settings = state.settings.lock().unwrap();
    let value = field(&mut settings);
    *value = !*value;
    settings.save();
}

fn apply_shortcut(app: &AppHandle, spec: &str) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    if spec.is_empty() {
        return Ok(());
    }
    shortcuts
        .on_shortcut(spec, |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                set_visible(app, !is_visible(app));
                refresh_menu(app);
            }
        })
        .map_err(|e| format!("The shortcut {spec} is unavailable: {e}"))
}

fn choose_shortcut(app: &AppHandle, spec: &str) {
    match apply_shortcut(app, spec) {
        Ok(()) => {
            let state = app.state::<AppState>();
            let mut settings = state.settings.lock().unwrap();
            settings.shortcut = spec.to_string();
            settings.save();
        }
        Err(message) => {
            let previous = app
                .state::<AppState>()
                .settings
                .lock()
                .unwrap()
                .shortcut
                .clone();
            let _ = apply_shortcut(app, &previous);
            set_notice(app, message);
        }
    }
}

// Options that need Claude Code to feed the widget through a settings.json command.
#[derive(Clone, Copy)]
enum Feature {
    Quotas,
    Agents,
}

impl Feature {
    fn hook(self) -> Hook {
        match self {
            Feature::Quotas => hook::STATUS_LINE,
            Feature::Agents => hook::SUBAGENT_STATUS_LINE,
        }
    }

    fn enabled(self, settings: &Settings) -> bool {
        match self {
            Feature::Quotas => settings.show_quotas,
            Feature::Agents => settings.show_agents,
        }
    }

    fn set(self, settings: &mut Settings, on: bool) {
        match self {
            Feature::Quotas => settings.show_quotas = on,
            Feature::Agents => settings.show_agents = on,
        }
    }

    fn register(self) -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        self.hook().enable(&hook::claude_settings_file(), &exe)
    }
}

fn toggle_feature(app: &AppHandle, feature: Feature) {
    let state = app.state::<AppState>();
    let enable = !feature.enabled(&state.settings.lock().unwrap());
    let result = if enable {
        feature.register()
    } else {
        feature.hook().disable(&hook::claude_settings_file())
    };
    match result {
        Ok(()) => {
            let mut settings = state.settings.lock().unwrap();
            feature.set(&mut settings, enable);
            settings.save();
        }
        Err(message) => set_notice(app, message),
    }
}

fn report(result: Result<(), String>) -> i32 {
    match result {
        Ok(()) => 0,
        Err(message) => {
            let _ = writeln!(std::io::stdout(), "{message}");
            2
        }
    }
}

fn read_stdin() -> String {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    input
}

// Command-line modes used by Claude Code (as a status line command) and by the installers.
fn run_cli() -> Option<i32> {
    let enable = |feature: Feature| {
        report(feature.register().inspect(|()| {
            let mut settings = Settings::load();
            feature.set(&mut settings, true);
            settings.save();
        }))
    };
    match std::env::args().nth(1)?.as_str() {
        "--statusline" => {
            let input = read_stdin();
            if let Some(quotas) = quota::parse_status(&input) {
                quota::save(&quotas);
            }
            if let Some((session_id, info)) = info::parse_status(&input) {
                info::save(&session_id, &info);
            }
            Some(0)
        }
        "--subagent-status" => {
            if let Some((session_id, tasks)) = agents::parse_input(&read_stdin()) {
                agents::save(&session_id, tasks, now_secs() * 1000);
            }
            Some(0)
        }
        "--enable-quotas" => Some(enable(Feature::Quotas)),
        "--enable-agents" => Some(enable(Feature::Agents)),
        "--remove-hooks" => Some(report(
            hook::ALL
                .iter()
                .try_for_each(|h| h.disable(&hook::claude_settings_file())),
        )),
        _ => None,
    }
}

fn open_folder(path: &std::path::Path) {
    let opener = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener).arg(path).spawn();
}

fn update_tray(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let sessions = app.state::<AppState>().sessions.lock().unwrap().clone();
    let count = |kind| sessions.iter().filter(|s| s.kind == kind).count();

    let mut tooltip = format!(
        "Claude: {} session(s), {} busy",
        sessions.len(),
        count(Kind::Busy)
    );
    if count(Kind::Attention) > 0 {
        tooltip += &format!(", {} need you", count(Kind::Attention));
    }
    let _ = tray.set_icon(Some(tray::icon(&tray::base_icon(), tray::color(&sessions))));
    let _ = tray.set_tooltip(Some(tooltip));
}

fn place_window(window: &WebviewWindow, settings: &Settings) {
    if let (Some(x), Some(y)) = (settings.x, settings.y) {
        let on_screen = window
            .available_monitors()
            .unwrap_or_default()
            .iter()
            .any(|m| {
                let (p, s) = (m.position(), m.size());
                (p.x..p.x + s.width as i32).contains(&(x + 20))
                    && (p.y..p.y + s.height as i32).contains(&(y + 20))
            });
        if on_screen {
            let _ = window.set_position(PhysicalPosition::new(x, y));
            return;
        }
    }
    if let Ok(Some(monitor)) = window.primary_monitor() {
        let width = window.outer_size().map(|s| s.width as i32).unwrap_or(280);
        let (p, s) = (monitor.position(), monitor.size());
        let _ = window.set_position(PhysicalPosition::new(
            p.x + s.width as i32 - width - 16,
            p.y + 16,
        ));
    }
}

fn main() {
    if let Some(code) = run_cli() {
        std::process::exit(code);
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            set_visible(app, true)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            settings: Mutex::new(Settings::load()),
            ..Default::default()
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            fit,
            show_menu,
            menu_action,
            open_session
        ])
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = handle.state::<AppState>().settings.lock().unwrap().clone();
            let window = main_window(&handle).expect("main window is declared in tauri.conf.json");

            window.set_always_on_top(settings.always_on_top)?;
            window.set_ignore_cursor_events(settings.click_through)?;
            place_window(&window, &settings);
            for feature in [Feature::Quotas, Feature::Agents] {
                if !feature.enabled(&settings) {
                    continue;
                }
                if let Err(message) = feature.register() {
                    set_notice(&handle, message);
                }
            }
            if let Err(message) = apply_shortcut(&handle, &settings.shortcut) {
                set_notice(&handle, message);
            }
            settings::prune_older_than(
                &settings::data_dir().join("agents"),
                KEEP_PER_SESSION_FILES,
            );
            settings::prune_older_than(&settings::data_dir().join("info"), KEEP_PER_SESSION_FILES);
            if settings.widget_visible {
                window.show()?;
            }

            let on_window_event = handle.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    set_visible(&on_window_event, false);
                }
                tauri::WindowEvent::Moved(position) => {
                    let state = on_window_event.state::<AppState>();
                    let mut settings = state.settings.lock().unwrap();
                    settings.x = Some(position.x);
                    settings.y = Some(position.y);
                    state.position_dirty.store(true, Ordering::SeqCst);
                }
                _ => {}
            });

            TrayIconBuilder::with_id(TRAY_ID)
                .icon(tray::icon(&tray::base_icon(), tray::color(&[])))
                .menu(&build_menu(&handle)?)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        set_visible(app, !is_visible(app));
                        refresh_menu(app);
                    }
                })
                .build(app)?;

            thread::spawn(move || {
                let mut last_snapshot = String::new();
                let mut watcher = notify::Watcher::default();
                loop {
                    let mut list = sessions::read(&sessions::dir());
                    let options = handle.state::<AppState>().settings.lock().unwrap().clone();
                    for session in &mut list {
                        if options.show_agents {
                            session.agents = agents::load(&session.session_id, now_secs() * 1000);
                        }
                        if options.show_quotas {
                            session.info = info::load(&session.session_id);
                        }
                    }
                    for (reason, name) in watcher.sessions(&list) {
                        match reason {
                            notify::Reason::NeedsYou if options.notify_attention => {
                                notify::show(&handle, &name, "Needs you");
                            }
                            notify::Reason::Finished if options.notify_finished => {
                                notify::show(&handle, &name, "Finished");
                            }
                            _ => {}
                        }
                    }
                    if options.notify_quota {
                        let now = now_secs();
                        for message in watcher.quotas(&quota::load(now), now) {
                            notify::show(&handle, "Claude usage", &message);
                        }
                    }
                    let snapshot = list
                        .iter()
                        .map(|s| format!("{}:{:?}", s.pid, s.kind))
                        .collect::<Vec<_>>()
                        .join("|");
                    let state = handle.state::<AppState>();
                    *state.sessions.lock().unwrap() = list;
                    push_state(&handle);

                    if snapshot != last_snapshot {
                        last_snapshot = snapshot;
                        update_tray(&handle);
                    }
                    if state.position_dirty.swap(false, Ordering::SeqCst) {
                        state.settings.lock().unwrap().save();
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run the widget");
}
