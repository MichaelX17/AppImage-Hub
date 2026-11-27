//! Full-page modern Settings screen ("Obsidian / Alabaster Studio").
//!
//! - Large / Medium viewports: Left icon-navigation category rail + right grouped cards.
//! - Narrow / Tight viewports: Wrapped horizontal category pills + full-width cards.
//! - Includes Curated Quick-Theme Presets, Dark/Light base cards, Custom Detail Color
//!   Pickers (Accent, Surface Tint, Border/Divider, Badge/Highlight), Corner Roundness,
//!   and a live Bento Card preview.

use crate::app::ManagerApp;
use crate::config::{ThemeMode, ThemeSettings};
use crate::ui::breakpoints::SizeClass;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, ButtonTone};
use crate::ui::{colorpick, fonts};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Margin, RichText, Sense, Stroke, StrokeKind, pos2,
    vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Appearance,
    Storage,
    Updates,
    Shortcuts,
    Github,
    About,
}

impl SettingsTab {
    pub const ALL: [Self; 6] = [
        Self::Appearance,
        Self::Storage,
        Self::Updates,
        Self::Shortcuts,
        Self::Github,
        Self::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Appearance => "Appearance",
            Self::Storage => "Storage",
            Self::Updates => "Updates",
            Self::Shortcuts => "Shortcuts",
            Self::Github => "GitHub API",
            Self::About => "About",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Self::Appearance => Icon::Palette,
            Self::Storage => Icon::Folder,
            Self::Updates => Icon::Refresh,
            Self::Shortcuts => Icon::Link,
            Self::Github => Icon::Key,
            Self::About => Icon::Info,
        }
    }
}

#[derive(Clone)]
pub struct SettingsFormState {
    pub active_tab: SettingsTab,
    pub theme: ThemeSettings,
    pub apps_dir: String,
    pub auto_check_updates: bool,
    pub hub_start_menu_shortcut: bool,
    pub hub_start_menu_category: String,
    pub hub_desktop_shortcut: bool,
    pub github_token: String,
    pub show_token: bool,
}

impl Default for SettingsFormState {
    fn default() -> Self {
        Self {
            active_tab: SettingsTab::Appearance,
            theme: ThemeSettings::default(),
            apps_dir: String::new(),
            auto_check_updates: true,
            hub_start_menu_shortcut: true,
            hub_start_menu_category: "Utility".to_string(),
            hub_desktop_shortcut: false,
            github_token: String::new(),
            show_token: false,
        }
    }
}

pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp, size_class: SizeClass) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        mgr.close_settings(ui.ctx());
        return;
    }

    let compact = matches!(size_class, SizeClass::Narrow | SizeClass::Tight);

    // ── Header ───────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if theme::pill_action(ui, Some(Icon::ChevronLeft), "Back", ButtonTone::Secondary, 32.0)
            .on_hover_text("Return to AppImages (Esc)")
            .clicked()
        {
            mgr.close_settings(ui.ctx());
            return;
        }
        ui.add_space(theme::SPACE_SM);
        ui.label(
            RichText::new("Settings")
                .font(fonts::bold(24.0))
                .color(theme::text_primary()),
        );
        if !compact {
            ui.add_space(theme::SPACE_SM);
            ui.label(
                theme::secondary_label("Theme, detail colors, storage, and preferences")
                    .font(fonts::regular(13.0)),
            );
        }
    });

    ui.add_space(theme::SPACE_MD);
    theme::divider(ui);
    ui.add_space(theme::SPACE_MD);

    let footer_h = 56.0;
    let body_h = (ui.available_height() - footer_h).max(140.0);
    let mut theme_changed = false;

    if compact {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tab in SettingsTab::ALL {
                let active = mgr.settings_form.active_tab == tab;
                let tone = if active {
                    ButtonTone::SubtleAccent
                } else {
                    ButtonTone::Ghost
                };
                if theme::pill_action(ui, Some(tab.icon()), tab.label(), tone, 30.0).clicked() {
                    mgr.settings_form.active_tab = tab;
                }
            }
        });
        ui.add_space(theme::SPACE_MD);

        let scroll_h = (body_h - 48.0).max(100.0);
        egui::ScrollArea::vertical()
            .id_salt("settings_content_compact")
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                theme_changed = render_tab_content(ui, mgr);
                ui.add_space(theme::SPACE_LG);
            });
    } else {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body_h),
            egui::Layout::left_to_right(Align::Min),
            |ui| {
                let nav_w = 192.0;
                ui.vertical(|ui| {
                    ui.set_width(nav_w);
                    ui.label(theme::section_heading("Categories"));
                    ui.add_space(6.0);
                    for tab in SettingsTab::ALL {
                        let active = mgr.settings_form.active_tab == tab;
                        if theme::nav_item(ui, tab.icon(), tab.label(), None, active).clicked() {
                            mgr.settings_form.active_tab = tab;
                        }
                        ui.add_space(2.0);
                    }
                });

                ui.add_space(theme::SPACE_XL);

                let content_w = ui.available_width().clamp(260.0, 760.0);
                ui.vertical(|ui| {
                    ui.set_width(content_w);
                    egui::ScrollArea::vertical()
                        .id_salt("settings_content_wide")
                        .auto_shrink([false, false])
                        .max_height(body_h)
                        .show(ui, |ui| {
                            theme_changed = render_tab_content(ui, mgr);
                            ui.add_space(theme::SPACE_LG);
                        });
                });
            },
        );
    }

    if theme_changed {
        theme::apply_theme(ui.ctx(), &mgr.settings_form.theme);
    }

    // ── Bottom Action Bar ────────────────────────────────────────────────
    theme::divider(ui);
    ui.add_space(theme::SPACE_SM);
    ui.horizontal(|ui| {
        if theme::pill_action(ui, Some(Icon::Check), "Save Settings", ButtonTone::Primary, 36.0)
            .clicked()
        {
            mgr.save_settings(ui.ctx());
        }
        ui.add_space(theme::SPACE_SM);
        if theme::pill_action(ui, None, "Cancel", ButtonTone::Ghost, 36.0).clicked() {
            mgr.close_settings(ui.ctx());
        }
    });
}

fn render_tab_content(ui: &mut egui::Ui, mgr: &mut ManagerApp) -> bool {
    match mgr.settings_form.active_tab {
        SettingsTab::Appearance => render_appearance_tab(ui, mgr),
        SettingsTab::Storage => {
            render_storage_tab(ui, mgr);
            false
        }
        SettingsTab::Updates => {
            render_updates_tab(ui, mgr);
            false
        }
        SettingsTab::Shortcuts => {
            render_shortcuts_tab(ui, mgr);
            false
        }
        SettingsTab::Github => {
            render_github_tab(ui, mgr);
            false
        }
        SettingsTab::About => {
            render_about_tab(ui, mgr);
            false
        }
    }
}

fn render_appearance_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) -> bool {
    let mut changed = false;

    // ── Section 1: Base Mode (Dark / Light) ──────────────────────────────
    ui.label(theme::section_heading("Base Theme"));
    ui.add_space(theme::SPACE_SM);

    theme::settings_card(ui, |ui| {
        let avail = ui.available_width();
        let stack = avail < 380.0;
        let card_w = if stack {
            avail
        } else {
            ((avail - theme::SPACE_MD) / 2.0).max(140.0)
        };

        let mut draw_cards = |ui: &mut egui::Ui| {
            if theme_mode_card(
                ui,
                ThemeMode::Dark,
                mgr.settings_form.theme.mode == ThemeMode::Dark,
                card_w,
            ) {
                mgr.settings_form.theme.mode = ThemeMode::Dark;
                changed = true;
            }
            if stack {
                ui.add_space(theme::SPACE_SM);
            }
            if theme_mode_card(
                ui,
                ThemeMode::Light,
                mgr.settings_form.theme.mode == ThemeMode::Light,
                card_w,
            ) {
                mgr.settings_form.theme.mode = ThemeMode::Light;
                changed = true;
            }
        };

        if stack {
            ui.vertical(draw_cards);
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme::SPACE_MD;
                draw_cards(ui);
            });
        }
    });

    // ── Section 2: Curated Studio Presets ────────────────────────────────
    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Curated Looks"));
    ui.add_space(theme::SPACE_SM);

    theme::settings_card(ui, |ui| {
        let presets = theme::curated_presets();
        let gap = 8.0;
        let avail = ui.available_width();
        let cols = if avail >= 520.0 {
            3
        } else if avail >= 320.0 {
            2
        } else {
            1
        };
        let pill_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(130.0);

        for chunk in presets.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for preset in chunk {
                    let is_active = mgr.settings_form.theme == preset.settings;
                    if preset_tile(ui, preset, is_active, pill_w) {
                        mgr.settings_form.theme = preset.settings.clone();
                        changed = true;
                    }
                }
            });
            ui.add_space(gap);
        }
    });

    // ── Section 3: Granular Detail Personalization ───────────────────────
    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Detail Personalization"));
    ui.add_space(theme::SPACE_SM);

    theme::settings_card(ui, |ui| {
        theme::setting_row(
            ui,
            "Accent color",
            Some("Primary buttons, active navigation indicators, toggles, and progress bars."),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if colorpick::swatch_button(
                        ui,
                        "accent_custom_picker",
                        &mut mgr.settings_form.theme.accent_rgb,
                    ) {
                        changed = true;
                    }
                    ui.label(
                        RichText::new(colorpick::to_hex(colorpick::rgb_to_color(
                            mgr.settings_form.theme.accent_rgb,
                        )))
                        .font(fonts::mono(12.0))
                        .color(theme::text_secondary()),
                    );
                });
            },
        );

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for &(name, rgb) in theme::ACCENT_PRESETS {
                let selected = mgr.settings_form.theme.accent_rgb == rgb;
                let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
                let resp = resp
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(name);
                let col = colorpick::rgb_to_color(rgb);
                ui.painter().circle_filled(rect.center(), 10.0, col);
                if selected {
                    ui.painter().circle_stroke(
                        rect.center(),
                        12.5,
                        Stroke::new(2.0_f32, theme::text_primary()),
                    );
                } else if resp.hovered() {
                    ui.painter().circle_stroke(
                        rect.center(),
                        12.0,
                        Stroke::new(1.5_f32, theme::with_alpha(theme::text_primary(), 135)),
                    );
                }
                if resp.clicked() {
                    mgr.settings_form.theme.accent_rgb = rgb;
                    changed = true;
                }
            }
        });
        ui.add_space(theme::SPACE_SM);
        theme::divider(ui);

        theme::setting_row(
            ui,
            "Card & surface tint",
            Some("Subtly tint Bento cards and panels while preserving Dark or Light contrast."),
            |ui| {
                if colorpick::optional_detail_picker(
                    ui,
                    "detail_surface",
                    &mut mgr.settings_form.theme.custom_surface_rgb,
                    theme::surface(),
                ) {
                    changed = true;
                }
            },
        );
        theme::divider(ui);

        theme::setting_row(
            ui,
            "Border & hairline color",
            Some("Customize outlines around Bento cards, inputs, and floating menus."),
            |ui| {
                if colorpick::optional_detail_picker(
                    ui,
                    "detail_border",
                    &mut mgr.settings_form.theme.custom_border_rgb,
                    theme::border(),
                ) {
                    changed = true;
                }
            },
        );
        theme::divider(ui);

        theme::setting_row(
            ui,
            "Badge & highlight detail",
            Some("Color for update tags, version arrows, and fallback app initials."),
            |ui| {
                if colorpick::optional_detail_picker(
                    ui,
                    "detail_badge",
                    &mut mgr.settings_form.theme.custom_detail_rgb,
                    theme::detail(),
                ) {
                    changed = true;
                }
            },
        );
        theme::divider(ui);

        theme::setting_row(
            ui,
            "Corner roundness",
            Some("Adjust curvature of cards, modals, and inputs."),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (label, r) in [("Sharp", 8_u8), ("Balanced", 14_u8), ("Soft", 18_u8)] {
                        let active = mgr.settings_form.theme.corner_radius == r;
                        let tone = if active {
                            ButtonTone::Primary
                        } else {
                            ButtonTone::Secondary
                        };
                        if theme::pill_action(ui, None, label, tone, 28.0).clicked() {
                            mgr.settings_form.theme.corner_radius = r;
                            changed = true;
                        }
                    }
                });
            },
        );
        theme::divider(ui);

        theme::setting_row(
            ui,
            "Reset appearance",
            Some("Restore clean default detail colors for the selected Dark or Light mode."),
            |ui| {
                if theme::pill_action(ui, Some(Icon::Refresh), "Reset Defaults", ButtonTone::Secondary, 30.0)
                    .clicked()
                {
                    let keep_mode = mgr.settings_form.theme.mode;
                    mgr.settings_form.theme = ThemeSettings {
                        mode: keep_mode,
                        ..ThemeSettings::default()
                    };
                    changed = true;
                }
            },
        );
    });

    // ── Section 4: Live Preview Bento Card ───────────────────────────────
    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Live Preview"));
    ui.add_space(theme::SPACE_SM);

    let (preview_rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), 82.0), Sense::hover());
    theme::paint_bento_card(ui.painter(), preview_rect, 0.25, true);
    let inner = preview_rect.shrink2(vec2(16.0, 14.0));
    let mut pui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::left_to_right(Align::Center)),
    );
    let (icon_r, _) = pui.allocate_exact_size(vec2(46.0, 46.0), Sense::hover());
    let cr = CornerRadius::same(11);
    pui.painter()
        .rect_filled(icon_r, cr, theme::surface_raised());
    pui.painter().rect_stroke(
        icon_r,
        cr,
        Stroke::new(1.0_f32, theme::border()),
        StrokeKind::Inside,
    );
    pui.painter().text(
        icon_r.center(),
        Align2::CENTER_CENTER,
        "A",
        fonts::bold(20.0),
        theme::detail(),
    );
    pui.add_space(10.0);
    pui.vertical(|ui| {
        ui.label(
            RichText::new("Sample AppImage")
                .font(fonts::bold(15.0))
                .color(theme::text_primary()),
        );
        ui.horizontal(|ui| {
            theme::version_pill(ui, "v1.4 → v1.5", theme::detail(), theme::detail_soft());
            theme::status_pill(ui, "Update ready", theme::BadgeKind::Accent);
        });
    });
    pui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
        let _ = theme::pill_action(ui, Some(Icon::Play), "Launch", ButtonTone::Primary, 32.0);
    });

    changed
}

fn preset_tile(
    ui: &mut egui::Ui,
    preset: &theme::ThemePreset,
    selected: bool,
    width: f32,
) -> bool {
    let height = 56.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());

    let p = theme::current();
    let cr = CornerRadius::same(p.radius_md.max(8));
    let fill = if selected {
        p.accent_soft
    } else {
        theme::lerp_color(p.surface_raised, p.surface_hover, hover)
    };
    let stroke = if selected {
        Stroke::new(1.5_f32, p.accent)
    } else {
        Stroke::new(1.0_f32, theme::lerp_color(p.border, p.border_hover, hover))
    };

    ui.painter().rect_filled(rect, cr, fill);
    ui.painter()
        .rect_stroke(rect, cr, stroke, StrokeKind::Inside);

    // Mini dual color dot (base bg + accent)
    let bg_dot = if preset.settings.mode == ThemeMode::Dark {
        Color32::from_rgb(0x12, 0x12, 0x18)
    } else {
        Color32::from_rgb(0xF5, 0xF5, 0xF8)
    };
    let acc_dot = colorpick::rgb_to_color(preset.settings.accent_rgb);
    ui.painter()
        .circle_filled(pos2(rect.left() + 18.0, rect.center().y), 9.0, bg_dot);
    ui.painter().circle_stroke(
        pos2(rect.left() + 18.0, rect.center().y),
        9.0,
        Stroke::new(1.0_f32, p.border_hover),
    );
    ui.painter()
        .circle_filled(pos2(rect.left() + 26.0, rect.center().y), 7.5, acc_dot);

    ui.painter().text(
        pos2(rect.left() + 42.0, rect.center().y - 7.0),
        Align2::LEFT_CENTER,
        preset.name,
        fonts::medium(12.5),
        p.text_primary,
    );
    ui.painter().text(
        pos2(rect.left() + 42.0, rect.center().y + 8.0),
        Align2::LEFT_CENTER,
        preset.subtitle,
        fonts::regular(10.5),
        p.text_muted,
    );

    resp.clicked()
}

fn theme_mode_card(ui: &mut egui::Ui, mode: ThemeMode, selected: bool, width: f32) -> bool {
    let height = 98.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let r = CornerRadius::same(theme::radius_lg());

    let (preview_bg, preview_side, preview_card, preview_text, icon) = match mode {
        ThemeMode::Dark => (
            Color32::from_rgb(0x0A, 0x0A, 0x0E),
            Color32::from_rgb(0x11, 0x11, 0x18),
            Color32::from_rgb(0x18, 0x18, 0x22),
            Color32::from_rgb(0xF4, 0xF4, 0xF8),
            Icon::Moon,
        ),
        ThemeMode::Light => (
            Color32::from_rgb(0xF5, 0xF5, 0xF7),
            Color32::from_rgb(0xEC, 0xEC, 0xF1),
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            Color32::from_rgb(0x11, 0x11, 0x18),
            Icon::Sun,
        ),
    };

    let p = ui.painter();
    p.rect_filled(rect, r, preview_bg);

    let side = egui::Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(34.0, height - 16.0));
    p.rect_filled(side, CornerRadius::same(7), preview_side);
    let pill = egui::Rect::from_min_size(side.min + vec2(6.0, 12.0), vec2(22.0, 6.0));
    p.rect_filled(pill, CornerRadius::same(3), theme::accent());

    // Mini 2-column Bento cards inside preview
    let grid_left = side.right() + 8.0;
    let half_w = ((rect.right() - 10.0 - grid_left) - 6.0) / 2.0;
    let c1 = egui::Rect::from_min_size(pos2(grid_left, rect.top() + 10.0), vec2(half_w, 44.0));
    let c2 = egui::Rect::from_min_size(
        pos2(c1.right() + 6.0, rect.top() + 10.0),
        vec2(half_w, 44.0),
    );
    p.rect_filled(c1, CornerRadius::same(7), preview_card);
    p.rect_filled(c2, CornerRadius::same(7), preview_card);

    icons::paint(
        p,
        icon,
        pos2(grid_left + 8.0, rect.bottom() - 16.0),
        14.0,
        if selected {
            theme::accent()
        } else {
            preview_text
        },
    );
    p.text(
        pos2(grid_left + 22.0, rect.bottom() - 16.0),
        Align2::LEFT_CENTER,
        mode.label(),
        fonts::bold(13.5),
        preview_text,
    );

    let stroke = if selected {
        Stroke::new(2.0_f32, theme::accent())
    } else if resp.hovered() {
        Stroke::new(1.5_f32, theme::with_alpha(theme::accent(), 140))
    } else {
        Stroke::new(1.0_f32, theme::border())
    };
    p.rect_stroke(rect, r, stroke, StrokeKind::Inside);

    resp.clicked()
}

fn render_storage_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("AppImage Storage"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        ui.label(theme::field_label("AppImage root folder"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let browse_w = 104.0;
            let spacing = ui.spacing().item_spacing.x;
            let input_w = (ui.available_width() - browse_w - spacing).max(120.0);
            ui.add_sized(
                [input_w, 38.0],
                egui::TextEdit::singleline(&mut mgr.settings_form.apps_dir)
                    .font(fonts::mono(12.5))
                    .hint_text("Folder where AppImages are stored")
                    .margin(Margin::symmetric(12, 9))
                    .vertical_align(Align::Center)
                    .desired_width(input_w),
            );
            if theme::pill_action(ui, Some(Icon::Folder), "Browse", ButtonTone::Secondary, 38.0)
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Choose AppImage storage folder")
                    .pick_folder()
                {
                    mgr.settings_form.apps_dir = path.display().to_string();
                }
            }
        });
        ui.add_space(6.0);
        ui.label(
            theme::muted_label("New AppImages are organized into per-app subfolders here.")
                .font(fonts::regular(12.0)),
        );
    });
}

fn render_updates_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("Update Checks"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        theme::setting_row(
            ui,
            "Auto-check on startup",
            Some("Check GitHub releases automatically whenever AppImage Hub launches."),
            |ui| {
                theme::toggle(ui, &mut mgr.settings_form.auto_check_updates);
            },
        );
        theme::divider(ui);
        let cooldown_text = if mgr.update_check_cooldown() <= 60 {
            "Rate-limited to once every 1 minute (GitHub token configured)."
        } else {
            "Rate-limited to once every 5 minutes. Add a GitHub token to shorten it to 1 minute."
        };
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::muted_label(cooldown_text).font(fonts::regular(12.0)));
        ui.add_space(theme::SPACE_XS);
    });
}

fn render_shortcuts_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("AppImage Hub Shortcuts"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        theme::setting_row(
            ui,
            "Start Menu shortcut",
            Some("Create and maintain AppImage Hub's own application launcher entry."),
            |ui| {
                theme::toggle(ui, &mut mgr.settings_form.hub_start_menu_shortcut);
            },
        );

        if mgr.settings_form.hub_start_menu_shortcut {
            theme::divider(ui);
            theme::setting_row(
                ui,
                "Start Menu category",
                Some("Application menu category for AppImage Hub."),
                |ui| {
                    let selected_label = crate::ui::side_panel::KDE_CATEGORIES
                        .iter()
                        .find(|(_, value)| *value == mgr.settings_form.hub_start_menu_category)
                        .map(|(label, _)| *label)
                        .unwrap_or("Utilities");
                    egui::ComboBox::from_id_salt("hub_start_menu_category")
                        .selected_text(selected_label)
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            let mut categories = crate::ui::side_panel::KDE_CATEGORIES.to_vec();
                            categories.sort_unstable_by(|a, b| a.0.cmp(b.0));
                            for (label, value) in categories {
                                let selected = mgr.settings_form.hub_start_menu_category == value;
                                if ui.selectable_label(selected, label).clicked() {
                                    mgr.settings_form.hub_start_menu_category = value.to_string();
                                }
                            }
                        });
                },
            );
        }

        theme::divider(ui);
        theme::setting_row(
            ui,
            "Desktop shortcut",
            Some("Place an AppImage Hub launcher on your Desktop."),
            |ui| {
                theme::toggle(ui, &mut mgr.settings_form.hub_desktop_shortcut);
            },
        );
    });
}

fn render_github_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("GitHub API Access"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        ui.label(theme::field_label("Personal access token (optional)"));
        ui.add_space(6.0);
        ui.add_sized(
            [ui.available_width(), 38.0],
            egui::TextEdit::singleline(&mut mgr.settings_form.github_token)
                .font(fonts::mono(12.5))
                .hint_text("ghp_…")
                .password(!mgr.settings_form.show_token)
                .margin(Margin::symmetric(12, 9))
                .vertical_align(Align::Center),
        );
        ui.add_space(theme::SPACE_SM);
        theme::setting_row(
            ui,
            "Show token while editing",
            Some("Raises the GitHub API rate limit from 60 to 5,000 requests per hour."),
            |ui| {
                theme::toggle(ui, &mut mgr.settings_form.show_token);
            },
        );
        theme::divider(ui);
        ui.add_space(theme::SPACE_SM);
        ui.add(
            egui::Label::new(
                theme::muted_label(
                    "Stored in the system keyring when available; otherwise encrypted with \
                     Argon2id in a 0600 file under your config folder. Never written to \
                     config.toml or beside a portable AppImage.",
                )
                .font(fonts::regular(12.0)),
            )
            .wrap(),
        );
        ui.add_space(theme::SPACE_XS);
    });
}

fn render_about_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("About AppImage Hub"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../assets/logo.png"))
                    .fit_to_exact_size(vec2(52.0, 52.0))
                    .corner_radius(CornerRadius::same(13)),
            );
            ui.add_space(theme::SPACE_MD);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("AppImage Hub")
                        .font(fonts::bold(20.0))
                        .color(theme::text_primary()),
                );
                ui.label(
                    RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                        .font(fonts::mono(12.0))
                        .color(theme::text_secondary()),
                );
                ui.label(
                    theme::muted_label("Modern desktop manager for Linux AppImages.")
                        .font(fonts::regular(12.0)),
                );
            });
        });

        ui.add_space(theme::SPACE_MD);
        theme::divider(ui);
        theme::setting_row(
            ui,
            "Managed apps",
            Some("Total AppImages currently tracked in your library."),
            |ui| {
                theme::version_pill(
                    ui,
                    &mgr.app_count().to_string(),
                    theme::text_primary(),
                    theme::current().neutral_soft,
                );
            },
        );
        theme::divider(ui);
        theme::setting_row(
            ui,
            "Config file",
            Some("Location where your library and theme settings are saved."),
            |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::config::Config::config_path().display().to_string())
                            .font(fonts::mono(11.0))
                            .color(theme::text_muted()),
                    )
                    .truncate(),
                );
            },
        );
    });
}
