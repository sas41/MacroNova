use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level configuration loaded from `~/.config/macronova/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Per-device configurations, keyed by a user-chosen device name (e.g. "G502X").
    #[serde(default)]
    pub device: HashMap<String, DeviceConfig>,
}

/// Configuration for a single device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeviceConfig {
    /// Wireless Product ID (4 hex digits, e.g. "407F" for G502 X Lightspeed).
    /// If absent, the device is matched by USB product ID only.
    pub wpid: Option<String>,

    /// USB product ID (4 hex digits, e.g. "C099" for G502 X wired).
    pub usb_pid: Option<String>,

    /// Button-to-macro bindings.
    #[serde(default)]
    pub bindings: Vec<ButtonBinding>,
}

/// A single button → macro binding.
///
/// `button` is a symbolic name matching [`crate::device::hidraw_input::ButtonId::name()`],
/// e.g. `"side_a"`, `"side_b"`, `"dpi_cycle"`, `"left"`, `"right"`, `"middle"`.
///
/// Legacy `cid`-keyed configs (used with REPROG_CONTROLS_V4 devices) are still
/// supported: if `button` is absent but `cid` is present the binding is silently
/// ignored on devices that use the hidraw input path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonBinding {
    /// Symbolic button name (preferred), e.g. `"side_a"`, `"dpi_cycle"`.
    pub button: Option<String>,

    /// Legacy Logitech Control ID (CID).  Used only on REPROG_CONTROLS_V4 devices.
    #[serde(default)]
    pub cid: u16,

    /// Path to the Rhai script to run when the button is pressed.
    /// Relative paths are resolved from the config directory.
    pub on_press: Option<String>,

    /// Path to the Rhai script to run when the button is released.
    pub on_release: Option<String>,
}

impl Config {
    /// Load configuration from the given path.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    /// Load configuration from the default location (`~/.config/macronova/config.toml`).
    /// Returns a default empty config if the file does not exist.
    pub fn load_default() -> Result<Self> {
        let path = default_config_path();
        if path.exists() {
            Self::load(&path)
        } else {
            Ok(Self::default())
        }
    }

    /// Save configuration to the given path.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }
        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))
    }

    /// Save to the default location.
    pub fn save_default(&self) -> Result<()> {
        self.save(&default_config_path())
    }

    /// Resolve a script path relative to the config directory.
    pub fn resolve_script_path(&self, script: &str) -> PathBuf {
        let config_dir = default_config_dir();
        config_dir.join(script)
    }
}

/// Returns `~/.config/macronova/`.
pub fn default_config_dir() -> PathBuf {
    dirs_next()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("macronova")
}

/// Returns `~/.config/macronova/config.toml`.
pub fn default_config_path() -> PathBuf {
    default_config_dir().join("config.toml")
}

/// Returns `~/.config/macronova/macros/`.
pub fn default_macros_dir() -> PathBuf {
    default_config_dir().join("macros")
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}
