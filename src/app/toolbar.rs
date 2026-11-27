//! Top bar (ytm-rs-inspired pill search + responsive action bar).
use super::*;
use crate::ui::theme;

impl ManagerApp {
    pub(crate) fn render_top_bar(
        &mut self,
        ui: &mut egui::Ui,
        size_class: SizeClass,
        has_sidebar: bool,
    ) {
        let show_actions_inline = matches!(size_class, SizeClass::Large);
        let tight = matches!(size_class, SizeClass::Tight);

        ui.horizontal(|ui| {
            // When the left sidebar is hidden (Narrow / Tight), show brand + quick nav menu
            if !has_sidebar {
                ui.add(
                    egui::Image::new(egui::include_image!("../../assets/logo.png"))
                        .fit_to_exact_size(egui::vec2(26.0, 26.0))
                        .corner_radius(egui::CornerRadius::same(6)),
                );
                ui.add_space(4.0);
                if tight {
                    ui.label(
                        RichText::new("Hub")
                            .size(18.0)
                            .strong()
                            .color(theme::text_primary()),
                    )
                    .on_hover_text("AppImage Hub");
                } else {
                    ui.label(
                        RichText::new("AppImage Hub")
                            .size(18.0)
                            .strong()
                            .color(theme::text_primary()),
                    );
                }
                ui.add_space(theme::SPACE_SM);
            }

            // Pill-shaped search box (ytm-rs style) when viewing the library
            if !self.show_settings {
                let search_w: f32 = match size_class {
                    SizeClass::Large => 280.0,
                    SizeClass::Medium => 220.0,
                    SizeClass::Narrow => 160.0,
                    SizeClass::Tight => 108.0,
                };
                egui::Frame::new()
                    .fill(theme::surface_raised())
                    .stroke(egui::Stroke::new(1.0_f32, theme::border()))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_PILL))
                    .inner_margin(egui::Margin::symmetric(12, 5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            let clear_w: f32 = if self.search_query.is_empty() { 0.0 } else { 20.0 };
                            ui.add_sized(
                                [(search_w - clear_w - 24.0).max(60.0), 22.0],
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .frame(false)
                                    .hint_text(
                                        RichText::new("Search apps…").color(theme::text_muted()),
                                    ),
                            );
                            if !self.search_query.is_empty()
                                && ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("×")
                                                .size(14.0)
                                                .color(theme::text_muted()),
                                        )
                                        .frame(false),
                                    )
                                    .on_hover_text("Clear search")
                                    .clicked()
                            {
                                self.search_query.clear();
                            }
                        });
                    });
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_toolbar_actions(ui, show_actions_inline, has_sidebar);
            });
        });

        // On Narrow / Tight viewports (no sidebar), show a clean wrapped filter pill strip
        if !has_sidebar && !self.show_settings {
            ui.add_space(theme::SPACE_SM);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let updates_count = self.updates_available_count();
                for filter in NavFilter::ALL {
                    let active = self.nav_filter == filter;
                    let label = if filter == NavFilter::Updates && updates_count > 0 {
                        format!("Updates ({updates_count})")
                    } else {
                        filter.label().to_string()
                    };
                    let btn = if active {
                        theme::primary_button(label).min_size(egui::vec2(64.0, 26.0))
                    } else {
                        theme::ghost_button(label).min_size(egui::vec2(64.0, 26.0))
                    };
                    if ui.add(btn).clicked() {
                        self.nav_filter = filter;
                    }
                }
            });
        }
    }

    fn update_all_label_text(&self) -> String {
        let updates_count = self.updates_available_count();
        if updates_count > 0 {
            format!("Update All ({updates_count})")
        } else {
            "Update All".to_string()
        }
    }

    fn check_updates_label_text(&self) -> &'static str {
        if self.update_check.is_some() {
            "Checking updates…"
        } else {
            "Check All Updates"
        }
    }

    pub(crate) fn render_toolbar_actions(
        &mut self,
        ui: &mut egui::Ui,
        inline: bool,
        has_sidebar: bool,
    ) {
        if inline {
            if self.hub_update_url.is_some() || self.hub_update_progress.is_some() {
                let hub_progress = self.hub_update_progress;
                let hub_enabled = hub_progress.is_none();
                let hub_clicked = progress_button(
                    ui,
                    "Update Hub",
                    "Updating Hub…",
                    hub_progress,
                    hub_enabled,
                    148.0,
                )
                .clicked();
                if hub_clicked && hub_enabled {
                    self.start_hub_update();
                }
            }

            let updates_count = self.updates_available_count();
            let has_active_downloads = !self.ops.is_empty();
            let update_all_label = self.update_all_label_text();
            let update_all_clicked = ui
                .add_enabled(
                    !has_active_downloads && updates_count > 0,
                    theme::ghost_button(update_all_label),
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
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !has_sidebar {
                    if ui
                        .add(theme::add_button("+ Add").min_size(egui::vec2(68.0, 30.0)))
                        .clicked()
                    {
                        if self.show_settings {
                            self.close_settings(ui.ctx());
                        }
                        self.open_add_form();
                    }
                    let settings_btn = if self.show_settings {
                        theme::primary_button("Settings").min_size(egui::vec2(74.0, 30.0))
                    } else {
                        theme::secondary_button("Settings").min_size(egui::vec2(74.0, 30.0))
                    };
                    if ui
                        .add(settings_btn)
                        .on_hover_text("Themes, storage, updates, and GitHub API")
                        .clicked()
                    {
                        if self.show_settings {
                            self.close_settings(ui.ctx());
                        } else {
                            self.open_settings();
                        }
                    }
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
