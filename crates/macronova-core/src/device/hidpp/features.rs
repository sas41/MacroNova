/// HID++ 2.0 feature table resolution.
///
/// Maps Feature enum values → runtime feature index (the table index
/// assigned by the device firmware, used in all subsequent requests).
use std::collections::HashMap;

use anyhow::{bail, Result};
use hidapi::HidDevice;
use tracing::debug;

use super::base::request;
use super::constants::Feature;

/// The ROOT feature is always at index 0x00.
const ROOT_FEATURE_INDEX: u8 = 0x00;
/// ROOT function 0x00 = getFeature(featureId) → returns feature index.
const ROOT_GET_FEATURE: u8 = 0x00;
/// FEATURE_SET function 0 = getCount() → number of features.
const FEATURE_SET_GET_COUNT: u8 = 0x00;
/// FEATURE_SET function 1 = getFeatureId(index) → feature ID + version.
/// NOTE: request() encodes function_id as (function_id << 4) | sw_id in the
/// address byte, so pass the bare function number here (not pre-shifted).
const FEATURE_SET_GET_FEATURE_ID: u8 = 0x01;

/// Resolved feature table for a single device.
#[derive(Debug, Default, Clone)]
pub struct FeatureTable {
    /// Maps Feature → runtime table index.
    pub index_of: HashMap<u16, u8>,
    /// Maps runtime index → Feature ID.
    pub feature_at: HashMap<u8, u16>,
}

impl FeatureTable {
    /// Query the device for its full feature table.
    pub fn query(device: &HidDevice, device_index: u8) -> Result<Self> {
        let mut table = FeatureTable::default();

        // Step 1: Get the index of FEATURE_SET (0x0001) via ROOT.
        let feature_set_idx = get_feature_index(device, device_index, Feature::FeatureSet)?;
        if feature_set_idx == 0 {
            bail!("Device does not support FEATURE_SET — not an HID++ 2.0 device");
        }
        table
            .index_of
            .insert(Feature::Root.as_u16(), ROOT_FEATURE_INDEX);
        table
            .feature_at
            .insert(ROOT_FEATURE_INDEX, Feature::Root.as_u16());
        table
            .index_of
            .insert(Feature::FeatureSet.as_u16(), feature_set_idx);
        table
            .feature_at
            .insert(feature_set_idx, Feature::FeatureSet.as_u16());

        // Step 2: Get the count of features.
        let count_data = request(
            device,
            device_index,
            feature_set_idx,
            FEATURE_SET_GET_COUNT,
            &[],
        )?;
        let count = count_data.first().copied().unwrap_or(0) as usize;
        debug!("Device has {} HID++ 2.0 features", count);

        // Step 3: Enumerate all features by index.
        for idx in 1..=(count as u8) {
            let feat_data = request(
                device,
                device_index,
                feature_set_idx,
                FEATURE_SET_GET_FEATURE_ID,
                &[idx],
            )?;
            if feat_data.len() < 2 {
                continue;
            }
            let feature_id = u16::from_be_bytes([feat_data[0], feat_data[1]]);
            table.index_of.insert(feature_id, idx);
            table.feature_at.insert(idx, feature_id);
            debug!(
                "  Feature[{:#04x}] = {:#06x} ({})",
                idx,
                feature_id,
                Feature::from_u16(feature_id).name()
            );
        }

        Ok(table)
    }

    /// Look up the runtime index for a feature, returning None if unsupported.
    pub fn get_index(&self, feature: Feature) -> Option<u8> {
        self.index_of.get(&feature.as_u16()).copied()
    }

    /// Reverse-look up the feature ID for a given runtime index.
    pub fn get_feature_id(&self, index: u8) -> Option<Feature> {
        self.feature_at.get(&index).map(|&id| Feature::from_u16(id))
    }
}

/// Query the ROOT feature (index 0) to get the runtime index for `feature`.
/// Returns 0 if the feature is not supported.
pub fn get_feature_index(device: &HidDevice, device_index: u8, feature: Feature) -> Result<u8> {
    let feature_id = feature.as_u16();
    let params = [(feature_id >> 8) as u8, (feature_id & 0xFF) as u8];
    let data = request(
        device,
        device_index,
        ROOT_FEATURE_INDEX,
        ROOT_GET_FEATURE,
        &params,
    )?;
    Ok(data.first().copied().unwrap_or(0))
}

impl Feature {
    pub fn name(self) -> &'static str {
        match self {
            Feature::Root => "ROOT",
            Feature::FeatureSet => "FEATURE_SET",
            Feature::FirmwareInfo => "FIRMWARE_INFO",
            Feature::DeviceNameType => "DEVICE_NAME_TYPE",
            Feature::BatteryStatus => "BATTERY_STATUS",
            Feature::BatteryVoltage => "BATTERY_VOLTAGE",
            Feature::UnifiedBattery => "UNIFIED_BATTERY",
            Feature::KbdReprogrammableKeys => "KBD_REPROGRAMMABLE_KEYS",
            Feature::ReprogramControlsV2 => "REPROG_CONTROLS_V2",
            Feature::ReprogramControlsV4 => "REPROG_CONTROLS_V4",
            Feature::MouseButtonSpy => "MOUSE_BUTTON_SPY",
            Feature::OnboardProfiles => "ONBOARD_PROFILES",
            Feature::ReportRate => "REPORT_RATE",
            Feature::AdjustableDpi => "ADJUSTABLE_DPI",
            Feature::HiresWheel => "HIRES_WHEEL",
            Feature::SmartShift => "SMART_SHIFT",
            Feature::Unknown => "UNKNOWN",
        }
    }
}
