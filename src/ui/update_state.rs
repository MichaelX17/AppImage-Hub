//! Floating Activity Dock for in-flight downloads & failures, plus floating toast notifications.
//!
//! Replaces the old full-width bottom status bar: when idle, zero vertical space
//! is wasted; when downloads run or fail, a sleek floating card rises cleanly at
//! the bottom-right of the canvas.

use crate::app::{ActiveOp, FailedOp, ManagerApp, OpKind};
use crate::ui::breakpoints::SizeClass;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, ButtonTone};
use crate::ui::{fonts, format};
use eframe::egui::{
    self, Align, CornerRadius, Layout, Margin, Rect, RichText, Sense, Stroke, pos2, vec2,
};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp, size_class: SizeClass) {
    render_toasts(ctx, mgr);

    let has_activity = !mgr.ops.is_empty() || !mgr.failed_ops.is_empty();
    if has_activity {
        render_floating_dock(ctx, mgr, size_class);
    }

    if mgr.last_checked.is_some() {
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }
}

const TOAST_FADE_SECS: f64 = 0.5;
const TOAST_LIFETIME_SECS: f64 = 5.0;

#[derive(Clone, Default)]
struct StatusToastAnim {
    text: String,
    started_at: f64,
    dismiss_at: Option<f64>,
}

fn render_toasts(ctx: &egui::Context, mgr: &mut ManagerApp) {
    let now = ctx.input(|i| i.time);
    let anim_id = egui::Id::new("status_toast_anim");

    // Track 0.5s fade-in, 5.0s total lifetime, and 0.5s fade-out for status notifications.
    let status_opacity = if let Some(status_text) = &mgr.status {
        let state = ctx
            .data(|d| d.get_temp::<StatusToastAnim>(anim_id))
            .filter(|s| s.text == *status_text)
            .unwrap_or_else(|| StatusToastAnim {
                text: status_text.clone(),
                started_at: now,
                dismiss_at: None,
            });

        let elapsed = (now - state.started_at).max(0.0);
        let fade_out_start = state
            .dismiss_at
            .unwrap_or(state.started_at + TOAST_LIFETIME_SECS - TOAST_FADE_SECS);
        let end_time = fade_out_start + TOAST_FADE_SECS;

        if now >= end_time {
            mgr.status = None;
            ctx.data_mut(|d| d.remove_temp::<StatusToastAnim>(anim_id));
            0.0_f32
        } else {
            let fade_in = (elapsed / TOAST_FADE_SECS).clamp(0.0, 1.0) as f32;
            let fade_out = if now >= fade_out_start {
                (1.0 - (now - fade_out_start) / TOAST_FADE_SECS).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            let opacity = fade_in.min(fade_out);

            if elapsed < TOAST_FADE_SECS || now >= fade_out_start {
                ctx.request_repaint();
            } else {
                let wait = (fade_out_start - now).max(0.01);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
            }
            ctx.data_mut(|d| d.insert_temp(anim_id, state));
            opacity
        }
    } else {
        ctx.data_mut(|d| d.remove_temp::<StatusToastAnim>(anim_id));
        0.0
    };

    let mut messages: Vec<(u8, String, bool, f32)> = Vec::new();
    if let Some(w) = &mgr.config_warning {
        messages.push((0, w.clone(), true, 1.0));
    }
    if let Some(w) = &mgr.token_warning {
        messages.push((1, w.clone(), true, 1.0));
    }
    if let Some(s) = &mgr.status {
        if status_opacity > 0.001 {
            messages.push((2, s.clone(), false, status_opacity));
        }
    }
    if messages.is_empty() {
        return;
    }

    let screen = ctx.screen_rect();
    let max_w = (screen.width() - 40.0).clamp(240.0, 440.0);
    let anchor_pos = pos2(screen.right() - max_w - 20.0, screen.top() + 18.0);

    let mut dismiss_kind: Option<u8> = None;

    egui::Area::new(egui::Id::new("floating_toasts"))
        .order(egui::Order::Foreground)
        .fixed_pos(anchor_pos)
        .show(ctx, |ui| {
            ui.set_width(max_w);
            ui.spacing_mut().item_spacing.y = 8.0;

            for (kind, text, is_warning, opacity) in &messages {
                let p = theme::current();
                let accent_col = if *is_warning { p.warning } else { p.accent };
                ui.scope(|ui| {
                    ui.multiply_opacity(*opacity);
                    theme::menu_frame()
                        .inner_margin(Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_width(max_w - 28.0);
                            ui.horizontal(|ui| {
                                icons::widget(
                                    ui,
                                    if *is_warning {
                                        Icon::AlertTriangle
                                    } else {
                                        Icon::Info
                                    },
                                    15.0,
                                    accent_col,
                                );
                                ui.add_space(4.0);
                                ui.vertical(|ui| {
                                    ui.set_width((max_w - 86.0).max(120.0));
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(text)
                                                .font(fonts::regular(12.5))
                                                .color(if *is_warning {
                                                    p.warning
                                                } else {
                                                    p.text_primary
                                                }),
                                        )
                                        .wrap(),
                                    );
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if theme::icon_button(ui, Icon::Close, 24.0, false, "Dismiss")
                                        .clicked()
                                    {
                                        dismiss_kind = Some(*kind);
                                    }
                                });
                            });
                        });
                });
            }
        });

    match dismiss_kind {
        Some(0) => mgr.config_warning = None,
        Some(1) => mgr.token_warning = None,
        Some(2) => {
            // Trigger smooth 0.5s fade-out on manual dismiss
            if let Some(mut state) = ctx.data(|d| d.get_temp::<StatusToastAnim>(anim_id)) {
                if state.dismiss_at.is_none() {
                    state.dismiss_at = Some(now);
                    ctx.data_mut(|d| d.insert_temp(anim_id, state));
                    ctx.request_repaint();
                }
            } else {
                mgr.status = None;
            }
        }
        _ => {}
    }
}

fn render_floating_dock(ctx: &egui::Context, mgr: &mut ManagerApp, size_class: SizeClass) {
    let mut ops: Vec<&ActiveOp> = mgr.ops.values().collect();
    ops.sort_by_key(|op| op.started_at);
    let has_ops = !ops.is_empty();
    let failed: &[FailedOp] = &mgr.failed_ops;

    let mut cancel_id: Option<String> = None;
    let mut retry_idx: Option<usize> = None;
    let mut dismiss_idx: Option<usize> = None;

    let screen = ctx.screen_rect();
    let dock_w = if matches!(size_class, SizeClass::Tight | SizeClass::Narrow) {
        (screen.width() - 32.0).max(260.0)
    } else {
        (screen.width() * 0.42).clamp(360.0, 480.0)
    };

    egui::Area::new(egui::Id::new("floating_activity_dock"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-20.0, -20.0))
        .show(ctx, |ui| {
            theme::modal_frame()
                .inner_margin(Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(dock_w);

                    if has_ops {
                        ui.horizontal(|ui| {
                            icons::widget(ui, Icon::Download, 15.0, theme::accent());
                            ui.label(
                                RichText::new("Activity")
                                    .font(fonts::bold(13.5))
                                    .color(theme::text_primary()),
                            );
                            ui.label(
                                theme::muted_label(active_count_label(ops.len()))
                                    .font(fonts::regular(12.0)),
                            );
                            if ops.len() > 1 {
                                if let Some(overall) = overall_fraction(&ops) {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        theme::version_pill(
                                            ui,
                                            &format!("{}%", percent_text(overall)),
                                            theme::accent(),
                                            theme::accent_soft(),
                                        );
                                    });
                                }
                            }
                        });
                        ui.add_space(theme::SPACE_SM);
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("progress_footer")
                        .max_height(footer_max_height(ctx, size_class))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for op in &ops {
                                render_active_row(ui, op, &mut cancel_id);
                            }
                            if !failed.is_empty() {
                                render_failed_rows(
                                    ui,
                                    failed,
                                    has_ops,
                                    &mut retry_idx,
                                    &mut dismiss_idx,
                                );
                            }
                        });
                });
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
            let title: &str = if op.label.trim().is_empty() {
                match op.kind {
                    OpKind::Install => "Installing",
                    OpKind::Update => "Updating",
                }
            } else {
                op.label.as_str()
            };
            ui.add(
                egui::Label::new(
                    RichText::new(title)
                        .font(fonts::medium(13.0))
                        .color(theme::text_primary()),
                )
                .truncate(),
            )
            .on_hover_text(title);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Close, 24.0, false, "Cancel download").clicked() {
                    *cancel_id = Some(op.op_id.clone());
                }
            });
        });

        ui.add_space(4.0);
        let unknown_total = op.total.is_none() && op.fraction < 1.0;
        let fraction = if unknown_total {
            let time = ui.input(|input| input.time) as f32;
            0.08 + 0.72 * (0.5 + 0.5 * (time * 2.0).sin())
        } else {
            sanitize_fraction(op.fraction)
        };

        // Custom crisp pill progress track
        let p = theme::current();
        let (bar_rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        ui.painter()
            .rect_filled(bar_rect, CornerRadius::same(theme::RADIUS_PILL), p.neutral_soft);
        let fill_w = (bar_rect.width() * fraction).max(6.0);
        let fill_rect = Rect::from_min_size(bar_rect.min, vec2(fill_w, bar_rect.height()));
        ui.painter()
            .rect_filled(fill_rect, CornerRadius::same(theme::RADIUS_PILL), p.accent);

        ui.add_space(4.0);
        let progress = format::format_progress(op.downloaded, op.total);
        let detail = if unknown_total {
            format!("{progress} • size unknown")
        } else {
            let eta = format::format_eta(op.started_at, op.downloaded, op.total);
            format!("{}% • {} • ETA {}", percent_text(fraction), progress, eta)
        };
        ui.label(
            RichText::new(detail)
                .font(fonts::mono(10.5))
                .color(theme::text_muted()),
        );
    });
    ui.add_space(6.0);
}

fn render_failed_rows(
    ui: &mut egui::Ui,
    failed: &[FailedOp],
    has_ops: bool,
    retry_idx: &mut Option<usize>,
    dismiss_idx: &mut Option<usize>,
) {
    if has_ops {
        theme::divider(ui);
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        icons::widget(ui, Icon::AlertTriangle, 14.0, theme::danger());
        ui.label(
            RichText::new("Failed")
                .font(fonts::bold(13.0))
                .color(theme::text_primary()),
        );
        ui.label(theme::muted_label(failed_count_label(failed.len())).font(fonts::regular(11.5)));
    });
    ui.add_space(6.0);

    for (i, f) in failed.iter().enumerate() {
        activity_frame(ui, |ui| {
            let width = ui.available_width();
            if width.is_finite() && width > 0.0 {
                ui.set_width(width);
            }
            ui.add(
                egui::Label::new(
                    RichText::new(&f.label)
                        .font(fonts::medium(12.5))
                        .color(theme::text_primary()),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(&f.message)
                        .font(fonts::regular(11.5))
                        .color(theme::danger()),
                )
                .truncate(),
            )
            .on_hover_text(&f.message);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(f.retry.is_some(), |ui| {
                    if theme::pill_action(ui, Some(Icon::Refresh), "Retry", ButtonTone::Primary, 28.0)
                        .clicked()
                    {
                        *retry_idx = Some(i);
                    }
                });
                if theme::pill_action(ui, None, "Dismiss", ButtonTone::Ghost, 28.0).clicked() {
                    *dismiss_idx = Some(i);
                }
            });
        });
        ui.add_space(6.0);
    }
}

fn activity_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let p = theme::current();
    egui::Frame::new()
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_md))
        .inner_margin(Margin::same(10))
        .show(ui, add_contents);
}

#[allow(dead_code)]
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
    let base = (ctx.screen_rect().height() * 0.28).clamp(110.0, 240.0);
    match size_class {
        SizeClass::Tight => base.min(150.0),
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
