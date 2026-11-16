use crate::app::ManagerApp;
use crate::ui::theme;
use eframe::egui::{self, RichText, Stroke};

pub fn render(ctx: &egui::Context, mgr: &mut ManagerApp) {
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
        // Dark focus layer behind the dialog. egui does not provide a native
        // gaussian backdrop blur, so the modal uses a strong dimming veil.
        .backdrop_color(egui::Color32::from_black_alpha(175))
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE_RAISED)
                .stroke(Stroke::new(1.5_f32, theme::ACCENT))
                .corner_radius(egui::CornerRadius::same(theme::RADIUS_LG))
                .inner_margin(egui::Margin::same(theme::SPACE_LG as i8))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 6],
                    blur: 24,
                    spread: 1,
                    color: theme::with_alpha(theme::ACCENT, 80),
                }),
        )
        .show(ctx, |ui| {
            ui.set_width(360.0);

            ui.label(
                RichText::new("Confirm Delete")
                    .size(21.0)
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );

            ui.add_space(theme::SPACE_MD);
            ui.label(
                RichText::new(format!("Are you sure you want to delete \"{app_name}\"?"))
                    .size(14.0)
                    .color(theme::TEXT_SECONDARY),
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
