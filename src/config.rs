use anyhow::{Context, Result};
use dirs::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILENAME: &str = "config.toml";
const DATA_DIR_NAME: &str = "tosser";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub piped_api: String,
    pub device_addr: String,
    pub device_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            piped_api: String::new(),
            device_addr: String::new(),
            device_port: 8009,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&raw).context("parsing config.toml")
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw)
            .with_context(|| format!("writing config {}", path.display()))
    }
}

pub fn data_path() -> Result<PathBuf> {
    let base = data_dir().context("could not determine XDG data dir")?;
    Ok(base.join(DATA_DIR_NAME))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(data_path()?.join(CONFIG_FILENAME))
}
