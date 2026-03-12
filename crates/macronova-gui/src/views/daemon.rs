use egui::{Color32, RichText, Ui};

/// Panel for controlling the macronova-daemon systemd user service.
pub struct DaemonView {
    status_cache: Option<DaemonStatus>,
    last_check: Option<std::time::Instant>,
    output: String,
}

#[derive(Clone, PartialEq)]
enum DaemonStatus {
    Running,
    Stopped,
    Unknown(String),
}

impl DaemonView {
    pub fn new() -> Self {
        Self {
            status_cache: None,
            last_check: None,
            output: String::new(),
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        ui.heading("Daemon Control");
        ui.add_space(8.0);

        // Refresh status every 2 seconds or on demand.
        let refresh = self
            .last_check
            .map(|t| t.elapsed() > std::time::Duration::from_secs(2))
            .unwrap_or(true);
        if refresh {
            self.refresh_status();
        }

        // Status display.
        let (dot, label, color) = match &self.status_cache {
            Some(DaemonStatus::Running) => ("●", "Running", Color32::GREEN),
            Some(DaemonStatus::Stopped) => ("●", "Stopped", Color32::RED),
            Some(DaemonStatus::Unknown(_)) => ("●", "Unknown", Color32::GRAY),
            None => ("○", "Checking...", Color32::GRAY),
        };

        ui.horizontal(|ui| {
            ui.colored_label(color, dot);
            ui.label(RichText::new(label).strong());
            if ui.small_button("Refresh").clicked() {
                self.refresh_status();
            }
        });

        ui.add_space(8.0);

        // Control buttons.
        ui.horizontal(|ui| {
            if ui.button("Start").clicked() {
                self.run_systemctl("start");
            }
            if ui.button("Stop").clicked() {
                self.run_systemctl("stop");
            }
            if ui.button("Restart").clicked() {
                self.run_systemctl("restart");
            }
            if ui.button("Enable (autostart)").clicked() {
                self.run_systemctl("enable");
            }
            if ui.button("Disable").clicked() {
                self.run_systemctl("disable");
            }
        });

        ui.add_space(8.0);
        ui.separator();
        ui.label(RichText::new("Install & Setup").strong());
        ui.add_space(4.0);
        ui.label("To install the daemon as a systemd user service:");
        ui.code("cp macronova.service ~/.config/systemd/user/\nsystemctl --user daemon-reload\nsystemctl --user enable --now macronova-daemon");
        ui.add_space(8.0);
        ui.label("To install the udev rules (requires sudo):");
        ui.code("sudo cp 42-macronova.rules /etc/udev/rules.d/\nsudo udevadm control --reload-rules && sudo udevadm trigger");

        if !self.output.is_empty() {
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("Output:").strong());
            egui::ScrollArea::vertical()
                .max_height(120.0)
                .id_salt("daemon_output")
                .show(ui, |ui| {
                    ui.code(&self.output);
                });
        }
    }

    fn refresh_status(&mut self) {
        let out = std::process::Command::new("systemctl")
            .args(["--user", "is-active", "macronova-daemon"])
            .output();
        self.last_check = Some(std::time::Instant::now());
        self.status_cache = Some(match out {
            Ok(output) => {
                let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if s == "active" {
                    DaemonStatus::Running
                } else if s == "inactive" || s == "failed" {
                    DaemonStatus::Stopped
                } else {
                    DaemonStatus::Unknown(s)
                }
            }
            Err(e) => DaemonStatus::Unknown(e.to_string()),
        });
    }

    fn run_systemctl(&mut self, action: &str) {
        let result = std::process::Command::new("systemctl")
            .args(["--user", action, "macronova-daemon"])
            .output();
        match result {
            Ok(out) => {
                self.output = format!(
                    "$ systemctl --user {} macronova-daemon\n{}{}",
                    action,
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                // Refresh status after a brief delay.
                std::thread::sleep(std::time::Duration::from_millis(300));
                self.refresh_status();
            }
            Err(e) => {
                self.output = format!("Error: {}", e);
            }
        }
    }
}
