use crate::runtime::{ConnectionStateEvent, RuntimeEvent};
use crate::state::AppState;
use tauri::{AppHandle, Emitter, Manager};

pub fn listen_events(
    app_handle: AppHandle,
    state: tauri::State<'_, AppState>,
    mut event_rx: tokio::sync::mpsc::Receiver<RuntimeEvent>,
) {
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
}
