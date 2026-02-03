use serde::{Deserialize, Serialize};

fn default_max_image_kb() -> u64 {
    512
}

fn default_material_effect() -> String {
    "mica".to_string()
}

fn default_theme_mode() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default = "default_max_image_kb")]
    pub max_image_kb: u64,
    #[serde(default = "default_material_effect")]
    pub material_effect: String,
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "wss://example.com/ws".to_string(),
            token: None,
            username: None,
            password: None,
            max_image_kb: default_max_image_kb(),
            material_effect: default_material_effect(),
            theme_mode: default_theme_mode(),
        }
    }
}

impl Config {
    pub const MIN_IMAGE_KB: u64 = 1;
    pub const MAX_IMAGE_KB: u64 = 524288;
}
