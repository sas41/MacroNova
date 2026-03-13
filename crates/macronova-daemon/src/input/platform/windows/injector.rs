/// Windows input injector stub.
///
/// TODO: implement using `SendInput()` with `INPUT_KEYBOARD` / `INPUT_MOUSE` structs.
/// Reference:
///   - https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
///   - https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-input
use anyhow::Result;
use tracing::warn;

use macronova_core::config::WarpMode;
use macronova_core::platform::input::InputInjector;

pub struct WindowsInjector;

impl WindowsInjector {
    pub fn new(_desktop_w: i32, _desktop_h: i32, _warp_mode: WarpMode) -> Result<Self> {
        warn!("WindowsInjector: input injection is not yet implemented on Windows");
        Ok(Self)
    }
}

impl InputInjector for WindowsInjector {
    fn key_down(&mut self, name: &str) -> Result<()> {
        warn!("key_down('{}') not implemented on Windows", name);
        Ok(())
    }
    fn key_up(&mut self, name: &str) -> Result<()> {
        warn!("key_up('{}') not implemented on Windows", name);
        Ok(())
    }
    fn tap_key(&mut self, name: &str) -> Result<()> {
        warn!("tap_key('{}') not implemented on Windows", name);
        Ok(())
    }
    fn type_text(&mut self, text: &str) -> Result<()> {
        warn!("type_text('{}') not implemented on Windows", text);
        Ok(())
    }
    fn click(&mut self, button: &str) -> Result<()> {
        warn!("click('{}') not implemented on Windows", button);
        Ok(())
    }
    fn button_down(&mut self, button: &str) -> Result<()> {
        warn!("button_down('{}') not implemented on Windows", button);
        Ok(())
    }
    fn button_up(&mut self, button: &str) -> Result<()> {
        warn!("button_up('{}') not implemented on Windows", button);
        Ok(())
    }
    fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        warn!("move_rel({}, {}) not implemented on Windows", dx, dy);
        Ok(())
    }
    fn warp(&mut self, x: i32, y: i32) -> Result<()> {
        // TODO: SetCursorPos(x, y)
        warn!("warp({}, {}) not implemented on Windows", x, y);
        Ok(())
    }
    fn scroll(&mut self, clicks: i32) -> Result<()> {
        warn!("scroll({}) not implemented on Windows", clicks);
        Ok(())
    }
    fn hscroll(&mut self, clicks: i32) -> Result<()> {
        warn!("hscroll({}) not implemented on Windows", clicks);
        Ok(())
    }
}
