use crate::app::ManagerApp;
use crate::ui::breakpoints::SizeClass;
use crate::ui::theme;
use eframe::egui::{self, Align, Layout, RichText};

#[derive(Default, Clone, PartialEq, Eq)]
pub enum SidePanelMode {
    #[default]
    LocalOnly,
    LocalWithRepo,
    GithubOnly,
}

pub const KDE_CATEGORIES: &[(&str, &str)] = &[
    ("Games", "Game"),
    ("Utilities", "Utility"),
    ("Development", "Development"),
    ("Education", "Education"),
    ("Graphics", "Graphics"),
    ("Internet", "Network"),
    ("Multimedia", "AudioVideo"),
    ("Office", "Office"),
    ("Science & Math", "Science"),
    ("System", "System"),
];

#[derive(Clone)]
pub struct SidePanelFormState {
    pub editing_app_id: Option<String>,
    pub display_name: String,
    pub generic_name: String,
    pub comment: String,
    pub category: String,
    pub custom_category: String,
    pub startup_notify: bool,
    pub mode: SidePanelMode,
    pub local_path: String,
    pub icon_path: String,
    pub github_url: String,
    pub launch_args: String,
}

impl Default for SidePanelFormState {
    fn default() -> Self {
        Self {
            editing_app_id: None,
            display_name: String::new(),
            generic_name: String::new(),
            comment: String::new(),
            category: "Utility".to_string(),
            custom_category: String::new(),
            startup_notify: true,
            mode: SidePanelMode::default(),
            local_path: String::new(),
            icon_path: String::new(),
            github_url: String::new(),
            launch_args: String::new(),
        }
    }
}

impl SidePanelFormState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn effective_category(&self) -> String {
        if self.category == "Custom" {
            let c = self.custom_category.trim();
            if c.is_empty() {
                "Utility".to_string()
            } else {
                c.to_string()
            }
        } else {
            self.category.clone()
        }
    }
}

pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp, size_class: SizeClass) {
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

    theme::panel_frame().show(ui, |ui| {
        draw_accent_rail(ui);

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
                    .add(theme::ghost_button("✕ Close"))
                    .on_hover_text("Close panel (Esc)")
                    .clicked()
                {
                    close_editor(mgr);
                }
                if ui
                    .add(theme::ghost_button("Hide"))
                    .on_hover_text("Hide panel and return to list")
                    .clicked()
                {
                    mgr.show_side_panel = false;
                }
            });
        });

        ui.add_space(theme::SPACE_SM);
        ui.label(theme::secondary_label(
            "Configure the AppImage source, metadata, and launch options.",
        ));
        ui.add_space(theme::SPACE_MD);
        ui.separator();
        ui.add_space(theme::SPACE_SM);

        // Compute footer height dynamically based on current interact size.
        // Footer contains: validation row (interact_size.y) + separator + button row (interact_size.y)
        // plus margins. Add a generous buffer to avoid cutoff.
        let interact_y = ui.spacing().interact_size.y;
        let footer_height = (interact_y * 2.5 + theme::SPACE_SM * 3.0).max(60.0);

        let available = ui.available_height();
        let available = if available.is_finite() {
            available
        } else {
            300.0
        };
        let form_height = (available - footer_height).max(80.0);

        egui::ScrollArea::vertical()
            .id_salt("editor_form_scroll")
            .auto_shrink([false, false])
            .max_height(form_height)
            .show(ui, |ui| {
                render_form_fields(ui, mgr, size_class);
            });

        ui.add_space(theme::SPACE_SM);
        ui.separator();
        ui.add_space(theme::SPACE_XS);

        let validation = validation_message(&mgr.side_panel_form);
        let can_save = validation.is_none();

        // Validation message row
        if let Some(msg) = validation {
            ui.label(
                RichText::new(format!("⚠ {}", msg))
                    .color(theme::DANGER)
                    .small(),
            );
        } else {
            ui.label(theme::muted_label("All required fields are complete.").small());
        }

        ui.add_space(theme::SPACE_XS);

        // Button row: Cancel | Save
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_enabled_ui(can_save, |ui| {
                let btn = ui.add(theme::primary_button("Save"));
                if !can_save {
                    btn.on_disabled_hover_text("Complete required fields to save");
                } else if btn.clicked() {
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

pub fn render_add_to(ui: &mut egui::Ui, mgr: &mut ManagerApp, _size_class: SizeClass) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        mgr.close_add_to();
        return;
    }
    let Some(mut state) = mgr.add_to.clone() else {
        return;
    };

    theme::panel_frame().show(ui, |ui| {
        draw_accent_rail(ui);

        ui.horizontal(|ui| {
            ui.label(RichText::new("+").size(18.0).color(theme::ACCENT_HOVER));
            ui.add_space(4.0);
            ui.label(RichText::new("Add to..").size(20.0).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(theme::ghost_button("✕ Close"))
                    .on_hover_text("Close panel (Esc)")
                    .clicked()
                {
                    mgr.close_add_to();
                }
                if ui
                    .add(theme::ghost_button("Hide"))
                    .on_hover_text("Hide panel and return to list")
                    .clicked()
                {
                    mgr.show_side_panel = false;
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

fn render_form_fields(ui: &mut egui::Ui, mgr: &mut ManagerApp, _size_class: SizeClass) {
    let avail_w = ui.available_width();

    // ── Application section ──────────────────────────────────────────────
    ui.label(theme::section_heading(
        "Application (KDE / Start Menu Metadata)",
    ));
    ui.add_space(theme::SPACE_XS);
    ui.label(theme::field_label("Display Name"));
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.display_name)
            .hint_text("Application name (e.g. PCSX2)")
            .desired_width(avail_w.max(80.0)),
    );
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Generic Name (optional)"));
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.generic_name)
            .hint_text("e.g. PlayStation 2 Emulator")
            .desired_width(avail_w.max(80.0)),
    );
    ui.label(
        theme::muted_label("Shown beside or under the name in KDE Application Launcher.").small(),
    );
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Comment / Description (optional)"));
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.comment)
            .hint_text("e.g. A Sony PlayStation 2 video game console emulator")
            .desired_width(avail_w.max(80.0)),
    );
    ui.label(theme::muted_label("Tooltip description in KDE Start Menu.").small());
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("KDE Menu Category"));
    let is_custom = mgr.side_panel_form.category == "Custom";
    let selected_label = if is_custom {
        "Custom Category…"
    } else {
        KDE_CATEGORIES
            .iter()
            .find(|(_, val)| *val == mgr.side_panel_form.category)
            .map(|(label, _)| *label)
            .unwrap_or("Custom Category…")
    };

    egui::ComboBox::from_id_salt("kde_category_combo")
        .selected_text(selected_label)
        .width(avail_w.max(80.0))
        .show_ui(ui, |ui| {
            for (label, val) in KDE_CATEGORIES {
                ui.selectable_value(&mut mgr.side_panel_form.category, val.to_string(), *label);
            }
            ui.selectable_value(
                &mut mgr.side_panel_form.category,
                "Custom".to_string(),
                "Custom Category…",
            );
        });

    if mgr.side_panel_form.category == "Custom" {
        ui.add_space(theme::SPACE_XS);
        ui.add(
            egui::TextEdit::singleline(&mut mgr.side_panel_form.custom_category)
                .hint_text("e.g. Game, Utility, AudioVideo")
                .desired_width(avail_w.max(80.0)),
        );
    }
    ui.label(
        theme::muted_label("Folder in the KDE Start Menu tree where the app will appear.").small(),
    );
    ui.add_space(theme::SPACE_SM);

    ui.checkbox(
        &mut mgr.side_panel_form.startup_notify,
        "Enable launch feedback (StartupNotify)",
    );
    ui.label(theme::muted_label("Shows launch animation/busy cursor in Plasma session.").small());
    ui.add_space(theme::SPACE_MD);

    // ── Source section ───────────────────────────────────────────────────
    ui.label(theme::section_heading("Source"));
    ui.add_space(theme::SPACE_XS);
    ui.label(theme::field_label("Source Type"));
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

    if needs_local_path {
        ui.label(theme::field_label("AppImage Path"));
        ui.horizontal(|ui| {
            let browse_w = 92.0;
            let path_editable = mgr.side_panel_form.editing_app_id.is_none();
            let w = ui.available_width().max(120.0);
            ui.add_enabled(
                path_editable,
                egui::TextEdit::singleline(&mut mgr.side_panel_form.local_path)
                    .hint_text("Path to .AppImage file")
                    .desired_width((w - browse_w - theme::SPACE_SM).max(80.0)),
            );
            if ui
                .add_enabled(path_editable, theme::secondary_button("Browse"))
                .on_disabled_hover_text("Managed path is fixed for existing entries")
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
        } else {
            ui.label(theme::muted_label("Select the local .AppImage to import or track.").small());
        }
        ui.add_space(theme::SPACE_MD);
    } else {
        ui.label(
            theme::muted_label("AppImage will be downloaded and managed after install.").small(),
        );
        ui.add_space(theme::SPACE_MD);
    }

    // ── Metadata section ─────────────────────────────────────────────────
    ui.label(theme::section_heading("Metadata"));
    ui.add_space(theme::SPACE_XS);
    ui.label(theme::field_label("Icon Path (optional)"));
    ui.horizontal(|ui| {
        let browse_w = 92.0;
        let w = ui.available_width().max(120.0);
        ui.add(
            egui::TextEdit::singleline(&mut mgr.side_panel_form.icon_path)
                .hint_text("Optional icon file")
                .desired_width((w - browse_w - theme::SPACE_SM).max(80.0)),
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

    // ── Repository section (shown when relevant) ─────────────────────────
    if needs_repo {
        ui.label(theme::field_label("GitHub URL"));
        ui.add(
            egui::TextEdit::singleline(&mut mgr.side_panel_form.github_url)
                .hint_text("owner/repo or https://github.com/owner/repo")
                .desired_width(avail_w.max(80.0)),
        );
        ui.label(theme::muted_label("Required for update checks and GitHub installs.").small());
        ui.add_space(theme::SPACE_MD);
    }
    // For LocalOnly we hide the GitHub URL field entirely to reduce noise.
    // The field value is preserved in state if the user switches back.

    // ── Launch section ───────────────────────────────────────────────────
    ui.label(theme::section_heading("Launch"));
    ui.add_space(theme::SPACE_XS);
    ui.label(theme::field_label("Launch Args (optional)"));
    ui.add(
        egui::TextEdit::singleline(&mut mgr.side_panel_form.launch_args)
            .hint_text("Optional arguments passed at launch")
            .desired_width(avail_w.max(80.0)),
    );
    ui.add_space(theme::SPACE_SM);
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

/// Draw the accent left-rail using the panel's available rect.
/// Uses a safe negative-x offset that works at any inner_margin size.
fn draw_accent_rail(ui: &mut egui::Ui) {
    let max = ui.max_rect();
    if !max.is_finite() || max.width() <= 0.0 || max.height() <= 0.0 {
        return;
    }
    // The panel_frame has inner_margin of SPACE_LG (16px) on the left.
    // We paint a 3px rail just inside the left edge of the full panel rect.
    // This avoids relying on exact margin values by anchoring to max_rect's left.
    let rail_rect = egui::Rect::from_min_max(
        egui::pos2(max.left() - 10.0, max.top()),
        egui::pos2(max.left() - 7.0, max.bottom()),
    );
    if rail_rect.is_finite() && rail_rect.is_positive() {
        ui.painter().rect_filled(rail_rect, 0.0, theme::ACCENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_empty_display_name() {
        let form = SidePanelFormState {
            display_name: "   ".into(),
            mode: SidePanelMode::LocalOnly,
            local_path: "/path/to/app.AppImage".into(),
            ..Default::default()
        };
        assert_eq!(validation_message(&form), Some("Display Name is required."));
    }

    #[test]
    fn test_validation_local_only() {
        let mut form = SidePanelFormState {
            display_name: "MyApp".into(),
            mode: SidePanelMode::LocalOnly,
            local_path: "".into(),
            ..Default::default()
        };
        assert_eq!(
            validation_message(&form),
            Some("AppImage path is required for Local Only.")
        );

        form.local_path = "/tmp/my.AppImage".into();
        assert_eq!(validation_message(&form), None);
    }

    #[test]
    fn test_validation_local_with_repo() {
        let mut form = SidePanelFormState {
            display_name: "MyApp".into(),
            mode: SidePanelMode::LocalWithRepo,
            local_path: "".into(),
            github_url: "".into(),
            ..Default::default()
        };
        assert_eq!(
            validation_message(&form),
            Some("AppImage path is required for Local + Repo.")
        );

        form.local_path = "/tmp/my.AppImage".into();
        assert_eq!(
            validation_message(&form),
            Some("GitHub URL is required for Local + Repo.")
        );

        form.github_url = "owner/repo".into();
        assert_eq!(validation_message(&form), None);
    }

    #[test]
    fn test_validation_github_only() {
        let mut form = SidePanelFormState {
            display_name: "MyApp".into(),
            mode: SidePanelMode::GithubOnly,
            github_url: "".into(),
            ..Default::default()
        };
        assert_eq!(
            validation_message(&form),
            Some("GitHub URL is required for GitHub Only.")
        );

        form.github_url = "https://github.com/owner/repo".into();
        assert_eq!(validation_message(&form), None);
    }
}
