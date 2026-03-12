/// Virtual uinput keyboard for injecting synthetic key events.
///
/// Uses BUS_USB so the kernel assigns it the `kbd` handler, which XWayland reads
/// directly — bypassing libinput (which ignores all /devices/virtual/input/ devices).
/// Mouse injection is handled separately via XTest (see xtest.rs).
use anyhow::{Context, Result};
use evdev::{
    uinput::VirtualDevice, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    SynchronizationCode,
};
use tracing::info;

/// A virtual input device that can inject key and mouse events.
pub struct UInputDevice {
    device: VirtualDevice,
}

impl UInputDevice {
    /// Create the virtual keyboard device.
    ///
    /// Uses `BUS_USB` so that libinput's compositor instance (KWin, Mutter, etc.)
    /// actually picks up and routes events from this device.  `BUS_VIRTUAL`
    /// is silently ignored by libinput, making key injection invisible on Wayland.
    ///
    /// Only `KEY_*` codes 1–255 are registered (standard keyboard keys).
    /// `BTN_*` codes (0x110+) and `EV_REL` axes are absent so libinput classifies
    /// this purely as a keyboard and never as a pointer.
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 1..=255u16 {
            keys.insert(KeyCode(code));
        }

        // BUS_USB (0x03) is required: libinput ignores BUS_VIRTUAL (0x06) devices,
        // so they never reach KWin/Mutter and key injection silently does nothing
        // on Wayland.  Vendor 0x0000 / product 0x0001 are clearly synthetic.
        let id = InputId::new(BusType::BUS_USB, 0x0000, 0x0001, 0x0001);

        let device = VirtualDevice::builder()
            .context("Failed to create VirtualDeviceBuilder")?
            .name("macronova-kbd")
            .input_id(id)
            .with_keys(&keys)
            .context("Failed to set keys")?
            .build()
            .context("Failed to build virtual device")?;

        info!("Created virtual input device 'macronova-kbd' (BUS_USB)");
        Ok(Self { device })
    }

    /// Press a key (send KEY_DOWN + SYN).
    pub fn key_down(&mut self, key: KeyCode) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, key.0, 1),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Release a key (send KEY_UP + SYN).
    pub fn key_up(&mut self, key: KeyCode) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, key.0, 0),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Tap a key (down + up).
    pub fn tap_key(&mut self, key: KeyCode) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, key.0, 1),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
            InputEvent::new(EventType::KEY.0, key.0, 0),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Type a string by tapping individual key codes.
    /// Only supports basic ASCII; non-ASCII characters are silently skipped.
    pub fn type_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            if let Some((key, shift)) = char_to_key(ch) {
                if shift {
                    self.key_down(KeyCode::KEY_LEFTSHIFT)?;
                }
                self.tap_key(key)?;
                if shift {
                    self.key_up(KeyCode::KEY_LEFTSHIFT)?;
                }
            }
        }
        Ok(())
    }
}

/// Look up an evdev KeyCode by name (case-insensitive).
///
/// Accepts:
/// - `"BTN_LEFT"`, `"BTN_RIGHT"`, `"BTN_MIDDLE"`, etc. (mouse buttons, passed through as-is)
/// - `"KEY_A"`, `"KEY_LEFTCTRL"`, etc. (full evdev names, passed through as-is)
/// - `"a"`, `"ctrl"`, `"enter"` etc. (short aliases, auto-prefixed with KEY_)
pub fn key_by_name(name: &str) -> Option<KeyCode> {
    let upper = name.to_uppercase();
    let with_prefix = if upper.starts_with("KEY_") || upper.starts_with("BTN_") {
        // Already a full evdev name — use as-is
        upper.clone()
    } else {
        // Try common aliases first, then auto-prefix with KEY_
        match upper.as_str() {
            "CTRL" | "CONTROL" => return Some(KeyCode::KEY_LEFTCTRL),
            "ALT" => return Some(KeyCode::KEY_LEFTALT),
            "SHIFT" => return Some(KeyCode::KEY_LEFTSHIFT),
            "SUPER" | "WIN" | "META" => return Some(KeyCode::KEY_LEFTMETA),
            "ENTER" | "RETURN" => return Some(KeyCode::KEY_ENTER),
            "ESC" | "ESCAPE" => return Some(KeyCode::KEY_ESC),
            "SPACE" => return Some(KeyCode::KEY_SPACE),
            "TAB" => return Some(KeyCode::KEY_TAB),
            "BACKSPACE" => return Some(KeyCode::KEY_BACKSPACE),
            "DELETE" | "DEL" => return Some(KeyCode::KEY_DELETE),
            "UP" => return Some(KeyCode::KEY_UP),
            "DOWN" => return Some(KeyCode::KEY_DOWN),
            "LEFT" => return Some(KeyCode::KEY_LEFT),
            "RIGHT" => return Some(KeyCode::KEY_RIGHT),
            "HOME" => return Some(KeyCode::KEY_HOME),
            "END" => return Some(KeyCode::KEY_END),
            "PGUP" | "PAGE_UP" | "PAGEUP" => return Some(KeyCode::KEY_PAGEUP),
            "PGDN" | "PAGE_DOWN" | "PAGEDOWN" => return Some(KeyCode::KEY_PAGEDOWN),
            _ => format!("KEY_{}", upper),
        }
    };

    // Walk evdev's key table by code, matching display name
    for code in 0..767u16 {
        let key = KeyCode(code);
        if format!("{:?}", key) == with_prefix {
            return Some(key);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btn_left_resolves() {
        let key = key_by_name("BTN_LEFT").expect("BTN_LEFT should resolve");
        assert_eq!(key.0, 272, "BTN_LEFT should be code 272 (0x110)");
    }

    #[test]
    fn btn_right_resolves() {
        let key = key_by_name("BTN_RIGHT").expect("BTN_RIGHT should resolve");
        assert_eq!(key.0, 273);
    }

    #[test]
    fn key_aliases_resolve() {
        assert!(key_by_name("ctrl").is_some());
        assert!(key_by_name("KEY_LEFTCTRL").is_some());
        assert!(key_by_name("a").is_some());
        assert!(key_by_name("KEY_Z").is_some());
    }
}

/// Map a single ASCII character to an evdev KeyCode + whether Shift is needed.
fn char_to_key(ch: char) -> Option<(KeyCode, bool)> {
    match ch {
        'a'..='z' => {
            let code = KeyCode::KEY_A.0 + (ch as u16 - b'a' as u16);
            Some((KeyCode(code), false))
        }
        'A'..='Z' => {
            let code = KeyCode::KEY_A.0 + (ch as u16 - b'A' as u16);
            Some((KeyCode(code), true))
        }
        '0'..='9' => {
            let code = KeyCode::KEY_0.0 + (ch as u16 - b'0' as u16);
            Some((KeyCode(code), false))
        }
        ' ' => Some((KeyCode::KEY_SPACE, false)),
        '\n' => Some((KeyCode::KEY_ENTER, false)),
        '\t' => Some((KeyCode::KEY_TAB, false)),
        '!' => Some((KeyCode::KEY_1, true)),
        '@' => Some((KeyCode::KEY_2, true)),
        '#' => Some((KeyCode::KEY_3, true)),
        '$' => Some((KeyCode::KEY_4, true)),
        '%' => Some((KeyCode::KEY_5, true)),
        '^' => Some((KeyCode::KEY_6, true)),
        '&' => Some((KeyCode::KEY_7, true)),
        '*' => Some((KeyCode::KEY_8, true)),
        '(' => Some((KeyCode::KEY_9, true)),
        ')' => Some((KeyCode::KEY_0, true)),
        '-' => Some((KeyCode::KEY_MINUS, false)),
        '_' => Some((KeyCode::KEY_MINUS, true)),
        '=' => Some((KeyCode::KEY_EQUAL, false)),
        '+' => Some((KeyCode::KEY_EQUAL, true)),
        '[' => Some((KeyCode::KEY_LEFTBRACE, false)),
        '{' => Some((KeyCode::KEY_LEFTBRACE, true)),
        ']' => Some((KeyCode::KEY_RIGHTBRACE, false)),
        '}' => Some((KeyCode::KEY_RIGHTBRACE, true)),
        '\\' => Some((KeyCode::KEY_BACKSLASH, false)),
        '|' => Some((KeyCode::KEY_BACKSLASH, true)),
        ';' => Some((KeyCode::KEY_SEMICOLON, false)),
        ':' => Some((KeyCode::KEY_SEMICOLON, true)),
        '\'' => Some((KeyCode::KEY_APOSTROPHE, false)),
        '"' => Some((KeyCode::KEY_APOSTROPHE, true)),
        ',' => Some((KeyCode::KEY_COMMA, false)),
        '<' => Some((KeyCode::KEY_COMMA, true)),
        '.' => Some((KeyCode::KEY_DOT, false)),
        '>' => Some((KeyCode::KEY_DOT, true)),
        '/' => Some((KeyCode::KEY_SLASH, false)),
        '?' => Some((KeyCode::KEY_SLASH, true)),
        '`' => Some((KeyCode::KEY_GRAVE, false)),
        '~' => Some((KeyCode::KEY_GRAVE, true)),
        _ => None,
    }
}
