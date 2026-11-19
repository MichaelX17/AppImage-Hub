use crate::app::ManagerApp;
use crate::ui::theme;
use eframe::egui::{self, Align, Margin, RichText};

#[derive(Clone)]
pub struct SettingsFormState {
    pub apps_dir: String,
    pub auto_check_updates: bool,
    pub hub_start_menu_shortcut: bool,
    pub hub_start_menu_category: String,
    pub hub_desktop_shortcut: bool,
    pub github_token: String,
    pub show_token: bool,
}

impl Default for SettingsFormState {
    fn default() -> Self {
        Self {
            apps_dir: String::new(),
            auto_check_updates: true,
            hub_start_menu_shortcut: true,
            hub_start_menu_category: "Utility".to_string(),
            hub_desktop_shortcut: false,
            github_token: String::new(),
            show_token: false,
        }
    }
}

pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        mgr.close_settings();
        return;
    }

    theme::panel_frame().show(ui, |ui| {
        ui.set_min_height(ui.available_height());
        ui.label(
            RichText::new("Settings")
                .size(32.0)
                .strong()
                .color(theme::TEXT_PRIMARY),
        );
        ui.add_space(theme::SPACE_XS);
        ui.label(
            theme::secondary_label("Configure storage, update checks, and GitHub access.")
                .size(13.0),
        );

        ui.add_space(theme::SPACE_LG);
        let footer_height = 104.0;
        let available = ui.available_height();
        let form_height = (available - footer_height).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("settings_scroll")
            .auto_shrink([false, false])
            .max_height(form_height)
            .show(ui, |ui| {
                render_settings_fields(ui, mgr);
            });

        ui.add_space(theme::SPACE_SM);
        let button_width = ui.available_width();
        if ui
            .add_sized([button_width, 46.0], theme::primary_button("Save Settings"))
            .clicked()
        {
            mgr.save_settings();
        }
        ui.add_space(theme::SPACE_XS);
        if ui
            .add_sized([button_width, 34.0], theme::ghost_button("Cancel"))
            .clicked()
        {
            mgr.close_settings();
        }
    });
}

fn render_settings_fields(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    ui.label(theme::section_heading("Storage"));
    ui.add_space(theme::SPACE_SM);
    ui.label(theme::field_label("AppImage root folder"));
    ui.horizontal(|ui| {
        let browse_w = 96.0;
        let spacing = ui.spacing().item_spacing.x;
        let input_w = (ui.available_width() - browse_w - spacing).max(100.0);
        ui.add_sized(
            [input_w, 42.0],
            egui::TextEdit::singleline(&mut mgr.settings_form.apps_dir)
                .hint_text("Folder where AppImages are stored")
                .margin(Margin::symmetric(12, 10))
                .vertical_align(Align::Center)
                .desired_width(input_w),
        );
        if ui
            .add_sized([browse_w, 42.0], theme::secondary_button("Browse"))
            .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Choose AppImage storage folder")
                .pick_folder()
            {
                mgr.settings_form.apps_dir = path.display().to_string();
            }
        }
    });
    ui.label(
        theme::muted_label("New AppImages are stored in per-app folders inside this directory.")
            .size(11.0),
    );

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Updates"));
    ui.add_space(theme::SPACE_SM);
    ui.checkbox(
        &mut mgr.settings_form.auto_check_updates,
        "Automatically check for updates on startup",
    );
    let cooldown_text = if mgr.update_check_cooldown() <= 2 * 60 {
        "Update checks are rate-limited to once every 2 minutes (GitHub token configured)."
    } else {
        "Update checks are rate-limited to once every 5 minutes. Add a GitHub token to shorten it to 2 minutes."
    };
    ui.label(theme::muted_label(cooldown_text).size(11.0));

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("AppImage Hub Shortcuts"));
    ui.add_space(theme::SPACE_SM);
    ui.checkbox(
        &mut mgr.settings_form.hub_start_menu_shortcut,
        "Create Start Menu Shortcut",
    );
    if mgr.settings_form.hub_start_menu_shortcut {
        ui.add_space(theme::SPACE_SM);
        ui.label(theme::field_label("Start Menu Category"));
        let selected_label = crate::ui::side_panel::KDE_CATEGORIES
            .iter()
            .find(|(_, value)| *value == mgr.settings_form.hub_start_menu_category)
            .map(|(label, _)| *label)
            .unwrap_or("Utilities");
        egui::ComboBox::from_id_salt("hub_start_menu_category")
            .selected_text(selected_label)
            .width(ui.available_width().max(120.0))
            .show_ui(ui, |ui| {
                let mut categories = crate::ui::side_panel::KDE_CATEGORIES.to_vec();
                categories.sort_unstable_by(|a, b| a.0.cmp(b.0));
                for (label, value) in categories {
                    let selected = mgr.settings_form.hub_start_menu_category == value;
                    if ui.selectable_label(selected, label).clicked() {
                        mgr.settings_form.hub_start_menu_category = value.to_string();
                    }
                }
            });
    }
    ui.add_space(theme::SPACE_XS);
    ui.checkbox(
        &mut mgr.settings_form.hub_desktop_shortcut,
        "Create Desktop Shortcut",
    );
    ui.label(
        theme::muted_label(
            "Shortcut Exec paths are updated automatically from the current executable.",
        )
        .size(11.0),
    );

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("GitHub API"));
    ui.add_space(theme::SPACE_SM);
    ui.label(theme::field_label("Personal access token (optional)"));
    ui.add_sized(
        [ui.available_width(), 42.0],
        egui::TextEdit::singleline(&mut mgr.settings_form.github_token)
            .hint_text("ghp_…")
            .password(!mgr.settings_form.show_token)
            .margin(Margin::symmetric(12, 10))
            .vertical_align(Align::Center),
    );
    ui.checkbox(
        &mut mgr.settings_form.show_token,
        "Show token while editing",
    );
    ui.label(
        theme::muted_label(
            "Stored in the system keyring when available; otherwise encrypted with \
             Argon2id in a 0600 file under your config folder. Never written to \
             config.toml or beside a portable AppImage.",
        )
        .size(11.0),
    );
    ui.label(
        theme::muted_label(
            "A token raises the GitHub API limit from 60 to 5,000 requests per hour.",
        )
        .size(11.0),
    );
    ui.add_space(theme::SPACE_SM);
}
