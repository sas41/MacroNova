/// mouse-test: standalone EIS click test.
///
/// Connects to the XDG RemoteDesktop portal, waits 3 seconds for device
/// enumeration, then sends a left-click and exits.
///
/// Usage:
///   RUST_LOG=debug cargo run --bin mouse-test
///
/// Watch the output carefully:
///   - "EIS: bound seat" → portal accepted and seat is ready
///   - "EIS: device done / start_emulating" → button device is live
///   - "EIS: button code=0x110 press=true/false" → click sent
///   - Any error lines tell you what failed
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use reis::{ei, handshake, PendingRequestResult};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("debug".parse().unwrap()),
        )
        .init();

    eprintln!("=== mouse-test: connecting to EIS portal ===");

    let context = connect_blocking().expect("failed to connect to EIS");

    eprintln!("=== portal connect ok; running EIS handshake ===");

    handshake::ei_handshake_blocking(&context, "macronova-test", ei::handshake::ContextType::Sender)
        .expect("EIS handshake failed");

    context.flush().expect("initial flush failed");

    eprintln!("=== handshake ok; waiting 3s for device enumeration ===");

    // Run the event loop for 3 seconds so KDE can send us the seat + devices.
    let mut seats: HashMap<ei::Seat, SeatData> = HashMap::new();
    let mut devices: HashMap<ei::Device, DeviceData> = HashMap::new();
    let mut last_serial = 0u32;
    let mut sequence = 0u32;

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        drain(&context, &mut seats, &mut devices, &mut last_serial, &mut sequence);
        std::thread::sleep(Duration::from_millis(50));
    }

    // Report what we found.
    eprintln!("=== after 3s: {} seat(s), {} device(s) ===", seats.len(), devices.len());
    for (_, d) in &devices {
        eprintln!(
            "  device name={:?} type={:?} emulating={} interfaces=[{}]",
            d.name,
            d.device_type,
            d.emulating,
            d.interfaces.keys().cloned().collect::<Vec<_>>().join(", ")
        );
    }

    // Try a left click — use only the relative-pointer device (has ei_pointer +
    // ei_button). Fall back to any device with ei_button if needed.
    let ts = Instant::now().elapsed().as_micros() as u64;
    let target = devices
        .iter()
        .find(|(_, d)| d.emulating && d.interface::<ei::Pointer>().is_some())
        .or_else(|| devices.iter().find(|(_, d)| d.emulating && d.interface::<ei::Button>().is_some()));

    let clicked = if let Some((device, data)) = target {
        let btn = data.interface::<ei::Button>().unwrap();
        eprintln!("  >>> sending BTN_LEFT press on device {:?}", data.name);
        btn.button(0x110, ei::button::ButtonState::Press);
        device.frame(last_serial, ts);
        sequence += 1;
        context.flush().ok();

        std::thread::sleep(Duration::from_millis(20));

        eprintln!("  >>> sending BTN_LEFT release on device {:?}", data.name);
        btn.button(0x110, ei::button::ButtonState::Released);
        device.frame(last_serial, ts + 20_000);
        sequence += 1;
        context.flush().ok();
        true
    } else {
        false
    };

    if !clicked {
        eprintln!("!!! NO device with ei_button interface found — click was not sent !!!");
        eprintln!("    Possible causes:");
        eprintln!("    - Portal permission not granted (check KDE system settings)");
        eprintln!("    - EIS server did not send a pointer/button virtual device");
        eprintln!("    - select_devices did not include DeviceType::Pointer");
    } else {
        eprintln!("=== click sent ===");
    }

    // Drain once more to catch any error responses.
    std::thread::sleep(Duration::from_millis(200));
    drain(&context, &mut seats, &mut devices, &mut last_serial, &mut sequence);
}

// ── Minimal EIS event loop ────────────────────────────────────────────────────

#[derive(Default)]
struct SeatData {
    capabilities: HashMap<String, u64>,
}

struct DeviceData {
    name: Option<String>,
    device_type: Option<ei::device::DeviceType>,
    interfaces: HashMap<String, reis::Object>,
    emulating: bool,
}

impl Default for DeviceData {
    fn default() -> Self {
        Self {
            name: None,
            device_type: None,
            interfaces: HashMap::new(),
            emulating: false,
        }
    }
}

impl DeviceData {
    fn interface<T: reis::Interface>(&self) -> Option<T> {
        self.interfaces.get(T::NAME)?.clone().downcast()
    }
}

fn drain(
    context: &ei::Context,
    seats: &mut HashMap<ei::Seat, SeatData>,
    devices: &mut HashMap<ei::Device, DeviceData>,
    last_serial: &mut u32,
    sequence: &mut u32,
) {
    if context.read().is_err() {
        return;
    }
    while let Some(result) = context.pending_event() {
        let event = match result {
            PendingRequestResult::Request(e) => e,
            PendingRequestResult::ParseError(msg) => {
                eprintln!("EIS parse error: {msg}");
                continue;
            }
            PendingRequestResult::InvalidObject(id) => {
                eprintln!("EIS invalid object {id}");
                continue;
            }
        };

        match event {
            ei::Event::Connection(_, req) => match req {
                ei::connection::Event::Seat { seat } => {
                    eprintln!("EIS: got seat");
                    seats.insert(seat, SeatData::default());
                }
                ei::connection::Event::Ping { ping } => {
                    if ping.is_alive() {
                        ping.done(0);
                        context.flush().ok();
                    }
                }
                ei::connection::Event::Disconnected { reason, explanation, .. } => {
                    eprintln!("EIS: DISCONNECTED reason={reason:?} {explanation:?}");
                }
                _ => {}
            },
            ei::Event::Seat(seat, req) => {
                let Some(data) = seats.get_mut(&seat) else { continue };
                match req {
                    ei::seat::Event::Capability { mask, interface } => {
                        eprintln!("EIS: seat capability interface={interface} mask={mask:#x}");
                        data.capabilities.insert(interface, mask);
                    }
                    ei::seat::Event::Done => {
                        let bitmask: u64 =
                            data.capabilities.values().copied().fold(0, |a, b| a | b);
                        eprintln!("EIS: seat Done — binding mask={bitmask:#x}");
                        seat.bind(bitmask);
                        context.flush().ok();
                    }
                    ei::seat::Event::Device { device } => {
                        eprintln!("EIS: seat sent device");
                        devices.insert(device, DeviceData::default());
                    }
                    _ => {}
                }
            }
            ei::Event::Device(device, req) => {
                let Some(data) = devices.get_mut(&device) else { continue };
                match req {
                    ei::device::Event::Name { name } => {
                        eprintln!("EIS: device name={name:?}");
                        data.name = Some(name);
                    }
                    ei::device::Event::DeviceType { device_type } => {
                        eprintln!("EIS: device type={device_type:?}");
                        data.device_type = Some(device_type);
                    }
                    ei::device::Event::Interface { object } => {
                        let iface = object.interface().to_string();
                        eprintln!("EIS: device interface={iface}");
                        data.interfaces.insert(iface, object);
                    }
                    ei::device::Event::Done => {
                        eprintln!("EIS: device Done — type={:?}", data.device_type);
                        if data.device_type == Some(ei::device::DeviceType::Virtual) {
                            device.start_emulating(*last_serial, *sequence);
                            *sequence += 1;
                            data.emulating = true;
                            context.flush().ok();
                            eprintln!("EIS: start_emulating sent");
                        }
                    }
                    ei::device::Event::Resumed { serial } => {
                        eprintln!("EIS: device Resumed serial={serial}");
                        *last_serial = serial;
                        if !data.emulating
                            && data.device_type == Some(ei::device::DeviceType::Virtual)
                        {
                            device.start_emulating(*last_serial, *sequence);
                            *sequence += 1;
                            data.emulating = true;
                            context.flush().ok();
                            eprintln!("EIS: start_emulating sent (Resumed)");
                        }
                    }
                    ei::device::Event::Paused { serial } => {
                        eprintln!("EIS: device Paused serial={serial}");
                        *last_serial = serial;
                        data.emulating = false;
                    }
                    ei::device::Event::Destroyed { .. } => {
                        devices.remove(&device);
                    }
                    _ => {}
                }
            }
            ei::Event::Keyboard(_, _) => {}
            _ => {}
        }
    }
    context.flush().ok();
}

// ── Portal setup ──────────────────────────────────────────────────────────────

async fn open_eis() -> anyhow::Result<ei::Context> {
    use ashpd::desktop::{
        remote_desktop::{ConnectToEISOptions, DeviceType, RemoteDesktop, SelectDevicesOptions},
        PersistMode,
    };
    use std::os::unix::io::{FromRawFd, IntoRawFd};

    if let Some(ctx) = ei::Context::connect_to_env()
        .map_err(|e| anyhow::anyhow!("{e}"))?
    {
        eprintln!("EIS: using LIBEI_SOCKET");
        return Ok(ctx);
    }

    eprintln!("EIS: calling XDG RemoteDesktop portal …");
    let proxy = RemoteDesktop::new().await.map_err(|e| anyhow::anyhow!("{e}"))?;
    let session = proxy.create_session(Default::default()).await.map_err(|e| anyhow::anyhow!("{e}"))?;
    eprintln!("EIS: session created");

    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(DeviceType::Keyboard | DeviceType::Pointer)
                .set_persist_mode(PersistMode::Application),
        )
        .await
        .map_err(|e| anyhow::anyhow!("select_devices: {e}"))?;
    eprintln!("EIS: devices selected (Keyboard | Pointer)");

    proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| anyhow::anyhow!("start: {e}"))?;
    eprintln!("EIS: session started (portal dialog accepted)");

    let owned_fd = proxy
        .connect_to_eis(&session, ConnectToEISOptions::default())
        .await
        .map_err(|e| anyhow::anyhow!("connect_to_eis: {e}"))?;
    eprintln!("EIS: got fd");

    let stream = unsafe { UnixStream::from_raw_fd(owned_fd.into_raw_fd()) };
    stream.set_nonblocking(true).map_err(|e| anyhow::anyhow!("{e}"))?;
    ei::Context::new(stream).map_err(|e| anyhow::anyhow!("{e}"))
}

fn connect_blocking() -> anyhow::Result<ei::Context> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(open_eis())
}
