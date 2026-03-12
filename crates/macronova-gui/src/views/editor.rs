use std::path::PathBuf;

use egui::{Color32, RichText, ScrollArea, TextEdit, Ui};
use macronova_core::config::{default_macros_dir, Config};

pub struct EditorView {
    /// Currently open file path.
    open_path: Option<PathBuf>,
    /// Content of the open file.
    content: String,
    /// Unsaved changes indicator.
    dirty: bool,
    /// Status message after save.
    status: Option<String>,
    /// New macro filename input.
    new_file_name: String,
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            open_path: None,
            content: String::new(),
            dirty: false,
            status: None,
            new_file_name: String::new(),
        }
    }

    /// Show the editor. Returns `true` if a file was saved this frame.
    pub fn show(&mut self, ui: &mut Ui, _config: &Config) -> bool {
        let mut saved = false;

        ui.heading("Macro Editor");
        ui.add_space(8.0);

        // Toolbar.
        ui.horizontal(|ui| {
            if ui.button("New").clicked() {
                self.new_macro();
            }
            if ui.button("Open").clicked() {
                self.open_file_dialog();
            }
            if let Some(ref path) = self.open_path.clone() {
                if ui
                    .button(if self.dirty { "Save *" } else { "Save" })
                    .clicked()
                {
                    self.save(path.clone());
                    saved = true;
                }
            }
            if let Some(ref msg) = self.status {
                ui.colored_label(Color32::LIGHT_GREEN, msg);
            }
        });

        ui.add_space(4.0);

        // File list from macros directory.
        ui.horizontal(|ui| {
            ui.label("Macros dir:");
            ui.label(
                RichText::new(default_macros_dir().display().to_string())
                    .color(Color32::GRAY)
                    .monospace(),
            );
        });

        ui.separator();

        let macros_dir = default_macros_dir();
        if macros_dir.exists() {
            egui::CollapsingHeader::new("Macro files")
                .default_open(true)
                .show(ui, |ui| {
                    if let Ok(entries) = std::fs::read_dir(&macros_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if path.extension().map_or(false, |e| e == "rhai") {
                                let name = path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                let is_open = self.open_path.as_ref() == Some(&path);
                                if ui.selectable_label(is_open, &name).clicked() {
                                    self.load(path.clone());
                                }
                            }
                        }
                    }

                    // New file input.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.new_file_name)
                            .on_hover_text("new-macro.rhai");
                        if ui.button("Create").clicked() && !self.new_file_name.is_empty() {
                            let name = if self.new_file_name.ends_with(".rhai") {
                                self.new_file_name.clone()
                            } else {
                                format!("{}.rhai", self.new_file_name)
                            };
                            let path = macros_dir.join(&name);
                            self.open_path = Some(path.clone());
                            self.content = default_template();
                            self.dirty = true;
                            self.new_file_name.clear();
                        }
                    });
                });
        } else {
            ui.label(RichText::new("Macros directory does not exist yet.").color(Color32::GRAY));
            if ui.button("Create macros directory").clicked() {
                let _ = std::fs::create_dir_all(&macros_dir);
            }
        }

        ui.separator();

        // Editor area.
        if let Some(ref path) = self.open_path.clone() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(path.display().to_string())
                        .monospace()
                        .small(),
                );
                if self.dirty {
                    ui.colored_label(Color32::YELLOW, "(unsaved)");
                }
            });
            ui.add_space(4.0);

            let before = self.content.clone();
            ScrollArea::vertical()
                .id_salt("editor_scroll")
                .show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut self.content)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24),
                    );
                });
            if self.content != before {
                self.dirty = true;
            }
        } else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No file open. Create or select a macro file above.")
                        .color(Color32::GRAY),
                );
            });
        }

        saved
    }

    fn load(&mut self, path: PathBuf) {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                self.content = content;
                self.open_path = Some(path);
                self.dirty = false;
                self.status = None;
            }
            Err(e) => {
                self.status = Some(format!("Failed to read: {}", e));
            }
        }
    }

    fn save(&mut self, path: PathBuf) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&path, &self.content) {
            Ok(_) => {
                self.dirty = false;
                self.status = Some("Saved.".into());
            }
            Err(e) => {
                self.status = Some(format!("Save failed: {}", e));
            }
        }
    }

    fn new_macro(&mut self) {
        self.open_path = None;
        self.content = default_template();
        self.dirty = false;
        self.status = None;
    }

    fn open_file_dialog(&mut self) {
        // Simple: just look in the macros dir. A full file picker would need rfd or similar.
        // For now this is a no-op placeholder — files are opened via the list.
    }
}

fn default_template() -> String {
    r#"// MacroNova Rhai macro
// Available functions:
//   press_key("ctrl")    release_key("ctrl")   tap_key("a")
//   type_text("hello")   scroll(3)             sleep(100)
//   held()  -> true while button is still held down

// Example: tap Ctrl+Z
press_key("ctrl");
tap_key("z");
release_key("ctrl");
"#
    .into()
}
