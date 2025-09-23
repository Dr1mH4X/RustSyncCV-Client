use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct AuthRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: AuthRequestPayload,
}

#[derive(Serialize, Deserialize)]
pub struct AuthRequestPayload {
    // Optional token or username/password
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClipboardUpdate {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: ClipboardUpdatePayload,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ClipboardUpdatePayload {
    pub content_type: String,
    pub data: String,
    pub sender_device_id: String,
}

#[derive(Serialize, Deserialize)]
pub struct AuthResponse {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: AuthResponsePayload,
}

#[derive(Serialize, Deserialize)]
pub struct AuthResponsePayload {
    pub success: bool,
    pub message: String,
    // Optional new token (e.g., JWT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ClipboardBroadcast {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub payload: ClipboardBroadcastPayload,
}

#[derive(Serialize, Deserialize)]
pub struct ClipboardBroadcastPayload {
    pub content_type: String,
    pub data: String,
}