//! `eframe::App` implementation (windowing, per-frame orchestration).
use super::*;

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
        // Backend tasks write the config from another thread; the generation
        // counter tells us when the cached snapshot is stale.
        if self.refresh_apps_snapshot() {
            self.request_file_scan();
        }
        self.poll_file_scan();
        self.tick_update_check(ctx);

        // One-shot auto-check on startup, honouring the persisted preference.
        // Automatic saves (the cooldown timestamps) are paused while a config
        // load warning has not been shown yet; the user must see the notice
        // before anything touches the freshly created config file.
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

        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin {
                        left: crate::ui::theme::SPACE_LG as i8,
                        right: crate::ui::theme::SPACE_LG as i8,
                        top: crate::ui::theme::SPACE_MD as i8,
                        bottom: crate::ui::theme::SPACE_MD as i8,
                    })
                    .stroke(egui::Stroke::new(1.0_f32, crate::ui::theme::BORDER)),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui, size_class);
                for (is_config, warning) in [
                    (true, self.config_warning.clone()),
                    (false, self.token_warning.clone()),
                ] {
                    let Some(warning) = warning else { continue };
                    ui.add_space(crate::ui::theme::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(warning)
                                .small()
                                .color(crate::ui::theme::WARNING),
                        );
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
                    ui.add_space(crate::ui::theme::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(status)
                                .small()
                                .color(crate::ui::theme::TEXT_SECONDARY),
                        );
                        if ui.small_button("Dismiss").clicked() {
                            self.status = None;
                        }
                    });
                }
            });

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
            let desired = (total * 0.37).clamp(360.0, 520.0);
            let max_allowed = (total - 420.0).max(320.0);
            let max_panel = desired.min(max_allowed).max(320.0).clamp(320.0, 520.0);
            let min_panel = 320.0_f32.min(max_panel);
            let default_width = self.side_panel_width.clamp(min_panel, max_panel);
            let panel = egui::SidePanel::right("editor_panel")
                .resizable(true)
                .default_width(default_width)
                .width_range(min_panel..=max_panel)
                .frame(egui::Frame::NONE.fill(crate::ui::theme::BG))
                .show(ctx, |ui| {
                    if self.show_settings {
                        crate::ui::settings_panel::render(ui, self);
                    } else if self.add_to.is_some() {
                        crate::ui::side_panel::render_add_to(ui, self, size_class);
                    } else if self.is_form_open {
                        crate::ui::side_panel::render(ui, self, size_class);
                    }
                });
            let actual = panel.response.rect.width();
            if actual.is_finite() && actual >= min_panel {
                self.side_panel_width = actual.clamp(min_panel, 520.0);
            }
        }

        crate::ui::update_state::render(ctx, self, size_class);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin::same(crate::ui::theme::SPACE_LG as i8)),
            )
            .show(ctx, |ui| {
                if panel_open && !use_side_panel {
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_panel_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let max_w = ui.available_width().min(640.0);
                            ui.set_max_width(max_w);
                            ui.horizontal(|ui| {
                                if ui
                                    .add(crate::ui::theme::secondary_button("← Back to list"))
                                    .clicked()
                                {
                                    self.show_side_panel = false;
                                }
                            });
                            ui.add_space(crate::ui::theme::SPACE_SM);
                            egui::Frame::NONE
                                .stroke(egui::Stroke::new(1.0_f32, crate::ui::theme::BORDER))
                                .corner_radius(egui::CornerRadius::same(
                                    crate::ui::theme::RADIUS_LG,
                                ))
                                .show(ui, |ui| {
                                    if self.show_settings {
                                        crate::ui::settings_panel::render(ui, self);
                                    } else if self.add_to.is_some() {
                                        crate::ui::side_panel::render_add_to(ui, self, size_class);
                                    } else if self.is_form_open {
                                        crate::ui::side_panel::render(ui, self, size_class);
                                    }
                                });
                        });
                } else {
                    // Cheap Arc clones; the app list itself is only rebuilt
                    // when the config generation changes.
                    let apps = Arc::clone(&self.snapshot.apps);
                    let meta = Arc::clone(&self.snapshot.meta);
                    crate::ui::app_list::render(ui, &apps, &meta, self, size_class);
                }
            });

        crate::ui::confirm_dialog::render(ctx, self);

        // Bottom-panel heights are cached by egui for one frame. Whenever the
        // progress list changes height (a download starts or finishes), the
        // frame that reflects the change lays the footer out against the old
        // height, leaving an unpainted gap. Requesting one extra frame
        // re-anchors the footer at the window bottom immediately instead of
        // leaving a blank block until the next input event.
        let activity_items = self.ops.len() + self.failed_ops.len();
        if activity_items != self.activity_footer_items {
            self.activity_footer_items = activity_items;
            ctx.request_repaint();
        }

        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if self.update_check.is_some() || self.hub_update_progress.is_some() {
            // Keep the progress animation at display refresh rate.
            ctx.request_repaint();
        }
        // Wake up once a second so the offline file scan notices AppImages
        // replaced or deleted while the window is otherwise idle.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}
