#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod focus;
mod log;
mod notify;
mod sessions;
mod settings;
mod transcript;
mod tray;

use serde::Serialize;
use serde_json::json;
use sessions::{Kind, Session};
use settings::Settings;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
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
const HEARTBEAT: Duration = Duration::from_secs(300);

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
    notice: Option<String>,
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
    Payload {
        sessions,
        compact: settings.compact,
        opacity: settings.opacity,
        locked: settings.locked,
        fold_idle: settings.fold_idle,
        auto_collapse: settings.auto_collapse,
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
    log::event("fit", json!({ "height": height }));
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
        let focused = focus::focus(pid, &folder);
        log::event(
            "open_session",
            json!({ "pid": pid, "folder": folder, "focused": focused }),
        );
        if !focused {
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
    log::event("visibility", json!({ "visible": visible }));
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
    let context = check("context", "Show session context", settings.show_context)?;
    let agents = check("agents", "Show agents", settings.show_agents)?;
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
    let log_folder = MenuItemBuilder::with_id("log_folder", "Open log folder").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    MenuBuilder::new(app)
        .items(&[
            &show,
            &top,
            &compact,
            &opacity,
            &context,
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
            &log_folder,
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
    log::event("menu", json!({ "id": id }));
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
        "context" => toggle(&state, |s| &mut s.show_context),
        "agents" => toggle(&state, |s| &mut s.show_agents),
        "fold_idle" => toggle(&state, |s| &mut s.fold_idle),
        "auto_collapse" => toggle(&state, |s| &mut s.auto_collapse),
        "notify_attention" => toggle(&state, |s| &mut s.notify_attention),
        "notify_finished" => toggle(&state, |s| &mut s.notify_finished),
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
        "log_folder" => open_folder(&settings::data_dir()),
        "quit" => {
            log::event("shutdown", json!({ "reason": "menu" }));
            app.exit(0);
        }
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
    log::event("settings", settings_json(&state));
    push_state(app);
    refresh_menu(app);
}

fn settings_json(state: &AppState) -> serde_json::Value {
    serde_json::to_value(&*state.settings.lock().unwrap()).unwrap_or_default()
}

fn toggle(state: &AppState, field: impl FnOnce(&mut Settings) -> &mut bool) {
    let mut settings = state.settings.lock().unwrap();
    let value = field(&mut settings);
    *value = !*value;
    settings.save();
}

fn apply_shortcut(app: &AppHandle, spec: &str) -> Result<(), String> {
    let result = register_shortcut(app, spec);
    log::event(
        "shortcut",
        json!({ "spec": spec, "error": result.as_ref().err() }),
    );
    result
}

fn register_shortcut(app: &AppHandle, spec: &str) -> Result<(), String> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all().map_err(|e| e.to_string())?;
    if spec.is_empty() {
        return Ok(());
    }
    shortcuts
        .on_shortcut(spec, |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                log::event("shortcut_pressed", json!({}));
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
    std::panic::set_hook(Box::new(|info| {
        log::event("panic", json!({ "message": info.to_string() }));
    }));
    log::event(
        "launch",
        json!({ "version": env!("CARGO_PKG_VERSION"), "args": std::env::args().skip(1).collect::<Vec<_>>() }),
    );
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
            log::event(
                "start",
                json!({ "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS,
                        "exe": std::env::current_exe().ok(),
                        "settings": settings_json(&handle.state::<AppState>()) }),
            );

            window.set_always_on_top(settings.always_on_top)?;
            window.set_ignore_cursor_events(settings.click_through)?;
            place_window(&window, &settings);
            if let Err(message) = apply_shortcut(&handle, &settings.shortcut) {
                set_notice(&handle, message);
            }
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
                let mut tracker = log::Tracker::default();
                let mut last_beat = Instant::now();
                let mut watcher = notify::Watcher::default();
                loop {
                    let mut list = sessions::read(&sessions::dir());
                    let options = handle.state::<AppState>().settings.lock().unwrap().clone();
                    if options.show_agents || options.show_context {
                        let projects = transcript::projects_dir();
                        for session in &mut list {
                            let Some(path) = transcript::find(&projects, &session.session_id) else {
                                continue;
                            };
                            if options.show_agents {
                                session.agents = transcript::agents(&path);
                            }
                            if options.show_context {
                                session.info = transcript::info(&path);
                            }
                        }
                    }
                    for (kind, data) in tracker.sessions(&list) {
                        log::event(kind, data);
                    }
                    for (reason, name) in watcher.sessions(&list) {
                        let (enabled, text) = match reason {
                            notify::Reason::NeedsYou => (options.notify_attention, "Needs you"),
                            notify::Reason::Finished => (options.notify_finished, "Finished"),
                        };
                        log::event(
                            "notification",
                            json!({ "session": name, "text": text, "enabled": enabled }),
                        );
                        if enabled {
                            notify::show(&handle, &name, text);
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
                        log::event("tray", json!({ "snapshot": snapshot }));
                        last_snapshot = snapshot;
                        update_tray(&handle);
                    }
                    if state.position_dirty.swap(false, Ordering::SeqCst) {
                        let settings = state.settings.lock().unwrap();
                        settings.save();
                        log::event("position", json!({ "x": settings.x, "y": settings.y }));
                    }
                    if last_beat.elapsed() >= HEARTBEAT {
                        last_beat = Instant::now();
                        let visible = is_visible(&handle);
                        log::event(
                            "heartbeat",
                            json!({ "sessions": state.sessions.lock().unwrap().len(), "visible": visible }),
                        );
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build the widget")
        .run(|_, event| {
            if let tauri::RunEvent::Exit = event {
                log::event("exit", json!({}));
            }
        });
}
