use egui::Ui;
use macronova_core::config::{Config, WarpMode};

pub struct SettingsView;

impl SettingsView {
    pub fn new() -> Self {
        Self
    }

    /// Render the settings panel.  Returns `Some(updated_config)` if the user
    /// changed a setting and it should be saved.
    pub fn show(&mut self, ui: &mut Ui, config: &Config) -> Option<Config> {
        let mut updated = config.clone();
        let mut changed = false;

        ui.heading("Settings");
        ui.add_space(8.0);

        ui.group(|ui| {
            ui.label(egui::RichText::new("Cursor warping (Linux / Wayland)").strong());
            ui.add_space(4.0);
            ui.label(
                "On Wayland the kernel ignores repeated warp_mouse() calls that land on \
                the same position. Choose how to work around this:",
            );
            ui.add_space(6.0);

            let prev = updated.warp_mode;

            ui.radio_value(
                &mut updated.warp_mode,
                WarpMode::Jitter,
                "Jitter (recommended)",
            );
            ui.label(
                egui::RichText::new(
                    "  Sends a one-pixel offset first, then the real position. \
                     Works on all compositors.",
                )
                .weak()
                .small(),
            );
            ui.add_space(4.0);

            ui.radio_value(&mut updated.warp_mode, WarpMode::Direct, "Direct");
            ui.label(
                egui::RichText::new(
                    "  Declares INPUT_PROP_DIRECT (tablet/touchscreen mode). \
                     Each event is always applied. May behave differently on some compositors.",
                )
                .weak()
                .small(),
            );

            if updated.warp_mode != prev {
                changed = true;
            }
        });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Note: changing warp mode requires restarting the daemon to take effect.",
            )
            .italics()
            .weak(),
        );

        if changed {
            Some(updated)
        } else {
            None
        }
    }
}
