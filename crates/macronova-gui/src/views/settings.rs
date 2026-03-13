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

        ui.add_space(12.0);

        ui.group(|ui| {
            ui.label(egui::RichText::new("Virtual Mode").strong());
            ui.add_space(4.0);
            ui.label(
                "When enabled, MacroNova grabs the input device exclusively and can \
                intercept individual button presses — consuming them so they never \
                reach the OS. All other events (motion, scroll, non-intercepted \
                buttons) are transparently re-injected so the device works normally.\n\n\
                When disabled (default), the daemon never grabs the device and the \
                Intercept option on bindings has no effect.",
            );
            ui.add_space(6.0);

            let prev = updated.virtual_mode;
            ui.checkbox(&mut updated.virtual_mode, "Enable Virtual Mode");
            if updated.virtual_mode != prev {
                changed = true;
            }
        });

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Note: toggling Virtual Mode takes effect immediately on the next \
                config reload (the daemon hot-reloads config.toml automatically).",
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
