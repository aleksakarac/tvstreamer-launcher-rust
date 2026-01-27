//! Configuration management - loads JSON config with minimal allocations

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// App configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub name: String,
    pub command: String,
    pub icon: String,
    #[serde(default = "default_icon_type")]
    pub icon_type: String,
    #[serde(default = "default_true")]
    pub hide_launcher: bool,
}

fn default_icon_type() -> String {
    "glyph".into()
}

fn default_true() -> bool {
    true
}

/// Stats configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatsConfig {
    #[serde(default = "default_true")]
    pub show_cpu: bool,
    #[serde(default = "default_true")]
    pub show_ram: bool,
    #[serde(default = "default_true")]
    pub show_temp: bool,
    #[serde(default = "default_true")]
    pub show_disk: bool,
    #[serde(default = "default_true")]
    pub show_network: bool,
    #[serde(default = "default_true")]
    pub show_bluetooth: bool,
    #[serde(default = "default_update_interval")]
    pub update_interval_ms: u64,
}

fn default_update_interval() -> u64 {
    2000
}

impl Default for StatsConfig {
    fn default() -> Self {
        Self {
            show_cpu: true,
            show_ram: true,
            show_temp: true,
            show_disk: true,
            show_network: true,
            show_bluetooth: true,
            update_interval_ms: 2000,
        }
    }
}

/// Display configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_true")]
    pub fullscreen: bool,
    #[serde(default = "default_columns")]
    pub grid_columns: u32,
}

fn default_backend() -> String {
    "auto".into()
}

fn default_columns() -> u32 {
    4
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            backend: "auto".into(),
            fullscreen: true,
            grid_columns: 4,
        }
    }
}

/// Screensaver configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreensaverConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_true")]
    pub show_clock: bool,
    #[serde(default = "default_true")]
    pub show_now_playing: bool,
}

fn default_timeout() -> u64 {
    300
}

impl Default for ScreensaverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            timeout_seconds: 300,
            show_clock: true,
            show_now_playing: true,
        }
    }
}

/// Root configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub apps: Vec<AppConfig>,
    #[serde(default)]
    pub stats: StatsConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub screensaver: ScreensaverConfig,
    pub wallpaper: Option<String>,
}

fn default_version() -> u32 {
    1
}

fn default_theme() -> String {
    "arc_blueberry".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            theme: "arc_blueberry".into(),
            apps: default_apps(),
            stats: StatsConfig::default(),
            display: DisplayConfig::default(),
            screensaver: ScreensaverConfig::default(),
            wallpaper: None,
        }
    }
}

/// Default apps list
fn default_apps() -> Vec<AppConfig> {
    vec![
        AppConfig {
            name: "Kodi".into(),
            command: "kodi".into(),
            icon: "󰕼".into(), // tv icon
            icon_type: "glyph".into(),
            hide_launcher: true,
        },
        AppConfig {
            name: "Stremio".into(),
            command: "/home/aleksa/.local/bin/stremio".into(),
            icon: "󰐊".into(), // play icon
            icon_type: "glyph".into(),
            hide_launcher: true,
        },
        AppConfig {
            name: "IPTV".into(),
            command: "/home/aleksa/omarchy-iptv".into(),
            icon: "󰕧".into(), // video icon
            icon_type: "glyph".into(),
            hide_launcher: true,
        },
        AppConfig {
            name: "Tidal".into(),
            command: "tidal-hifi".into(),
            icon: "󰝚".into(), // music icon
            icon_type: "glyph".into(),
            hide_launcher: true,
        },
        AppConfig {
            name: "Bluetooth".into(),
            command: "blueman-manager".into(),
            icon: "󰂯".into(), // bluetooth icon
            icon_type: "glyph".into(),
            hide_launcher: true,
        },
    ]
}

impl Config {
    /// Get config file path
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tvstreamer")
            .join("config.json")
    }

    /// Load config from file, fallback to defaults
    pub fn load() -> Self {
        let path = Self::path();

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(config) => return config,
                    Err(e) => eprintln!("Failed to parse config: {e}"),
                },
                Err(e) => eprintln!("Failed to read config: {e}"),
            }
        }

        // Return defaults
        Self::default()
    }

    /// Save config to file
    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)
    }
}
