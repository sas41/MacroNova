/// REPROG_CONTROLS_V4 (feature 0x1B04) — button enumeration, diversion, and notification decoding.
///
/// Functions:
///   0x00 = getCapabilities  → button count, max simultaneously active buttons
///   0x10 = getCidInfo(idx)  → CID, task, flags, position, group, gmask
///   0x20 = getCidReporting  → current diversion state for a CID
///   0x30 = setCidReporting  → set diversion state for a CID
///
/// Notifications (address byte bits 7:4 == 0x0, sw_id == 0):
///   data[0..8] = 4 × uint16-BE CIDs currently held down (0 = empty slot)
use anyhow::Result;
use hidapi::HidDevice;
use tracing::debug;

use super::base::request;
use super::constants::{key_flags, mapping_flags};
use super::features::FeatureTable;

/// A single reprogrammable button returned by getCidInfo.
#[derive(Debug, Clone)]
pub struct ButtonInfo {
    /// Logitech Control ID.
    pub cid: u16,
    /// Default task / hardware action code.
    pub task_id: u16,
    /// Capability flags (see key_flags module).
    pub flags: u16,
    /// Whether this button can be diverted (intercepted by software).
    pub divertable: bool,
    /// Button position index on the device.
    pub pos: u8,
    /// Logical group index.
    pub group: u8,
}

/// Enumerate all buttons on the device via REPROG_CONTROLS_V4.
pub fn enumerate_buttons(
    device: &HidDevice,
    device_index: u8,
    features: &FeatureTable,
) -> Result<Vec<ButtonInfo>> {
    let feat_idx = match features.get_index(super::constants::Feature::ReprogramControlsV4) {
        Some(i) => i,
        None => {
            debug!("Device does not support REPROG_CONTROLS_V4");
            return Ok(vec![]);
        }
    };

    // getCapabilities (function 0x00) → byte 0 = button count
    let caps = request(device, device_index, feat_idx, 0x00, &[])?;
    let count = caps.first().copied().unwrap_or(0) as usize;
    debug!("REPROG_CONTROLS_V4: {} buttons", count);

    let mut buttons = Vec::with_capacity(count);
    for i in 0..count {
        // getCidInfo (function 0x10) → CID(2), task(2), flags1(1), pos(1), group(1), gmask(1), flags2(1)
        let data = request(device, device_index, feat_idx, 0x10, &[i as u8])?;
        if data.len() < 9 {
            continue;
        }
        let cid = u16::from_be_bytes([data[0], data[1]]);
        let task_id = u16::from_be_bytes([data[2], data[3]]);
        let flags1 = data[4] as u16;
        let pos = data[5];
        let group = data[6];
        // flags2 is in data[8] (byte 8, 0-indexed from the params start)
        let flags2 = if data.len() > 8 { data[8] as u16 } else { 0 };
        let flags = flags1 | (flags2 << 8);

        buttons.push(ButtonInfo {
            cid,
            task_id,
            flags,
            divertable: (flags & key_flags::DIVERTABLE) != 0,
            pos,
            group,
        });
        debug!(
            "  Button[{}]: CID={:#06x} task={:#06x} flags={:#06x} divertable={}",
            i,
            cid,
            task_id,
            flags,
            (flags & key_flags::DIVERTABLE) != 0
        );
    }

    Ok(buttons)
}

/// Enable or disable HID++ diversion for a specific CID.
///
/// When diverted, button presses are reported via REPROG_CONTROLS_V4 notifications
/// rather than standard HID reports.
pub fn set_cid_diversion(
    device: &HidDevice,
    device_index: u8,
    features: &FeatureTable,
    cid: u16,
    diverted: bool,
) -> Result<()> {
    let feat_idx = match features.get_index(super::constants::Feature::ReprogramControlsV4) {
        Some(i) => i,
        None => anyhow::bail!("Device does not support REPROG_CONTROLS_V4"),
    };

    let divert_flag = if diverted {
        mapping_flags::DIVERTED
    } else {
        0u8
    };

    // setCidReporting (function 0x30): CID(2), divert_flags(1), remap_cid(2)
    // remap_cid = 0 means no remapping, keep default task.
    let params = [
        (cid >> 8) as u8,
        (cid & 0xFF) as u8,
        divert_flag,
        0x00, // remap high
        0x00, // remap low
    ];

    request(device, device_index, feat_idx, 0x30, &params)?;
    debug!("CID {:#06x} diversion set to {}", cid, diverted);
    Ok(())
}

/// Decode a REPROG_CONTROLS_V4 notification payload.
///
/// Returns up to 4 CIDs that are currently held down.
/// CID value 0 means an empty slot.
pub fn decode_button_notification(data: &[u8]) -> [u16; 4] {
    let mut cids = [0u16; 4];
    for (i, cid) in cids.iter_mut().enumerate() {
        let offset = i * 2;
        if offset + 1 < data.len() {
            *cid = u16::from_be_bytes([data[offset], data[offset + 1]]);
        }
    }
    cids
}
