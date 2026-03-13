use anyhow::Result;
/// Platform-agnostic input abstractions.
///
/// # Input reading
/// [`ButtonEvent`] and [`RawInputReader`] abstract over how the OS delivers
/// hardware button events:
/// - **Linux**: evdev `EV_KEY` events from `/dev/input/eventN`
/// - **Windows** (stub): Raw Input API / `WM_INPUT`
/// - **macOS** (stub): IOKit `IOHIDManagerRegisterInputValueCallback`
///
/// # Input injection
/// [`InputInjector`] abstracts over how synthetic input events are sent:
/// - **Linux**: uinput virtual device (`/dev/uinput`)
/// - **Windows** (stub): `SendInput()` with `INPUT_KEYBOARD` / `INPUT_MOUSE`
/// - **macOS** (stub): `CGEventPost()` with `CGEventCreateKeyboardEvent` / `CGEventCreateMouseEvent`
///
/// # Cursor position
/// [`get_cursor_position`] abstracts over cursor position queries:
/// - **Linux**: `XQueryPointer` via `x11-dl`
/// - **Windows** (stub): `GetCursorPos()`
/// - **macOS** (stub): `CGEventGetLocation()`
use std::time::Duration;

// ── Button events ─────────────────────────────────────────────────────────────

/// A platform-agnostic button press or release event.
#[derive(Debug, Clone)]
pub struct ButtonEvent {
    /// Stable string identifier for the button, e.g.
    /// `"usb-Logitech_USB_Receiver-event-mouse/key0x0110"` on Linux or
    /// `"hid-{GUID}/btn0x0001"` on Windows.
    pub button: String,
    pub pressed: bool,
}

/// Trait for a platform-specific input reader.
///
/// Implementations open OS-level device handles and produce [`ButtonEvent`]s.
/// The reader is expected to be polled in a tight loop by the daemon.
pub trait RawInputReader: Send {
    /// Block until a button event arrives or `timeout` elapses.
    /// Returns `Ok(Some(event))`, `Ok(None)` on timeout, or `Err` on I/O failure.
    fn poll(&mut self, timeout: Duration) -> Result<Option<ButtonEvent>>;
}

// ── Input injection ───────────────────────────────────────────────────────────

/// Trait for a platform-specific input injector.
///
/// All methods are best-effort: on platforms where an operation is not yet
/// implemented the method should log a warning and return `Ok(())`.
pub trait InputInjector: Send {
    // ── Keyboard ─────────────────────────────────────────────────────────────

    /// Press a key by name (e.g. `"a"`, `"ctrl"`, `"KEY_LEFTSHIFT"`).
    fn key_down(&mut self, name: &str) -> Result<()>;
    /// Release a key by name.
    fn key_up(&mut self, name: &str) -> Result<()>;
    /// Tap a key (down + up).
    fn tap_key(&mut self, name: &str) -> Result<()>;
    /// Type a UTF-8 string.
    fn type_text(&mut self, text: &str) -> Result<()>;

    // ── Mouse buttons ─────────────────────────────────────────────────────────

    /// Click a mouse button by name (`"left"`, `"right"`, `"middle"`, `"side"`, `"extra"`).
    fn click(&mut self, button: &str) -> Result<()>;
    /// Press a mouse button down.
    fn button_down(&mut self, button: &str) -> Result<()>;
    /// Release a mouse button.
    fn button_up(&mut self, button: &str) -> Result<()>;

    // ── Mouse motion ──────────────────────────────────────────────────────────

    /// Move the cursor by (dx, dy) pixels relative to its current position.
    fn move_rel(&mut self, dx: i32, dy: i32) -> Result<()>;
    /// Move the cursor to an absolute screen position.
    /// Implementations that cannot determine the current cursor position should
    /// warn and no-op rather than panic.
    fn warp(&mut self, x: i32, y: i32) -> Result<()>;

    // ── Scroll ────────────────────────────────────────────────────────────────

    /// Vertical scroll. Positive = down (one unit = one detent).
    fn scroll(&mut self, clicks: i32) -> Result<()>;
    /// Horizontal scroll. Positive = right.
    fn hscroll(&mut self, clicks: i32) -> Result<()>;
}

// ── Cursor position ───────────────────────────────────────────────────────────

/// Query the current cursor position from the display server.
/// Returns `None` if the display server is unavailable or the query fails.
pub fn get_cursor_position() -> Option<(i32, i32)> {
    platform_get_cursor_position()
}

/// Query the total logical desktop size (bounding box of all monitors).
/// Returns `None` if the display server is unavailable or the query fails.
pub fn get_desktop_size() -> Option<(i32, i32)> {
    platform_get_desktop_size()
}

// ── Platform implementations ──────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn platform_get_cursor_position() -> Option<(i32, i32)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        tracing::warn!(
            "get_cursor_position: Wayland does not support querying the global cursor \
             position from an unprivileged client. Returning None. \
             Use move_mouse(dx, dy) for relative motion instead of warp_mouse(x, y)."
        );
        return None;
    }

    use x11_dl::xlib;

    let xlib = xlib::Xlib::open().ok()?;
    let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
    if display.is_null() {
        return None;
    }
    let screen = unsafe { (xlib.XDefaultScreen)(display) };
    let root = unsafe { (xlib.XRootWindow)(display, screen) };

    let mut root_return: xlib::Window = 0;
    let mut child_return: xlib::Window = 0;
    let mut root_x: i32 = -1;
    let mut root_y: i32 = -1;
    let mut win_x: i32 = 0;
    let mut win_y: i32 = 0;
    let mut mask: u32 = 0;

    unsafe {
        (xlib.XQueryPointer)(
            display,
            root,
            &mut root_return,
            &mut child_return,
            &mut root_x,
            &mut root_y,
            &mut win_x,
            &mut win_y,
            &mut mask,
        );
        (xlib.XCloseDisplay)(display);
    }

    if root_x == -1 && root_y == -1 {
        return None;
    }
    Some((root_x, root_y))
}

#[cfg(target_os = "windows")]
fn platform_get_cursor_position() -> Option<(i32, i32)> {
    // TODO: implement using GetCursorPos() from winapi/windows-sys
    None
}

#[cfg(target_os = "macos")]
fn platform_get_cursor_position() -> Option<(i32, i32)> {
    // TODO: implement using CGEventGetLocation() from core-graphics
    None
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn platform_get_cursor_position() -> Option<(i32, i32)> {
    None
}

// ── Desktop size ──────────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn platform_get_desktop_size() -> Option<(i32, i32)> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        wayland_get_desktop_size()
    } else {
        x11_get_desktop_size()
    }
}

/// Sum all active `wl_output` logical sizes to get the total desktop bounding box.
#[cfg(target_os = "linux")]
fn wayland_get_desktop_size() -> Option<(i32, i32)> {
    use wayland_client::{
        protocol::{wl_output, wl_registry},
        Connection, Dispatch, QueueHandle, WEnum,
    };

    #[derive(Default, Clone)]
    struct RawOutput {
        x: i32,
        y: i32,
        mode_w: i32,
        mode_h: i32,
        scale: i32,
    }

    #[derive(Default)]
    struct State {
        outputs: Vec<RawOutput>,
        pending: RawOutput,
    }

    impl Dispatch<wl_registry::WlRegistry, ()> for State {
        fn event(
            _state: &mut Self,
            registry: &wl_registry::WlRegistry,
            event: wl_registry::Event,
            _: &(),
            _: &Connection,
            qh: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name,
                interface,
                version,
            } = event
            {
                if interface == "wl_output" {
                    registry.bind::<wl_output::WlOutput, _, _>(name, version.min(4), qh, ());
                }
            }
        }
    }

    impl Dispatch<wl_output::WlOutput, ()> for State {
        fn event(
            state: &mut Self,
            _: &wl_output::WlOutput,
            event: wl_output::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                wl_output::Event::Geometry { x, y, .. } => {
                    state.pending.x = x;
                    state.pending.y = y;
                }
                wl_output::Event::Mode {
                    width,
                    height,
                    flags,
                    ..
                } => {
                    if let WEnum::Value(f) = flags {
                        if f.contains(wl_output::Mode::Current) {
                            state.pending.mode_w = width;
                            state.pending.mode_h = height;
                        }
                    }
                }
                wl_output::Event::Scale { factor } => {
                    state.pending.scale = factor;
                }
                wl_output::Event::Done => {
                    state.outputs.push(std::mem::take(&mut state.pending));
                }
                _ => {}
            }
        }
    }

    let conn = Connection::connect_to_env().ok()?;
    let mut eq = conn.new_event_queue::<State>();
    let qh = eq.handle();
    conn.display().get_registry(&qh, ());
    let mut state = State::default();
    eq.roundtrip(&mut state).ok()?;
    eq.roundtrip(&mut state).ok()?;

    if state.outputs.is_empty() {
        return None;
    }

    // Bounding box: max(x + logical_w) × max(y + logical_h)
    let w = state
        .outputs
        .iter()
        .map(|o| {
            let scale = if o.scale > 0 { o.scale } else { 1 };
            o.x + o.mode_w / scale
        })
        .max()?;
    let h = state
        .outputs
        .iter()
        .map(|o| {
            let scale = if o.scale > 0 { o.scale } else { 1 };
            o.y + o.mode_h / scale
        })
        .max()?;

    Some((w, h))
}

/// Get desktop size via X11 screen dimensions.
#[cfg(target_os = "linux")]
fn x11_get_desktop_size() -> Option<(i32, i32)> {
    use x11_dl::xlib;

    let xlib = xlib::Xlib::open().ok()?;
    let display = unsafe { (xlib.XOpenDisplay)(std::ptr::null()) };
    if display.is_null() {
        return None;
    }
    let screen = unsafe { (xlib.XDefaultScreen)(display) };
    let w = unsafe { (xlib.XDisplayWidth)(display, screen) };
    let h = unsafe { (xlib.XDisplayHeight)(display, screen) };
    unsafe { (xlib.XCloseDisplay)(display) };
    Some((w, h))
}

#[cfg(target_os = "windows")]
fn platform_get_desktop_size() -> Option<(i32, i32)> {
    // TODO: GetSystemMetrics(SM_CXVIRTUALSCREEN) / SM_CYVIRTUALSCREEN
    None
}

#[cfg(target_os = "macos")]
fn platform_get_desktop_size() -> Option<(i32, i32)> {
    // TODO: CGDisplayBounds for all displays
    None
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn platform_get_desktop_size() -> Option<(i32, i32)> {
    None
}
