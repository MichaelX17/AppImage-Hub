//! Spatial shell & per-frame orchestration (`eframe::App`).
//!
//! Architecture:
//! - Left inset sidebar on Large/Medium viewports (brand, instant Dark/Light toggle,
//!   Command Palette trigger, icon navigation, bottom Settings & telemetry).
//! - Unified main canvas with subtle top ambient accent lighting, integrated Hero Header,
//!   and Bento Grid / Linear List view (or full-page Settings screen).
//! - Floating slide-over drawer for Add/Edit/Shortcuts over a soft dimmed veil.
//! - Floating Activity Dock, Floating Toasts, Confirm Modals, and `Ctrl+K` Command Palette.

use super::*;
use crate::ui::icons::{self, Icon};
use crate::ui::{fonts, theme};
use eframe::egui::{Color32, CornerRadius, Key, Layout, Margin, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

impl eframe::App for ManagerApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let [r, g, b, _] = theme::bg().to_normalized_gamma_f32();
        [r, g, b, 1.0]
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("side_panel_width", self.side_panel_width.to_string());
        storage.set_string("show_side_panel", self.show_side_panel.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_background_startup();

        if self.first_run_setup {
            self.render_first_run_setup(ctx);
            return;
        }

        let size_class = SizeClass::from_width(ctx.screen_rect().width());
        while let Ok(evt) = self.evt_rx.try_recv() {
            self.apply_event(evt);
        }
        if self.refresh_apps_snapshot() {
            self.request_file_scan();
        }
        self.poll_file_scan();
        self.tick_update_check(ctx);

        // Wait until background token load completes before running startup
        // update checks so authenticated requests and the 1-minute cooldown apply.
        if self.startup_init_done && !self.auto_check_done && self.config_warning.is_none() {
            self.auto_check_done = true;
            if self.auto_check_enabled() {
                self.check_all_updates();
            }
        }
        if self.startup_init_done
            && !self.hub_update_check_started
            && self.config_warning.is_none()
        {
            self.hub_update_check_started = true;
            self.check_hub_update_if_allowed();
        }

        // Global keyboard shortcuts (Ctrl+K palette, Ctrl+N add, Ctrl+, settings)
        self.handle_global_shortcuts(ctx);

        let show_left_sidebar = matches!(size_class, SizeClass::Large | SizeClass::Medium);

        // ── Left Inset Sidebar ───────────────────────────────────────────
        if show_left_sidebar {
            let sidebar_w = if size_class == SizeClass::Large {
                232.0
            } else {
                198.0
            };
            egui::SidePanel::left("nav_sidebar")
                .resizable(false)
                .exact_width(sidebar_w)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::sidebar_bg())
                        .stroke(Stroke::new(1.0_f32, theme::divider_color()))
                        .inner_margin(Margin::symmetric(14, 18)),
                )
                .show(ctx, |ui| {
                    self.render_left_sidebar(ui);
                });
        }

        // ── Unified Main Canvas ──────────────────────────────────────────
        let pad = if matches!(size_class, SizeClass::Tight) {
            14
        } else {
            24
        };
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::bg())
                    .inner_margin(Margin::same(pad)),
            )
            .show(ctx, |ui| {
                // Subtle ambient accent glow across the top of the canvas
                let full_rect = ui.max_rect().expand(pad as f32);
                theme::paint_canvas_ambient(ui, full_rect);

                if self.show_settings {
                    crate::ui::settings_panel::render(ui, self, size_class);
                } else {
                    self.render_canvas_header(ui, size_class, show_left_sidebar);
                    ui.add_space(theme::SPACE_LG);
                    let apps = Arc::clone(&self.snapshot.apps);
                    let meta = Arc::clone(&self.snapshot.meta);
                    crate::ui::app_list::render(ui, &apps, &meta, self, size_class);
                }
            });

        // ── Floating Slide-Over Inspector Drawer (Add / Edit / Shortcuts) ─
        let drawer_open = self.is_right_panel_open() && self.show_side_panel;
        if drawer_open {
            self.render_floating_drawer(ctx, size_class);
        }

        // ── Floating Activity Dock, Toasts, Modals & Command Palette ─────
        crate::ui::update_state::render(ctx, self, size_class);
        crate::ui::confirm_dialog::render(ctx, self);
        crate::ui::palette::render(ctx, self);

        let activity_items = self.ops.len() + self.failed_ops.len();
        if activity_items != self.activity_footer_items {
            self.activity_footer_items = activity_items;
            ctx.request_repaint();
        }
        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if self.update_check.is_some() || self.hub_update_progress.is_some() {
            ctx.request_repaint();
        }
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

impl ManagerApp {
    fn handle_global_shortcuts(&mut self, ctx: &egui::Context) {
        let (cmd_k, cmd_n, cmd_comma) = ctx.input(|i| {
            let m = i.modifiers.command;
            (
                m && i.key_pressed(Key::K),
                m && i.key_pressed(Key::N),
                m && i.key_pressed(Key::Comma),
            )
        });

        if cmd_k {
            self.palette.toggle();
        } else if cmd_n && !self.palette.open {
            if self.show_settings {
                self.close_settings(ctx);
            }
            self.open_add_form();
        } else if cmd_comma && !self.palette.open {
            if self.show_settings {
                self.close_settings(ctx);
            } else {
                self.open_settings(ctx);
            }
        }
    }

    fn render_left_sidebar(&mut self, ui: &mut egui::Ui) {
        // Brand row + instant Dark/Light switcher
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../../assets/logo.png"))
                    .fit_to_exact_size(vec2(28.0, 28.0))
                    .corner_radius(CornerRadius::same(8)),
            );
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    RichText::new("AppImage Hub")
                        .font(fonts::bold(14.5))
                        .color(theme::text_primary()),
                );
                ui.label(
                    RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                        .font(fonts::mono(10.0))
                        .color(theme::text_muted()),
                );
            });
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let (icon, tip) = if theme::is_light() {
                    (Icon::Moon, "Switch to Dark theme")
                } else {
                    (Icon::Sun, "Switch to Light theme")
                };
                if theme::icon_button(ui, icon, 28.0, false, tip).clicked() {
                    self.toggle_theme_mode(ui.ctx());
                }
            });
        });

        ui.add_space(16.0);

        // Raycast-style Command Palette trigger pill
        let p = theme::current();
        let (cmd_rect, cmd_resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
        let cmd_resp = cmd_resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Open Command Palette (Ctrl+K)");
        let hover = ui.ctx().animate_bool_responsive(cmd_resp.id, cmd_resp.hovered());
        let fill = theme::lerp_color(p.surface, p.surface_hover, hover * 0.7);
        let stroke = theme::lerp_color(p.border, p.border_hover, hover);
        let cr = CornerRadius::same(p.radius_md.max(8));
        ui.painter().rect_filled(cmd_rect, cr, fill);
        ui.painter()
            .rect_stroke(cmd_rect, cr, Stroke::new(1.0_f32, stroke), StrokeKind::Inside);
        icons::paint(
            ui.painter(),
            Icon::Search,
            pos2(cmd_rect.left() + 14.0, cmd_rect.center().y),
            13.5,
            p.text_muted,
        );
        ui.painter().text(
            pos2(cmd_rect.left() + 28.0, cmd_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Quick Launch…",
            fonts::regular(12.0),
            p.text_muted,
        );
        // Subtle ⌘K / ^K badge on right
        let k_rect = Rect::from_center_size(
            pos2(cmd_rect.right() - 22.0, cmd_rect.center().y),
            vec2(32.0, 18.0),
        );
        ui.painter()
            .rect_filled(k_rect, CornerRadius::same(5), p.neutral_soft);
        ui.painter().text(
            k_rect.center(),
            egui::Align2::CENTER_CENTER,
            "Ctrl K",
            fonts::mono(9.0),
            p.text_muted,
        );
        if cmd_resp.clicked() {
            self.palette.toggle();
        }

        ui.add_space(20.0);
        ui.label(theme::section_heading("Library"));
        ui.add_space(6.0);

        let total_apps = self.app_count();
        let updates_count = self.updates_available_count();
        let nav_specs = [
            (NavFilter::All, Icon::AppsGrid, Some((total_apps, false))),
            (NavFilter::Updates, Icon::Updates, Some((updates_count, true))),
            (NavFilter::Github, Icon::GitBranch, None),
            (NavFilter::Local, Icon::HardDrive, None),
        ];

        for (filter, icon, badge) in nav_specs {
            let active = !self.show_settings && self.nav_filter == filter;
            if theme::nav_item(ui, icon, filter.label(), badge, active).clicked() {
                if self.show_settings {
                    self.close_settings(ui.ctx());
                }
                self.nav_filter = filter;
            }
            ui.add_space(2.0);
        }

        // Bottom-anchored Settings + subtle telemetry
        ui.with_layout(Layout::bottom_up(egui::Align::Min), |ui| {
            let checked_text = match self.last_checked {
                Some(when) => format!("Checked {}", crate::ui::format::format_relative_time(when)),
                None => "Not checked yet".to_string(),
            };
            ui.label(
                RichText::new(checked_text)
                    .font(fonts::regular(11.0))
                    .color(theme::text_muted()),
            );
            ui.add_space(6.0);
            if theme::nav_item(ui, Icon::Settings, "Settings", None, self.show_settings).clicked() {
                if self.show_settings {
                    self.close_settings(ui.ctx());
                } else {
                    self.open_settings(ui.ctx());
                }
            }
            ui.add_space(6.0);
            theme::divider(ui);
        });
    }

    fn render_floating_drawer(&mut self, ctx: &egui::Context, size_class: SizeClass) {
        let screen = ctx.screen_rect();
        let compact = matches!(size_class, SizeClass::Narrow | SizeClass::Tight);

        // Soft dimmed backdrop (click outside to dismiss)
        let backdrop = egui::Area::new(egui::Id::new("drawer_backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                let alpha = if theme::is_light() { 65 } else { 125 };
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(alpha));
                resp
            });
        if backdrop.inner.clicked() {
            self.is_form_open = false;
            self.add_to = None;
            self.show_side_panel = false;
            return;
        }

        let margin = if compact { 12.0 } else { 18.0 };
        let drawer_w = if compact {
            (screen.width() - margin * 2.0).max(280.0)
        } else {
            (screen.width() * 0.42).clamp(400.0, 500.0)
        };
        let drawer_h = (screen.height() - margin * 2.0).max(300.0);
        let pos = pos2(screen.right() - drawer_w - margin, screen.top() + margin);

        egui::Area::new(egui::Id::new("floating_inspector_drawer"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                ui.set_min_size(vec2(drawer_w, drawer_h));
                ui.set_max_size(vec2(drawer_w, drawer_h));
                if self.add_to.is_some() {
                    crate::ui::side_panel::render_add_to(ui, self, size_class);
                } else if self.is_form_open {
                    crate::ui::side_panel::render(ui, self, size_class);
                }
            });
    }
}
