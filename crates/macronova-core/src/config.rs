use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::device::evdev_input::discover_evdev_paths;

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
    /// Per-device configurations, keyed by a user-chosen device name (e.g. "G502X").
    #[serde(default)]
    pub device: HashMap<String, DeviceConfig>,

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

    /// When `true` the button event is consumed by the daemon and never
    /// forwarded to the compositor / OS.  Only the bound macro runs.
    /// When `false` (default) the button behaves normally in addition to
    /// triggering the macro.
    #[serde(default)]
    pub intercept: bool,
}

impl Config {
    /// Load configuration from the given path.
    ///
    /// Button names using the legacy `eventN/key0x...` format are automatically
    /// migrated to the stable by-id label format on load.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        cfg.migrate_event_node_names();
        Ok(cfg)
    }

    /// Rewrite any button names still using the legacy `eventN/key0x...` format
    /// to the stable by-id symlink label.
    ///
    /// This is a best-effort migration: if `discover_evdev_paths()` returns
    /// `None` (device not plugged in at load time) the names are left unchanged.
    fn migrate_event_node_names(&mut self) {
        let paths = match discover_evdev_paths() {
            Some(p) => p,
            None => return, // device absent — cannot migrate right now
        };

        // Build a table: eventN_basename -> stable label, for each discovered node.
        let mut node_map: HashMap<String, String> = HashMap::new();
        for (path, label) in [
            (paths.mouse_path.as_str(), paths.mouse_label.as_str()),
            (paths.kbd_path.as_str(), paths.kbd_label.as_str()),
        ] {
            if path.is_empty() || label.is_empty() {
                continue;
            }
            let basename = std::path::Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(path)
                .to_string();
            node_map.insert(basename, label.to_string());
        }

        if node_map.is_empty() {
            return;
        }

        for device in self.device.values_mut() {
            for binding in &mut device.bindings {
                if let Some(ref name) = binding.button {
                    // Match "eventN/key0x..." where "eventN" is a pure eventN basename.
                    if let Some((node, rest)) = name.split_once('/') {
                        let is_legacy = node.starts_with("event")
                            && node[5..].chars().all(|c| c.is_ascii_digit());
                        if is_legacy {
                            if let Some(label) = node_map.get(node) {
                                binding.button = Some(format!("{label}/{rest}"));
                            }
                        }
                    }
                }
            }
        }
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
