mod app;
mod views;

use std::sync::Arc;

use anyhow::Result;
use eframe::NativeOptions;
use egui::ViewportBuilder;
use tracing_subscriber::EnvFilter;

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/logo-256.png");

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("macronova_gui=info".parse()?))
        .init();

    let icon = match eframe::icon_data::from_png_bytes(LOGO_PNG) {
        Ok(data) => Some(Arc::new(data)),
        Err(e) => {
            eprintln!("Warning: failed to load app icon: {e}");
            None
        }
    };

    let mut viewport = ViewportBuilder::default()
        .with_title("MacroNova")
        .with_app_id("macronova-gui")
        .with_inner_size([900.0, 600.0])
        .with_min_inner_size([700.0, 400.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "MacroNova",
        options,
        Box::new(|cc| Ok(Box::new(app::MacroNovaApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}
