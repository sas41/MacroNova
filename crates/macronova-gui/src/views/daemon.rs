use egui::{Color32, RichText, Ui};

// Embed the bundled files at compile time so the GUI can install them
// without needing to find them on disk.
const UDEV_RULES: &str = include_str!("../../../../publish/linux/42-macronova.rules");
const SERVICE_UNIT: &str = include_str!("../../../../publish/linux/macronova.service");

const UDEV_RULES_DEST: &str = "/etc/udev/rules.d/42-macronova.rules";
const SERVICE_NAME: &str = "macronova-daemon";

/// Panel for controlling the macronova-daemon systemd user service.
pub struct DaemonView {
    status_cache: Option<DaemonStatus>,
    last_check: Option<std::time::Instant>,
    output: String,
    setup_status: SetupStatus,
    last_setup_check: Option<std::time::Instant>,
}

#[derive(Clone, PartialEq)]
enum DaemonStatus {
    Running,
    Stopped,
    Unknown(String),
}

/// Tracks which setup steps are complete.
#[derive(Default)]
struct SetupStatus {
    udev_installed: bool,
    service_installed: bool,
    service_enabled: bool,
}

impl DaemonView {
    pub fn new() -> Self {
        Self {
            status_cache: None,
            last_check: None,
            output: String::new(),
            setup_status: SetupStatus::default(),
            last_setup_check: None,
        }
    }

    pub fn show(&mut self, ui: &mut Ui) {
        ui.heading("Daemon Control");
        ui.add_space(8.0);

        // Refresh setup status every 4 seconds or on first show.
        let setup_stale = self
            .last_setup_check
            .map(|t| t.elapsed() > std::time::Duration::from_secs(4))
            .unwrap_or(true);
        if setup_stale {
            self.refresh_setup_status();
        }

        // Refresh daemon status every 2 seconds.
        let status_stale = self
            .last_check
            .map(|t| t.elapsed() > std::time::Duration::from_secs(2))
            .unwrap_or(true);
        if status_stale {
            self.refresh_status();
        }

        // ── Setup checklist ──────────────────────────────────────────────────
        ui.label(RichText::new("Setup").strong());
        ui.add_space(4.0);

        self.show_setup_row(
            ui,
            self.setup_status.udev_installed,
            "udev rules installed",
            "Install udev rules",
            "Grants access to /dev/uinput and Logitech devices (requires sudo)",
            SetupAction::InstallUdev,
        );

        self.show_setup_row(
            ui,
            self.setup_status.service_installed,
            "systemd service installed",
            "Install service",
            "Copies macronova-daemon.service to ~/.config/systemd/user/",
            SetupAction::InstallService,
        );

        self.show_setup_row(
            ui,
            self.setup_status.service_enabled,
            "autostart enabled",
            "Enable autostart",
            "Runs the daemon automatically when you log in",
            SetupAction::EnableService,
        );

        ui.add_space(8.0);
        ui.separator();

        // ── Daemon status + controls ─────────────────────────────────────────
        ui.label(RichText::new("Daemon").strong());
        ui.add_space(4.0);

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
        });

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

    // ── Private helpers ──────────────────────────────────────────────────────

    fn show_setup_row(
        &mut self,
        ui: &mut Ui,
        done: bool,
        done_label: &str,
        action_label: &str,
        tooltip: &str,
        action: SetupAction,
    ) {
        ui.horizontal(|ui| {
            if done {
                ui.colored_label(Color32::GREEN, "✔");
                ui.label(done_label);
            } else {
                ui.colored_label(Color32::GRAY, "○");
                ui.label(done_label);
                if ui
                    .add(egui::Button::new(action_label).small())
                    .on_hover_text(tooltip)
                    .clicked()
                {
                    self.run_setup_action(action);
                }
            }
        });
    }

    fn run_setup_action(&mut self, action: SetupAction) {
        match action {
            SetupAction::InstallUdev => self.install_udev_rules(),
            SetupAction::InstallService => self.install_service(),
            SetupAction::EnableService => self.enable_service(),
        }
    }

    fn install_udev_rules(&mut self) {
        // Write rules to a temp file, then use pkexec to copy it to /etc/udev/rules.d/.
        let tmp = std::env::temp_dir().join("42-macronova.rules");
        match std::fs::write(&tmp, UDEV_RULES) {
            Err(e) => {
                self.output = format!("Failed to write temp file: {e}");
                return;
            }
            Ok(_) => {}
        }

        let tmp_str = tmp.to_string_lossy().to_string();
        let result = std::process::Command::new("pkexec")
            .args([
                "sh",
                "-c",
                &format!(
                    "cp {tmp_str} {UDEV_RULES_DEST} && \
                     udevadm control --reload-rules && \
                     udevadm trigger"
                ),
            ])
            .output();

        self.output = match result {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let combined = format!("{stdout}{stderr}").trim().to_string();
                if out.status.success() {
                    format!("udev rules installed.\n{combined}")
                } else {
                    format!("pkexec exited with {}: {combined}", out.status)
                }
            }
            Err(e) => format!("Failed to run pkexec: {e}"),
        };

        let _ = std::fs::remove_file(&tmp);
        self.refresh_setup_status();
    }

    fn install_service(&mut self) {
        let service_dir = dirs_service_dir();
        if let Err(e) = std::fs::create_dir_all(&service_dir) {
            self.output = format!("Failed to create service dir: {e}");
            return;
        }

        let dest = service_dir.join(format!("{SERVICE_NAME}.service"));
        match std::fs::write(&dest, SERVICE_UNIT) {
            Ok(_) => {}
            Err(e) => {
                self.output = format!("Failed to write service file: {e}");
                return;
            }
        }

        let result = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output();

        self.output = match result {
            Ok(out) => {
                let s = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
                .trim()
                .to_string();
                if out.status.success() {
                    format!("Service installed to {}\n{s}", dest.display())
                } else {
                    format!("daemon-reload failed: {s}")
                }
            }
            Err(e) => format!("Failed to run systemctl: {e}"),
        };

        self.refresh_setup_status();
    }

    fn enable_service(&mut self) {
        let result = std::process::Command::new("systemctl")
            .args(["--user", "enable", SERVICE_NAME])
            .output();

        self.output = match result {
            Ok(out) => {
                let s = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
                .trim()
                .to_string();
                if out.status.success() {
                    format!("Autostart enabled.\n{s}")
                } else {
                    format!("enable failed: {s}")
                }
            }
            Err(e) => format!("Failed to run systemctl: {e}"),
        };

        self.refresh_setup_status();
    }

    fn refresh_setup_status(&mut self) {
        self.last_setup_check = Some(std::time::Instant::now());

        self.setup_status.udev_installed = std::path::Path::new(UDEV_RULES_DEST).exists();

        let service_file = dirs_service_dir().join(format!("{SERVICE_NAME}.service"));
        self.setup_status.service_installed = service_file.exists();

        // Check if the service is enabled (has a wants/requires symlink).
        let enabled = std::process::Command::new("systemctl")
            .args(["--user", "is-enabled", SERVICE_NAME])
            .output()
            .map(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                s == "enabled" || s == "static"
            })
            .unwrap_or(false);
        self.setup_status.service_enabled = enabled;
    }

    fn refresh_status(&mut self) {
        let out = std::process::Command::new("systemctl")
            .args(["--user", "is-active", SERVICE_NAME])
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
            .args(["--user", action, SERVICE_NAME])
            .output();
        match result {
            Ok(out) => {
                self.output = format!(
                    "$ systemctl --user {action} {SERVICE_NAME}\n{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                std::thread::sleep(std::time::Duration::from_millis(300));
                self.refresh_status();
            }
            Err(e) => {
                self.output = format!("Error: {e}");
            }
        }
    }
}

enum SetupAction {
    InstallUdev,
    InstallService,
    EnableService,
}

fn dirs_service_dir() -> std::path::PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
            std::path::PathBuf::from(home).join(".config")
        });
    base.join("systemd").join("user")
}
