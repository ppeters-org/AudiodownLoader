use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persistent settings, stored as JSON in the platform config directory
/// (e.g. ~/.config/ytaudiodownloader/config.json on Linux).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub url: String,
    pub download_path: String,
    pub audio_format: String,
    pub audio_quality: String,
    pub max_tracks: u32,
    pub save_videos: bool,
    pub prefer_h264: bool,
    pub debug_mode: bool,
    pub extended_mode: bool,
    pub clipboard_monitoring: bool,
    pub autodownload_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url: String::new(),
            download_path: String::new(),
            audio_format: "mp3".into(),
            audio_quality: "192".into(),
            max_tracks: 0,
            save_videos: true,
            prefer_h264: true,
            debug_mode: false,
            extended_mode: true,
            clipboard_monitoring: false,
            autodownload_enabled: false,
        }
    }
}

impl Config {
    fn file() -> Option<PathBuf> {
        ProjectDirs::from("", "", "ytaudiodownloader").map(|d| d.config_dir().join("config.json"))
    }

    pub fn load() -> Self {
        Self::file()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::file() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(text) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, text);
            }
        }
    }
}