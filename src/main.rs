mod app;
mod config;
mod desktop_entry;
mod downloader;
mod github;
mod launcher;
mod permissions;
mod ui;
mod xdg;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        persistence_path: Some(
            dirs::config_dir()
                .unwrap_or_else(|| std::env::current_dir().unwrap())
                .join("appimage-manager")
                .join("window_state.json"),
        ),
        ..Default::default()
    };
    eframe::run_native(
        "AppImage Manager",
        options,
        Box::new(|cc| Ok(Box::new(app::ManagerApp::new(cc)))),
    )
}
