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

pub fn build_exec(app: &AppEntry) -> String {
    std::iter::once(app.local_path.to_string_lossy().into_owned())
        .chain(app.args.iter().cloned())
        .map(|a| quote_exec_arg(&a))
        .collect::<Vec<_>>()
        .join(" ")
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
    let categories = app
        .categories
        .as_deref()
        .map(|c| c.trim())
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
        app.display_name,
    );

    if let Some(generic) = &app.generic_name {
        let g = generic.trim();
        if !g.is_empty() {
            content.push_str(&format!("GenericName={g}\n"));
        }
    }

    if let Some(comment) = &app.comment {
        let c = comment.trim();
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

    if let Some(icon) = &app.icon_path {
        let icon = icon.to_string_lossy();
        if !icon.trim().is_empty() {
            content.push_str(&format!("Icon={}\n", icon));
        }
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
