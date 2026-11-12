use crate::app::{AppDownloadState, BackendCommand, ManagerApp};
use crate::config::{AppEntry, AppSource};
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Vec2};

pub fn render(ui: &mut egui::Ui, apps: &[AppEntry], mgr: &mut ManagerApp) {
    if apps.is_empty() {
        render_empty_state(ui, mgr);
        return;
    }

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for app in apps {
                ui.push_id(&app.id, |ui| {
                    render_app_card(ui, mgr, app);
                });
                ui.add_space(theme::SPACE_MD);
            }
        });
}

fn render_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(
            RichText::new("No AppImages managed yet")
                .size(20.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::muted_label(
            "Add an AppImage to manage launches, updates, and desktop entries.",
        ));
        ui.add_space(theme::SPACE_LG);
        if ui.add(theme::primary_button("Add AppImage")).clicked() {
            mgr.open_add_form();
        }
    });
}

fn render_app_card(ui: &mut egui::Ui, mgr: &mut ManagerApp, app: &AppEntry) {
    let update_info = mgr.update_available.get(&app.id).cloned();
    let latest_tag = update_info.as_ref().map(|(tag, _, _)| tag.clone());
    let has_update = update_info.is_some();

    let mut cancel_op_id: Option<String> = None;
    let mut retry_app = false;
    let mut launch_clicked = false;
    let mut edit_clicked = false;
    let mut update_clicked = false;
    let mut add_to_clicked = false;
    let mut delete_clicked = false;
    let mut show_folder_clicked = false;

    let downloading = if let AppDownloadState::Downloading(op) = mgr.app_download_state(&app.id) {
        Some((
            op.op_id.clone(),
            op.fraction,
            op.downloaded,
            op.total,
            op.started_at,
        ))
    } else {
        None
    };
    let failed_msg = if let AppDownloadState::Failed(f) = mgr.app_download_state(&app.id) {
        Some((f.message.clone(), f.retry.is_some()))
    } else {
        None
    };

    theme::card_frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(84.0);
            render_icon(ui, app, 76.0);
            ui.add_space(theme::SPACE_LG);

            let actions_width = if downloading.is_some() || failed_msg.is_some() {
                280.0
            } else if has_update {
                470.0
            } else {
                410.0
            };
            let info_width = (ui.available_width() - actions_width - theme::SPACE_MD).max(160.0);

            ui.allocate_ui_with_layout(
                Vec2::new(info_width, ui.available_height().max(84.0)),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&app.display_name).strong().size(18.0))
                            .truncate(),
                    );

                    if let Some(source) = source_text(app) {
                        ui.add(
                            egui::Label::new(theme::secondary_label(source.clone()).small())
                                .truncate(),
                        )
                        .on_hover_text(source);
                    }

                    ui.add_space(theme::SPACE_XS);
                    let current = if app.current_version.trim().is_empty() {
                        "unknown"
                    } else {
                        app.current_version.as_str()
                    };
                    ui.label(theme::secondary_label(format!("Current: {}", current)));

                    let latest_known = latest_tag
                        .as_deref()
                        .or_else(|| {
                            mgr.latest_versions
                                .get(&app.id)
                                .map(|s| s.as_str())
                        })
                        .or_else(|| {
                            if app.current_version.trim().is_empty() {
                                None
                            } else {
                                Some(current)
                            }
                        });
                    match latest_known {
                        Some(tag) => {
                            if has_update {
                                ui.horizontal(|ui| {
                                    theme::status_dot(ui, theme::ACCENT_HOVER);
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(format!("Latest: {} (update available)", tag))
                                            .strong()
                                            .color(theme::ACCENT_HOVER),
                                    );
                                });
                            } else {
                                ui.label(theme::secondary_label(format!("Latest: {}", tag)));
                            }
                        }
                        None => {
                            ui.label(theme::muted_label("Latest: —"));
                        }
                    }
                },
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some((op_id, fraction, downloaded, total, started_at)) = &downloading {
                    ui.allocate_ui_with_layout(
                        Vec2::new(actions_width.min(ui.available_width().max(200.0)), 80.0),
                        Layout::top_down(Align::Max),
                        |ui| {
                            let pb = egui::ProgressBar::new((*fraction).clamp(0.0, 1.0))
                                .show_percentage()
                                .fill(theme::ACCENT);
                            ui.add(pb);
                            let downloaded_mb = *downloaded as f64 / (1024.0 * 1024.0);
                            let total_mb = total
                                .map(|t| format!("{:.1}", t as f64 / (1024.0 * 1024.0)))
                                .unwrap_or_else(|| "?".into());
                            let eta = format_eta_parts(*started_at, *downloaded, *total);
                            ui.label(
                                theme::secondary_label(format!(
                                    "{:.1} / {} MB • ETA {}",
                                    downloaded_mb, total_mb, eta
                                ))
                                .small(),
                            );
                            if ui.add(theme::secondary_button("Cancel")).clicked() {
                                cancel_op_id = Some(op_id.clone());
                            }
                        },
                    );
                } else if let Some((msg, can_retry)) = &failed_msg {
                    ui.allocate_ui_with_layout(
                        Vec2::new(actions_width.min(ui.available_width().max(200.0)), 80.0),
                        Layout::top_down(Align::Max),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(msg).color(theme::DANGER).small())
                                    .truncate(),
                            )
                            .on_hover_text(msg);
                            if ui
                                .add_enabled(*can_retry, theme::primary_button("Retry"))
                                .clicked()
                            {
                                retry_app = true;
                            }
                        },
                    );
                } else {
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(theme::danger_button("Delete")).clicked() {
                            delete_clicked = true;
                        }
                        if ui.add(theme::secondary_button("Edit")).clicked() {
                            edit_clicked = true;
                        }
                        if ui.add(theme::ghost_button("Show in Folder")).clicked() {
                            show_folder_clicked = true;
                        }
                        if ui.add(theme::ghost_button("Add to..")).clicked() {
                            add_to_clicked = true;
                        }
                        if has_update {
                            if ui.add(theme::ghost_button("Update")).clicked() {
                                update_clicked = true;
                            }
                        }
                        if ui.add(theme::primary_button("Launch")).clicked() {
                            launch_clicked = true;
                        }
                    });
                }
            });
        });
    });

    if let Some(op_id) = cancel_op_id {
        mgr.cancel_op(&op_id);
    }
    if retry_app {
        mgr.retry_failed_for_app(&app.id);
    }
    if launch_clicked {
        mgr.send_cmd(BackendCommand::Launch {
            app_id: app.id.clone(),
        });
    }
    if edit_clicked {
        mgr.open_edit(app);
    }
    if update_clicked {
        if let Some((tag, url, asset_name)) = update_info {
            mgr.start_update_download(app.id.clone(), url, tag, asset_name);
        }
    }
    if add_to_clicked {
        mgr.open_add_to(app);
    }
    if show_folder_clicked {
        if let Some(dir) = app.local_path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
    if delete_clicked {
        mgr.request_delete(app.id.clone());
    }
}

fn source_text(app: &AppEntry) -> Option<String> {
    match &app.source {
        AppSource::LocalWithRepo { github_repo } | AppSource::GithubOnly { github_repo } => {
            Some(github_repo.clone())
        }
        AppSource::LocalOnly => None,
    }
}

fn render_icon(ui: &mut egui::Ui, app: &AppEntry, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let rounding = egui::CornerRadius::same(theme::RADIUS_MD);
    ui.painter()
        .rect_filled(rect, rounding, theme::SURFACE_RAISED);
    ui.painter().rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    if let Some(path) = &app.icon_path {
        if path.exists() {
            let uri = format!("file://{}", path.display());
            let image = egui::Image::new(uri).corner_radius(rounding);
            ui.put(rect.shrink(6.0), image);
            return;
        }
    }

    let initial = app
        .display_name
        .trim()
        .chars()
        .next()
        .unwrap_or('A')
        .to_uppercase()
        .to_string();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(size * 0.42),
        theme::ACCENT_HOVER,
    );
}

fn format_eta_parts(started_at: std::time::Instant, downloaded: u64, total: Option<u64>) -> String {
    let Some(total) = total else {
        return "--".into();
    };
    if downloaded == 0 {
        return "--".into();
    }
    let elapsed = started_at.elapsed().as_secs_f64();
    if elapsed <= 0.5 {
        return "--".into();
    }
    let rate = downloaded as f64 / elapsed;
    if rate <= 0.0 {
        return "--".into();
    }
    let remaining = total.saturating_sub(downloaded) as f64 / rate;
    if !remaining.is_finite() || remaining < 0.0 {
        return "--".into();
    }
    let secs = remaining.round() as u64;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
