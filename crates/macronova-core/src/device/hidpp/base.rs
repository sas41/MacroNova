/// Low-level HID++ 2.0 framing and request/reply over a hidapi device handle.
///
/// Message formats (from Solaar base.py):
///   Short: [0x10, devnum, sub_id, address, param0..param3]        = 7 bytes
///   Long:  [0x11, devnum, sub_id, address, param0..param15]       = 20 bytes
///
/// HID++ 2.0 feature requests use:
///   sub_id  = feature_index (from feature table)
///   address = (function_id << 4) | software_id
use std::time::Duration;

use anyhow::{bail, Result};
use hidapi::HidDevice;
use tracing::{debug, trace, warn};

use super::constants::{LONG_MESSAGE_LEN, REPORT_ID_LONG, REPORT_ID_SHORT, SOFTWARE_ID};

/// A raw HID++ notification packet.
#[derive(Debug, Clone)]
pub struct Notification {
    pub report_id: u8,
    pub device_index: u8,
    pub feature_index: u8,
    pub function_id: u8, // high nibble of address byte
    pub software_id: u8, // low nibble of address byte
    pub data: Vec<u8>,
}

/// Send a short HID++ request and wait for the matching reply.
///
/// `feature_index` is the runtime index of the feature (from feature table lookup).
/// `function_id`   is the function within that feature (0..0xF).
/// `params`        up to 15 bytes of parameters (zero-padded to fill a long message).
pub fn request(
    device: &HidDevice,
    device_index: u8,
    feature_index: u8,
    function_id: u8,
    params: &[u8],
) -> Result<Vec<u8>> {
    let address = (function_id << 4) | SOFTWARE_ID;

    // Build a long message (20 bytes) for all requests.
    let mut msg = vec![0u8; LONG_MESSAGE_LEN];
    msg[0] = REPORT_ID_LONG;
    msg[1] = device_index;
    msg[2] = feature_index;
    msg[3] = address;
    let param_len = params.len().min(LONG_MESSAGE_LEN - 4);
    msg[4..4 + param_len].copy_from_slice(&params[..param_len]);

    trace!(
        "HID++ request: dev={:#04x} feat={:#04x} fn={:#03x} data={:02x?}",
        device_index,
        feature_index,
        function_id,
        &msg[4..4 + param_len]
    );

    device.write(&msg)?;

    // Read replies until we get one matching our sw_id.
    let timeout_ms = 2000i32;
    let mut buf = vec![0u8; 32];
    loop {
        let n = device.read_timeout(&mut buf, timeout_ms)?;
        if n == 0 {
            bail!(
                "HID++ request timed out (feat={:#06x} fn={:#03x})",
                feature_index,
                function_id
            );
        }

        let reply = &buf[..n];
        trace!("HID++ reply raw: {:02x?}", reply);

        // Check for HID++ 2.0 error: reply[2] = 0xFF, reply[3] = feature_index, reply[4] = address
        if reply.len() >= 5 && reply[0] == REPORT_ID_LONG && reply[2] == 0xFF {
            let err_code = reply[5];
            bail!(
                "HID++ 2.0 error: feat={:#06x} fn={:#03x} code={:#04x}",
                feature_index,
                function_id,
                err_code
            );
        }

        // Check if this reply matches our request (same feature_index, function, sw_id).
        if reply.len() >= 4
            && (reply[0] == REPORT_ID_SHORT || reply[0] == REPORT_ID_LONG)
            && reply[1] == device_index
            && reply[2] == feature_index
            && reply[3] == address
        {
            let data_start = 4;
            let data = reply[data_start..].to_vec();
            debug!(
                "HID++ reply: dev={:#04x} feat={:#04x} fn={:#03x} data={:02x?}",
                device_index, feature_index, function_id, &data
            );
            return Ok(data);
        }

        // Not our reply — could be an async notification. Log and continue.
        debug!("HID++ ignored packet: {:02x?}", reply);
    }
}

/// Read a single HID++ notification from the device with an optional timeout.
/// Returns `None` on timeout.
pub fn read_notification(device: &HidDevice, timeout: Duration) -> Result<Option<Notification>> {
    let mut buf = vec![0u8; 32];
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let n = device.read_timeout(&mut buf, timeout_ms)?;
    if n == 0 {
        return Ok(None);
    }
    let pkt = &buf[..n];
    if pkt.len() < 4 {
        warn!("Received short packet ({} bytes), ignoring", pkt.len());
        return Ok(None);
    }

    // Only process notification packets (software_id == 0 in address nibble means
    // it's a device-initiated notification, not a reply to our request).
    let sw_id = pkt[3] & 0x0F;
    let notif = Notification {
        report_id: pkt[0],
        device_index: pkt[1],
        feature_index: pkt[2],
        function_id: (pkt[3] >> 4) & 0x0F,
        software_id: sw_id,
        data: pkt[4..].to_vec(),
    };
    Ok(Some(notif))
}
