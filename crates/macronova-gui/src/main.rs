mod app;
mod views;

use anyhow::Result;
use eframe::NativeOptions;
use egui::ViewportBuilder;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("macronova_gui=info".parse()?))
        .init();

    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("MacroNova")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([700.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "MacroNova",
        options,
        Box::new(|cc| Ok(Box::new(app::MacroNovaApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}
