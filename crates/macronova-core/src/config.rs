use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// How `warp_mouse(x, y)` emits absolute position events on Linux/Wayland.
///
/// The kernel suppresses `EV_ABS` events whose value hasn't changed since the
/// last report.  `Jitter` works around this by emitting a one-pixel offset first
/// so every call produces a state change.  `Direct` relies on `INPUT_PROP_DIRECT`
/// (tablet/touchscreen semantics) which some compositors may treat differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WarpMode {
    /// Emit `(x, y+1)` then `(x, y)` — works around kernel dedup on all compositors.
    #[default]
    Jitter,
    /// Declare `INPUT_PROP_DIRECT` on the uinput device and emit a single event.
    Direct,
}

/// Top-level configuration loaded from `~/.config/macronova/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Configured input devices.
    #[serde(default)]
    pub devices: Vec<InputDeviceConfig>,

    /// How `warp_mouse` positions the cursor on Linux/Wayland.
    /// Defaults to `jitter` (most compatible).
    #[serde(default)]
    pub warp_mode: WarpMode,

    /// When `true`, Virtual Mode is active: the daemon can grab the input
    /// device exclusively and intercept individual button presses so they
    /// are consumed rather than forwarded to the OS.  All non-intercepted
    /// events (motion, scroll, and buttons without `intercept = true`) are
    /// transparently re-injected via uinput so the device still works normally.
    ///
    /// When `false` (default), the daemon never grabs the device and the
    /// `intercept` field on bindings has no effect — behaviour is identical
    /// to a version of MacroNova without this feature.
    #[serde(default)]
    pub virtual_mode: bool,
}

/// Configuration for a single device.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InputDeviceConfig {
    /// Stable user-visible identifier.
    pub id: String,

    /// Display label shown in the GUI.
    pub display_name: String,

    /// Path to the primary evdev mouse node.
    pub mouse_path: String,

    /// Optional companion keyboard/consumer evdev node.
    pub kbd_path: Option<String>,

    /// Button-to-macro bindings.
    #[serde(default)]
    pub bindings: Vec<ButtonBinding>,
}

/// A single button → macro binding.
///
/// `button` is the canonical evdev button name captured from a configured device,
/// e.g. `"my-mouse::usb-Logitech_USB_Receiver-event-mouse/key0x0113"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonBinding {
    /// Captured button name.
    pub button: Option<String>,

    /// Path to the Rhai script to run when the button is pressed.
    /// Relative paths are resolved from the config directory.
    pub on_press: Option<String>,

    /// Path to the Rhai script to run when the button is released.
    pub on_release: Option<String>,

    /// When `true` the button event is consumed by the daemon and never
    /// forwarded to the compositor / OS.  Only the bound macro runs.
    /// When `false` (default) the button behaves normally in addition to
    /// triggering the macro.
    #[serde(default)]
    pub intercept: bool,
}

impl Config {
    /// Load configuration from the given path.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let cfg: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(cfg)
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

/// Returns the platform-appropriate config base directory joined with `macronova/`.
///
/// | Platform | Path                                          |
/// |----------|-----------------------------------------------|
/// | Linux    | `$XDG_CONFIG_HOME/macronova` or `~/.config/macronova` |
/// | Windows  | `%APPDATA%\macronova`                         |
/// | macOS    | `~/Library/Application Support/macronova`     |
pub fn default_config_dir() -> PathBuf {
    platform_config_base()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("macronova")
}

/// Returns `<config_dir>/config.toml`.
pub fn default_config_path() -> PathBuf {
    default_config_dir().join("config.toml")
}

/// Returns `<config_dir>/macros/`.
pub fn default_macros_dir() -> PathBuf {
    default_config_dir().join("macros")
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn platform_config_base() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
}

#[cfg(target_os = "windows")]
fn platform_config_base() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn platform_config_base() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Application Support"))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "windows",
    target_os = "macos",
)))]
fn platform_config_base() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
