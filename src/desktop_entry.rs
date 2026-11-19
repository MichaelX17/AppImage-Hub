use crate::config::AppEntry;
use crate::xdg;
use anyhow::{Context, Result};
use std::path::Path;

/// Freedesktop-compliant Exec quoting.
pub fn quote_exec_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|c| c.is_alphanumeric() || "_-./".contains(c))
    {
        arg.to_string()
    } else {
        // Wrap in double quotes; escape backslash and double quote.
        format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn single_line(value: &str) -> String {
    value.replace(['\r', '\n'], " ").trim().to_string()
}

fn sanitize_mime_types(raw: &str) -> Option<String> {
    let values: Vec<&str> = raw
        .split(';')
        .map(str::trim)
        .filter(|mime| {
            !mime.is_empty()
                && mime
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-+./".contains(c))
        })
        .collect();
    (!values.is_empty()).then(|| format!("{};", values.join(";")))
}

fn is_custom_scheme(mime: &str) -> bool {
    mime.starts_with("x-scheme-handler/")
}

fn is_sensitive_scheme(mime: &str) -> bool {
    const SENSITIVE: &[&str] = &[
        "x-scheme-handler/http",
        "x-scheme-handler/https",
        "x-scheme-handler/file",
        "x-scheme-handler/mailto",
        "x-scheme-handler/tel",
        "x-scheme-handler/ftp",
        "x-scheme-handler/ssh",
    ];
    SENSITIVE.contains(&mime)
}

pub fn build_exec(app: &AppEntry) -> String {
    let mut parts = vec![quote_exec_arg(&app.local_path.to_string_lossy())];
    if let Some(placeholder) = app
        .exec_arg
        .as_deref()
        .map(str::trim)
        .filter(|arg| !arg.is_empty())
    {
        // Exec field codes such as %U must remain unquoted.
        parts.push(placeholder.to_string());
    }
    parts.extend(app.args.iter().map(|arg| quote_exec_arg(arg)));
    parts.join(" ")
}

pub fn desktop_filename(app: &AppEntry) -> String {
    let name = xdg::sanitize_name(&app.display_name);
    let name = if name.is_empty() {
        app.id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(8)
            .collect()
    } else {
        name
    };
    format!("{name}.desktop")
}

pub fn legacy_id_filename(app_id: &str) -> String {
    format!("{app_id}.desktop")
}

pub fn desktop_file_content(app: &AppEntry) -> String {
    let exec = build_exec(app);
    let display_name = single_line(&app.display_name);
    let categories = app
        .categories
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            if c.ends_with(';') {
                c.to_string()
            } else {
                format!("{c};")
            }
        })
        .unwrap_or_else(|| "Utility;".to_string());

    let mut content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n",
        display_name,
    );

    if let Some(generic) = &app.generic_name {
        let g = single_line(generic);
        if !g.is_empty() {
            content.push_str(&format!("GenericName={g}\n"));
        }
    }

    if let Some(comment) = &app.comment {
        let c = single_line(comment);
        if !c.is_empty() {
            content.push_str(&format!("Comment={c}\n"));
        }
    }

    content.push_str(&format!(
        "Exec={}\n\
         Terminal=false\n\
         StartupNotify={}\n\
         Categories={}\n",
        exec,
        if app.startup_notify { "true" } else { "false" },
        categories,
    ));

    if let Some(mime_types) = app.mime_types.as_deref().and_then(sanitize_mime_types) {
        content.push_str(&format!("MimeType={mime_types}\n"));
    }

    let icon_to_use = if let Some(icon) = &app.icon_path {
        let s = icon.to_string_lossy();
        if !s.trim().is_empty() {
            Some(s.to_string())
        } else {
            None
        }
    } else if let Some(folder) = app.local_path.parent() {
        let p_png = folder.join("icon.png");
        let p_svg = folder.join("icon.svg");
        if p_png.exists() {
            Some(p_png.to_string_lossy().to_string())
        } else if p_svg.exists() {
            Some(p_svg.to_string_lossy().to_string())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(icon) = icon_to_use {
        content.push_str(&format!("Icon={}\n", icon));
    }
    content
}

#[cfg(unix)]
fn set_entry_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn set_entry_permissions(_: &Path) -> Result<()> {
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::remove_file(path).with_context(|| format!("remove {}", path.display()))?;
    }
    Ok(())
}

pub fn menu_entry_path(app: &AppEntry, applications_dir: &Path) -> std::path::PathBuf {
    applications_dir.join(desktop_filename(app))
}

pub fn desktop_shortcut_path(app: &AppEntry, desktop_dir: &Path) -> std::path::PathBuf {
    desktop_dir.join(desktop_filename(app))
}

fn mimeapps_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join("mimeapps.list"))
}

/// Adds or removes conservative default URI-handler entries in
/// `mimeapps.list`. Ordinary file MIME types and sensitive schemes are never
/// registered automatically, and existing user defaults are preserved.
fn update_mimeapps_defaults(desktop_filename: &str, mime_types: Option<&str>) -> Result<()> {
    let Some(path) = mimeapps_path() else {
        return Ok(());
    };

    let wanted: Vec<String> = mime_types
        .unwrap_or_default()
        .split(';')
        .map(str::trim)
        .filter(|mime| is_custom_scheme(mime) && !is_sensitive_scheme(mime))
        .map(str::to_string)
        .collect();

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut found_section = false;
    let mut present: Vec<String> = Vec::new();

    let flush_missing = |out: &mut Vec<String>, present: &Vec<String>| {
        for mime in &wanted {
            if !present.contains(mime) {
                out.push(format!("{mime}={desktop_filename}"));
            }
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_defaults {
                flush_missing(&mut out, &present);
            }
            in_defaults = trimmed == "[Default Applications]";
            found_section |= in_defaults;
            out.push(line.to_string());
            continue;
        }

        if in_defaults {
            if let Some((key, value)) = trimmed.split_once('=') {
                let key = key.trim();
                let value = value.trim();
                if value == desktop_filename {
                    // Remove stale mapping for this app; if still wanted it
                    // will be re-added by flush_missing.
                    continue;
                }
                if wanted.iter().any(|mime| mime == key) {
                    // Preserve an existing default owned by another app.
                    present.push(key.to_string());
                }
            }
        }
        out.push(line.to_string());
    }

    if in_defaults {
        flush_missing(&mut out, &present);
    } else if !wanted.is_empty() {
        if !out.is_empty() && out.last().is_some_and(|line| !line.trim().is_empty()) {
            out.push(String::new());
        }
        out.push("[Default Applications]".to_string());
        for mime in &wanted {
            out.push(format!("{mime}={desktop_filename}"));
        }
    }

    if !found_section && wanted.is_empty() && !path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    if path.exists() {
        let backup = path.with_extension("list.bak");
        let _ = std::fs::copy(&path, backup);
    }
    let tmp = path.with_extension("list.tmp");
    std::fs::write(&tmp, out.join("\n") + "\n")
        .with_context(|| format!("failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn refresh_menu_cache(applications_dir: &Path) {
    let _ = std::process::Command::new("update-desktop-database")
        .arg(applications_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::process::Command::new("kbuildsycoca6")
        .arg("--noincremental")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    let _ = std::process::Command::new("kbuildsycoca5")
        .arg("--noincremental")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// Returns the .desktop content for AppImage Hub itself. The Exec path is
/// always derived from the currently running executable.
fn hub_executable_path() -> std::path::PathBuf {
    // Inside an AppImage, current_exe() points into the temporary SquashFS
    // mount that disappears when the app exits. $APPIMAGE is the real,
    // persistent AppImage path. Fall back to current_exe() when running from
    // a normal binary during development.
    std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("appimage-manager"))
}

fn hub_desktop_content(category: &str) -> String {
    let exec = hub_executable_path().to_string_lossy().to_string();
    let category = category.trim();
    let category = if category.is_empty() {
        "Utility;".to_string()
    } else if category.ends_with(';') {
        category.to_string()
    } else {
        format!("{category};")
    };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=AppImage Hub\n\
         GenericName=AppImage Manager\n\
         Comment=Manage, launch, and update AppImages\n\
         Exec={}\n\
         Icon=appimage-hub\n\
         Terminal=false\n\
         StartupNotify=true\n\
         StartupWMClass=appimage-hub\n\
         Categories={}\n",
        quote_exec_arg(&exec),
        category
    )
}

fn install_hub_icon() {
    let data_dir = dirs::data_dir().unwrap_or_else(|| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    let icon_dir = data_dir.join("icons/hicolor/512x512/apps");
    let icon_path = icon_dir.join("appimage-hub.png");
    let logo_bytes = include_bytes!("../assets/logo.png");
    let need_write = std::fs::read(&icon_path)
        .map(|existing| existing != logo_bytes)
        .unwrap_or(true);
    if need_write {
        let _ = std::fs::create_dir_all(&icon_dir);
        let _ = std::fs::write(&icon_path, logo_bytes);
    }
}

/// Rewrites or removes AppImage Hub's own shortcuts according to settings.
/// Called on startup, after saving Settings, and after a self-update restart.
pub fn sync_hub_shortcuts(create_menu: bool, create_desktop: bool, start_menu_category: &str) {
    install_hub_icon();
    let content = hub_desktop_content(start_menu_category);

    if let Ok(apps_dir) = xdg::applications_dir() {
        let path = apps_dir.join("appimage-hub.desktop");
        if create_menu {
            let _ = std::fs::create_dir_all(&apps_dir);
            let _ = std::fs::write(&path, &content);
            let _ = set_entry_permissions(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
        refresh_menu_cache(&apps_dir);
    }

    if let Ok(desktop_dir) = xdg::desktop_dir() {
        let path = desktop_dir.join("appimage-hub.desktop");
        if create_desktop {
            let _ = std::fs::create_dir_all(&desktop_dir);
            let _ = std::fs::write(&path, &content);
            let _ = set_entry_permissions(&path);
            let _ = std::process::Command::new("gio")
                .args(["set", &path.to_string_lossy(), "metadata::trusted", "true"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub fn write_menu_entry(app: &AppEntry, applications_dir: &Path) -> Result<()> {
    let content = desktop_file_content(app);
    let filename = desktop_filename(app);
    let legacy_filename = legacy_id_filename(&app.id);

    std::fs::create_dir_all(applications_dir).context("create applications dir")?;
    let path = applications_dir.join(&filename);
    std::fs::write(&path, &content).with_context(|| format!("write {}", path.display()))?;
    let _ = set_entry_permissions(&path);
    if legacy_filename != filename {
        let legacy = applications_dir.join(&legacy_filename);
        if legacy.exists() {
            let _ = std::fs::remove_file(legacy);
        }
    }

    let _ = update_mimeapps_defaults(&filename, app.mime_types.as_deref());
    refresh_menu_cache(applications_dir);
    Ok(())
}

pub fn write_desktop_shortcut(app: &AppEntry, desktop_dir: &Path) -> Result<()> {
    let content = desktop_file_content(app);
    let filename = desktop_filename(app);
    let legacy_filename = legacy_id_filename(&app.id);

    std::fs::create_dir_all(desktop_dir).context("create desktop dir")?;
    let path = desktop_dir.join(&filename);
    std::fs::write(&path, &content).with_context(|| format!("write {}", path.display()))?;
    let _ = set_entry_permissions(&path);
    if legacy_filename != filename {
        let legacy = desktop_dir.join(&legacy_filename);
        if legacy.exists() {
            let _ = std::fs::remove_file(legacy);
        }
    }
    // Best-effort: mark trusted on desktops/environments that use gio metadata.
    let _ = std::process::Command::new("gio")
        .args(["set", &path.to_string_lossy(), "metadata::trusted", "true"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    Ok(())
}

pub fn remove_menu_entry(app: &AppEntry, applications_dir: &Path) -> Result<()> {
    let new_name = desktop_filename(app);
    let legacy_name = legacy_id_filename(&app.id);
    remove_file_if_exists(&applications_dir.join(&new_name))?;
    if legacy_name != new_name {
        remove_file_if_exists(&applications_dir.join(&legacy_name))?;
    }
    let _ = update_mimeapps_defaults(&new_name, None);
    if legacy_name != new_name {
        let _ = update_mimeapps_defaults(&legacy_name, None);
    }
    refresh_menu_cache(applications_dir);
    Ok(())
}

pub fn remove_desktop_shortcut(app: &AppEntry, desktop_dir: &Path) -> Result<()> {
    let new_name = desktop_filename(app);
    let legacy_name = legacy_id_filename(&app.id);
    remove_file_if_exists(&desktop_dir.join(&new_name))?;
    if legacy_name != new_name {
        remove_file_if_exists(&desktop_dir.join(&legacy_name))?;
    }
    Ok(())
}

pub fn remove_entry_files(
    app: &AppEntry,
    desktop_dir: &Path,
    applications_dir: &Path,
) -> Result<()> {
    let _ = remove_desktop_shortcut(app, desktop_dir);
    remove_menu_entry(app, applications_dir)?;
    Ok(())
}

#[allow(dead_code)]
pub fn write_entry(app: &AppEntry, desktop_dir: &Path, applications_dir: &Path) -> Result<()> {
    // Applications menu entry is required for Start Menu integration.
    write_menu_entry(app, applications_dir)?;

    // Desktop shortcut is best-effort.
    let _ = write_desktop_shortcut(app, desktop_dir);

    Ok(())
}

pub fn remove_entry(app: &AppEntry, desktop_dir: &Path, applications_dir: &Path) -> Result<()> {
    remove_entry_files(app, desktop_dir, applications_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppEntry, AppSource};
    use std::path::PathBuf;

    fn sample_app(args: Vec<String>) -> AppEntry {
        AppEntry {
            id: "id-1".to_string(),
            display_name: "Test App".to_string(),
            source: AppSource::LocalOnly,
            local_path: PathBuf::from("/tmp/My App.AppImage"),
            current_version: String::new(),
            args,
            icon_path: None,
            generic_name: None,
            comment: None,
            categories: None,
            startup_notify: true,
            asset_preference: None,
            added_at: None,
            commit_hash: None,
            file_fingerprint: None,
            desktop_shortcut: true,
            menu_entry: true,
            mime_types: None,
            exec_arg: None,
        }
    }

    #[test]
    fn desktop_file_content_includes_kde_metadata() {
        let mut app = sample_app(vec![]);
        app.generic_name = Some("PlayStation 2 Emulator".into());
        app.comment = Some("Sony PS2 Emulator".into());
        app.categories = Some("Game;Emulator;".into());
        app.startup_notify = true;
        let content = desktop_file_content(&app);
        assert!(content.contains("Name=Test App\n"));
        assert!(content.contains("GenericName=PlayStation 2 Emulator\n"));
        assert!(content.contains("Comment=Sony PS2 Emulator\n"));
        assert!(content.contains("Categories=Game;Emulator;\n"));
        assert!(content.contains("StartupNotify=true\n"));
    }

    #[test]
    fn desktop_file_content_uses_single_selected_category() {
        let mut app = sample_app(vec![]);
        app.categories = Some("Graphics;".into());
        let content = desktop_file_content(&app);
        assert!(content.contains("Categories=Graphics;\n"));
        assert!(!content.contains("Categories=Utility;"));
    }

    #[test]
    fn desktop_file_content_adds_missing_trailing_semicolon() {
        let mut app = sample_app(vec![]);
        app.categories = Some("AudioVideo".into());
        let content = desktop_file_content(&app);
        assert!(content.contains("Categories=AudioVideo;\n"));
    }

    #[test]
    fn mime_sanitization_keeps_valid_tokens_only() {
        assert_eq!(
            sanitize_mime_types("x-scheme-handler/heroic; application/x-iso9660-image;bad\nmime")
                .as_deref(),
            Some("x-scheme-handler/heroic;application/x-iso9660-image;")
        );
        assert_eq!(sanitize_mime_types(";;;"), None);
    }

    #[test]
    fn sensitive_schemes_are_not_auto_registered() {
        assert!(is_custom_scheme("x-scheme-handler/heroic"));
        assert!(is_sensitive_scheme("x-scheme-handler/https"));
        assert!(!is_sensitive_scheme("x-scheme-handler/heroic"));
    }

    #[test]
    fn exec_placeholder_is_not_quoted() {
        let mut app = sample_app(vec![]);
        app.exec_arg = Some("%U".into());
        let exec = build_exec(&app);
        assert!(exec.ends_with(" %U"));
        assert!(!exec.contains("\"%U\""));
    }

    #[test]
    fn quote_exec_arg_plain_is_unquoted() {
        assert_eq!(quote_exec_arg("/tmp/app.AppImage"), "/tmp/app.AppImage");
        assert_eq!(quote_exec_arg("--flag"), "--flag");
    }

    #[test]
    fn quote_exec_arg_escapes_spaces_quotes_backslash_and_specials() {
        assert_eq!(quote_exec_arg("hello world"), "\"hello world\"");
        assert_eq!(quote_exec_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(quote_exec_arg("c:\\temp"), "\"c:\\\\temp\"");
        assert_eq!(quote_exec_arg("$HOME"), "\"$HOME\"");
        assert_eq!(quote_exec_arg("`cmd`"), "\"`cmd`\"");
    }

    #[test]
    fn build_exec_quotes_path_and_args() {
        let app = sample_app(vec!["--name".into(), "hello world".into()]);
        let exec = build_exec(&app);
        assert_eq!(exec, "\"/tmp/My App.AppImage\" --name \"hello world\"");
    }

    #[test]
    fn desktop_filename_uses_sanitized_display_name() {
        let app = sample_app(vec![]);
        assert_eq!(desktop_filename(&app), "test-app.desktop");
    }

    #[test]
    fn desktop_file_content_includes_icon_when_set() {
        let mut app = sample_app(vec![]);
        app.icon_path = Some(PathBuf::from("/tmp/icon.png"));
        let content = desktop_file_content(&app);
        assert!(content.contains("Icon=/tmp/icon.png"));
    }

    #[test]
    fn write_and_remove_entry_manage_both_files_and_legacy() -> Result<()> {
        let base = std::env::temp_dir().join(format!("aim-desktop-{}", uuid::Uuid::new_v4()));
        let desktop = base.join("Desktop");
        let apps = base.join("applications");
        std::fs::create_dir_all(&desktop)?;
        std::fs::create_dir_all(&apps)?;
        // Simulate legacy UUID-named files from older versions.
        std::fs::write(desktop.join("id-1.desktop"), "old")?;
        std::fs::write(apps.join("id-1.desktop"), "old")?;

        let app = sample_app(vec!["--flag".into()]);
        write_entry(&app, &desktop, &apps)?;
        let name = "test-app.desktop";
        assert!(desktop.join(name).exists());
        assert!(apps.join(name).exists());
        assert!(!desktop.join("id-1.desktop").exists());
        assert!(!apps.join("id-1.desktop").exists());
        let content = std::fs::read_to_string(apps.join(name))?;
        assert!(content.contains("Name=Test App"));
        assert!(content.contains("Exec=\"/tmp/My App.AppImage\" --flag"));

        remove_entry(&app, &desktop, &apps)?;
        assert!(!desktop.join(name).exists());
        assert!(!apps.join(name).exists());
        std::fs::remove_dir_all(base)?;
        Ok(())
    }
}
