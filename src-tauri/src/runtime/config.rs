use serde::{Deserialize, Serialize};

fn default_max_image_kb() -> u64 {
    512
}

fn default_material_effect() -> String {
    "acrylic".to_string()
}

fn default_theme_mode() -> String {
    "system".to_string()
}

fn default_language() -> String {
    "en".to_string()
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SettingsForm {
    pub server_url: String,
    pub token: String,
    pub username: String,
    pub password: String,
    pub max_image_kb: i32,
    pub material_effect: String,
    pub theme_mode: String,
    pub language: String,
}

impl From<&Config> for SettingsForm {
    fn from(cfg: &Config) -> Self {
        let max_image = cfg
            .max_image_kb
            .clamp(Config::MIN_IMAGE_KB, Config::MAX_IMAGE_KB);

        SettingsForm {
            server_url: cfg.server_url.clone(),
            token: cfg.token.clone().unwrap_or_default(),
            username: cfg.username.clone().unwrap_or_default(),
            password: cfg.password.clone().unwrap_or_default(),
            max_image_kb: max_image as i32,
            material_effect: cfg.material_effect.clone(),
            theme_mode: cfg.theme_mode.clone(),
            language: cfg.language.clone(),
        }
    }
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
    #[serde(default = "default_language")]
    pub language: String,
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
            language: default_language(),
        }
    }
}

impl Config {
    pub const MIN_IMAGE_KB: u64 = 1;
    pub const MAX_IMAGE_KB: u64 = 524288;
}
