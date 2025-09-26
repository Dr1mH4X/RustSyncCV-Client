use serde::Deserialize;
// Default sync interval is 5 seconds
fn default_sync_interval() -> u64 {
    5
}
use std::fs;

fn default_max_image_kb() -> u64 {
    512
} // 默认 512KB 限制

fn default_material_effect() -> String {
    "mica".to_string()
}

#[derive(Deserialize)]
pub struct Config {
    pub server_url: String,
    // Optional token authentication
    pub token: Option<String>,
    // Optional username/password authentication
    pub username: Option<String>,
    pub password: Option<String>,
    /// Sync interval in seconds for periodic clipboard sync
    #[serde(default = "default_sync_interval")]
    #[allow(dead_code)]
    pub sync_interval: u64,
    /// 最大允许的图片大小 (KB)，超出将跳过广播
    #[serde(default = "default_max_image_kb")]
    pub max_image_kb: u64,
    /// 是否信任不安全(自签名/无效) TLS 证书 (仅调试用, 默认 false)
    #[serde(default)]
    pub trust_insecure_cert: bool,
    /// Windows 材质效果: mica 或 acrylic
    #[serde(default = "default_material_effect")]
    pub material_effect: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // Read config.toml from current working directory
        let cwd = std::env::current_dir()?;
        let config_file = cwd.join("config.toml");
        if !config_file.exists() {
            // Generate default config file
            let template = "# clipsync_rust_client configuration\nserver_url = \"wss://example.com/ws\"\n# token = \"YOUR_TOKEN\"\n# username = \"user\"\n# password = \"pass\"\n# sync_interval = 5\n# max_image_kb = 512\n# trust_insecure_cert = false  # 仅调试: true 时跳过 TLS 证书校验(风险!)\n# material_effect = \"mica\"  # 可选: mica 或 acrylic\n";
            fs::write(&config_file, template)?;
            return Err(anyhow::anyhow!(
                "Default config created at {:?}. Please update it and rerun.",
                config_file
            ));
        }
        let content = fs::read_to_string(&config_file)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    }
}
