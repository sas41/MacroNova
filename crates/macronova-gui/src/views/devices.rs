use egui::{Color32, RichText, Ui};
use macronova_core::device::DeviceInfo;

pub struct DevicesView {
    devices: Vec<DeviceInfo>,
}

impl DevicesView {
    pub fn new(devices: Vec<DeviceInfo>) -> Self {
        Self { devices }
    }

    pub fn update_devices(&mut self, devices: Vec<DeviceInfo>) {
        self.devices = devices;
    }

    pub fn show(&self, ui: &mut Ui) {
        ui.heading("Detected Devices");
        ui.add_space(8.0);

        if self.devices.is_empty() {
            ui.label(RichText::new("No Logitech HID++ devices found.").color(Color32::GRAY));
            ui.add_space(4.0);
            ui.label("Make sure your device is connected and the udev rule is installed:");
            ui.code("sudo cp 42-macronova.rules /etc/udev/rules.d/\nsudo udevadm control --reload-rules && sudo udevadm trigger");
            return;
        }

        for dev in &self.devices {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        if dev.connected {
                            Color32::GREEN
                        } else {
                            Color32::RED
                        },
                        "●",
                    );
                    ui.heading(dev.display_name());
                });

                ui.add_space(4.0);

                egui::Grid::new(format!("dev_{}", dev.hidraw_path))
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Vendor ID:");
                        ui.label(format!("{:04X}", dev.vendor_id));
                        ui.end_row();

                        ui.label("Product ID:");
                        ui.label(format!("{:04X}", dev.product_id));
                        ui.end_row();

                        if let Some(wpid) = dev.wpid {
                            ui.label("Wireless ID:");
                            ui.label(format!("{:04X}", wpid));
                            ui.end_row();
                        }

                        if let Some((major, minor)) = dev.hidpp_version {
                            ui.label("HID++ Version:");
                            ui.label(format!("{}.{}", major, minor));
                            ui.end_row();
                        }

                        ui.label("HID++ path:");
                        ui.label(RichText::new(&dev.hidraw_path).monospace());
                        ui.end_row();

                        // Derive sibling paths for display.
                        if let Some(prefix) = dev.hidraw_path.strip_prefix("/dev/hidraw") {
                            if let Ok(n) = prefix.parse::<u32>() {
                                ui.label("Mouse path:");
                                ui.label(
                                    RichText::new(format!("/dev/hidraw{}", n.saturating_sub(2)))
                                        .monospace(),
                                );
                                ui.end_row();
                                ui.label("Kbd path:");
                                ui.label(
                                    RichText::new(format!("/dev/hidraw{}", n.saturating_sub(1)))
                                        .monospace(),
                                );
                                ui.end_row();
                            }
                        }
                    });
            });
            ui.add_space(8.0);
        }
    }
}
