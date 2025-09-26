use crate::messages::{
    ClipboardUpdate, ClipboardUpdatePayload, CONTENT_TYPE_IMAGE_PNG, CONTENT_TYPE_TEXT,
    MSG_TYPE_CLIPBOARD_UPDATE,
};
use arboard::Clipboard;
use image::{DynamicImage, ImageFormat, RgbaImage};
use tokio::task;
use tokio::time::{sleep, Duration};
// (WebSocket Message alias removed; no direct send here now)
use base64::Engine;
use std::io::Cursor;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{broadcast, mpsc}; // for .encode on STANDARD engine

// 监控剪贴板文本 / 图片变化并通过 broadcast::Sender 发送
pub async fn start_clipboard_monitor(
    tx: broadcast::Sender<ClipboardUpdate>,
    disable_flag: Arc<AtomicBool>,
    device_id: String,
    max_image_kb: u64,
) {
    let mut last_text = String::new();
    let mut last_image_hash: Option<u64> = None;
    let mut last_send_time = std::time::Instant::now();
    const MIN_INTERVAL: Duration = Duration::from_millis(400); // 发送节流间隔
    loop {
        if disable_flag.load(Ordering::SeqCst) {
            sleep(Duration::from_millis(300)).await;
            continue;
        }

        let result = task::spawn_blocking(|| {
            let mut cb = Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
            // 优先尝试文本
            if let Ok(t) = cb.get_text() {
                return Ok::<_, String>(ClipboardContent::Text(t));
            }
            // 再尝试图片
            if let Ok(img) = cb.get_image() {
                return Ok(ClipboardContent::Image(img));
            }
            Err("No supported clipboard content".to_string())
        })
        .await;

        if let Ok(Ok(content)) = result {
            let now = std::time::Instant::now();
            if now.duration_since(last_send_time) < MIN_INTERVAL { /* 节流:太频繁 */
            } else {
                match content {
                    ClipboardContent::Text(text) => {
                        if !text.is_empty() && text != last_text {
                            last_text = text.clone();
                            last_send_time = now;
                            println!("[clip:text] changed len={} broadcasting", last_text.len());
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
                    ClipboardContent::Image(raw_img) => {
                        // 转 RGBA 并编码 PNG -> base64
                        if let Some(rgba) = RgbaImage::from_raw(
                            raw_img.width as u32,
                            raw_img.height as u32,
                            raw_img.bytes.into_owned(),
                        ) {
                            let mut cursor = Cursor::new(Vec::new());
                            if DynamicImage::ImageRgba8(rgba)
                                .write_to(&mut cursor, ImageFormat::Png)
                                .is_ok()
                            {
                                let bytes = cursor.into_inner();
                                // 大小限制检查
                                if (bytes.len() as u64) > max_image_kb * 1024 {
                                    println!(
                                        "[clip:image] skip oversized size={} limit={}KB",
                                        bytes.len(),
                                        max_image_kb
                                    );
                                } else {
                                    // 计算简单哈希避免重复
                                    use std::hash::{Hash, Hasher};
                                    let mut hasher =
                                        std::collections::hash_map::DefaultHasher::new();
                                    bytes.hash(&mut hasher);
                                    let h = hasher.finish();
                                    if Some(h) != last_image_hash {
                                        last_image_hash = Some(h);
                                        last_send_time = now;
                                        println!(
                                            "[clip:image] broadcasting size={} bytes",
                                            bytes.len()
                                        );
                                        let b64 = base64::engine::general_purpose::STANDARD
                                            .encode(&bytes);
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
            }
        }
        sleep(Duration::from_millis(500)).await;
    }
}

// 接收来自服务器的广播并设置本地剪贴板
pub async fn start_clipboard_setter(
    mut rx: mpsc::Receiver<crate::messages::ClipboardBroadcastPayload>,
    disable_flag: Arc<AtomicBool>,
) {
    while let Some(payload) = rx.recv().await {
        if payload.content_type == CONTENT_TYPE_TEXT {
            let text = payload.data.clone();
            disable_flag.store(true, Ordering::SeqCst); // 防止回环
            let set_res = task::spawn_blocking(move || {
                let mut cb = Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
                cb.set_text(text)
                    .map_err(|e| format!("Clipboard set_text error: {e}"))?;
                Ok::<(), String>(())
            })
            .await;
            if let Err(e) = set_res {
                eprintln!("[setter] join error: {e}");
            }
            disable_flag.store(false, Ordering::SeqCst);
        } else if payload.content_type == CONTENT_TYPE_IMAGE_PNG {
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&payload.data) {
                // arboard 需要 BGRA 或 RGBA? arboard::ImageData 是 RGBA 8bit
                // 这里假设服务器下发的是我们自己编码的 PNG: 解码后再用 image crate 解析
                if let Ok(img) = image::load_from_memory(&bytes) {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    disable_flag.store(true, Ordering::SeqCst);
                    let res = task::spawn_blocking(move || {
                        let mut cb =
                            Clipboard::new().map_err(|e| format!("Clipboard init error: {e}"))?;
                        let data = arboard::ImageData {
                            width: w as usize,
                            height: h as usize,
                            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
                        };
                        cb.set_image(data)
                            .map_err(|e| format!("Clipboard set_image error: {e}"))?;
                        Ok::<(), String>(())
                    })
                    .await;
                    if let Err(e) = res {
                        eprintln!("[setter] image join error: {e}");
                    }
                    disable_flag.store(false, Ordering::SeqCst);
                }
            }
        }
    }
}

// 内部枚举：临时区分读取到的剪贴板内容
enum ClipboardContent {
    Text(String),
    Image(arboard::ImageData<'static>),
}
