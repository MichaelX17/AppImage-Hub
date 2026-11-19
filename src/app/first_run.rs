//! First-run storage setup screen.
use super::*;
impl ManagerApp {
    pub(crate) fn finish_first_run_setup(&mut self, apps_dir: PathBuf) {
        if let Err(e) = std::fs::create_dir_all(&apps_dir) {
            self.status = Some(format!("failed to create AppImage folder: {e:#}"));
            return;
        }
        let saved = self
            .config
            .lock()
            .map(|mut cfg| {
                cfg.custom_download_dir = Some(apps_dir.clone());
                cfg.save()
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("config lock poisoned")));
        if let Err(e) = saved {
            self.status = Some(format!("failed to save AppImage folder: {e:#}"));
            return;
        }

        self.first_run_setup = false;
        self.config_warning = None;
        self.repair_missing_icons();
        self.normalize_managed_filenames();
        let report = self.sync_metadata_files();
        let mut message = format!("AppImages will be stored in {}", apps_dir.display());
        if report.adopted > 0 {
            message.push_str(&format!(". Found {} existing app(s)", report.adopted));
        }
        if let Some(note) = report.note() {
            message.push_str(&format!(". {note}"));
        }
        self.status = Some(message);
    }

    pub(crate) fn render_first_run_setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::ui::theme::BG))
            .show(ctx, |ui| {
                ui.add_space((ui.available_height() * 0.16).clamp(32.0, 120.0));
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../../assets/logo.png"))
                            .fit_to_exact_size(egui::vec2(96.0, 96.0))
                            .corner_radius(egui::CornerRadius::same(20)),
                    );
                    ui.add_space(crate::ui::theme::SPACE_MD);
                    ui.label(
                        RichText::new("AppImage Hub")
                            .size(28.0)
                            .strong()
                            .color(crate::ui::theme::TEXT_PRIMARY),
                    );
                    ui.add_space(crate::ui::theme::SPACE_SM);
                    ui.label(
                        crate::ui::theme::secondary_label(
                            "Choose where your AppImages should be stored.",
                        )
                        .size(14.0),
                    );
                    for warning in [self.config_warning.clone(), self.token_warning.clone()]
                        .into_iter()
                        .flatten()
                    {
                        ui.add_space(crate::ui::theme::SPACE_MD);
                        ui.add(
                            egui::Label::new(
                                RichText::new(warning)
                                    .small()
                                    .color(crate::ui::theme::WARNING),
                            )
                            .wrap(),
                        );
                    }
                    ui.add_space(crate::ui::theme::SPACE_XL);

                    if ui
                        .add_sized([190.0, 44.0], crate::ui::theme::primary_button("Browse"))
                        .clicked()
                    {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_title("Choose AppImage storage folder")
                            .pick_folder()
                        {
                            self.finish_first_run_setup(dir);
                        }
                    }

                    ui.add_space(crate::ui::theme::SPACE_SM);
                    if ui
                        .add_sized([140.0, 38.0], crate::ui::theme::ghost_button("Default"))
                        .on_hover_text(format!("Use {}", xdg::default_managed_apps_dir().display()))
                        .clicked()
                    {
                        self.finish_first_run_setup(xdg::default_managed_apps_dir());
                    }
                });
            });
    }
}
