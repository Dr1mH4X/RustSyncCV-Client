use serde::Deserialize;
// Default sync interval is 5 seconds
fn default_sync_interval() -> u64 {
    5
}
use std::fs;

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
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // Read config.toml from current working directory
        let cwd = std::env::current_dir()?;
        let config_file = cwd.join("config.toml");
        if !config_file.exists() {
            // Generate default config file
            let template = "# clipsync_rust_client configuration\nserver_url = \"http://example.com\"\ntoken = \"YOUR_TOKEN\"\n";
            fs::write(&config_file, template)?;
            return Err(anyhow::anyhow!(
                "Default config created at {:?}. Please update it and rerun.", config_file
            ));
        }
        let content = fs::read_to_string(&config_file)?;
        let cfg: Config = toml::from_str(&content)?;
        Ok(cfg)
    }
}