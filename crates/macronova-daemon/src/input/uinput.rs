/// Virtual uinput device for injecting keyboard, mouse button, and mouse motion events.
///
/// Uses BUS_USB so the kernel assigns it handlers that libinput and compositors
/// will actually route — BUS_VIRTUAL is silently ignored on Wayland.
///
/// Replaces the former EIS/RemoteDesktop portal path entirely.  `warp_mouse`
/// (absolute positioning) is not supported and is a no-op.
use anyhow::{Context, Result};
use evdev::{
    uinput::VirtualDevice, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    RelativeAxisCode, SynchronizationCode,
};
use tracing::info;

const EV_REL: u16 = 0x02;

/// A virtual input device that can inject key, mouse-button, and relative-motion events.
pub struct UInputDevice {
    device: VirtualDevice,
}

impl UInputDevice {
    /// Create the virtual input device.
    ///
    /// Registers:
    /// - `KEY_*` codes 1–255 (keyboard keys)
    /// - `BTN_*` codes 0x110–0x11f (mouse buttons: left, right, middle, side, extra…)
    /// - `EV_REL` axes: X, Y, WHEEL, HWHEEL (relative mouse motion and scroll)
    pub fn new() -> Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for code in 1..=255u16 {
            keys.insert(KeyCode(code));
        }
        for code in 0x110..=0x11fu16 {
            keys.insert(KeyCode(code));
        }

        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_X);
        axes.insert(RelativeAxisCode::REL_Y);
        axes.insert(RelativeAxisCode::REL_WHEEL);
        axes.insert(RelativeAxisCode::REL_HWHEEL);

        let id = InputId::new(BusType::BUS_USB, 0x0000, 0x0001, 0x0001);

        let device = VirtualDevice::builder()
            .context("Failed to create VirtualDeviceBuilder")?
            .name("macronova-input")
            .input_id(id)
            .with_keys(&keys)
            .context("Failed to set keys")?
            .with_relative_axes(&axes)
            .context("Failed to set relative axes")?
            .build()
            .context("Failed to build virtual device")?;

        info!("Created virtual input device 'macronova-input' (BUS_USB)");
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

    /// Press a mouse button (BTN_LEFT etc.) down.
    pub fn button_down(&mut self, code: u16) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, code, 1),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Release a mouse button.
    pub fn button_up(&mut self, code: u16) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, code, 0),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Click a mouse button (down + up).
    pub fn button_click(&mut self, code: u16) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EventType::KEY.0, code, 1),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
            InputEvent::new(EventType::KEY.0, code, 0),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Move the mouse cursor relatively by (dx, dy) pixels.
    pub fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EV_REL, RelativeAxisCode::REL_X.0, dx),
            InputEvent::new(EV_REL, RelativeAxisCode::REL_Y.0, dy),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Vertical scroll. Positive = down (one unit = one detent).
    pub fn scroll(&mut self, clicks: i32) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EV_REL, RelativeAxisCode::REL_WHEEL.0, -clicks),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    /// Horizontal scroll. Positive = right.
    pub fn hscroll(&mut self, clicks: i32) -> Result<()> {
        self.device.emit(&[
            InputEvent::new(EV_REL, RelativeAxisCode::REL_HWHEEL.0, clicks),
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

/// Resolve a human-friendly mouse button name → evdev BTN code.
///
/// Accepts: "left", "right", "middle", "side"/"back", "extra"/"forward",
/// or numeric evdev codes like "0x110".
pub fn btn_by_name(name: &str) -> Option<u16> {
    match name.to_uppercase().as_str() {
        "LEFT" | "BTN_LEFT" | "1" => Some(0x110),
        "RIGHT" | "BTN_RIGHT" | "3" => Some(0x111),
        "MIDDLE" | "BTN_MIDDLE" | "2" => Some(0x112),
        "SIDE" | "BTN_SIDE" | "BACK" | "8" => Some(0x113),
        "EXTRA" | "BTN_EXTRA" | "FORWARD" | "9" => Some(0x114),
        _ => None,
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
