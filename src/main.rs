//! Native window using egui (immediate mode) via eframe.

mod editor;
mod gui;
mod toolbar;
mod welcome;

fn main() -> eframe::Result {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_fullscreen(true),
        ..Default::default()
    };
    eframe::run_native(
        "Lain Studio",
        native_options,
        Box::new(|cc| Ok(Box::new(gui::LainApp::new(cc)))),
    )
}
