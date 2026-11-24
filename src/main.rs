//! BibleWolf — a Bible study desktop app.
//! (C) 2026 IntelligentWolf Ltd — https://wolf.uk.com — MIT License

// Release builds on Windows are GUI apps: no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bible;
mod hymnary;
mod store;
mod theme;

fn main() -> eframe::Result {
    // Before eframe opens its storage, so settings move along with notes.
    let migration = store::migrate_legacy_dir();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("BibleWolf")
        .with_app_id(store::APP_DIR)
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1000.0, 600.0]);
    match eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon/biblewolf-256.png")) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        // Cosmetic only; the app works without a window icon.
        Err(e) => eprintln!("biblewolf: window icon not loaded: {e}"),
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "BibleWolf",
        options,
        Box::new(|cc| Ok(Box::new(app::BibleApp::new(cc, migration)))),
    )
}
