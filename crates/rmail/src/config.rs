//! Rudimentary persisted settings.
//!
//! For now we only remember a few layout sizes (the main window and the two
//! resizable sidebar/list columns) so the app reopens the way the user left it.
//! Settings are stored as JSON at `~/.config/rMail/config.json` on every
//! platform — by request, we use this fixed path rather than each OS's native
//! config directory. Reads and writes are best-effort: any error (missing file,
//! bad JSON, no home directory) falls back to defaults and never crashes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Persisted layout settings. Sizes are in logical pixels.
///
/// `#[serde(default)]` lets older/partial config files load: any missing field
/// falls back to its default instead of failing the whole parse.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// X position (left edge) of the main window, in screen coordinates.
    pub window_x: f32,
    /// Y position (top edge) of the main window, in screen coordinates.
    pub window_y: f32,
    /// Width of the main window (the *restored* size, ignoring maximize).
    pub window_width: f32,
    /// Height of the main window (the *restored* size, ignoring maximize).
    pub window_height: f32,
    /// Whether the window was maximized (macOS: zoomed). When set, the app reopens
    /// maximized but restores to the saved position/size once moved.
    pub maximized: bool,
    /// The maximized frame (position + size). Saved so the window can open
    /// directly at this size when `maximized` is set, avoiding the
    /// restore-then-maximize flicker on macOS.
    pub max_x: f32,
    pub max_y: f32,
    pub max_width: f32,
    pub max_height: f32,
    /// Width of the accounts/folders sidebar.
    pub sidebar_width: f32,
    /// Width of the message list column.
    pub list_width: f32,
    /// Whether to load remote content (e.g. images fetched over `http(s)`) in
    /// e-mail bodies. Off by default: remote resources are a privacy leak
    /// (tracking pixels reveal when/where a message was opened), so the user opts
    /// in. Inline `data:` images are unaffected and always render.
    pub load_remote_images: bool,
}

impl Default for Config {
    fn default() -> Self {
        // Mirrors the app's built-in defaults (see `main.rs` and `root.rs`).
        Self {
            window_x: 0.0,
            window_y: 0.0,
            window_width: 1100.0,
            window_height: 720.0,
            maximized: false,
            max_x: 0.0,
            max_y: 0.0,
            max_width: 0.0,
            max_height: 0.0,
            sidebar_width: 200.0,
            list_width: 360.0,
            load_remote_images: false,
        }
    }
}

/// Best-effort home directory. We avoid an extra dependency: `HOME` covers
/// Linux/macOS and `USERPROFILE` covers Windows, which is all we need since the
/// rest of the path is fixed.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Absolute path to the config file (`~/.config/rMail/config.json`), or `None`
/// if the home directory can't be determined.
pub fn config_path() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".config").join("rMail").join("config.json"))
}

/// Loads the settings, returning defaults if the file is missing or unreadable.
pub fn load() -> Config {
    config_path().map(load_from).unwrap_or_default()
}

/// Persists the settings (best-effort; errors are ignored).
pub fn save(config: &Config) {
    if let Some(path) = config_path() {
        let _ = save_to(&path, config);
    }
}

/// Reads and parses a config file, falling back to defaults on any error.
fn load_from(path: PathBuf) -> Config {
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Writes the config as pretty JSON, creating the parent directory if needed.
fn save_to(path: &Path, config: &Config) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_uses_fixed_dot_config_location() {
        if let Some(path) = config_path() {
            assert!(path.ends_with("rMail/config.json"));
            assert!(path.to_string_lossy().contains(".config"));
        }
    }

    #[test]
    fn json_round_trips() {
        let config = Config {
            window_x: 120.0,
            window_y: 64.0,
            window_width: 1280.0,
            window_height: 800.0,
            maximized: true,
            max_x: 0.0,
            max_y: 25.0,
            max_width: 1512.0,
            max_height: 945.0,
            sidebar_width: 200.0,
            list_width: 420.0,
            load_remote_images: true,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config, parsed);
    }

    #[test]
    fn remote_images_are_blocked_by_default() {
        // Privacy: a fresh install (and any config predating the field) must not
        // load tracking pixels until the user opts in.
        assert!(!Config::default().load_remote_images);
        let parsed: Config = serde_json::from_str(r#"{ "sidebar_width": 175.0 }"#).unwrap();
        assert!(!parsed.load_remote_images);
    }

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        // Only one field present: the rest must come from `Default`.
        let parsed: Config = serde_json::from_str(r#"{ "sidebar_width": 175.0 }"#).unwrap();
        assert_eq!(parsed.sidebar_width, 175.0);
        assert_eq!(parsed.window_width, Config::default().window_width);
    }

    #[test]
    fn invalid_json_loads_defaults() {
        let path = unique_temp_path();
        std::fs::write(&path, b"not json at all").unwrap();
        assert_eq!(load_from(path.clone()), Config::default());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_then_load_round_trips_on_disk() {
        let path = unique_temp_path();
        let config = Config {
            window_x: -40.0,
            window_y: 30.0,
            window_width: 999.0,
            window_height: 555.0,
            maximized: false,
            max_x: 0.0,
            max_y: 0.0,
            max_width: 0.0,
            max_height: 0.0,
            sidebar_width: 160.0,
            list_width: 400.0,
            load_remote_images: false,
        };
        save_to(&path, &config).unwrap();
        assert_eq!(load_from(path.clone()), config);
        let _ = std::fs::remove_file(&path);
    }

    fn unique_temp_path() -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "rmail-config-test-{}-{nanos}.json",
            std::process::id()
        ))
    }
}
