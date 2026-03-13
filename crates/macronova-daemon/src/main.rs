mod engine;
mod input;

use std::collections::{HashMap, HashSet};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

use anyhow::{Context, Result};
use macronova_core::{
    config::{default_config_dir, Config},
    device::evdev_input::{discover_evdev_paths, DeviceEvent, EvdevPaths, EvdevReader, RawEvent},
};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use macronova_core::platform::input::{get_desktop_size, InputInjector};

use crate::input::hidpp_reader::{cid_from_button_name, spawn as spawn_hidpp, HidppButtonEvent};
use crate::input::PlatformInjector;

/// Try to discover and open an evdev reader.  Returns `None` if the device is
/// not present yet (caller should retry later).
fn try_open_reader() -> Option<EvdevReader> {
    let paths = discover_evdev_paths().or_else(|| {
        warn!("Could not discover evdev paths via by-id; trying event5/event6 fallback");
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
        Ok(r) => {
            info!(
                "evdev reader opened: mouse={} (label={}), kbd={} (label={})",
                paths.mouse_path, paths.mouse_label, paths.kbd_path, paths.kbd_label
            );
            Some(r)
        }
        Err(e) => {
            warn!(
                "Failed to open evdev reader ({}, {}): {e}",
                paths.mouse_path, paths.kbd_path
            );
            None
        }
    }
}

/// Returns `true` if Virtual Mode is enabled.
/// When Virtual Mode is off the daemon never grabs the device.
fn config_needs_grab(config: &Config) -> bool {
    config.virtual_mode
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("macronova_daemon=info".parse()?),
        )
        .init();

    info!("MacroNova daemon starting");

    let config_dir = default_config_dir();
    let config_path = config_dir.join("config.toml");
    let config = Arc::new(Mutex::new(Config::load_default()?));

    let (desktop_w, desktop_h) = get_desktop_size().unwrap_or_else(|| {
        warn!("Could not determine desktop size; defaulting to 1920x1080 for warp scaling");
        (1920, 1080)
    });
    let warp_mode = config.lock().unwrap().warp_mode;
    let injector: Arc<Mutex<dyn InputInjector>> = Arc::new(Mutex::new(
        PlatformInjector::new(desktop_w, desktop_h, warp_mode)
            .context("Failed to create platform input injector")?,
    ));
    info!("Loaded config from {}", config_path.display());

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<Event>>();
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    if config_dir.exists() {
        watcher.watch(&config_dir, RecursiveMode::Recursive)?;
        info!("Watching config directory: {}", config_dir.display());
    }

    // Collect CIDs for all bound buttons that use the cid/0x... naming scheme.
    let cids_to_divert: HashSet<u16> = {
        let cfg = config.lock().unwrap();
        cfg.device
            .values()
            .flat_map(|dc| dc.bindings.iter())
            .filter_map(|b| b.button.as_deref())
            .filter_map(cid_from_button_name)
            .collect()
    };

    // Spawn the HID++ reader thread if there are any CID-based bindings.
    let (hidpp_tx, hidpp_rx) = std::sync::mpsc::channel::<HidppButtonEvent>();
    let hidpp_alive = Arc::new(AtomicBool::new(false));
    if !cids_to_divert.is_empty() {
        info!(
            "Spawning HID++ reader for {} CID binding(s)",
            cids_to_divert.len()
        );
        if let Err(e) = spawn_hidpp(
            cids_to_divert.clone(),
            hidpp_tx.clone(),
            Arc::clone(&hidpp_alive),
        ) {
            warn!("HID++ reader unavailable: {e} — falling back to evdev only");
        }
    }

    // Initial device open — wait with backoff until the device appears.
    let mut reconnect_delay = Duration::from_millis(500);
    let mut reader: Option<EvdevReader> = None;
    while reader.is_none() {
        match try_open_reader() {
            Some(r) => reader = Some(r),
            None => {
                info!(
                    "Waiting for evdev device — retrying in {}ms",
                    reconnect_delay.as_millis()
                );
                std::thread::sleep(reconnect_delay);
                reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));
            }
        }
    }
    let mut reader = reader.unwrap();
    reconnect_delay = Duration::from_millis(500);

    // Apply initial grab state based on config.
    let mut grabbed = false;
    {
        let cfg = config.lock().unwrap();
        if config_needs_grab(&cfg) {
            if let Err(e) = reader.grab(true) {
                warn!("EVIOCGRAB failed: {e} — interception will not work");
            } else {
                grabbed = true;
                info!("evdev grab acquired (intercept bindings present)");
            }
        }
    }

    let mut active_held: HashMap<String, Arc<AtomicBool>> = HashMap::new();

    info!("Entering main event loop");
    loop {
        // Process config file watcher events.
        while let Ok(Ok(event)) = rx.try_recv() {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if event.paths.iter().any(|p| p == &config_path) {
                    match Config::load(&config_path) {
                        Ok(new_cfg) => {
                            info!("Config reloaded");
                            let new_grab = config_needs_grab(&new_cfg);
                            *config.lock().unwrap() = new_cfg;
                            // Update grab state if it changed.
                            if new_grab != grabbed {
                                match reader.grab(new_grab) {
                                    Ok(()) => {
                                        grabbed = new_grab;
                                        if grabbed {
                                            info!("evdev grab acquired");
                                        } else {
                                            info!("evdev grab released");
                                        }
                                    }
                                    Err(e) => warn!("EVIOCGRAB({new_grab}) failed: {e}"),
                                }
                            }
                        }
                        Err(e) => warn!("Failed to reload config: {}", e),
                    }
                }
            }
        }

        // Drain HID++ notification events (non-blocking).
        // HID++ buttons are diverted at the firmware level so no passthrough
        // is needed for them — they never reach the OS event stack anyway.
        while let Ok(ev) = hidpp_rx.try_recv() {
            if ev.name.is_empty() {
                continue;
            }
            if ev.pressed {
                handle_button_down(
                    &ev.name,
                    None,
                    &config,
                    Arc::clone(&injector),
                    &mut active_held,
                );
            } else {
                handle_button_up(&ev.name, &config, Arc::clone(&injector), &mut active_held);
            }
        }

        // Poll evdev (blocks up to 100ms).
        match reader.poll(Duration::from_millis(100)) {
            Ok(None) => {
                reconnect_delay = Duration::from_millis(500);
                continue;
            }
            Ok(Some(DeviceEvent::Passthrough(pt))) => {
                reconnect_delay = Duration::from_millis(500);
                // Only forward when the device is actually grabbed.  When not
                // grabbed the kernel still delivers events to the compositor
                // directly — re-injecting here would double every motion event.
                if grabbed {
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
                reconnect_delay = Duration::from_millis(500);
                let name = ev.button.name();
                let raw = ev.raw;
                if ev.pressed {
                    handle_button_down(
                        &name,
                        if grabbed { Some(raw) } else { None },
                        &config,
                        Arc::clone(&injector),
                        &mut active_held,
                    );
                } else {
                    handle_button_up_raw(
                        &name,
                        if grabbed { Some(raw) } else { None },
                        &config,
                        Arc::clone(&injector),
                        &mut active_held,
                    );
                }
            }
            Err(e) => {
                error!("evdev read error (device lost?): {e} — attempting reconnect");
                drop(reader);

                loop {
                    std::thread::sleep(reconnect_delay);
                    reconnect_delay = (reconnect_delay * 2).min(Duration::from_secs(5));

                    match try_open_reader() {
                        Some(new_reader) => {
                            info!("evdev device reconnected — resuming");
                            reader = new_reader;
                            reconnect_delay = Duration::from_millis(500);

                            // Re-apply grab after reconnect.
                            if grabbed {
                                if let Err(e) = reader.grab(true) {
                                    warn!("EVIOCGRAB failed after reconnect: {e}");
                                    grabbed = false;
                                }
                            }

                            if !cids_to_divert.is_empty() && !hidpp_alive.load(Ordering::Relaxed) {
                                info!("Re-spawning HID++ reader after reconnect");
                                if let Err(e) = spawn_hidpp(
                                    cids_to_divert.clone(),
                                    hidpp_tx.clone(),
                                    Arc::clone(&hidpp_alive),
                                ) {
                                    warn!("HID++ reader still unavailable: {e}");
                                }
                            }

                            break;
                        }
                        None => {
                            info!(
                                "Device not yet available — retrying in {}ms",
                                reconnect_delay.as_millis()
                            );
                        }
                    }
                }
            }
        }
    }
}

/// Look up the binding for a button name.
/// Returns `(on_press_path, intercept)`.
fn lookup_binding_down(button_name: &str, config: &Arc<Mutex<Config>>) -> (Option<String>, bool) {
    let cfg = config.lock().unwrap();
    cfg.device
        .values()
        .flat_map(|dc| dc.bindings.iter())
        .find(|b| b.button.as_deref() == Some(button_name))
        .map(|b| (b.on_press.clone(), b.intercept))
        .unwrap_or((None, false))
}

/// Look up the on_release script and intercept flag for a button.
fn lookup_binding_up(button_name: &str, config: &Arc<Mutex<Config>>) -> (Option<String>, bool) {
    let cfg = config.lock().unwrap();
    cfg.device
        .values()
        .flat_map(|dc| dc.bindings.iter())
        .find(|b| b.button.as_deref() == Some(button_name))
        .map(|b| (b.on_release.clone(), b.intercept))
        .unwrap_or((None, false))
}

fn handle_button_down(
    button_name: &str,
    // Raw event for passthrough, present only when the device is grabbed.
    raw: Option<RawEvent>,
    config: &Arc<Mutex<Config>>,
    injector: Arc<Mutex<dyn InputInjector>>,
    active_held: &mut HashMap<String, Arc<AtomicBool>>,
) {
    let (script_path, intercept) = lookup_binding_down(button_name, config);

    // If grabbed and not intercepted, forward the event to the OS.
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
        None => {
            let suppress = matches!(button_name,
                n if n.ends_with("/key0x0110")   // BTN_LEFT
                  || n.ends_with("/key0x0111")   // BTN_RIGHT
                  || n.ends_with("/key0x0112")); // BTN_MIDDLE
            if !suppress {
                info!("Button {:?} DOWN (no binding)", button_name);
            }
            return;
        }
    };

    let resolved = {
        let cfg = config.lock().unwrap();
        cfg.resolve_script_path(&script_path)
    };

    let held = Arc::new(AtomicBool::new(true));
    active_held.insert(button_name.to_string(), Arc::clone(&held));
    info!("Button {:?} DOWN → running {:?}", button_name, resolved);

    if let Err(e) =
        engine::rhai::run_script(resolved.to_str().unwrap_or(&script_path), held, injector)
    {
        error!("Failed to launch macro for {:?}: {}", button_name, e);
    }
}

fn handle_button_up(
    button_name: &str,
    config: &Arc<Mutex<Config>>,
    injector: Arc<Mutex<dyn InputInjector>>,
    active_held: &mut HashMap<String, Arc<AtomicBool>>,
) {
    handle_button_up_raw(button_name, None, config, injector, active_held);
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

    // If grabbed and not intercepted, forward the release event to the OS.
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

    let suppress = matches!(button_name,
        n if n.ends_with("/key0x0110")   // BTN_LEFT
          || n.ends_with("/key0x0111")   // BTN_RIGHT
          || n.ends_with("/key0x0112")); // BTN_MIDDLE

    if let Some(script_path) = release_path {
        let resolved = {
            let cfg = config.lock().unwrap();
            cfg.resolve_script_path(&script_path)
        };
        info!("Button {:?} UP → running {:?}", button_name, resolved);
        let held = Arc::new(AtomicBool::new(true));
        if let Err(e) =
            engine::rhai::run_script(resolved.to_str().unwrap_or(&script_path), held, injector)
        {
            error!(
                "Failed to launch on_release macro for {:?}: {}",
                button_name, e
            );
        }
    } else if !suppress {
        info!("Button {:?} UP", button_name);
    }
}
