//! Modern integrated Canvas Hero Header (replaces the old chunky top toolbar).
//!
//! Combines page title, inline search pill, Grid/List switcher, Sort popover,
//! animated Sync button, and primary `+ Add App` CTA — collapsing gracefully
//! on Narrow and Tight viewports.

use super::*;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, ButtonTone};
use crate::ui::fonts;
use eframe::egui::{Align, CornerRadius, Layout, Margin, RichText, Stroke, vec2};

impl ManagerApp {
    pub(crate) fn render_canvas_header(
        &mut self,
        ui: &mut egui::Ui,
        size_class: SizeClass,
        has_sidebar: bool,
    ) {
        let compact = matches!(size_class, SizeClass::Narrow | SizeClass::Tight);
        let tight = matches!(size_class, SizeClass::Tight);

        // Compact brand & quick utilities row when sidebar is hidden (< 900 px)
        if !has_sidebar {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::new(egui::include_image!("../../assets/logo.png"))
                        .fit_to_exact_size(vec2(26.0, 26.0))
                        .corner_radius(CornerRadius::same(7)),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(if tight { "Hub" } else { "AppImage Hub" })
                        .font(fonts::bold(17.0))
                        .color(theme::text_primary()),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if theme::pill_action(ui, Some(Icon::Plus), "Add", ButtonTone::Primary, 30.0)
                        .clicked()
                    {
                        self.open_add_form();
                    }
                    if theme::icon_button(ui, Icon::Settings, 30.0, false, "Settings (Ctrl+,)")
                        .clicked()
                    {
                        self.open_settings(ui.ctx());
                    }
                    let (theme_icon, theme_tip) = if theme::is_light() {
                        (Icon::Moon, "Switch to Dark theme")
                    } else {
                        (Icon::Sun, "Switch to Light theme")
                    };
                    if theme::icon_button(ui, theme_icon, 30.0, false, theme_tip).clicked() {
                        self.toggle_theme_mode(ui.ctx());
                    }
                    if theme::icon_button(
                        ui,
                        Icon::Command,
                        30.0,
                        false,
                        "Command Palette (Ctrl+K)",
                    )
                    .clicked()
                    {
                        self.palette.toggle();
                    }
                });
            });

            ui.add_space(theme::SPACE_MD);

            // Horizontally wrapped filter pills on Narrow/Tight
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let updates_count = self.updates_available_count();
                for filter in NavFilter::ALL {
                    let active = self.nav_filter == filter;
                    let label = if filter == NavFilter::Updates && updates_count > 0 {
                        format!("Updates ({updates_count})")
                    } else {
                        filter.label().to_string()
                    };
                    let tone = if active {
                        ButtonTone::SubtleAccent
                    } else {
                        ButtonTone::Ghost
                    };
                    if theme::pill_action(ui, None, &label, tone, 28.0).clicked() {
                        self.nav_filter = filter;
                    }
                }
            });

            ui.add_space(theme::SPACE_SM);
        }

        // Main Hero Header row (Title on left, Search + Controls + CTA on right)
        if compact {
            ui.horizontal(|ui| {
                self.render_search_pill(ui, (ui.available_width() - 122.0).max(110.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    self.render_sort_button(ui);
                    self.render_view_toggle(ui);
                    self.render_sync_icon_button(ui);
                });
            });
        } else {
            ui.horizontal(|ui| {
                let page_title = match self.nav_filter {
                    NavFilter::All => "All Apps",
                    NavFilter::Updates => "Updates Available",
                    NavFilter::Github => "GitHub Apps",
                    NavFilter::Local => "Local AppImages",
                };
                ui.label(
                    RichText::new(page_title)
                        .font(fonts::bold(24.0))
                        .color(theme::text_primary()),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;

                    if theme::pill_action(
                        ui,
                        Some(Icon::Plus),
                        "Add App",
                        ButtonTone::Primary,
                        34.0,
                    )
                    .on_hover_text("Add or install an AppImage (Ctrl+N)")
                    .clicked()
                    {
                        self.open_add_form();
                    }

                    self.render_sync_icon_button(ui);
                    self.render_sort_button(ui);
                    self.render_view_toggle(ui);

                    let search_w = if size_class == SizeClass::Large {
                        240.0
                    } else {
                        175.0
                    };
                    self.render_search_pill(ui, search_w);
                });
            });
        }
    }

    fn render_search_pill(&mut self, ui: &mut egui::Ui, width: f32) {
        let p = theme::current();
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
            .inner_margin(Margin::symmetric(12, 0))
            .show(ui, |ui| {
                ui.allocate_ui_with_layout(
                    vec2((width - 24.0).max(80.0), 34.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        icons::widget(ui, Icon::Search, 14.0, p.text_muted);
                        let clear_w: f32 = if self.search_query.is_empty() { 0.0 } else { 20.0 };
                        ui.add_sized(
                            [(width - 54.0 - clear_w).max(52.0), 22.0],
                            egui::TextEdit::singleline(&mut self.search_query)
                                .frame(false)
                                .vertical_align(Align::Center)
                                .font(fonts::regular(13.0))
                                .hint_text(
                                    RichText::new("Filter apps…")
                                        .font(fonts::regular(13.0))
                                        .color(p.text_muted),
                                ),
                        );
                        if !self.search_query.is_empty()
                            && theme::icon_button(ui, Icon::Close, 18.0, false, "Clear filter")
                                .clicked()
                        {
                            self.search_query.clear();
                        }
                    },
                );
            });
    }

    fn render_view_toggle(&mut self, ui: &mut egui::Ui) {
        let p = theme::current();
        egui::Frame::new()
            .fill(p.surface)
            .stroke(Stroke::new(1.0_f32, p.border))
            .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
            .inner_margin(Margin::same(3))
            .show(ui, |ui| {
                ui.allocate_ui_with_layout(
                    vec2(58.0, 28.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if theme::icon_button(
                            ui,
                            Icon::LayoutGrid,
                            28.0,
                            self.view_mode == ViewMode::Grid,
                            "Grid view",
                        )
                        .clicked()
                        {
                            self.view_mode = ViewMode::Grid;
                        }
                        if theme::icon_button(
                            ui,
                            Icon::LayoutList,
                            28.0,
                            self.view_mode == ViewMode::List,
                            "List view",
                        )
                        .clicked()
                        {
                            self.view_mode = ViewMode::List;
                        }
                    },
                );
            });
    }

    fn render_sort_button(&mut self, ui: &mut egui::Ui) {
        let popup_id = ui.make_persistent_id("header_sort_popover");
        let is_open = ui.memory(|m| m.is_popup_open(popup_id));
        let tip = format!("Sort by: {}", self.sort_by.label());
        let p = theme::current();
        let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::click());
        let resp = resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip);
        let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());
        let cr = CornerRadius::same(theme::RADIUS_PILL);
        let fill = if is_open {
            p.accent_soft
        } else {
            theme::lerp_color(p.surface, p.surface_hover, hover)
        };
        let fg = if is_open {
            p.accent
        } else {
            theme::lerp_color(p.text_secondary, p.text_primary, hover)
        };
        ui.painter().rect_filled(rect, cr, fill);
        ui.painter().rect_stroke(
            rect,
            cr,
            Stroke::new(1.0_f32, if is_open { theme::with_alpha(p.accent, 90) } else { p.border }),
            egui::StrokeKind::Inside,
        );
        icons::paint(ui.painter(), Icon::Sliders, rect.center(), 15.0, fg);
        if resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }

        egui::popup_below_widget(
            ui,
            popup_id,
            &resp,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| {
                ui.set_width(152.0);
                ui.label(theme::section_heading("Sort By"));
                ui.add_space(4.0);
                for sort in AppSortBy::ALL {
                    let active = self.sort_by == sort;
                    let icon = if active { Icon::Check } else { Icon::Sliders };
                    if theme::popover_item(ui, icon, sort.label(), false).clicked() {
                        self.sort_by = sort;
                        ui.memory_mut(|m| m.close_popup());
                    }
                }
            },
        );
    }

    fn render_sync_icon_button(&mut self, ui: &mut egui::Ui) {
        let checking = self.update_check.is_some();
        let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), egui::Sense::click());
        let tip = if let Some(prog) = &self.update_check {
            format!("Checking updates ({}%)…", (prog.fraction() * 100.0).round() as i32)
        } else {
            "Check all apps for updates".to_string()
        };
        let resp = resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(tip);
        let p = theme::current();
        let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered() && !checking);
        let cr = CornerRadius::same(theme::RADIUS_PILL);
        let fill = if checking {
            p.accent_soft
        } else {
            theme::lerp_color(p.surface, p.surface_hover, hover)
        };
        ui.painter().rect_filled(rect, cr, fill);
        ui.painter().rect_stroke(
            rect,
            cr,
            Stroke::new(
                1.0_f32,
                if checking {
                    theme::with_alpha(p.accent, 90)
                } else {
                    p.border
                },
            ),
            egui::StrokeKind::Inside,
        );

        let angle = if checking {
            ui.ctx().request_repaint();
            (ui.input(|i| i.time) as f32) * 3.8
        } else {
            0.0
        };
        let fg = if checking {
            p.accent
        } else {
            theme::lerp_color(p.text_secondary, p.text_primary, hover)
        };
        icons::paint_rotated(ui.painter(), Icon::Refresh, rect.center(), 15.0, fg, angle);

        if resp.clicked() && !checking {
            self.check_all_updates();
        }
    }
}
