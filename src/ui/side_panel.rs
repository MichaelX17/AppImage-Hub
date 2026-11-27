//! Add / Edit AppImage side panel (neon/dark redesign).
//!
//! Layout: header (icon + title + Hide/Close), scrollable form with
//! field-level validation, fixed footer with Cancel/Save.

use crate::app::ManagerApp;
use crate::ui::breakpoints::SizeClass;
use crate::ui::icons::{self, Icon};
use crate::ui::{fonts, theme};
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Margin, RichText, Stroke};
use std::path::Path;

const FORM_INPUT_HEIGHT: f32 = 40.0;

fn form_text_input(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    ui.add_sized(
        [width, FORM_INPUT_HEIGHT],
        egui::TextEdit::singleline(value)
            .font(fonts::regular(13.0))
            .hint_text(
                RichText::new(hint)
                    .font(fonts::regular(13.0))
                    .color(theme::with_alpha(theme::text_muted(), 180)),
            )
            .margin(Margin::symmetric(12, 9))
            .vertical_align(Align::Center)
            .desired_width(width),
    )
}

fn form_primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .font(fonts::bold(14.0))
            .color(theme::on_accent()),
    )
    .fill(theme::accent())
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
}

fn form_secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .font(fonts::medium(13.0))
            .color(theme::text_primary()),
    )
    .fill(theme::surface_raised())
    .stroke(Stroke::new(1.0_f32, theme::border()))
    .corner_radius(CornerRadius::same(theme::radius_md()))
}

fn form_ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .font(fonts::medium(13.0))
            .color(theme::text_secondary()),
    )
    .fill(Color32::TRANSPARENT)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
}

fn toggle_row(ui: &mut egui::Ui, value: &mut bool, label: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(fonts::medium(13.0))
                .color(theme::text_primary()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            theme::toggle(ui, value);
        });
    });
}

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

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetSelectionMode {
    #[default]
    Smart,
    Manual,
}

/// Maximum length accepted by the "Display Name" input.
pub const DISPLAY_NAME_LIMIT: usize = 100;

#[derive(Clone)]
pub struct SidePanelFormState {
    pub editing_app_id: Option<String>,
    pub display_name: String,
    pub generic_name: String,
    pub comment: String,
    pub category: String,
    pub custom_category: String,
    /// Set once the user explicitly picks a category, so metadata
    /// auto-detection can never overwrite their choice.
    pub category_user_selected: bool,
    /// Category detected from the selected AppImage/sidecar. Kept separate so
    /// the selector can remain on its Utility default until the user chooses.
    pub detected_category: Option<String>,
    pub startup_notify: bool,
    /// Window class written as `StartupWMClass=` in generated shortcuts.
    pub startup_wm_class: String,
    pub desktop_shortcut: bool,
    pub menu_entry: bool,
    pub mime_types: String,
    pub exec_arg: String,
    pub mode: SidePanelMode,
    pub local_path: String,
    pub icon_path: String,
    pub github_url: String,
    /// Display buffer for launch arguments — not the source of truth. The
    /// stored value is the `Vec<String>` in `AppEntry.args`; `args::split_args`
    /// parses this buffer when the form is submitted and `args::join_args`
    /// renders it when editing.
    pub launch_args: String,
    pub asset_selection_mode: AssetSelectionMode,
    pub manual_asset: Option<String>,
    pub available_assets: Vec<crate::github::GithubAssetInfo>,
    pub is_fetching_assets: bool,
    pub fetch_assets_error: Option<String>,
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
            category_user_selected: false,
            detected_category: None,
            startup_notify: true,
            startup_wm_class: String::new(),
            desktop_shortcut: true,
            menu_entry: true,
            mime_types: String::new(),
            exec_arg: String::new(),
            mode: SidePanelMode::default(),
            local_path: String::new(),
            icon_path: String::new(),
            github_url: String::new(),
            launch_args: String::new(),
            asset_selection_mode: AssetSelectionMode::default(),
            manual_asset: None,
            available_assets: Vec::new(),
            is_fetching_assets: false,
            fetch_assets_error: None,
        }
    }
}

impl SidePanelFormState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The category the user actually selected, always non-empty and without a
    /// trailing `;` (callers append it when writing `Categories=`).
    pub fn effective_category(&self) -> String {
        let chosen = if self.category == "Custom" {
            self.custom_category.trim()
        } else {
            self.category.trim()
        };
        let chosen = chosen.trim_end_matches(';').trim();
        if chosen.is_empty() {
            "Utility".to_string()
        } else {
            chosen.to_string()
        }
    }
}

pub fn render(ui: &mut egui::Ui, mgr: &mut ManagerApp, _size_class: SizeClass) {
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

    let mut close_clicked = false;
    theme::panel_frame().show(ui, |ui| {
        ui.set_min_height(ui.available_height());
        if render_header(ui, title, is_edit) {
            close_clicked = true;
        }

        ui.add_space(theme::SPACE_MD);
        theme::divider(ui);
        ui.add_space(theme::SPACE_MD);

        // Reserve enough room for the validation row, primary action, and the
        // secondary Cancel action at the bottom of the panel.
        let footer_height = 122.0;
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
                render_form_fields(ui, mgr);
            });

        ui.add_space(theme::SPACE_SM);

        let validation = validation_message(&mgr.side_panel_form);
        let can_save = validation.is_none();

        if let Some(msg) = validation {
            ui.label(RichText::new(msg).color(theme::danger()).size(12.0));
        } else {
            ui.label(theme::muted_label("All required fields are complete.").size(11.0));
        }

        ui.add_space(theme::SPACE_SM);

        let button_width = ui.available_width();
        ui.add_enabled_ui(can_save, |ui| {
            let label = if is_edit {
                "Save changes"
            } else {
                "Add AppImage"
            };
            let btn = ui.add_sized(
                [button_width, FORM_INPUT_HEIGHT + 4.0],
                form_primary_button(label),
            );
            if !can_save {
                btn.on_disabled_hover_text("Complete required fields to save");
            } else if btn.clicked() {
                mgr.submit_side_panel_form();
            }
        });

        ui.add_space(theme::SPACE_XS);
        if ui
            .add_sized([button_width, 34.0], form_ghost_button("Cancel"))
            .clicked()
        {
            close_editor(mgr);
        }
    });
    if close_clicked {
        close_editor(mgr);
    }
}

fn render_header(ui: &mut egui::Ui, title: &str, is_edit: bool) -> bool {
    let mut closed = false;
    ui.horizontal(|ui| {
        icons::widget(
            ui,
            if is_edit { Icon::Edit } else { Icon::Plus },
            20.0,
            theme::accent(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(title)
                .font(fonts::bold(21.0))
                .color(theme::text_primary()),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(ui, Icon::Close, 28.0, false, "Close (Esc)").clicked() {
                closed = true;
            }
        });
    });
    ui.add_space(2.0);
    ui.label(
        theme::secondary_label("Configure the AppImage source, metadata, and launch options.")
            .font(fonts::regular(12.5)),
    );
    closed
}

fn close_editor(mgr: &mut ManagerApp) {
    mgr.is_form_open = false;
    mgr.show_side_panel = false;
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

    let mut close_clicked = false;
    theme::panel_frame().show(ui, |ui| {
        ui.set_min_height(ui.available_height());

        ui.horizontal(|ui| {
            icons::widget(ui, Icon::Link, 20.0, theme::accent());
            ui.add_space(4.0);
            ui.label(
                RichText::new("Manage Shortcuts")
                    .font(fonts::bold(21.0))
                    .color(theme::text_primary()),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(ui, Icon::Close, 28.0, false, "Close (Esc)").clicked() {
                    close_clicked = true;
                }
            });
        });
        ui.add_space(2.0);
        ui.label(
            theme::secondary_label(format!(
                "Choose where to create shortcuts for {}.",
                state.app_name
            ))
            .font(fonts::regular(12.5)),
        );

        ui.add_space(theme::SPACE_MD);
        theme::divider(ui);
        ui.add_space(theme::SPACE_MD);
        ui.label(theme::section_heading("Targets"));
        ui.add_space(theme::SPACE_SM);

        let desktop_path = state.desktop_path.clone();
        let menu_path = state.menu_path.clone();

        if let Some(p) = &desktop_path {
            shortcut_target(ui, "Desktop shortcut", p);
            ui.add_space(6.0);
        }
        if let Some(p) = &menu_path {
            shortcut_target(ui, "Start menu entry", p);
        }

        ui.add_space(theme::SPACE_LG);
        ui.label(theme::section_heading("Shortcuts"));
        ui.add_space(theme::SPACE_SM);
        toggle_row(ui, &mut state.desktop, "Desktop shortcut");
        ui.add_space(theme::SPACE_SM);
        toggle_row(ui, &mut state.menu, "Start menu entry");
        ui.add_space(theme::SPACE_SM);
        ui.label(
            theme::muted_label("Enabled = create/keep. Disabled = remove if present.")
                .font(fonts::regular(11.5)),
        );

        mgr.add_to = Some(state);

        ui.add_space((ui.available_height() - 94.0).max(theme::SPACE_LG));
        let button_width = ui.available_width();
        if ui
            .add_sized(
                [button_width, FORM_INPUT_HEIGHT + 2.0],
                form_primary_button("Apply Shortcuts"),
            )
            .clicked()
        {
            mgr.apply_add_to();
        }
        ui.add_space(theme::SPACE_XS);
        if ui
            .add_sized([button_width, 34.0], form_ghost_button("Cancel"))
            .clicked()
        {
            mgr.close_add_to();
        }
    });
    if close_clicked {
        mgr.close_add_to();
    }
}

fn shortcut_target(ui: &mut egui::Ui, label: &str, path: &Path) {
    egui::Frame::new()
        .fill(theme::surface_raised())
        .stroke(Stroke::new(1.0_f32, theme::border()))
        .corner_radius(CornerRadius::same(theme::radius_md()))
        .inner_margin(Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(theme::field_label(label));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if path.exists() {
                        ui.label(
                            RichText::new("exists")
                                .size(10.0)
                                .strong()
                                .color(theme::detail()),
                        );
                    }
                });
            });
            ui.add(
                egui::Label::new(theme::muted_label(path.display().to_string()).size(11.0))
                    .truncate(),
            )
            .on_hover_text(path.display().to_string());
        });
}

fn render_form_fields(ui: &mut egui::Ui, mgr: &mut ManagerApp) {
    let width = ui.available_width().max(120.0);
    ui.spacing_mut().interact_size.y = FORM_INPUT_HEIGHT;
    let touched = form_touched(&mgr.side_panel_form);

    // ── Application section ──────────────────────────────────────────────
    ui.label(theme::section_heading(
        "Application (KDE / Start Menu Metadata)",
    ));
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Display Name"));
    ui.add_sized(
        [width, FORM_INPUT_HEIGHT],
        egui::TextEdit::singleline(&mut mgr.side_panel_form.display_name)
            .hint_text(
                RichText::new("Application name (e.g. PCSX2)")
                    .color(theme::with_alpha(theme::text_muted(), 180)),
            )
            .char_limit(DISPLAY_NAME_LIMIT)
            .margin(Margin::symmetric(12, 10))
            .vertical_align(Align::Center)
            .desired_width(width),
    );
    // Character counter, right-aligned below the input.
    // NOTE: uses a fixed-height row with `Align::Min` — `Align::Center` makes
    // egui's horizontal layout claim the full remaining height of the form,
    // which would leave a huge gap before the next field.
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 14.0),
        Layout::right_to_left(Align::Min),
        |ui| {
            let used = mgr.side_panel_form.display_name.chars().count();
            let color = if used >= DISPLAY_NAME_LIMIT {
                theme::warning()
            } else {
                theme::text_muted()
            };
            ui.label(
                RichText::new(format!("{used}/{DISPLAY_NAME_LIMIT}"))
                    .size(11.0)
                    .color(color),
            );
        },
    );
    required_hint(
        ui,
        touched && mgr.side_panel_form.display_name.trim().is_empty(),
        "Display Name is required.",
    );
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Generic Name (optional)"));
    form_text_input(
        ui,
        &mut mgr.side_panel_form.generic_name,
        "e.g. PlayStation 2 Emulator",
        width,
    );
    ui.label(
        theme::muted_label("Shown beside or under the name in KDE Application Launcher.")
            .size(11.0),
    );
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Comment / Description (optional)"));
    form_text_input(
        ui,
        &mut mgr.side_panel_form.comment,
        "e.g. A Sony PlayStation 2 video game console emulator",
        width,
    );
    ui.label(theme::muted_label("Tooltip description in KDE Start Menu.").size(11.0));
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
        .width(width.max(80.0))
        .show_ui(ui, |ui| {
            let mut categories: Vec<(&str, Option<&str>)> = KDE_CATEGORIES
                .iter()
                .map(|(label, value)| (*label, Some(*value)))
                .collect();
            categories.push(("Custom Category…", None));
            categories.sort_unstable_by(|a, b| a.0.cmp(b.0));

            for (label, value) in categories {
                let selected = match value {
                    Some(value) => mgr.side_panel_form.category == value,
                    None => mgr.side_panel_form.category == "Custom",
                };
                if ui.selectable_label(selected, label).clicked() {
                    mgr.side_panel_form.category = value.unwrap_or("Custom").to_string();
                    mgr.side_panel_form.category_user_selected = true;
                    mgr.side_panel_form.detected_category = None;
                }
            }
        });

    if mgr.side_panel_form.category == "Custom" {
        ui.add_space(theme::SPACE_XS);
        let custom_before = mgr.side_panel_form.custom_category.clone();
        form_text_input(
            ui,
            &mut mgr.side_panel_form.custom_category,
            "e.g. Game, Utility, AudioVideo",
            width,
        );
        if mgr.side_panel_form.custom_category != custom_before {
            mgr.side_panel_form.category_user_selected = true;
        }
    }
    ui.label(
        theme::muted_label("Folder in the KDE Start Menu tree where the app will appear.")
            .size(11.0),
    );
    ui.add_space(theme::SPACE_SM);

    toggle_row(
        ui,
        &mut mgr.side_panel_form.startup_notify,
        "Enable launch feedback (StartupNotify)",
    );
    ui.label(
        theme::muted_label("Shows launch animation/busy cursor in Plasma session.").size(11.0),
    );

    ui.add_space(theme::SPACE_SM);
    ui.label(theme::field_label(
        "Window class / StartupWMClass (optional)",
    ));
    form_text_input(
        ui,
        &mut mgr.side_panel_form.startup_wm_class,
        "e.g. pcsx2, heroic, code",
        width,
    );
    ui.label(
        theme::muted_label(
            "Keeps the running app and its pinned shortcut in one task manager icon. \
             Detected from the AppImage; change it if a second icon appears.",
        )
        .size(11.0),
    );

    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Shortcuts"));
    ui.add_space(theme::SPACE_SM);
    toggle_row(
        ui,
        &mut mgr.side_panel_form.desktop_shortcut,
        "Desktop shortcut",
    );
    ui.add_space(theme::SPACE_SM);
    toggle_row(ui, &mut mgr.side_panel_form.menu_entry, "Start menu entry");
    ui.add_space(theme::SPACE_SM);
    ui.label(theme::field_label("MIME types / URI schemes (optional)"));
    form_text_input(
        ui,
        &mut mgr.side_panel_form.mime_types,
        "e.g. x-scheme-handler/heroic;application/x-iso9660-image;",
        width,
    );
    ui.label(
        theme::muted_label(
            "Detected from the AppImage's internal .desktop file. Separate entries with ';'.",
        )
        .size(11.0),
    );
    ui.add_space(theme::SPACE_SM);
    ui.label(theme::field_label("Open argument placeholder (optional)"));
    form_text_input(
        ui,
        &mut mgr.side_panel_form.exec_arg,
        "e.g. %U, %u, %F, or %f",
        width,
    );
    ui.label(
        theme::muted_label(
            "Passed to Exec when opening URLs/files. Detected automatically when possible.",
        )
        .size(11.0),
    );
    ui.add_space(theme::SPACE_LG);
    // ── Source section ───────────────────────────────────────────────────
    ui.label(theme::section_heading("Source"));
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Source Type"));
    let mode_index = match mgr.side_panel_form.mode {
        SidePanelMode::LocalOnly => 0,
        SidePanelMode::LocalWithRepo => 1,
        SidePanelMode::GithubOnly => 2,
    };
    if let Some(index) = segmented_row(
        ui,
        &["Local Only", "Local + Repo", "GitHub Only"],
        mode_index,
    ) {
        mgr.side_panel_form.mode = match index {
            0 => SidePanelMode::LocalOnly,
            1 => SidePanelMode::LocalWithRepo,
            _ => SidePanelMode::GithubOnly,
        };
    }
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
        let path_editable = mgr.side_panel_form.editing_app_id.is_none();
        ui.horizontal(|ui| {
            let browse_w = 96.0;
            let spacing = ui.spacing().item_spacing.x;
            let input_w = (ui.available_width() - browse_w - spacing).max(100.0);
            ui.add_enabled_ui(path_editable, |ui| {
                form_text_input(
                    ui,
                    &mut mgr.side_panel_form.local_path,
                    "Path to .AppImage file",
                    input_w,
                );
            });
            if ui
                .add_enabled(path_editable, form_secondary_button("Browse"))
                .on_disabled_hover_text("Managed path is fixed for existing entries")
                .clicked()
            {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("AppImage", &["AppImage", "appimage"])
                    .pick_file()
                {
                    mgr.side_panel_form.local_path = p.display().to_string();
                    mgr.auto_populate_from_appimage(&p);
                }
            }
        });
        if mgr.side_panel_form.editing_app_id.is_some() {
            ui.label(theme::muted_label("Managed path is fixed for existing entries.").size(11.0));
        } else {
            ui.label(
                theme::muted_label("Select the local .AppImage to import or track.").size(11.0),
            );
            ui.label(
                theme::muted_label(
                    "A local file may be run once (--appimage-extract) to read its metadata.",
                )
                .size(11.0),
            );
        }
        required_hint(
            ui,
            touched && mgr.side_panel_form.local_path.trim().is_empty(),
            "An AppImage file is required.",
        );
        ui.add_space(theme::SPACE_MD);
    } else {
        ui.label(
            theme::muted_label("AppImage will be downloaded and managed after install.").size(11.0),
        );
        ui.label(
            theme::muted_label(
                "Downloaded AppImages are not executed for metadata; if no non-executing \
                 extractor is available, fill these fields manually.",
            )
            .size(11.0),
        );
        ui.add_space(theme::SPACE_MD);
    }
    // ── Metadata section ─────────────────────────────────────────────────
    ui.add_space(theme::SPACE_LG);
    ui.label(theme::section_heading("Metadata"));
    ui.add_space(theme::SPACE_SM);

    ui.label(theme::field_label("Icon (optional)"));
    ui.horizontal(|ui| {
        let current_icon = mgr.side_panel_form.icon_path.trim().to_string();
        if !current_icon.is_empty() {
            let icon_p = std::path::Path::new(&current_icon);
            if icon_p.exists() {
                let uri = format!("file://{}", icon_p.display());
                ui.add(
                    egui::Image::new(uri)
                        .fit_to_exact_size(egui::vec2(26.0, 26.0))
                        .corner_radius(CornerRadius::same(theme::radius_sm())),
                );
                ui.add_space(theme::SPACE_XS);
            }
        }

        let browse_w = 96.0;
        let spacing = ui.spacing().item_spacing.x;
        let input_w = (ui.available_width() - browse_w - spacing).max(100.0);
        form_text_input(
            ui,
            &mut mgr.side_panel_form.icon_path,
            "Extracted from AppImage automatically",
            input_w,
        );
        if ui.add(form_secondary_button("Browse")).clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Image", &["png", "svg", "xpm", "ico"])
                .pick_file()
            {
                mgr.side_panel_form.icon_path = p.display().to_string();
            }
        }
    });
    ui.label(
        theme::muted_label("Embedded AppImage icon is extracted automatically if left empty.")
            .size(11.0),
    );
    ui.add_space(theme::SPACE_MD);

    // ── Repository section (shown when relevant) ─────────────────────────
    if needs_repo {
        ui.add_space(theme::SPACE_LG);
        ui.label(theme::field_label("GitHub URL"));
        form_text_input(
            ui,
            &mut mgr.side_panel_form.github_url,
            "owner/repo or https://github.com/owner/repo",
            width,
        );
        ui.label(theme::muted_label("Required for update checks and GitHub installs.").size(11.0));
        required_hint(
            ui,
            touched && mgr.side_panel_form.github_url.trim().is_empty(),
            "Repository is required (owner/repo).",
        );
        ui.add_space(theme::SPACE_SM);

        ui.label(theme::field_label("Asset Selection Mode"));
        let asset_index = match mgr.side_panel_form.asset_selection_mode {
            AssetSelectionMode::Smart => 0,
            AssetSelectionMode::Manual => 1,
        };
        if let Some(index) = segmented_row(ui, &["Smart (Auto)", "Manual"], asset_index) {
            mgr.side_panel_form.asset_selection_mode = if index == 0 {
                AssetSelectionMode::Smart
            } else {
                AssetSelectionMode::Manual
            };
        }
        ui.add_space(theme::SPACE_SM);
        match mgr.side_panel_form.asset_selection_mode {
            AssetSelectionMode::Smart => {
                ui.label(
                    theme::muted_label(
                        "Auto-detects the optimal AppImage for your CPU architecture (x86_64/amd64) and excludes hashes/zsync.",
                    )
                    .size(11.0),
                );
            }
            AssetSelectionMode::Manual => {
                ui.horizontal(|ui| {
                    let fetch_btn = ui.add(form_secondary_button("Fetch Assets"));
                    if fetch_btn.clicked() {
                        let repo = mgr.side_panel_form.github_url.trim().to_string();
                        if !repo.is_empty() {
                            mgr.side_panel_form.is_fetching_assets = true;
                            mgr.side_panel_form.fetch_assets_error = None;
                            mgr.send_cmd(crate::app::BackendCommand::FetchRepoAssets { repo });
                        }
                    }
                    if mgr.side_panel_form.is_fetching_assets {
                        ui.spinner();
                        ui.label(theme::muted_label("Fetching assets...").size(11.0));
                    }
                });

                if let Some(ref err) = mgr.side_panel_form.fetch_assets_error {
                    ui.label(RichText::new(err).color(theme::danger()).size(11.0));
                }

                if !mgr.side_panel_form.available_assets.is_empty() {
                    let selected_text = mgr
                        .side_panel_form
                        .manual_asset
                        .as_deref()
                        .unwrap_or("Select an AppImage asset…");
                    egui::ComboBox::from_id_salt("manual_asset_combo")
                        .selected_text(selected_text)
                        .width(width.max(80.0))
                        .show_ui(ui, |ui| {
                            for asset in &mgr.side_panel_form.available_assets {
                                let mb = asset.size as f64 / (1024.0 * 1024.0);
                                let label = format!("{} ({:.1} MB)", asset.name, mb);
                                let is_selected = mgr.side_panel_form.manual_asset.as_deref()
                                    == Some(&asset.name);
                                if ui.selectable_label(is_selected, label).clicked() {
                                    mgr.side_panel_form.manual_asset = Some(asset.name.clone());
                                }
                            }
                        });
                } else if !mgr.side_panel_form.is_fetching_assets
                    && mgr.side_panel_form.fetch_assets_error.is_none()
                {
                    if let Some(ref current) = mgr.side_panel_form.manual_asset {
                        ui.label(
                            theme::muted_label(format!("Tracking asset: {current}")).size(11.0),
                        );
                    } else {
                        ui.label(
                            theme::muted_label(
                                "Click 'Fetch Assets' to choose from available .AppImages.",
                            )
                            .size(11.0),
                        );
                    }
                }

                required_hint(
                    ui,
                    touched
                        && mgr.side_panel_form.asset_selection_mode == AssetSelectionMode::Manual
                        && mgr.side_panel_form.manual_asset.is_none(),
                    "Select an AppImage asset (or switch to Smart mode).",
                );
            }
        }
        ui.add_space(theme::SPACE_MD);
    }
    // For LocalOnly we hide the GitHub URL field entirely to reduce noise.
    // The field value is preserved in state if the user switches back.

    // ── Launch section ───────────────────────────────────────────────────
    ui.add_space(theme::SPACE_LG);
    egui::CollapsingHeader::new(
        RichText::new("Launch Args")
            .strong()
            .color(theme::text_primary()),
    )
    .id_salt("launch_args_accordion")
    .default_open(false)
    .show(ui, |ui| {
        ui.label(
            theme::muted_label("Optional arguments passed to the AppImage at launch.").size(11.0),
        );
        ui.label(
            theme::muted_label(
                "Use quotes for arguments with spaces, e.g. --profile=\"My Profile\".",
            )
            .size(11.0),
        );
        ui.add_space(theme::SPACE_XS);
        form_text_input(
            ui,
            &mut mgr.side_panel_form.launch_args,
            "--fullscreen --foo=bar",
            ui.available_width().max(120.0),
        );
    });
    ui.add_space(theme::SPACE_SM);
}
/// True once the user has entered anything (or when editing). Field-level
/// errors stay hidden on a pristine "Add" form to avoid a wall of red.
fn form_touched(form: &SidePanelFormState) -> bool {
    form.editing_app_id.is_some()
        || [
            form.display_name.as_str(),
            form.generic_name.as_str(),
            form.comment.as_str(),
            form.local_path.as_str(),
            form.icon_path.as_str(),
            form.github_url.as_str(),
            form.launch_args.as_str(),
        ]
        .iter()
        .any(|value| !value.trim().is_empty())
}

/// Red inline validation message shown directly below the offending field.
fn required_hint(ui: &mut egui::Ui, show: bool, message: &str) {
    if show {
        ui.label(RichText::new(message).color(theme::danger()).size(11.0));
    }
}

/// Segmented control: a row of pill buttons, the active one filled with accent.
/// Returns the index the user clicked, if any.
fn segmented_row(ui: &mut egui::Ui, labels: &[&str], selected: usize) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    let gap = ui.spacing().item_spacing.x;
    let total = ui.available_width();
    let count = labels.len() as f32;
    let width = ((total - gap * (count - 1.0)) / count).max(64.0);
    let mut clicked = None;
    ui.horizontal(|ui| {
        for (index, label) in labels.iter().enumerate() {
            if ui
                .add_sized(
                    [width, FORM_INPUT_HEIGHT],
                    segment_button(label, index == selected),
                )
                .clicked()
            {
                clicked = Some(index);
            }
        }
    });
    clicked
}

fn segment_button(label: &str, selected: bool) -> egui::Button<'static> {
    let (fill, color, stroke) = if selected {
        (
            theme::accent(),
            theme::on_accent(),
            Stroke::NONE,
        )
    } else {
        (
            theme::surface_raised(),
            theme::text_secondary(),
            Stroke::new(1.0_f32, theme::border()),
        )
    };
    egui::Button::new(RichText::new(label).font(fonts::medium(12.5)).color(color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(theme::RADIUS_PILL))
}
fn validation_message(form: &SidePanelFormState) -> Option<&'static str> {
    if form.display_name.trim().is_empty() {
        return Some("Display Name is required.");
    }
    if crate::args::split_args(&form.launch_args).is_err() {
        return Some("Unterminated quote or escape in launch arguments.");
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
            } else if form.asset_selection_mode == AssetSelectionMode::Manual
                && form.manual_asset.is_none()
            {
                Some("Select an AppImage asset, or switch to Smart mode.")
            } else {
                None
            }
        }
    }
}

/// Draw the accent left-rail using the panel's available rect.
/// Uses a safe negative-x offset that works at any inner_margin size.
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
    fn test_validation_rejects_unterminated_launch_arg_quote() {
        let mut form = SidePanelFormState {
            display_name: "MyApp".into(),
            mode: SidePanelMode::LocalOnly,
            local_path: "/tmp/my.AppImage".into(),
            launch_args: "a \"b c".into(),
            ..Default::default()
        };
        assert_eq!(
            validation_message(&form),
            Some("Unterminated quote or escape in launch arguments.")
        );

        form.launch_args = "a \"b c\"".into();
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

    #[test]
    fn test_pristine_form_is_not_touched() {
        assert!(!form_touched(&SidePanelFormState::default()));
    }

    #[test]
    fn test_typing_marks_form_touched() {
        let form = SidePanelFormState {
            display_name: "A".into(),
            ..Default::default()
        };
        assert!(form_touched(&form));
    }

    #[test]
    fn effective_category_returns_standard_selection() {
        let form = SidePanelFormState {
            category: "Graphics".into(),
            ..Default::default()
        };
        assert_eq!(form.effective_category(), "Graphics");
    }

    #[test]
    fn effective_category_uses_custom_value() {
        let form = SidePanelFormState {
            category: "Custom".into(),
            custom_category: "  Game;Emulator;  ".into(),
            ..Default::default()
        };
        assert_eq!(form.effective_category(), "Game;Emulator");
    }

    #[test]
    fn effective_category_never_returns_empty() {
        let form = SidePanelFormState {
            category: "Custom".into(),
            custom_category: "   ".into(),
            ..Default::default()
        };
        assert_eq!(form.effective_category(), "Utility");
    }

    #[test]
    fn category_selection_flag_defaults_to_false() {
        assert!(!SidePanelFormState::default().category_user_selected);
    }
}
