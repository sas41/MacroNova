use egui::{Color32, RichText, Ui};
use macronova_core::{
    config::{default_macros_dir, ButtonBinding, Config, InputDeviceConfig},
    device::evdev_input::list_evdev_device_candidates,
};

type RowKey = (String, usize);

#[derive(PartialEq, Eq)]
enum CaptureState {
    Idle,
    Waiting(RowKey),
}

pub struct BindingsView {
    config: Config,
    capture: CaptureState,
    macro_files: Vec<String>,
    pending_save: bool,
    add_device_open: bool,
}

impl BindingsView {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            capture: CaptureState::Idle,
            macro_files: scan_macro_files(),
            pending_save: false,
            add_device_open: false,
        }
    }

    pub fn update_config(&mut self, config: Config) {
        self.config = config;
        self.capture = CaptureState::Idle;
        self.pending_save = false;
        self.macro_files = scan_macro_files();
    }

    pub fn on_button_event(&mut self, button: String) -> bool {
        if let CaptureState::Waiting(ref key) = self.capture {
            let key = key.clone();
            let device_id = &key.0;
            let idx = key.1;

            let Some((event_device, _event_name)) = button.split_once("::") else {
                return false;
            };
            if event_device != device_id {
                return false;
            }

            if let Some(device_cfg) = self.config.devices.iter_mut().find(|d| &d.id == device_id) {
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

    pub fn refresh_macro_files(&mut self) {
        self.macro_files = scan_macro_files();
    }

    pub fn show(&mut self, ui: &mut Ui, config: &Config) -> Option<Config> {
        let capture_flushed = std::mem::take(&mut self.pending_save);
        let mut new_config = if capture_flushed {
            self.config.clone()
        } else {
            config.clone()
        };
        let mut changed = capture_flushed;

        ui.heading("Device Bindings");
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            if ui.button("+ Add device").clicked() {
                self.add_device_open = true;
            }
            ui.label(RichText::new("Pick a physical device, then add bindings under it.").weak());
        });

        if self.add_device_open {
            egui::Window::new("Add Device")
                .collapsible(false)
                .resizable(true)
                .show(ui.ctx(), |ui| {
                    let candidates = list_evdev_device_candidates();
                    if candidates.is_empty() {
                        ui.label("No evdev by-id devices found.");
                    } else {
                        for c in candidates {
                            ui.group(|ui| {
                                ui.label(RichText::new(&c.base_name).strong());
                                ui.label(format!("mouse: {}", c.mouse_path));
                                if let Some(ref k) = c.kbd_path {
                                    ui.label(format!("kbd: {}", k));
                                }
                                let already =
                                    new_config.devices.iter().any(|d| d.id == c.base_name);
                                if ui
                                    .add_enabled(!already, egui::Button::new("Add this device"))
                                    .clicked()
                                {
                                    new_config.devices.push(InputDeviceConfig {
                                        id: c.base_name.clone(),
                                        display_name: c.base_name.clone(),
                                        mouse_path: c.mouse_path.clone(),
                                        kbd_path: c.kbd_path.clone(),
                                        bindings: Vec::new(),
                                    });
                                    changed = true;
                                    self.add_device_open = false;
                                }
                            });
                            ui.add_space(4.0);
                        }
                    }

                    if ui.button("Close").clicked() {
                        self.add_device_open = false;
                    }
                });
        }

        if self.capture != CaptureState::Idle {
            ui.horizontal(|ui| {
                ui.colored_label(Color32::YELLOW, "● Capture mode");
                ui.label("Capture is scoped to the selected device.");
                if ui.button("Cancel").clicked() {
                    self.capture = CaptureState::Idle;
                }
            });
            ui.add_space(6.0);
        }

        if new_config.devices.is_empty() {
            ui.label(RichText::new("No devices configured yet.").color(Color32::GRAY));
        }

        let macro_files = self.macro_files.clone();

        for di in 0..new_config.devices.len() {
            let device_id = new_config.devices[di].id.clone();
            let display_name = new_config.devices[di].display_name.clone();

            ui.collapsing(
                RichText::new(format!("Device: {}", display_name)).strong(),
                |ui| {
                    ui.label(RichText::new(format!("id: {}", device_id)).weak());
                    ui.label(
                        RichText::new(format!("mouse: {}", new_config.devices[di].mouse_path))
                            .weak(),
                    );
                    if let Some(ref kbd) = new_config.devices[di].kbd_path {
                        ui.label(RichText::new(format!("kbd: {}", kbd)).weak());
                    }
                    ui.add_space(4.0);

                    let mut remove_binding: Option<usize> = None;
                    for idx in 0..new_config.devices[di].bindings.len() {
                        let row_key: RowKey = (device_id.clone(), idx);
                        let binding = new_config.devices[di].bindings[idx].clone();
                        let is_capturing = self.capture == CaptureState::Waiting(row_key.clone());

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let btn_label = binding
                                    .button
                                    .as_deref()
                                    .and_then(|b| b.split_once("::").map(|(_, n)| n))
                                    .unwrap_or("(unset)");
                                if is_capturing {
                                    ui.colored_label(Color32::YELLOW, "▶ press button now...");
                                } else {
                                    ui.strong(btn_label);
                                    if ui.small_button("Capture").clicked() {
                                        self.capture = CaptureState::Waiting(row_key.clone());
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.small_button("Remove").clicked() {
                                            remove_binding = Some(idx);
                                        }
                                    },
                                );
                            });

                            ui.horizontal(|ui| {
                                ui.label("on_press:");
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
                                        let none_selected = binding.on_press.is_none();
                                        if ui.selectable_label(none_selected, "(none)").clicked()
                                            && !none_selected
                                        {
                                            new_config.devices[di].bindings[idx].on_press = None;
                                            changed = true;
                                        }

                                        for file_name in &macro_files {
                                            let is_selected = &current_file == file_name;
                                            if ui.selectable_label(is_selected, file_name).clicked()
                                                && !is_selected
                                            {
                                                new_config.devices[di].bindings[idx].on_press =
                                                    Some(format!("macros/{}", file_name));
                                                changed = true;
                                            }
                                        }
                                    });

                                ui.add_space(8.0);
                                ui.add_enabled_ui(new_config.virtual_mode, |ui| {
                                    let mut intercept = if new_config.virtual_mode {
                                        new_config.devices[di].bindings[idx].intercept
                                    } else {
                                        false
                                    };
                                    if ui.checkbox(&mut intercept, "Intercept").changed()
                                        && new_config.virtual_mode
                                    {
                                        new_config.devices[di].bindings[idx].intercept = intercept;
                                        changed = true;
                                    }
                                });
                            });
                        });
                        ui.add_space(2.0);
                    }

                    if let Some(idx) = remove_binding {
                        new_config.devices[di].bindings.remove(idx);
                        changed = true;
                        if self.capture == CaptureState::Waiting((device_id.clone(), idx)) {
                            self.capture = CaptureState::Idle;
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui.button("+ Add binding").clicked() {
                            new_config.devices[di].bindings.push(ButtonBinding {
                                button: None,
                                on_press: None,
                                on_release: None,
                                intercept: false,
                            });
                            changed = true;
                        }

                        if ui.button("Remove device").clicked() {
                            new_config.devices.remove(di);
                            changed = true;
                        }
                    });
                },
            );

            ui.add_space(4.0);
            if changed {
                break;
            }
        }

        if changed {
            self.config = new_config.clone();
            Some(new_config)
        } else {
            None
        }
    }
}

fn scan_macro_files() -> Vec<String> {
    let dir = default_macros_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().is_some_and(|ext| ext == "rhai") {
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
