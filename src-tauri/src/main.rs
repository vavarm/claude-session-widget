#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod sessions;
mod settings;

use serde::Serialize;
use sessions::{Kind, Session};
use settings::Settings;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tauri::{
    image::Image,
    menu::{
        CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
        SubmenuBuilder,
    },
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow, Wry,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const WIDTH: f64 = 280.0;
const TRAY_ID: &str = "main";
const OPACITIES: [u32; 5] = [100, 90, 80, 60, 40];

#[derive(Default)]
struct AppState {
    settings: Mutex<Settings>,
    sessions: Mutex<Vec<Session>>,
    position_dirty: AtomicBool,
}

#[derive(Serialize, Clone)]
struct Payload {
    sessions: Vec<Session>,
    compact: bool,
    opacity: f64,
}

fn payload(app: &AppHandle) -> Payload {
    let state = app.state::<AppState>();
    let sessions = state.sessions.lock().unwrap().clone();
    let settings = state.settings.lock().unwrap();
    Payload {
        sessions,
        compact: settings.compact,
        opacity: settings.opacity,
    }
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
            &startup,
            &PredefinedMenuItem::separator(app)?,
            &folder,
            &quit,
        ])
        .build()
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
            }
        }
    }
    push_state(app);
    refresh_menu(app);
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

fn tray_color(sessions: &[Session]) -> [u8; 3] {
    let any = |kind| sessions.iter().any(|s| s.kind == kind);
    if any(Kind::Attention) {
        [248, 81, 73]
    } else if any(Kind::Busy) {
        [245, 165, 36]
    } else if sessions.is_empty() {
        [130, 130, 140]
    } else {
        [63, 185, 80]
    }
}

fn tray_icon(color: [u8; 3]) -> Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 16.0);
            let alpha = (15.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[color[0], color[1], color[2], (alpha * 255.0) as u8]);
        }
    }
    Image::new_owned(rgba, SIZE, SIZE)
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
    let _ = tray.set_icon(Some(tray_icon(tray_color(&sessions))));
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
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            set_visible(app, true)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            settings: Mutex::new(Settings::load()),
            ..Default::default()
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            fit,
            show_menu,
            menu_action
        ])
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = handle.state::<AppState>().settings.lock().unwrap().clone();
            let window = main_window(&handle).expect("main window is declared in tauri.conf.json");

            window.set_always_on_top(settings.always_on_top)?;
            place_window(&window, &settings);
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
                .icon(tray_icon(tray_color(&[])))
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
                loop {
                    let list = sessions::read(&sessions::dir());
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
