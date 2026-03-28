mod engine;
mod input;

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context, Result};
use macronova_core::{
    config::{default_config_dir, Config, InputDeviceConfig},
    device::evdev_input::{DeviceEvent, EvdevReader, RawEvent},
    platform::input::{get_desktop_size, InputInjector},
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::input::PlatformInjector;

struct ActiveReader {
    device_id: String,
    reader: EvdevReader,
    grabbed: bool,
}

fn path_label(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

fn devices_for_open(cfg: &InputDeviceConfig) -> Vec<(String, String)> {
    let mut out = vec![(cfg.mouse_path.clone(), path_label(&cfg.mouse_path))];
    if let Some(kbd_path) = cfg.kbd_path.as_ref() {
        if !kbd_path.is_empty() {
            out.push((kbd_path.clone(), path_label(kbd_path)));
        }
    }
    out
}

fn open_readers(cfg: &Config) -> Vec<ActiveReader> {
    let mut readers = Vec::new();

    for dev in &cfg.devices {
        let pairs = devices_for_open(dev);
        let refs: Vec<(&str, &str)> = pairs
            .iter()
            .map(|(p, l)| (p.as_str(), l.as_str()))
            .collect();

        match EvdevReader::open(&refs) {
            Ok(reader) => {
                info!(
                    "opened device '{}' mouse='{}' kbd='{}'",
                    dev.id,
                    dev.mouse_path,
                    dev.kbd_path.clone().unwrap_or_default()
                );
                readers.push(ActiveReader {
                    device_id: dev.id.clone(),
                    reader,
                    grabbed: false,
                });
            }
            Err(e) => warn!("failed to open device '{}' : {e}", dev.id),
        }
    }

    readers
}

fn apply_grab_state(readers: &mut [ActiveReader], grab: bool) {
    for active in readers {
        match active.reader.grab(grab) {
            Ok(()) => active.grabbed = grab,
            Err(e) => {
                active.grabbed = false;
                warn!("EVIOCGRAB({grab}) failed on '{}': {e}", active.device_id);
            }
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("macronova_daemon=info".parse()?),
        )
        .init();

    let config_dir = default_config_dir();
    let config_path = config_dir.join("config.toml");
    let config = Arc::new(Mutex::new(Config::load_default()?));

    let (desktop_w, desktop_h) = get_desktop_size().unwrap_or((1920, 1080));
    let warp_mode = config.lock().unwrap().warp_mode;
    let injector: Arc<Mutex<dyn InputInjector>> = Arc::new(Mutex::new(
        PlatformInjector::new(desktop_w, desktop_h, warp_mode)
            .context("Failed to create platform input injector")?,
    ));

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    if config_dir.exists() {
        watcher.watch(&config_dir, RecursiveMode::Recursive)?;
    }

    let mut readers = {
        let cfg = config.lock().unwrap();
        open_readers(&cfg)
    };
    {
        let cfg = config.lock().unwrap();
        if cfg.virtual_mode {
            apply_grab_state(&mut readers, true);
        }
    }

    let mut active_held: HashMap<String, Arc<AtomicBool>> = HashMap::new();

    loop {
        while let Ok(Ok(event)) = rx.try_recv() {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                && event.paths.iter().any(|p| p == &config_path)
            {
                match Config::load(&config_path) {
                    Ok(new_cfg) => {
                        let needs_grab = new_cfg.virtual_mode;
                        *config.lock().unwrap() = new_cfg.clone();
                        readers = open_readers(&new_cfg);
                        if needs_grab {
                            apply_grab_state(&mut readers, true);
                        }
                        info!("Config reloaded");
                    }
                    Err(e) => warn!("Failed to reload config: {e}"),
                }
            }
        }

        let mut had_event = false;
        for active in &mut readers {
            match active.reader.poll(Duration::from_millis(10)) {
                Ok(Some(DeviceEvent::Passthrough(pt))) => {
                    had_event = true;
                    if active.grabbed {
                        if let Err(e) = injector.lock().unwrap().passthrough_raw(
                            pt.raw.ev_type,
                            pt.raw.code,
                            pt.raw.value,
                        ) {
                            warn!("passthrough_raw failed: {e}");
                        }
                    }
                }
                Ok(Some(DeviceEvent::Button(ev))) => {
                    had_event = true;
                    let name = format!("{}::{}", active.device_id, ev.button.name());
                    let raw = if active.grabbed { Some(ev.raw) } else { None };
                    if ev.pressed {
                        handle_button_down(
                            &name,
                            raw,
                            &config,
                            Arc::clone(&injector),
                            &mut active_held,
                        );
                    } else {
                        handle_button_up_raw(
                            &name,
                            raw,
                            &config,
                            Arc::clone(&injector),
                            &mut active_held,
                        );
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    error!("evdev read error on '{}': {e}", active.device_id);
                }
            }
        }

        if !had_event {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

fn lookup_binding_down(button_name: &str, config: &Arc<Mutex<Config>>) -> (Option<String>, bool) {
    let cfg = config.lock().unwrap();
    cfg.devices
        .iter()
        .flat_map(|dc| dc.bindings.iter())
        .find(|b| b.button.as_deref() == Some(button_name))
        .map(|b| (b.on_press.clone(), b.intercept))
        .unwrap_or((None, false))
}

fn lookup_binding_up(button_name: &str, config: &Arc<Mutex<Config>>) -> (Option<String>, bool) {
    let cfg = config.lock().unwrap();
    cfg.devices
        .iter()
        .flat_map(|dc| dc.bindings.iter())
        .find(|b| b.button.as_deref() == Some(button_name))
        .map(|b| (b.on_release.clone(), b.intercept))
        .unwrap_or((None, false))
}

fn handle_button_down(
    button_name: &str,
    raw: Option<RawEvent>,
    config: &Arc<Mutex<Config>>,
    injector: Arc<Mutex<dyn InputInjector>>,
    active_held: &mut HashMap<String, Arc<AtomicBool>>,
) {
    let (script_path, intercept) = lookup_binding_down(button_name, config);

    if raw.is_some() && !intercept {
        if let Some(r) = raw {
            if let Err(e) = injector
                .lock()
                .unwrap()
                .passthrough_raw(r.ev_type, r.code, r.value)
            {
                warn!("passthrough_raw failed: {e}");
            }
        }
    }

    let script_path = match script_path {
        Some(p) => p,
        None => return,
    };

    let resolved = {
        let cfg = config.lock().unwrap();
        cfg.resolve_script_path(&script_path)
    };

    let held = Arc::new(AtomicBool::new(true));
    active_held.insert(button_name.to_string(), Arc::clone(&held));

    if let Err(e) =
        engine::rhai::run_script(resolved.to_str().unwrap_or(&script_path), held, injector)
    {
        error!("Failed to launch macro for {:?}: {}", button_name, e);
    }
}

fn handle_button_up_raw(
    button_name: &str,
    raw: Option<RawEvent>,
    config: &Arc<Mutex<Config>>,
    injector: Arc<Mutex<dyn InputInjector>>,
    active_held: &mut HashMap<String, Arc<AtomicBool>>,
) {
    if let Some(held) = active_held.remove(button_name) {
        held.store(false, Ordering::Relaxed);
    }

    let (release_path, intercept) = lookup_binding_up(button_name, config);

    if raw.is_some() && !intercept {
        if let Some(r) = raw {
            if let Err(e) = injector
                .lock()
                .unwrap()
                .passthrough_raw(r.ev_type, r.code, r.value)
            {
                warn!("passthrough_raw failed: {e}");
            }
        }
    }

    if let Some(script_path) = release_path {
        let resolved = {
            let cfg = config.lock().unwrap();
            cfg.resolve_script_path(&script_path)
        };
        let held = Arc::new(AtomicBool::new(true));
        if let Err(e) =
            engine::rhai::run_script(resolved.to_str().unwrap_or(&script_path), held, injector)
        {
            error!(
                "Failed to launch on_release macro for {:?}: {}",
                button_name, e
            );
        }
    }
}
