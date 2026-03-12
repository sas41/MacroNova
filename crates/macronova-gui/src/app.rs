use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::CreationContext;
use egui::{CentralPanel, Context, TopBottomPanel};
use macronova_core::{
    config::{default_config_path, Config},
    device::{
        evdev_input::{discover_evdev_paths, ButtonEvent, EvdevPaths, EvdevReader},
        logitech::discover_devices,
        DeviceInfo,
    },
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::warn;

use crate::views::{
    bindings::BindingsView, daemon::DaemonView, devices::DevicesView, editor::EditorView,
};

/// Which tab is currently active.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Devices,
    Bindings,
    Editor,
    Daemon,
}

/// Live button state fed from background threads to the UI.
/// `button` is the canonical button name string (e.g. `"event5/key0x00c4"` or `"cid/0x00c4"`).
#[derive(Debug, Clone)]
pub struct LiveButtonEvent {
    pub button: String,
    pub pressed: bool,
}

pub struct MacroNovaApp {
    pub tab: Tab,
    pub devices: Vec<DeviceInfo>,
    pub config: Config,
    pub config_path: std::path::PathBuf,

    pub button_rx: mpsc::Receiver<LiveButtonEvent>,
    pub last_pressed: Option<String>,

    pub devices_view: DevicesView,
    pub bindings_view: BindingsView,
    pub editor_view: EditorView,
    pub daemon_view: DaemonView,

    pub last_device_scan: Instant,
    pub status_message: Option<String>,
    _watcher: Option<RecommendedWatcher>,
    config_rx: mpsc::Receiver<notify::Result<Event>>,
}

impl MacroNovaApp {
    pub fn new(_cc: &CreationContext) -> Self {
        let config_path = default_config_path();
        let config = Config::load_default().unwrap_or_default();
        let devices = discover_devices().unwrap_or_default();

        let (cfg_tx, cfg_rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = RecommendedWatcher::new(cfg_tx, notify::Config::default()).ok();
        if let Some(ref mut w) = watcher {
            let config_dir = macronova_core::config::default_config_dir();
            if config_dir.exists() {
                let _ = w.watch(&config_dir, RecursiveMode::Recursive);
            }
        }

        let (btn_tx, btn_rx) = mpsc::channel::<LiveButtonEvent>();

        // Evdev preview thread — always re-discovers paths on (re)connect.
        let btn_tx_evdev = btn_tx.clone();
        std::thread::Builder::new()
            .name("evdev-preview".into())
            .spawn(move || evdev_preview_thread(btn_tx_evdev))
            .ok();

        // HID++ preview thread — produces cid/0x... names for diverted buttons.
        let btn_tx_hidpp = btn_tx.clone();
        std::thread::Builder::new()
            .name("hidpp-preview".into())
            .spawn(move || hidpp_preview_thread(btn_tx_hidpp))
            .ok();

        let bindings_view = BindingsView::new(config.clone());
        let editor_view = EditorView::new();
        let devices_view = DevicesView::new(devices.clone());
        let daemon_view = DaemonView::new();

        Self {
            tab: Tab::Bindings,
            devices,
            config,
            config_path,
            button_rx: btn_rx,
            last_pressed: None,
            devices_view,
            bindings_view,
            editor_view,
            daemon_view,
            last_device_scan: Instant::now(),
            status_message: None,
            _watcher: watcher,
            config_rx: cfg_rx,
        }
    }

    pub fn rescan_devices(&mut self) {
        self.devices = discover_devices().unwrap_or_default();
        self.devices_view.update_devices(self.devices.clone());
        self.last_device_scan = Instant::now();
    }

    pub fn save_config(&mut self) {
        match self.config.save(&self.config_path) {
            Ok(_) => self.status_message = Some("Config saved.".into()),
            Err(e) => self.status_message = Some(format!("Save failed: {}", e)),
        }
    }

    fn poll_config_watcher(&mut self) {
        while let Ok(Ok(event)) = self.config_rx.try_recv() {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if event.paths.iter().any(|p| p == &self.config_path) {
                    match Config::load(&self.config_path) {
                        Ok(new_cfg) => {
                            self.config = new_cfg.clone();
                            self.bindings_view.update_config(new_cfg);
                            self.status_message = Some("Config reloaded from disk.".into());
                        }
                        Err(e) => warn!("GUI: failed to reload config: {}", e),
                    }
                }
            }
        }
    }

    fn poll_button_events(&mut self) {
        while let Ok(ev) = self.button_rx.try_recv() {
            if ev.pressed {
                self.last_pressed = Some(ev.button.clone());
                self.bindings_view.on_button_event(ev.button);
            }
        }
    }
}

impl eframe::App for MacroNovaApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.poll_config_watcher();
        self.poll_button_events();

        if self.last_device_scan.elapsed() > Duration::from_secs(5) {
            self.rescan_devices();
        }

        ctx.request_repaint_after(Duration::from_millis(50));

        TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("MacroNova");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Bindings, "Bindings");
                ui.selectable_value(&mut self.tab, Tab::Editor, "Editor");
                ui.selectable_value(&mut self.tab, Tab::Devices, "Devices");
                ui.selectable_value(&mut self.tab, Tab::Daemon, "Daemon");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(ref msg) = self.status_message {
                        ui.label(egui::RichText::new(msg).color(egui::Color32::LIGHT_GREEN));
                    }
                });
            });
        });

        TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} device(s) detected", self.devices.len()));
                if let Some(ref btn) = self.last_pressed {
                    ui.separator();
                    ui.colored_label(egui::Color32::YELLOW, format!("Last pressed: {}", btn));
                }
            });
        });

        CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Bindings => {
                if let Some(updated_config) = self.bindings_view.show(ui, &self.config) {
                    self.config = updated_config;
                    self.save_config();
                }
            }
            Tab::Editor => {
                let saved = self.editor_view.show(ui, &self.config);
                if saved {
                    self.bindings_view.refresh_macro_files();
                }
            }
            Tab::Devices => {
                self.devices_view.show(ui);
            }
            Tab::Daemon => {
                self.daemon_view.show(ui);
            }
        });
    }
}

/// Try to open an evdev reader, discovering paths fresh each call.
/// Returns `None` if the device is not currently present.
fn try_open_evdev_reader() -> Option<EvdevReader> {
    let paths = discover_evdev_paths().or_else(|| {
        warn!("evdev preview: could not discover paths, trying event5/event6");
        Some(EvdevPaths {
            mouse_path: "/dev/input/event5".into(),
            kbd_path: "/dev/input/event6".into(),
            mouse_label: String::new(),
            kbd_label: String::new(),
        })
    })?;

    let devices: Vec<(&str, &str)> = if paths.kbd_path.is_empty() {
        vec![(paths.mouse_path.as_str(), paths.mouse_label.as_str())]
    } else {
        vec![
            (paths.mouse_path.as_str(), paths.mouse_label.as_str()),
            (paths.kbd_path.as_str(), paths.kbd_label.as_str()),
        ]
    };

    match EvdevReader::open(&devices) {
        Ok(r) => Some(r),
        Err(e) => {
            warn!(
                "evdev preview: failed to open ({}, {}): {e}",
                paths.mouse_path, paths.kbd_path
            );
            None
        }
    }
}

/// Background thread: read evdev button events and send canonical names to the UI.
/// Automatically reconnects when the device is unplugged and replugged.
fn evdev_preview_thread(tx: mpsc::Sender<LiveButtonEvent>) {
    // Always re-discover on (re)connect so we pick up the correct eventN after
    // a plug/unplug cycle and use the stable by-id label as the button prefix.
    let mut reconnect_delay = Duration::from_millis(500);

    let mut reader = loop {
        match try_open_evdev_reader() {
            Some(r) => break r,
            None => {
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
            }
        }
    };
    reconnect_delay = Duration::from_millis(500);

    loop {
        match reader.poll(Duration::from_millis(200)) {
            Ok(Some(ButtonEvent { button, pressed })) => {
                reconnect_delay = Duration::from_millis(500);
                if tx
                    .send(LiveButtonEvent {
                        button: button.name(),
                        pressed,
                    })
                    .is_err()
                {
                    // UI has gone away — exit thread.
                    break;
                }
            }
            Ok(None) => {
                reconnect_delay = Duration::from_millis(500);
            }
            Err(e) => {
                // Device was unplugged — drop dead reader and wait for reconnect.
                warn!("evdev preview: read error (device lost?): {e} — reconnecting");
                drop(reader);

                reader = loop {
                    std::thread::sleep(reconnect_delay);
                    reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));

                    match try_open_evdev_reader() {
                        Some(r) => {
                            warn!("evdev preview: device reconnected");
                            reconnect_delay = Duration::from_millis(500);
                            break r;
                        }
                        None => {}
                    }
                };
            }
        }
    }
}

/// Background thread: read HID++ REPROG_CONTROLS_V4 notifications and send
/// `cid/0xNNNN` button names to the UI.  This runs in parallel with the evdev
/// thread so the GUI capture feature sees both sources.
/// Automatically reconnects when the device is unplugged and replugged.
fn hidpp_preview_thread(tx: mpsc::Sender<LiveButtonEvent>) {
    use macronova_core::device::hidpp::{
        base::read_notification,
        constants::{Feature, LOGITECH_VENDOR_ID},
        decode_button_notification, FeatureTable,
    };
    use std::collections::{HashMap, HashSet};

    let mut reconnect_delay = Duration::from_millis(500);

    'reconnect: loop {
        // Find the HID++ vendor channel (re-scan every reconnect attempt).
        let api = match hidapi::HidApi::new() {
            Ok(a) => a,
            Err(e) => {
                warn!("hidpp preview: hidapi init failed: {e}");
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
                continue 'reconnect;
            }
        };

        let (hidraw_path, device_index) = {
            let mut found = None;
            for info in api.device_list() {
                let info: &hidapi::DeviceInfo = info;
                if info.vendor_id() != LOGITECH_VENDOR_ID {
                    continue;
                }
                if info.usage_page() != 0xFF00 {
                    continue;
                }
                if let Ok(p) = info.path().to_str() {
                    let name = info.product_string().unwrap_or("").to_lowercase();
                    let idx = if name.contains("receiver") {
                        0x01u8
                    } else {
                        0xFF
                    };
                    found = Some((p.to_string(), idx));
                    break;
                }
            }
            match found {
                Some(v) => v,
                None => {
                    // Device not yet available — wait and retry.
                    std::thread::sleep(reconnect_delay);
                    reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
                    continue 'reconnect;
                }
            }
        };

        let device = match api.open_path(
            std::ffi::CStr::from_bytes_with_nul(format!("{}\0", hidraw_path).as_bytes()).unwrap(),
        ) {
            Ok(d) => d,
            Err(e) => {
                warn!("hidpp preview: failed to open {hidraw_path}: {e}");
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
                continue 'reconnect;
            }
        };

        let features = match FeatureTable::query(&device, device_index) {
            Ok(f) => f,
            Err(e) => {
                warn!("hidpp preview: feature table query failed: {e}");
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
                continue 'reconnect;
            }
        };

        let reprog_feat_idx = match features.get_index(Feature::ReprogramControlsV4) {
            Some(i) => i,
            None => {
                warn!("hidpp preview: device does not support REPROG_CONTROLS_V4");
                // Non-recoverable for this device type — exit thread.
                return;
            }
        };

        reconnect_delay = Duration::from_millis(500); // reset after successful open
        let mut held: HashMap<u16, bool> = HashMap::new();

        loop {
            match read_notification(&device, Duration::from_millis(200)) {
                Ok(None) => {}
                Ok(Some(notif)) => {
                    if notif.feature_index != reprog_feat_idx || notif.software_id != 0 {
                        continue;
                    }

                    let cids = decode_button_notification(&notif.data);
                    let now_held: HashSet<u16> = cids.iter().copied().filter(|&c| c != 0).collect();

                    for &cid in &now_held {
                        if !held.contains_key(&cid) {
                            let name = format!("cid/0x{:04x}", cid);
                            if tx
                                .send(LiveButtonEvent {
                                    button: name,
                                    pressed: true,
                                })
                                .is_err()
                            {
                                return; // UI gone
                            }
                            held.insert(cid, true);
                        }
                    }

                    let released: Vec<u16> = held
                        .keys()
                        .copied()
                        .filter(|c| !now_held.contains(c))
                        .collect();
                    for cid in released {
                        let name = format!("cid/0x{:04x}", cid);
                        if tx
                            .send(LiveButtonEvent {
                                button: name,
                                pressed: false,
                            })
                            .is_err()
                        {
                            return; // UI gone
                        }
                        held.remove(&cid);
                    }
                }
                Err(e) => {
                    // Device lost — break inner loop to trigger reconnect.
                    warn!("hidpp preview: read error (device lost?): {e} — reconnecting");
                    break;
                }
            }
        }
        // Fall through to 'reconnect with backoff.
        std::thread::sleep(reconnect_delay);
        reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
    }
}
