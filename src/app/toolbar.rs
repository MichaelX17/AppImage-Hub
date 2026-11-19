//! Top bar and Actions menu rendering.
use super::*;
impl ManagerApp {
    pub(crate) fn render_top_bar(&mut self, ui: &mut egui::Ui, size_class: SizeClass) {
        let full_brand = "AppImage Hub";
        let show_actions_inline = matches!(size_class, SizeClass::Large);
        let tight = matches!(size_class, SizeClass::Tight);

        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../assets/logo.png"))
                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                    .corner_radius(egui::CornerRadius::same(6)),
            );
            ui.add_space(6.0);
            if tight {
                ui.label(
                    RichText::new("AIM")
                        .size(22.0)
                        .strong()
                        .color(crate::ui::theme::TEXT_PRIMARY),
                )
                .on_hover_text(full_brand);
            } else {
                ui.label(
                    RichText::new(full_brand)
                        .size(22.0)
                        .strong()
                        .color(crate::ui::theme::TEXT_PRIMARY),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .size(11.0)
                        .strong()
                        .color(crate::ui::theme::TEXT_MUTED),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_toolbar_actions(ui, show_actions_inline);
            });
        });
    }

    /// Single source of truth for the "Update All" label.
    fn update_all_label_text(&self) -> String {
        let updates_count = self.updates_available_count();
        if updates_count > 0 {
            format!("Update All ({updates_count})")
        } else {
            "Update All".to_string()
        }
    }

    /// Single source of truth for the "Check All Updates" label.
    fn check_updates_label_text(&self) -> &'static str {
        if self.update_check.is_some() {
            "Checking updates…"
        } else {
            "Check All Updates"
        }
    }

    pub(crate) fn render_toolbar_actions(&mut self, ui: &mut egui::Ui, inline: bool) {
        if inline {
            // Primary "Add" action: neon pill with a hover glow.
            let add = ui.add(crate::ui::theme::add_button("+ Add AppImage"));
            if add.hovered() {
                crate::ui::theme::draw_glow(
                    ui,
                    add.rect,
                    crate::ui::theme::RADIUS_PILL,
                    crate::ui::theme::ACCENT,
                );
            }
            if add.clicked() {
                self.open_add_form();
            }

            let updates_count = self.updates_available_count();
            let has_active_downloads = !self.ops.is_empty();
            let update_all_label = self.update_all_label_text();
            let update_all_clicked = ui
                .add_enabled(
                    !has_active_downloads && updates_count > 0,
                    crate::ui::theme::ghost_button(update_all_label),
                )
                .on_disabled_hover_text(if has_active_downloads {
                    "Wait for active downloads to finish"
                } else {
                    "No updates available"
                })
                .clicked();
            if update_all_clicked {
                self.update_all();
            }

            let check_progress = self
                .update_check
                .as_ref()
                .map(UpdateCheckProgress::fraction);
            let checking = check_progress.is_some();
            let check_clicked = check_updates_button(ui, check_progress, !checking).clicked();
            if check_clicked && !checking {
                self.check_all_updates();
            }

            if self.hub_update_url.is_some() || self.hub_update_progress.is_some() {
                let hub_progress = self.hub_update_progress;
                let hub_enabled = hub_progress.is_none();
                let hub_clicked = progress_button(
                    ui,
                    "Update Hub",
                    "Updating Hub…",
                    hub_progress,
                    hub_enabled,
                    152.0,
                )
                .clicked();
                if hub_clicked && hub_enabled {
                    self.start_hub_update();
                }
            }

            ui.add_space(4.0);
            if ui
                .add(crate::ui::theme::secondary_button("Settings"))
                .on_hover_text("Storage, updates, and GitHub API settings")
                .clicked()
            {
                self.open_settings();
            }
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(crate::ui::theme::add_button("+ Add")).clicked() {
                    self.open_add_form();
                }
                if ui
                    .add(crate::ui::theme::secondary_button("Settings"))
                    .on_hover_text("Storage, updates, and GitHub API settings")
                    .clicked()
                {
                    self.open_settings();
                }
                ui.menu_button("Actions", |ui| {
                    if self.hub_update_url.is_some() {
                        if self.hub_update_progress.is_some() {
                            ui.add_enabled(false, egui::Button::new("Updating Hub…"));
                        } else if ui.button("Update Hub").clicked() {
                            self.start_hub_update();
                            ui.close_menu();
                        }
                        ui.separator();
                    }

                    let checking = self.update_check.is_some();
                    let check_label = self.check_updates_label_text();
                    let check_clicked = ui
                        .add_enabled(!checking, egui::Button::new(check_label))
                        .clicked();
                    if check_clicked {
                        self.check_all_updates();
                        ui.close_menu();
                    }

                    let updates_count = self.updates_available_count();
                    let has_active_downloads = !self.ops.is_empty();
                    let update_all_label = self.update_all_label_text();
                    let update_all_clicked = ui
                        .add_enabled(
                            !has_active_downloads && updates_count > 0,
                            egui::Button::new(update_all_label),
                        )
                        .clicked();
                    if update_all_clicked {
                        self.update_all();
                        ui.close_menu();
                    }
                });
            });
        }
    }
}
