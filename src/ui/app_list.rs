//! AppImage cards: neon/dark launcher-style rows.
//!
//! Layout:  [icon] [name + badge / repo / versions] [⋯ more]
//!          ─────────────────────────────────────────────
//!          [primary action] [secondary actions …]
//!
//! All backend interactions are funneled through [`CardActions`] so the card is
//! rendered first and `ManagerApp` is mutated afterwards (no borrow conflicts).

use crate::app::{
    AppDownloadState, AppSortBy, CachedAppMeta, LocalFileIssue, ManagerApp, NavFilter,
};
use crate::config::{AppEntry, AppSource};
use crate::ui::breakpoints::SizeClass;
use crate::ui::theme::{self, BadgeKind};
use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum CardState {
    Idle,
    Downloading,
    Failed,
}

/// Per-card layout decisions derived from the available card width.
struct CardLayout {
    /// App icon edge length (large 64 px, down to 48 px when tight).
    icon_size: f32,
    /// Display-name font size.
    name_size: f32,
    /// Compact cards use tighter, full-width actions.
    compact: bool,
}

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
    let _ = size_class;
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
            app.display_name.to_lowercase().contains(&query)
                || source_text(app).is_some_and(|repo| repo.to_lowercase().contains(&query))
        })
        .collect();

    if sorted_apps.is_empty() {
        render_filtered_empty_state(ui, mgr);
        return;
    }

    sort_apps(&mut sorted_apps, meta, mgr.sort_by);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for app in sorted_apps {
                ui.push_id(&app.id, |ui| {
                    render_app_card(ui, mgr, app);
                });
                ui.add_space(theme::SPACE_MD);
            }
            ui.add_space(theme::SPACE_SM);
        });
}

fn sort_apps(apps: &mut [&AppEntry], meta: &HashMap<String, CachedAppMeta>, sort_by: AppSortBy) {
    match sort_by {
        AppSortBy::Name => apps.sort_by_cached_key(|app| app.display_name.to_lowercase()),
        AppSortBy::Size => apps.sort_by_cached_key(|app| {
            (
                std::cmp::Reverse(app_size(app, meta)),
                app.display_name.to_lowercase(),
            )
        }),
        AppSortBy::DateAdded => apps.sort_by_cached_key(|app| {
            (
                std::cmp::Reverse(app_added_at(app, meta)),
                app.display_name.to_lowercase(),
            )
        }),
        AppSortBy::Github => apps.sort_by_cached_key(|app| {
            (
                std::cmp::Reverse(source_rank(app)),
                app.display_name.to_lowercase(),
            )
        }),
        AppSortBy::Local => {
            apps.sort_by_cached_key(|app| (source_rank(app), app.display_name.to_lowercase()))
        }
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

fn render_filtered_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(64.0);
        ui.label(
            RichText::new("No matching AppImages")
                .size(20.0)
                .strong()
                .color(theme::text_primary()),
        );
        ui.add_space(theme::SPACE_XS);
        ui.label(theme::muted_label(
            "Try clearing your search or switching to All Apps.",
        ));
        ui.add_space(theme::SPACE_MD);
        if ui.add(theme::secondary_button("Show All Apps")).clicked() {
            mgr.search_query.clear();
            mgr.nav_filter = NavFilter::All;
        }
    });
}

fn render_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(76.0, 76.0), Sense::hover());
        let center = rect.center();
        ui.painter().circle_filled(center, 38.0, theme::accent_soft());
        ui.painter()
            .circle_stroke(center, 38.0, Stroke::new(1.5_f32, theme::accent()));
        let arm = 13.0;
        ui.painter().line_segment(
            [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
            Stroke::new(2.5_f32, theme::accent()),
        );
        ui.painter().line_segment(
            [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
            Stroke::new(2.5_f32, theme::accent()),
        );
        ui.add_space(theme::SPACE_MD);
        ui.label(
            RichText::new("No AppImages managed yet")
                .size(22.0)
                .strong()
                .color(theme::text_primary()),
        );
        ui.add_space(theme::SPACE_XS);
        ui.label(theme::muted_label(
            "Add an AppImage to manage launches, updates and desktop entries.",
        ));
        ui.add_space(theme::SPACE_LG);
        if ui.add(theme::primary_button("+ Add AppImage")).clicked() {
            mgr.open_add_form();
        }
        ui.add_space(56.0);
    });
}
/// Collected action intents from card buttons; avoids &mut bool proliferation.
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

fn render_app_card(ui: &mut egui::Ui, mgr: &mut ManagerApp, app: &AppEntry) {
    let update_info = mgr.update_available.get(&app.id).cloned();
    let latest_tag = update_info.as_ref().map(|update| update.tag.clone());
    let has_update = update_info.is_some();
    let file_issue = mgr.file_issues.get(&app.id).copied();

    // Download progress itself lives in the single bottom progress view; the
    // card only reflects *what* is happening so it can disable its actions.
    let (downloading, failed_msg) = match mgr.app_download_state(&app.id) {
        AppDownloadState::Downloading => (true, None),
        AppDownloadState::Failed(failed) => (
            false,
            Some((failed.message.clone(), failed.retry.is_some())),
        ),
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

    // Overflow-menu state, kept per card in egui's temp storage.
    let menu_id = ui.make_persistent_id(("card_more_menu", app.id.as_str()));
    let mut menu_open = ui.data(|data| data.get_temp::<bool>(menu_id).unwrap_or(false));
    let mut more_btn_rect = egui::Rect::NOTHING;

    let card = theme::card_frame().show(ui, |ui| {
        let content_w = ui.available_width();
        ui.set_min_width(content_w);
        let layout = card_layout_params(content_w);

        let mut toggle_menu = false;

        ui.horizontal(|ui| {
            render_icon(ui, app, layout.icon_size);
            ui.add_space(theme::SPACE_LG);

            // Reserve room on the right for the overflow ("more") toggle.
            let more_w = 56.0;
            let info_w = (ui.available_width() - more_w - theme::SPACE_MD).max(140.0);
            ui.vertical(|ui| {
                ui.set_width(info_w);
                render_info_block(ui, mgr, app, latest_tag.as_deref(), has_update, &layout);
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let more = ui
                    .add(theme::menu_toggle_button(menu_open))
                    .on_hover_text("More actions");
                if more.clicked() {
                    toggle_menu = true;
                }
                more_btn_rect = more.rect;
            });
        });

        if toggle_menu {
            menu_open = !menu_open;
        }

        ui.add_space(theme::SPACE_MD);

        let badge_w = 118.0;
        let actions_w = (ui.available_width() - badge_w - theme::SPACE_MD).max(180.0);
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(actions_w, 34.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    render_actions(
                        ui,
                        &layout,
                        card_state,
                        has_update,
                        failed_msg.as_ref(),
                        &mut actions,
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                render_status_badge(
                    ui,
                    app,
                    card_state,
                    has_update,
                    file_issue,
                    mgr,
                    latest_tag.as_deref(),
                    egui::vec2(badge_w, 26.0),
                );
            });
        });
    });

    // Overflow menu: foreground overlay anchored under the ••• button and kept
    // inside the card's bounds. It does not participate in the card's layout.
    if menu_open && more_btn_rect.is_finite() {
        let menu_height = if file_issue == Some(LocalFileIssue::Changed) {
            theme::MENU_HEIGHT
        } else {
            theme::MENU_HEIGHT - (theme::MENU_ITEM_HEIGHT + theme::SPACE_SM)
        };
        let pos = theme::menu_popup_pos(more_btn_rect, card.response.rect, menu_height);
        let area = egui::Area::new(menu_id.with("popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                theme::menu_frame().show(ui, |ui| {
                    ui.set_min_width(theme::MENU_WIDTH);
                    ui.spacing_mut().item_spacing.y = theme::SPACE_SM;

                    if file_issue == Some(LocalFileIssue::Changed)
                        && ui
                            .add_sized(
                                [theme::MENU_WIDTH, theme::MENU_ITEM_HEIGHT],
                                theme::menu_secondary_button("Mark as Up to Date"),
                            )
                            .on_hover_text(
                                "Accept the current AppImage file and clear the changed-on-disk warning.",
                            )
                            .clicked()
                    {
                        actions.accept_current = true;
                        menu_open = false;
                    }
                    if ui
                        .add_sized(
                            [theme::MENU_WIDTH, theme::MENU_ITEM_HEIGHT],
                            theme::menu_secondary_button("Add Shortcut…"),
                        )
                        .clicked()
                    {
                        actions.add_to = true;
                        menu_open = false;
                    }
                    if ui
                        .add_sized(
                            [theme::MENU_WIDTH, theme::MENU_ITEM_HEIGHT],
                            theme::menu_danger_button("Delete AppImage"),
                        )
                        .clicked()
                    {
                        actions.delete = true;
                        menu_open = false;
                    }
                })
            });
        let menu_rect = area.response.rect;

        // Close on Escape or on a click outside the menu / toggle.
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let clicked_elsewhere = ui.input(|i| i.pointer.any_click())
            && !pointer.is_some_and(|p| menu_rect.contains(p) || more_btn_rect.contains(p));
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || clicked_elsewhere {
            menu_open = false;
        }
    }

    // Keep the card's normal border only; no outer hover/selection glow.

    // Persist the overflow menu state for this card.
    ui.data_mut(|data| data.insert_temp(menu_id, menu_open));

    // Apply actions after card is rendered (avoids borrow conflicts)
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
                update.url.clone(),
                update.tag.clone(),
                update.commit_hash.clone(),
                update.checksum_url.clone(),
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

fn render_info_block(
    ui: &mut egui::Ui,
    mgr: &ManagerApp,
    app: &AppEntry,
    latest_tag: Option<&str>,
    has_update: bool,
    layout: &CardLayout,
) {
    // ── Display name ─────────────────────────────────────────────────────
    ui.add(
        egui::Label::new(
            RichText::new(&app.display_name)
                .size(layout.name_size)
                .strong()
                .color(theme::text_primary()),
        )
        .truncate(),
    );

    // ── Repository / source ─────────────────────────────────────────────
    match source_text(app) {
        Some(repo) => {
            ui.add(egui::Label::new(theme::muted_label(repo).size(12.0)).truncate())
                .on_hover_text(repo);
        }
        None => {
            ui.label(theme::muted_label("Local file (no repository)").size(12.0));
        }
    }

    ui.add_space(theme::SPACE_XS);

    // ── Current / latest versions ───────────────────────────────────────
    let file_issue = mgr.file_issues.get(&app.id).copied();
    let (current, current_text_color) = match file_issue {
        // The metadata no longer describes the file on disk.
        Some(LocalFileIssue::Missing) => ("missing".to_string(), theme::danger()),
        Some(LocalFileIssue::Changed) => ("modified".to_string(), theme::warning()),
        None if app.current_version.trim().is_empty() => {
            ("unknown".to_string(), theme::text_secondary())
        }
        None => (app.current_version.clone(), theme::text_secondary()),
    };
    let current_dot_color = match file_issue {
        Some(LocalFileIssue::Missing) => theme::danger(),
        Some(LocalFileIssue::Changed) => theme::warning(),
        None => theme::text_muted(),
    };
    let latest_known = latest_tag
        .map(|tag| tag.to_string())
        .or_else(|| mgr.latest_versions.get(&app.id).cloned());

    let (latest_text, latest_color) = match &latest_known {
        Some(tag) if has_update => (tag.clone(), theme::detail()),
        Some(tag) => (tag.clone(), theme::success()),
        None => ("unknown".to_string(), theme::text_muted()),
    };

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        theme::status_dot(ui, current_dot_color);
        ui.label(theme::muted_label("Current").size(11.0));
        let current_label = RichText::new(current)
            .size(12.0)
            .strong()
            .color(current_text_color);
        let current_response = ui.label(current_label);
        match file_issue {
            Some(LocalFileIssue::Missing) => {
                current_response.on_hover_text(
                    "The managed AppImage file is missing. Check for updates to restore it.",
                );
            }
            Some(LocalFileIssue::Changed) => {
                current_response.on_hover_text(
                    "Replaced outside AppImage Hub; its real version is unknown. Update it or use ⋯ → Mark as Up to Date.",
                );
            }
            None => {}
        }
        ui.add_space(theme::SPACE_SM);
        theme::status_dot(ui, latest_color);
        ui.label(theme::muted_label("Latest").size(11.0));
        ui.label(
            RichText::new(latest_text)
                .size(12.0)
                .strong()
                .color(latest_color),
        );
    });
}

/// Status pill: green = up to date, purple = update available, grey = none.
#[allow(clippy::too_many_arguments)]
fn render_status_badge(
    ui: &mut egui::Ui,
    app: &AppEntry,
    state: CardState,
    has_update: bool,
    file_issue: Option<LocalFileIssue>,
    mgr: &ManagerApp,
    latest_tag: Option<&str>,
    size: egui::Vec2,
) {
    let (text, kind) = match state {
        CardState::Downloading => ("Updating", BadgeKind::Accent),
        CardState::Failed => ("Failed", BadgeKind::Danger),
        CardState::Idle => {
            if file_issue == Some(LocalFileIssue::Missing) {
                ("Missing file", BadgeKind::Danger)
            } else if file_issue == Some(LocalFileIssue::Changed) {
                ("Changed on disk", BadgeKind::Warning)
            } else if has_update {
                ("Update available", BadgeKind::Accent)
            } else if source_text(app).is_none() {
                ("Local only", BadgeKind::Neutral)
            } else if latest_tag.is_some() || mgr.latest_versions.contains_key(&app.id) {
                ("Up to date", BadgeKind::Success)
            } else {
                ("No updates", BadgeKind::Neutral)
            }
        }
    };
    let response = theme::badge_sized(ui, text, kind, size);
    match file_issue {
        Some(LocalFileIssue::Missing) => {
            response
                .on_hover_text("The managed AppImage is missing. Check for updates to restore it.");
        }
        Some(LocalFileIssue::Changed) => {
            response.on_hover_text(
                "Replaced outside AppImage Hub. Update it, or use ⋯ → Mark as Up to Date to accept the current file.",
            );
        }
        None => {}
    }
}

fn render_actions(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    state: CardState,
    has_update: bool,
    failed_msg: Option<&(String, bool)>,
    actions: &mut CardActions,
) {
    match state {
        CardState::Downloading => render_updating(ui),
        CardState::Failed => {
            if let Some((msg, can_retry)) = failed_msg {
                render_failed(ui, layout, msg, *can_retry, &mut actions.retry_app);
            }
        }
        CardState::Idle => render_idle_actions(ui, layout, has_update, actions),
    }
}

/// The card no longer renders its own progress bar; the single progress view
/// at the bottom of the window owns all download details and the Cancel
/// action for every in-flight update.
fn render_updating(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.add(egui::Spinner::new().size(14.0));
        ui.label(theme::muted_label("Updating…").size(12.0));
    })
    .response
    .on_hover_text("Progress is shown in the download bar at the bottom of the window");
}

fn render_failed(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    msg: &str,
    can_retry: bool,
    retry_app: &mut bool,
) {
    ui.add(egui::Label::new(RichText::new(msg).color(theme::danger()).size(12.0)).truncate())
        .on_hover_text(msg);
    ui.add_space(theme::SPACE_XS);
    let w = if layout.compact {
        ui.available_width().max(120.0)
    } else {
        116.0
    };
    let clicked = ui
        .add_enabled_ui(can_retry, |ui| {
            ui.add_sized([w, 32.0], theme::primary_button("Retry"))
        })
        .inner
        .on_disabled_hover_text("Retry unavailable")
        .clicked();
    if clicked {
        *retry_app = true;
    }
}

fn render_idle_actions(
    ui: &mut egui::Ui,
    _layout: &CardLayout,
    has_update: bool,
    actions: &mut CardActions,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(theme::SPACE_SM, theme::SPACE_SM);

        if has_update {
            run_action_selector(ui, actions);
        } else {
            // Idle primary action: Launch
            let launch = ui.add(theme::primary_button("Launch"));
            if launch.hovered() {
                theme::draw_glow(ui, launch.rect, theme::RADIUS_PILL, theme::accent());
            }
            if launch.clicked() {
                actions.launch = true;
            }
        }

        if ui.add(theme::secondary_button("Edit")).clicked() {
            actions.edit = true;
        }
        if ui.add(theme::ghost_button("Show in Folder")).clicked() {
            actions.show_folder = true;
        }
    });
}
fn run_action_selector(ui: &mut egui::Ui, actions: &mut CardActions) {
    let mut chosen: Option<RunAction> = None;
    egui::ComboBox::from_id_salt(ui.id().with("run_action"))
        .selected_text(
            RichText::new("Run...")
                .size(13.0)
                .strong()
                .color(theme::text_primary()),
        )
        .width(104.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(false, "Launch").clicked() {
                chosen = Some(RunAction::Launch);
            }
            if ui.selectable_label(false, "Update").clicked() {
                chosen = Some(RunAction::Update);
            }
        });

    match chosen {
        Some(RunAction::Launch) => actions.launch = true,
        Some(RunAction::Update) => actions.update = true,
        None => {}
    }
}

#[derive(Clone, Copy)]
enum RunAction {
    Launch,
    Update,
}

fn source_text(app: &AppEntry) -> Option<&str> {
    match &app.source {
        AppSource::LocalWithRepo { github_repo } | AppSource::GithubOnly { github_repo } => {
            Some(github_repo.as_str())
        }
        AppSource::LocalOnly => None,
    }
}

fn render_icon(ui: &mut egui::Ui, app: &AppEntry, size: f32) {
    let size = if size.is_finite() && size > 0.0 {
        size.clamp(1.0, 256.0)
    } else {
        64.0
    };

    let avail = ui.available_size();
    if !avail.x.is_finite() || !avail.y.is_finite() {
        return;
    }

    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(size), Sense::hover());
    if !rect.is_finite() || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let rounding = egui::CornerRadius::same(theme::radius_lg());
    ui.painter()
        .rect_filled(rect, rounding, theme::surface_raised());
    ui.painter().rect_stroke(
        rect,
        rounding,
        Stroke::new(1.0_f32, theme::border()),
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
        theme::detail(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_card_layout_responsiveness() {
        // Large / Medium: full 64 px icon, default name size.
        let large = card_layout_params(1400.0);
        assert_eq!(large.icon_size, 64.0);
        assert!(!large.compact);

        let medium = card_layout_params(1000.0);
        assert_eq!(medium.icon_size, 64.0);

        // Narrow: smaller icon and compact actions.
        let narrow = card_layout_params(700.0);
        assert_eq!(narrow.icon_size, 56.0);
        assert!(narrow.compact);

        // Tight: 48 px icon, compact.
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
        let alpha = sample_app("1", "alpha", AppSource::LocalOnly, None);
        let beta = sample_app("2", "Beta", AppSource::LocalOnly, None);
        let mut apps = vec![&beta, &alpha];
        sort_apps(&mut apps, &HashMap::new(), AppSortBy::Name);
        assert_eq!(
            apps.iter()
                .map(|app| app.display_name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "Beta"]
        );
    }

    #[test]
    fn sort_uses_cached_size() {
        let small = sample_app("small", "Small", AppSource::LocalOnly, None);
        let big = sample_app("big", "Big", AppSource::LocalOnly, None);
        let mut meta = HashMap::new();
        meta.insert(
            big.id.clone(),
            CachedAppMeta {
                size: 900,
                added_at: 0,
            },
        );
        meta.insert(
            small.id.clone(),
            CachedAppMeta {
                size: 1,
                added_at: 0,
            },
        );

        let mut apps = vec![&small, &big];
        sort_apps(&mut apps, &meta, AppSortBy::Size);

        assert_eq!(apps[0].id, "big");
        assert_eq!(apps[1].id, "small");
    }

    #[test]
    fn sort_by_date_added_uses_newest_first() {
        let old = sample_app("1", "Old", AppSource::LocalOnly, Some(100));
        let new = sample_app("2", "New", AppSource::LocalOnly, Some(200));
        let mut apps = vec![&old, &new];
        sort_apps(&mut apps, &HashMap::new(), AppSortBy::DateAdded);
        assert_eq!(apps[0].display_name, "New");
        assert_eq!(apps[1].display_name, "Old");
    }

    #[test]
    fn source_sort_puts_requested_source_first() {
        let local = sample_app("1", "Local", AppSource::LocalOnly, None);
        let github = sample_app(
            "2",
            "GitHub",
            AppSource::GithubOnly {
                github_repo: "owner/repo".into(),
            },
            None,
        );

        let mut apps = vec![&local, &github];
        sort_apps(&mut apps, &HashMap::new(), AppSortBy::Github);
        assert_eq!(apps[0].display_name, "GitHub");

        let mut apps = vec![&github, &local];
        sort_apps(&mut apps, &HashMap::new(), AppSortBy::Local);
        assert_eq!(apps[0].display_name, "Local");
    }
}
