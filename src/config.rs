use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Directory containing the actual running executable. For an AppImage this
/// resolves the original `.AppImage` file via `$APPIMAGE`, not the temporary
/// SquashFS mount path.
pub fn executable_dir() -> PathBuf {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn resolve_config_dir(app_dir: &Path, portable: bool) -> PathBuf {
    if portable {
        app_dir.join("AppImageHub.config")
    } else {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"));
        base.join("appimage-manager")
    }
}

/// Resolves the configuration directory once at startup. Portable mode is
/// enabled by placing a `portable.ini` file beside the running executable.
pub fn config_dir() -> &'static Path {
    CONFIG_DIR
        .get_or_init(|| {
            let app_dir = executable_dir();
            let portable = app_dir.join("portable.ini").exists();
            let dir = resolve_config_dir(&app_dir, portable);
            let _ = std::fs::create_dir_all(&dir);
            dir
        })
        .as_path()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub applications: Vec<AppEntry>,
    #[serde(default)]
    pub custom_download_dir: Option<PathBuf>,
    /// Whether to check GitHub for new releases automatically on startup.
    #[serde(default = "default_auto_check_updates")]
    pub auto_check_updates: bool,
    /// Unix timestamp of the last app update check, used for the rate-limit
    /// cooldown (5 minutes anonymously, 2 minutes with a GitHub token).
    #[serde(default)]
    pub last_update_check: Option<i64>,
    /// Unix timestamp of the last AppImage Hub self-update check.
    #[serde(default)]
    pub last_hub_update_check: Option<i64>,
    /// Create/update AppImage Hub's own Start Menu shortcut.
    #[serde(default = "default_true")]
    pub hub_start_menu_shortcut: bool,
    /// XDG category for AppImage Hub's own Start Menu shortcut.
    #[serde(default = "default_hub_category")]
    pub hub_start_menu_category: String,
    /// Create/update AppImage Hub's own Desktop shortcut.
    #[serde(default)]
    pub hub_desktop_shortcut: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            applications: Vec::new(),
            custom_download_dir: None,
            auto_check_updates: default_auto_check_updates(),
            last_update_check: None,
            last_hub_update_check: None,
            hub_start_menu_shortcut: true,
            hub_start_menu_category: default_hub_category(),
            hub_desktop_shortcut: false,
        }
    }
}

fn default_auto_check_updates() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_hub_category() -> String {
    "Utility".to_string()
}

/// Cheap identity of the managed AppImage file.
///
/// The app records this after installing/updating an app and compares it with
/// the file on disk during update checks, so files replaced outside AppImage
/// Hub are detected instead of trusting stale commit metadata forever.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub size: u64,
    /// Modification time as seconds since the Unix epoch.
    pub mtime: i64,
    /// SHA-256 of the file contents, captured at install/update/baseline time.
    ///
    /// Hashing is only performed when cheap metadata (size/mtime) changes or
    /// when the hash is missing, so a copied/touched-but-identical file is not
    /// mistaken for a different build.
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Reads the cheap (size/mtime) fingerprint of `path`, if it exists.
pub fn file_fingerprint(path: &Path) -> Option<FileFingerprint> {
    let metadata = std::fs::metadata(path).ok()?;
    let mtime = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(FileFingerprint {
        size: metadata.len(),
        mtime,
        sha256: None,
    })
}

/// SHA-256 of a file, hex-encoded. Returns `None` if the file cannot be read.
pub fn sha256_file(path: &Path) -> Option<String> {
    use ring::digest::{Context, SHA256};
    use std::fmt::Write as _;
    use std::io::Read as _;

    let mut file = std::fs::File::open(path).ok()?;
    let mut context = Context::new(&SHA256);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        context.update(&buffer[..read]);
    }
    let digest = context.finish();
    let mut hex = String::with_capacity(64);
    for byte in digest.as_ref() {
        let _ = write!(hex, "{byte:02x}");
    }
    Some(hex)
}

/// Full fingerprint including the content hash. Expensive: only call from a
/// background thread.
pub fn file_fingerprint_with_hash(path: &Path) -> Option<FileFingerprint> {
    let mut fingerprint = file_fingerprint(path)?;
    fingerprint.sha256 = sha256_file(path);
    Some(fingerprint)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub id: String, // UUID v4
    pub display_name: String,
    pub source: AppSource,
    pub local_path: PathBuf,
    pub current_version: String, // Git tag or empty for local-only
    #[serde(default)]
    pub args: Vec<String>, // custom launch args
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
    #[serde(default)]
    pub generic_name: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub categories: Option<String>,
    #[serde(default = "default_startup_notify")]
    pub startup_notify: bool,
    #[serde(default)]
    pub asset_preference: Option<String>,
    /// Unix timestamp for when the app was added. Older configs may omit it.
    #[serde(default)]
    pub added_at: Option<i64>,
    /// Commit hash of the installed release used for update comparison.
    #[serde(default)]
    pub commit_hash: Option<String>,
    /// Fingerprint of the managed AppImage when the app last installed or
    /// updated it. A mismatch means the file changed outside AppImage Hub.
    #[serde(default)]
    pub file_fingerprint: Option<FileFingerprint>,
    /// Whether this app has a desktop shortcut.
    #[serde(default = "default_true")]
    pub desktop_shortcut: bool,
    /// Whether this app has a Start Menu entry.
    #[serde(default = "default_true")]
    pub menu_entry: bool,
    /// MIME types / URI schemes declared by the app's embedded desktop file.
    #[serde(default)]
    pub mime_types: Option<String>,
    /// Exec placeholder such as %u, %U, %f, or %F.
    #[serde(default)]
    pub exec_arg: Option<String>,
}

fn default_startup_notify() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppSource {
    LocalOnly,
    LocalWithRepo { github_repo: String },
    GithubOnly { github_repo: String },
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = toml::from_str(&content)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let content = toml::to_string_pretty(self).context("failed to serialize config")?;
        std::fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn config_path() -> PathBuf {
        config_dir().join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_config_dir_lives_beside_executable() {
        let app_dir = Path::new("/opt/AppImageHub");
        assert_eq!(
            resolve_config_dir(app_dir, true),
            PathBuf::from("/opt/AppImageHub/AppImageHub.config")
        );
    }

    #[test]
    fn normal_config_dir_uses_os_config_directory() {
        let app_dir = Path::new("/opt/AppImageHub");
        let resolved = resolve_config_dir(app_dir, false);
        assert!(resolved.ends_with("appimage-manager"));
        assert!(!resolved.ends_with("AppImageHub.config"));
    }
}
