#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod runtime;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use simplelog::{
    ColorChoice, CombinedLogger, ConfigBuilder, LevelFilter, SharedLogger, TermLogger,
    TerminalMode, WriteLogger,
};
use std::fs::File;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_store::StoreExt;
use tokio::runtime::Runtime;
#[cfg(target_os = "windows")]
use window_vibrancy::{apply_acrylic, apply_mica};

use runtime::config::Config;
use runtime::{spawn_runtime, ConnectionStateEvent, RuntimeEvent, RuntimeHandle, StartOptions};

// --- Data Structures ---

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SettingsForm {
    pub server_url: String,
    pub token: String,
    pub username: String,
    pub password: String,
    pub max_image_kb: i32,
    pub material_effect: String,
    pub theme_mode: String,
}

#[derive(Serialize, Clone)]
struct InitialState {
    paused: bool,
    config: SettingsForm,
    logs: Vec<String>,
}

struct AppState {
    runtime: Arc<Runtime>,
    handle: RuntimeHandle,
    // We keep logs in memory to support "clear logs" and initial load
    logs: Mutex<Vec<String>>,
    // Simple state to track paused status for initial load
    paused: Mutex<bool>,
}

impl AppState {
    fn push_log(&self, line: String) {
        let mut logs = self.logs.lock();
        if logs.len() > 2000 {
            let keep = 2000;
            let remove = logs.len().saturating_sub(keep);
            if remove > 0 {
                logs.drain(0..remove);
            }
        }
        logs.push(line);
    }

    fn get_logs(&self) -> Vec<String> {
        self.logs.lock().clone()
    }

    fn clear_logs(&self) {
        self.logs.lock().clear();
    }

    fn set_paused(&self, paused: bool) {
        *self.paused.lock() = paused;
    }

    fn is_paused(&self) -> bool {
        *self.paused.lock()
    }
}

// --- Helper Functions ---

fn settings_form_from_config(cfg: &Config) -> SettingsForm {
    let max_image = cfg
        .max_image_kb
        .clamp(Config::MIN_IMAGE_KB, Config::MAX_IMAGE_KB);

    SettingsForm {
        server_url: cfg.server_url.clone(),
        token: cfg.token.clone().unwrap_or_default(),
        username: cfg.username.clone().unwrap_or_default(),
        password: cfg.password.clone().unwrap_or_default(),
        max_image_kb: max_image as i32,
        material_effect: "acrylic".to_string(),
        theme_mode: "system".to_string(),
    }
}

// --- Commands ---

#[tauri::command]
async fn get_initial_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<InitialState, String> {
    let store = app
        .store("config.json")
        .map_err(|e| format!("Store error: {}", e))?;
    let config_val = store.get("config");
    let config = if let Some(val) = config_val {
        serde_json::from_value::<Config>(val)
            .map(|c| settings_form_from_config(&c))
            .unwrap_or_default()
    } else {
        settings_form_from_config(&Config::default())
    };

    let logs = state.get_logs();
    let paused = state.is_paused();

    Ok(InitialState {
        paused,
        config,
        logs,
    })
}

#[tauri::command]
async fn toggle_pause(state: State<'_, AppState>) -> Result<(), String> {
    let paused = state.is_paused();
    let handle = &state.handle;

    if paused {
        handle.resume().await.map_err(|e| e.to_string())?;
    } else {
        handle.pause().await.map_err(|e| e.to_string())?;
    }

    // Note: The UI update will happen via the event stream (ConnectionStateEvent)
    Ok(())
}

#[tauri::command]
fn clear_logs(state: State<'_, AppState>) -> Result<(), String> {
    state.clear_logs();
    // We assume the frontend clears its local state immediately upon calling this
    Ok(())
}

#[tauri::command]
fn open_log_folder() -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let log_dir = exe_path.parent().unwrap().join("logs");
    if log_dir.exists() {
        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(log_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(not(target_os = "windows"))]
        std::process::Command::new("xdg-open")
            .arg(log_dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn frontend_log(level: String, message: String) {
    let target = "frontend";
    match level.as_str() {
        "error" => log::error!(target: target, "{}", message),
        "warn" => log::warn!(target: target, "{}", message),
        "info" => log::info!(target: target, "{}", message),
        "debug" => log::debug!(target: target, "{}", message),
        _ => log::info!(target: target, "{}", message),
    }
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    form: SettingsForm,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let server_url = form.server_url.trim();
    if server_url.is_empty() {
        return Err("Server URL cannot be empty".to_string());
    }

    let token_str = form.token.trim();
    let username_str = form.username.trim();
    let password_str = form.password.trim();

    let (token_opt, username_opt, password_opt) = if !token_str.is_empty() {
        (Some(token_str.to_string()), None, None)
    } else {
        if username_str.is_empty() || password_str.is_empty() {
            return Err("Please provide either Token or Username/Password".to_string());
        }
        (
            None,
            Some(username_str.to_string()),
            Some(password_str.to_string()),
        )
    };

    let max_image_kb =
        form.max_image_kb
            .clamp(Config::MIN_IMAGE_KB as i32, Config::MAX_IMAGE_KB as i32) as u64;

    let updated_config = Config {
        server_url: server_url.to_string(),
        token: token_opt,
        username: username_opt,
        password: password_opt,
        max_image_kb,
        material_effect: "acrylic".to_string(),
        theme_mode: "system".to_string(),
    };

    let store = app
        .store("config.json")
        .map_err(|e| format!("Store error: {}", e))?;
    store.set("config", serde_json::json!(updated_config));
    store.save().map_err(|e| format!("Save error: {}", e))?;

    // Reload runtime
    let options = StartOptions {
        config: updated_config,
    };

    state
        .handle
        .reload(options)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn apply_window_effects(window: WebviewWindow, _effect: String, _theme: String) {
    #[cfg(target_os = "windows")]
    {
        // Basic theme handling
        let _ = window.set_skip_taskbar(false);

        // Apply effect
        if _effect == "acrylic" {
            let _ = apply_acrylic(&window, Some((0, 0, 0, 0)));
        } else {
            let _ = apply_mica(&window, Some(_theme == "dark"));
        }
    }
}

// --- Main ---

fn main() -> Result<()> {
    // Initialize Logger
    let exe_path = std::env::current_exe().context("Failed to get exe path")?;
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new("."));
    let log_dir = exe_dir.join("logs");
    std::fs::create_dir_all(&log_dir).context("Failed to create log dir")?;

    let backend_log_file =
        File::create(log_dir.join("backend.log")).context("Failed to create backend log file")?;
    let frontend_log_file =
        File::create(log_dir.join("frontend.log")).context("Failed to create frontend log file")?;

    // Backend config: ignore "frontend" target
    let backend_config = ConfigBuilder::new()
        .add_filter_ignore_str("fontdb")
        .add_filter_ignore_str("frontend")
        .build();

    // Frontend config: allow ONLY "frontend" target
    let frontend_config = ConfigBuilder::new()
        .add_filter_allow_str("frontend")
        .build();

    // Terminal config (shows everything)
    let term_config = ConfigBuilder::new().add_filter_ignore_str("fontdb").build();

    let mut loggers: Vec<Box<dyn SharedLogger>> = Vec::new();

    loggers.push(TermLogger::new(
        LevelFilter::Info,
        term_config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    ));

    loggers.push(WriteLogger::new(
        LevelFilter::Debug,
        backend_config,
        backend_log_file,
    ));
    loggers.push(WriteLogger::new(
        LevelFilter::Debug,
        frontend_config,
        frontend_log_file,
    ));

    CombinedLogger::init(loggers).ok();
    log::info!("Backend initialized");

    let runtime = Arc::new(Runtime::new()?);

    // Spawn core runtime
    let (handle, mut event_rx) = spawn_runtime(&runtime);

    let app_state = AppState {
        runtime: runtime.clone(),
        handle: handle.clone(),
        logs: Mutex::new(Vec::new()),
        paused: Mutex::new(true),
    };

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

            runtime_clone.spawn(async move {
                let options = StartOptions { config };
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
                    }
                }
            });

            // Apply initial window effect (Mica/Dark)
            let window = app.get_webview_window("main").unwrap();

            #[cfg(target_os = "windows")]
            {
                // Default to mica/system
                let _ = apply_mica(&window, None);
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_state,
            toggle_pause,
            clear_logs,
            open_log_folder,
            frontend_log,
            save_settings,
            apply_window_effects
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    window.hide().unwrap();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    Ok(())
}
