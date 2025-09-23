use crate::messages::{ClipboardUpdate, ClipboardUpdatePayload, ClipboardBroadcastPayload};
use arboard::Clipboard;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use tokio::sync::mpsc;
use uuid::Uuid;

/// Starts monitoring the system clipboard. Sends ClipboardUpdate messages via `tx`.
pub async fn start_clipboard_monitor(
    tx: tokio::sync::broadcast::Sender<ClipboardUpdate>,
    disable_flag: Arc<AtomicBool>,
    device_id: String,
) {
    let mut clipboard = Clipboard::new().expect("Failed to create clipboard context");
    let mut last_data = String::new();
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        // Skip one cycle if setter just updated clipboard
        if disable_flag.swap(false, Ordering::SeqCst) {
            continue;
        }
        if let Ok(text) = clipboard.get_text() {
            if text != last_data {
                last_data = text.clone();
                let payload = ClipboardUpdatePayload {
                    content_type: "text".to_string(),
                    data: text,
                    sender_device_id: device_id.clone(),
                };
                let msg = ClipboardUpdate {
                    msg_type: "clipboard_update".to_string(),
                    payload,
                };
                let _ = tx.send(msg);
            }
        }
        // TODO: handle image data
    }
}

/// Starts a clipboard setter that listens for ClipboardBroadcast messages and updates the system clipboard.
pub async fn start_clipboard_setter(
    mut rx: mpsc::Receiver<ClipboardBroadcastPayload>,
    disable_flag: Arc<AtomicBool>,
) {
    let mut clipboard = Clipboard::new().expect("Failed to create clipboard context");
    while let Some(payload) = rx.recv().await {
        disable_flag.store(true, Ordering::SeqCst);
        match payload.content_type.as_str() {
            "text" => {
                if let Err(e) = clipboard.set_text(payload.data) {
                    eprintln!("Failed to set clipboard text: {}", e);
                }
            }
            _ => {
                // TODO: handle image data
            }
        }
    }
}