//! evdev-based input reader for Logitech G502 X Lightspeed.
//!
//! All buttons — including Sniper, the two buttons next to LMB, side buttons,
//! DPI buttons, and scroll tilts — are delivered by the kernel as EV_KEY events
//! on the mouse evdev node (e.g. `/dev/input/event5`).
//! The DPI cycle and consumer-page buttons arrive on the kbd evdev node
//! (e.g. `/dev/input/event6`) also as EV_KEY events.
//!
//! ## Non-interference guarantee
//!
//! Each `/dev/input/eventN` fd opened by any process receives an **independent
//! copy** of the kernel's event stream.  The daemon opens the device with
//! `O_RDONLY | O_NONBLOCK` via a raw libc call — no `EVIOCGRAB`, no evdev
//! sync-stream state machine, no implicit exclusive ownership.  The
//! compositor's fd (managed by logind / libinput) is completely unaffected.
//!
//! ## ButtonId encoding
//! Every button is identified by the **evdev node path** and **key code** it came from:
//!
//! ```text
//! event5/key0x0110   // BTN_LEFT  (left click)
//! event5/key0x0113   // BTN_SIDE  (back thumb button)
//! event5/key0x0117   // code 279  (sniper button)
//! event6/key0x00cd   // consumer DPI cycle (as EV_KEY on kbd node)
//! ```
//!
//! The string form is `"<node>/key0x<HHHH>"` where `<node>` is the basename of
//! the evdev path (e.g. `event5`) and `<HHHH>` is the 4-digit lowercase hex key code.
//! This is unambiguous, stable across reboots (via `/dev/input/by-id` symlinks),
//! and exactly what the GUI Capture feature records.

use std::os::fd::RawFd;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

// ──────────────────────────────────────────────────────────────────────────────
// Raw kernel input_event layout (matches <linux/input.h>)
// struct input_event { struct timeval time; __u16 type; __u16 code; __s32 value; }
// timeval is two longs (8 bytes each on 64-bit), so the struct is 24 bytes total.
// ──────────────────────────────────────────────────────────────────────────────
#[repr(C)]
struct RawInputEvent {
    /// Seconds (ignored)
    tv_sec: libc::c_long,
    /// Microseconds (ignored)
    tv_usec: libc::c_long,
    /// EV_KEY = 1, EV_SYN = 0, etc.
    ev_type: u16,
    /// Key code (e.g. 0x0110 = BTN_LEFT)
    code: u16,
    /// 1 = press, 0 = release, 2 = autorepeat
    value: i32,
}

const EV_KEY: u16 = 0x01;
const EV_SYN: u16 = 0x00;
const INPUT_EVENT_SIZE: usize = std::mem::size_of::<RawInputEvent>();

/// A button identified by its evdev node and key code.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ButtonId {
    /// Basename of the evdev path, e.g. `"event5"`.
    pub node: String,
    /// EV_KEY code (e.g. 0x0110 = BTN_LEFT).
    pub code: u16,
}

impl ButtonId {
    /// Canonical string form: `"event5/key0x0110"`.
    pub fn name(&self) -> String {
        format!("{}/key0x{:04x}", self.node, self.code)
    }

    /// Parse from the canonical string form.
    pub fn from_name(s: &str) -> Option<Self> {
        let (node, rest) = s.split_once('/')?;
        let hex = rest.strip_prefix("key0x")?;
        let code = u16::from_str_radix(hex, 16).ok()?;
        Some(ButtonId {
            node: node.to_string(),
            code,
        })
    }
}

/// A button press or release event.
#[derive(Debug, Clone)]
pub struct ButtonEvent {
    pub button: ButtonId,
    pub pressed: bool,
}

/// Per-device state: a raw O_RDONLY | O_NONBLOCK file descriptor.
struct DeviceHandle {
    node: String,
    fd: RawFd,
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

/// Reads button events from one or more evdev nodes without grabbing them.
///
/// Opens each path with `O_RDONLY | O_NONBLOCK` via a raw libc syscall.
/// No `EVIOCGRAB` is issued — the compositor's input pipeline is untouched.
pub struct EvdevReader {
    handles: Vec<DeviceHandle>,
    pending: Vec<ButtonEvent>,
}

impl EvdevReader {
    /// Open the given evdev devices for read-only, non-blocking access.
    ///
    /// Each entry is `(path, label)` where:
    /// - `path` is the `/dev/input/eventN` path to open.
    /// - `label` is the stable name stored in `ButtonId::node` (e.g. the
    ///   by-id symlink basename).  If empty, the `eventN` basename is used
    ///   as a fallback.
    pub fn open(devices: &[(&str, &str)]) -> Result<Self> {
        let mut handles = Vec::new();
        for &(path, label) in devices {
            if path.is_empty() {
                continue;
            }
            let c_path =
                std::ffi::CString::new(path).with_context(|| format!("Invalid path: {path}"))?;
            let fd = unsafe {
                libc::open(
                    c_path.as_ptr(),
                    libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC,
                )
            };
            if fd < 0 {
                let err = std::io::Error::last_os_error();
                bail!("Failed to open evdev device {path}: {err}");
            }
            // Use the supplied label if non-empty, otherwise fall back to the
            // eventN basename so nothing breaks when called without by-id info.
            let node = if !label.is_empty() {
                label.to_string()
            } else {
                std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path)
                    .to_string()
            };
            handles.push(DeviceHandle { node, fd });
        }
        if handles.is_empty() {
            bail!("No evdev paths provided");
        }
        Ok(Self {
            handles,
            pending: Vec::new(),
        })
    }

    /// Poll all devices for the next button event, blocking up to `timeout`.
    ///
    /// Returns `Ok(Some(event))`, `Ok(None)` on timeout, or `Err` on I/O failure.
    pub fn poll(&mut self, timeout: Duration) -> Result<Option<ButtonEvent>> {
        if let Some(ev) = self.pending.pop() {
            return Ok(Some(ev));
        }

        let deadline = Instant::now() + timeout;

        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }

            let mut got_any = false;
            for handle in &self.handles {
                loop {
                    let mut ev: RawInputEvent = unsafe { std::mem::zeroed() };
                    let n = unsafe {
                        libc::read(
                            handle.fd,
                            &mut ev as *mut RawInputEvent as *mut libc::c_void,
                            INPUT_EVENT_SIZE,
                        )
                    };

                    if n < 0 {
                        let err = std::io::Error::last_os_error();
                        if err.kind() == std::io::ErrorKind::WouldBlock {
                            break; // no more events on this fd right now
                        }
                        return Err(err).context(format!("evdev read error on {}", handle.node));
                    }

                    if n as usize != INPUT_EVENT_SIZE {
                        break; // partial read — shouldn't happen, skip
                    }

                    // Skip SYN events and anything that isn't EV_KEY
                    if ev.ev_type == EV_SYN || ev.ev_type != EV_KEY {
                        continue;
                    }

                    // Skip autorepeat (value == 2)
                    if ev.value == 2 {
                        continue;
                    }

                    self.pending.push(ButtonEvent {
                        button: ButtonId {
                            node: handle.node.clone(),
                            code: ev.code,
                        },
                        pressed: ev.value == 1,
                    });
                    got_any = true;
                }
            }

            if got_any {
                // Reverse so the first-pushed event is returned first (pending is a stack).
                self.pending.reverse();
                if let Some(ev) = self.pending.pop() {
                    return Ok(Some(ev));
                }
            }

            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(remaining.min(Duration::from_millis(2)));
        }
    }
}

/// Paths and stable labels discovered via `/dev/input/by-id/`.
///
/// `mouse_path` / `kbd_path` are canonical `/dev/input/eventN` strings used to
/// open the fds.  `mouse_label` / `kbd_label` are the by-id symlink basenames
/// (e.g. `"usb-Logitech_USB_Receiver-event-mouse"`) used as the stable node
/// name in [`ButtonId`] so that config button names survive replug and reboot.
#[derive(Debug, Clone, Default)]
pub struct EvdevPaths {
    pub mouse_path: String,
    pub kbd_path: String,
    /// Stable label derived from the by-id symlink name (used as `ButtonId::node`).
    pub mouse_label: String,
    /// Stable label derived from the by-id symlink name (used as `ButtonId::node`).
    /// Empty string when no kbd node was found.
    pub kbd_label: String,
}

/// Discover the evdev paths for the Logitech USB Receiver by scanning
/// `/dev/input/by-id/` for the well-known symlink names.
///
/// Returns an [`EvdevPaths`] containing both the canonical `/dev/input/eventN`
/// paths (for opening fds) and the stable by-id symlink basenames (for use as
/// `ButtonId` node labels in config files).
pub fn discover_evdev_paths() -> Option<EvdevPaths> {
    let by_id = std::path::Path::new("/dev/input/by-id");
    if !by_id.exists() {
        return None;
    }

    let entries = std::fs::read_dir(by_id).ok()?;
    let mut mouse: Option<(String, String)> = None; // (path, label)
    let mut kbd: Option<(String, String)> = None;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Match the Logitech USB Receiver symlinks.
        // The mouse interface has no "-if" infix; the kbd interface has "-if01".
        if name.contains("Logitech") && name.contains("USB_Receiver") {
            if name.ends_with("-event-mouse") && !name.contains("-if") {
                let target = std::fs::read_link(entry.path()).ok()?;
                let canon = by_id.join(&target);
                let canon = std::fs::canonicalize(&canon).unwrap_or_else(|_| canon.clone());
                mouse = Some((canon.to_string_lossy().to_string(), name));
            } else if name.ends_with("-event-kbd") {
                let target = std::fs::read_link(entry.path()).ok()?;
                let canon = by_id.join(&target);
                let canon = std::fs::canonicalize(&canon).unwrap_or_else(|_| canon.clone());
                kbd = Some((canon.to_string_lossy().to_string(), name));
            }
        }
    }

    match (mouse, kbd) {
        (Some((mp, ml)), Some((kp, kl))) => Some(EvdevPaths {
            mouse_path: mp,
            kbd_path: kp,
            mouse_label: ml,
            kbd_label: kl,
        }),
        (Some((mp, ml)), None) => Some(EvdevPaths {
            mouse_path: mp,
            kbd_path: String::new(),
            mouse_label: ml,
            kbd_label: String::new(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_id_roundtrip() {
        let id = ButtonId {
            node: "event5".into(),
            code: 0x0110,
        };
        assert_eq!(id.name(), "event5/key0x0110");
        assert_eq!(ButtonId::from_name("event5/key0x0110"), Some(id));
    }

    #[test]
    fn button_id_sniper() {
        let id = ButtonId {
            node: "event5".into(),
            code: 0x0117,
        };
        assert_eq!(id.name(), "event5/key0x0117");
        assert_eq!(ButtonId::from_name("event5/key0x0117"), Some(id));
    }

    #[test]
    fn button_id_from_name_invalid() {
        assert_eq!(ButtonId::from_name("event5/0x0110"), None);
        assert_eq!(ButtonId::from_name("event5"), None);
        assert_eq!(ButtonId::from_name(""), None);
    }
}
