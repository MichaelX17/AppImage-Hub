use crate::app::{AppDownloadState, BackendCommand, ManagerApp};
use crate::config::{AppEntry, AppSource};
use crate::ui::breakpoints::SizeClass;
use crate::ui::format;
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Vec2};

#[derive(Clone, Copy)]
enum CardState {
    Idle,
    Downloading,
    Failed,
}

/// Per-card layout decisions based on available width and card state.
struct CardLayout {
    /// Whether to stack icon+info and then actions vertically.
    stack_actions: bool,
    /// Number of action columns in the inline (non-stacked) grid.
    /// Only meaningful when `stack_actions` is false.
    action_cols: usize,
    /// Minimum width allocated to the info block.
    info_min_width: f32,
    /// Target button size for inline action grids.
    action_btn_size: egui::Vec2,
}

fn card_layout(ui: &egui::Ui, ui_width: f32, state: CardState) -> CardLayout {
    card_layout_params(ui.spacing().interact_size.y, ui_width, state)
}

fn card_layout_params(interact_y: f32, ui_width: f32, state: CardState) -> CardLayout {
    // For non-idle states we show only 1 action (cancel/retry).
    let _n_actions = match state {
        CardState::Idle => 6,
        CardState::Downloading | CardState::Failed => 1,
    };
    match SizeClass::from_width(ui_width) {
        SizeClass::Large => CardLayout {
            stack_actions: false,
            action_cols: 3,
            info_min_width: 120.0,
            // Width sized to fit label text comfortably; will be overridden per-button.
            action_btn_size: egui::vec2(120.0, 30.0_f32.max(interact_y)),
        },
        SizeClass::Medium => CardLayout {
            stack_actions: false,
            action_cols: 2,
            info_min_width: 120.0,
            action_btn_size: egui::vec2(110.0, 30.0_f32.max(interact_y)),
        },
        SizeClass::Narrow | SizeClass::Tight => CardLayout {
            stack_actions: true,
            action_cols: 0,
            info_min_width: 120.0,
            action_btn_size: egui::vec2(110.0, 32.0_f32.max(interact_y)),
        },
    }
}

pub fn render(ui: &mut egui::Ui, apps: &[AppEntry], mgr: &mut ManagerApp, size_class: SizeClass) {
    let _ = size_class;
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

/// Collected action intents from card buttons; avoids &mut bool proliferation.
#[derive(Default)]
struct CardActions {
    cancel_op_id: Option<String>,
    retry_app: bool,
    launch: bool,
    edit: bool,
    update: bool,
    add_to: bool,
    delete: bool,
    show_folder: bool,
}

fn render_app_card(ui: &mut egui::Ui, mgr: &mut ManagerApp, app: &AppEntry) {
    let update_info = mgr.update_available.get(&app.id).cloned();
    let latest_tag = update_info.as_ref().map(|(tag, _, _)| tag.clone());
    let has_update = update_info.is_some();

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

    let card_state = if downloading.is_some() {
        CardState::Downloading
    } else if failed_msg.is_some() {
        CardState::Failed
    } else {
        CardState::Idle
    };

    let mut actions = CardActions::default();

    theme::card_frame().show(ui, |ui| {
        let card_width = ui.available_width();
        let layout = card_layout(ui, card_width, card_state);
        let icon_size = if card_width < 360.0 { 56.0 } else { 76.0 };

        if layout.stack_actions {
            // Narrow/Tight: icon + info row, then actions below
            ui.horizontal(|ui| {
                render_icon(ui, app, icon_size);
                ui.add_space(if card_width < 360.0 {
                    theme::SPACE_MD
                } else {
                    theme::SPACE_LG
                });
                ui.vertical(|ui| {
                    let info_w = ui.available_width().max(0.0);
                    ui.set_min_width(info_w.min(layout.info_min_width));
                    ui.set_max_width(info_w);
                    render_info_block(ui, mgr, app, latest_tag.as_deref(), has_update);
                });
            });
            ui.add_space(theme::SPACE_MD);
            render_actions(
                ui,
                &layout,
                card_state,
                has_update,
                downloading.as_ref(),
                failed_msg.as_ref(),
                &mut actions,
            );
        } else {
            // Wide/Medium: icon | info block | actions (flush right)
            ui.horizontal(|ui| {
                render_icon(ui, app, icon_size);
                ui.add_space(theme::SPACE_LG);

                let avail_w = ui.available_width();
                let avail_w = if avail_w.is_finite() { avail_w } else { 600.0 };

                // Compute action grid width from column count × button width + spacing
                let cols = layout.action_cols;
                let btn_w = layout.action_btn_size.x;
                let spacing_x = ui.spacing().item_spacing.x;
                let actions_width = cols as f32 * btn_w + cols.saturating_sub(1) as f32 * spacing_x;
                let info_width =
                    (avail_w - actions_width - theme::SPACE_MD).max(layout.info_min_width);

                ui.allocate_ui_with_layout(
                    egui::vec2(info_width, ui.available_height()),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_min_width(layout.info_min_width);
                        render_info_block(ui, mgr, app, latest_tag.as_deref(), has_update);
                    },
                );

                // Actions flush-right
                let remaining = ui.available_width();
                if remaining > actions_width {
                    ui.add_space(remaining - actions_width);
                }
                ui.vertical(|ui| {
                    render_actions(
                        ui,
                        &layout,
                        card_state,
                        has_update,
                        downloading.as_ref(),
                        failed_msg.as_ref(),
                        &mut actions,
                    );
                });
            });
        }
    });

    // Apply actions after card is rendered (avoids borrow conflicts)
    if let Some(op_id) = actions.cancel_op_id {
        mgr.cancel_op(&op_id);
    }
    if actions.retry_app {
        mgr.retry_failed_for_app(&app.id);
    }
    if actions.launch {
        mgr.send_cmd(BackendCommand::Launch {
            app_id: app.id.clone(),
        });
    }
    if actions.edit {
        mgr.open_edit(app);
    }
    if actions.update {
        if let Some((tag, url, asset_name)) = update_info {
            mgr.start_update_download(app.id.clone(), url, tag, asset_name);
        }
    }
    if actions.add_to {
        mgr.open_add_to(app);
    }
    if actions.show_folder {
        if let Some(dir) = app.local_path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
    if actions.delete {
        mgr.request_delete(app.id.clone());
    }
}

fn render_info_block(
    ui: &mut egui::Ui,
    mgr: &ManagerApp,
    app: &AppEntry,
    latest_tag: Option<&str>,
    has_update: bool,
) {
    ui.horizontal(|ui| {
        ui.add(egui::Label::new(RichText::new(&app.display_name).strong().size(18.0)).truncate());
        if let Some(generic) = &app.generic_name {
            let g = generic.trim();
            if !g.is_empty() {
                ui.label(theme::secondary_label(format!("({g})")).small());
            }
        }
    });

    if let Some(comment) = &app.comment {
        let c = comment.trim();
        if !c.is_empty() {
            ui.add(egui::Label::new(theme::secondary_label(c).small()).truncate())
                .on_hover_text(c);
        }
    }

    if let Some(source) = source_text(app) {
        ui.add(egui::Label::new(theme::muted_label(source.clone()).small()).truncate())
            .on_hover_text(source);
    } else {
        ui.label(theme::muted_label("Source: local file").small());
    }

    ui.add_space(theme::SPACE_XS);
    let current = if app.current_version.trim().is_empty() {
        "unknown"
    } else {
        app.current_version.as_str()
    };
    ui.label(theme::secondary_label(format!("Current: {}", current)));

    let latest_known = latest_tag
        .or_else(|| mgr.latest_versions.get(&app.id).map(|s| s.as_str()))
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
                // Update available: color + ↑ arrow so it's not color-alone
                ui.label(theme::update_available_label(tag));
            } else {
                // Up to date: color + ✓ so it's not color-alone
                ui.label(theme::up_to_date_label(tag));
            }
        }
        None => {
            ui.label(theme::muted_label("Latest: unknown — check for updates").small());
        }
    }
}

fn render_actions(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    state: CardState,
    has_update: bool,
    downloading: Option<&(String, f32, u64, Option<u64>, std::time::Instant)>,
    failed_msg: Option<&(String, bool)>,
    actions: &mut CardActions,
) {
    match state {
        CardState::Downloading => {
            if let Some((op_id, fraction, downloaded, total, started_at)) = downloading {
                render_downloading(
                    ui,
                    layout,
                    op_id,
                    *fraction,
                    *downloaded,
                    *total,
                    *started_at,
                    &mut actions.cancel_op_id,
                );
            }
        }
        CardState::Failed => {
            if let Some((msg, can_retry)) = failed_msg {
                render_failed(ui, layout, msg, *can_retry, &mut actions.retry_app);
            }
        }
        CardState::Idle => {
            render_idle_actions(ui, layout, has_update, actions);
        }
    }
}

fn render_downloading(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    op_id: &str,
    fraction: f32,
    downloaded: u64,
    total: Option<u64>,
    started_at: std::time::Instant,
    cancel_op_id: &mut Option<String>,
) {
    let fraction = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let eta = format::format_eta(started_at, downloaded, total);
    let progress = format::format_progress(downloaded, total);

    if layout.stack_actions {
        ui.vertical(|ui| {
            let w = ui.available_width();
            ui.add_sized(
                egui::vec2(w, ui.spacing().interact_size.y),
                egui::ProgressBar::new(fraction)
                    .show_percentage()
                    .fill(theme::ACCENT),
            );
            ui.label(theme::secondary_label(format!("{} • ETA {}", progress, eta)).small());
            if ui
                .add_sized(
                    egui::vec2(w, layout.action_btn_size.y),
                    theme::secondary_button("Cancel"),
                )
                .clicked()
            {
                *cancel_op_id = Some(op_id.to_string());
            }
        });
    } else {
        ui.vertical(|ui| {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .show_percentage()
                    .fill(theme::ACCENT),
            );
            ui.label(theme::secondary_label(format!("{} • ETA {}", progress, eta)).small());
            if ui
                .add_sized(layout.action_btn_size, theme::secondary_button("Cancel"))
                .clicked()
            {
                *cancel_op_id = Some(op_id.to_string());
            }
        });
    }
}

fn render_failed(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    msg: &str,
    can_retry: bool,
    retry_app: &mut bool,
) {
    if layout.stack_actions {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(msg).color(theme::DANGER).small()).truncate())
                .on_hover_text(msg);
            let w = ui.available_width();
            let clicked = ui
                .add_enabled_ui(can_retry, |ui| {
                    ui.add_sized(
                        egui::vec2(w, layout.action_btn_size.y),
                        theme::primary_button("Retry"),
                    )
                })
                .inner
                .on_disabled_hover_text("Retry unavailable")
                .clicked();
            if clicked {
                *retry_app = true;
            }
        });
    } else {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(msg).color(theme::DANGER).small()).truncate())
                .on_hover_text(msg);
            if ui
                .add_enabled_ui(can_retry, |ui| {
                    ui.add_sized(layout.action_btn_size, theme::primary_button("Retry"))
                })
                .inner
                .clicked()
            {
                *retry_app = true;
            }
        });
    }
}

fn render_idle_actions(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    has_update: bool,
    actions: &mut CardActions,
) {
    if layout.stack_actions {
        // Narrow/Tight: 2-wide button row for primary actions, then overflow menu
        let w = ui.available_width().max(120.0);
        ui.vertical(|ui| {
            ui.set_width(w);
            ui.horizontal_wrapped(|ui| {
                let half = ((w - ui.spacing().item_spacing.x) / 2.0).max(100.0);
                // Primary: Launch
                if ui
                    .add_sized(
                        egui::vec2(half, layout.action_btn_size.y),
                        theme::primary_button("Launch"),
                    )
                    .clicked()
                {
                    actions.launch = true;
                }
                // Conditional primary: Update (purple when available, ghost when not)
                let update_btn = if has_update {
                    theme::update_button()
                } else {
                    theme::secondary_button("Update")
                };
                if ui
                    .add_enabled_ui(has_update, |ui| {
                        ui.add_sized(egui::vec2(half, layout.action_btn_size.y), update_btn)
                    })
                    .inner
                    .on_disabled_hover_text("No update available")
                    .clicked()
                {
                    actions.update = true;
                }
            });
            ui.add_space(theme::SPACE_XS);
            ui.horizontal(|ui| {
                ui.menu_button("More actions", |ui| {
                    if ui.button("Edit").clicked() {
                        actions.edit = true;
                        ui.close_menu();
                    }
                    if ui.button("Show in Folder").clicked() {
                        actions.show_folder = true;
                        ui.close_menu();
                    }
                    if ui.button("Add to..").clicked() {
                        actions.add_to = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.add(theme::danger_button("Delete")).clicked() {
                        actions.delete = true;
                        ui.close_menu();
                    }
                });
            });
        });
    } else {
        // Wide/Medium: aligned button rows.
        // Action order (left-to-right per design spec):
        //   Row 1: Launch, Update, Edit         (cols: 3 for Large, 2 for Medium)
        //   Row 2: Show in Folder, Add to, Delete
        let btn_size = layout.action_btn_size;
        let cols = layout.action_cols;

        ui.vertical(|ui| {
            if cols == 3 {
                // Row 1: Launch | Update | Edit
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(btn_size, theme::primary_button("Launch"))
                        .clicked()
                    {
                        actions.launch = true;
                    }
                    let update_btn = if has_update {
                        theme::update_button()
                    } else {
                        theme::ghost_button("Update")
                    };
                    if ui
                        .add_enabled_ui(has_update, |ui| ui.add_sized(btn_size, update_btn))
                        .inner
                        .on_disabled_hover_text("No update available")
                        .clicked()
                    {
                        actions.update = true;
                    }
                    if ui
                        .add_sized(btn_size, theme::secondary_button("Edit"))
                        .clicked()
                    {
                        actions.edit = true;
                    }
                });

                ui.add_space(theme::SPACE_SM);

                // Row 2: Show in Folder | Add to | Delete
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(btn_size, theme::ghost_button("Show in Folder"))
                        .clicked()
                    {
                        actions.show_folder = true;
                    }
                    if ui
                        .add_sized(btn_size, theme::ghost_button("Add to.."))
                        .clicked()
                    {
                        actions.add_to = true;
                    }
                    if ui
                        .add_sized(btn_size, theme::danger_button("Delete"))
                        .clicked()
                    {
                        actions.delete = true;
                    }
                });
            } else {
                // 2-column Medium layout:
                // Row 1: Launch | Update
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(btn_size, theme::primary_button("Launch"))
                        .clicked()
                    {
                        actions.launch = true;
                    }
                    let update_btn = if has_update {
                        theme::update_button()
                    } else {
                        theme::ghost_button("Update")
                    };
                    if ui
                        .add_enabled_ui(has_update, |ui| ui.add_sized(btn_size, update_btn))
                        .inner
                        .on_disabled_hover_text("No update available")
                        .clicked()
                    {
                        actions.update = true;
                    }
                });

                ui.add_space(theme::SPACE_SM);

                // Row 2: Edit | Show in Folder
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(btn_size, theme::secondary_button("Edit"))
                        .clicked()
                    {
                        actions.edit = true;
                    }
                    if ui
                        .add_sized(btn_size, theme::ghost_button("Show in Folder"))
                        .clicked()
                    {
                        actions.show_folder = true;
                    }
                });

                ui.add_space(theme::SPACE_SM);

                // Row 3: Add to.. | Delete
                ui.horizontal(|ui| {
                    if ui
                        .add_sized(btn_size, theme::ghost_button("Add to.."))
                        .clicked()
                    {
                        actions.add_to = true;
                    }
                    if ui
                        .add_sized(btn_size, theme::danger_button("Delete"))
                        .clicked()
                    {
                        actions.delete = true;
                    }
                });
            }
        });
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
    let size = if size.is_finite() && size > 0.0 {
        size.clamp(1.0, 256.0)
    } else {
        76.0
    };

    let avail = ui.available_size();
    if !avail.x.is_finite() || !avail.y.is_finite() {
        return;
    }

    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let rounding = egui::CornerRadius::same(theme::RADIUS_MD);
    ui.painter()
        .rect_filled(rect, rounding, theme::SURFACE_RAISED);
    ui.painter().rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0_f32, theme::BORDER),
        egui::StrokeKind::Inside,
    );

    let icon_file = app
        .icon_path
        .as_ref()
        .filter(|p| p.exists())
        .cloned()
        .or_else(|| {
            app.local_path.parent().and_then(|folder| {
                let p1 = folder.join("icon.png");
                if p1.exists() {
                    return Some(p1);
                }
                let p2 = folder.join("icon.svg");
                if p2.exists() {
                    return Some(p2);
                }
                None
            })
        });

    if let Some(path) = &icon_file {
        let uri = format!("file://{}", path.display());
        let image = egui::Image::new(uri).corner_radius(rounding);
        let inner = rect.shrink(6.0);
        if inner.is_finite() && inner.width() > 1.0 && inner.height() > 1.0 {
            ui.put(inner, image);
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
        egui::FontId::proportional((size * 0.42).max(1.0)),
        theme::ACCENT_HOVER,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_card_layout_responsiveness() {
        // Large width >= 1200
        let large = card_layout_params(30.0, 1400.0, CardState::Idle);
        assert!(!large.stack_actions);
        assert_eq!(large.action_cols, 3);

        // Medium width 900..1200
        let medium = card_layout_params(30.0, 1000.0, CardState::Idle);
        assert!(!medium.stack_actions);
        assert_eq!(medium.action_cols, 2);

        // Narrow width < 900
        let narrow = card_layout_params(30.0, 700.0, CardState::Idle);
        assert!(narrow.stack_actions);
        assert_eq!(narrow.action_cols, 0);

        // Tight width < 640
        let tight = card_layout_params(30.0, 500.0, CardState::Idle);
        assert!(tight.stack_actions);
        assert_eq!(tight.action_cols, 0);
    }
}
