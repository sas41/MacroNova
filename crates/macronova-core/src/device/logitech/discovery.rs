/// Linux hidraw-based device discovery for Logitech HID++ devices.
///
/// A Logitech USB receiver exposes three hidraw nodes:
///   interface 0 — generic mouse/keyboard (usage_page 0x0001)
///   interface 1 — keyboard / consumer (usage_page 0x0001 / 0x000C)
///   interface 2 — HID++ vendor channel (usage_page 0xFF00)
///
/// We must use the 0xFF00 interface for all HID++ communication.
/// Wireless devices connected through a receiver use device_index 0x01–0x06.
/// Devices connected directly (wired or direct-mode Bolt) use device_index 0xFF.
use std::collections::HashSet;

use anyhow::{Context, Result};
use hidapi::{HidApi, HidDevice};
use tracing::debug;

use crate::device::hidpp::LOGITECH_VENDOR_ID;
use crate::device::DeviceInfo;

/// Usage page for the HID++ vendor-defined interface.
const HIDPP_USAGE_PAGE: u16 = 0xFF00;

/// Scan all hidraw nodes and return one entry per HID++ channel found.
///
/// For a Unifying/Bolt receiver the single hidraw node with usage_page 0xFF00
/// represents the receiver itself; devices are addressed by device_index 1..=6.
/// We return one DeviceInfo per unique 0xFF00 hidraw path, with device_index
/// set to 0x01 (receiver slot 1) so the daemon probes the first paired device.
pub fn discover_devices() -> Result<Vec<DeviceInfo>> {
    let api = HidApi::new().context("Failed to initialize hidapi")?;
    let mut devices = Vec::new();
    let mut seen_paths = HashSet::new();

    for device_info in api.device_list() {
        if device_info.vendor_id() != LOGITECH_VENDOR_ID {
            continue;
        }

        // Only the HID++ vendor interface is useful for protocol communication.
        if device_info.usage_page() != HIDPP_USAGE_PAGE {
            continue;
        }

        let path = match device_info.path().to_str() {
            Ok(p) => p.to_string(),
            Err(_) => continue,
        };

        // Each hidraw node appears multiple times in the list (once per usage
        // within the interface). Deduplicate by path.
        if !seen_paths.insert(path.clone()) {
            continue;
        }

        let name = device_info
            .product_string()
            .unwrap_or("Unknown Logitech Device")
            .to_string();

        // A receiver dongle product string contains "Receiver" or "receiver".
        // Direct-connected devices will have their own product name.
        // For a receiver we start with device_index 0x01 (first paired slot).
        // For a direct device we use 0xFF.
        let is_receiver = name.to_lowercase().contains("receiver");
        let device_index = if is_receiver { 0x01_u8 } else { 0xFF_u8 };

        debug!(
            "Discovered HID++ node: {:04X}:{:04X} at {} (index={:#04x}, receiver={})",
            device_info.vendor_id(),
            device_info.product_id(),
            path,
            device_index,
            is_receiver,
        );

        devices.push(DeviceInfo {
            name,
            vendor_id: device_info.vendor_id(),
            product_id: device_info.product_id(),
            wpid: None,
            hidpp_version: None,
            hidraw_path: path,
            device_index,
            connected: true,
        });
    }

    Ok(devices)
}

/// Open a hidraw device by path.
pub fn open_device(hidraw_path: &str) -> Result<HidDevice> {
    let api = HidApi::new().context("Failed to initialize hidapi")?;
    api.open_path(
        std::ffi::CStr::from_bytes_with_nul(format!("{}\0", hidraw_path).as_bytes())
            .context("Invalid path")?,
    )
    .with_context(|| format!("Failed to open HID device: {}", hidraw_path))
}

/// Probe a device to determine its HID++ protocol version.
/// Returns (major, minor) or None if not an HID++ device.
pub fn probe_hidpp_version(device: &HidDevice, device_index: u8) -> Option<(u8, u8)> {
    use crate::device::hidpp::constants::{REPORT_ID_SHORT, SHORT_MESSAGE_LEN, SOFTWARE_ID};

    let mut msg = vec![0u8; SHORT_MESSAGE_LEN];
    msg[0] = REPORT_ID_SHORT;
    msg[1] = device_index;
    msg[2] = 0x00; // feature_index 0 = ROOT
    msg[3] = 0x10 | SOFTWARE_ID; // function 0x1 (ping), sw_id
    msg[4] = 0x00;
    msg[5] = 0x00;
    msg[6] = 0xAA; // ping byte

    if device.write(&msg).is_err() {
        return None;
    }

    let mut buf = vec![0u8; 32];
    match device.read_timeout(&mut buf, 1000) {
        Ok(n) if n >= 7 => {
            if buf[0] == 0x11 && buf[2] == 0x00 {
                return Some((2, 0));
            }
            if buf[2] == 0x8F {
                return Some((1, 0));
            }
            None
        }
        _ => None,
    }
}
