/// macOS input injector stub.
///
/// TODO: implement using CoreGraphics `CGEventPost()`:
///   - `CGEventCreateKeyboardEvent(NULL, key_code, true/false)` for keys
///   - `CGEventCreateMouseEvent(NULL, kCGEventLeftMouseDown, pos, kCGMouseButtonLeft)` for mouse
///   - `CGEventPost(kCGHIDEventTap, event)` to inject
/// Reference:
///   - https://developer.apple.com/documentation/coregraphics/1456564-cgeventpost
///   - https://developer.apple.com/documentation/coregraphics/1456081-cgeventcreatekeyboardevent
use anyhow::Result;
use tracing::warn;

use macronova_core::config::WarpMode;
use macronova_core::platform::input::InputInjector;

pub struct MacOSInjector;

impl MacOSInjector {
    pub fn new(_desktop_w: i32, _desktop_h: i32, _warp_mode: WarpMode) -> Result<Self> {
        warn!("MacOSInjector: input injection is not yet implemented on macOS");
        Ok(Self)
    }
}

impl InputInjector for MacOSInjector {
    fn key_down(&mut self, name: &str) -> Result<()> {
        warn!("key_down('{}') not implemented on macOS", name);
        Ok(())
    }
    fn key_up(&mut self, name: &str) -> Result<()> {
        warn!("key_up('{}') not implemented on macOS", name);
        Ok(())
    }
    fn tap_key(&mut self, name: &str) -> Result<()> {
        warn!("tap_key('{}') not implemented on macOS", name);
        Ok(())
    }
    fn type_text(&mut self, text: &str) -> Result<()> {
        warn!("type_text('{}') not implemented on macOS", text);
        Ok(())
    }
    fn click(&mut self, button: &str) -> Result<()> {
        warn!("click('{}') not implemented on macOS", button);
        Ok(())
    }
    fn button_down(&mut self, button: &str) -> Result<()> {
        warn!("button_down('{}') not implemented on macOS", button);
        Ok(())
    }
    fn button_up(&mut self, button: &str) -> Result<()> {
        warn!("button_up('{}') not implemented on macOS", button);
        Ok(())
    }
    fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()> {
        warn!("move_rel({}, {}) not implemented on macOS", dx, dy);
        Ok(())
    }
    fn warp(&mut self, x: i32, y: i32) -> Result<()> {
        // TODO: CGDisplayMoveCursorToPoint(kCGDirectMainDisplay, CGPointMake(x, y))
        warn!("warp({}, {}) not implemented on macOS", x, y);
        Ok(())
    }
    fn scroll(&mut self, clicks: i32) -> Result<()> {
        warn!("scroll({}) not implemented on macOS", clicks);
        Ok(())
    }
    fn hscroll(&mut self, clicks: i32) -> Result<()> {
        warn!("hscroll({}) not implemented on macOS", clicks);
        Ok(())
    }
}
