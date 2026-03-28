use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::CreationContext;
use egui::{CentralPanel, Context, TopBottomPanel};
use macronova_core::{
    config::{default_config_path, Config},
    device::{
        evdev_input::{DeviceEvent, EvdevReader},
        logitech::discover_devices,
        DeviceInfo,
    },
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::warn;

use crate::views::{
    bindings::BindingsView, daemon::DaemonView, devices::DevicesView, editor::EditorView,
    settings::SettingsView,
};

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Tab {
    Devices,
    Bindings,
    Editor,
    Daemon,
    Settings,
}

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
    pub settings_view: SettingsView,

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
        let btn_tx_evdev = btn_tx.clone();
        std::thread::Builder::new()
            .name("evdev-preview".into())
            .spawn(move || evdev_preview_thread(btn_tx_evdev))
            .ok();

        let bindings_view = BindingsView::new(config.clone());
        let editor_view = EditorView::new();
        let devices_view = DevicesView::new(devices.clone());
        let daemon_view = DaemonView::new();
        let settings_view = SettingsView::new();

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
            settings_view,
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
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
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
            Tab::Settings => {
                if let Some(updated_config) = self.settings_view.show(ui, &self.config) {
                    self.config = updated_config;
                    self.save_config();
                }
            }
        });
    }
}

struct PreviewReader {
    device_id: String,
    reader: EvdevReader,
}

fn path_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

fn try_open_evdev_readers() -> Vec<PreviewReader> {
    let cfg = match Config::load_default() {
        Ok(c) => c,
        Err(e) => {
            warn!("evdev preview: failed to load config: {e}");
            return Vec::new();
        }
    };

    let mut readers = Vec::new();
    for dev in cfg.devices {
        let mut pairs = vec![(dev.mouse_path.clone(), path_label(&dev.mouse_path))];
        if let Some(kbd_path) = dev.kbd_path.as_ref() {
            if !kbd_path.is_empty() {
                pairs.push((kbd_path.clone(), path_label(kbd_path)));
            }
        }
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(p, l)| (p.as_str(), l.as_str()))
            .collect();
        match EvdevReader::open(&refs) {
            Ok(reader) => readers.push(PreviewReader {
                device_id: dev.id,
                reader,
            }),
            Err(e) => warn!("evdev preview: failed to open '{}': {e}", dev.display_name),
        }
    }

    readers
}

fn evdev_preview_thread(tx: mpsc::Sender<LiveButtonEvent>) {
    let mut reconnect_delay = Duration::from_millis(500);

    let mut readers = loop {
        let open = try_open_evdev_readers();
        if !open.is_empty() {
            break open;
        }
        std::thread::sleep(reconnect_delay);
        reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
    };
    reconnect_delay = Duration::from_millis(500);

    loop {
        let mut had_event = false;
        let mut had_error = false;

        for active in &mut readers {
            match active.reader.poll(Duration::from_millis(10)) {
                Ok(Some(DeviceEvent::Passthrough(_))) => {
                    had_event = true;
                }
                Ok(Some(DeviceEvent::Button(btn_ev))) => {
                    had_event = true;
                    if tx
                        .send(LiveButtonEvent {
                            button: format!("{}::{}", active.device_id, btn_ev.button.name()),
                            pressed: btn_ev.pressed,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!(
                        "evdev preview: read error on '{}': {e} — reconnecting",
                        active.device_id
                    );
                    had_error = true;
                    break;
                }
            }
        }

        if had_error {
            readers.clear();
            readers = loop {
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
                let open = try_open_evdev_readers();
                if !open.is_empty() {
                    reconnect_delay = Duration::from_millis(500);
                    break open;
                }
            };
            continue;
        }

        if !had_event {
            std::thread::sleep(Duration::from_millis(2));
        } else {
            reconnect_delay = Duration::from_millis(500);
        }
    }
}
