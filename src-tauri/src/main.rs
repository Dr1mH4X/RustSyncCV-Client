#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod runtime;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use simplelog::{ColorChoice, ConfigBuilder, LevelFilter, TermLogger, TerminalMode};
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State, WebviewWindow};
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
    config_dir: PathBuf,
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

fn resolve_config_dir() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    if cwd.join("config.toml").exists() {
        return Ok(cwd);
    }
    if let Some(parent) = cwd.parent() {
        let parent = parent.to_path_buf();
        if parent.join("config.toml").exists() {
            return Ok(parent);
        }
    }
    Ok(cwd)
}

fn settings_form_from_config(cfg: &Config) -> SettingsForm {
    let max_image = cfg.max_image_kb.clamp(32, 8192);

    SettingsForm {
        server_url: cfg.server_url.clone(),
        token: cfg.token.clone().unwrap_or_default(),
        username: cfg.username.clone().unwrap_or_default(),
        password: cfg.password.clone().unwrap_or_default(),
        max_image_kb: max_image as i32,
        material_effect: cfg.material_effect.clone(),
        theme_mode: cfg.theme_mode.clone(),
    }
}

// --- Commands ---

#[tauri::command]
async fn get_initial_state(state: State<'_, AppState>) -> Result<InitialState, String> {
    let config_dir = state.config_dir.clone();
    let config = Config::load_from_dir(&config_dir)
        .map(|cfg| settings_form_from_config(&cfg))
        .unwrap_or_default();

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
async fn save_settings(form: SettingsForm, state: State<'_, AppState>) -> Result<(), String> {
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

    let max_image_kb = form.max_image_kb.clamp(32, 8192) as u64;

    let updated_config = Config {
        server_url: server_url.to_string(),
        token: token_opt,
        username: username_opt,
        password: password_opt,
        max_image_kb,
        material_effect: form.material_effect.clone(),
        theme_mode: form.theme_mode.clone(),
    };

    updated_config
        .save_to_dir(&state.config_dir)
        .map_err(|e| e.to_string())?;

    // Reload runtime
    let options = StartOptions {
        config_dir: state.config_dir.clone(),
    };

    state
        .handle
        .reload(options)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn apply_window_effects(window: WebviewWindow, effect: String, theme: String) {
    #[cfg(target_os = "windows")]
    {
        // Basic theme handling
        let _ = window.set_skip_taskbar(false);

        // Apply effect
        if effect == "acrylic" {
            let _ = apply_acrylic(&window, Some((0, 0, 0, 0)));
        } else {
            let _ = apply_mica(&window, Some(theme == "dark"));
        }
    }
}

// --- Main ---

fn main() -> Result<()> {
    // Initialize Logger
    let log_config = ConfigBuilder::new().add_filter_ignore_str("fontdb").build();
    TermLogger::init(
        LevelFilter::Info,
        log_config,
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .ok();

    let runtime = Arc::new(Runtime::new()?);
    let config_dir = resolve_config_dir()?;

    // Spawn core runtime
    let (handle, mut event_rx) = spawn_runtime(&runtime);

    let app_state = AppState {
        runtime: runtime.clone(),
        handle: handle.clone(),
        config_dir: config_dir.clone(),
        logs: Mutex::new(Vec::new()),
        paused: Mutex::new(true),
    };

    // Initialize application logic (auto start)
    {
        let start_handle = app_state.handle.clone();
        let start_conf_dir = app_state.config_dir.clone();
        app_state.runtime.spawn(async move {
            let options = StartOptions {
                config_dir: start_conf_dir,
            };
            // Allow some time for UI to potentially be ready
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(err) = start_handle.start(options).await {
                log::error!("Failed to auto-start runtime: {}", err);
            }
        });
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let state = app.state::<AppState>();

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
            save_settings,
            apply_window_effects
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");

    Ok(())
}
