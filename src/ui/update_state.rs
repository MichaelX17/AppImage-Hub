use crate::app::{ManagerApp, OpKind};
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, RichText};
use std::time::Instant;

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp) {
    if mgr.ops.is_empty() && mgr.failed_ops.is_empty() {
        return;
    }

    egui::TopBottomPanel::bottom("activity_footer")
        .frame(
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .inner_margin(egui::Margin::symmetric(
                    theme::SPACE_LG as i8,
                    theme::SPACE_SM as i8,
                )),
        )
        .show(ctx, |ui| {
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
                for op in &ops {
                    ui.group(|ui| {
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
                                    cancel_id = Some(op.op_id.clone());
                                }
                            });
                        });
                        let fraction = if op.fraction.is_finite() {
                            op.fraction.clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let pb = egui::ProgressBar::new(fraction)
                            .show_percentage()
                            .fill(theme::ACCENT);
                        ui.add(pb);
                        let downloaded_mb = op.downloaded as f64 / (1024.0 * 1024.0);
                        let total_mb = op
                            .total
                            .map(|t| format!("{:.1}", t as f64 / (1024.0 * 1024.0)))
                            .unwrap_or_else(|| "?".into());
                        let eta = format_eta_parts(op.started_at, op.downloaded, op.total);
                        ui.label(
                            theme::secondary_label(format!(
                                "{:.1} / {} MB • ETA {}",
                                downloaded_mb, total_mb, eta
                            ))
                            .small(),
                        );
                    });
                    ui.add_space(theme::SPACE_XS);
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
                for (i, f) in mgr.failed_ops.iter().enumerate() {
                    ui.group(|ui| {
                        let w = ui.available_width();
                        if w.is_finite() && w > 0.0 {
                            ui.set_width(w);
                        }
                        ui.label(
                            RichText::new(format!("{}: {}", f.label, f.message))
                                .color(theme::DANGER),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(f.retry.is_some(), theme::primary_button("Retry"))
                                .clicked()
                            {
                                to_retry = Some(i);
                            }
                            if ui.add(theme::secondary_button("Dismiss")).clicked() {
                                to_dismiss = Some(i);
                            }
                        });
                    });
                    ui.add_space(theme::SPACE_XS);
                }
                if let Some(i) = to_retry.take() {
                    mgr.retry_failed(i);
                } else if let Some(i) = to_dismiss.take() {
                    if i < mgr.failed_ops.len() {
                        mgr.failed_ops.remove(i);
                    }
                }
            }
        });
}

fn format_eta_parts(started_at: Instant, downloaded: u64, total: Option<u64>) -> String {
    let Some(total) = total else {
        return "--".into();
    };
    if downloaded == 0 {
        return "--".into();
    }
    let elapsed = started_at.elapsed().as_secs_f64();
    if elapsed <= 0.5 {
        return "--".into();
    }
    let rate = downloaded as f64 / elapsed;
    if rate <= 0.0 {
        return "--".into();
    }
    let remaining = total.saturating_sub(downloaded) as f64 / rate;
    if !remaining.is_finite() || remaining < 0.0 {
        return "--".into();
    }
    let secs = remaining.round() as u64;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
