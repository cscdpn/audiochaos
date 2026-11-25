//! What the standalone program remembers between runs, in a JSON file in
//! the user's configuration directory. The plugin saves the same with the
//! host's project instead.
//!
//! eframe has its own persistence, but turning it on turns on egui's for
//! the whole workspace, and the plugin's egui integration does not build
//! with it.

use std::path::{Path, PathBuf};
use std::{env, fs, io};

use serde::{Deserialize, Serialize};

use super::Theme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
}

impl Settings {
    /// The settings saved at `path`, or the defaults if there are none or
    /// they do not read: nothing there is worth refusing to start for.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).expect("settings serialize");
        fs::write(path, json + "\n")
    }

    /// Where the settings live: `audiochaos/settings.json` in the
    /// configuration directory, if the environment says where that is.
    pub fn path() -> Option<PathBuf> {
        let dir = if cfg!(windows) {
            PathBuf::from(env::var_os("APPDATA")?)
        } else if cfg!(target_os = "macos") {
            PathBuf::from(env::var_os("HOME")?).join("Library/Application Support")
        } else {
            match env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
                Some(dir) if dir.is_absolute() => dir,
                _ => PathBuf::from(env::var_os("HOME")?).join(".config"),
            }
        };
        Some(dir.join("audiochaos").join("settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        env::temp_dir()
            .join(format!("audiochaos-settings-{}", std::process::id()))
            .join(name)
    }

    #[test]
    fn saved_settings_load_back() {
        let path = scratch("saved").join("settings.json");
        let settings = Settings {
            theme: Theme::Chaos,
        };
        settings.save(&path).unwrap();
        assert_eq!(Settings::load(&path), settings);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_or_broken_settings_fall_back_to_the_defaults() {
        assert_eq!(
            Settings::load(&scratch("nowhere.json")),
            Settings::default()
        );
        let path = scratch("broken.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::write(&path, r#"{"theme": "neon"}"#).unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_file(&path).unwrap();
    }
}
