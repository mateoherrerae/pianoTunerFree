// Sin ventana de consola en la version final.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod widgets;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Piano Tuner Free")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 560.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Piano Tuner Free",
        options,
        Box::new(|cc| Ok(Box::new(app::TunerApp::new(cc)))),
    )
}
