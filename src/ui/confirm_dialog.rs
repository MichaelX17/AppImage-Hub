use crate::app::ManagerApp;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, ButtonTone};
use crate::ui::fonts;
use eframe::egui::{self, RichText};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp) {
    if mgr.pending_delete.is_some() {
        render_delete(ctx, mgr);
    }
    if mgr.pending_storage_change.is_some() {
        render_storage_change(ctx, mgr);
    }
}

fn render_delete(ctx: &egui::Context, mgr: &mut ManagerApp) {
    let Some(app_id) = mgr.pending_delete.clone() else {
        return;
    };

    let app_name = mgr
        .config
        .lock()
        .ok()
        .and_then(|c| {
            c.applications
                .iter()
                .find(|a| a.id == app_id)
                .map(|a| a.display_name.clone())
        })
        .unwrap_or_else(|| app_id.clone());

    let mut request_close = false;
    let modal = egui::Modal::new(egui::Id::new("confirm_delete_modal"))
        .backdrop_color(egui::Color32::from_black_alpha(145))
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(260.0, 380.0));

            ui.horizontal(|ui| {
                icons::widget(ui, Icon::Trash, 20.0, theme::danger());
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Delete AppImage")
                        .font(fonts::bold(19.0))
                        .color(theme::text_primary()),
                );
            });

            ui.add_space(theme::SPACE_MD);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "Are you sure you want to delete \"{app_name}\" and remove its managed files?"
                    ))
                    .font(fonts::regular(13.5))
                    .color(theme::text_secondary()),
                )
                .wrap(),
            );
            ui.add_space(theme::SPACE_XL);

            ui.horizontal(|ui| {
                if theme::pill_action(ui, Some(Icon::Trash), "Delete", ButtonTone::Danger, 34.0)
                    .clicked()
                {
                    mgr.confirm_delete(&app_id);
                    mgr.pending_delete = None;
                    request_close = true;
                }
                ui.add_space(6.0);
                if theme::pill_action(ui, None, "Cancel", ButtonTone::Ghost, 34.0).clicked() {
                    mgr.pending_delete = None;
                    request_close = true;
                }
            });
        });

    if request_close || modal.should_close() {
        mgr.pending_delete = None;
    }
}

fn render_storage_change(ctx: &egui::Context, mgr: &mut ManagerApp) {
    let Some(new_path) = mgr.pending_storage_change.clone() else {
        return;
    };

    let mut request_close = false;
    let modal = egui::Modal::new(egui::Id::new("confirm_storage_change_modal"))
        .backdrop_color(egui::Color32::from_black_alpha(145))
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 440.0));

            ui.horizontal(|ui| {
                icons::widget(ui, Icon::Folder, 20.0, theme::accent());
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Change Storage Folder?")
                        .font(fonts::bold(19.0))
                        .color(theme::text_primary()),
                );
            });

            ui.add_space(theme::SPACE_MD);
            ui.add(
                egui::Label::new(
                    RichText::new(format!(
                        "New downloads will be stored in {}.\n\nExisting AppImages stay in their \
                         current folders and keep working; they are not moved automatically.",
                        new_path.display()
                    ))
                    .font(fonts::regular(13.5))
                    .color(theme::text_secondary()),
                )
                .wrap(),
            );
            ui.add_space(theme::SPACE_XL);

            ui.horizontal(|ui| {
                if theme::pill_action(
                    ui,
                    Some(Icon::Check),
                    "Change Folder",
                    ButtonTone::Primary,
                    34.0,
                )
                .clicked()
                {
                    mgr.confirm_storage_change(ctx);
                    request_close = true;
                }
                ui.add_space(6.0);
                if theme::pill_action(ui, None, "Cancel", ButtonTone::Ghost, 34.0).clicked() {
                    mgr.pending_storage_change = None;
                    request_close = true;
                }
            });
        });

    if request_close || modal.should_close() {
        mgr.pending_storage_change = None;
    }
}
