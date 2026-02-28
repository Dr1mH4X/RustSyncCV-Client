#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[path = "log.rs"]
mod app_log;
mod config;
mod runtime;
mod state;
mod syseffects;

use anyhow::Result;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_store::StoreExt;
use tokio::runtime::Runtime;

use app_log::{frontend_log, open_log_folder, setup_logger};
use config::{get_hostname, get_initial_state, save_settings};
use runtime::config::Config;
use runtime::{spawn_runtime, ConnectionStateEvent, RuntimeEvent, StartOptions};
use state::AppState;
use syseffects::apply_window_effects;

#[tauri::command]
async fn toggle_pause(state: State<'_, AppState>) -> Result<(), String> {
    let paused = state.is_paused();
    let handle = &state.handle;

    if paused {
        handle.resume().await.map_err(|e| e.to_string())?;
    } else {
        handle.pause().await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Read the current `close_behavior` from the persisted config store.
/// Returns one of `"minimize_to_tray"`, `"minimize"`, or `"quit"`.
/// Falls back to `"minimize_to_tray"` when anything goes wrong.
fn read_close_behavior(app: &AppHandle) -> String {
    let store = match app.store("config.json") {
        Ok(s) => s,
        Err(_) => return "minimize_to_tray".to_string(),
    };
    let config_val = store.get("config");
    if let Some(val) = config_val {
        if let Ok(cfg) = serde_json::from_value::<Config>(val) {
            return cfg.close_behavior;
        }
    }
    "minimize_to_tray".to_string()
}

fn main() -> Result<()> {
    setup_logger()?;

    let runtime = Arc::new(Runtime::new()?);

    // Spawn core runtime
    let (handle, mut event_rx) = spawn_runtime(&runtime);

    let app_state = AppState::new(runtime.clone(), handle.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .manage(app_state)
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let state = app.state::<AppState>();

            // --- Tray Icon ---
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            let _tray = TrayIconBuilder::new()
                .menu(&menu)
                .icon(app.default_window_icon().unwrap().clone())
                .on_menu_event(|app: &AppHandle, event| match event.id().as_ref() {
                    "quit" => app.exit(0),
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event| {
                    if let TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;

            // --- Config Load & Runtime Start ---
            let store = app.store("config.json")?;
            let config_val = store.get("config");
            let config: Config = if let Some(val) = config_val {
                serde_json::from_value(val).unwrap_or_default()
            } else {
                Config::default()
            };

            let start_handle = state.handle.clone();
            let runtime_clone = state.runtime.clone();

            let config_clone = config.clone();
            runtime_clone.spawn(async move {
                let options = StartOptions {
                    config: config_clone,
                };
                // Allow some time for UI to potentially be ready
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                if let Err(err) = start_handle.start(options).await {
                    log::error!("Failed to auto-start runtime: {}", err);
                }
            });

            // Spawn event listener
            let runtime_clone = state.runtime.clone();

            runtime_clone.spawn(async move {
                while let Some(event) = event_rx.recv().await {
                    match &event {
                        RuntimeEvent::Status(text) => {
                            let _ = app_handle.emit("status-update", text);
                        }
                        RuntimeEvent::Connection(conn_state) => {
                            let paused = matches!(
                                conn_state,
                                ConnectionStateEvent::Paused | ConnectionStateEvent::Idle
                            );

                            // Update internal state
                            if let Some(state) = app_handle.try_state::<AppState>() {
                                state.set_paused(paused);
                            }

                            let _ = app_handle.emit(
                                "connection-state",
                                serde_json::json!({
                                    "paused": paused,
                                    "state": format!("{:?}", conn_state)
                                }),
                            );
                        }
                        RuntimeEvent::Log(record) => {
                            log::log!(record.level, "{}", record.message);
                            let line = format!("[{}] {}", record.level, record.message);

                            // Update internal state
                            if let Some(state) = app_handle.try_state::<AppState>() {
                                state.push_log(line.clone());
                            }

                            let _ = app_handle.emit(
                                "log-entry",
                                serde_json::json!({
                                    "line": line,
                                    "level": record.level.to_string()
                                }),
                            );
                        }
                        RuntimeEvent::ClipboardSent { content_type } => {
                            let _ = app_handle.emit(
                                "clipboard-event",
                                serde_json::json!({
                                    "type": "sent",
                                    "contentType": content_type
                                }),
                            );
                            let _ = app_handle.emit(
                                "status-update",
                                format!("Broadcasting clipboard ({})", content_type),
                            );
                        }
                        RuntimeEvent::ClipboardReceived { content_type } => {
                            let _ = app_handle.emit(
                                "clipboard-event",
                                serde_json::json!({
                                    "type": "received",
                                    "contentType": content_type
                                }),
                            );
                            let _ = app_handle.emit(
                                "status-update",
                                format!("Received remote clipboard ({})", content_type),
                            );
                        }
                        RuntimeEvent::Error(msg) => {
                            let _ = app_handle.emit("status-update", format!("Error: {}", msg));
                        }
                        RuntimeEvent::LanPeersChanged(peers_json) => {
                            let _ = app_handle.emit("lan-peers-changed", peers_json);
                        }
                    }
                }
            });

            #[cfg(target_os = "windows")]
            {
                if let Some(window) = app.get_webview_window("main") {
                    apply_window_effects(window, config.material_effect.clone());
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            toggle_pause,
            open_log_folder,
            frontend_log,
            save_settings,
            apply_window_effects,
            get_hostname
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let behavior = read_close_behavior(window.app_handle());

                    match behavior.as_str() {
                        "quit" => {
                            // Let the close proceed — the app will exit.
                        }
                        "minimize" => {
                            // Minimise the window instead of closing.
                            let _ = window.minimize();
                            api.prevent_close();
                        }
                        // "minimize_to_tray" and any unknown value — hide to
                        // system tray (works on all platforms; on Linux the
                        // window is simply hidden since tray support varies).
                        _ => {
                            let _ = window.hide();
                            api.prevent_close();
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    Ok(())
}
