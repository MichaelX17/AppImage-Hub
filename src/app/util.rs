//! Shared helpers for actions, rendering, and the backend.
use super::*;

pub(crate) fn current_appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| std::env::current_exe().ok())
}

pub(crate) fn hub_update_is_available(remote_commit: &str, remote_tag: &str) -> bool {
    hub_update_available_with(
        env!("APPIMAGE_HUB_COMMIT"),
        remote_commit,
        env!("CARGO_PKG_VERSION"),
        remote_tag,
    )
}

pub(crate) fn hub_update_available_with(
    local_commit: &str,
    remote_commit: &str,
    current_version: &str,
    remote_tag: &str,
) -> bool {
    let local_commit = local_commit.trim();
    if !local_commit.is_empty() {
        return !local_commit.eq_ignore_ascii_case(remote_commit.trim());
    }

    // Source builds without git metadata fall back to the package version.
    let remote = remote_tag.trim().trim_start_matches('v');
    remote != current_version.trim()
}

pub(crate) fn check_updates_button(
    ui: &mut egui::Ui,
    progress: Option<f32>,
    enabled: bool,
) -> egui::Response {
    progress_button(
        ui,
        "Check All Updates",
        "Checking updates…",
        progress,
        enabled,
        176.0,
    )
}

pub(crate) fn progress_button(
    ui: &mut egui::Ui,
    idle_label: &str,
    active_label: &str,
    progress: Option<f32>,
    enabled: bool,
    width: f32,
) -> egui::Response {
    let text = match progress {
        Some(fraction) => format!(
            "{active_label} {}%",
            (fraction.clamp(0.0, 1.0) * 100.0).round() as i32
        ),
        None => idle_label.to_string(),
    };
    let desired = egui::vec2(width, ui.spacing().interact_size.y.max(32.0));
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired, sense);

    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let fill = if enabled && response.hovered() {
        crate::ui::theme::SURFACE_HOVER
    } else {
        crate::ui::theme::NEUTRAL
    };
    let stroke = if enabled && response.hovered() {
        crate::ui::theme::ACCENT_HOVER
    } else {
        crate::ui::theme::with_alpha(crate::ui::theme::BORDER, 180)
    };
    let radius = egui::CornerRadius::same(crate::ui::theme::RADIUS_PILL);

    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );

    if let Some(fraction) = progress {
        let fraction = fraction.clamp(0.0, 1.0);
        if fraction > 0.0 {
            let progress_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + rect.width() * fraction, rect.bottom()),
            );
            ui.painter().with_clip_rect(progress_rect).rect_filled(
                rect,
                radius,
                crate::ui::theme::ACCENT,
            );
        }
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        crate::ui::theme::TEXT_PRIMARY,
    );

    response
}

pub(crate) fn resolve_final_icon(
    user_icon_path: Option<PathBuf>,
    managed_appimage: &std::path::Path,
    target_folder: &std::path::Path,
) -> Option<PathBuf> {
    if let Some(user_icon) = user_icon_path {
        if user_icon.exists() {
            if user_icon.parent() == Some(target_folder) {
                return Some(user_icon);
            }
            let ext = user_icon
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let dest = target_folder.join(format!("icon.{}", ext));
            if std::fs::copy(&user_icon, &dest).is_ok() {
                return Some(dest);
            }
            return Some(user_icon);
        }
    }

    let p_png = target_folder.join("icon.png");
    if p_png.exists() {
        return Some(p_png);
    }
    let p_svg = target_folder.join("icon.svg");
    if p_svg.exists() {
        return Some(p_svg);
    }

    crate::appimage::extract_icon_with_policy(
        managed_appimage,
        target_folder,
        crate::appimage::ExtractionPolicy::NeverExecute,
    )
    .ok()
}

pub(crate) fn collect_appimages(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_appimages(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
        {
            out.push(path);
        }
    }
}

pub(crate) fn app_entry_from_appimage(path: &Path) -> AppEntry {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let display_name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "AppImage".to_string());
    let icon_path = [folder.join("icon.png"), folder.join("icon.svg")]
        .into_iter()
        .find(|icon| icon.exists());

    AppEntry {
        id: Uuid::new_v4().to_string(),
        display_name,
        generic_name: None,
        comment: None,
        categories: None,
        startup_notify: true,
        source: AppSource::LocalOnly,
        local_path: path.to_path_buf(),
        current_version: String::new(),
        args: Vec::new(),
        icon_path,
        asset_preference: None,
        added_at: Some(
            path.metadata()
                .ok()
                .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_else(now_unix),
        ),
        commit_hash: None,
        file_fingerprint: crate::config::file_fingerprint(path),
        desktop_shortcut: true,
        menu_entry: true,
        mime_types: None,
        exec_arg: None,
    }
}

pub(crate) fn apply_app_shortcuts(entry: &AppEntry) -> anyhow::Result<()> {
    let desktop = xdg::desktop_dir()?;
    let apps = xdg::applications_dir()?;
    if entry.desktop_shortcut {
        desktop_entry::write_desktop_shortcut(entry, &desktop)?;
    } else {
        let _ = desktop_entry::remove_desktop_shortcut(entry, &desktop);
    }
    if entry.menu_entry {
        desktop_entry::write_menu_entry(entry, &apps)?;
    } else {
        let _ = desktop_entry::remove_menu_entry(entry, &apps);
    }
    Ok(())
}

/// Refresh desktop/menu entries only when they already exist. Used by update,
/// migration, and folder-scan paths so they never create new shortcuts.
pub(crate) fn refresh_existing_shortcuts(entry: &AppEntry) {
    if let Ok(desktop_dir) = xdg::desktop_dir() {
        let path = desktop_entry::desktop_shortcut_path(entry, &desktop_dir);
        if path.exists() {
            let _ = desktop_entry::write_desktop_shortcut(entry, &desktop_dir);
        }
    }
    if let Ok(apps_dir) = xdg::applications_dir() {
        let path = desktop_entry::menu_entry_path(entry, &apps_dir);
        if path.exists() {
            let _ = desktop_entry::write_menu_entry(entry, &apps_dir);
        }
    }
}

pub(crate) fn ensure_in_managed_folder(
    source_path: &std::path::Path,
    display_name: &str,
    id: &str,
    custom_base: Option<&std::path::Path>,
) -> anyhow::Result<PathBuf> {
    let folder = if let Some(base) = custom_base {
        xdg::managed_app_folder_in(base, display_name, id)?
    } else {
        xdg::managed_app_folder(display_name, id)?
    };
    // Always store under the clean, user-typed name (`example.appimage`)
    // rather than the upstream asset name (`example_v2.4_x86_64.AppImage`).
    let fallback = source_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let filename = xdg::managed_appimage_filename(display_name, &fallback);
    let dest = folder.join(filename);

    if source_path == dest {
        return Ok(dest);
    }

    if std::fs::rename(source_path, &dest).is_err() {
        std::fs::copy(source_path, &dest)?;
        let _ = std::fs::remove_file(source_path);
    }
    Ok(dest)
}

/// Clean target path for a binary that already lives in a managed app folder.
/// Returns `None` when the name is already correct.
pub(crate) fn clean_managed_path(local_path: &Path, display_name: &str) -> Option<PathBuf> {
    let parent = local_path.parent()?;
    let desired = parent.join(xdg::managed_appimage_filename(display_name, "app"));
    if desired == local_path {
        None
    } else {
        Some(desired)
    }
}

pub(crate) fn ensure_trailing_semicolon(category: &str) -> String {
    let cat = category.trim();
    if cat.ends_with(';') {
        cat.to_string()
    } else {
        format!("{cat};")
    }
}

/// Applies an XDG category list detected from AppImage/sidecar metadata,
/// using the same primary-category mapping as the embedded-desktop reader.
pub(crate) fn apply_detected_category(
    form: &mut crate::ui::side_panel::SidePanelFormState,
    raw: &str,
) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    let (category, _is_custom) = crate::appimage::map_kde_category(raw);
    if !form.category_user_selected {
        form.detected_category = Some(category);
    }
}

/// Category value submitted for the current form. GitHub-only installs return
/// `None` unless the user made an explicit category choice, so the downloaded
/// AppImage's internal metadata can supply the category.
pub(crate) fn submitted_categories(
    form: &crate::ui::side_panel::SidePanelFormState,
) -> Option<String> {
    if form.category_user_selected {
        return Some(ensure_trailing_semicolon(&form.effective_category()));
    }

    if matches!(
        form.mode,
        crate::ui::side_panel::SidePanelMode::LocalOnly
            | crate::ui::side_panel::SidePanelMode::LocalWithRepo
    ) {
        return Some(
            form.detected_category
                .as_deref()
                .map(ensure_trailing_semicolon)
                .unwrap_or_else(|| "Utility;".to_string()),
        );
    }

    // GitHub-only installs resolve the category from the downloaded AppImage.
    None
}

/// Resolves the `Categories=` value written to the menu entry.
///
/// The category the user picked in the Add/Edit form always wins; the
/// AppImage's own metadata is only a fallback, and `Utility;` is the last
/// resort so a valid `.desktop` file is always produced.
pub(crate) fn resolve_categories(
    user_selected: Option<String>,
    meta: Option<&crate::appimage::AppImageMetadata>,
) -> Option<String> {
    if let Some(cat) = user_selected
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        return Some(ensure_trailing_semicolon(cat));
    }

    if let Some(meta) = meta {
        // Prefer the KDE-mapped primary category (e.g. "AudioVideo") over the
        // raw XDG list, since Plasma places apps by the first known category.
        if let Some(mapped) = meta
            .matched_category
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            return Some(ensure_trailing_semicolon(mapped));
        }
        if let Some(raw) = meta
            .categories
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            return Some(ensure_trailing_semicolon(raw));
        }
    }

    Some("Utility;".to_string())
}
