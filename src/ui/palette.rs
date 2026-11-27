//! Raycast / Linear-style Command Palette (`Ctrl+K`).
//!
//! Lets the user fuzzy-search any installed AppImage to launch it instantly,
//! or invoke core actions (Add AppImage, Check Updates, Toggle Theme, Settings).

use crate::app::{ManagerApp, NavFilter, ViewMode};
use crate::ui::icons::{self, Icon};
use crate::ui::{fonts, theme};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Key, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2,
};

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub just_opened: bool,
}

impl CommandPalette {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.query.clear();
            self.selected = 0;
            self.just_opened = true;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
    }
}

#[derive(Clone)]
enum PaletteAction {
    LaunchApp(String),
    AddApp,
    CheckUpdates,
    UpdateAll,
    ToggleTheme,
    ToggleView,
    OpenSettings,
    Filter(NavFilter),
}

struct PaletteItem {
    icon: Icon,
    title: String,
    subtitle: String,
    shortcut: Option<&'static str>,
    action: PaletteAction,
}

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp) {
    if !mgr.palette.open {
        return;
    }

    let screen = ctx.screen_rect();
    let mut items = build_items(mgr);
    let q = mgr.palette.query.trim().to_lowercase();
    if !q.is_empty() {
        items.retain(|it| {
            it.title.to_lowercase().contains(&q) || it.subtitle.to_lowercase().contains(&q)
        });
    }

    if mgr.palette.selected >= items.len() {
        mgr.palette.selected = items.len().saturating_sub(1);
    }

    let (esc, up, down, enter) = ctx.input(|i| {
        (
            i.key_pressed(Key::Escape),
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
        )
    });

    if esc {
        mgr.palette.close();
        return;
    }
    if up && !items.is_empty() {
        mgr.palette.selected = mgr.palette.selected.saturating_sub(1);
    }
    if down && !items.is_empty() {
        mgr.palette.selected = (mgr.palette.selected + 1).min(items.len() - 1);
    }

    let mut chosen: Option<PaletteAction> = None;
    if enter {
        if let Some(item) = items.get(mgr.palette.selected) {
            chosen = Some(item.action.clone());
        }
    }

    // Dimmed backdrop
    let backdrop = egui::Area::new(egui::Id::new("cmd_palette_backdrop"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(125));
            resp
        });
    if backdrop.inner.clicked() {
        mgr.palette.close();
        return;
    }

    let card_w = (screen.width() - 48.0).clamp(300.0, 560.0);
    let top_y = screen.top() + (screen.height() * 0.14).clamp(48.0, 120.0);
    let card_pos = pos2(screen.center().x - card_w / 2.0, top_y);

    egui::Area::new(egui::Id::new("cmd_palette_card"))
        .order(egui::Order::Tooltip)
        .fixed_pos(card_pos)
        .show(ctx, |ui| {
            theme::modal_frame()
                .inner_margin(egui::Margin::same(0))
                .show(ui, |ui| {
                    ui.set_width(card_w);

                    // Search header row
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(16, 14))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                icons::widget(ui, Icon::Search, 17.0, theme::text_muted());
                                ui.add_space(6.0);
                                let edit = ui.add_sized(
                                    [ui.available_width() - 46.0, 26.0],
                                    egui::TextEdit::singleline(&mut mgr.palette.query)
                                        .frame(false)
                                        .font(fonts::regular(15.0))
                                        .hint_text(
                                            RichText::new("Launch an app or run a command…")
                                                .font(fonts::regular(15.0))
                                                .color(theme::text_muted()),
                                        ),
                                );
                                if mgr.palette.just_opened {
                                    edit.request_focus();
                                    mgr.palette.just_opened = false;
                                }
                                if edit.changed() {
                                    mgr.palette.selected = 0;
                                }
                                theme::keycap(ui, "Esc");
                            });
                        });

                    theme::divider(ui);

                    // Results list
                    let max_h = (screen.height() * 0.50).clamp(180.0, 360.0);
                    egui::ScrollArea::vertical()
                        .id_salt("cmd_palette_results")
                        .max_height(max_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(8))
                                .show(ui, |ui| {
                                    if items.is_empty() {
                                        ui.add_space(20.0);
                                        ui.vertical_centered(|ui| {
                                            ui.label(
                                                theme::muted_label("No matching apps or commands."),
                                            );
                                        });
                                        ui.add_space(20.0);
                                        return;
                                    }

                                    let p = theme::current();
                                    for (idx, item) in items.iter().enumerate() {
                                        let is_sel = idx == mgr.palette.selected;
                                        let row_w = ui.available_width();
                                        let (rect, resp) = ui.allocate_exact_size(
                                            vec2(row_w, 42.0),
                                            Sense::click(),
                                        );
                                        let resp = resp
                                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if resp.hovered() {
                                            mgr.palette.selected = idx;
                                        }

                                        if is_sel {
                                            ui.painter().rect_filled(
                                                rect,
                                                CornerRadius::same(p.radius_md),
                                                p.accent_soft,
                                            );
                                            ui.painter().rect_stroke(
                                                rect,
                                                CornerRadius::same(p.radius_md),
                                                Stroke::new(1.0_f32, theme::with_alpha(p.accent, 90)),
                                                StrokeKind::Inside,
                                            );
                                        }

                                        let icon_col = if is_sel {
                                            p.accent
                                        } else {
                                            p.text_secondary
                                        };
                                        icons::paint(
                                            ui.painter(),
                                            item.icon,
                                            pos2(rect.left() + 20.0, rect.center().y),
                                            16.0,
                                            icon_col,
                                        );

                                        ui.painter().text(
                                            pos2(rect.left() + 40.0, rect.center().y - 6.5),
                                            Align2::LEFT_CENTER,
                                            &item.title,
                                            fonts::medium(13.5),
                                            p.text_primary,
                                        );
                                        ui.painter().text(
                                            pos2(rect.left() + 40.0, rect.center().y + 8.5),
                                            Align2::LEFT_CENTER,
                                            &item.subtitle,
                                            fonts::regular(11.0),
                                            p.text_muted,
                                        );

                                        if let Some(sc) = item.shortcut {
                                            let g = ui.painter().layout_no_wrap(
                                                sc.to_owned(),
                                                fonts::mono(10.0),
                                                p.text_muted,
                                            );
                                            let pill = Rect::from_center_size(
                                                pos2(
                                                    rect.right() - 14.0 - g.size().x / 2.0,
                                                    rect.center().y,
                                                ),
                                                vec2(g.size().x + 10.0, 20.0),
                                            );
                                            ui.painter().rect_filled(
                                                pill,
                                                CornerRadius::same(5),
                                                p.neutral_soft,
                                            );
                                            ui.painter().galley(
                                                pos2(
                                                    pill.center().x - g.size().x / 2.0,
                                                    pill.center().y - g.size().y / 2.0,
                                                ),
                                                g,
                                                p.text_muted,
                                            );
                                        }

                                        if resp.clicked() {
                                            chosen = Some(item.action.clone());
                                        }
                                    }
                                });
                        });
                });
        });

    if let Some(action) = chosen {
        mgr.palette.close();
        execute_action(ctx, mgr, action);
    }
}

fn build_items(mgr: &ManagerApp) -> Vec<PaletteItem> {
    let mut items = Vec::new();

    // Installed apps first so typing an app name launches it in 1 keystroke
    for app in mgr.snapshot.apps.iter() {
        let subtitle = if app.current_version.trim().is_empty() {
            "Launch AppImage".to_string()
        } else {
            format!("Launch • {}", app.current_version)
        };
        items.push(PaletteItem {
            icon: Icon::Play,
            title: app.display_name.clone(),
            subtitle,
            shortcut: Some("Enter"),
            action: PaletteAction::LaunchApp(app.id.clone()),
        });
    }

    items.push(PaletteItem {
        icon: Icon::Plus,
        title: "Add AppImage".into(),
        subtitle: "Import a local AppImage or install from GitHub".into(),
        shortcut: Some("Ctrl+N"),
        action: PaletteAction::AddApp,
    });
    items.push(PaletteItem {
        icon: Icon::Refresh,
        title: "Check All Updates".into(),
        subtitle: "Check GitHub releases for every tracked app".into(),
        shortcut: None,
        action: PaletteAction::CheckUpdates,
    });
    let updates_count = mgr.updates_available_count();
    if updates_count > 0 {
        items.push(PaletteItem {
            icon: Icon::Download,
            title: format!("Update All ({updates_count})"),
            subtitle: "Download and install all available updates".into(),
            shortcut: None,
            action: PaletteAction::UpdateAll,
        });
    }
    items.push(PaletteItem {
        icon: if theme::is_light() {
            Icon::Moon
        } else {
            Icon::Sun
        },
        title: if theme::is_light() {
            "Switch to Dark Theme".into()
        } else {
            "Switch to Light Theme".into()
        },
        subtitle: "Toggle between Dark and Light appearance".into(),
        shortcut: None,
        action: PaletteAction::ToggleTheme,
    });
    items.push(PaletteItem {
        icon: match mgr.view_mode {
            ViewMode::Grid => Icon::LayoutList,
            ViewMode::List => Icon::LayoutGrid,
        },
        title: match mgr.view_mode {
            ViewMode::Grid => "Switch to List View".into(),
            ViewMode::List => "Switch to Grid View".into(),
        },
        subtitle: "Toggle between Bento Grid and Dense List layout".into(),
        shortcut: None,
        action: PaletteAction::ToggleView,
    });
    items.push(PaletteItem {
        icon: Icon::Settings,
        title: "Open Settings".into(),
        subtitle: "Themes, detail colors, storage, shortcuts, and GitHub API".into(),
        shortcut: Some("Ctrl+,"),
        action: PaletteAction::OpenSettings,
    });
    items.push(PaletteItem {
        icon: Icon::Updates,
        title: "Show Updates Only".into(),
        subtitle: "Filter library to apps with pending updates".into(),
        shortcut: None,
        action: PaletteAction::Filter(NavFilter::Updates),
    });
    items.push(PaletteItem {
        icon: Icon::AppsGrid,
        title: "Show All Apps".into(),
        subtitle: "Reset library filter and view all managed apps".into(),
        shortcut: None,
        action: PaletteAction::Filter(NavFilter::All),
    });

    items
}

fn execute_action(ctx: &egui::Context, mgr: &mut ManagerApp, action: PaletteAction) {
    match action {
        PaletteAction::LaunchApp(id) => mgr.launch_app(&id),
        PaletteAction::AddApp => {
            if mgr.show_settings {
                mgr.close_settings(ctx);
            }
            mgr.open_add_form();
        }
        PaletteAction::CheckUpdates => mgr.check_all_updates(),
        PaletteAction::UpdateAll => mgr.update_all(),
        PaletteAction::ToggleTheme => mgr.toggle_theme_mode(ctx),
        PaletteAction::ToggleView => {
            mgr.view_mode = match mgr.view_mode {
                ViewMode::Grid => ViewMode::List,
                ViewMode::List => ViewMode::Grid,
            };
        }
        PaletteAction::OpenSettings => mgr.open_settings(ctx),
        PaletteAction::Filter(f) => {
            if mgr.show_settings {
                mgr.close_settings(ctx);
            }
            mgr.nav_filter = f;
        }
    }
}
