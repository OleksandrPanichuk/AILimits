mod credentials;
mod pace;
mod tray_icon;
mod usage;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_positioner::{Position, WindowExt};
use usage::Snapshot;

const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Refresh on popup open if the data is older than this.
const STALE_AFTER: chrono::Duration = chrono::Duration::seconds(60);
const TRAY_ID: &str = "main";

#[derive(Default, Clone, Serialize)]
struct UiState {
    snapshot: Option<Snapshot>,
    error: Option<String>,
    refreshing: bool,
}

struct AppState {
    ui: Mutex<UiState>,
    client: reqwest::Client,
    in_flight: AtomicBool,
    last_hidden: Mutex<Option<Instant>>,
}

#[tauri::command]
fn get_state(state: tauri::State<'_, AppState>) -> UiState {
    state.ui.lock().unwrap().clone()
}

#[tauri::command]
async fn refresh_now(app: AppHandle) -> UiState {
    refresh(&app).await;
    app.state::<AppState>().ui.lock().unwrap().clone()
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

async fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.in_flight.swap(true, Ordering::SeqCst) {
        return;
    }
    set_refreshing(app, true);

    let result = match credentials::load() {
        Ok(creds) => usage::fetch(&state.client, &creds).await,
        Err(e) => Err(e),
    };
    {
        let mut ui = state.ui.lock().unwrap();
        match result {
            Ok(snapshot) => {
                ui.snapshot = Some(snapshot);
                ui.error = None;
            }
            // Keep the last good snapshot so the popup still has something to show.
            Err(e) => ui.error = Some(e),
        }
        ui.refreshing = false;
    }
    state.in_flight.store(false, Ordering::SeqCst);
    publish(app);
}

fn set_refreshing(app: &AppHandle, value: bool) {
    app.state::<AppState>().ui.lock().unwrap().refreshing = value;
    let _ = app.emit("state", app.state::<AppState>().ui.lock().unwrap().clone());
}

/// Push the current state to the tray icon, its tooltip and the popup.
fn publish(app: &AppHandle) {
    let ui = app.state::<AppState>().ui.lock().unwrap().clone();
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        update_tray(&tray, &ui);
    }
    let _ = app.emit("state", ui);
}

fn update_tray(tray: &TrayIcon, ui: &UiState) {
    let rgba = tray_icon::render(ui.snapshot.as_ref());
    let _ = tray.set_icon(Some(Image::new_owned(rgba, tray_icon::SIZE, tray_icon::SIZE)));
    let _ = tray.set_tooltip(Some(tooltip(ui)));
}

fn tooltip(ui: &UiState) -> String {
    let mut parts = vec!["Claude".to_string()];
    if let Some(s) = &ui.snapshot {
        if let Some(l) = s.session() {
            parts.push(format!("5h: {:.0}% left", (100.0 - l.used_percent).max(0.0)));
        }
        if let Some(l) = s.weekly() {
            parts.push(format!("Week: {:.0}% left", (100.0 - l.used_percent).max(0.0)));
        }
    }
    if ui.error.is_some() {
        parts.push("(update failed)".into());
    }
    parts.join(" · ")
}

fn toggle_popup(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // A click on the tray icon first blurs (and hides) the popup, then arrives here.
    // Without this check that click would immediately reopen it.
    let recently_hidden = app
        .state::<AppState>()
        .last_hidden
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(300));
    if recently_hidden {
        return;
    }
    show_popup(app);
}

fn show_popup(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else { return };
    #[cfg(target_os = "macos")]
    let pos = Position::TrayBottomCenter;
    #[cfg(not(target_os = "macos"))]
    let pos = Position::TrayCenter;
    if window.move_window(pos).is_err() {
        let _ = window.move_window(Position::BottomRight);
    }
    let _ = window.show();
    let _ = window.set_focus();

    let stale = {
        let state = app.state::<AppState>();
        let ui = state.ui.lock().unwrap();
        ui.snapshot
            .as_ref()
            .is_none_or(|s| chrono::Utc::now() - s.fetched_at > STALE_AFTER)
    };
    if stale {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { refresh(&app).await });
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let refresh_item = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let autostart_label = if cfg!(windows) { "Start with Windows" } else { "Start at login" };
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart_item =
        CheckMenuItem::with_id(app, "autostart", autostart_label, true, autostart_on, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&refresh_item, &autostart_item, &PredefinedMenuItem::separator(app)?, &quit_item],
    )?;

    let rgba = tray_icon::render(None);
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::new_owned(rgba, tray_icon::SIZE, tray_icon::SIZE))
        .tooltip("Claude Limits")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "refresh" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { refresh(&app).await });
            }
            "autostart" => {
                let launcher = app.autolaunch();
                let enabled = launcher.is_enabled().unwrap_or(false);
                let _ = if enabled { launcher.disable() } else { launcher.enable() };
                let _ = autostart_item.set_checked(launcher.is_enabled().unwrap_or(false));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_popup(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_popup(app)))
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(AppState {
            ui: Mutex::new(UiState::default()),
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("HTTP client"),
            in_flight: AtomicBool::new(false),
            last_hidden: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![get_state, refresh_now, quit])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            build_tray(app.handle())?;

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    refresh(&handle).await;
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                *window.state::<AppState>().last_hidden.lock().unwrap() = Some(Instant::now());
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running Claude Limits");
}
