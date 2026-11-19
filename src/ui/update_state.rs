//! Bottom status footer.
//!
//! The footer is the single home for download progress: every in-flight
//! update/install is listed here with its own bar, bytes, ETA and Cancel
//! button. AppImage cards only show that something is happening — they never
//! duplicate progress UI. The status row (app count, "last checked", sort
//! selector) stays pinned at the very bottom of the window.

use crate::app::{ActiveOp, AppSortBy, FailedOp, ManagerApp, OpKind};
use crate::ui::breakpoints::SizeClass;
use crate::ui::format;
use crate::ui::theme;
use eframe::egui::{self, Align, CornerRadius, Layout, Margin, RichText, Stroke};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp, size_class: SizeClass) {
    let app_count = mgr.app_count();
    let has_activity = !mgr.ops.is_empty() || !mgr.failed_ops.is_empty();

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
            if has_activity {
                render_progress(ui, mgr, size_class);
                ui.add_space(theme::SPACE_XS);
                ui.separator();
                ui.add_space(theme::SPACE_XS);
            }

            render_status_bar(ui, mgr, app_count, size_class);
        });

    // Keep "Last checked: 2 minutes ago" fresh while idle.
    if mgr.last_checked.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }
}

/// Aggregate progress view for all current downloads.
fn render_progress(ui: &mut egui::Ui, mgr: &mut ManagerApp, size_class: SizeClass) {
    // Borrow the live operations and failures; never clone them per frame.
    let mut ops: Vec<&ActiveOp> = mgr.ops.values().collect();
    ops.sort_by_key(|op| op.started_at);
    let has_ops = !ops.is_empty();
    let failed: &[FailedOp] = &mgr.failed_ops;

    let mut cancel_id: Option<String> = None;
    let mut retry_idx: Option<usize> = None;
    let mut dismiss_idx: Option<usize> = None;

    if has_ops {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Progress")
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            ui.label(theme::muted_label(active_count_label(ops.len())).size(12.0));
            if ops.len() > 1 {
                if let Some(overall) = overall_fraction(&ops) {
                    ui.label(
                        theme::secondary_label(format!("• {}% overall", percent_text(overall)))
                            .size(12.0),
                    );
                }
            }
        });
        ui.add_space(theme::SPACE_XS);
    }

    egui::ScrollArea::vertical()
        .id_salt("progress_footer")
        .max_height(footer_max_height(ui.ctx(), size_class))
        // Height must follow the content: never reserve a tall empty block in
        // the footer when only one (or no) operation is listed.
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for op in &ops {
                render_active_row(ui, op, &mut cancel_id);
            }
            if !failed.is_empty() {
                render_failed_rows(ui, failed, has_ops, &mut retry_idx, &mut dismiss_idx);
            }
        });

    if let Some(op_id) = cancel_id {
        mgr.cancel_op(&op_id);
    }
    if let Some(idx) = retry_idx {
        mgr.retry_failed(idx);
    } else if let Some(idx) = dismiss_idx {
        if idx < mgr.failed_ops.len() {
            mgr.failed_ops.remove(idx);
        }
    }
}

fn render_active_row(ui: &mut egui::Ui, op: &ActiveOp, cancel_id: &mut Option<String>) {
    activity_frame(ui, |ui| {
        let width = ui.available_width();
        if width.is_finite() && width > 0.0 {
            ui.set_width(width);
        }
        ui.horizontal(|ui| {
            // The stored label is already specific ("Update Codium").
            let title: &str = if op.label.trim().is_empty() {
                match op.kind {
                    OpKind::Install => "Installing",
                    OpKind::Update => "Updating",
                }
            } else {
                op.label.as_str()
            };
            ui.add(
                egui::Label::new(RichText::new(title).strong().color(theme::TEXT_PRIMARY))
                    .truncate(),
            )
            .on_hover_text(title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    *cancel_id = Some(op.op_id.clone());
                }
            });
        });

        // Without a Content-Length the server gives no percentage; pulse the
        // fill instead of leaving the bar stuck at 0%.
        let unknown_total = op.total.is_none() && op.fraction < 1.0;
        let fraction = if unknown_total {
            let time = ui.input(|input| input.time) as f32;
            0.08 + 0.72 * (0.5 + 0.5 * (time * 2.0).sin())
        } else {
            sanitize_fraction(op.fraction)
        };
        let progress = format::format_progress(op.downloaded, op.total);
        let detail = if unknown_total {
            format!("{progress} • total size unknown")
        } else {
            let eta = format::format_eta(op.started_at, op.downloaded, op.total);
            format!("{}% • {} • ETA {}", percent_text(fraction), progress, eta)
        };
        ui.add(
            egui::ProgressBar::new(fraction)
                .text(RichText::new(detail).size(11.0))
                .fill(theme::ACCENT)
                .desired_height(18.0),
        );
    });
    ui.add_space(theme::SPACE_XS);
}

fn render_failed_rows(
    ui: &mut egui::Ui,
    failed: &[FailedOp],
    has_ops: bool,
    retry_idx: &mut Option<usize>,
    dismiss_idx: &mut Option<usize>,
) {
    if has_ops {
        ui.separator();
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Failed").strong().color(theme::TEXT_PRIMARY));
        ui.label(theme::muted_label(failed_count_label(failed.len())).size(12.0));
    });
    ui.add_space(theme::SPACE_XS);

    for (i, f) in failed.iter().enumerate() {
        activity_frame(ui, |ui| {
            let width = ui.available_width();
            if width.is_finite() && width > 0.0 {
                ui.set_width(width);
            }
            ui.add(
                egui::Label::new(
                    RichText::new(&f.label)
                        .strong()
                        .color(theme::TEXT_PRIMARY)
                        .size(12.0),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(RichText::new(&f.message).color(theme::DANGER).size(12.0))
                    .truncate(),
            )
            .on_hover_text(&f.message);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(f.retry.is_some(), theme::primary_button("Retry"))
                    .clicked()
                {
                    *retry_idx = Some(i);
                }
                if ui.add(theme::secondary_button("Dismiss")).clicked() {
                    *dismiss_idx = Some(i);
                }
            });
        });
        ui.add_space(theme::SPACE_XS);
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

/// Status row: `3 AppImages · Last checked: 2 minutes ago   [Sort: Name]`
fn render_status_bar(
    ui: &mut egui::Ui,
    mgr: &mut ManagerApp,
    app_count: usize,
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

fn apps_label(count: usize) -> String {
    if count == 1 {
        "1 AppImage".to_string()
    } else {
        format!("{count} AppImages")
    }
}

fn active_count_label(count: usize) -> String {
    if count == 1 {
        "1 download in progress".to_string()
    } else {
        format!("{count} downloads in progress")
    }
}

fn failed_count_label(count: usize) -> String {
    if count == 1 {
        "1 failed download".to_string()
    } else {
        format!("{count} failed downloads")
    }
}

fn sanitize_fraction(fraction: f32) -> f32 {
    if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn percent_text(fraction: f32) -> i32 {
    (sanitize_fraction(fraction) * 100.0).round() as i32
}

/// Weighted progress across every active operation. Falls back to the mean of
/// per-operation fractions while content lengths are still unknown.
fn overall_fraction(ops: &[&ActiveOp]) -> Option<f32> {
    if ops.is_empty() {
        return None;
    }
    let all_have_total = ops.iter().all(|op| op.total.is_some_and(|total| total > 0));
    if all_have_total {
        let total = ops
            .iter()
            .filter_map(|op| op.total)
            .fold(0_u64, u64::saturating_add);
        let downloaded = ops
            .iter()
            .map(|op| op.downloaded)
            .fold(0_u64, u64::saturating_add);
        Some(sanitize_fraction(
            (downloaded as f64 / total.max(1) as f64) as f32,
        ))
    } else {
        let sum: f32 = ops.iter().map(|op| sanitize_fraction(op.fraction)).sum();
        Some(sanitize_fraction(sum / ops.len() as f32))
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
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn apps_label_is_plural_aware() {
        assert_eq!(apps_label(0), "0 AppImages");
        assert_eq!(apps_label(1), "1 AppImage");
        assert_eq!(apps_label(3), "3 AppImages");
    }

    #[test]
    fn counts_are_plural_aware() {
        assert_eq!(active_count_label(1), "1 download in progress");
        assert_eq!(active_count_label(4), "4 downloads in progress");
        assert_eq!(failed_count_label(1), "1 failed download");
        assert_eq!(failed_count_label(2), "2 failed downloads");
    }

    fn op(fraction: f32, downloaded: u64, total: Option<u64>) -> ActiveOp {
        ActiveOp {
            op_id: "op".into(),
            app_id: Some("app".into()),
            label: "Update App".into(),
            kind: OpKind::Update,
            fraction,
            downloaded,
            total,
            started_at: Instant::now(),
            last_update: Instant::now(),
            cancel: Arc::new(AtomicBool::new(false)),
            retry: None,
        }
    }

    #[test]
    fn overall_fraction_is_byte_weighted_when_totals_are_known() {
        let values = [op(0.9, 90, Some(100)), op(0.0, 0, Some(300))];
        let ops: Vec<&ActiveOp> = values.iter().collect();
        let overall = overall_fraction(&ops).unwrap();
        assert!((overall - 0.225).abs() < 1e-4, "got {overall}");
    }

    #[test]
    fn overall_fraction_falls_back_to_mean_while_totals_are_unknown() {
        let values = [op(0.5, 10, None), op(0.25, 5, None)];
        let ops: Vec<&ActiveOp> = values.iter().collect();
        let overall = overall_fraction(&ops).unwrap();
        assert!((overall - 0.375).abs() < 1e-4, "got {overall}");
    }

    #[test]
    fn overall_fraction_handles_no_operations() {
        assert!(overall_fraction(&[]).is_none());
    }

    #[test]
    fn percentages_and_fractions_are_sanitized() {
        assert_eq!(percent_text(f32::NAN), 0);
        assert_eq!(percent_text(-1.0), 0);
        assert_eq!(percent_text(1.5), 100);
        assert_eq!(sanitize_fraction(0.42), 0.42);
    }
}
