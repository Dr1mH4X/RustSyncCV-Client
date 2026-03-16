#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

#[path = "log.rs"]
mod app_log;
mod config;
mod events;
mod runtime;
mod state;
mod syseffects;
mod tray;

use anyhow::Result;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_store::StoreExt;
use tokio::runtime::Runtime;

use app_log::{frontend_log, open_log_folder, setup_logger};
use config::{get_hostname, get_initial_state, save_settings};
use runtime::config::Config;
use runtime::{spawn_runtime, StartOptions};
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
    let (handle, event_rx) = spawn_runtime(&runtime);

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
            tray::setup_tray(&app_handle)?;

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
            events::listen_events(app_handle.clone(), state, event_rx);

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
