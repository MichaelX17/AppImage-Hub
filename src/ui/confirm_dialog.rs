use crate::app::ManagerApp;
use crate::ui::theme;
use eframe::egui;

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

    let mut open = true;
    egui::Window::new("Confirm Delete")
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(format!("Are you sure you want to delete \"{}\"?", app_name));
            ui.add_space(theme::SPACE_MD);
            ui.horizontal(|ui| {
                if ui.add(theme::danger_button("Yes, Delete")).clicked() {
                    mgr.confirm_delete(&app_id);
                    mgr.pending_delete = None;
                }
                if ui.add(theme::secondary_button("Cancel")).clicked() {
                    mgr.pending_delete = None;
                }
            });
        });

    if !open {
        mgr.pending_delete = None;
    }
}
