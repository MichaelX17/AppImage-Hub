use anyhow::{Context, Result};
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppImageMetadata {
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub categories: Option<String>,
    pub matched_category: Option<String>,
    pub is_custom_category: bool,
    pub startup_notify: Option<bool>,
    pub startup_wm_class: Option<String>,
    pub icon_path: Option<PathBuf>,
    pub mime_types: Option<String>,
    pub exec_arg: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DesktopFileMetadata {
    pub name: Option<String>,
    pub generic_name: Option<String>,
    pub comment: Option<String>,
    pub categories: Option<String>,
    pub startup_notify: Option<bool>,
    pub startup_wm_class: Option<String>,
    pub icon: Option<String>,
    pub mime_types: Option<String>,
    pub exec: Option<String>,
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
    crate::fsutil::ensure_private_dir(&dir).context("failed to create icon cache directory")?;
    Ok(dir)
}

/// Trims a `StartupWMClass` value and rejects control characters. A value with
/// a newline could otherwise inject extra keys when written to our own
/// `.desktop` file.
pub fn normalize_startup_wm_class(value: &str) -> Option<String> {
    let value = value.trim().trim_matches('"').trim();
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_string())
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
                "StartupWMClass" if meta.startup_wm_class.is_none() => {
                    meta.startup_wm_class = Some(val.to_string());
                }
                "MimeType" if meta.mime_types.is_none() => {
                    meta.mime_types = Some(val.to_string());
                }
                "Exec" if meta.exec.is_none() => meta.exec = Some(val.to_string()),
                _ => {}
            }
        }
    }

    meta
}

fn exec_placeholder(exec: Option<&str>, mime_types: Option<&str>) -> Option<String> {
    let exec = exec.unwrap_or_default();
    for placeholder in ["%U", "%u", "%F", "%f"] {
        if exec.contains(placeholder) {
            return Some(placeholder.to_string());
        }
    }
    if mime_types
        .unwrap_or_default()
        .split(';')
        .any(|mime| mime.trim().starts_with("x-scheme-handler/"))
    {
        return Some("%U".to_string());
    }
    None
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

/// How an AppImage's metadata may be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractionPolicy {
    /// The user explicitly selected this file and approved running it once.
    ExecuteAllowed,
    /// Never execute the AppImage payload; use non-executing tools only.
    NeverExecute,
}

impl ExtractionPolicy {
    fn executes(self) -> bool {
        matches!(self, Self::ExecuteAllowed)
    }
}

/// Bound for one extraction helper command.
const EXTRACTION_TIMEOUT: Duration = Duration::from_secs(15);
/// Upper bound on the number of files considered in an extracted directory.
const MAX_EXTRACTED_ENTRIES: usize = 64;

struct TempDirGuard<'a>(&'a Path);
impl Drop for TempDirGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0);
    }
}

/// Runs a helper command with null stdio and a hard timeout. Returns `true`
/// when the process finished before the deadline (its exit status is ignored;
/// callers inspect the extracted files). A hung process is killed.
fn run_command_bounded(command: &mut Command, timeout: Duration) -> bool {
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

fn run_appimage_extract(appimage_path: &Path, args: &[&str], cwd: &Path) -> bool {
    let mut command = Command::new(appimage_path);
    command
        .args(args)
        .current_dir(cwd)
        .env("APPIMAGE_SILENT_INSTALL", "1")
        .env("DESKTOPINTEGRATION", "no");
    run_command_bounded(&mut command, EXTRACTION_TIMEOUT)
}

/// Extracts the interesting files with `7z`, which never executes the AppImage.
fn extract_with_7z_patterns(appimage_path: &Path, out_dir: &Path, patterns: &[&str]) -> bool {
    let mut command = Command::new("7z");
    command
        .args(["e", "-y"])
        .arg(format!("-o{}", out_dir.display()))
        .arg(appimage_path)
        .args(patterns)
        .current_dir(out_dir);
    run_command_bounded(&mut command, EXTRACTION_TIMEOUT)
}

fn extract_with_7z(appimage_path: &Path, out_dir: &Path) -> bool {
    let root = extract_with_7z_patterns(
        appimage_path,
        out_dir,
        &[".DirIcon", "AppRun", "*.desktop", "*.png", "*.svg"],
    );
    // The root `.desktop` entry of many AppImages (Tauri, Electron, some
    // appimagetool builds) is a symlink into `usr/share/applications`. `7z e`
    // extracts the symlink itself, not its target, so the real desktop file
    // must be extracted too. It is written last on purpose: the regular file
    // replaces the dangling symlink and exposes `StartupWMClass`.
    let nested = extract_with_7z_patterns(
        appimage_path,
        out_dir,
        &["usr/share/applications/*.desktop"],
    );
    root || nested
}

/// Validates a SquashFS superblock at `offset` and returns the payload end.
///
/// The fields checked are the magic, block size/log, compression id, version,
/// and `bytes_used`. This distinguishes the real AppImage payload from the
/// string `hsqs` occurring inside an embedded blob.
fn squashfs_payload_end(header: &[u8], offset: u64, file_len: u64) -> Option<u64> {
    if header.len() < 96 || &header[..4] != b"hsqs" {
        return None;
    }
    let block_size = u32::from_le_bytes(header[12..16].try_into().ok()?);
    let compression = u16::from_le_bytes(header[20..22].try_into().ok()?);
    let block_log = u16::from_le_bytes(header[22..24].try_into().ok()?);
    let major = u16::from_le_bytes(header[28..30].try_into().ok()?);
    let bytes_used = u64::from_le_bytes(header[40..48].try_into().ok()?);
    let end = offset.checked_add(bytes_used)?;
    let valid = block_size.is_power_of_two()
        && (4096..=1_048_576).contains(&block_size)
        && block_size.trailing_zeros() as u16 == block_log
        && (1..=6).contains(&compression)
        && major == 4
        && bytes_used >= 96
        && end <= file_len;
    valid.then_some(end)
}

/// Finds the outer SquashFS payload inside an AppImage.
///
/// Most AppImages can be read by `7z` directly, but some (notably builds that
/// contain an embedded compressed stream before the SquashFS) are misdetected
/// as a different archive format. Locating the SquashFS superblock lets us
/// extract the same payload without executing the AppImage.
fn find_squashfs_offset(path: &Path) -> Option<u64> {
    let mut file = fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut tail: Vec<u8> = Vec::new();
    let mut read_total = 0_u64;
    let mut best: Option<(u64, u64)> = None;

    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        let mut data = Vec::with_capacity(tail.len() + read);
        data.extend_from_slice(&tail);
        data.extend_from_slice(&buffer[..read]);
        let data_offset = read_total - tail.len() as u64;

        if data.len() >= 96 {
            for index in 0..=data.len() - 96 {
                if &data[index..index + 4] != b"hsqs" {
                    continue;
                }
                let offset = data_offset + index as u64;
                if let Some(end) = squashfs_payload_end(&data[index..index + 96], offset, file_len)
                {
                    // The outer payload is the one that ends closest to EOF.
                    if best.is_none_or(|(_, best_end)| end > best_end) {
                        best = Some((offset, end));
                    }
                }
            }
        }

        let keep = data.len().min(95);
        tail.clear();
        tail.extend_from_slice(&data[data.len() - keep..]);
        read_total += read as u64;
    }

    best.map(|(offset, _)| offset)
}

/// Copies the SquashFS payload from `offset` into a temporary archive so `7z`
/// can read formats it could not detect inside the original AppImage.
fn extract_squashfs_payload(appimage_path: &Path, out_dir: &Path) -> bool {
    let Some(offset) = find_squashfs_offset(appimage_path) else {
        return false;
    };
    if offset == 0 {
        return false;
    }
    let payload = out_dir.join(".appimage-squashfs");
    let copied = (|| -> std::io::Result<()> {
        use std::io::{Seek, SeekFrom};
        let mut input = fs::File::open(appimage_path)?;
        input.seek(SeekFrom::Start(offset))?;
        let mut output = fs::File::create(&payload)?;
        std::io::copy(&mut input, &mut output)?;
        Ok(())
    })();
    if copied.is_err() {
        let _ = fs::remove_file(&payload);
        return false;
    }
    let extracted = extract_with_7z(&payload, out_dir);
    let _ = fs::remove_file(&payload);
    extracted
}

/// Derives the program name from a desktop entry's `Exec=` value. This is a
/// good `StartupWMClass` fallback because Qt/GTK window classes are normally
/// the executable basename.
fn exec_program_name(exec: &str) -> Option<String> {
    let tokens = crate::args::split_args(exec).ok()?;
    let mut command = None;
    for token in tokens.iter().filter(|token| !token.is_empty()) {
        if matches!(token.as_str(), "env" | "/usr/bin/env") {
            continue;
        }
        // Leading VAR=value assignments before the actual command.
        if token.contains('=') && !token.contains('/') && !token.starts_with('-') {
            continue;
        }
        if token.starts_with('-') || token.starts_with('%') {
            continue;
        }
        command = Some(token.as_str());
        break;
    }

    let name = Path::new(command?).file_name()?.to_string_lossy();
    let name = name.strip_suffix(".AppImage").unwrap_or(&name);
    if matches!(name, "sh" | "bash" | "env" | "AppRun") {
        return None;
    }
    normalize_startup_wm_class(name)
}

fn read_first_desktop(dir: &Path) -> Option<DesktopFileMetadata> {
    let entries = fs::read_dir(dir).ok()?;
    for entry in entries.flatten().take(MAX_EXTRACTED_ENTRIES) {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "desktop")
        {
            if let Ok(content) = fs::read_to_string(&path) {
                let mut meta = parse_desktop_entry(&content);
                // Many AppImages do not declare StartupWMClass. Prefer the
                // `Exec=` program name (the usual window class), then the
                // desktop file id, so the generated shortcut groups with its
                // own window in the task manager instead of opening a second
                // icon.
                if meta.startup_wm_class.is_none() {
                    meta.startup_wm_class = meta
                        .exec
                        .as_deref()
                        .and_then(exec_program_name)
                        .or_else(|| {
                            path.file_stem().and_then(|stem| {
                                normalize_startup_wm_class(&stem.to_string_lossy())
                            })
                        });
                }
                return Some(meta);
            }
        }
    }
    None
}

/// Reads the executable named by an AppImage's `AppRun` when no embedded
/// `.desktop` file exists. AppRun is often a symlink or a small script that
/// execs the real binary.
fn read_apprun_wm_class(dir: &Path) -> Option<String> {
    let path = dir.join("AppRun");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() {
        let target = fs::read_link(&path).ok()?;
        let name = target.file_name()?.to_string_lossy();
        if matches!(name.as_ref(), "AppRun" | "sh" | "bash") {
            return None;
        }
        return normalize_startup_wm_class(&name);
    }

    let content = fs::read_to_string(&path).ok()?;
    content.lines().rev().find_map(|line| {
        line.trim()
            .strip_prefix("exec ")
            .and_then(exec_program_name)
    })
}

fn icon_cache_name(appimage_path: &Path, extension: &str) -> String {
    use ring::digest::{Context, SHA256};
    use std::fmt::Write as _;

    let mut context = Context::new(&SHA256);
    context.update(appimage_path.to_string_lossy().as_bytes());
    let digest = context.finish();
    let mut name = String::with_capacity(20);
    for byte in &digest.as_ref()[..8] {
        let _ = write!(name, "{byte:02x}");
    }
    name.push('.');
    name.push_str(extension);
    name
}

/// Extracts metadata and an icon from an AppImage.
///
/// Non-executing extraction (`7z`) is always attempted first. The AppImage
/// itself is only run (`--appimage-extract`) when the caller explicitly allows
/// it — never for a freshly downloaded file or during startup repair.
pub fn extract_metadata_with_policy(
    appimage_path: &Path,
    dest_icon_dir: Option<&Path>,
    policy: ExtractionPolicy,
) -> Result<AppImageMetadata> {
    if !appimage_path.exists() {
        anyhow::bail!("AppImage does not exist: {}", appimage_path.display());
    }

    let temp_base = std::env::temp_dir().join(format!("aim_extract_{}", uuid::Uuid::new_v4()));
    crate::fsutil::ensure_private_dir(&temp_base)
        .context("failed to create temporary extraction directory")?;
    let _guard = TempDirGuard(&temp_base);

    let mut desktop_meta: Option<DesktopFileMetadata> = None;
    let mut icon_data: Option<(Vec<u8>, &'static str)> = None;

    // Strategy 1: non-executing extraction. Works for most AppImages.
    let seven_dir = temp_base.join("7z_out");
    if fs::create_dir_all(&seven_dir).is_ok() && extract_with_7z(appimage_path, &seven_dir) {
        desktop_meta = read_first_desktop(&seven_dir);
        icon_data = resolve_icon_from_root(&seven_dir);
    }

    // Strategy 1b: some AppImages (e.g. Zen Browser) contain an embedded
    // compressed stream that makes `7z` misdetect the whole file and miss the
    // SquashFS payload. Locate the payload and extract that copy instead.
    if desktop_meta.is_none()
        && fs::create_dir_all(&seven_dir).is_ok()
        && extract_squashfs_payload(appimage_path, &seven_dir)
    {
        desktop_meta = read_first_desktop(&seven_dir);
        if icon_data.is_none() {
            icon_data = resolve_icon_from_root(&seven_dir);
        }
    }

    // Strategy 2: execute the payload — only with explicit approval.
    if policy.executes() && (desktop_meta.is_none() || icon_data.is_none()) {
        let _ = crate::permissions::set_executable(appimage_path);
        let squashfs_root = temp_base.join("squashfs-root");

        run_appimage_extract(
            appimage_path,
            &["--appimage-extract", ".DirIcon"],
            &temp_base,
        );
        run_appimage_extract(
            appimage_path,
            &["--appimage-extract", "*.desktop"],
            &temp_base,
        );
        run_appimage_extract(appimage_path, &["--appimage-extract", "AppRun"], &temp_base);

        // .DirIcon may be a symlink to a file that was not extracted yet.
        let dir_icon = squashfs_root.join(".DirIcon");
        if dir_icon.is_symlink() {
            if let Ok(link_target) = fs::read_link(&dir_icon) {
                let stripped = link_target
                    .to_string_lossy()
                    .trim_start_matches('/')
                    .to_string();
                let resolved = squashfs_root.join(&stripped);
                if !resolved.exists() && !stripped.is_empty() {
                    run_appimage_extract(
                        appimage_path,
                        &["--appimage-extract", &stripped],
                        &temp_base,
                    );
                }
            }
        }

        if desktop_meta.is_none() {
            desktop_meta = read_first_desktop(&squashfs_root);
        }
        if icon_data.is_none() {
            icon_data = resolve_icon_from_root(&squashfs_root);
        }
        if icon_data.is_none() {
            if let Some(icon_name) = desktop_meta.as_ref().and_then(|meta| meta.icon.as_deref()) {
                let pattern = format!("{icon_name}.*");
                run_appimage_extract(appimage_path, &["--appimage-extract", &pattern], &temp_base);
                icon_data = resolve_icon_from_root(&squashfs_root);
            }
        }
    }

    // Last-resort non-executing hint for AppImages without an embedded
    // `.desktop` file.
    let apprun_wm_class = read_apprun_wm_class(&seven_dir)
        .or_else(|| read_apprun_wm_class(&temp_base.join("squashfs-root")));

    let saved_icon_path = if let Some((bytes, extension)) = icon_data {
        let target_path = if let Some(dest_dir) = dest_icon_dir {
            fs::create_dir_all(dest_dir)?;
            dest_dir.join(format!("icon.{extension}"))
        } else {
            icon_cache_dir()?.join(icon_cache_name(appimage_path, extension))
        };
        fs::write(&target_path, bytes)?;
        Some(target_path)
    } else {
        None
    };

    let (matched_category, is_custom_category) = desktop_meta
        .as_ref()
        .and_then(|desktop| desktop.categories.as_deref())
        .map(map_kde_category)
        .map(|(category, custom)| (Some(category), custom))
        .unwrap_or((None, false));

    let mime_types = desktop_meta
        .as_ref()
        .and_then(|desktop| desktop.mime_types.clone());
    let exec_arg = exec_placeholder(
        desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.exec.as_deref()),
        mime_types.as_deref(),
    );

    Ok(AppImageMetadata {
        name: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.name.clone()),
        generic_name: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.generic_name.clone()),
        comment: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.comment.clone()),
        categories: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.categories.clone()),
        matched_category,
        is_custom_category,
        startup_notify: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.startup_notify),
        startup_wm_class: desktop_meta
            .as_ref()
            .and_then(|desktop| desktop.startup_wm_class.as_deref())
            .and_then(normalize_startup_wm_class)
            .or(apprun_wm_class),
        icon_path: saved_icon_path,
        mime_types,
        exec_arg,
    })
}

/// Extracts just the icon from an AppImage into `dest_icon_dir/icon.<ext>`.
pub fn extract_icon_with_policy(
    appimage_path: &Path,
    dest_icon_dir: &Path,
    policy: ExtractionPolicy,
) -> Result<PathBuf> {
    let meta = extract_metadata_with_policy(appimage_path, Some(dest_icon_dir), policy)?;
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
        for entry in entries.flatten().take(MAX_EXTRACTED_ENTRIES) {
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
StartupWMClass=pcsx2
MimeType=x-scheme-handler/heroic;application/x-iso9660-image;
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
        assert_eq!(meta.startup_wm_class.as_deref(), Some("pcsx2"));
        assert_eq!(meta.icon.as_deref(), Some("pcsx2"));
        assert_eq!(
            meta.mime_types.as_deref(),
            Some("x-scheme-handler/heroic;application/x-iso9660-image;")
        );
        assert_eq!(meta.exec.as_deref(), Some("pcsx2 %F"));
        assert_eq!(
            exec_placeholder(meta.exec.as_deref(), meta.mime_types.as_deref()).as_deref(),
            Some("%F")
        );
    }

    #[test]
    fn exec_program_name_is_a_wm_class_fallback() {
        assert_eq!(exec_program_name("zen %u").as_deref(), Some("zen"));
        assert_eq!(
            exec_program_name("env FOO=bar /usr/bin/heroic --flag %U").as_deref(),
            Some("heroic")
        );
        assert_eq!(
            exec_program_name("\"/opt/My App/app\" %F").as_deref(),
            Some("app")
        );
        assert_eq!(exec_program_name("sh -c 'echo hi'"), None);
    }

    #[test]
    fn read_first_desktop_falls_back_to_desktop_stem_for_wm_class() {
        let dir = std::env::temp_dir().join(format!("aim-wmclass-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pcsx2.desktop"),
            "[Desktop Entry]\nType=Application\nName=PCSX2\nExec=pcsx2 %F\n",
        )
        .unwrap();

        let meta = read_first_desktop(&dir).expect("desktop entry");
        assert_eq!(meta.startup_wm_class.as_deref(), Some("pcsx2"));
        assert!(normalize_startup_wm_class("pcsx2\nExec=evil").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extraction_reads_nested_desktop_file() {
        // Real AppImages often expose the desktop entry only as a symlink at
        // the archive root, with the real file under usr/share/applications.
        // Skip when the optional runtime helper is unavailable.
        if Command::new("7z")
            .arg("i")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return;
        }
        let base =
            std::env::temp_dir().join(format!("aim-nested-desktop-{}", uuid::Uuid::new_v4()));
        let src = base.join("src");
        let apps = src.join("usr/share/applications");
        std::fs::create_dir_all(&apps).unwrap();
        std::fs::write(
            apps.join("DSH Tauri.desktop"),
            "[Desktop Entry]\nType=Application\nName=DSH Tauri\nExec=deepseek-harness-desktop\nStartupWMClass=DSH Tauri\n",
        )
        .unwrap();
        let archive = base.join("app.AppImage");
        let status = Command::new("7z")
            .args(["a", "-y"])
            .arg(&archive)
            .arg("usr/share/applications/DSH Tauri.desktop")
            .current_dir(&src)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !status.is_ok_and(|status| status.success()) {
            let _ = std::fs::remove_dir_all(&base);
            return;
        }

        let out = base.join("out");
        std::fs::create_dir_all(&out).unwrap();
        assert!(extract_with_7z(&archive, &out));
        let meta = read_first_desktop(&out).expect("nested desktop entry");
        assert_eq!(meta.startup_wm_class.as_deref(), Some("DSH Tauri"));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn apprun_script_supplies_wm_class_without_desktop_file() {
        let dir = std::env::temp_dir().join(format!("aim-apprun-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("AppRun"),
            "#!/bin/sh\nCURRENTDIR=\"$(dirname \"$0\")\"\nexec \"${CURRENTDIR}/zen\" \"$@\"\n",
        )
        .unwrap();
        assert_eq!(read_apprun_wm_class(&dir).as_deref(), Some("zen"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_outer_squashfs_payload_after_false_magic() {
        let base = std::env::temp_dir().join(format!("aim-squashfs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("app.AppImage");

        let mut bytes = vec![0_u8; 4096];
        // A false `hsqs` occurrence inside the runtime.
        bytes[1024..1028].copy_from_slice(b"hsqs");

        let offset = 16 * 1024usize;
        bytes.resize(offset, 0);
        let used = 8192u64;
        let mut header = [0_u8; 96];
        header[..4].copy_from_slice(b"hsqs");
        header[12..16].copy_from_slice(&131_072_u32.to_le_bytes());
        header[20..22].copy_from_slice(&6_u16.to_le_bytes());
        header[22..24].copy_from_slice(&17_u16.to_le_bytes());
        header[28..30].copy_from_slice(&4_u16.to_le_bytes());
        header[40..48].copy_from_slice(&used.to_le_bytes());
        bytes.extend_from_slice(&header);
        bytes.resize(offset + used as usize, 0);
        std::fs::write(&path, &bytes).unwrap();

        assert_eq!(find_squashfs_offset(&path), Some(offset as u64));
        let _ = std::fs::remove_dir_all(&base);
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

    #[test]
    fn extraction_timeout_kills_child() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let finished = run_command_bounded(&mut command, Duration::from_millis(100));
        assert!(!finished, "sleep must hit the timeout");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "the hung child must be killed promptly"
        );
    }

    #[cfg(unix)]
    #[test]
    fn downloaded_app_requires_confirmation_before_exec() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = std::env::temp_dir().join(format!("aim-extract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("executed");
        let script = dir.join("app.AppImage");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho ran > '{}'\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Downloaded files are inspected with non-executing extraction only.
        let meta = extract_metadata_with_policy(&script, None, ExtractionPolicy::NeverExecute)
            .expect("non-executing extraction should not error out");
        assert!(meta.icon_path.is_none());
        assert!(
            !marker.exists(),
            "NeverExecute must never run the AppImage payload"
        );

        // An explicitly approved local file may be run once.
        let _ = extract_metadata_with_policy(&script, None, ExtractionPolicy::ExecuteAllowed);
        assert!(
            marker.exists(),
            "ExecuteAllowed may run the payload to read metadata"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
