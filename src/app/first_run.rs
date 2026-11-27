//! Modern First-Run Storage Setup Onboarding Screen.
use super::*;
use crate::ui::icons::Icon;
use crate::ui::theme::{self, ButtonTone};
use crate::ui::fonts;
use eframe::egui::{CornerRadius, Sense, vec2};

impl ManagerApp {
    pub(crate) fn finish_first_run_setup(&mut self, ctx: &egui::Context, apps_dir: PathBuf) {
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
        self.status = Some(format!("AppImages will be stored in {}", apps_dir.display()));
        self.start_background_storage_scan(ctx, false);
    }

    pub(crate) fn render_first_run_setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::bg()))
            .show(ctx, |ui| {
                theme::paint_canvas_ambient(ui, ui.max_rect());

                let avail_w = ui.available_width();
                let card_w = (avail_w - 48.0).clamp(300.0, 480.0);
                let top_pad = ((ui.available_height() - 360.0) * 0.42).clamp(24.0, 140.0);

                ui.add_space(top_pad);
                ui.vertical_centered(|ui| {
                    let (card_rect, _) = ui.allocate_exact_size(vec2(card_w, 340.0), Sense::hover());
                    theme::paint_bento_card(ui.painter(), card_rect, 0.0, false);

                    let inner = card_rect.shrink(28.0);
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(inner)
                            .layout(egui::Layout::top_down(egui::Align::Center)),
                    );

                    child.add(
                        egui::Image::new(egui::include_image!("../../assets/logo.png"))
                            .fit_to_exact_size(vec2(76.0, 76.0))
                            .corner_radius(CornerRadius::same(18)),
                    );
                    child.add_space(16.0);
                    child.label(
                        RichText::new("Welcome to AppImage Hub")
                            .font(fonts::bold(24.0))
                            .color(theme::text_primary()),
                    );
                    child.add_space(6.0);
                    child.add(
                        egui::Label::new(
                            RichText::new("Choose a folder where your managed AppImages will live.")
                                .font(fonts::regular(13.5))
                                .color(theme::text_secondary()),
                        )
                        .wrap(),
                    );

                    for warning in [self.config_warning.clone(), self.token_warning.clone()]
                        .into_iter()
                        .flatten()
                    {
                        child.add_space(10.0);
                        child.add(
                            egui::Label::new(
                                RichText::new(warning)
                                    .font(fonts::regular(12.0))
                                    .color(theme::warning()),
                            )
                            .wrap(),
                        );
                    }

                    child.add_space(26.0);
                    if theme::pill_action(
                        &mut child,
                        Some(Icon::Folder),
                        "Choose Storage Folder",
                        ButtonTone::Primary,
                        40.0,
                    )
                    .clicked()
                    {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_title("Choose AppImage storage folder")
                            .pick_folder()
                        {
                            self.finish_first_run_setup(ctx, dir);
                        }
                    }

                    child.add_space(10.0);
                    let default_dir = xdg::default_managed_apps_dir();
                    if theme::pill_action(
                        &mut child,
                        None,
                        "Use Default Folder",
                        ButtonTone::Ghost,
                        34.0,
                    )
                    .on_hover_text(format!("Use {}", default_dir.display()))
                    .clicked()
                    {
                        self.finish_first_run_setup(ctx, default_dir);
                    }
                });
            });
    }
}
