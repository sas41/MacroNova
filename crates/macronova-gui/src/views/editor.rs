use std::path::PathBuf;

use egui::{Color32, Key, RichText, ScrollArea, TextEdit, Ui};
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
    /// Inline rename state: (original path, current input text, needs_focus).
    rename_state: Option<(PathBuf, String, bool)>,
    /// Path pending deletion confirmation.
    confirm_delete: Option<PathBuf>,
}

impl EditorView {
    pub fn new() -> Self {
        Self {
            open_path: None,
            content: String::new(),
            dirty: false,
            status: None,
            new_file_name: String::new(),
            rename_state: None,
            confirm_delete: None,
        }
    }

    /// Show the editor. Returns `true` if a file was saved this frame.
    pub fn show(&mut self, ui: &mut Ui, _config: &Config) -> bool {
        let mut saved = false;

        ui.heading("Macro Editor");
        ui.add_space(8.0);

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
                        let mut paths: Vec<PathBuf> = entries
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| p.extension().map_or(false, |e| e == "rhai"))
                            .collect();
                        paths.sort();
                        let mut commit_rename: Option<(PathBuf, String)> = None;
                        let mut cancel_rename = false;
                        for path in &paths {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            let is_open = self.open_path.as_ref() == Some(path);
                            let is_renaming = self
                                .rename_state
                                .as_ref()
                                .map_or(false, |(p, _, _)| p == path);

                            ui.horizontal(|ui| {
                                if is_renaming {
                                    let state = self.rename_state.as_mut().unwrap();
                                    let resp = ui.add(
                                        TextEdit::singleline(&mut state.1)
                                            .desired_width(160.0)
                                            .font(egui::TextStyle::Body),
                                    );
                                    // Only request focus on the first frame so lost_focus() works.
                                    if state.2 {
                                        resp.request_focus();
                                        state.2 = false;
                                    }
                                    let enter = ui.input(|i| i.key_pressed(Key::Enter));
                                    let escape = ui.input(|i| i.key_pressed(Key::Escape));
                                    if escape {
                                        cancel_rename = true;
                                    } else if enter || resp.lost_focus() {
                                        commit_rename = Some((state.0.clone(), state.1.clone()));
                                    }
                                } else {
                                    let lbl = ui.selectable_label(is_open, &name);
                                    if lbl.clicked() {
                                        self.load(path.clone());
                                    }
                                    if is_open {
                                        let ren = ui.add(
                                            egui::Button::new(
                                                RichText::new("[r]")
                                                    .color(Color32::from_rgb(150, 150, 220))
                                                    .monospace(),
                                            )
                                            .small()
                                            .frame(false),
                                        );
                                        if ren.on_hover_text("Rename file").clicked() {
                                            self.rename_state =
                                                Some((path.clone(), name.clone(), true));
                                        }
                                        let del = ui.add(
                                            egui::Button::new(
                                                RichText::new("[x]")
                                                    .color(Color32::from_rgb(200, 80, 80))
                                                    .monospace(),
                                            )
                                            .small()
                                            .frame(false),
                                        );
                                        if del.on_hover_text("Delete file").clicked() {
                                            self.confirm_delete = Some(path.clone());
                                        }
                                    }
                                }
                            });
                        }

                        if cancel_rename {
                            self.rename_state = None;
                        }
                        if let Some((old_path, new_name)) = commit_rename {
                            let new_name = if new_name.ends_with(".rhai") {
                                new_name
                            } else {
                                format!("{}.rhai", new_name)
                            };
                            let new_path = macros_dir.join(&new_name);
                            match std::fs::rename(&old_path, &new_path) {
                                Ok(_) => {
                                    if self.open_path.as_ref() == Some(&old_path) {
                                        self.open_path = Some(new_path);
                                    }
                                    self.status = Some("Renamed.".into());
                                }
                                Err(e) => {
                                    self.status = Some(format!("Rename failed: {e}"));
                                }
                            }
                            self.rename_state = None;
                        }
                    }

                    // New file input.
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        let response = ui
                            .text_edit_singleline(&mut self.new_file_name)
                            .on_hover_text("new-macro.rhai");
                        let enter_pressed =
                            response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                        if (ui.button("Create").clicked() || enter_pressed)
                            && !self.new_file_name.is_empty()
                        {
                            let name = if self.new_file_name.ends_with(".rhai") {
                                self.new_file_name.clone()
                            } else {
                                format!("{}.rhai", self.new_file_name)
                            };
                            let path = macros_dir.join(&name);
                            let content = default_template();
                            // Write immediately so the file exists on disk.
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            match std::fs::write(&path, &content) {
                                Ok(_) => {
                                    self.open_path = Some(path);
                                    self.content = content;
                                    self.dirty = false;
                                    self.status = Some("Created.".into());
                                }
                                Err(e) => {
                                    self.status = Some(format!("Create failed: {e}"));
                                }
                            }
                            self.new_file_name.clear();
                            saved = true;
                        }
                    });
                });
        } else {
            ui.label(RichText::new("Macros directory does not exist yet.").color(Color32::GRAY));
            if ui.button("Create macros directory").clicked() {
                let _ = std::fs::create_dir_all(&macros_dir);
            }
        }

        // Delete confirmation dialog.
        if let Some(ref path) = self.confirm_delete.clone() {
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new("Delete macro?")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!("Delete \"{}\"? This cannot be undone.", file_name));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new(
                                RichText::new("Delete").color(Color32::from_rgb(200, 80, 80)),
                            ))
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancelled = true;
                        }
                    });
                });
            if confirmed {
                if std::fs::remove_file(path).is_ok() {
                    if self.open_path.as_ref() == Some(path) {
                        self.open_path = None;
                        self.content.clear();
                        self.dirty = false;
                        self.status = None;
                    }
                }
                self.confirm_delete = None;
            } else if cancelled {
                self.confirm_delete = None;
            }
        }

        ui.separator();

        // Editor area.
        if let Some(ref path) = self.open_path.clone() {
            // File path + unsaved indicator.
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

            // Save button sits directly above the text field.
            ui.horizontal(|ui| {
                let save_btn = ui.button(if self.dirty { "Save *" } else { "Save" });
                let ctrl_s = ui.input(|i| i.modifiers.ctrl && i.key_pressed(Key::S));
                if save_btn.clicked() || ctrl_s {
                    self.save(path.clone());
                    saved = true;
                }
                if let Some(ref msg) = self.status {
                    ui.colored_label(Color32::LIGHT_GREEN, msg);
                }
            });

            ui.add_space(2.0);

            let before = self.content.clone();

            // Capture Tab key before the TextEdit consumes it.
            let tab_pressed = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Tab));

            let output = ScrollArea::vertical()
                .id_salt("editor_scroll")
                .show(ui, |ui| {
                    ui.add(
                        TextEdit::multiline(&mut self.content)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY)
                            .desired_rows(24),
                    )
                });

            // Insert four spaces at the start of the cursor's line on Tab.
            if tab_pressed {
                if let Some(mut state) = TextEdit::load_state(ui.ctx(), output.inner.id) {
                    if let Some(cursor) = state.cursor.char_range() {
                        // Find the start of the line the cursor is on.
                        let pos = cursor.primary.index.min(self.content.len());
                        let line_start = self.content[..pos].rfind('\n').map_or(0, |i| i + 1);
                        self.content.insert_str(line_start, "    ");
                        // Move cursor forward by 4 to stay in place.
                        let new_pos = egui::text::CCursor::new(pos + 4);
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(new_pos)));
                        TextEdit::store_state(ui.ctx(), output.inner.id, state);
                    }
                }
                self.dirty = true;
            }

            if self.content != before && !tab_pressed {
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
}

fn default_template() -> String {
    r#"// MacroNova Rhai macro
// See SCRIPTING.md for the full API reference.

// Example: tap Ctrl+Z
press_key("ctrl");
tap_key("z");
release_key("ctrl");
"#
    .into()
}
