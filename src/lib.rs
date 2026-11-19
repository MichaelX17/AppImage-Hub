//! AppImage Hub library: GUI state, backend workers, and release-facing helpers.
//!
//! The binary is a thin wrapper around [`run`]; integration tests in `tests/`
//! exercise the same public modules that ship in the application.

pub mod app;
pub mod appimage;
pub mod appmeta;
pub mod args;
pub mod backend;
pub mod config;
pub mod desktop_entry;
pub mod downloader;
pub mod fsutil;
pub mod github;
pub mod launcher;
pub mod permissions;
pub mod scan;
pub mod secrets;
pub mod storage;
pub mod ui;
pub mod xdg;

/// Starts the AppImage Hub desktop application.
pub fn run() -> eframe::Result<()> {
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/logo.png")).ok();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1270.0, 802.0])
        .with_min_inner_size([480.0, 360.0])
        .with_app_id("appimage-hub");

    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        persistence_path: Some(config::config_dir().join("window_state.json")),
        ..Default::default()
    };
    eframe::run_native(
        "AppImage Hub",
        options,
        Box::new(|cc| Ok(Box::new(app::ManagerApp::new(cc)))),
    )
}
