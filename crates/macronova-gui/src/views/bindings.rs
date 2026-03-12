use egui::{Color32, RichText, Ui};
use macronova_core::config::{default_macros_dir, ButtonBinding, Config, DeviceConfig};

/// Unique key for a binding row: (device_name, binding_index).
type RowKey = (String, usize);

/// What the capture state machine is doing.
#[derive(PartialEq, Eq)]
enum CaptureState {
    Idle,
    /// Waiting for a physical button press for the given row.
    Waiting(RowKey),
}

pub struct BindingsView {
    config: Config,
    capture: CaptureState,
    /// Cached list of macro filenames discovered from the macros directory.
    /// Each entry is the filename only (e.g. `"undo.rhai"`); the stored config
    /// value is the relative path `"macros/<name>"`.
    macro_files: Vec<String>,
    /// Set by `on_button_event` so that the next `show()` call flushes the
    /// capture result back to the app as an updated config.
    pending_save: bool,
}

impl BindingsView {
    pub fn new(config: Config) -> Self {
        let macro_files = scan_macro_files();
        Self {
            config,
            capture: CaptureState::Idle,
            macro_files,
            pending_save: false,
        }
    }

    pub fn update_config(&mut self, config: Config) {
        self.config = config;
        // Cancel any pending capture/save on config reload; also refresh file list.
        self.capture = CaptureState::Idle;
        self.pending_save = false;
        self.macro_files = scan_macro_files();
    }

    /// Called by the app when a physical button is pressed.
    /// If we're in capture mode, assign it to the waiting binding row.
    /// Returns true if the event was consumed.
    pub fn on_button_event(&mut self, button: String) -> bool {
        if let CaptureState::Waiting(ref key) = self.capture {
            let key = key.clone();
            let device_name = &key.0;
            let idx = key.1;
            if let Some(device_cfg) = self.config.device.get_mut(device_name) {
                if let Some(binding) = device_cfg.bindings.get_mut(idx) {
                    binding.button = Some(button);
                }
            }
            self.capture = CaptureState::Idle;
            self.pending_save = true;
            return true;
        }
        false
    }

    /// Refresh the cached macro file list (call when the Editor creates a new file).
    pub fn refresh_macro_files(&mut self) {
        self.macro_files = scan_macro_files();
    }

    /// Show the bindings panel. Returns Some(updated_config) if a change was made.
    pub fn show(&mut self, ui: &mut Ui, config: &Config) -> Option<Config> {
        // If a capture completed between frames, self.config already has the new
        // button name written in.  Use it as the base and treat it as changed so
        // the app saves and re-broadcasts the config.
        let capture_flushed = std::mem::take(&mut self.pending_save);
        let mut new_config = if capture_flushed {
            self.config.clone()
        } else {
            config.clone()
        };
        let mut changed = capture_flushed;

        ui.heading("Button Bindings");
        ui.add_space(6.0);

        if self.capture != CaptureState::Idle {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::YELLOW, "● Capture mode");
                ui.label("Press a button on the mouse now, or");
                if ui.button("Cancel").clicked() {
                    self.capture = CaptureState::Idle;
                }
            });
            ui.add_space(6.0);
        }

        // One section per configured device.
        if new_config.device.is_empty() {
            ui.label(
                RichText::new("No devices configured. Add a [device.Name] section to your config.")
                    .color(Color32::GRAY),
            );
            if ui.button("Add G502X device").clicked() {
                new_config.device.insert(
                    "G502X".into(),
                    DeviceConfig {
                        wpid: Some("407F".into()),
                        usb_pid: None,
                        bindings: vec![],
                    },
                );
                changed = true;
            }
        }

        // Snapshot the macro file list for use inside closures.
        let macro_files = self.macro_files.clone();

        let device_names: Vec<String> = new_config.device.keys().cloned().collect();

        for device_name in device_names {
            let device_cfg = new_config.device.get_mut(&device_name).unwrap();

            ui.collapsing(
                RichText::new(format!("Device: {}", device_name)).strong(),
                |ui| {
                    let mut to_remove: Option<usize> = None;

                    for idx in 0..device_cfg.bindings.len() {
                        let row_key: RowKey = (device_name.clone(), idx);
                        let binding = device_cfg.bindings[idx].clone();
                        let is_capturing = self.capture == CaptureState::Waiting(row_key.clone());

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            // ── Row 1: button name + capture + remove ──────────
                            ui.horizontal(|ui| {
                                let btn_label = binding.button.as_deref().unwrap_or("(unset)");
                                if is_capturing {
                                    ui.colored_label(Color32::YELLOW, "▶ press button now…");
                                } else {
                                    ui.strong(btn_label);
                                    if ui.small_button("Capture").clicked() {
                                        self.capture = CaptureState::Waiting(row_key.clone());
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("✕ Remove").clicked() {
                                            to_remove = Some(idx);
                                        }
                                    },
                                );
                            });

                            // ── Row 2: on_press script dropdown ───────────────
                            ui.horizontal(|ui| {
                                ui.label("on_press:");

                                // Determine the currently selected filename.
                                // Config stores e.g. "macros/undo.rhai"; we display "undo.rhai".
                                let current_file = binding
                                    .on_press
                                    .as_deref()
                                    .and_then(|p| {
                                        std::path::Path::new(p)
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .map(|s| s.to_string())
                                    })
                                    .unwrap_or_default();

                                let display_label = if current_file.is_empty() {
                                    "(none)".to_string()
                                } else {
                                    current_file.clone()
                                };

                                let combo_id = egui::Id::new(("script_combo", &row_key));
                                egui::ComboBox::from_id_salt(combo_id)
                                    .selected_text(&display_label)
                                    .width(220.0)
                                    .show_ui(ui, |ui| {
                                        // "(none)" option — clears the binding.
                                        let none_selected = binding.on_press.is_none();
                                        if ui.selectable_label(none_selected, "(none)").clicked()
                                            && !none_selected
                                        {
                                            device_cfg.bindings[idx].on_press = None;
                                            changed = true;
                                        }

                                        // One entry per discovered .rhai file.
                                        for file_name in &macro_files {
                                            let is_selected = &current_file == file_name;
                                            if ui.selectable_label(is_selected, file_name).clicked()
                                                && !is_selected
                                            {
                                                device_cfg.bindings[idx].on_press =
                                                    Some(format!("macros/{}", file_name));
                                                changed = true;
                                            }
                                        }
                                    });

                                // Subtle hint when no scripts exist yet.
                                if macro_files.is_empty() {
                                    ui.colored_label(
                                        Color32::DARK_GRAY,
                                        "no scripts — create one in the Editor tab",
                                    );
                                }
                            });
                        });
                        ui.add_space(2.0);
                    }

                    if let Some(idx) = to_remove {
                        device_cfg.bindings.remove(idx);
                        changed = true;
                        if self.capture == CaptureState::Waiting((device_name.clone(), idx)) {
                            self.capture = CaptureState::Idle;
                        }
                    }

                    ui.add_space(4.0);
                    if ui.button("+ Add binding").clicked() {
                        device_cfg.bindings.push(ButtonBinding {
                            button: None,
                            cid: 0,
                            on_press: None,
                            on_release: None,
                        });
                        changed = true;
                    }
                },
            );

            ui.add_space(4.0);
        }

        if changed {
            self.config = new_config.clone();
            Some(new_config)
        } else {
            None
        }
    }
}

/// Scan `~/.config/macronova/macros/` and return a sorted list of `.rhai` filenames.
fn scan_macro_files() -> Vec<String> {
    let dir = default_macros_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().map_or(false, |ext| ext == "rhai") {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())
            } else {
                None
            }
        })
        .collect();
    files.sort();
    files
}
