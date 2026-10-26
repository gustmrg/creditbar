//! User preferences, stored as JSON in the app's config directory.
//! Launch at login is not stored here: the OS login item is the source of truth.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// DeepSeek's peak / off-peak section in the panel and the tray menu.
    pub shows_deepseek_pricing: bool,
    /// `HH:MM:SS` to the next DeepSeek switch next to the tray icon (macOS and Linux; Windows
    /// trays have no title). Only while the pricing section is shown.
    pub shows_countdown: bool,
    /// The whale swims (peak) or breathes (off-peak) in the pricing section's aquarium; off, it
    /// is drawn still.
    pub animates_aquarium: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shows_deepseek_pricing: true,
            shows_countdown: false,
            animates_aquarium: true,
        }
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join(FILE))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .and_then(|path| fs::read(path).ok())
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) {
    let Some(path) = path(app) else { return };
    let result = fs::create_dir_all(path.parent().unwrap())
        .and_then(|_| fs::write(&path, serde_json::to_vec_pretty(settings).unwrap()));
    if let Err(error) = result {
        eprintln!("couldn't save settings: {error}");
    }
}
