/// Linux input injector — two uinput virtual devices.
///
/// Split into a keyboard device (KEY_* only) and a mouse device (BTN_* + EV_REL
/// only) so that libinput classifies each correctly. A combined device with both
/// KEY_* and EV_REL capabilities is treated as an ambiguous "keyboard+pointer"
/// profile and the compositor may ignore its relative motion events.
use anyhow::{Context, Result};
use evdev::{
    uinput::VirtualDevice, AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent,
    InputId, KeyCode, PropType, RelativeAxisCode, SynchronizationCode, UinputAbsSetup,
};
use tracing::{info, warn};

use macronova_core::config::WarpMode;
use macronova_core::platform::input::InputInjector;

/// Absolute axis range used for `warp`. The compositor maps [0, ABS_MAX] linearly
/// to the full logical desktop. Coords passed to `warp` are scaled accordingly.
const ABS_MAX: i32 = 32767;

const EV_REL: u16 = 0x02;

pub struct UInputInjector {
    kbd: VirtualDevice,
    mouse: VirtualDevice,
    /// Total logical desktop size, used to scale warp coords to [0, ABS_MAX].
    desktop_w: i32,
    desktop_h: i32,
    warp_mode: WarpMode,
}

impl UInputInjector {
    /// Create the injector.
    ///
    /// `desktop_w` and `desktop_h` are the total logical desktop dimensions
    /// (sum of all monitor widths × height). They are used to scale absolute
    /// warp coordinates into the `[0, ABS_MAX]` range the kernel expects.
    pub fn new(desktop_w: i32, desktop_h: i32, warp_mode: WarpMode) -> Result<Self> {
        // ── Keyboard device: KEY_* codes 1–255 only ──────────────────────────
        let mut kbd_keys = AttributeSet::<KeyCode>::new();
        for code in 1..=255u16 {
            kbd_keys.insert(KeyCode(code));
        }
        let kbd = VirtualDevice::builder()
            .context("Failed to create kbd VirtualDeviceBuilder")?
            .name("macronova-kbd")
            .input_id(InputId::new(BusType::BUS_USB, 0x0000, 0x0001, 0x0001))
            .with_keys(&kbd_keys)
            .context("Failed to set kbd keys")?
            .build()
            .context("Failed to build kbd virtual device")?;
        info!("Created virtual keyboard device 'macronova-kbd' (BUS_USB)");

        // ── Mouse device: BTN_* + EV_REL + EV_ABS ────────────────────────────
        // EV_REL is used for relative motion and scroll.
        // EV_ABS (ABS_X / ABS_Y) enables compositor-level absolute warping on
        // both X11 and Wayland without needing to query the current position.
        let mut mouse_btns = AttributeSet::<KeyCode>::new();
        for code in 0x110..=0x11fu16 {
            mouse_btns.insert(KeyCode(code));
        }
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_X);
        axes.insert(RelativeAxisCode::REL_Y);
        axes.insert(RelativeAxisCode::REL_WHEEL);
        axes.insert(RelativeAxisCode::REL_HWHEEL);
        // AbsInfo::new(value, minimum, maximum, fuzz, flat, resolution)
        let abs_info = AbsInfo::new(0, 0, ABS_MAX, 0, 0, 0);
        let mut mouse_builder = VirtualDevice::builder()
            .context("Failed to create mouse VirtualDeviceBuilder")?
            .name("macronova-mouse")
            .input_id(InputId::new(BusType::BUS_USB, 0x0000, 0x0002, 0x0001))
            .with_keys(&mouse_btns)
            .context("Failed to set mouse buttons")?
            .with_relative_axes(&axes)
            .context("Failed to set relative axes")?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_X, abs_info))
            .context("Failed to set ABS_X")?
            .with_absolute_axis(&UinputAbsSetup::new(AbsoluteAxisCode::ABS_Y, abs_info))
            .context("Failed to set ABS_Y")?;
        if warp_mode == WarpMode::Direct {
            let mut props = AttributeSet::<PropType>::new();
            props.insert(PropType::DIRECT);
            mouse_builder = mouse_builder
                .with_properties(&props)
                .context("Failed to set INPUT_PROP_DIRECT")?;
        }
        let mouse = mouse_builder
            .build()
            .context("Failed to build mouse virtual device")?;
        info!("Created virtual mouse device 'macronova-mouse' (BUS_USB) desktop={desktop_w}x{desktop_h} warp_mode={warp_mode:?}");

        Ok(Self {
            kbd,
            mouse,
            desktop_w,
            desktop_h,
            warp_mode,
        })
    }

    fn emit_key(&mut self, code: u16, value: i32) -> Result<()> {
        self.kbd.emit(&[
            InputEvent::new(EventType::KEY.0, code, value),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }
}

impl InputInjector for UInputInjector {
    fn key_down(&mut self, name: &str) -> Result<()> {
        match key_by_name(name) {
            Some(k) => self.emit_key(k.0, 1),
            None => {
                warn!("key_down: unknown key '{}'", name);
                Ok(())
            }
        }
    }

    fn key_up(&mut self, name: &str) -> Result<()> {
        match key_by_name(name) {
            Some(k) => self.emit_key(k.0, 0),
            None => {
                warn!("key_up: unknown key '{}'", name);
                Ok(())
            }
        }
    }

    fn tap_key(&mut self, name: &str) -> Result<()> {
        match key_by_name(name) {
            Some(k) => {
                self.kbd.emit(&[
                    InputEvent::new(EventType::KEY.0, k.0, 1),
                    InputEvent::new(
                        EventType::SYNCHRONIZATION.0,
                        SynchronizationCode::SYN_REPORT.0,
                        0,
                    ),
                    InputEvent::new(EventType::KEY.0, k.0, 0),
                    InputEvent::new(
                        EventType::SYNCHRONIZATION.0,
                        SynchronizationCode::SYN_REPORT.0,
                        0,
                    ),
                ])?;
                Ok(())
            }
            None => {
                warn!("tap_key: unknown key '{}'", name);
                Ok(())
            }
        }
    }

    fn type_text(&mut self, text: &str) -> Result<()> {
        for ch in text.chars() {
            if let Some((key, shift)) = char_to_key(ch) {
                if shift {
                    self.emit_key(KeyCode::KEY_LEFTSHIFT.0, 1)?;
                }
                self.kbd.emit(&[
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
                if shift {
                    self.emit_key(KeyCode::KEY_LEFTSHIFT.0, 0)?;
                }
            }
        }
        Ok(())
    }

    fn click(&mut self, button: &str) -> Result<()> {
        match btn_by_name(button) {
            Some(code) => {
                self.mouse.emit(&[
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
            None => {
                warn!("click: unknown button '{}'", button);
                Ok(())
            }
        }
    }

    fn button_down(&mut self, button: &str) -> Result<()> {
        match btn_by_name(button) {
            Some(code) => {
                self.mouse.emit(&[
                    InputEvent::new(EventType::KEY.0, code, 1),
                    InputEvent::new(
                        EventType::SYNCHRONIZATION.0,
                        SynchronizationCode::SYN_REPORT.0,
                        0,
                    ),
                ])?;
                Ok(())
            }
            None => {
                warn!("button_down: unknown button '{}'", button);
                Ok(())
            }
        }
    }

    fn button_up(&mut self, button: &str) -> Result<()> {
        match btn_by_name(button) {
            Some(code) => {
                self.mouse.emit(&[
                    InputEvent::new(EventType::KEY.0, code, 0),
                    InputEvent::new(
                        EventType::SYNCHRONIZATION.0,
                        SynchronizationCode::SYN_REPORT.0,
                        0,
                    ),
                ])?;
                Ok(())
            }
            None => {
                warn!("button_up: unknown button '{}'", button);
                Ok(())
            }
        }
    }

    fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        self.mouse.emit(&[
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

    fn warp(&mut self, x: i32, y: i32) -> Result<()> {
        // Scale logical coords to [0, ABS_MAX] and emit as EV_ABS events.
        // The compositor maps [0, ABS_MAX] linearly to the full desktop.
        let abs_x = if self.desktop_w > 0 {
            (x.clamp(0, self.desktop_w - 1) as i64 * ABS_MAX as i64
                / (self.desktop_w - 1).max(1) as i64) as i32
        } else {
            x.clamp(0, ABS_MAX)
        };
        let abs_y = if self.desktop_h > 0 {
            (y.clamp(0, self.desktop_h - 1) as i64 * ABS_MAX as i64
                / (self.desktop_h - 1).max(1) as i64) as i32
        } else {
            y.clamp(0, ABS_MAX)
        };

        if self.warp_mode == WarpMode::Jitter {
            // The kernel suppresses EV_ABS events whose value hasn't changed
            // since the last report.  Emit a one-pixel-offset position first
            // so that every call produces a state change, then emit the real
            // target position.  Both syncs happen within one frame, so the
            // compositor sees only the final position.
            let jitter_y = if abs_y < ABS_MAX {
                abs_y + 1
            } else {
                abs_y - 1
            };
            self.mouse.emit(&[
                InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, abs_x),
                InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, jitter_y),
                InputEvent::new(
                    EventType::SYNCHRONIZATION.0,
                    SynchronizationCode::SYN_REPORT.0,
                    0,
                ),
            ])?;
        }

        self.mouse.emit(&[
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, abs_x),
            InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, abs_y),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    fn scroll(&mut self, clicks: i32) -> Result<()> {
        self.mouse.emit(&[
            InputEvent::new(EV_REL, RelativeAxisCode::REL_WHEEL.0, -clicks),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }

    fn hscroll(&mut self, clicks: i32) -> Result<()> {
        self.mouse.emit(&[
            InputEvent::new(EV_REL, RelativeAxisCode::REL_HWHEEL.0, clicks),
            InputEvent::new(
                EventType::SYNCHRONIZATION.0,
                SynchronizationCode::SYN_REPORT.0,
                0,
            ),
        ])?;
        Ok(())
    }
}

// ── Key/button name resolution ────────────────────────────────────────────────

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

pub fn key_by_name(name: &str) -> Option<KeyCode> {
    let upper = name.to_uppercase();
    let with_prefix = if upper.starts_with("KEY_") || upper.starts_with("BTN_") {
        upper.clone()
    } else {
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
    for code in 0..767u16 {
        let key = KeyCode(code);
        if format!("{:?}", key) == with_prefix {
            return Some(key);
        }
    }
    None
}

fn char_to_key(ch: char) -> Option<(KeyCode, bool)> {
    match ch {
        'a'..='z' => Some((KeyCode(KeyCode::KEY_A.0 + ch as u16 - b'a' as u16), false)),
        'A'..='Z' => Some((KeyCode(KeyCode::KEY_A.0 + ch as u16 - b'A' as u16), true)),
        '0'..='9' => Some((KeyCode(KeyCode::KEY_0.0 + ch as u16 - b'0' as u16), false)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btn_left_resolves() {
        assert_eq!(btn_by_name("left"), Some(0x110));
        assert_eq!(btn_by_name("BTN_LEFT"), Some(0x110));
    }

    #[test]
    fn key_aliases_resolve() {
        assert!(key_by_name("ctrl").is_some());
        assert!(key_by_name("KEY_LEFTCTRL").is_some());
        assert!(key_by_name("a").is_some());
        assert!(key_by_name("KEY_Z").is_some());
    }
}
