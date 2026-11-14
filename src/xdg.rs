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

pub fn managed_apps_dir() -> Result<PathBuf> {
    let data = dirs::data_dir().unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share")
    });
    let dir = data.join("appimage-manager/apps");
    std::fs::create_dir_all(&dir).context("failed to create managed apps directory")?;
    Ok(dir)
}

pub fn sanitize_name(input: &str) -> String {
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
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "app".to_string()
    } else {
        out
    }
}

pub fn managed_app_folder(hint: &str, id: &str) -> Result<PathBuf> {
    let base = managed_apps_dir()?;
    managed_app_folder_in(&base, hint, id)
}

pub fn managed_app_folder_in(base: &Path, hint: &str, id: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(base).context("failed to create managed apps base directory")?;
    let safe_hint = sanitize_name(hint);
    let short_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    let folder_name = if short_id.is_empty() {
        safe_hint
    } else {
        format!("{safe_hint}-{short_id}")
    };
    let dir = base.join(folder_name);
    std::fs::create_dir_all(&dir).context("failed to create per-app managed folder")?;
    Ok(dir)
}

pub fn is_path_in_managed_apps(path: &Path) -> bool {
    if let Ok(base) = managed_apps_dir() {
        path.starts_with(base)
    } else {
        false
    }
}
