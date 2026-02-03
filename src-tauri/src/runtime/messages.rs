use serde::{Deserialize, Serialize};

// 消息类型常量
pub const MSG_TYPE_CLIPBOARD_UPDATE: &str = "clipboard_update";

// 内容类型常量
pub const CONTENT_TYPE_TEXT: &str = "text/plain";
pub const CONTENT_TYPE_IMAGE_PNG: &str = "image/png";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: AuthRequestPayload,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthRequestPayload {
    // Optional token or username/password
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardUpdate {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: ClipboardUpdatePayload,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardUpdatePayload {
    pub content_type: String,
    pub data: String,
    pub sender_device_id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthResponse {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: AuthResponsePayload,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AuthResponsePayload {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardBroadcast {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: ClipboardBroadcastPayload,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClipboardBroadcastPayload {
    pub content_type: String,
    pub data: String,
}

// 客户端发送的消息结构
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", content = "payload")]
pub enum ClientMessage {
    // 文本消息
    #[serde(rename = "text")]
    Text { data: String },
    // 图片消息（PNG 格式二进制）
    #[serde(rename = "image")]
    Image { data: Vec<u8> },
}
