pub mod evdev_input;
pub mod hidpp;
pub mod hidraw_input;
pub mod logitech;

use serde::{Deserialize, Serialize};

/// Information about a discovered HID device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Human-readable product name, if known.
    pub name: String,
    /// Logitech vendor ID is always 0x046D; other vendors may differ.
    pub vendor_id: u16,
    /// USB product ID.
    pub product_id: u16,
    /// Wireless product ID (4 hex), present for wireless devices.
    pub wpid: Option<u16>,
    /// HID++ protocol version (e.g. `(2, 0)`).
    pub hidpp_version: Option<(u8, u8)>,
    /// Path to the hidraw device node, e.g. `/dev/hidraw0`.
    pub hidraw_path: String,
    /// HID++ device index.
    /// 0xFF = device connected directly (wired or direct Bolt).
    /// 1..=6 = device slot on a Unifying/Bolt receiver.
    pub device_index: u8,
    /// Whether the device is currently reachable.
    pub connected: bool,
}

impl DeviceInfo {
    pub fn display_name(&self) -> String {
        if self.name.is_empty() {
            format!("{:04X}:{:04X}", self.vendor_id, self.product_id)
        } else {
            self.name.clone()
        }
    }
}
