//! Direct hidraw HID report reading for Logitech G502 X Lightspeed.
//!
//! The G502 X Lightspeed does NOT support REPROG_CONTROLS_V4 button diversion.
//! Instead, button events are delivered as standard HID reports on two interfaces,
//! confirmed from the HID report descriptors:
//!
//! Note: this module is for low-level hidraw research/debugging. The daemon input
//! runtime uses evdev paths configured by the user.
//!
//! ## hidraw16 — interface 0 (Mouse, Usage Page 0x0001)
//! No report ID prefix. Total ~13 bytes.
//! - Bytes 0-1: **16-bit bitmask** (LE) — one bit per button, buttons 1-16.
//!   Descriptor: UsagePage=Button, UsageMin=1, UsageMax=16, ReportCount=16, ReportSize=1.
//! - Bytes 2-5: X (i16 LE) + Y (i16 LE) relative axes.
//! - Byte  6:   Wheel (i8, relative).
//! - Byte  7:   Horizontal scroll (i8, Consumer usage 0x0238).
//! - Bytes 8-12: Vendor-defined constant bytes.
//!
//! ## hidraw17 — interface 1 (Keyboard + Consumer + SystemControl, Usage Page 0x0001)
//! All reports are prefixed with a report ID byte.
//!
//! ### Report 0x01 — Keyboard (14 bytes total incl. report ID)
//! - Byte 0: report ID = 0x01
//! - Byte 1: **modifier bitmask** — bits 0-7 = LCtrl LShift LAlt LMeta RCtrl RShift RAlt RMeta
//! - Bytes 2-14: **112-bit keycode array** — one bit per HID keycode 0x04–0x73.
//!   Bit N set means keycode (0x04 + N) is pressed.
//!
//! ### Report 0x03 — Consumer Control (5 bytes total incl. report ID)
//! - Byte 0: report ID = 0x03
//! - Bytes 1-2: consumer usage code slot 1 (u16 LE; 0 = not pressed)
//! - Bytes 3-4: consumer usage code slot 2 (u16 LE; 0 = not pressed)
//!   Descriptor: Variable, not Array — each slot holds one active usage code.
//!   Examples: 0x00CD = DPI Cycle, 0x00B5 = DPI+, 0x00B6 = DPI−.
//!
//! ### Report 0x04 — System Control (2 bytes total incl. report ID)
//! - Byte 0: report ID = 0x04
//! - Byte 1 bits 0-1: 2-bit field (1=PowerDown, 2=Sleep, 3=WakeUp)
//! - Byte 1 bit 2: display-toggle bit (usage 0x65)
//!
//! ## ButtonId encoding
//! Every button is identified by the **raw HID report address** it came from:
//!
//! | Variant | String form | Example |
//! |---|---|---|
//! | `Bitmask { node, bit }` | `"hidrawN/bitB"` | `"hidraw16/bit0"` = left click |
//! | `Modifier { node, bit }` | `"hidrawN/r01/mod/bitB"` | `"hidraw17/r01/mod/bit0"` = LCtrl |
//! | `Keycode { node, code }` | `"hidrawN/r01/key/CC"` | `"hidraw17/r01/key/04"` = A |
//! | `Consumer { node, slot, usage }` | `"hidrawN/r03/slotS/UUUU"` | `"hidraw17/r03/slot0/00cd"` = DPI Cycle |
//! | `SysCtrl { node, bit }` | `"hidrawN/r04/sysbitB"` | `"hidraw17/r04/sysbit0"` = PowerDown |

use std::fs::OpenOptions;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

/// A button identified by its raw HID report address.
///
/// Intentionally free of symbolic meaning — every button shows exactly where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonId {
    /// A single bit in the hidraw mouse-interface bitmask report (no report ID).
    /// `node` = hidraw node number (e.g. 16), `bit` = 0-based bit index (0=button1).
    Bitmask { node: u32, bit: u8 },

    /// A modifier key bit from hidraw17 report 0x01, byte 1.
    /// `bit` 0-7 = LCtrl, LShift, LAlt, LMeta, RCtrl, RShift, RAlt, RMeta.
    Modifier { node: u32, bit: u8 },

    /// A key from hidraw17 report 0x01 keycode bitmap (bytes 2+).
    /// `code` = HID keycode (0x04–0x73), matching USB HID usage page 0x07.
    Keycode { node: u32, code: u8 },

    /// A consumer control usage from hidraw17 report 0x03.
    /// `slot` = 0 or 1 (two independent 16-bit slots in the report).
    /// `usage` = 16-bit HID consumer usage code (e.g. 0x00CD = DPI Cycle).
    Consumer { node: u32, slot: u8, usage: u16 },

    /// A system-control bit from hidraw17 report 0x04.
    /// `bit` 0 = PowerDown, 1 = Sleep, 2 = DisplayToggle.
    SysCtrl { node: u32, bit: u8 },
}

impl ButtonId {
    /// Canonical string form used in config files and the GUI.
    pub fn name(self) -> String {
        match self {
            ButtonId::Bitmask { node, bit } => format!("hidraw{}/bit{}", node, bit),
            ButtonId::Modifier { node, bit } => format!("hidraw{}/r01/mod/bit{}", node, bit),
            ButtonId::Keycode { node, code } => format!("hidraw{}/r01/key/{:02x}", node, code),
            ButtonId::Consumer { node, slot, usage } => {
                format!("hidraw{}/r03/slot{}/{:04x}", node, slot, usage)
            }
            ButtonId::SysCtrl { node, bit } => format!("hidraw{}/r04/sysbit{}", node, bit),
        }
    }

    /// Parse from the canonical string form produced by `name()`.
    pub fn from_name(s: &str) -> Option<Self> {
        let rest = s.strip_prefix("hidraw")?;
        let (node_str, path) = rest.split_once('/')?;
        let node: u32 = node_str.parse().ok()?;

        // hidrawN/bitB  — mouse bitmask
        if let Some(bit_str) = path.strip_prefix("bit") {
            let bit: u8 = bit_str.parse().ok()?;
            return Some(ButtonId::Bitmask { node, bit });
        }

        // hidrawN/r01/...
        if let Some(r01_path) = path.strip_prefix("r01/") {
            // hidrawN/r01/mod/bitB
            if let Some(bit_str) = r01_path.strip_prefix("mod/bit") {
                let bit: u8 = bit_str.parse().ok()?;
                return Some(ButtonId::Modifier { node, bit });
            }
            // hidrawN/r01/key/CC
            if let Some(code_str) = r01_path.strip_prefix("key/") {
                let code = u8::from_str_radix(code_str, 16).ok()?;
                return Some(ButtonId::Keycode { node, code });
            }
        }

        // hidrawN/r03/slotS/UUUU
        if let Some(r03_path) = path.strip_prefix("r03/slot") {
            let (slot_str, usage_str) = r03_path.split_once('/')?;
            let slot: u8 = slot_str.parse().ok()?;
            let usage = u16::from_str_radix(usage_str, 16).ok()?;
            return Some(ButtonId::Consumer { node, slot, usage });
        }

        // hidrawN/r04/sysbitB
        if let Some(bit_str) = path.strip_prefix("r04/sysbit") {
            let bit: u8 = bit_str.parse().ok()?;
            return Some(ButtonId::SysCtrl { node, bit });
        }

        None
    }
}

/// A button press or release event.
#[derive(Debug, Clone, Copy)]
pub struct ButtonEvent {
    pub button: ButtonId,
    pub pressed: bool,
}

/// Reads button events from the two hidraw interfaces of a Logitech G502 X Lightspeed.
///
/// Both files are opened in non-blocking mode and polled on each call to `poll()`.
pub struct HidrawReader {
    mouse: std::fs::File,
    mouse_node: u32,
    kbd: std::fs::File,
    kbd_node: u32,

    // State for change detection (we emit events only on transitions).
    last_mouse_buttons: u16,
    last_modifiers: u8,
    last_keycodes: [u8; 14], // 112 bits = 14 bytes
    last_consumer: [u16; 2],
    last_sysctrl: u8,

    pending: Vec<ButtonEvent>,
}

impl HidrawReader {
    /// Open the given hidraw paths in non-blocking read mode.
    pub fn open(mouse_path: &str, kbd_path: &str) -> Result<Self> {
        let mouse_node = node_number(mouse_path).unwrap_or(16);
        let kbd_node = node_number(kbd_path).unwrap_or(17);

        let mouse = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(mouse_path)
            .with_context(|| format!("Failed to open mouse hidraw: {mouse_path}"))?;

        let kbd = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(kbd_path)
            .with_context(|| format!("Failed to open kbd hidraw: {kbd_path}"))?;

        Ok(Self {
            mouse,
            mouse_node,
            kbd,
            kbd_node,
            last_mouse_buttons: 0,
            last_modifiers: 0,
            last_keycodes: [0u8; 14],
            last_consumer: [0u16; 2],
            last_sysctrl: 0,
            pending: Vec::new(),
        })
    }

    /// Poll both hidraw nodes for new events, blocking for up to `timeout`.
    ///
    /// Returns `Ok(Some(event))` for the next available event,
    /// `Ok(None)` on timeout, or `Err` on I/O failure.
    pub fn poll(&mut self, timeout: Duration) -> Result<Option<ButtonEvent>> {
        if let Some(ev) = self.pending.pop() {
            return Ok(Some(ev));
        }

        let deadline = Instant::now() + timeout;
        let mut mouse_buf = [0u8; 16];
        let mut kbd_buf = [0u8; 32];

        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }

            // --- hidraw16: mouse interface (no report ID) ---
            match self.mouse.read(&mut mouse_buf) {
                Ok(n) if n >= 2 => {
                    let buttons = u16::from_le_bytes([mouse_buf[0], mouse_buf[1]]);
                    let changed = buttons ^ self.last_mouse_buttons;
                    if changed != 0 {
                        let events =
                            Self::diff_bitmask(self.mouse_node, changed, buttons, "Bitmask");
                        self.last_mouse_buttons = buttons;
                        if !events.is_empty() {
                            self.queue_all_but_first(events);
                            if let Some(ev) = self.pending.pop() {
                                return Ok(Some(ev));
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e).context("Error reading mouse hidraw"),
            }

            // --- hidraw17: kbd/consumer/sysctrl (report ID prefixed) ---
            match self.kbd.read(&mut kbd_buf) {
                Ok(n) if n >= 2 => {
                    if let Some(first) = self.decode_kbd_report(&kbd_buf[..n]) {
                        return Ok(Some(first));
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e).context("Error reading kbd hidraw"),
            }

            // Check pending queue (may have been filled by decode_kbd_report).
            if let Some(ev) = self.pending.pop() {
                return Ok(Some(ev));
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(remaining.min(Duration::from_millis(1)));
        }
    }

    // ---- private helpers ----

    /// Decode a hidraw17 report and push events into `self.pending`.
    /// Returns the first event immediately (or None if nothing changed).
    fn decode_kbd_report(&mut self, data: &[u8]) -> Option<ButtonEvent> {
        let report_id = data[0];
        match report_id {
            0x01 => self.decode_r01_keyboard(data),
            0x03 => self.decode_r03_consumer(data),
            0x04 => self.decode_r04_sysctrl(data),
            _ => None,
        }
    }

    /// Report 0x01 — Keyboard.
    /// Byte 1: modifier bits. Bytes 2+: 112-bit keycode bitmap.
    fn decode_r01_keyboard(&mut self, data: &[u8]) -> Option<ButtonEvent> {
        if data.len() < 2 {
            return None;
        }
        let node = self.kbd_node;
        let modifiers = data[1];
        let mod_changed = modifiers ^ self.last_modifiers;

        // Keycode bytes: up to 14 bytes from byte 2 onward (112 bits).
        let kc_len = (data.len() - 2).min(14);
        let mut new_kc = [0u8; 14];
        new_kc[..kc_len].copy_from_slice(&data[2..2 + kc_len]);

        let mut events: Vec<ButtonEvent> = Vec::new();

        // Modifier bits.
        for bit in 0u8..8 {
            if mod_changed & (1 << bit) != 0 {
                let pressed = modifiers & (1 << bit) != 0;
                events.push(ButtonEvent {
                    button: ButtonId::Modifier { node, bit },
                    pressed,
                });
            }
        }

        // Keycode bitmap: each bit maps to keycode = 0x04 + (byte_index*8 + bit_index).
        for byte_idx in 0..14usize {
            let changed = new_kc[byte_idx] ^ self.last_keycodes[byte_idx];
            for bit in 0u8..8 {
                if changed & (1 << bit) != 0 {
                    let code = 0x04u8.wrapping_add((byte_idx as u8) * 8 + bit);
                    let pressed = new_kc[byte_idx] & (1 << bit) != 0;
                    events.push(ButtonEvent {
                        button: ButtonId::Keycode { node, code },
                        pressed,
                    });
                }
            }
        }

        self.last_modifiers = modifiers;
        self.last_keycodes = new_kc;

        if events.is_empty() {
            return None;
        }
        self.queue_all_but_first(events);
        self.pending.pop()
    }

    /// Report 0x03 — Consumer Control.
    /// Two 16-bit variable fields (slots 0 and 1); 0 = not pressed.
    fn decode_r03_consumer(&mut self, data: &[u8]) -> Option<ButtonEvent> {
        if data.len() < 5 {
            return None;
        }
        let node = self.kbd_node;
        let new_usage = [
            u16::from_le_bytes([data[1], data[2]]),
            u16::from_le_bytes([data[3], data[4]]),
        ];
        let mut events: Vec<ButtonEvent> = Vec::new();

        for slot in 0..2usize {
            let old = self.last_consumer[slot];
            let new = new_usage[slot];
            if old != new {
                // Release old usage if it was pressed.
                if old != 0 {
                    events.push(ButtonEvent {
                        button: ButtonId::Consumer {
                            node,
                            slot: slot as u8,
                            usage: old,
                        },
                        pressed: false,
                    });
                }
                // Press new usage if non-zero.
                if new != 0 {
                    events.push(ButtonEvent {
                        button: ButtonId::Consumer {
                            node,
                            slot: slot as u8,
                            usage: new,
                        },
                        pressed: true,
                    });
                }
            }
        }

        self.last_consumer = new_usage;

        if events.is_empty() {
            return None;
        }
        self.queue_all_but_first(events);
        self.pending.pop()
    }

    /// Report 0x04 — System Control.
    /// Byte 1 bits 0-2: PowerDown(0), Sleep(1), DisplayToggle(2).
    fn decode_r04_sysctrl(&mut self, data: &[u8]) -> Option<ButtonEvent> {
        if data.len() < 2 {
            return None;
        }
        let node = self.kbd_node;
        let byte = data[1];
        let changed = byte ^ self.last_sysctrl;
        let mut events: Vec<ButtonEvent> = Vec::new();

        for bit in 0u8..3 {
            if changed & (1 << bit) != 0 {
                let pressed = byte & (1 << bit) != 0;
                events.push(ButtonEvent {
                    button: ButtonId::SysCtrl { node, bit },
                    pressed,
                });
            }
        }

        self.last_sysctrl = byte;

        if events.is_empty() {
            return None;
        }
        self.queue_all_but_first(events);
        self.pending.pop()
    }

    /// Given a bitmask change, produce press/release events for every changed bit.
    fn diff_bitmask(node: u32, changed: u16, new_state: u16, _kind: &str) -> Vec<ButtonEvent> {
        let mut events = Vec::new();
        for bit in 0..16u8 {
            if changed & (1 << bit) != 0 {
                events.push(ButtonEvent {
                    button: ButtonId::Bitmask { node, bit },
                    pressed: new_state & (1 << bit) != 0,
                });
            }
        }
        events
    }

    /// Push all events into `self.pending` in reverse order so that `pop()` returns
    /// them in the original order (FIFO via reversed stack).
    fn queue_all_but_first(&mut self, mut events: Vec<ButtonEvent>) {
        // Reverse so the first event ends up at the top of the stack after we push all.
        events.reverse();
        self.pending.extend(events);
    }
}

/// Extract the numeric suffix from a hidraw path, e.g. `/dev/hidraw17` → `17`.
fn node_number(path: &str) -> Option<u32> {
    path.strip_prefix("/dev/hidraw")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ButtonId name roundtrip tests ----

    #[test]
    fn bitmask_roundtrip() {
        let id = ButtonId::Bitmask { node: 16, bit: 3 };
        assert_eq!(id.name(), "hidraw16/bit3");
        assert_eq!(ButtonId::from_name("hidraw16/bit3"), Some(id));
    }

    #[test]
    fn modifier_roundtrip() {
        let id = ButtonId::Modifier { node: 17, bit: 0 };
        assert_eq!(id.name(), "hidraw17/r01/mod/bit0");
        assert_eq!(ButtonId::from_name("hidraw17/r01/mod/bit0"), Some(id));
    }

    #[test]
    fn keycode_roundtrip() {
        let id = ButtonId::Keycode {
            node: 17,
            code: 0x04,
        };
        assert_eq!(id.name(), "hidraw17/r01/key/04");
        assert_eq!(ButtonId::from_name("hidraw17/r01/key/04"), Some(id));
    }

    #[test]
    fn consumer_roundtrip() {
        let id = ButtonId::Consumer {
            node: 17,
            slot: 0,
            usage: 0x00CD,
        };
        assert_eq!(id.name(), "hidraw17/r03/slot0/00cd");
        assert_eq!(ButtonId::from_name("hidraw17/r03/slot0/00cd"), Some(id));
    }

    #[test]
    fn sysctrl_roundtrip() {
        let id = ButtonId::SysCtrl { node: 17, bit: 1 };
        assert_eq!(id.name(), "hidraw17/r04/sysbit1");
        assert_eq!(ButtonId::from_name("hidraw17/r04/sysbit1"), Some(id));
    }

    // ---- Decoding tests (via HidrawReader methods) ----

    fn make_reader() -> HidrawReader {
        // Open /dev/null as a harmless stand-in for the file handles.
        // The poll() method is never called in these unit tests — we call the
        // decode_* methods directly — so the file contents don't matter.
        use std::fs::OpenOptions;
        let null = || OpenOptions::new().read(true).open("/dev/null").unwrap();
        HidrawReader {
            mouse: null(),
            mouse_node: 16,
            kbd: null(),
            kbd_node: 17,
            last_mouse_buttons: 0,
            last_modifiers: 0,
            last_keycodes: [0; 14],
            last_consumer: [0; 2],
            last_sysctrl: 0,
            pending: Vec::new(),
        }
    }

    #[test]
    fn decode_consumer_press_release() {
        let mut r = make_reader();
        // Press: slot0 = 0x00CD, slot1 = 0x0000
        let press = [0x03u8, 0xCD, 0x00, 0x00, 0x00];
        let ev = r.decode_r03_consumer(&press).unwrap();
        assert!(ev.pressed);
        assert_eq!(
            ev.button,
            ButtonId::Consumer {
                node: 17,
                slot: 0,
                usage: 0x00CD
            }
        );

        // Release: both slots zero
        let release = [0x03u8, 0x00, 0x00, 0x00, 0x00];
        let ev = r.decode_r03_consumer(&release).unwrap();
        assert!(!ev.pressed);
        assert_eq!(
            ev.button,
            ButtonId::Consumer {
                node: 17,
                slot: 0,
                usage: 0x00CD
            }
        );
    }

    #[test]
    fn decode_keyboard_modifier_press() {
        let mut r = make_reader();
        // LCtrl pressed (bit 0 of modifier byte)
        let mut report = [0u8; 16];
        report[0] = 0x01;
        report[1] = 0x01; // LCtrl
        let ev = r.decode_r01_keyboard(&report).unwrap();
        assert!(ev.pressed);
        assert_eq!(ev.button, ButtonId::Modifier { node: 17, bit: 0 });
    }

    #[test]
    fn decode_keyboard_keycode_press() {
        let mut r = make_reader();
        // Key 0x04 ('A') pressed: byte 2, bit 0
        let mut report = [0u8; 16];
        report[0] = 0x01;
        report[1] = 0x00; // no modifiers
        report[2] = 0x01; // bit 0 of keycode byte 0 = keycode 0x04
        let ev = r.decode_r01_keyboard(&report).unwrap();
        assert!(ev.pressed);
        assert_eq!(
            ev.button,
            ButtonId::Keycode {
                node: 17,
                code: 0x04
            }
        );
    }

    #[test]
    fn decode_sysctrl_sleep() {
        let mut r = make_reader();
        // bit 1 = Sleep
        let report = [0x04u8, 0x02];
        let ev = r.decode_r04_sysctrl(&report).unwrap();
        assert!(ev.pressed);
        assert_eq!(ev.button, ButtonId::SysCtrl { node: 17, bit: 1 });
    }

    #[test]
    fn bitmask_all_16_bits() {
        let events = HidrawReader::diff_bitmask(16, 0xFFFF, 0xFFFF, "");
        assert_eq!(events.len(), 16);
        assert!(events.iter().all(|e| e.pressed));
    }
}
