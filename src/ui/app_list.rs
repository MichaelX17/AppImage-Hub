//! AppImage cards: neon/dark launcher-style rows.
//!
//! Layout:  [icon] [name + badge / repo / versions] [⋯ more]
//!          ─────────────────────────────────────────────
//!          [primary action] [secondary actions …]
//!
//! All backend interactions are funneled through [`CardActions`] so the card is
//! rendered first and `ManagerApp` is mutated afterwards (no borrow conflicts).

use crate::app::{AppDownloadState, AppSortBy, BackendCommand, ManagerApp};
use crate::config::{AppEntry, AppSource};
use crate::ui::breakpoints::SizeClass;
use crate::ui::format;
use crate::ui::theme::{self, BadgeKind};
use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke};

#[derive(Clone, Copy, PartialEq, Eq)]
enum CardState {
    Idle,
    Downloading,
    Failed,
}

/// Snapshot of an in-flight download for one card.
type DownloadSnapshot = (String, f32, u64, Option<u64>, std::time::Instant);

/// Per-card layout decisions derived from the available card width.
struct CardLayout {
    /// App icon edge length (large 64 px, down to 48 px when tight).
    icon_size: f32,
    /// Display-name font size.
    name_size: f32,
    /// Compact cards use a full-width progress row and tighter actions.
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

pub fn render(ui: &mut egui::Ui, apps: &[AppEntry], mgr: &mut ManagerApp, size_class: SizeClass) {
    let _ = size_class;
    if apps.is_empty() {
        render_empty_state(ui, mgr);
        return;
    }

    let mut sorted_apps: Vec<&AppEntry> = apps.iter().collect();
    sort_apps(&mut sorted_apps, mgr.sort_by);

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

fn sort_apps(apps: &mut [&AppEntry], sort_by: AppSortBy) {
    let by_name = |a: &&AppEntry, b: &&AppEntry| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    };

    match sort_by {
        AppSortBy::Name => apps.sort_by(by_name),
        AppSortBy::Size => {
            apps.sort_by(|a, b| app_size(b).cmp(&app_size(a)).then_with(|| by_name(a, b)))
        }
        AppSortBy::DateAdded => apps.sort_by(|a, b| {
            app_added_at(b)
                .cmp(&app_added_at(a))
                .then_with(|| by_name(a, b))
        }),
        AppSortBy::Github => apps.sort_by(|a, b| {
            source_rank(b)
                .cmp(&source_rank(a))
                .then_with(|| by_name(a, b))
        }),
        AppSortBy::Local => apps.sort_by(|a, b| {
            source_rank(a)
                .cmp(&source_rank(b))
                .then_with(|| by_name(a, b))
        }),
    }
}

fn app_size(app: &AppEntry) -> u64 {
    app.local_path
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0)
}

fn app_added_at(app: &AppEntry) -> i64 {
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

fn render_empty_state(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.vertical_centered(|ui| {
        ui.add_space(56.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(76.0, 76.0), Sense::hover());
        let center = rect.center();
        ui.painter().circle_filled(center, 38.0, theme::ACCENT_SOFT);
        ui.painter()
            .circle_stroke(center, 38.0, Stroke::new(1.5_f32, theme::ACCENT));
        let arm = 13.0;
        ui.painter().line_segment(
            [center - egui::vec2(arm, 0.0), center + egui::vec2(arm, 0.0)],
            Stroke::new(2.5_f32, theme::ACCENT_HOVER),
        );
        ui.painter().line_segment(
            [center - egui::vec2(0.0, arm), center + egui::vec2(0.0, arm)],
            Stroke::new(2.5_f32, theme::ACCENT_HOVER),
        );
        ui.add_space(theme::SPACE_MD);
        ui.label(
            RichText::new("No AppImages managed yet")
                .size(22.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
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

    let downloading: Option<DownloadSnapshot> =
        if let AppDownloadState::Downloading(op) = mgr.app_download_state(&app.id) {
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
                        downloading.as_ref(),
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
        let pos = theme::menu_popup_pos(more_btn_rect, card.response.rect);
        let area = egui::Area::new(menu_id.with("popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                theme::menu_frame().show(ui, |ui| {
                    ui.set_min_width(theme::MENU_WIDTH);
                    ui.spacing_mut().item_spacing.y = theme::SPACE_SM;

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
            && !pointer.map_or(false, |p| {
                menu_rect.contains(p) || more_btn_rect.contains(p)
            });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) || clicked_elsewhere {
            menu_open = false;
        }
    }

    // Keep the card's normal border only; no outer hover/selection glow.

    // Persist the overflow menu state for this card.
    ui.data_mut(|data| data.insert_temp(menu_id, menu_open));

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
        if let Some((tag, url, _asset)) = update_info {
            mgr.start_update_download(app.id.clone(), url, tag);
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
                .color(theme::TEXT_PRIMARY),
        )
        .truncate(),
    );

    // ── Repository / source ─────────────────────────────────────────────
    match source_text(app) {
        Some(repo) => {
            ui.add(egui::Label::new(theme::muted_label(repo.clone()).size(12.0)).truncate())
                .on_hover_text(repo);
        }
        None => {
            ui.label(theme::muted_label("Local file (no repository)").size(12.0));
        }
    }

    ui.add_space(theme::SPACE_XS);

    // ── Current / latest versions ───────────────────────────────────────
    let current = if app.current_version.trim().is_empty() {
        "unknown".to_string()
    } else {
        app.current_version.clone()
    };
    let latest_known = latest_tag
        .map(|tag| tag.to_string())
        .or_else(|| mgr.latest_versions.get(&app.id).cloned());

    let (latest_text, latest_color) = match &latest_known {
        Some(tag) if has_update => (tag.clone(), theme::ACCENT_HOVER),
        Some(tag) => (tag.clone(), theme::SUCCESS),
        None => ("unknown".to_string(), theme::TEXT_MUTED),
    };

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        theme::status_dot(ui, theme::TEXT_MUTED);
        ui.label(theme::muted_label("Current").size(11.0));
        ui.label(theme::secondary_label(current).size(12.0).strong());
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
fn render_status_badge(
    ui: &mut egui::Ui,
    app: &AppEntry,
    state: CardState,
    has_update: bool,
    mgr: &ManagerApp,
    latest_tag: Option<&str>,
    size: egui::Vec2,
) {
    let (text, kind) = match state {
        CardState::Downloading => ("Downloading", BadgeKind::Accent),
        CardState::Failed => ("Failed", BadgeKind::Danger),
        CardState::Idle => {
            if has_update {
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
    theme::badge_sized(ui, text, kind, size);
}

fn render_actions(
    ui: &mut egui::Ui,
    layout: &CardLayout,
    state: CardState,
    has_update: bool,
    downloading: Option<&DownloadSnapshot>,
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
        CardState::Idle => render_idle_actions(ui, layout, has_update, actions),
    }
}

/// Progress bar + percentage + ETA + Cancel.
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

    let bar = |ui: &mut egui::Ui, width: f32| {
        ui.add(
            egui::ProgressBar::new(fraction)
                .show_percentage()
                .fill(theme::ACCENT)
                .desired_width(width.max(80.0)),
        );
    };

    if layout.compact {
        ui.vertical(|ui| {
            let w = ui.available_width().max(120.0);
            bar(ui, w);
            ui.add_space(theme::SPACE_XS);
            ui.label(theme::secondary_label(format!("{progress} • ETA {eta}")).size(11.0));
            if ui
                .add_sized([w, 32.0], theme::secondary_button("Cancel"))
                .clicked()
            {
                *cancel_op_id = Some(op_id.to_string());
            }
        });
    } else {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                let cancel_w = 96.0;
                let spacing = ui.spacing().item_spacing.x;
                let bar_w = (ui.available_width() - cancel_w - spacing).max(140.0);
                bar(ui, bar_w);
                if ui
                    .add_sized([cancel_w, 32.0], theme::secondary_button("Cancel"))
                    .clicked()
                {
                    *cancel_op_id = Some(op_id.to_string());
                }
            });
            ui.label(theme::secondary_label(format!("{progress} • ETA {eta}")).size(11.0));
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
    ui.add(egui::Label::new(RichText::new(msg).color(theme::DANGER).size(12.0)).truncate())
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
            // Glowing primary action: Update
            let update = ui.add(theme::update_button());
            let glow = if update.hovered() {
                theme::ACCENT_HOVER
            } else {
                theme::with_alpha(theme::ACCENT, 190)
            };
            theme::draw_glow(ui, update.rect, theme::RADIUS_PILL, glow);
            if update.clicked() {
                actions.update = true;
            }
            if ui.add(theme::secondary_button("Launch")).clicked() {
                actions.launch = true;
            }
        } else {
            // Idle primary action: Launch
            let launch = ui.add(theme::primary_button("Launch"));
            if launch.hovered() {
                theme::draw_glow(ui, launch.rect, theme::RADIUS_PILL, theme::ACCENT);
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

    let rounding = egui::CornerRadius::same(theme::RADIUS_LG);
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

    #[test]
    fn test_sources() {
        let local = AppEntry {
            id: "1".into(),
            display_name: "App".into(),
            source: AppSource::LocalOnly,
            local_path: std::path::PathBuf::from("/tmp/app.AppImage"),
            current_version: String::new(),
            args: Vec::new(),
            icon_path: None,
            generic_name: None,
            comment: None,
            categories: None,
            startup_notify: true,
            asset_preference: None,
            added_at: None,
        };
        assert!(source_text(&local).is_none());

        let repo = AppEntry {
            source: AppSource::GithubOnly {
                github_repo: "owner/repo".into(),
            },
            ..local
        };
        assert_eq!(source_text(&repo).as_deref(), Some("owner/repo"));
    }
}
