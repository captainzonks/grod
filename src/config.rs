use anyhow::{Context, Result};
use dirs::data_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILENAME: &str = "config.toml";
const DATA_DIR_NAME: &str = "grod";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Best,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "360p")]
    P360,
}

impl Quality {
    pub fn target_height(&self) -> u32 {
        match self {
            Quality::Best | Quality::P1080 => 1080,
            Quality::P720 => 720,
            Quality::P480 => 480,
            Quality::P360 => 360,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Quality::Best => "best",
            Quality::P1080 => "1080p",
            Quality::P720 => "720p",
            Quality::P480 => "480p",
            Quality::P360 => "360p",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "best" => Some(Quality::Best),
            "1080p" | "1080" => Some(Quality::P1080),
            "720p" | "720" => Some(Quality::P720),
            "480p" | "480" => Some(Quality::P480),
            "360p" | "360" => Some(Quality::P360),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub piped_api: String,
    pub device_addr: String,
    pub device_port: u16,
    /// Port for the local HTTP API server (used by mobile app). Default: 7878.
    #[serde(default = "default_api_port")]
    pub api_port: u16,
    /// Port for the local muxing stream server. Default: 7879.
    #[serde(default = "default_stream_port")]
    pub stream_port: u16,
    /// Optional PIN for API authentication. Empty string = no auth.
    #[serde(default)]
    pub api_pin: String,
    /// Default cast quality. Default: 1080p.
    #[serde(default = "default_quality")]
    pub default_quality: Quality,
}

fn default_api_port() -> u16 {
    7878
}

fn default_stream_port() -> u16 {
    7879
}

fn default_quality() -> Quality {
    Quality::P1080
}

impl Default for Config {
    fn default() -> Self {
        Self {
            piped_api: String::new(),
            device_addr: String::new(),
            device_port: 8009,
            api_port: 7878,
            stream_port: 7879,
            api_pin: String::new(),
            default_quality: Quality::P1080,
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
