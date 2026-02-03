use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, Context, Result};
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

impl Config {
    pub fn load_from_dir(base_dir: &Path) -> Result<Self> {
        let config_path = base_dir.join("config.toml");
        if !config_path.exists() {
            let template = r#"# clipsync_rust_client configuration
server_url = "wss://example.com/ws"
# token = "YOUR_TOKEN"
# username = "user"
# password = "pass"
# max_image_kb = 512
# material_effect = "mica"  # 可选: mica 或 acrylic
# theme_mode = "system"  # 可选: system, dark, light; Windows 上影响窗口主题（优先于系统默认）
"#;
            fs::write(&config_path, template)
                .with_context(|| format!("无法写入默认配置文件: {:?}", config_path))?;
            return Err(anyhow!(
                "已在 {:?} 生成默认配置模板，请先填写后重试。",
                config_path
            ));
        }

        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("读取配置文件失败: {:?}", config_path))?;
        let cfg: Config = toml::from_str(&content)
            .with_context(|| format!("解析配置文件失败: {:?}", config_path))?;
        Ok(cfg)
    }

    pub fn locate_and_load() -> Result<(Self, PathBuf)> {
        let cwd = std::env::current_dir().context("获取当前工作目录失败")?;
        let cfg = Self::load_from_dir(&cwd)?;
        Ok((cfg, cwd))
    }

    pub fn save_to_dir(&self, base_dir: &Path) -> Result<()> {
        let config_path = base_dir.join("config.toml");
        let content = toml::to_string_pretty(self).context("序列化配置失败")?;
        fs::write(&config_path, content)
            .with_context(|| format!("写入配置文件失败: {:?}", config_path))?;
        Ok(())
    }
}
