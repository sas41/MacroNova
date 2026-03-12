//! HID++ notification reader for Logitech devices.
//!
//! When Solaar (or any other tool) diverts buttons via REPROG_CONTROLS_V4
//! `setCidReporting`, the device firmware stops sending standard HID reports
//! for those buttons — they vanish from the evdev stream entirely.
//!
//! This module opens the HID++ vendor channel (`usage_page 0xFF00`), diverts
//! the CIDs that match bindings in the config, and translates incoming
//! REPROG_CONTROLS_V4 notifications into `ButtonEvent`s that feed the same
//! event pipeline as the evdev reader.
//!
//! Button names produced here have the form `"cid/0xNNNN"`, e.g. `"cid/0x00c4"`
//! for the Sniper button.  These are the names the GUI Capture feature records
//! when the device is in HID++ notification mode.

use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

use anyhow::{Context, Result};
use hidapi::HidApi;
use tracing::{debug, info, warn};

use macronova_core::device::hidpp::base::read_notification;
use macronova_core::device::hidpp::{
    constants::{Feature, LOGITECH_VENDOR_ID},
    decode_button_notification, enumerate_buttons, set_cid_diversion, FeatureTable,
};

/// A button event produced from a HID++ notification.
#[derive(Debug, Clone)]
pub struct HidppButtonEvent {
    /// Button name in the form `"cid/0xNNNN"`.
    pub name: String,
    pub pressed: bool,
}

/// Canonical button name for a CID, matching what the GUI Capture records.
pub fn cid_button_name(cid: u16) -> String {
    format!("cid/0x{:04x}", cid)
}

/// Parse a CID from a button name produced by `cid_button_name`.
pub fn cid_from_button_name(name: &str) -> Option<u16> {
    let hex = name.strip_prefix("cid/0x")?;
    u16::from_str_radix(hex, 16).ok()
}

/// Usage page for the HID++ vendor interface.
const HIDPP_USAGE_PAGE: u16 = 0xFF00;

/// Spawn a background thread that:
/// 1. Opens the HID++ vendor channel for the first Logitech device found.
/// 2. Enumerates buttons and diverts those whose CIDs match `cids_to_divert`.
/// 3. Reads REPROG_CONTROLS_V4 notifications and sends `HidppButtonEvent`s on `tx`.
/// 4. On thread exit (channel closed or device lost), un-diverts all CIDs it diverted.
///
/// `alive` is set to `false` when the thread exits so callers can detect device loss.
///
/// Returns `Ok(())` if the thread was spawned, `Err` if no suitable device was found.
pub fn spawn(
    cids_to_divert: HashSet<u16>,
    tx: mpsc::Sender<HidppButtonEvent>,
    alive: Arc<AtomicBool>,
) -> Result<()> {
    let (hidraw_path, device_index) = find_hidpp_channel()
        .context("No Logitech HID++ device found — HID++ notification path unavailable")?;

    info!(
        "HID++ reader: using {} (device_index={:#04x}), diverting {} CID(s)",
        hidraw_path,
        device_index,
        cids_to_divert.len()
    );

    alive.store(true, Ordering::Relaxed);

    std::thread::Builder::new()
        .name("macronova-hidpp".into())
        .spawn(move || {
            if let Err(e) = run_reader(hidraw_path, device_index, cids_to_divert, tx) {
                warn!("HID++ reader exited: {e}");
            }
            alive.store(false, Ordering::Relaxed);
        })
        .context("Failed to spawn HID++ reader thread")?;

    Ok(())
}

// ── internals ────────────────────────────────────────────────────────────────

fn run_reader(
    hidraw_path: String,
    device_index: u8,
    cids_to_divert: HashSet<u16>,
    tx: mpsc::Sender<HidppButtonEvent>,
) -> Result<()> {
    let api = HidApi::new().context("hidapi init")?;
    let device = api
        .open_path(
            std::ffi::CStr::from_bytes_with_nul(format!("{}\0", hidraw_path).as_bytes())
                .context("Invalid hidraw path")?,
        )
        .with_context(|| format!("Failed to open {hidraw_path}"))?;

    // Build feature table.
    let features = FeatureTable::query(&device, device_index)
        .context("Failed to query HID++ feature table")?;

    // Enumerate available buttons and intersect with what we need to divert.
    let available = enumerate_buttons(&device, device_index, &features).unwrap_or_default();
    let available_cids: HashSet<u16> = available.iter().map(|b| b.cid).collect();

    let to_divert: HashSet<u16> = cids_to_divert
        .iter()
        .filter(|cid| {
            if available_cids.contains(cid) {
                true
            } else {
                warn!("CID {:#06x} not found on device — cannot divert", cid);
                false
            }
        })
        .copied()
        .collect();

    // Divert each CID.
    let mut diverted: HashSet<u16> = HashSet::new();
    for &cid in &to_divert {
        match set_cid_diversion(&device, device_index, &features, cid, true) {
            Ok(_) => {
                info!(
                    "Diverted CID {:#06x} ({})",
                    cid,
                    macronova_core::device::hidpp::constants::cid_name(cid)
                );
                diverted.insert(cid);
            }
            Err(e) => warn!("Failed to divert CID {:#06x}: {e}", cid),
        }
    }

    if diverted.is_empty() {
        info!("HID++ reader: no CIDs diverted, exiting");
        return Ok(());
    }

    // Determine the feature index for REPROG_CONTROLS_V4 (used to filter notifications).
    let reprog_feat_idx = features.get_index(Feature::ReprogramControlsV4);

    // Track currently-held CIDs so we can synthesise releases.
    let mut held: HashMap<u16, bool> = HashMap::new();

    info!("HID++ reader: listening for button notifications");

    loop {
        match read_notification(&device, Duration::from_millis(200)) {
            Ok(None) => {
                // Timeout — check if receiver is still alive.
                if tx
                    .send(HidppButtonEvent {
                        name: String::new(),
                        pressed: false,
                    })
                    .is_err()
                {
                    // Channel closed: GUI or main loop exited.
                    break;
                }
                // Ignore the sentinel we just sent — main loop must filter empty names.
                continue;
            }
            Ok(Some(notif)) => {
                // Only process REPROG_CONTROLS_V4 notifications.
                let is_reprog = reprog_feat_idx
                    .map(|idx| notif.feature_index == idx)
                    .unwrap_or(false);
                if !is_reprog {
                    continue;
                }

                // Notifications from device have software_id == 0.
                if notif.software_id != 0 {
                    continue;
                }

                let cids = decode_button_notification(&notif.data);
                debug!("HID++ notification CIDs: {:04x?}", cids);

                // Determine which diverted CIDs are now held.
                let now_held: HashSet<u16> = cids
                    .iter()
                    .copied()
                    .filter(|&c| c != 0 && diverted.contains(&c))
                    .collect();

                // Emit press events for newly-held CIDs.
                for &cid in &now_held {
                    if !held.contains_key(&cid) {
                        let name = cid_button_name(cid);
                        if tx
                            .send(HidppButtonEvent {
                                name,
                                pressed: true,
                            })
                            .is_err()
                        {
                            break;
                        }
                        held.insert(cid, true);
                    }
                }

                // Emit release events for CIDs no longer held.
                let released: Vec<u16> = held
                    .keys()
                    .copied()
                    .filter(|c| !now_held.contains(c))
                    .collect();
                for cid in released {
                    let name = cid_button_name(cid);
                    if tx
                        .send(HidppButtonEvent {
                            name,
                            pressed: false,
                        })
                        .is_err()
                    {
                        break;
                    }
                    held.remove(&cid);
                }
            }
            Err(e) => {
                warn!("HID++ read error: {e} — device lost, exiting reader thread");
                // Exit so the main loop can detect the thread is gone and re-spawn.
                return Err(e.into());
            }
        }
    }

    // Clean up: un-divert all CIDs before exiting.
    for &cid in &diverted {
        if let Err(e) = set_cid_diversion(&device, device_index, &features, cid, false) {
            warn!("Failed to un-divert CID {:#06x}: {e}", cid);
        } else {
            debug!("Un-diverted CID {:#06x}", cid);
        }
    }

    Ok(())
}

/// Find the first Logitech HID++ vendor-channel hidraw node.
/// Returns (hidraw_path, device_index).
fn find_hidpp_channel() -> Option<(String, u8)> {
    let api = HidApi::new().ok()?;
    for info in api.device_list() {
        let info: &hidapi::DeviceInfo = info;
        if info.vendor_id() != LOGITECH_VENDOR_ID {
            continue;
        }
        if info.usage_page() != HIDPP_USAGE_PAGE {
            continue;
        }
        let path = info.path().to_str().ok()?.to_string();
        let name = info.product_string().unwrap_or("").to_lowercase();
        let device_index = if name.contains("receiver") {
            0x01
        } else {
            0xFF
        };
        return Some((path, device_index));
    }
    None
}
