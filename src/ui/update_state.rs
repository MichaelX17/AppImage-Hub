//! Bottom status footer: app count, "last checked", auto-check toggle, plus the
//! activity / failed-operation list when work is in flight.

use crate::app::{AppSortBy, ManagerApp, OpKind};
use crate::ui::breakpoints::SizeClass;
use crate::ui::format;
use crate::ui::theme;
use eframe::egui::{self, Align, CornerRadius, Layout, Margin, RichText, Sense, Stroke};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp, size_class: SizeClass) {
    let app_count = mgr.config.lock().map(|c| c.applications.len()).unwrap_or(0);
    let mut auto_check = mgr.auto_check_enabled();

    egui::TopBottomPanel::bottom("status_footer")
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .stroke(Stroke::new(1.0_f32, theme::BORDER))
                .inner_margin(Margin::symmetric(
                    theme::SPACE_LG as i8,
                    theme::SPACE_SM as i8,
                )),
        )
        .show(ctx, |ui| {
            // Activity first so the status row always sits at the very bottom.
            if !mgr.ops.is_empty() || !mgr.failed_ops.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("activity_footer")
                    .max_height(footer_max_height(ctx, size_class))
                    .show(ui, |ui| {
                        render_activity(ui, mgr, size_class);
                    });
                ui.add_space(theme::SPACE_XS);
                ui.separator();
                ui.add_space(theme::SPACE_XS);
            }

            render_status_bar(ui, mgr, app_count, &mut auto_check, size_class);
        });

    if auto_check != mgr.auto_check_enabled() {
        mgr.set_auto_check_enabled(auto_check);
    }

    // Keep "Last checked: 2 minutes ago" fresh while idle.
    if mgr.last_checked.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }
}

fn render_activity(ui: &mut egui::Ui, mgr: &mut ManagerApp, size_class: SizeClass) {
    if !mgr.ops.is_empty() {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Activity")
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            ui.label(theme::muted_label(format!("{} in progress", mgr.ops.len())));
        });
        ui.add_space(theme::SPACE_XS);

        let mut ops: Vec<_> = mgr.ops.values().cloned().collect();
        ops.sort_by_key(|o| o.started_at);
        let mut cancel_id: Option<String> = None;
        let active_limit = if size_class == SizeClass::Large {
            usize::MAX
        } else {
            5
        };
        let active_visible = ops.len().min(active_limit);
        let render_op = |ui: &mut egui::Ui,
                         op: &crate::app::ActiveOp,
                         cancel_id: &mut Option<String>| {
            activity_frame(ui, |ui| {
                let w = ui.available_width();
                if w.is_finite() && w > 0.0 {
                    ui.set_width(w);
                }
                ui.horizontal(|ui| {
                    let kind = match op.kind {
                        OpKind::Install => "Installing",
                        OpKind::Update => "Updating",
                    };
                    ui.label(
                        RichText::new(format!("{} • {}", kind, op.label))
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(theme::secondary_button("Cancel")).clicked() {
                            *cancel_id = Some(op.op_id.clone());
                        }
                    });
                });
                let fraction = if op.fraction.is_finite() {
                    op.fraction.clamp(0.0, 1.0)
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .show_percentage()
                        .fill(theme::ACCENT),
                );
                let progress = format::format_progress(op.downloaded, op.total);
                let eta = format::format_eta(op.started_at, op.downloaded, op.total);
                ui.label(theme::secondary_label(format!("{} • ETA {}", progress, eta)).size(11.0));
            });
            ui.add_space(theme::SPACE_XS);
        };
        for op in ops.iter().take(active_visible) {
            render_op(ui, op, &mut cancel_id);
        }
        if ops.len() > active_visible {
            ui.collapsing(
                format!("Show {} more…", ops.len() - active_visible),
                |ui| {
                    for op in ops.iter().skip(active_visible) {
                        render_op(ui, op, &mut cancel_id);
                    }
                },
            );
        }
        if let Some(id) = cancel_id {
            mgr.cancel_op(&id);
        }
    }
    if !mgr.failed_ops.is_empty() {
        if !mgr.ops.is_empty() {
            ui.separator();
        }
        ui.label(
            RichText::new("Failed operations")
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_XS);
        let mut to_retry: Option<usize> = None;
        let mut to_dismiss: Option<usize> = None;
        let failed_limit = if size_class == SizeClass::Large {
            usize::MAX
        } else {
            3
        };
        let failed_visible = mgr.failed_ops.len().min(failed_limit);
        let render_failed = |ui: &mut egui::Ui,
                             i: usize,
                             f: &crate::app::FailedOp,
                             to_retry: &mut Option<usize>,
                             to_dismiss: &mut Option<usize>| {
            activity_frame(ui, |ui| {
                let w = ui.available_width();
                if w.is_finite() && w > 0.0 {
                    ui.set_width(w);
                }
                ui.label(
                    RichText::new(format!("{}: {}", f.label, f.message))
                        .color(theme::DANGER)
                        .size(12.0),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(f.retry.is_some(), theme::primary_button("Retry"))
                        .clicked()
                    {
                        *to_retry = Some(i);
                    }
                    if ui.add(theme::secondary_button("Dismiss")).clicked() {
                        *to_dismiss = Some(i);
                    }
                });
            });
            ui.add_space(theme::SPACE_XS);
        };
        for (i, f) in mgr.failed_ops.iter().take(failed_visible).enumerate() {
            render_failed(ui, i, f, &mut to_retry, &mut to_dismiss);
        }
        if mgr.failed_ops.len() > failed_visible {
            ui.collapsing(
                format!("Show {} more…", mgr.failed_ops.len() - failed_visible),
                |ui| {
                    for (offset, f) in mgr.failed_ops.iter().skip(failed_visible).enumerate() {
                        let i = failed_visible + offset;
                        render_failed(ui, i, f, &mut to_retry, &mut to_dismiss);
                    }
                },
            );
        }
        if let Some(i) = to_retry.take() {
            mgr.retry_failed(i);
        } else if let Some(i) = to_dismiss.take() {
            if i < mgr.failed_ops.len() {
                mgr.failed_ops.remove(i);
            }
        }
    }
}

fn activity_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::SURFACE_RAISED)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .corner_radius(CornerRadius::same(theme::RADIUS_MD))
        .inner_margin(Margin::same(theme::SPACE_SM as i8))
        .show(ui, add_contents);
}
/// Status row: `3 AppImages · Last checked: 2 minutes ago   [Auto-check ◉]`
fn render_status_bar(
    ui: &mut egui::Ui,
    mgr: &mut ManagerApp,
    app_count: usize,
    auto_check: &mut bool,
    size_class: SizeClass,
) {
    let count_text = apps_label(app_count);
    let checked_text = match mgr.last_checked {
        Some(when) => format!("Last checked: {}", format::format_relative_time(when)),
        None => "Last checked: never".to_string(),
    };

    if size_class == SizeClass::Tight {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(count_text)
                        .size(12.0)
                        .strong()
                        .color(theme::TEXT_SECONDARY),
                );
                ui.label(theme::muted_label(checked_text).size(12.0));
            });
            ui.horizontal_wrapped(|ui| {
                let _ = toggle_switch(ui, auto_check)
                    .on_hover_text("Check GitHub for new releases on startup");
                ui.label(theme::secondary_label("Auto-check for updates").size(12.0));
                ui.add_space(theme::SPACE_SM);
                sort_selector(ui, mgr);
            });
        });
        return;
    }

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(count_text)
                .size(12.0)
                .strong()
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(theme::SPACE_SM);
        ui.separator();
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::muted_label(checked_text).size(12.0));

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let _ = toggle_switch(ui, auto_check)
                .on_hover_text("Check GitHub for new releases on startup");
            ui.add_space(theme::SPACE_XS);
            ui.label(theme::secondary_label("Auto-check for updates").size(12.0));
            ui.add_space(theme::SPACE_MD);
            sort_selector(ui, mgr);
        });
    });
}

fn sort_selector(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    egui::ComboBox::from_id_salt("app_sort_selector")
        .selected_text(
            RichText::new(format!("Sort: {}", mgr.sort_by.label()))
                .size(12.0)
                .color(theme::TEXT_SECONDARY),
        )
        .width(132.0)
        .show_ui(ui, |ui| {
            for sort_by in AppSortBy::ALL {
                ui.selectable_value(&mut mgr.sort_by, sort_by, sort_by.label());
            }
        });
}

/// Minimal toggle switch: pill track + sliding knob (no bitmap assets).
fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let desired = egui::vec2(38.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(desired, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
        let t = ui.ctx().animate_bool(response.id, *on);
        let track = if *on { theme::ACCENT } else { theme::NEUTRAL };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS_PILL), track);
        if *on {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(theme::RADIUS_PILL),
                Stroke::new(1.0_f32, theme::ACCENT_HOVER),
                egui::StrokeKind::Inside,
            );
        }
        let radius = (rect.height() * 0.5 - 3.0).max(1.0);
        let left = rect.left() + radius + 3.0;
        let right = rect.right() - radius - 3.0;
        let cx = egui::lerp(left..=right, t);
        ui.painter().circle_filled(
            egui::pos2(cx, rect.center().y),
            radius,
            egui::Color32::WHITE,
        );
    }
    response
}

fn apps_label(count: usize) -> String {
    if count == 1 {
        "1 AppImage".to_string()
    } else {
        format!("{count} AppImages")
    }
}

fn footer_max_height(ctx: &egui::Context, size_class: SizeClass) -> f32 {
    let base = (ctx.screen_rect().height() * 0.30).clamp(120.0, 280.0);
    match size_class {
        SizeClass::Tight => base.min(160.0),
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_label_is_plural_aware() {
        assert_eq!(apps_label(0), "0 AppImages");
        assert_eq!(apps_label(1), "1 AppImage");
        assert_eq!(apps_label(3), "3 AppImages");
    }
}
