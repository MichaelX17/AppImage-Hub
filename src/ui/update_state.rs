use crate::app::{ManagerApp, OpKind};
use crate::ui::breakpoints::SizeClass;
use crate::ui::format;
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, RichText};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp, size_class: SizeClass) {
    if mgr.ops.is_empty() && mgr.failed_ops.is_empty() {
        return;
    }

    let footer_max = footer_max_height(ctx, size_class);

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
            egui::ScrollArea::vertical()
                .id_salt("activity_footer")
                .max_height(footer_max)
                .show(ui, |ui| {
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
                        let render_op =
                            |ui: &mut egui::Ui,
                             op: &crate::app::ActiveOp,
                             cancel_id: &mut Option<String>| {
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
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(theme::secondary_button("Cancel"))
                                                    .clicked()
                                                {
                                                    *cancel_id = Some(op.op_id.clone());
                                                }
                                            },
                                        );
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
                                    let progress = format::format_progress(op.downloaded, op.total);
                                    let eta =
                                        format::format_eta(op.started_at, op.downloaded, op.total);
                                    ui.label(
                                        theme::secondary_label(format!(
                                            "{} • ETA {}",
                                            progress, eta
                                        ))
                                        .small(),
                                    );
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
                        let render_failed =
                            |ui: &mut egui::Ui,
                             i: usize,
                             f: &crate::app::FailedOp,
                             to_retry: &mut Option<usize>,
                             to_dismiss: &mut Option<usize>| {
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
                                            .add_enabled(
                                                f.retry.is_some(),
                                                theme::primary_button("Retry"),
                                            )
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
                                    for (offset, f) in
                                        mgr.failed_ops.iter().skip(failed_visible).enumerate()
                                    {
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
                });
        });
}

fn footer_max_height(ctx: &egui::Context, size_class: SizeClass) -> f32 {
    let base = (ctx.screen_rect().height() * 0.30).clamp(120.0, 280.0);
    match size_class {
        SizeClass::Tight => base.min(160.0),
        _ => base,
    }
}
