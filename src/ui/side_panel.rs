use crate::app::ManagerApp;
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, Margin, RichText, Stroke};

#[derive(Default, Clone, PartialEq, Eq)]
pub enum SidePanelMode {
    #[default]
    LocalOnly,
    LocalWithRepo,
    GithubOnly,
}

#[derive(Default, Clone)]
pub struct SidePanelFormState {
    pub editing_app_id: Option<String>,
    pub display_name: String,
    pub mode: SidePanelMode,
    pub local_path: String,
    pub icon_path: String,
    pub github_url: String,
    pub launch_args: String,
}

impl SidePanelFormState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        close_editor(mgr);
        return;
    }
    let is_edit = mgr.side_panel_form.editing_app_id.is_some();
    let title = if is_edit {
        "Edit AppImage"
    } else {
        "Add AppImage"
    };

    let panel_frame = egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(Margin::same(theme::SPACE_LG as i8));

    panel_frame.show(ui, |ui| {
        let max = ui.max_rect();
        if max.is_finite() && max.width() > 0.0 && max.height() > 0.0 {
            let rail_rect = egui::Rect::from_min_max(
                max.left_top() + egui::vec2(-10.0, 0.0),
                egui::pos2(max.left() - 7.0, max.bottom()),
            );
            if rail_rect.is_finite() && rail_rect.is_positive() {
                ui.painter().rect_filled(rail_rect, 0.0, theme::ACCENT);
            }
        }

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(if is_edit { "✎" } else { "+" })
                    .size(18.0)
                    .color(theme::ACCENT_HOVER),
            );
            ui.add_space(4.0);
            ui.label(RichText::new(title).size(20.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(theme::ghost_button("✕"))
                    .on_hover_text("Close")
                    .clicked()
                {
                    close_editor(mgr);
                }
            });
        });

        ui.add_space(theme::SPACE_MD);
        ui.separator();
        ui.add_space(theme::SPACE_SM);

        let footer_height = 44.0;
        let available = ui.available_height();
        let available = if available.is_finite() { available } else { 300.0 };
        let form_height = (available - footer_height - theme::SPACE_SM).max(120.0);

        egui::ScrollArea::vertical()
            .id_salt("editor_form_scroll")
            .auto_shrink([false, false])
            .max_height(form_height)
            .show(ui, |ui| {
                render_form_fields(ui, mgr);
            });

        ui.add_space(theme::SPACE_SM);
        ui.separator();
        ui.add_space(theme::SPACE_SM);
        let can_save = is_form_valid(&mgr.side_panel_form);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(can_save, |ui| {
                if ui.add(theme::primary_button("Save")).clicked() {
                    mgr.submit_side_panel_form();
                }
            });
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                close_editor(mgr);
            }
        });
    });
}

fn close_editor(mgr: &mut ManagerApp) {
    mgr.is_form_open = false;
    mgr.side_panel_form.clear();
}

pub fn render_add_to(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        mgr.close_add_to();
        return;
    }
    let Some(mut state) = mgr.add_to.clone() else {
        return;
    };

    let panel_frame = egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(Stroke::new(1.0_f32, theme::BORDER))
        .inner_margin(Margin::same(theme::SPACE_LG as i8));

    panel_frame.show(ui, |ui| {
        let max = ui.max_rect();
        if max.is_finite() && max.width() > 0.0 && max.height() > 0.0 {
            let rail_rect = egui::Rect::from_min_max(
                max.left_top() + egui::vec2(-10.0, 0.0),
                egui::pos2(max.left() - 7.0, max.bottom()),
            );
            if rail_rect.is_finite() && rail_rect.is_positive() {
                ui.painter().rect_filled(rail_rect, 0.0, theme::ACCENT);
            }
        }

        ui.horizontal(|ui| {
            ui.label(RichText::new("+").size(18.0).color(theme::ACCENT_HOVER));
            ui.add_space(4.0);
            ui.label(RichText::new("Add to..").size(20.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(theme::ghost_button("✕"))
                    .on_hover_text("Close")
                    .clicked()
                {
                    mgr.close_add_to();
                }
            });
        });

        ui.add_space(theme::SPACE_MD);
        ui.separator();
        ui.add_space(theme::SPACE_MD);

        ui.label(RichText::new(&state.app_name).strong().size(16.0));
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::secondary_label(
            "Choose where to add this application:",
        ));
        ui.add_space(theme::SPACE_MD);

        let desktop_path = crate::xdg::desktop_dir().ok().and_then(|d| {
            mgr.config.lock().ok().and_then(|c| {
                c.applications
                    .iter()
                    .find(|a| a.id == state.app_id)
                    .map(|app| crate::desktop_entry::desktop_shortcut_path(app, &d))
            })
        });
        let menu_path = crate::xdg::applications_dir().ok().and_then(|a_dir| {
            mgr.config.lock().ok().and_then(|c| {
                c.applications
                    .iter()
                    .find(|a| a.id == state.app_id)
                    .map(|app| crate::desktop_entry::menu_entry_path(app, &a_dir))
            })
        });

        if let Some(p) = &desktop_path {
            let exists = p.exists();
            ui.label(
                theme::muted_label(format!(
                    "Desktop: {}{}",
                    p.display(),
                    if exists { " (exists)" } else { "" }
                ))
                .small(),
            );
        }
        if let Some(p) = &menu_path {
            let exists = p.exists();
            ui.label(
                theme::muted_label(format!(
                    "Menu: {}{}",
                    p.display(),
                    if exists { " (exists)" } else { "" }
                ))
                .small(),
            );
        }

        ui.add_space(theme::SPACE_SM);
        ui.checkbox(&mut state.desktop, "Desktop shortcut");
        ui.add_space(theme::SPACE_XS);
        ui.checkbox(&mut state.menu, "Start menu entry");
        ui.add_space(theme::SPACE_MD);
        ui.label(theme::muted_label(
            "Checked = create/keep. Unchecked = remove if present.",
        ));

        mgr.add_to = Some(state);

        ui.add_space(theme::SPACE_LG);
        ui.separator();
        ui.add_space(theme::SPACE_SM);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui.add(theme::primary_button("Apply")).clicked() {
                mgr.apply_add_to();
            }
            if ui.add(theme::secondary_button("Cancel")).clicked() {
                mgr.close_add_to();
            }
        });
    });
}

fn render_form_fields(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(RichText::new("Display Name").strong());
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.display_name)
            .hint_text("Application name")
            .desired_width({ let w = ui.available_width(); if w.is_finite() { w.max(80.0) } else { 240.0 } }),
    );
    ui.add_space(theme::SPACE_MD);

    ui.label(RichText::new("Source Type").strong());
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(
            &mut mgr.side_panel_form.mode,
            SidePanelMode::LocalOnly,
            "Local Only",
        );
        ui.selectable_value(
            &mut mgr.side_panel_form.mode,
            SidePanelMode::LocalWithRepo,
            "Local + Repo",
        );
        ui.selectable_value(
            &mut mgr.side_panel_form.mode,
            SidePanelMode::GithubOnly,
            "GitHub Only",
        );
    });
    ui.add_space(theme::SPACE_MD);

    let needs_local_path = matches!(
        mgr.side_panel_form.mode,
        SidePanelMode::LocalOnly | SidePanelMode::LocalWithRepo
    );
    let needs_repo = matches!(
        mgr.side_panel_form.mode,
        SidePanelMode::LocalWithRepo | SidePanelMode::GithubOnly
    );

    ui.add_enabled_ui(
        needs_local_path || mgr.side_panel_form.editing_app_id.is_some(),
        |ui| {
            ui.label(RichText::new("AppImage Path").strong());
            ui.horizontal(|ui| {
                let browse_w = 80.0;
                let path_editable =
                    needs_local_path && mgr.side_panel_form.editing_app_id.is_none();
                let w = ui.available_width();
                let w = if w.is_finite() { w } else { 240.0 };
                ui.add_enabled(
                    path_editable,
                    egui::TextEdit::singleline(&mut mgr.side_panel_form.local_path)
                        .hint_text(if needs_local_path {
                            "Path to .AppImage file"
                        } else {
                            "Managed after install"
                        })
                        .desired_width((w - browse_w - 8.0).max(80.0)),
                );
                if ui
                    .add_enabled(path_editable, theme::secondary_button("Browse"))
                    .clicked()
                {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("AppImage", &["AppImage", "appimage"])
                        .pick_file()
                    {
                        mgr.side_panel_form.local_path = p.display().to_string();
                        if mgr.side_panel_form.display_name.trim().is_empty() {
                            if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                                mgr.side_panel_form.display_name = stem.to_string();
                            }
                        }
                    }
                }
            });
            if mgr.side_panel_form.editing_app_id.is_some() {
                ui.label(theme::muted_label("Managed path is fixed for existing entries.").small());
            } else if !needs_local_path {
                ui.label(theme::muted_label("Not required for GitHub-only install.").small());
            }
        },
    );
    ui.add_space(theme::SPACE_MD);

    ui.label(RichText::new("Icon Path (optional)").strong());
    ui.horizontal(|ui| {
        let browse_w = 80.0;
        let w = ui.available_width();
        let w = if w.is_finite() { w } else { 240.0 };
        ui.add(
            egui::TextEdit::singleline(&mut mgr.side_panel_form.icon_path)
                .hint_text("Optional icon file")
                .desired_width((w - browse_w - 8.0).max(80.0)),
        );
        if ui.add(theme::secondary_button("Browse")).clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "svg", "xpm", "ico"])
                .pick_file()
            {
                mgr.side_panel_form.icon_path = p.display().to_string();
            }
        }
    });
    ui.add_space(theme::SPACE_MD);

    ui.add_enabled_ui(
        needs_repo || !mgr.side_panel_form.github_url.trim().is_empty(),
        |ui| {
            ui.label(RichText::new("GitHub URL").strong());
            ui.add(
                egui::TextEdit::singleline(&mut mgr.side_panel_form.github_url)
                    .hint_text("owner/repo or https://github.com/owner/repo")
                    .desired_width({ let w = ui.available_width(); if w.is_finite() { w.max(80.0) } else { 240.0 } }),
            );
            if !needs_repo {
                ui.label(theme::muted_label("Optional for local-only entries.").small());
            }
        },
    );
    ui.add_space(theme::SPACE_MD);

    ui.label(RichText::new("Launch Args").strong());
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.launch_args)
            .hint_text("Optional arguments")
            .desired_width({ let w = ui.available_width(); if w.is_finite() { w.max(80.0) } else { 240.0 } }),
    );
    ui.add_space(theme::SPACE_SM);

    let validation = validation_message(&mgr.side_panel_form);
    if let Some(msg) = validation {
        ui.add_space(theme::SPACE_XS);
        ui.label(RichText::new(msg).color(theme::DANGER).small());
    }
}

fn validation_message(form: &SidePanelFormState) -> Option<&'static str> {
    if form.display_name.trim().is_empty() {
        return Some("Display Name is required.");
    }
    match form.mode {
        SidePanelMode::LocalOnly => {
            if form.local_path.trim().is_empty() {
                Some("AppImage path is required for Local Only.")
            } else {
                None
            }
        }
        SidePanelMode::LocalWithRepo => {
            if form.local_path.trim().is_empty() {
                Some("AppImage path is required for Local + Repo.")
            } else if form.github_url.trim().is_empty() {
                Some("GitHub URL is required for Local + Repo.")
            } else {
                None
            }
        }
        SidePanelMode::GithubOnly => {
            if form.github_url.trim().is_empty() {
                Some("GitHub URL is required for GitHub Only.")
            } else {
                None
            }
        }
    }
}

fn is_form_valid(form: &SidePanelFormState) -> bool {
    validation_message(form).is_none()
}
