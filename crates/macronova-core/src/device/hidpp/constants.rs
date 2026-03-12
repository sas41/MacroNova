/// Logitech vendor ID.
pub const LOGITECH_VENDOR_ID: u16 = 0x046D;

/// HID++ report IDs.
pub const REPORT_ID_SHORT: u8 = 0x10; // 7-byte messages
pub const REPORT_ID_LONG: u8 = 0x11; // 20-byte messages
pub const REPORT_ID_DJ: u8 = 0x20; // 15-byte DJ messages

pub const SHORT_MESSAGE_LEN: usize = 7;
pub const LONG_MESSAGE_LEN: usize = 20;

/// Software ID used in the low nibble of request_id to correlate replies.
/// Solaar cycles 0x2–0xF; we use a fixed value for simplicity.
pub const SOFTWARE_ID: u8 = 0x03;

/// HID++ 2.0 feature IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Feature {
    Root = 0x0000,
    FeatureSet = 0x0001,
    FirmwareInfo = 0x0003,
    DeviceNameType = 0x0005,
    BatteryStatus = 0x1000,
    BatteryVoltage = 0x1001,
    UnifiedBattery = 0x1004,
    KbdReprogrammableKeys = 0x1B00,
    ReprogramControlsV2 = 0x1B01,
    ReprogramControlsV4 = 0x1B04,
    MouseButtonSpy = 0x8110,
    OnboardProfiles = 0x8100,
    ReportRate = 0x8060,
    AdjustableDpi = 0x2201,
    HiresWheel = 0x2121,
    SmartShift = 0x2110,
    Unknown = 0xFFFF,
}

impl Feature {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x0000 => Self::Root,
            0x0001 => Self::FeatureSet,
            0x0003 => Self::FirmwareInfo,
            0x0005 => Self::DeviceNameType,
            0x1000 => Self::BatteryStatus,
            0x1001 => Self::BatteryVoltage,
            0x1004 => Self::UnifiedBattery,
            0x1B00 => Self::KbdReprogrammableKeys,
            0x1B01 => Self::ReprogramControlsV2,
            0x1B04 => Self::ReprogramControlsV4,
            0x8110 => Self::MouseButtonSpy,
            0x8100 => Self::OnboardProfiles,
            0x8060 => Self::ReportRate,
            0x2201 => Self::AdjustableDpi,
            0x2121 => Self::HiresWheel,
            0x2110 => Self::SmartShift,
            _ => Self::Unknown,
        }
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// Known Control IDs (CIDs) for Logitech mice.
/// Source: Solaar special_keys.py
pub mod cid {
    pub const LEFT_BUTTON: u16 = 0x0050;
    pub const RIGHT_BUTTON: u16 = 0x0051;
    pub const MIDDLE_BUTTON: u16 = 0x0052;
    pub const BACK_BUTTON: u16 = 0x0053;
    pub const FORWARD_BUTTON: u16 = 0x0056;
    pub const APPSWITCH: u16 = 0x010A;
    pub const SMART_SHIFT: u16 = 0x00C4;
    pub const GESTURE_BUTTON: u16 = 0x00C3;
    pub const DPI_CHANGE: u16 = 0x00ED;
    pub const DPI_SWITCH: u16 = 0x00FD;
    // G502 X Lightspeed specific
    pub const G502_SNIPER: u16 = 0x00C4; // Sniper/DPI shift button
    pub const G502_DPI_CYCLE: u16 = 0x00ED;
    pub const G502_SIDE_BACK: u16 = 0x0053;
    pub const G502_SIDE_FWD: u16 = 0x0056;
}

/// Human-readable name for a known CID.
pub fn cid_name(cid: u16) -> &'static str {
    match cid {
        0x0050 => "Left Button",
        0x0051 => "Right Button",
        0x0052 => "Middle Button",
        0x0053 => "Back Button",
        0x0056 => "Forward Button",
        0x00C3 => "Gesture Button",
        0x00C4 => "Sniper / DPI Shift",
        0x00ED => "DPI Cycle",
        0x00FD => "DPI Switch",
        0x010A => "App Switch",
        _ => "Unknown",
    }
}

/// Key flags from getCidInfo (REPROG_CONTROLS_V4 function 0x10).
pub mod key_flags {
    pub const MSE: u16 = 0x0001;
    pub const FN_TOGGLE: u16 = 0x0002;
    pub const HOT_KEY: u16 = 0x0004;
    pub const FN_KEY: u16 = 0x0008;
    pub const REPROGRAMMABLE: u16 = 0x0010;
    pub const DIVERTABLE: u16 = 0x0020;
    pub const PERSISTENTLY_DIVERTABLE: u16 = 0x0040;
    pub const VIRTUAL: u16 = 0x0080;
    pub const RAW_XY: u16 = 0x0100;
}

/// Mapping flags for setCidReporting (REPROG_CONTROLS_V4 function 0x30).
pub mod mapping_flags {
    pub const DIVERTED: u8 = 0x01;
    pub const PERSISTENTLY_DIVERTED: u8 = 0x04;
    pub const RAW_XY_DIVERTED: u8 = 0x10;
}
