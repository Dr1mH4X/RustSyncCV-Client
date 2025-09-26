use std::{
    borrow::Cow,
    hash::{Hash, Hasher},
    io::Cursor,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::Result;
use arboard::{Clipboard, ImageData as ClipboardImage};
use base64::Engine;
use image::{DynamicImage, ImageFormat, RgbaImage};
use log::Level;
use tokio::sync::{broadcast, mpsc};
use tokio::{
    task,
    time::{sleep, Duration},
};
use tokio_util::sync::CancellationToken;

use super::{RuntimeEvent, RuntimeLogEvent};
use crate::runtime::messages::{
    ClipboardBroadcastPayload, ClipboardUpdate, ClipboardUpdatePayload, CONTENT_TYPE_IMAGE_PNG,
    CONTENT_TYPE_TEXT, MSG_TYPE_CLIPBOARD_UPDATE,
};

const MONITOR_INTERVAL: Duration = Duration::from_millis(500);
const MIN_BROADCAST_INTERVAL: Duration = Duration::from_millis(400);

pub async fn start_clipboard_monitor(
    tx: broadcast::Sender<ClipboardUpdate>,
    disable_flag: Arc<AtomicBool>,
    device_id: String,
    max_image_kb: u64,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    let mut last_text = String::new();
    let mut last_image_hash: Option<u64> = None;
    let mut last_send_time = std::time::Instant::now();

    loop {
        if cancel.is_cancelled() {
            break;
        }

        if disable_flag.load(Ordering::SeqCst) {
            if tokio::select! {
                _ = cancel.cancelled() => true,
                _ = sleep(MONITOR_INTERVAL) => false,
            } {
                break;
            }
            continue;
        }

        let clipboard_state = task::spawn_blocking(|| read_clipboard_content()).await;

        if let Ok(Ok(content)) = clipboard_state {
            let now = std::time::Instant::now();
            if now.duration_since(last_send_time) >= MIN_BROADCAST_INTERVAL {
                match content {
                    ClipboardContent::Text(text) => {
                        if !text.is_empty() && text != last_text {
                            last_text = text.clone();
                            last_send_time = now;
                            let _ = events
                                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                                    Level::Info,
                                    format!("检测到文本剪贴板更新 len={}", text.len()),
                                )))
                                .await;
                            let _ = events
                                .send(RuntimeEvent::ClipboardSent {
                                    content_type: CONTENT_TYPE_TEXT.to_string(),
                                })
                                .await;
                            let update = ClipboardUpdate {
                                msg_type: MSG_TYPE_CLIPBOARD_UPDATE.to_string(),
                                payload: ClipboardUpdatePayload {
                                    content_type: CONTENT_TYPE_TEXT.to_string(),
                                    data: text,
                                    sender_device_id: device_id.clone(),
                                },
                            };
                            let _ = tx.send(update);
                        }
                    }
                    ClipboardContent::Image(bytes, width, height) => {
                        if let Ok(encoded) = encode_png(bytes, width, height) {
                            if (encoded.len() as u64) > max_image_kb * 1024 {
                                let _ = events
                                    .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                                        Level::Warn,
                                        format!(
                                            "跳过过大的图片 size={} limit={}KB",
                                            encoded.len(),
                                            max_image_kb
                                        ),
                                    )))
                                    .await;
                            } else {
                                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                                encoded.hash(&mut hasher);
                                let hash = hasher.finish();
                                if Some(hash) != last_image_hash {
                                    last_image_hash = Some(hash);
                                    last_send_time = now;
                                    let _ = events
                                        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                                            Level::Info,
                                            format!(
                                                "检测到图片剪贴板更新 size={} bytes",
                                                encoded.len()
                                            ),
                                        )))
                                        .await;
                                    let _ = events
                                        .send(RuntimeEvent::ClipboardSent {
                                            content_type: CONTENT_TYPE_IMAGE_PNG.to_string(),
                                        })
                                        .await;
                                    let b64 =
                                        base64::engine::general_purpose::STANDARD.encode(&encoded);
                                    let update = ClipboardUpdate {
                                        msg_type: MSG_TYPE_CLIPBOARD_UPDATE.to_string(),
                                        payload: ClipboardUpdatePayload {
                                            content_type: CONTENT_TYPE_IMAGE_PNG.to_string(),
                                            data: b64,
                                            sender_device_id: device_id.clone(),
                                        },
                                    };
                                    let _ = tx.send(update);
                                }
                            }
                        }
                    }
                }
            }
        } else if let Ok(Err(e)) = clipboard_state {
            let _ = events
                .send(RuntimeEvent::Log(RuntimeLogEvent::new(
                    Level::Error,
                    format!("读取剪贴板失败: {}", e),
                )))
                .await;
        }

        if tokio::select! {
            _ = cancel.cancelled() => true,
            _ = sleep(MONITOR_INTERVAL) => false,
        } {
            break;
        }
    }

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            String::from("剪贴板监听退出"),
        )))
        .await;
}

pub async fn start_clipboard_setter(
    mut rx: mpsc::Receiver<ClipboardBroadcastPayload>,
    disable_flag: Arc<AtomicBool>,
    events: mpsc::Sender<RuntimeEvent>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            maybe_payload = rx.recv() => {
                if let Some(payload) = maybe_payload {
                    match payload.content_type.as_str() {
                        CONTENT_TYPE_TEXT => {
                            if let Err(err) = set_text(&payload.data, disable_flag.clone()).await {
                                let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Error, format!("设置文本剪贴板失败: {}", err)))) .await;
                            } else {
                                let _ = events.send(RuntimeEvent::ClipboardReceived { content_type: CONTENT_TYPE_TEXT.to_string() }).await;
                                let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Info, String::from("已应用来自远端的文本剪贴板")))).await;
                            }
                        }
                        CONTENT_TYPE_IMAGE_PNG => {
                            if let Err(err) = set_image_from_base64(&payload.data, disable_flag.clone()).await {
                                let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Error, format!("设置图片剪贴板失败: {}", err)))).await;
                            } else {
                                let _ = events.send(RuntimeEvent::ClipboardReceived { content_type: CONTENT_TYPE_IMAGE_PNG.to_string() }).await;
                                let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Info, String::from("已应用来自远端的图片剪贴板")))).await;
                            }
                        }
                        other => {
                            let _ = events.send(RuntimeEvent::Log(RuntimeLogEvent::new(Level::Warn, format!("收到未知类型剪贴板: {}", other)))).await;
                        }
                    }
                } else {
                    break;
                }
            }
        }
    }

    let _ = events
        .send(RuntimeEvent::Log(RuntimeLogEvent::new(
            Level::Debug,
            String::from("剪贴板写入任务退出"),
        )))
        .await;
}

async fn set_text(text: &str, disable_flag: Arc<AtomicBool>) -> Result<()> {
    let content = text.to_string();
    disable_flag.store(true, Ordering::SeqCst);
    let result = task::spawn_blocking(move || {
        let mut cb = Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
        cb.set_text(content)
            .map_err(|e| format!("Clipboard set_text error: {e}"))?;
        Ok::<(), String>(())
    })
    .await;
    disable_flag.store(false, Ordering::SeqCst);
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(anyhow::anyhow!(err)),
        Err(join_err) => Err(anyhow::anyhow!("任务 join 出错: {}", join_err)),
    }
}

async fn set_image_from_base64(data: &str, disable_flag: Arc<AtomicBool>) -> Result<()> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|err| anyhow::anyhow!("Base64 解码失败: {}", err))?;
    let image =
        image::load_from_memory(&bytes).map_err(|err| anyhow::anyhow!("PNG 解码失败: {}", err))?;
    let rgba = image.to_rgba8();
    let (w, h) = rgba.dimensions();
    disable_flag.store(true, Ordering::SeqCst);
    let result = task::spawn_blocking(move || {
        let mut cb = Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
        let data = ClipboardImage {
            width: w as usize,
            height: h as usize,
            bytes: Cow::Owned(rgba.into_raw()),
        };
        cb.set_image(data)
            .map_err(|e| format!("Clipboard set_image error: {e}"))?;
        Ok::<(), String>(())
    })
    .await;
    disable_flag.store(false, Ordering::SeqCst);
    match result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(anyhow::anyhow!(err)),
        Err(join_err) => Err(anyhow::anyhow!("任务 join 出错: {}", join_err)),
    }
}

fn read_clipboard_content() -> Result<ClipboardContent, String> {
    let mut cb = Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
    if let Ok(text) = cb.get_text() {
        return Ok(ClipboardContent::Text(text));
    }
    if let Ok(image) = cb.get_image() {
        let width = image.width as u32;
        let height = image.height as u32;
        Ok(ClipboardContent::Image(
            image.bytes.into_owned(),
            width,
            height,
        ))
    } else {
        Err("剪贴板没有可识别的内容".into())
    }
}

fn encode_png(bytes: Vec<u8>, width: u32, height: u32) -> Result<Vec<u8>, String> {
    if let Some(rgba) = RgbaImage::from_raw(width, height, bytes) {
        let mut cursor = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut cursor, ImageFormat::Png)
            .map_err(|e| format!("PNG 编码失败: {e}"))?;
        Ok(cursor.into_inner())
    } else {
        Err("无法构造图像".into())
    }
}

enum ClipboardContent {
    Text(String),
    Image(Vec<u8>, u32, u32),
}
