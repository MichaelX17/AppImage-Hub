//! `eframe::App` implementation (windowing, `ytm-rs`-inspired responsive shell, per-frame orchestration).
use super::*;
use crate::ui::theme;

impl eframe::App for ManagerApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("side_panel_width", self.side_panel_width.to_string());
        storage.set_string("show_side_panel", self.show_side_panel.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_run_setup {
            self.render_first_run_setup(ctx);
            return;
        }
        // Defer icon repair / sidecar sync past the first painted frame.
        if self.startup_scan_pending {
            if self.startup_scan_started {
                self.startup_scan_pending = false;
                self.run_startup_scan();
            } else {
                self.startup_scan_started = true;
            }
        }
        let size_class = SizeClass::from_width(ctx.screen_rect().width());
        while let Ok(evt) = self.evt_rx.try_recv() {
            self.apply_event(evt);
        }
        if self.refresh_apps_snapshot() {
            self.request_file_scan();
        }
        self.poll_file_scan();
        self.tick_update_check(ctx);

        if !self.auto_check_done && self.config_warning.is_none() {
            self.auto_check_done = true;
            if self.auto_check_enabled() {
                self.check_all_updates();
            }
        }

        if !self.hub_update_check_started && self.config_warning.is_none() {
            self.hub_update_check_started = true;
            self.check_hub_update_if_allowed();
        }

        let show_left_sidebar = matches!(size_class, SizeClass::Large | SizeClass::Medium);

        // ── Left Navigation Sidebar (ytm-rs shell) ───────────────────────
        if show_left_sidebar {
            let sidebar_w = if size_class == SizeClass::Large {
                216.0
            } else {
                184.0
            };
            egui::SidePanel::left("nav_sidebar")
                .resizable(false)
                .exact_width(sidebar_w)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::sidebar_bg())
                        .stroke(egui::Stroke::new(1.0_f32, theme::border()))
                        .inner_margin(egui::Margin::symmetric(12, 16)),
                )
                .show(ctx, |ui| {
                    self.render_left_sidebar(ui);
                });
        }

        // ── Top Bar ──────────────────────────────────────────────────────
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(theme::bg())
                    .inner_margin(egui::Margin {
                        left: theme::SPACE_LG as i8,
                        right: theme::SPACE_LG as i8,
                        top: theme::SPACE_MD as i8,
                        bottom: theme::SPACE_MD as i8,
                    })
                    .stroke(egui::Stroke::new(1.0_f32, theme::divider_color())),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui, size_class, show_left_sidebar);
                for (is_config, warning) in [
                    (true, self.config_warning.clone()),
                    (false, self.token_warning.clone()),
                ] {
                    let Some(warning) = warning else { continue };
                    ui.add_space(theme::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(warning).small().color(theme::warning()));
                        if ui.small_button("Dismiss").clicked() {
                            if is_config {
                                self.config_warning = None;
                            } else {
                                self.token_warning = None;
                            }
                        }
                    });
                }
                let status_text = self.status.clone();
                if let Some(status) = status_text {
                    ui.add_space(theme::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(status).small().color(theme::text_secondary()));
                        if ui.small_button("Dismiss").clicked() {
                            self.status = None;
                        }
                    });
                }
            });

        // ── Right Editor Panel (Add / Edit / Shortcuts) ──────────────────
        let panel_open = self.is_right_panel_open() && self.show_side_panel;
        let use_side_panel =
            panel_open && matches!(size_class, SizeClass::Large | SizeClass::Medium);

        if use_side_panel {
            let total = ctx.screen_rect().width();
            let total = if total.is_finite() && total > 0.0 {
                total
            } else {
                800.0
            };
            let sidebar_w = if size_class == SizeClass::Large {
                216.0
            } else {
                184.0
            };
            let remaining = (total - sidebar_w).max(400.0);
            let desired = (remaining * 0.40).clamp(340.0, 480.0);
            let max_allowed = (remaining - 360.0).max(320.0);
            let max_panel = desired.min(max_allowed).max(320.0).clamp(320.0, 480.0);
            let min_panel = 320.0_f32.min(max_panel);
            let default_width = self.side_panel_width.clamp(min_panel, max_panel);
            let panel = egui::SidePanel::right("editor_panel")
                .resizable(true)
                .default_width(default_width)
                .width_range(min_panel..=max_panel)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::bg())
                        .inner_margin(egui::Margin {
                            left: 0,
                            right: theme::SPACE_MD as i8,
                            top: theme::SPACE_MD as i8,
                            bottom: theme::SPACE_MD as i8,
                        }),
                )
                .show(ctx, |ui| {
                    if self.add_to.is_some() {
                        crate::ui::side_panel::render_add_to(ui, self, size_class);
                    } else if self.is_form_open {
                        crate::ui::side_panel::render(ui, self, size_class);
                    }
                });
            let actual = panel.response.rect.width();
            if actual.is_finite() && actual >= min_panel {
                self.side_panel_width = actual.clamp(min_panel, 480.0);
            }
        }

        // ── Bottom Status / Download Progress Bar ────────────────────────
        crate::ui::update_state::render(ctx, self, size_class);

        // ── Central Content Area ─────────────────────────────────────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::bg())
                    .inner_margin(egui::Margin::same(theme::SPACE_LG as i8)),
            )
            .show(ctx, |ui| {
                if self.show_settings {
                    crate::ui::settings_panel::render(ui, self, size_class);
                } else if panel_open && !use_side_panel {
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_panel_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let max_w = ui.available_width().min(640.0);
                            ui.set_max_width(max_w);
                            ui.horizontal(|ui| {
                                if ui
                                    .add(theme::secondary_button("← Back to list"))
                                    .clicked()
                                {
                                    self.show_side_panel = false;
                                }
                            });
                            ui.add_space(theme::SPACE_SM);
                            if self.add_to.is_some() {
                                crate::ui::side_panel::render_add_to(ui, self, size_class);
                            } else if self.is_form_open {
                                crate::ui::side_panel::render(ui, self, size_class);
                            }
                        });
                } else {
                    let apps = Arc::clone(&self.snapshot.apps);
                    let meta = Arc::clone(&self.snapshot.meta);
                    crate::ui::app_list::render(ui, &apps, &meta, self, size_class);
                }
            });

        crate::ui::confirm_dialog::render(ctx, self);

        let activity_items = self.ops.len() + self.failed_ops.len();
        if activity_items != self.activity_footer_items {
            self.activity_footer_items = activity_items;
            ctx.request_repaint();
        }

        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if self.update_check.is_some() || self.hub_update_progress.is_some() {
            ctx.request_repaint();
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

impl ManagerApp {
    fn render_left_sidebar(&mut self, ui: &mut egui::Ui) {
        // Brand header
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../assets/logo.png"))
                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                    .corner_radius(egui::CornerRadius::same(7)),
            );
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new("AppImage Hub")
                        .size(16.0)
                        .strong()
                        .color(theme::text_primary()),
                );
                ui.label(
                    theme::muted_label(concat!("v", env!("CARGO_PKG_VERSION"))).size(10.5),
                );
            });
        });

        ui.add_space(theme::SPACE_LG);

        // Primary CTA pill
        let add_btn = ui.add_sized(
            [ui.available_width(), 36.0],
            theme::add_button("+ Add AppImage"),
        );
        if add_btn.hovered() {
            theme::draw_glow(ui, add_btn.rect, theme::RADIUS_PILL, theme::accent());
        }
        if add_btn.clicked() {
            if self.show_settings {
                self.close_settings(ui.ctx());
            }
            self.open_add_form();
        }

        ui.add_space(theme::SPACE_LG);
        ui.label(theme::section_heading("Library"));
        ui.add_space(theme::SPACE_XS);

        let updates_count = self.updates_available_count();
        for filter in NavFilter::ALL {
            let active = !self.show_settings && self.nav_filter == filter;
            let badge = (filter == NavFilter::Updates).then_some(updates_count);
            if theme::nav_item_button(ui, filter.label(), badge, active).clicked() {
                if self.show_settings {
                    self.close_settings(ui.ctx());
                }
                self.nav_filter = filter;
            }
            ui.add_space(2.0);
        }

        // Bottom-anchored Settings item
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            if theme::nav_item_button(ui, "Settings", None, self.show_settings).clicked() {
                if self.show_settings {
                    self.close_settings(ui.ctx());
                } else {
                    self.open_settings();
                }
            }
            ui.add_space(theme::SPACE_XS);
            theme::divider(ui);
        });
    }
}
