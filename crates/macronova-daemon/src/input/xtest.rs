/// Mouse injection stub.
///
/// Mouse click/button injection is not yet implemented. See MOUSE.md in the
/// project root for a detailed explanation of what was tried and what the
/// correct path forward is (XDG RemoteDesktop portal via `ashpd` + `reis`).
///
/// All methods on `MouseInjector` are no-ops that emit a one-time warning.
use std::sync::Once;
use tracing::warn;

static WARNED: Once = Once::new();

fn warn_once() {
    WARNED.call_once(|| {
        warn!(
            "Mouse injection is not implemented. \
             See MOUSE.md for details. \
             Mouse Rhai functions (click, press_mouse, etc.) are no-ops."
        );
    });
}

/// Stub mouse injector. All methods are no-ops.
#[derive(Debug, Default)]
pub struct MouseInjector;

impl MouseInjector {
    pub fn new() -> Self {
        Self
    }

    pub fn button_event(&self, _button: u32, _press: bool) {
        warn_once();
    }

    pub fn click(&self, _button: u32) {
        warn_once();
    }

    pub fn move_abs(&self, _x: i32, _y: i32) {
        warn_once();
    }

    pub fn move_rel(&self, _dx: i32, _dy: i32) {
        warn_once();
    }

    pub fn scroll(&self, _amount: i32) {
        warn_once();
    }

    pub fn hscroll(&self, _amount: i32) {
        warn_once();
    }
}

/// Resolve a human-friendly button name to an X11-convention button number.
///
/// X11 button numbers (kept for future portal implementation):
///   1 = left, 2 = middle, 3 = right, 8 = side/back, 9 = extra/forward
pub fn xbtn_by_name(name: &str) -> Option<u32> {
    match name.to_uppercase().as_str() {
        "LEFT" | "BTN_LEFT" | "1" => Some(1),
        "MIDDLE" | "BTN_MIDDLE" | "2" => Some(2),
        "RIGHT" | "BTN_RIGHT" | "3" => Some(3),
        "SIDE" | "BTN_SIDE" | "BACK" | "8" => Some(8),
        "EXTRA" | "BTN_EXTRA" | "FORWARD" | "9" => Some(9),
        _ => None,
    }
}
