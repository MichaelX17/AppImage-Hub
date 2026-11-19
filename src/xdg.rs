use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

pub fn desktop_dir() -> Result<PathBuf> {
    if let Ok(output) = std::process::Command::new("xdg-user-dir")
        .arg("DESKTOP")
        .output()
    {
        if output.status.success() {
            let p = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !p.is_empty() {
                return Ok(PathBuf::from(p));
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let cfg = home.join(".config/user-dirs.dirs");
        if let Ok(content) = std::fs::read_to_string(&cfg) {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("XDG_DESKTOP_DIR=") {
                    let val = val.trim().trim_matches('"');
                    let resolved = if let Some(rest) = val.strip_prefix("$HOME/") {
                        home.join(rest)
                    } else if val == "$HOME" {
                        home.clone()
                    } else {
                        PathBuf::from(val)
                    };
                    return Ok(resolved);
                }
            }
        }
        return Ok(home.join("Desktop"));
    }

    Err(anyhow!("could not resolve XDG desktop directory"))
}

pub fn applications_dir() -> Result<PathBuf> {
    let data = dirs::data_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    Ok(data.join("applications"))
}

pub fn default_managed_apps_dir() -> PathBuf {
    let data = dirs::data_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    data.join("appimage-manager/apps")
}

pub fn managed_apps_dir() -> Result<PathBuf> {
    let dir = default_managed_apps_dir();
    std::fs::create_dir_all(&dir).context("failed to create managed apps directory")?;
    Ok(dir)
}

pub fn sanitize_name(input: &str) -> String {
    let out = slug(input);
    if out.is_empty() {
        "app".to_string()
    } else {
        out
    }
}

/// Lowercases and replaces every unsafe character with `-`, without applying
/// the `"app"` fallback that [`sanitize_name`] adds. Returns `""` when the
/// input collapses to nothing.
fn slug(input: &str) -> String {
    let mut out: String = input
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

pub fn managed_app_folder(hint: &str, id: &str) -> Result<PathBuf> {
    let base = managed_apps_dir()?;
    managed_app_folder_in(&base, hint, id)
}

/// Directory name used for a managed app: `<sanitized name>-<id prefix>`.
pub fn managed_app_folder_name(hint: &str, id: &str) -> String {
    let safe_hint = sanitize_name(hint);
    let short_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    if short_id.is_empty() {
        safe_hint
    } else {
        format!("{safe_hint}-{short_id}")
    }
}

pub fn managed_app_folder_in(base: &Path, hint: &str, id: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(base).context("failed to create managed apps base directory")?;
    let dir = base.join(managed_app_folder_name(hint, id));
    std::fs::create_dir_all(&dir).context("failed to create per-app managed folder")?;
    Ok(dir)
}

/// Canonical file name used for a managed AppImage: `<sanitized name>.appimage`.
///
/// The display name the user typed wins; `fallback` is only used when that
/// sanitizes to an empty string (e.g. a name made purely of punctuation).
pub fn managed_appimage_filename(display_name: &str, fallback: &str) -> String {
    let mut base = slug(display_name);
    if base.is_empty() {
        // Drop any existing extension before sanitizing, so
        // `Some App v1.AppImage` becomes `some-app-v1`, not `some-app-v1-appimage`.
        let stem = Path::new(fallback)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| fallback.to_string());
        base = slug(&stem);
    }
    if base.is_empty() {
        base = "app".to_string();
    }
    format!("{base}.appimage")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_filename_uses_display_name() {
        assert_eq!(
            managed_appimage_filename("Example", "Example_v2.4_x86_64.AppImage"),
            "example.appimage"
        );
    }

    #[test]
    fn managed_filename_sanitizes_spaces_and_symbols() {
        assert_eq!(
            managed_appimage_filename("My  Cool App!", "x.AppImage"),
            "my-cool-app.appimage"
        );
    }

    #[test]
    fn managed_filename_falls_back_when_name_is_empty() {
        assert_eq!(
            managed_appimage_filename("   ", "Some App v1.AppImage"),
            "some-app-v1.appimage"
        );
        assert_eq!(managed_appimage_filename("!!!", "???"), "app.appimage");
    }

    #[test]
    fn sanitize_name_is_stable() {
        assert_eq!(sanitize_name("Hello_World-2"), "hello_world-2");
        assert_eq!(sanitize_name("--a---b--"), "a-b");
    }
}
