use crate::runtime::config::{Config, SettingsForm};
use crate::runtime::StartOptions;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_store::StoreExt;

#[derive(Serialize, Clone)]
pub struct InitialState {
    paused: bool,
    config: SettingsForm,
    logs: Vec<String>,
}

#[tauri::command]
pub async fn get_initial_state(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<InitialState, String> {
    let store = app
        .store("config.json")
        .map_err(|e| format!("Store error: {}", e))?;
    let config_val = store.get("config");
    let config = if let Some(val) = config_val {
        serde_json::from_value::<Config>(val)
            .map(|c| SettingsForm::from(&c))
            .unwrap_or_else(|_| SettingsForm::from(&Config::default()))
    } else {
        SettingsForm::from(&Config::default())
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
pub fn get_hostname() -> Result<String, String> {
    hostname::get()
        .map_err(|e| format!("Failed to get hostname: {}", e))
        .and_then(|os_str| {
            os_str
                .into_string()
                .map_err(|_| "Hostname contains invalid UTF-8".to_string())
        })
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    form: SettingsForm,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let connection_mode = form.connection_mode.trim();
    let is_lan = connection_mode == "lan";

    // ── Mode-specific validation ─────────────────────────────────────────

    let (server_url, token_opt, username_opt, password_opt) = if is_lan {
        // In LAN mode the server fields are not required; we preserve
        // whatever the user had so switching back to server mode keeps
        // previous values.
        let token = if form.token.trim().is_empty() {
            None
        } else {
            Some(form.token.trim().to_string())
        };
        let username = if form.username.trim().is_empty() {
            None
        } else {
            Some(form.username.trim().to_string())
        };
        let password = if form.password.trim().is_empty() {
            None
        } else {
            Some(form.password.trim().to_string())
        };
        (
            form.server_url.trim().to_string(),
            token,
            username,
            password,
        )
    } else {
        // Server (WebSocket) mode — original validation.
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

        (
            server_url.to_string(),
            token_opt,
            username_opt,
            password_opt,
        )
    };

    // ── Shared field validation ──────────────────────────────────────────

    let max_image_kb =
        form.max_image_kb
            .clamp(Config::MIN_IMAGE_KB as i32, Config::MAX_IMAGE_KB as i32) as u64;

    let close_behavior = match form.close_behavior.as_str() {
        "minimize" | "quit" | "minimize_to_tray" => form.close_behavior.clone(),
        _ => "minimize_to_tray".to_string(),
    };

    let updated_config = Config {
        server_url,
        token: token_opt,
        username: username_opt,
        password: password_opt,
        max_image_kb,
        material_effect: form.material_effect,
        theme_mode: form.theme_mode,
        language: form.language,
        connection_mode: if is_lan {
            "lan".to_string()
        } else {
            "server".to_string()
        },
        lan_device_name: form.lan_device_name.trim().to_string(),
        close_behavior,
    };

    let store = app
        .store("config.json")
        .map_err(|e| format!("Store error: {}", e))?;
    store.set("config", serde_json::json!(updated_config));
    store.save().map_err(|e| format!("Save error: {}", e))?;

    // Reload runtime
    let options = StartOptions {
        config: updated_config.clone(),
    };

    state
        .handle
        .reload(options)
        .await
        .map_err(|e| e.to_string())?;

    app.emit("config-changed", SettingsForm::from(&updated_config))
        .map_err(|e| e.to_string())?;

    Ok(())
}
