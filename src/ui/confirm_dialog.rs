use crate::app::ManagerApp;
use crate::ui::theme;
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
        .backdrop_color(egui::Color32::from_black_alpha(150))
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(260.0, 380.0));

            ui.label(
                RichText::new("Confirm Delete")
                    .size(20.0)
                    .strong()
                    .color(theme::text_primary()),
            );

            ui.add_space(theme::SPACE_MD);
            ui.label(
                RichText::new(format!("Are you sure you want to delete \"{app_name}\"?"))
                    .size(14.0)
                    .color(theme::text_secondary()),
            );
            ui.add_space(theme::SPACE_LG);

            ui.horizontal(|ui| {
                if ui.add(theme::danger_button("Yes, Delete")).clicked() {
                    mgr.confirm_delete(&app_id);
                    mgr.pending_delete = None;
                    request_close = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    mgr.pending_delete = None;
                    request_close = true;
                }
            });
        });

    if request_close || modal.should_close() {
        mgr.pending_delete = None;
    }
}

/// D4: changing the storage folder never moves existing AppImages. Explain
/// that explicitly before applying the new root.
fn render_storage_change(ctx: &egui::Context, mgr: &mut ManagerApp) {
    let Some(new_path) = mgr.pending_storage_change.clone() else {
        return;
    };

    let mut request_close = false;
    let modal = egui::Modal::new(egui::Id::new("confirm_storage_change_modal"))
        .backdrop_color(egui::Color32::from_black_alpha(150))
        .frame(theme::modal_frame())
        .show(ctx, |ui| {
            ui.set_width((ctx.screen_rect().width() - 64.0).clamp(280.0, 440.0));

            ui.label(
                RichText::new("Change AppImage Folder?")
                    .size(20.0)
                    .strong()
                    .color(theme::text_primary()),
            );

            ui.add_space(theme::SPACE_MD);
            ui.label(
                RichText::new(format!(
                    "New downloads will be stored in {}.\n\nExisting AppImages stay in their \
                     current folders and keep working; they are not moved automatically. \
                     Move them manually if you want everything in the new folder.",
                    new_path.display()
                ))
                .size(14.0)
                .color(theme::text_secondary()),
            );
            ui.add_space(theme::SPACE_LG);

            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Change Folder")).clicked() {
                    mgr.confirm_storage_change(ctx);
                    request_close = true;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    mgr.pending_storage_change = None;
                    request_close = true;
                }
            });
        });

    if request_close || modal.should_close() {
        mgr.pending_storage_change = None;
    }
}
