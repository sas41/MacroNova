/// Mouse injection via the XDG RemoteDesktop portal + EIS (libei).
///
/// Architecture:
///   A single background thread owns the EIS `ei::Context` socket and runs a
///   blocking read/poll loop. Rhai script calls post `MouseCmd` messages
///   through a `std::sync::mpsc::SyncSender`. The worker drains both the
///   command channel and the EIS event stream in a tight loop.
///
/// Portal setup (async, runs once at init):
///   A temporary tokio current-thread runtime establishes the XDG
///   RemoteDesktop portal session and calls `ConnectToEIS` to obtain the EIS
///   socket fd. KDE shows a one-time permission dialog on first use; subsequent
///   daemon launches reuse the approved session automatically.
///
/// On failure (portal unavailable, user denied, etc.) `MouseInjector::new()`
/// returns an injector with `tx = None`; all mouse calls are silent no-ops.
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use reis::{ei, handshake, PendingRequestResult};
use tracing::{debug, error, info, warn};

// ── Commands from Rhai threads → EIS worker ───────────────────────────────────

#[derive(Debug)]
enum MouseCmd {
    /// Press or release a button (Linux evdev button code).
    Button { code: u32, press: bool },
    /// Relative motion in pixels.
    MoveRel { dx: f32, dy: f32 },
    /// Absolute motion in pixels.
    MoveAbs { x: f32, y: f32 },
    /// Discrete (wheel) vertical scroll. One unit = 120 in EIS convention.
    ScrollDiscreteV { clicks: i32 },
    /// Discrete (wheel) horizontal scroll.
    ScrollDiscreteH { clicks: i32 },
}

// ── Per-device bookkeeping ────────────────────────────────────────────────────

#[derive(Default)]
struct DeviceData {
    interfaces: HashMap<String, reis::Object>,
    device_type: Option<ei::device::DeviceType>,
    emulating: bool,
}

impl DeviceData {
    fn interface<T: reis::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }
}

#[derive(Default)]
struct SeatData {
    capabilities: HashMap<String, u64>,
}

// ── Worker ────────────────────────────────────────────────────────────────────

struct Worker {
    context: ei::Context,
    seats: HashMap<ei::Seat, SeatData>,
    devices: HashMap<ei::Device, DeviceData>,
    last_serial: u32,
    sequence: u32,
    time_origin: Instant,
    cmd_rx: mpsc::Receiver<MouseCmd>,
    /// Signalled once the first emulating device has been confirmed by the EIS
    /// server, so that `try_new()` can block until the injector is usable.
    ready: Arc<(Mutex<bool>, Condvar)>,
}

impl Worker {
    fn timestamp_us(&self) -> u64 {
        self.time_origin.elapsed().as_micros() as u64
    }

    /// Drain all pending EIS events without blocking.
    fn drain_eis(&mut self) {
        if self.context.read().is_err() {
            return;
        }
        while let Some(result) = self.context.pending_event() {
            let event = match result {
                PendingRequestResult::Request(e) => e,
                PendingRequestResult::ParseError(msg) => {
                    warn!("EIS parse error: {msg}");
                    continue;
                }
                PendingRequestResult::InvalidObject(id) => {
                    warn!("EIS invalid object id={id}");
                    continue;
                }
            };
            self.handle_event(event);
        }
        let _ = self.context.flush();
    }

    fn handle_event(&mut self, event: ei::Event) {
        match event {
            ei::Event::Connection(_, req) => match req {
                ei::connection::Event::Seat { seat } => {
                    self.seats.insert(seat, SeatData::default());
                }
                ei::connection::Event::Ping { ping } => {
                    if ping.is_alive() {
                        ping.done(0);
                        let _ = self.context.flush();
                    }
                }
                ei::connection::Event::Disconnected { reason, explanation, .. } => {
                    warn!("EIS disconnected: reason={reason:?} {explanation:?}");
                }
                _ => {}
            },

            ei::Event::Seat(seat, req) => {
                let Some(data) = self.seats.get_mut(&seat) else { return };
                match req {
                    ei::seat::Event::Capability { mask, interface } => {
                        data.capabilities.insert(interface, mask);
                    }
                    ei::seat::Event::Done => {
                        let bitmask: u64 =
                            data.capabilities.values().copied().fold(0, |a, b| a | b);
                        seat.bind(bitmask);
                        let _ = self.context.flush();
                        debug!("EIS: bound seat capability mask={bitmask:#x}");
                    }
                    ei::seat::Event::Device { device } => {
                        self.devices.insert(device, DeviceData::default());
                    }
                    ei::seat::Event::Destroyed { .. } => {
                        self.seats.remove(&seat);
                    }
                    _ => {}
                }
            }

            ei::Event::Device(device, req) => {
                let Some(data) = self.devices.get_mut(&device) else { return };
                match req {
                    ei::device::Event::DeviceType { device_type } => {
                        data.device_type = Some(device_type);
                    }
                    ei::device::Event::Interface { object } => {
                        data.interfaces.insert(object.interface().to_string(), object);
                    }
                    ei::device::Event::Done => {
                        debug!("EIS: device done type={:?}", data.device_type);
                        if data.device_type == Some(ei::device::DeviceType::Virtual) {
                            device.start_emulating(self.last_serial, self.sequence);
                            self.sequence = self.sequence.wrapping_add(1);
                            data.emulating = true;
                            let _ = self.context.flush();
                            debug!("EIS: start_emulating (Done)");
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        self.last_serial = serial;
                        if !data.emulating
                            && data.device_type == Some(ei::device::DeviceType::Virtual)
                        {
                            device.start_emulating(self.last_serial, self.sequence);
                            self.sequence = self.sequence.wrapping_add(1);
                            data.emulating = true;
                            let _ = self.context.flush();
                            debug!("EIS: start_emulating (Resumed)");
                            // Signal the main thread that at least one device is ready.
                            let (lock, cvar) = &*self.ready;
                            *lock.lock().unwrap() = true;
                            cvar.notify_all();
                        }
                    }
                    ei::device::Event::Paused { serial } => {
                        self.last_serial = serial;
                        data.emulating = false;
                    }
                    ei::device::Event::Destroyed { .. } => {
                        self.devices.remove(&device);
                    }
                    _ => {}
                }
            }

            // Keyboard events arrive (e.g. keymap) but we don't use them.
            ei::Event::Keyboard(_, _) => {}

            _ => {}
        }
    }

    fn execute(&mut self, cmd: MouseCmd) {
        let ts = self.timestamp_us();
        let serial = self.last_serial;

        match cmd {
            MouseCmd::Button { code, press } => {
                let state = if press {
                    ei::button::ButtonState::Press
                } else {
                    ei::button::ButtonState::Released
                };
                // Use the relative-pointer device for buttons (it has ei_pointer
                // + ei_button). Prefer it over the absolute device to avoid
                // sending duplicate clicks.
                let sent = self
                    .devices
                    .iter()
                    .find(|(_, d)| d.emulating && d.interface::<ei::Pointer>().is_some())
                    .or_else(|| {
                        self.devices
                            .iter()
                            .find(|(_, d)| d.emulating && d.interface::<ei::Button>().is_some())
                    });
                if let Some((device, data)) = sent {
                    let btn = data.interface::<ei::Button>().unwrap();
                    btn.button(code, state);
                    device.frame(serial, ts);
                    self.sequence = self.sequence.wrapping_add(1);
                    debug!("EIS: button code={code:#x} press={press}");
                } else {
                    warn!("EIS: no emulating device with ei_button — click dropped");
                }
            }

            MouseCmd::MoveRel { dx, dy } => {
                if let Some((device, data)) = self
                    .devices
                    .iter()
                    .find(|(_, d)| d.emulating && d.interface::<ei::Pointer>().is_some())
                {
                    data.interface::<ei::Pointer>().unwrap().motion_relative(dx, dy);
                    device.frame(serial, ts);
                    self.sequence = self.sequence.wrapping_add(1);
                }
            }

            MouseCmd::MoveAbs { x, y } => {
                if let Some((device, data)) = self
                    .devices
                    .iter()
                    .find(|(_, d)| d.emulating && d.interface::<ei::PointerAbsolute>().is_some())
                {
                    data.interface::<ei::PointerAbsolute>().unwrap().motion_absolute(x, y);
                    device.frame(serial, ts);
                    self.sequence = self.sequence.wrapping_add(1);
                }
            }

            MouseCmd::ScrollDiscreteV { clicks } => {
                // Prefer the relative-pointer device for scroll too.
                let found = self
                    .devices
                    .iter()
                    .find(|(_, d)| d.emulating && d.interface::<ei::Pointer>().is_some())
                    .or_else(|| {
                        self.devices
                            .iter()
                            .find(|(_, d)| d.emulating && d.interface::<ei::Scroll>().is_some())
                    });
                if let Some((device, data)) = found {
                    data.interface::<ei::Scroll>().unwrap().scroll_discrete(0, clicks * 120);
                    device.frame(serial, ts);
                    self.sequence = self.sequence.wrapping_add(1);
                }
            }

            MouseCmd::ScrollDiscreteH { clicks } => {
                let found = self
                    .devices
                    .iter()
                    .find(|(_, d)| d.emulating && d.interface::<ei::Pointer>().is_some())
                    .or_else(|| {
                        self.devices
                            .iter()
                            .find(|(_, d)| d.emulating && d.interface::<ei::Scroll>().is_some())
                    });
                if let Some((device, data)) = found {
                    data.interface::<ei::Scroll>().unwrap().scroll_discrete(clicks * 120, 0);
                    device.frame(serial, ts);
                    self.sequence = self.sequence.wrapping_add(1);
                }
            }
        }

        let _ = self.context.flush();
    }

    fn run(mut self) {
        use std::os::unix::io::AsRawFd;
        let eis_fd = self.context.as_raw_fd();

        loop {
            // Drain commands (non-blocking).
            loop {
                match self.cmd_rx.try_recv() {
                    Ok(cmd) => {
                        self.drain_eis(); // refresh device state first
                        self.execute(cmd);
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        info!("EIS worker: command channel closed, exiting");
                        return;
                    }
                }
            }

            // Wait up to 50 ms for the EIS socket to become readable.
            let mut pfd = libc::pollfd {
                fd: eis_fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: pfd is a valid stack-allocated pollfd.
            let ret = unsafe { libc::poll(std::ptr::addr_of_mut!(pfd), 1, 50) };
            if ret < 0 {
                let err = io::Error::last_os_error();
                if err.kind() != io::ErrorKind::Interrupted {
                    error!("EIS worker poll: {err}");
                }
                continue;
            }
            if ret > 0 {
                self.drain_eis();
            }
        }
    }
}

// ── Portal setup ──────────────────────────────────────────────────────────────

async fn open_eis_connection() -> anyhow::Result<ei::Context> {
    use ashpd::desktop::{
        remote_desktop::{
            ConnectToEISOptions, DeviceType, RemoteDesktop, SelectDevicesOptions,
        },
        PersistMode,
    };
    use std::os::unix::io::{FromRawFd, IntoRawFd};

    // Use a direct EIS socket if the environment provides one (testing/dev).
    if let Some(ctx) = ei::Context::connect_to_env()
        .map_err(|e| anyhow::anyhow!("connect_to_env: {e}"))?
    {
        info!("EIS: connected via LIBEI_SOCKET");
        return Ok(ctx);
    }

    info!("EIS: opening XDG RemoteDesktop portal session (KDE will prompt once for permission)");

    let proxy = RemoteDesktop::new()
        .await
        .map_err(|e| anyhow::anyhow!("RemoteDesktop::new: {e}"))?;

    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| anyhow::anyhow!("create_session: {e}"))?;

    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                .set_persist_mode(PersistMode::Application),
        )
        .await
        .map_err(|e| anyhow::anyhow!("select_devices: {e}"))?;

    proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| anyhow::anyhow!("start: {e}"))?;

    let owned_fd = proxy
        .connect_to_eis(&session, ConnectToEISOptions::default())
        .await
        .map_err(|e| anyhow::anyhow!("connect_to_eis: {e}"))?;

    // SAFETY: owned_fd is the sole owner of a valid open file descriptor.
    let stream =
        unsafe { UnixStream::from_raw_fd(owned_fd.into_raw_fd()) };
    stream
        .set_nonblocking(true)
        .map_err(|e| anyhow::anyhow!("set_nonblocking: {e}"))?;

    ei::Context::new(stream).map_err(|e| anyhow::anyhow!("ei::Context::new: {e}"))
}

fn connect_blocking() -> anyhow::Result<ei::Context> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| anyhow::anyhow!("tokio runtime: {e}"))?
        .block_on(open_eis_connection())
}

// ── Public API ────────────────────────────────────────────────────────────────

pub struct MouseInjector {
    tx: Option<mpsc::SyncSender<MouseCmd>>,
}

impl std::fmt::Debug for MouseInjector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseInjector")
            .field("available", &self.tx.is_some())
            .finish()
    }
}

impl MouseInjector {
    pub fn new() -> Self {
        match Self::try_new() {
            Ok(inj) => inj,
            Err(e) => {
                warn!(
                    "Mouse injector unavailable: {e:#}  \
                     (ensure DBUS_SESSION_BUS_ADDRESS and WAYLAND_DISPLAY are set). \
                     Mouse functions will be no-ops."
                );
                Self { tx: None }
            }
        }
    }

    fn try_new() -> anyhow::Result<Self> {
        let context = connect_blocking()?;

        // Synchronous EIS handshake — must complete before the worker starts.
        handshake::ei_handshake_blocking(&context, "macronova", ei::handshake::ContextType::Sender)
            .map_err(|e| anyhow::anyhow!("EIS handshake: {e}"))?;

        context
            .flush()
            .map_err(|_| anyhow::anyhow!("EIS initial flush failed"))?;

        info!("EIS handshake complete; spawning mouse worker thread");

        let (tx, cmd_rx) = mpsc::sync_channel::<MouseCmd>(64);
        let ready = Arc::new((Mutex::new(false), Condvar::new()));

        let worker = Worker {
            context,
            seats: HashMap::new(),
            devices: HashMap::new(),
            last_serial: 0,
            sequence: 0,
            time_origin: Instant::now(),
            cmd_rx,
            ready: Arc::clone(&ready),
        };

        std::thread::Builder::new()
            .name("macronova-eis".to_string())
            .spawn(move || worker.run())
            .map_err(|e| anyhow::anyhow!("spawn EIS worker: {e}"))?;

        // Block until the EIS server has sent at least one Resumed device (seat
        // enumeration is complete) or until a 3-second timeout elapses. The
        // timeout ensures the daemon still starts if the session is slow.
        let (lock, cvar) = &*ready;
        let result = cvar
            .wait_timeout_while(
                lock.lock().unwrap(),
                Duration::from_secs(3),
                |ready| !*ready,
            )
            .unwrap();
        if result.1.timed_out() {
            warn!("EIS: timed out waiting for device enumeration — clicks may be dropped until devices arrive");
        } else {
            info!("EIS: device ready");
        }

        Ok(Self { tx: Some(tx) })
    }

    fn send(&self, cmd: MouseCmd) {
        if let Some(tx) = &self.tx {
            if let Err(e) = tx.try_send(cmd) {
                warn!("Mouse command dropped: {e}");
            }
        }
    }

    // ── Methods called from Rhai ─────────────────────────────────────────────

    pub fn click(&self, button: u32) {
        let code = x11_to_evdev_button(button);
        self.send(MouseCmd::Button { code, press: true });
        self.send(MouseCmd::Button { code, press: false });
    }

    pub fn button_event(&self, button: u32, press: bool) {
        let code = x11_to_evdev_button(button);
        self.send(MouseCmd::Button { code, press });
    }

    pub fn move_rel(&self, dx: i32, dy: i32) {
        self.send(MouseCmd::MoveRel { dx: dx as f32, dy: dy as f32 });
    }

    pub fn move_abs(&self, x: i32, y: i32) {
        self.send(MouseCmd::MoveAbs { x: x as f32, y: y as f32 });
    }

    pub fn scroll(&self, amount: i32) {
        self.send(MouseCmd::ScrollDiscreteV { clicks: amount });
    }

    pub fn hscroll(&self, amount: i32) {
        self.send(MouseCmd::ScrollDiscreteH { clicks: amount });
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert X11 button number → Linux evdev button code.
fn x11_to_evdev_button(button: u32) -> u32 {
    match button {
        1 => 0x110, // BTN_LEFT
        2 => 0x112, // BTN_MIDDLE
        3 => 0x111, // BTN_RIGHT
        8 => 0x116, // BTN_SIDE  (back)
        9 => 0x115, // BTN_EXTRA (forward)
        _ => {
            warn!("Unknown X11 button {button}, defaulting to BTN_LEFT");
            0x110
        }
    }
}

/// Resolve a human-friendly button name → X11 button number.
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
