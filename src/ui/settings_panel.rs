//! Full-page responsive Settings screen styled after `ytm-rs`.
//!
//! Layout:
//! - Large / Medium viewports: Left category rail + right scrollable grouped cards.
//! - Narrow / Tight viewports: Horizontal wrapped category pills + full-width cards.
//!
//! Theme changes apply live as the user edits them; clicking Cancel reverts to
//! the saved theme in `Config`.

use crate::app::ManagerApp;
use crate::config::{ThemeMode, ThemeSettings};
use crate::ui::breakpoints::SizeClass;
use crate::ui::{colorpick, theme};
use eframe::egui::{self, Align, Color32, CornerRadius, Margin, RichText, Sense, Stroke, pos2, vec2};

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

/// Renders the full-screen responsive Settings view inside the central area.
pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp, size_class: SizeClass) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        mgr.close_settings(ui.ctx());
        return;
    }

    let compact = matches!(size_class, SizeClass::Narrow | SizeClass::Tight);

    // ── Header row ────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui
            .add(theme::secondary_button("← Back"))
            .on_hover_text("Return to AppImages (Esc)")
            .clicked()
        {
            mgr.close_settings(ui.ctx());
            return;
        }
        ui.add_space(theme::SPACE_SM);
        ui.label(
            RichText::new("Settings")
                .size(26.0)
                .strong()
                .color(theme::text_primary()),
        );
        if !compact {
            ui.add_space(theme::SPACE_SM);
            ui.label(
                theme::secondary_label("Customize theme, detail colors, storage, and updates")
                    .size(13.0),
            );
        }
    });

    ui.add_space(theme::SPACE_MD);
    theme::divider(ui);
    ui.add_space(theme::SPACE_MD);

    // Reserve space for the bottom Save / Cancel action bar.
    let footer_h = 56.0;
    let body_h = (ui.available_height() - footer_h).max(140.0);

    let mut theme_changed = false;

    if compact {
        // Horizontal category pills on Narrow / Tight widths
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for tab in SettingsTab::ALL {
                let active = mgr.settings_form.active_tab == tab;
                let btn = if active {
                    theme::primary_button(tab.label()).min_size(vec2(72.0, 30.0))
                } else {
                    theme::ghost_button(tab.label()).min_size(vec2(72.0, 30.0))
                };
                if ui.add(btn).clicked() {
                    mgr.settings_form.active_tab = tab;
                }
            }
        });
        ui.add_space(theme::SPACE_MD);

        let scroll_h = (body_h - 46.0).max(100.0);
        egui::ScrollArea::vertical()
            .id_salt("settings_content_compact")
            .auto_shrink([false, false])
            .max_height(scroll_h)
            .show(ui, |ui| {
                theme_changed = render_tab_content(ui, mgr);
                ui.add_space(theme::SPACE_LG);
            });
    } else {
        // Two-column ytm-rs Settings layout: Left category list + right content cards
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), body_h),
            egui::Layout::left_to_right(Align::Min),
            |ui| {
                let nav_w = 196.0;
                ui.vertical(|ui| {
                    ui.set_width(nav_w);
                    ui.label(theme::section_heading("Categories"));
                    ui.add_space(theme::SPACE_SM);
                    for tab in SettingsTab::ALL {
                        let active = mgr.settings_form.active_tab == tab;
                        if theme::nav_item_button(ui, tab.label(), None, active).clicked() {
                            mgr.settings_form.active_tab = tab;
                        }
                        ui.add_space(2.0);
                    }
                });

                ui.add_space(theme::SPACE_XL);

                let content_w = ui.available_width().clamp(260.0, 740.0);
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

    // ── Bottom sticky action bar ──────────────────────────────────────────
    theme::divider(ui);
    ui.add_space(theme::SPACE_SM);
    ui.horizontal(|ui| {
        if ui
            .add_sized([148.0, 38.0], theme::primary_button("Save Settings"))
            .clicked()
        {
            mgr.save_settings(ui.ctx());
        }
        ui.add_space(theme::SPACE_SM);
        if ui
            .add_sized([96.0, 38.0], theme::ghost_button("Cancel"))
            .clicked()
        {
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

    ui.label(theme::section_heading("Base Theme"));
    ui.add_space(theme::SPACE_SM);

    // Visual Dark / Light mode selector cards
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

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Detail Personalization"));
    ui.add_space(theme::SPACE_SM);

    theme::settings_card(ui, |ui| {
        // Accent Color row (swatches + custom HSV picker)
        theme::setting_row(
            ui,
            "Accent color",
            Some("Used for primary buttons, toggles, active navigation, and progress bars."),
            |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                    if colorpick::swatch_button(
                        ui,
                        "accent_custom_picker",
                        &mut mgr.settings_form.theme.accent_rgb,
                    ) {
                        changed = true;
                    }
                    ui.label(
                        theme::secondary_label(colorpick::to_hex(colorpick::rgb_to_color(
                            mgr.settings_form.theme.accent_rgb,
                        )))
                        .size(12.0),
                    );
                });
            },
        );

        // Curated accent preset dots
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
                        Stroke::new(1.5_f32, theme::with_alpha(theme::text_primary(), 140)),
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

        // Card / Surface Tint detail
        theme::setting_row(
            ui,
            "Card & surface tint",
            Some("Subtly tint app cards and panels while keeping Dark or Light readability."),
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

        // Border & Divider Detail Color
        theme::setting_row(
            ui,
            "Border & divider color",
            Some("Customize outlines around cards, inputs, and menus."),
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

        // Highlight / Badge Detail Color
        theme::setting_row(
            ui,
            "Badge & highlight detail",
            Some("Color for update badges, version tags, and fallback app initials."),
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

        // Corner Roundness
        theme::setting_row(
            ui,
            "Corner roundness",
            Some("Adjust the curvature of cards, panels, and inputs."),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (label, r) in [("Sharp", 6_u8), ("Balanced", 12_u8), ("Soft", 16_u8)] {
                        let active = mgr.settings_form.theme.corner_radius == r;
                        let btn = if active {
                            theme::primary_button(label).min_size(vec2(68.0, 28.0))
                        } else {
                            theme::ghost_button(label).min_size(vec2(68.0, 28.0))
                        };
                        if ui.add(btn).clicked() {
                            mgr.settings_form.theme.corner_radius = r;
                            changed = true;
                        }
                    }
                });
            },
        );
        theme::divider(ui);

        // Reset button row
        theme::setting_row(
            ui,
            "Reset appearance",
            Some("Restore the default ytm-rs colors and corner roundness for the active mode."),
            |ui| {
                if ui.add(theme::secondary_button("Reset Defaults")).clicked() {
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

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Live Preview"));
    ui.add_space(theme::SPACE_SM);

    // Mini preview card showing how current theme + details look together
    theme::card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            let (icon_r, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            let cr = CornerRadius::same(theme::radius_md());
            ui.painter()
                .rect_filled(icon_r, cr, theme::surface_raised());
            ui.painter()
                .rect_stroke(icon_r, cr, Stroke::new(1.0_f32, theme::border()), egui::StrokeKind::Inside);
            ui.painter().text(
                icon_r.center(),
                egui::Align2::CENTER_CENTER,
                "A",
                egui::FontId::proportional(20.0),
                theme::detail(),
            );
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Sample AppImage")
                        .size(15.0)
                        .strong()
                        .color(theme::text_primary()),
                );
                ui.label(theme::muted_label("owner/sample-app • v1.4.2").size(12.0));
            });
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                theme::badge_sized(
                    ui,
                    "Update available",
                    theme::BadgeKind::Accent,
                    vec2(114.0, 24.0),
                );
                ui.add_space(6.0);
                let _ = ui.add(theme::primary_button("Launch"));
            });
        });
    });

    changed
}

fn theme_mode_card(ui: &mut egui::Ui, mode: ThemeMode, selected: bool, width: f32) -> bool {
    let height = 96.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let r = CornerRadius::same(theme::radius_lg());

    let (preview_bg, preview_side, preview_card, preview_text) = match mode {
        ThemeMode::Dark => (
            Color32::from_rgb(0x0C, 0x0A, 0x0E),
            Color32::from_rgb(0x16, 0x13, 0x1B),
            Color32::from_rgb(0x22, 0x1E, 0x28),
            Color32::from_rgb(0xF2, 0xF0, 0xF5),
        ),
        ThemeMode::Light => (
            Color32::from_rgb(0xF6, 0xF4, 0xF8),
            Color32::from_rgb(0xEB, 0xE7, 0xF0),
            Color32::from_rgb(0xFF, 0xFF, 0xFF),
            Color32::from_rgb(0x16, 0x14, 0x1A),
        ),
    };

    let p = ui.painter();
    p.rect_filled(rect, r, preview_bg);

    // Mini sidebar strip
    let side = egui::Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(32.0, height - 16.0));
    p.rect_filled(side, CornerRadius::same(6), preview_side);
    // Mini accent pill inside sidebar
    let pill = egui::Rect::from_min_size(side.min + vec2(5.0, 10.0), vec2(22.0, 6.0));
    p.rect_filled(pill, CornerRadius::same(3), theme::accent());

    // Mini cards on right
    let c1 = egui::Rect::from_min_max(
        pos2(side.right() + 8.0, rect.top() + 10.0),
        pos2(rect.right() - 10.0, rect.top() + 36.0),
    );
    p.rect_filled(c1, CornerRadius::same(6), preview_card);
    let c2 = egui::Rect::from_min_max(
        pos2(side.right() + 8.0, c1.bottom() + 6.0),
        pos2(rect.right() - 10.0, rect.bottom() - 28.0),
    );
    p.rect_filled(c2, CornerRadius::same(6), preview_card);

    // Mode title bottom-right
    p.text(
        pos2(side.right() + 10.0, rect.bottom() - 14.0),
        egui::Align2::LEFT_CENTER,
        mode.label(),
        egui::FontId::proportional(13.5),
        preview_text,
    );

    let stroke = if selected {
        Stroke::new(2.0_f32, theme::accent())
    } else if resp.hovered() {
        Stroke::new(1.5_f32, theme::with_alpha(theme::accent(), 140))
    } else {
        Stroke::new(1.0_f32, theme::border())
    };
    p.rect_stroke(rect, r, stroke, egui::StrokeKind::Inside);

    resp.clicked()
}

fn render_storage_tab(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("AppImage Storage"));
    ui.add_space(theme::SPACE_SM);
    theme::settings_card(ui, |ui| {
        ui.label(theme::field_label("AppImage root folder"));
        ui.add_space(theme::SPACE_XS);
        ui.horizontal(|ui| {
            let browse_w = 96.0;
            let spacing = ui.spacing().item_spacing.x;
            let input_w = (ui.available_width() - browse_w - spacing).max(120.0);
            ui.add_sized(
                [input_w, 40.0],
                egui::TextEdit::singleline(&mut mgr.settings_form.apps_dir)
                    .hint_text("Folder where AppImages are stored")
                    .margin(Margin::symmetric(12, 9))
                    .vertical_align(Align::Center)
                    .desired_width(input_w),
            );
            if ui
                .add_sized([browse_w, 40.0], theme::secondary_button("Browse"))
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
        ui.add_space(theme::SPACE_XS);
        ui.label(
            theme::muted_label(
                "New AppImages are stored in per-app folders inside this directory.",
            )
            .size(12.0),
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
            Some("Automatically check GitHub releases for managed apps when AppImage Hub starts."),
            |ui| {
                theme::toggle(ui, &mut mgr.settings_form.auto_check_updates);
            },
        );
        theme::divider(ui);
        let cooldown_text = if mgr.update_check_cooldown() <= 2 * 60 {
            "Rate-limited to once every 2 minutes (GitHub token configured)."
        } else {
            "Rate-limited to once every 5 minutes. Add a GitHub token to shorten it to 2 minutes."
        };
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::muted_label(cooldown_text).size(12.0));
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
            Some("Create and keep AppImage Hub's own application menu entry up to date."),
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
        ui.add_space(theme::SPACE_XS);
        ui.add_sized(
            [ui.available_width(), 40.0],
            egui::TextEdit::singleline(&mut mgr.settings_form.github_token)
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
                .size(12.0),
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
                    .corner_radius(CornerRadius::same(12)),
            );
            ui.add_space(theme::SPACE_MD);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("AppImage Hub")
                        .size(20.0)
                        .strong()
                        .color(theme::text_primary()),
                );
                ui.label(
                    theme::secondary_label(concat!("Version ", env!("CARGO_PKG_VERSION")))
                        .size(13.0),
                );
                ui.label(
                    theme::muted_label("Native desktop manager for Linux AppImages.").size(12.0),
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
                ui.label(
                    RichText::new(mgr.app_count().to_string())
                        .size(14.0)
                        .strong()
                        .color(theme::text_primary()),
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
                        theme::muted_label(crate::config::Config::config_path().display().to_string())
                            .size(12.0),
                    )
                    .truncate(),
                );
            },
        );
    });
}
