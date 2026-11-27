//! Modern Bento Grid & Linear-style List view for managed AppImages.
//!
//! All backend interactions funnel through [`CardActions`] so cards render
//! cleanly without borrow conflicts.

use crate::app::{
    AppDownloadState, AppSortBy, CachedAppMeta, LocalFileIssue, ManagerApp, NavFilter, ViewMode,
};
use crate::config::{AppEntry, AppSource};
use crate::ui::breakpoints::SizeClass;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, BadgeKind, ButtonTone};
use crate::ui::{fonts, format};
use eframe::egui::{
    self, Align, Align2, CornerRadius, Layout, Rect, RichText, Sense, Stroke, StrokeKind, vec2,
};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CardState {
    Idle,
    Downloading,
    Failed,
}

#[cfg(test)]
struct CardLayout {
    icon_size: f32,
    name_size: f32,
    compact: bool,
}

#[cfg(test)]
fn card_layout_params(ui_width: f32) -> CardLayout {
    match SizeClass::from_width(ui_width) {
        SizeClass::Large | SizeClass::Medium => CardLayout {
            icon_size: 64.0,
            name_size: 21.0,
            compact: false,
        },
        SizeClass::Narrow => CardLayout {
            icon_size: 56.0,
            name_size: 18.0,
            compact: true,
        },
        SizeClass::Tight => CardLayout {
            icon_size: 48.0,
            name_size: 16.0,
            compact: true,
        },
    }
}

pub(crate) fn render(
    ui: &mut egui::Ui,
    apps: &[AppEntry],
    meta: &HashMap<String, CachedAppMeta>,
    mgr: &mut ManagerApp,
    size_class: SizeClass,
) {
    if apps.is_empty() {
        render_empty_state(ui, mgr);
        return;
    }

    let query = mgr.search_query.trim().to_lowercase();
    let nav_filter = mgr.nav_filter;
    let mut sorted_apps: Vec<&AppEntry> = apps
        .iter()
        .filter(|app| {
            let matches_nav = match nav_filter {
                NavFilter::All => true,
                NavFilter::Updates => mgr.update_available.contains_key(&app.id),
                NavFilter::Github => !matches!(app.source, AppSource::LocalOnly),
                NavFilter::Local => matches!(app.source, AppSource::LocalOnly),
            };
            if !matches_nav {
                return false;
            }
            if query.is_empty() {
                return true;
            }
            if let Some(cached) = meta.get(&app.id) {
                cached.name_lower.contains(&query)
                    || cached
                        .repo_lower
                        .as_deref()
                        .is_some_and(|r| r.contains(&query))
            } else {
                app.display_name.to_lowercase().contains(&query)
                    || source_text(app).is_some_and(|repo| repo.to_lowercase().contains(&query))
            }
        })
        .collect();

    sort_apps(&mut sorted_apps, meta, mgr.sort_by);

    egui::ScrollArea::vertical()
        .id_salt("apps_main_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Bento summary strip on All Apps when not actively searching
            if nav_filter == NavFilter::All
                && query.is_empty()
                && !matches!(size_class, SizeClass::Tight)
            {
                render_bento_summary(ui, apps, mgr, size_class);
                ui.add_space(theme::SPACE_LG);
            }

            if sorted_apps.is_empty() {
                render_filtered_empty_state(ui, mgr);
                return;
            }

            match mgr.view_mode {
                ViewMode::Grid => render_bento_grid(ui, &sorted_apps, meta, mgr),
                ViewMode::List => render_dense_list(ui, &sorted_apps, meta, mgr, size_class),
            }

            // Extra breathing room at bottom so floating activity dock never covers the last card
            let bottom_pad = if !mgr.ops.is_empty() || !mgr.failed_ops.is_empty() {
                128.0
            } else {
                24.0
            };
            ui.add_space(bottom_pad);
        });
}

// ─── Bento Overview Strip ──────────────────────────────────────────────────

fn render_bento_summary(
    ui: &mut egui::Ui,
    apps: &[AppEntry],
    mgr: &mut ManagerApp,
    size_class: SizeClass,
) {
    let total = apps.len();
    let github_count = apps
        .iter()
        .filter(|a| !matches!(a.source, AppSource::LocalOnly))
        .count();
    let local_count = total.saturating_sub(github_count);
    let updates_count = mgr.updates_available_count();
    let has_active_dl = !mgr.ops.is_empty();
    let checking = mgr.update_check.is_some();

    let gap = 14.0;
    let avail = (ui.available_width() - 2.0).max(200.0);
    let cols = if matches!(size_class, SizeClass::Large | SizeClass::Medium) && avail >= 600.0 {
        3
    } else if avail >= 420.0 {
        2
    } else {
        1
    };
    let tile_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor().max(170.0);
    let tile_h = 88.0;

    let mut trigger_update_all = false;
    let mut trigger_check_all = false;
    let mut trigger_hub_update = false;

    let hub_avail = mgr.hub_update_url.is_some() || mgr.hub_update_progress.is_some();
    let show_third = cols == 3 || hub_avail;

    let tile_size = vec2(tile_w, tile_h);

    let draw_tile_1 = |ui: &mut egui::Ui| {
        bento_stat_tile(ui, tile_size, false, Icon::AppsGrid, theme::text_muted(), "Library", |ui| {
            ui.label(
                RichText::new(format!("{total}"))
                    .font(fonts::bold(21.0))
                    .color(theme::text_primary()),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("{github_count} GitHub · {local_count} Local"))
                    .font(fonts::regular(12.0))
                    .color(theme::text_muted()),
            );
        });
    };

    let draw_tile_2 = |ui: &mut egui::Ui, trigger_update_all: &mut bool| {
        let (ic, ic_col) = if updates_count > 0 {
            (Icon::Download, theme::accent())
        } else {
            (Icon::Check, theme::success())
        };
        bento_stat_tile(ui, tile_size, updates_count > 0, ic, ic_col, "Updates", |ui| {
            if updates_count > 0 {
                ui.label(
                    RichText::new(format!("{updates_count} ready"))
                        .font(fonts::bold(17.0))
                        .color(theme::text_primary()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_enabled_ui(!has_active_dl, |ui| {
                        if theme::pill_action(
                            ui,
                            Some(Icon::Download),
                            "Update All",
                            ButtonTone::SubtleAccent,
                            30.0,
                        )
                        .clicked()
                        {
                            *trigger_update_all = true;
                        }
                    });
                });
            } else {
                ui.label(
                    RichText::new("All up to date")
                        .font(fonts::medium(15.0))
                        .color(theme::success()),
                );
            }
        });
    };

    let draw_tile_3 = |ui: &mut egui::Ui, trigger_hub: &mut bool, trigger_check: &mut bool| {
        if hub_avail {
            bento_stat_tile(
                ui,
                tile_size,
                true,
                Icon::ArrowUpRight,
                theme::accent(),
                "AppImage Hub",
                |ui| {
                    if let Some(frac) = mgr.hub_update_progress {
                        ui.label(
                            RichText::new(format!(
                                "Updating {}%",
                                (frac.clamp(0.0, 1.0) * 100.0).round() as i32
                            ))
                            .font(fonts::medium(14.5))
                            .color(theme::accent()),
                        );
                    } else {
                        ui.label(
                            RichText::new("New build")
                                .font(fonts::bold(16.0))
                                .color(theme::text_primary()),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::pill_action(
                                ui,
                                Some(Icon::Download),
                                "Update Hub",
                                ButtonTone::SubtleAccent,
                                30.0,
                            )
                            .clicked()
                            {
                                *trigger_hub = true;
                            }
                        });
                    }
                },
            );
        } else {
            bento_stat_tile(
                ui,
                tile_size,
                false,
                Icon::Refresh,
                theme::text_muted(),
                "Sync Status",
                |ui| {
                    let checked_text = match mgr.last_checked {
                        Some(when) => format::format_relative_time(when),
                        None => "Never checked".to_string(),
                    };
                    ui.add(
                        egui::Label::new(
                            RichText::new(checked_text)
                                .font(fonts::medium(13.5))
                                .color(theme::text_secondary()),
                        )
                        .truncate(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(!checking, |ui| {
                            let label = if checking { "Checking…" } else { "Check" };
                            if theme::pill_action(
                                ui,
                                Some(Icon::Refresh),
                                label,
                                ButtonTone::Secondary,
                                28.0,
                            )
                            .clicked()
                            {
                                *trigger_check = true;
                            }
                        });
                    });
                },
            );
        }
    };

    if cols == 3 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            draw_tile_1(ui);
            draw_tile_2(ui, &mut trigger_update_all);
            draw_tile_3(ui, &mut trigger_hub_update, &mut trigger_check_all);
        });
    } else if cols == 2 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            draw_tile_1(ui);
            draw_tile_2(ui, &mut trigger_update_all);
        });
        if show_third {
            ui.add_space(gap);
            draw_tile_3(ui, &mut trigger_hub_update, &mut trigger_check_all);
        }
    } else {
        draw_tile_1(ui);
        ui.add_space(gap);
        draw_tile_2(ui, &mut trigger_update_all);
        if show_third {
            ui.add_space(gap);
            draw_tile_3(ui, &mut trigger_hub_update, &mut trigger_check_all);
        }
    }

    if trigger_update_all {
        mgr.update_all();
    }
    if trigger_check_all {
        mgr.check_all_updates();
    }
    if trigger_hub_update {
        mgr.start_hub_update();
    }
}

fn bento_stat_tile(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    soft_accent: bool,
    icon: Icon,
    icon_color: egui::Color32,
    heading: &str,
    bottom_row: impl FnOnce(&mut egui::Ui),
) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
    theme::paint_info_card(ui.painter(), rect, hover * 0.5, soft_accent);

    // Paint header row directly at fixed top coordinates so it never inflates layout height
    let inner = rect.shrink2(vec2(18.0, 14.0));
    icons::paint(
        ui.painter(),
        icon,
        egui::pos2(inner.left() + 7.0, inner.top() + 8.0),
        14.0,
        icon_color,
    );
    ui.painter().text(
        egui::pos2(inner.left() + 20.0, inner.top() + 8.0),
        Align2::LEFT_CENTER,
        heading.to_uppercase(),
        fonts::bold(10.5),
        theme::text_muted(),
    );

    // Bottom value/action row strictly pinned to the lower half of the tile
    let bottom_rect = Rect::from_min_max(
        egui::pos2(inner.left(), inner.bottom() - 32.0),
        inner.max,
    );
    let mut row_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bottom_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row_ui.set_clip_rect(rect);
    bottom_row(&mut row_ui);
}

// ─── Bento Grid View ───────────────────────────────────────────────────────

fn render_bento_grid(
    ui: &mut egui::Ui,
    apps: &[&AppEntry],
    meta: &HashMap<String, CachedAppMeta>,
    mgr: &mut ManagerApp,
) {
    let gap = 14.0;
    let avail = (ui.available_width() - 2.0).max(260.0);
    let cols = ((avail + gap) / (296.0 + gap)).floor().clamp(1.0, 3.0) as usize;
    let card_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32)
        .floor()
        .max(240.0);

    for chunk in apps.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for app in chunk {
                ui.push_id(&app.id, |ui| {
                    let icon_uri = meta.get(&app.id).and_then(|m| m.icon_uri.as_deref());
                    render_grid_card(ui, mgr, app, icon_uri, card_w);
                });
            }
        });
        ui.add_space(gap);
    }
}

fn render_grid_card(
    ui: &mut egui::Ui,
    mgr: &mut ManagerApp,
    app: &AppEntry,
    icon_uri: Option<&str>,
    card_w: f32,
) {
    let update_info = mgr.update_available.get(&app.id).cloned();
    let latest_tag = update_info.as_ref().map(|u| u.tag.clone());
    let has_update = update_info.is_some();
    let file_issue = mgr.file_issues.get(&app.id).copied();

    let (downloading, failed_msg) = match mgr.app_download_state(&app.id) {
        AppDownloadState::Downloading => (true, None),
        AppDownloadState::Failed(failed) => {
            (false, Some((failed.message.clone(), failed.retry.is_some())))
        }
        AppDownloadState::Idle => (false, None),
    };

    let card_state = if downloading {
        CardState::Downloading
    } else if failed_msg.is_some() {
        CardState::Failed
    } else {
        CardState::Idle
    };

    let mut actions = CardActions::default();
    let menu_id = ui.make_persistent_id(("card_more_menu", app.id.as_str()));
    let mut menu_open = ui.data(|d| d.get_temp::<bool>(menu_id).unwrap_or(false));
    let mut more_btn_rect = Rect::NOTHING;

    let card_h = 178.0;
    let (card_rect, card_resp) = ui.allocate_exact_size(vec2(card_w, card_h), Sense::hover());
    let hovered = ui.rect_contains_pointer(card_rect);
    let hover_t = ui.ctx().animate_bool_responsive(card_resp.id, hovered);
    theme::paint_bento_card(ui.painter(), card_rect, hover_t, has_update);

    let inner = card_rect.shrink2(vec2(18.0, 16.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );

    // ── Top row: Squircle Icon + Title/Source + Overflow Button ──
    let mut toggle_menu = false;
    child.allocate_ui_with_layout(
        vec2(inner.width(), 48.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            render_icon(ui, app, icon_uri, 46.0);
            ui.add_space(10.0);

            let info_w = (ui.available_width() - 34.0).max(96.0);
            ui.vertical(|ui| {
                ui.set_width(info_w);
                ui.spacing_mut().item_spacing.y = 3.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&app.display_name)
                            .font(fonts::bold(15.0))
                            .color(theme::text_primary()),
                    )
                    .truncate(),
                );
                match source_text(app) {
                    Some(repo) => {
                        render_github_repo_label(ui, &app.id, repo, true);
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            icons::widget(ui, Icon::HardDrive, 12.0, theme::text_muted());
                            ui.label(
                                RichText::new("Local AppImage")
                                    .font(fonts::regular(11.5))
                                    .color(theme::text_muted()),
                            );
                        });
                    }
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let more =
                    theme::icon_button(ui, Icon::MoreHorizontal, 28.0, menu_open, "More actions");
                if more.clicked() {
                    toggle_menu = true;
                }
                more_btn_rect = more.rect;
            });
        },
    );

    if toggle_menu {
        menu_open = !menu_open;
    }

    child.add_space(16.0);

    // ── Middle row: Version Pill + Single Status Pill ──
    child.allocate_ui_with_layout(
        vec2(inner.width(), 24.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            render_version_pill(ui, app, latest_tag.as_deref(), has_update);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                render_status_pill(
                    ui,
                    app,
                    card_state,
                    has_update,
                    file_issue,
                    mgr,
                    latest_tag.as_deref(),
                );
            });
        },
    );

    // ── Bottom Action Bar (Soft, Calm Button Hierarchy) ──
    child.with_layout(Layout::bottom_up(Align::Min), |ui| {
        ui.allocate_ui_with_layout(
            vec2(inner.width(), 34.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                match card_state {
                    CardState::Downloading => {
                        let angle = (ui.input(|i| i.time) as f32) * 3.5;
                        let (r, _) = ui.allocate_exact_size(vec2(16.0, 32.0), Sense::hover());
                        icons::paint_rotated(
                            ui.painter(),
                            Icon::Refresh,
                            r.center(),
                            15.0,
                            theme::accent(),
                            angle,
                        );
                        ui.label(
                            RichText::new("Updating…")
                                .font(fonts::medium(12.5))
                                .color(theme::accent()),
                        );
                        ui.ctx().request_repaint();
                    }
                    CardState::Failed => {
                        if let Some((msg, can_retry)) = failed_msg.as_ref() {
                            ui.add_enabled_ui(*can_retry, |ui| {
                                if theme::pill_action(
                                    ui,
                                    Some(Icon::Refresh),
                                    "Retry",
                                    ButtonTone::SubtleAccent,
                                    32.0,
                                )
                                .clicked()
                                {
                                    actions.retry_app = true;
                                }
                            });
                            ui.add(
                                egui::Label::new(
                                    RichText::new(msg)
                                        .font(fonts::regular(11.5))
                                        .color(theme::danger()),
                                )
                                .truncate(),
                            )
                            .on_hover_text(msg);
                        }
                    }
                    CardState::Idle => {
                        if has_update {
                            if theme::pill_action(
                                ui,
                                Some(Icon::Download),
                                "Update",
                                ButtonTone::SubtleAccent,
                                32.0,
                            )
                            .clicked()
                            {
                                actions.update = true;
                            }
                            if theme::pill_action(
                                ui,
                                Some(Icon::Play),
                                "Launch",
                                ButtonTone::Secondary,
                                32.0,
                            )
                            .clicked()
                            {
                                actions.launch = true;
                            }
                        } else if theme::pill_action(
                            ui,
                            Some(Icon::Play),
                            "Launch",
                            ButtonTone::Secondary,
                            32.0,
                        )
                        .clicked()
                        {
                            actions.launch = true;
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if theme::icon_button(
                                ui,
                                Icon::Folder,
                                32.0,
                                false,
                                "Open containing folder",
                            )
                            .clicked()
                            {
                                actions.show_folder = true;
                            }
                            if theme::icon_button(
                                ui,
                                Icon::Edit,
                                32.0,
                                false,
                                "Edit AppImage settings",
                            )
                            .clicked()
                            {
                                actions.edit = true;
                            }
                        });
                    }
                }
            },
        );
    });

    render_overflow_popover(
        ui,
        menu_id,
        &mut menu_open,
        more_btn_rect,
        file_issue,
        &mut actions,
    );
    ui.data_mut(|d| d.insert_temp(menu_id, menu_open));
    apply_card_actions(mgr, app, update_info, actions);
}

// ─── Linear-Style Dense List View ──────────────────────────────────────────

fn render_dense_list(
    ui: &mut egui::Ui,
    apps: &[&AppEntry],
    meta: &HashMap<String, CachedAppMeta>,
    mgr: &mut ManagerApp,
    size_class: SizeClass,
) {
    let narrow = matches!(size_class, SizeClass::Narrow | SizeClass::Tight);
    for app in apps {
        ui.push_id(&app.id, |ui| {
            let icon_uri = meta.get(&app.id).and_then(|m| m.icon_uri.as_deref());
            render_list_row(ui, mgr, app, icon_uri, narrow);
        });
        ui.add_space(8.0);
    }
}

fn render_list_row(
    ui: &mut egui::Ui,
    mgr: &mut ManagerApp,
    app: &AppEntry,
    icon_uri: Option<&str>,
    narrow: bool,
) {
    let update_info = mgr.update_available.get(&app.id).cloned();
    let latest_tag = update_info.as_ref().map(|u| u.tag.clone());
    let has_update = update_info.is_some();
    let file_issue = mgr.file_issues.get(&app.id).copied();

    let (downloading, failed_msg) = match mgr.app_download_state(&app.id) {
        AppDownloadState::Downloading => (true, None),
        AppDownloadState::Failed(failed) => {
            (false, Some((failed.message.clone(), failed.retry.is_some())))
        }
        AppDownloadState::Idle => (false, None),
    };

    let card_state = if downloading {
        CardState::Downloading
    } else if failed_msg.is_some() {
        CardState::Failed
    } else {
        CardState::Idle
    };

    let mut actions = CardActions::default();
    let menu_id = ui.make_persistent_id(("list_more_menu", app.id.as_str()));
    let mut menu_open = ui.data(|d| d.get_temp::<bool>(menu_id).unwrap_or(false));
    let mut more_btn_rect = Rect::NOTHING;

    let row_w = ui.available_width().max(260.0);
    let row_h = if narrow { 104.0 } else { 66.0 };
    let (row_rect, row_resp) = ui.allocate_exact_size(vec2(row_w, row_h), Sense::hover());
    let hover_t = ui
        .ctx()
        .animate_bool_responsive(row_resp.id, ui.rect_contains_pointer(row_rect));
    theme::paint_bento_card(ui.painter(), row_rect, hover_t, has_update);

    let inner = row_rect.shrink2(vec2(14.0, 10.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );

    let mut toggle_menu = false;

    child.horizontal(|ui| {
        render_icon(ui, app, icon_uri, 40.0);
        ui.add_space(10.0);

        let info_w = if narrow {
            (ui.available_width() - 36.0).max(120.0)
        } else {
            (ui.available_width() * 0.42).clamp(140.0, 360.0)
        };
        ui.vertical(|ui| {
            ui.set_width(info_w);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&app.display_name)
                            .font(fonts::bold(14.5))
                            .color(theme::text_primary()),
                    )
                    .truncate(),
                );
                if !narrow {
                    render_version_pill(ui, app, latest_tag.as_deref(), has_update);
                }
            });
            match source_text(app) {
                Some(repo) => render_github_repo_label(ui, &app.id, repo, false),
                None => {
                    ui.add(
                        egui::Label::new(
                            RichText::new("Local AppImage")
                                .font(fonts::regular(11.5))
                                .color(theme::text_muted()),
                        )
                        .truncate(),
                    );
                }
            }
        });

        if !narrow {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let more =
                    theme::icon_button(ui, Icon::MoreHorizontal, 30.0, menu_open, "More actions");
                if more.clicked() {
                    toggle_menu = true;
                }
                more_btn_rect = more.rect;

                if theme::icon_button(ui, Icon::Folder, 30.0, false, "Open containing folder")
                    .clicked()
                {
                    actions.show_folder = true;
                }
                if theme::icon_button(ui, Icon::Edit, 30.0, false, "Edit AppImage").clicked() {
                    actions.edit = true;
                }

                match card_state {
                    CardState::Downloading => {
                        ui.label(
                            RichText::new("Updating…")
                                .font(fonts::medium(12.0))
                                .color(theme::accent()),
                        );
                    }
                    CardState::Failed => {
                        if let Some((_, can_retry)) = failed_msg.as_ref() {
                            ui.add_enabled_ui(*can_retry, |ui| {
                                if theme::pill_action(
                                    ui,
                                    Some(Icon::Refresh),
                                    "Retry",
                                    ButtonTone::Primary,
                                    30.0,
                                )
                                .clicked()
                                {
                                    actions.retry_app = true;
                                }
                            });
                        }
                    }
                    CardState::Idle => {
                        if has_update {
                            if theme::pill_action(
                                ui,
                                Some(Icon::Download),
                                "Update",
                                ButtonTone::SubtleAccent,
                                30.0,
                            )
                            .clicked()
                            {
                                actions.update = true;
                            }
                            if theme::pill_action(
                                ui,
                                Some(Icon::Play),
                                "Launch",
                                ButtonTone::Secondary,
                                30.0,
                            )
                            .clicked()
                            {
                                actions.launch = true;
                            }
                        } else if theme::pill_action(
                            ui,
                            Some(Icon::Play),
                            "Launch",
                            ButtonTone::Secondary,
                            30.0,
                        )
                        .clicked()
                        {
                            actions.launch = true;
                        }
                    }
                }

                ui.add_space(4.0);
                render_status_pill(
                    ui,
                    app,
                    card_state,
                    has_update,
                    file_issue,
                    mgr,
                    latest_tag.as_deref(),
                );
            });
        } else {
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let more =
                    theme::icon_button(ui, Icon::MoreHorizontal, 30.0, menu_open, "More actions");
                if more.clicked() {
                    toggle_menu = true;
                }
                more_btn_rect = more.rect;
            });
        }
    });

    if narrow {
        child.add_space(8.0);
        child.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if has_update {
                if theme::pill_action(
                    ui,
                    Some(Icon::Download),
                    "Update",
                    ButtonTone::SubtleAccent,
                    30.0,
                )
                .clicked()
                {
                    actions.update = true;
                }
                if theme::pill_action(ui, Some(Icon::Play), "Launch", ButtonTone::Secondary, 30.0)
                    .clicked()
                {
                    actions.launch = true;
                }
            } else if theme::pill_action(ui, Some(Icon::Play), "Launch", ButtonTone::Secondary, 30.0)
                .clicked()
            {
                actions.launch = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Edit, 30.0, false, "Edit").clicked() {
                    actions.edit = true;
                }
                render_status_pill(
                    ui,
                    app,
                    card_state,
                    has_update,
                    file_issue,
                    mgr,
                    latest_tag.as_deref(),
                );
            });
        });
    }

    if toggle_menu {
        menu_open = !menu_open;
    }

    render_overflow_popover(
        ui,
        menu_id,
        &mut menu_open,
        more_btn_rect,
        file_issue,
        &mut actions,
    );
    ui.data_mut(|d| d.insert_temp(menu_id, menu_open));
    apply_card_actions(mgr, app, update_info, actions);
}

// ─── Shared Card Parts ─────────────────────────────────────────────────────

/// Strips verbose `https://github.com/` prefixes so cards show crisp `owner/repo` labels.
fn short_repo_label(repo: &str) -> &str {
    let trimmed = repo.trim().trim_end_matches('/');
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "github.com/",
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest;
        }
    }
    trimmed
}

/// Normalizes a stored GitHub repository string into a full `https://github.com/owner/repo` URL.
fn github_repo_web_url(repo: &str) -> String {
    let clean = short_repo_label(repo)
        .trim_end_matches(".git")
        .trim_end_matches('/');
    if clean.starts_with("https://") || clean.starts_with("http://") {
        clean.to_string()
    } else {
        format!("https://github.com/{clean}")
    }
}

const GITHUB_HOVER_DELAY_SECS: f64 = 3.0;

#[derive(Clone, Copy, Default)]
struct RepoHoverState {
    hover_started: Option<f64>,
    popup_open: bool,
}

/// Renders an app's GitHub repository label and, after hovering for more than 3 seconds,
/// displays a floating "Go to GitHub" button to open the repository in the web browser.
fn render_github_repo_label(
    ui: &mut egui::Ui,
    app_id: &str,
    repo: &str,
    show_branch_icon: bool,
) {
    let state_id = ui.make_persistent_id(("repo_hover_state", app_id));
    let mut state = ui.data(|d| d.get_temp::<RepoHoverState>(state_id).unwrap_or_default());

    let short = short_repo_label(repo);
    let text_color = if state.popup_open || state.hover_started.is_some() {
        theme::text_secondary()
    } else {
        theme::text_muted()
    };

    let row_resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            if show_branch_icon {
                icons::widget(ui, Icon::GitBranch, 12.0, text_color);
            }
            let lbl = ui.add(
                egui::Label::new(
                    RichText::new(short)
                        .font(fonts::regular(11.5))
                        .color(text_color),
                )
                .truncate()
                .sense(Sense::hover()),
            );
            if !state.popup_open {
                lbl.on_hover_text(repo);
            }
        })
        .response;

    let url_rect = row_resp.rect;
    let hovering_url = row_resp.contains_pointer();
    let now = ui.input(|i| i.time);

    if hovering_url {
        let start = *state.hover_started.get_or_insert(now);
        let elapsed = now - start;
        if elapsed >= GITHUB_HOVER_DELAY_SECS {
            state.popup_open = true;
        } else {
            let remaining = (GITHUB_HOVER_DELAY_SECS - elapsed).max(0.05);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(remaining));
        }
    } else if !state.popup_open {
        state.hover_started = None;
    }

    if state.popup_open {
        let bounds = ui.ctx().screen_rect();
        let popup_h = 44.0;
        let popup_w = 152.0;
        let x = url_rect.left().clamp(
            bounds.left() + 8.0,
            (bounds.right() - popup_w - 8.0).max(bounds.left() + 8.0),
        );
        let y = if url_rect.bottom() + 4.0 + popup_h <= bounds.bottom() - 8.0 {
            url_rect.bottom() + 4.0
        } else {
            (url_rect.top() - popup_h - 4.0).max(bounds.top() + 8.0)
        };

        let area = egui::Area::new(state_id.with("go_to_github_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .show(ui.ctx(), |ui| {
                theme::menu_frame().show(ui, |ui| {
                    if theme::pill_action(
                        ui,
                        Some(Icon::ArrowUpRight),
                        "Go to GitHub",
                        ButtonTone::SubtleAccent,
                        28.0,
                    )
                    .clicked()
                    {
                        let url = github_repo_web_url(repo);
                        if std::process::Command::new("xdg-open").arg(&url).spawn().is_err() {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                        }
                        state.popup_open = false;
                        state.hover_started = None;
                    }
                });
            });

        if state.popup_open {
            let popup_rect = area.response.rect;
            let safe_zone = url_rect.union(popup_rect).expand(8.0);
            let pointer_pos = ui.input(|i| i.pointer.latest_pos());
            let still_in_zone = pointer_pos.is_some_and(|p| safe_zone.contains(p));
            let clicked_outside = ui.input(|i| i.pointer.any_click()) && !still_in_zone;
            if !still_in_zone || clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                state.popup_open = false;
                state.hover_started = None;
            }
        }
    }

    ui.data_mut(|d| d.insert_temp(state_id, state));
}

fn render_version_pill(
    ui: &mut egui::Ui,
    app: &AppEntry,
    latest_tag: Option<&str>,
    has_update: bool,
) {
    let p = theme::current();
    if has_update {
        let cur = if app.current_version.trim().is_empty() {
            "current"
        } else {
            app.current_version.trim()
        };
        let next = latest_tag.unwrap_or("new");
        theme::version_pill(ui, &format!("{cur} → {next}"), p.detail, p.detail_soft);
    } else {
        let ver = if app.current_version.trim().is_empty() {
            "local"
        } else {
            app.current_version.trim()
        };
        theme::version_pill(ui, ver, p.text_secondary, p.neutral_soft);
    }
}

fn render_status_pill(
    ui: &mut egui::Ui,
    app: &AppEntry,
    state: CardState,
    has_update: bool,
    file_issue: Option<LocalFileIssue>,
    mgr: &ManagerApp,
    latest_tag: Option<&str>,
) {
    let (text, kind) = match state {
        CardState::Downloading => ("Updating", BadgeKind::Accent),
        CardState::Failed => ("Failed", BadgeKind::Danger),
        CardState::Idle => {
            if file_issue == Some(LocalFileIssue::Missing) {
                ("Missing", BadgeKind::Danger)
            } else if file_issue == Some(LocalFileIssue::Changed) {
                ("Modified", BadgeKind::Warning)
            } else if has_update {
                ("Update ready", BadgeKind::Accent)
            } else if source_text(app).is_none() {
                ("Local", BadgeKind::Neutral)
            } else if latest_tag.is_some() || mgr.latest_versions.contains_key(&app.id) {
                ("Up to date", BadgeKind::Success)
            } else {
                ("Tracked", BadgeKind::Neutral)
            }
        }
    };
    let resp = theme::status_pill(ui, text, kind);
    match file_issue {
        Some(LocalFileIssue::Missing) => {
            resp.on_hover_text(
                "The managed AppImage is missing on disk. Check for updates to restore it.",
            );
        }
        Some(LocalFileIssue::Changed) => {
            resp.on_hover_text(
                "File changed outside AppImage Hub. Update it or choose ⋯ → Mark as Up to Date.",
            );
        }
        None => {}
    }
}

fn render_overflow_popover(
    ui: &egui::Ui,
    menu_id: egui::Id,
    menu_open: &mut bool,
    more_btn_rect: Rect,
    file_issue: Option<LocalFileIssue>,
    actions: &mut CardActions,
) {
    if !*menu_open || !more_btn_rect.is_finite() {
        return;
    }

    let items_count = if file_issue == Some(LocalFileIssue::Changed) {
        4
    } else {
        3
    };
    let menu_h = (items_count as f32) * (theme::MENU_ITEM_HEIGHT + 4.0) + 16.0;
    let bounds = ui.ctx().screen_rect();
    let pos = theme::menu_popup_pos(more_btn_rect, bounds, menu_h);

    let area = egui::Area::new(menu_id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            theme::menu_frame().show(ui, |ui| {
                ui.set_width(theme::MENU_WIDTH);
                ui.spacing_mut().item_spacing.y = 3.0;

                if file_issue == Some(LocalFileIssue::Changed)
                    && theme::popover_item(ui, Icon::Check, "Mark as Up to Date", false)
                        .on_hover_text("Accept the current file and clear the modified warning.")
                        .clicked()
                {
                    actions.accept_current = true;
                    *menu_open = false;
                }
                if theme::popover_item(ui, Icon::Link, "Manage Shortcuts…", false).clicked() {
                    actions.add_to = true;
                    *menu_open = false;
                }
                if theme::popover_item(ui, Icon::Folder, "Open Folder", false).clicked() {
                    actions.show_folder = true;
                    *menu_open = false;
                }
                theme::divider(ui);
                if theme::popover_item(ui, Icon::Trash, "Delete AppImage", true).clicked() {
                    actions.delete = true;
                    *menu_open = false;
                }
            });
        });

    let menu_rect = area.response.rect;
    let pointer = ui.input(|i| i.pointer.interact_pos());
    let clicked_elsewhere = ui.input(|i| i.pointer.any_click())
        && !pointer.is_some_and(|p| menu_rect.contains(p) || more_btn_rect.contains(p));
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) || clicked_elsewhere {
        *menu_open = false;
    }
}

#[derive(Default)]
struct CardActions {
    retry_app: bool,
    launch: bool,
    edit: bool,
    update: bool,
    add_to: bool,
    delete: bool,
    show_folder: bool,
    accept_current: bool,
}

fn apply_card_actions(
    mgr: &mut ManagerApp,
    app: &AppEntry,
    update_info: Option<crate::app::AvailableUpdate>,
    actions: CardActions,
) {
    if actions.retry_app {
        mgr.retry_failed_for_app(&app.id);
    }
    if actions.accept_current {
        mgr.accept_current_file(&app.id);
    }
    if actions.launch {
        mgr.launch_app(&app.id);
    }
    if actions.edit {
        mgr.open_edit(app);
    }
    if actions.update {
        if let Some(update) = update_info {
            mgr.start_update_download(
                app.id.clone(),
                update.url,
                update.tag,
                update.commit_hash,
                update.checksum_url,
            );
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

fn render_icon(ui: &mut egui::Ui, app: &AppEntry, icon_uri: Option<&str>, size: f32) {
    let size = if size.is_finite() && size > 0.0 {
        size.clamp(1.0, 256.0)
    } else {
        48.0
    };
    let avail = ui.available_size();
    if !avail.x.is_finite() || !avail.y.is_finite() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(size), Sense::hover());
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let p = theme::current();
    let rounding = CornerRadius::same((size * 0.25).round() as u8);
    ui.painter()
        .rect_filled(rect, rounding, p.surface_raised);
    ui.painter()
        .rect_stroke(rect, rounding, Stroke::new(1.0_f32, p.border), StrokeKind::Inside);

    // Zero filesystem syscalls per frame: `icon_uri` was pre-resolved once in AppSnapshotCache.
    if let Some(uri) = icon_uri {
        let image = egui::Image::new(uri).corner_radius(rounding);
        let inner = rect.shrink(5.0);
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
        Align2::CENTER_CENTER,
        initial,
        fonts::bold((size * 0.42).max(1.0)),
        p.detail,
    );
}

// ─── Empty States & Sorting ────────────────────────────────────────────────

fn render_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(72.0);
        let (rect, _) = ui.allocate_exact_size(vec2(72.0, 72.0), Sense::hover());
        let p = theme::current();
        ui.painter()
            .circle_filled(rect.center(), 36.0, p.accent_soft);
        ui.painter()
            .circle_stroke(rect.center(), 36.0, Stroke::new(1.0_f32, theme::with_alpha(p.accent, 110)));
        icons::paint(ui.painter(), Icon::AppsGrid, rect.center(), 28.0, p.accent);

        ui.add_space(theme::SPACE_LG);
        ui.label(
            RichText::new("Your AppImage library is empty")
                .font(fonts::bold(21.0))
                .color(theme::text_primary()),
        );
        ui.add_space(theme::SPACE_XS);
        ui.label(
            theme::secondary_label(
                "Add a local AppImage or connect a GitHub repository for one-click updates.",
            )
            .font(fonts::regular(13.5)),
        );
        ui.add_space(theme::SPACE_XL);
        if theme::pill_action(ui, Some(Icon::Plus), "Add AppImage", ButtonTone::Primary, 38.0)
            .clicked()
        {
            mgr.open_add_form();
        }
    });
}

fn render_filtered_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(64.0);
        let (rect, _) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::hover());
        ui.painter()
            .circle_filled(rect.center(), 28.0, theme::surface_raised());
        icons::paint(ui.painter(), Icon::Search, rect.center(), 22.0, theme::text_muted());

        ui.add_space(theme::SPACE_MD);
        ui.label(
            RichText::new("No matching AppImages")
                .font(fonts::bold(18.0))
                .color(theme::text_primary()),
        );
        ui.add_space(theme::SPACE_XS);
        ui.label(theme::muted_label(
            "Adjust your search query or switch back to All Apps.",
        ));
        ui.add_space(theme::SPACE_MD);
        if theme::pill_action(ui, None, "Show All Apps", ButtonTone::Secondary, 34.0).clicked() {
            mgr.search_query.clear();
            mgr.nav_filter = NavFilter::All;
        }
    });
}

fn app_name_lower<'a>(app: &'a AppEntry, meta: &'a HashMap<String, CachedAppMeta>) -> std::borrow::Cow<'a, str> {
    if let Some(cached) = meta.get(&app.id) {
        std::borrow::Cow::Borrowed(cached.name_lower.as_str())
    } else {
        std::borrow::Cow::Owned(app.display_name.to_lowercase())
    }
}

fn sort_apps(apps: &mut [&AppEntry], meta: &HashMap<String, CachedAppMeta>, sort_by: AppSortBy) {
    match sort_by {
        AppSortBy::Name => apps.sort_by(|a, b| app_name_lower(a, meta).cmp(&app_name_lower(b, meta))),
        AppSortBy::Size => apps.sort_by(|a, b| {
            std::cmp::Reverse(app_size(a, meta))
                .cmp(&std::cmp::Reverse(app_size(b, meta)))
                .then_with(|| app_name_lower(a, meta).cmp(&app_name_lower(b, meta)))
        }),
        AppSortBy::DateAdded => apps.sort_by(|a, b| {
            std::cmp::Reverse(app_added_at(a, meta))
                .cmp(&std::cmp::Reverse(app_added_at(b, meta)))
                .then_with(|| app_name_lower(a, meta).cmp(&app_name_lower(b, meta)))
        }),
        AppSortBy::Github => apps.sort_by(|a, b| {
            std::cmp::Reverse(source_rank(a))
                .cmp(&std::cmp::Reverse(source_rank(b)))
                .then_with(|| app_name_lower(a, meta).cmp(&app_name_lower(b, meta)))
        }),
        AppSortBy::Local => apps.sort_by(|a, b| {
            source_rank(a)
                .cmp(&source_rank(b))
                .then_with(|| app_name_lower(a, meta).cmp(&app_name_lower(b, meta)))
        }),
    }
}

fn app_size(app: &AppEntry, meta: &HashMap<String, CachedAppMeta>) -> u64 {
    meta.get(&app.id)
        .map(|cached| cached.size)
        .unwrap_or_else(|| {
            app.local_path
                .metadata()
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        })
}

fn app_added_at(app: &AppEntry, meta: &HashMap<String, CachedAppMeta>) -> i64 {
    if let Some(cached) = meta.get(&app.id) {
        return cached.added_at;
    }
    app.added_at.unwrap_or_else(|| {
        app.local_path
            .metadata()
            .ok()
            .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or(0)
    })
}

fn source_rank(app: &AppEntry) -> u8 {
    match app.source {
        AppSource::LocalOnly => 0,
        AppSource::LocalWithRepo { .. } | AppSource::GithubOnly { .. } => 1,
    }
}

fn source_text(app: &AppEntry) -> Option<&str> {
    match &app.source {
        AppSource::LocalWithRepo { github_repo } | AppSource::GithubOnly { github_repo } => {
            Some(github_repo.as_str())
        }
        AppSource::LocalOnly => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_card_layout_responsiveness() {
        let large = card_layout_params(1400.0);
        assert_eq!(large.icon_size, 64.0);
        assert!(!large.compact);

        let medium = card_layout_params(1000.0);
        assert_eq!(medium.icon_size, 64.0);

        let narrow = card_layout_params(700.0);
        assert_eq!(narrow.icon_size, 56.0);
        assert!(narrow.compact);

        let tight = card_layout_params(500.0);
        assert_eq!(tight.icon_size, 48.0);
        assert_eq!(tight.name_size, 16.0);
        assert!(tight.compact);
    }

    fn sample_app(id: &str, name: &str, source: AppSource, added_at: Option<i64>) -> AppEntry {
        AppEntry {
            id: id.into(),
            display_name: name.into(),
            source,
            local_path: std::path::PathBuf::from(format!("/tmp/{id}.AppImage")),
            current_version: String::new(),
            args: Vec::new(),
            icon_path: None,
            generic_name: None,
            comment: None,
            categories: None,
            startup_notify: true,
            startup_wm_class: None,
            startup_wm_class_auto: None,
            asset_preference: None,
            added_at,
            commit_hash: None,
            file_fingerprint: None,
            desktop_shortcut: true,
            menu_entry: true,
            mime_types: None,
            exec_arg: None,
        }
    }

    #[test]
    fn test_sources() {
        let local = sample_app("1", "App", AppSource::LocalOnly, None);
        assert!(source_text(&local).is_none());

        let repo = AppEntry {
            source: AppSource::GithubOnly {
                github_repo: "owner/repo".into(),
            },
            ..local
        };
        assert_eq!(source_text(&repo), Some("owner/repo"));
    }

    #[test]
    fn sort_by_name_is_case_insensitive() {
        let a = sample_app("1", "zed", AppSource::LocalOnly, None);
        let b = sample_app("2", "Alpha", AppSource::LocalOnly, None);
        let mut list = vec![&a, &b];
        sort_apps(&mut list, &HashMap::new(), AppSortBy::Name);
        assert_eq!(list[0].id, "2");
    }

    #[test]
    fn sort_uses_cached_size() {
        let a = sample_app("1", "Small", AppSource::LocalOnly, None);
        let b = sample_app("2", "Big", AppSource::LocalOnly, None);
        let mut meta = HashMap::new();
        meta.insert(
            "1".into(),
            CachedAppMeta {
                size: 10,
                added_at: 0,
                icon_uri: None,
                name_lower: "small".into(),
                repo_lower: None,
            },
        );
        meta.insert(
            "2".into(),
            CachedAppMeta {
                size: 500,
                added_at: 0,
                icon_uri: None,
                name_lower: "big".into(),
                repo_lower: None,
            },
        );
        let mut list = vec![&a, &b];
        sort_apps(&mut list, &meta, AppSortBy::Size);
        assert_eq!(list[0].id, "2");
    }

    #[test]
    fn sort_by_date_added_uses_newest_first() {
        let a = sample_app("1", "Old", AppSource::LocalOnly, Some(100));
        let b = sample_app("2", "New", AppSource::LocalOnly, Some(200));
        let mut list = vec![&a, &b];
        sort_apps(&mut list, &HashMap::new(), AppSortBy::DateAdded);
        assert_eq!(list[0].id, "2");
    }

    #[test]
    fn source_sort_puts_requested_source_first() {
        let local = sample_app("1", "Local", AppSource::LocalOnly, None);
        let gh = sample_app(
            "2",
            "GH",
            AppSource::GithubOnly {
                github_repo: "o/r".into(),
            },
            None,
        );
        let mut list = vec![&local, &gh];
        sort_apps(&mut list, &HashMap::new(), AppSortBy::Github);
        assert_eq!(list[0].id, "2");
        sort_apps(&mut list, &HashMap::new(), AppSortBy::Local);
        assert_eq!(list[0].id, "1");
    }

    #[test]
    fn github_repo_web_url_normalizes_common_forms() {
        assert_eq!(
            github_repo_web_url("MichaelX17/AppImage-Hub"),
            "https://github.com/MichaelX17/AppImage-Hub"
        );
        assert_eq!(
            github_repo_web_url("https://github.com/MichaelX17/AppImage-Hub/"),
            "https://github.com/MichaelX17/AppImage-Hub"
        );
        assert_eq!(
            github_repo_web_url("https://github.com/MichaelX17/AppImage-Hub.git"),
            "https://github.com/MichaelX17/AppImage-Hub"
        );
        assert_eq!(
            github_repo_web_url("github.com/MichaelX17/AppImage-Hub"),
            "https://github.com/MichaelX17/AppImage-Hub"
        );
    }
}
