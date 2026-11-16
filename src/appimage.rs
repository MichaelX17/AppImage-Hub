use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppImageMetadata {
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub categories: Option<String>,
    pub matched_category: Option<String>,
    pub is_custom_category: bool,
    pub startup_notify: Option<bool>,
    pub icon_path: Option<PathBuf>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DesktopFileMetadata {
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub categories: Option<String>,
    pub startup_notify: Option<bool>,
    pub icon: Option<String>,
}

/// Detects the image file format extension from raw bytes.
pub fn detect_image_extension(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "png"
    } else if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"\x00\x00\x01\x00") {
        "ico"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "gif"
    } else if bytes.starts_with(b"\xFF\xD8\xFF") {
        "jpg"
    } else {
        let sample_len = bytes.len().min(1024);
        if let Ok(sample) = std::str::from_utf8(&bytes[..sample_len]) {
            if sample.contains("<svg") || sample.contains("<?xml") {
                return "svg";
            }
        }
        "png"
    }
}

/// Returns the cache directory where staged AppImage icons are saved.
pub fn icon_cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("appimage-manager")
        .join("icons");
    fs::create_dir_all(&dir).context("failed to create icon cache directory")?;
    Ok(dir)
}

/// Parses the [Desktop Entry] section of a `.desktop` file.
pub fn parse_desktop_entry(content: &str) -> DesktopFileMetadata {
    let mut meta = DesktopFileMetadata::default();
    let mut in_desktop_entry = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }

        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim();
            let val = val.trim();

            // Ignore localized keys like Name[es] unless base key is unset
            match key {
                "Name" => {
                    if meta.name.is_none() {
                        meta.name = Some(val.to_string());
                    }
                }
                "GenericName" => {
                    if meta.generic_name.is_none() {
                        meta.generic_name = Some(val.to_string());
                    }
                }
                "Comment" => {
                    if meta.comment.is_none() {
                        meta.comment = Some(val.to_string());
                    }
                }
                "Categories" => {
                    if meta.categories.is_none() {
                        meta.categories = Some(val.to_string());
                    }
                }
                "StartupNotify" => {
                    if meta.startup_notify.is_none() {
                        meta.startup_notify = Some(val.eq_ignore_ascii_case("true"));
                    }
                }
                "Icon" if meta.icon.is_none() => meta.icon = Some(val.to_string()),
                _ => {}
            }
        }
    }

    meta
}

/// Maps standard XDG desktop categories to the KDE Plasma categories used by AppImage Hub.
/// Returns (Category string, is_custom boolean).
pub fn map_kde_category(categories_str: &str) -> (String, bool) {
    let tokens: Vec<&str> = categories_str
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for token in &tokens {
        match *token {
            "Game" | "Games" | "Emulator" => return ("Game".to_string(), false),
            "Utility" | "Utilities" | "Accessories" => return ("Utility".to_string(), false),
            "Development" | "Programming" | "IDE" => return ("Development".to_string(), false),
            "Education" | "Teaching" => return ("Education".to_string(), false),
            "Graphics" | "2DGraphics" | "RasterGraphics" | "VectorGraphics" => {
                return ("Graphics".to_string(), false)
            }
            "Network" | "Internet" | "WebBrowser" | "Chat" => {
                return ("Network".to_string(), false)
            }
            "AudioVideo" | "Audio" | "Video" | "Multimedia" | "Player" | "Recorder" => {
                return ("AudioVideo".to_string(), false)
            }
            "Office" | "WordProcessor" | "Spreadsheet" => return ("Office".to_string(), false),
            "Science" | "Math" => return ("Science".to_string(), false),
            "System" | "Settings" => return ("System".to_string(), false),
            _ => {}
        }
    }

    if let Some(first) = tokens.first() {
        (first.to_string(), true)
    } else {
        ("Utility".to_string(), false)
    }
}

struct TempDirGuard<'a>(&'a Path);
impl<'a> Drop for TempDirGuard<'a> {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0);
    }
}

/// Extracts metadata and icon from an AppImage.
/// If `dest_icon_dir` is provided, the icon is placed directly in `dest_icon_dir/icon.<ext>`.
/// Otherwise, it is placed in the icon cache directory.
pub fn extract_metadata(
    appimage_path: &Path,
    dest_icon_dir: Option<&Path>,
) -> Result<AppImageMetadata> {
    if !appimage_path.exists() {
        anyhow::bail!("AppImage does not exist: {}", appimage_path.display());
    }

    // Ensure executable permission so runtime can run --appimage-extract
    let _ = crate::permissions::set_executable(appimage_path);

    let temp_base = std::env::temp_dir().join(format!("aim_extract_{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&temp_base).context("failed to create temporary extraction directory")?;
    let _guard = TempDirGuard(&temp_base);

    let squashfs_root = temp_base.join("squashfs-root");

    // Primary strategy: execute AppImage with --appimage-extract
    let _ = Command::new(appimage_path)
        .args(["--appimage-extract", ".DirIcon"])
        .current_dir(&temp_base)
        .env("APPIMAGE_SILENT_INSTALL", "1")
        .env("DESKTOPINTEGRATION", "no")
        .output();

    let _ = Command::new(appimage_path)
        .args(["--appimage-extract", "*.desktop"])
        .current_dir(&temp_base)
        .env("APPIMAGE_SILENT_INSTALL", "1")
        .env("DESKTOPINTEGRATION", "no")
        .output();

    // Check if .DirIcon is a symlink pointing to an unextracted file
    let dir_icon = squashfs_root.join(".DirIcon");
    if dir_icon.is_symlink() {
        if let Ok(link_target) = fs::read_link(&dir_icon) {
            let target_str = link_target.to_string_lossy();
            let stripped = target_str.trim_start_matches('/');
            let resolved = squashfs_root.join(stripped);
            if !resolved.exists() {
                let _ = Command::new(appimage_path)
                    .args(["--appimage-extract", stripped])
                    .current_dir(&temp_base)
                    .env("APPIMAGE_SILENT_INSTALL", "1")
                    .env("DESKTOPINTEGRATION", "no")
                    .output();
            }
        }
    }

    // Parse desktop file if extracted
    let mut desktop_meta: Option<DesktopFileMetadata> = None;
    if let Ok(entries) = fs::read_dir(&squashfs_root) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().map(|e| e == "desktop").unwrap_or(false) {
                if let Ok(content) = fs::read_to_string(&p) {
                    desktop_meta = Some(parse_desktop_entry(&content));
                    break;
                }
            }
        }
    }

    // If icon is still missing, try extracting the icon named in the desktop file
    let mut icon_data: Option<(Vec<u8>, &'static str)> = resolve_icon_from_root(&squashfs_root);

    if icon_data.is_none() {
        if let Some(ref d_meta) = desktop_meta {
            if let Some(ref icon_name) = d_meta.icon {
                let _ = Command::new(appimage_path)
                    .args(["--appimage-extract", &format!("{icon_name}.*")])
                    .current_dir(&temp_base)
                    .env("APPIMAGE_SILENT_INSTALL", "1")
                    .env("DESKTOPINTEGRATION", "no")
                    .output();
                icon_data = resolve_icon_from_root(&squashfs_root);
            }
        }
    }

    // Secondary fallback: 7z extraction if --appimage-extract did not produce an icon
    if icon_data.is_none() {
        let out_7z = temp_base.join("7z_out");
        let _ = fs::create_dir_all(&out_7z);
        let _ = Command::new("7z")
            .args([
                "e",
                "-y",
                &format!("-o{}", out_7z.display()),
                appimage_path.to_str().unwrap_or(""),
                ".DirIcon",
                "*.desktop",
                "*.png",
                "*.svg",
            ])
            .output();

        if desktop_meta.is_none() {
            if let Ok(entries) = fs::read_dir(&out_7z) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().map(|e| e == "desktop").unwrap_or(false) {
                        if let Ok(content) = fs::read_to_string(&p) {
                            desktop_meta = Some(parse_desktop_entry(&content));
                            break;
                        }
                    }
                }
            }
        }

        icon_data = resolve_icon_from_root(&out_7z);
    }

    // Save icon if found
    let saved_icon_path = if let Some((bytes, ext)) = icon_data {
        let target_path = if let Some(dir) = dest_icon_dir {
            fs::create_dir_all(dir)?;
            dir.join(format!("icon.{}", ext))
        } else {
            let cache = icon_cache_dir()?;
            let stem = appimage_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("app");
            cache.join(format!("{}.{}", crate::xdg::sanitize_name(stem), ext))
        };
        fs::write(&target_path, bytes)?;
        Some(target_path)
    } else {
        None
    };

    let (matched_category, is_custom_category) = desktop_meta
        .as_ref()
        .and_then(|d| d.categories.as_deref())
        .map(map_kde_category)
        .map(|(cat, custom)| (Some(cat), custom))
        .unwrap_or((None, false));

    Ok(AppImageMetadata {
        name: desktop_meta.as_ref().and_then(|d| d.name.clone()),
        generic_name: desktop_meta.as_ref().and_then(|d| d.generic_name.clone()),
        comment: desktop_meta.as_ref().and_then(|d| d.comment.clone()),
        categories: desktop_meta.as_ref().and_then(|d| d.categories.clone()),
        matched_category,
        is_custom_category,
        startup_notify: desktop_meta.as_ref().and_then(|d| d.startup_notify),
        icon_path: saved_icon_path,
    })
}

/// Helper to extract just the icon from an AppImage and save it to `dest_icon_dir/icon.<ext>`.
pub fn extract_icon(appimage_path: &Path, dest_icon_dir: &Path) -> Result<PathBuf> {
    let meta = extract_metadata(appimage_path, Some(dest_icon_dir))?;
    meta.icon_path
        .context("no icon found or extracted from AppImage")
}

/// Attempts to read icon bytes and format from a directory containing extracted files.
fn resolve_icon_from_root(dir: &Path) -> Option<(Vec<u8>, &'static str)> {
    if !dir.exists() {
        return None;
    }

    // 1. Check .DirIcon
    let dir_icon = dir.join(".DirIcon");
    if dir_icon.exists() {
        if let Ok(bytes) = fs::read(&dir_icon) {
            if !bytes.is_empty() {
                let ext = detect_image_extension(&bytes);
                return Some((bytes, ext));
            }
        }
    }

    // 2. Check root directory for any .png or .svg
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "png" || ext_lower == "svg" {
                    candidates.push(p);
                }
            }
        }
    }

    // Pick the largest candidate (likely highest resolution)
    candidates.sort_by_key(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0));
    if let Some(best) = candidates.pop() {
        if let Ok(bytes) = fs::read(&best) {
            if !bytes.is_empty() {
                let ext = detect_image_extension(&bytes);
                return Some((bytes, ext));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_desktop_entry() {
        let content = r#"
[Desktop Entry]
Type=Application
Name=PCSX2
GenericName=PlayStation 2 Emulator
Comment=A Sony PlayStation 2 video game console emulator
Exec=pcsx2 %F
Icon=pcsx2
Categories=Game;Emulator;
StartupNotify=true
"#;
        let meta = parse_desktop_entry(content);
        assert_eq!(meta.name.as_deref(), Some("PCSX2"));
        assert_eq!(meta.generic_name.as_deref(), Some("PlayStation 2 Emulator"));
        assert_eq!(
            meta.comment.as_deref(),
            Some("A Sony PlayStation 2 video game console emulator")
        );
        assert_eq!(meta.categories.as_deref(), Some("Game;Emulator;"));
        assert_eq!(meta.startup_notify, Some(true));
        assert_eq!(meta.icon.as_deref(), Some("pcsx2"));
    }

    #[test]
    fn test_detect_image_extension() {
        assert_eq!(detect_image_extension(b"\x89PNG\r\n\x1a\n123"), "png");
        assert_eq!(
            detect_image_extension(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            "webp"
        );
        assert_eq!(detect_image_extension(b"\x00\x00\x01\x00\x01\x00"), "ico");
        assert_eq!(
            detect_image_extension(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"),
            "svg"
        );
        assert_eq!(
            detect_image_extension(b"<?xml version=\"1.0\"?><svg></svg>"),
            "svg"
        );
    }

    #[test]
    fn test_map_kde_category() {
        assert_eq!(map_kde_category("Game;Emulator;"), ("Game".into(), false));
        assert_eq!(
            map_kde_category("AudioVideo;Recorder;"),
            ("AudioVideo".into(), false)
        );
        assert_eq!(
            map_kde_category("Development;IDE;"),
            ("Development".into(), false)
        );
        assert_eq!(
            map_kde_category("Utility;System;"),
            ("Utility".into(), false)
        );
        assert_eq!(
            map_kde_category("UnknownCategory;"),
            ("UnknownCategory".into(), true)
        );
        assert_eq!(map_kde_category(""), ("Utility".into(), false));
    }
}
