use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_state_on_start")]
    pub state_on_start: bool,
    
    #[serde(default = "default_disable_on_lock")]
    pub disable_on_lock: bool,
    
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_state_on_start() -> bool {
    false
}

fn default_disable_on_lock() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            state_on_start: default_state_on_start(),
            disable_on_lock: default_disable_on_lock(),
            log_level: default_log_level(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path();
        
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            let config: Config = toml::from_str(&content)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path();
        
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
    }
    
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("logind-idle-control")
            .join("config.toml")
    }
}
