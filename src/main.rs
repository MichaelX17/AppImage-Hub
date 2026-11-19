mod app;
mod appimage;
mod appmeta;
mod config;
mod desktop_entry;
mod downloader;
mod github;
mod launcher;
mod permissions;
mod secrets;
mod ui;
mod xdg;

fn main() -> eframe::Result<()> {
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
