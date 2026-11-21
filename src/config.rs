use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Bumped after every config write so the UI can cache an app snapshot and
/// rebuild it only when something actually changed.
static CONFIG_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Monotonic counter of successful config writes.
pub fn generation() -> u64 {
    CONFIG_GENERATION.load(Ordering::Relaxed)
}

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

/// XDG config directory used for non-portable state, including secrets.
///
/// Secrets intentionally never follow portable mode: a portable AppImage's
/// folder is meant to be copied or uploaded, so credentials must not live
/// there.
pub fn xdg_config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    base.join("appimage-manager")
}

pub fn resolve_config_dir(app_dir: &Path, portable: bool) -> PathBuf {
    if portable {
        app_dir.join("AppImageHub.config")
    } else {
        xdg_config_dir()
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
            // The config folder may also hold the encrypted GitHub token, so
            // keep it private to the current user.
            let _ = crate::fsutil::ensure_private_dir(&dir);
            dir
        })
        .as_path()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// State of the last successfully applied Hub self-update.
    #[serde(default)]
    pub last_hub_update: Option<HubUpdateState>,
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
            last_hub_update: None,
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

/// Recorded metadata for the last self-update of AppImage Hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubUpdateState {
    pub asset_id: u64,
    pub asset_size: u64,
    #[serde(default)]
    pub updated_at: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Window class used to group the running app with its shortcuts in the
    /// task manager. Detected from the AppImage's embedded `.desktop` file.
    #[serde(default)]
    pub startup_wm_class: Option<String>,
    /// `Some(true)` when [`Self::startup_wm_class`] was auto-detected and is
    /// safe to trust, `Some(false)` when the user supplied it, and `None` for
    /// entries written before this flag existed (re-detected once at startup).
    #[serde(default)]
    pub startup_wm_class_auto: Option<bool>,
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
        Self::load_from(&Self::config_path())
    }

    /// Loads the config from an explicit path.
    ///
    /// A missing file yields defaults (first run). Any other I/O error is
    /// reported. A file that exists but cannot be parsed is **quarantined**
    /// (renamed to `config.toml.corrupt-<unix-seconds>`) before the error is
    /// returned, so the original bytes are preserved and a later save can
    /// never silently overwrite the user's data.
    pub fn load_from(path: &Path) -> Result<Self> {
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("failed to read {}", path.display()));
            }
        };

        match toml::from_str(&content) {
            Ok(config) => Ok(config),
            Err(parse_error) => match crate::fsutil::quarantine(path) {
                Ok(quarantine) => Err(parse_error).with_context(|| {
                    format!(
                        "failed to parse {} (the original file was preserved at {})",
                        path.display(),
                        quarantine.display()
                    )
                }),
                Err(quarantine_error) => Err(parse_error).with_context(|| {
                    format!(
                        "failed to parse {} and the file could not be preserved: {quarantine_error:#}",
                        path.display()
                    )
                }),
            },
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    /// Atomically writes the config to an explicit path.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self).context("failed to serialize config")?;
        crate::fsutil::write_atomic(path, content.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
        CONFIG_GENERATION.fetch_add(1, Ordering::Relaxed);
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

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-config-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_config() -> Config {
        Config {
            applications: vec![AppEntry {
                id: "1e5f2f5d-0000-4000-8000-000000000001".into(),
                display_name: "Example".into(),
                source: AppSource::LocalWithRepo {
                    github_repo: "owner/example".into(),
                },
                local_path: PathBuf::from("/managed/Example/example.appimage"),
                current_version: "v2.4.0".into(),
                args: vec!["--flag".into(), "value with spaces".into()],
                icon_path: Some(PathBuf::from("/managed/Example/icon.png")),
                generic_name: Some("Example Tool".into()),
                comment: Some("Does example things".into()),
                categories: Some("Graphics;".into()),
                startup_notify: false,
                startup_wm_class: Some("example".into()),
                startup_wm_class_auto: None,
                asset_preference: Some("Example-x86_64.AppImage".into()),
                added_at: Some(1_700_000_000),
                commit_hash: Some("deadbeef".into()),
                file_fingerprint: Some(FileFingerprint {
                    size: 42,
                    mtime: 1_700_000_001,
                    sha256: Some("abc123".into()),
                }),
                desktop_shortcut: true,
                menu_entry: false,
                mime_types: Some("x-scheme-handler/example;".into()),
                exec_arg: Some("%U".into()),
            }],
            custom_download_dir: Some(PathBuf::from("/managed")),
            auto_check_updates: false,
            last_update_check: Some(1_700_000_002),
            last_hub_update_check: Some(1_700_000_003),
            last_hub_update: None,
            hub_start_menu_shortcut: false,
            hub_start_menu_category: "Development".into(),
            hub_desktop_shortcut: true,
        }
    }

    #[test]
    fn removing_an_app_persists_to_config_file() -> Result<()> {
        let dir = temp_dir("delete-entry");
        let path = dir.join("config.toml");
        let mut config = sample_config();
        let id = config.applications[0].id.clone();
        config.save_to(&path)?;

        config.applications.retain(|app| app.id != id);
        config.save_to(&path)?;

        let loaded = Config::load_from(&path)?;
        assert!(loaded.applications.iter().all(|app| app.id != id));
        assert!(loaded.applications.is_empty());

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn save_then_load_round_trips_all_fields() -> Result<()> {
        let dir = temp_dir("roundtrip");
        let path = dir.join("config.toml");
        let config = sample_config();

        config.save_to(&path)?;
        let loaded = Config::load_from(&path)?;

        assert_eq!(loaded, config);
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn missing_config_returns_default() -> Result<()> {
        let dir = temp_dir("missing");
        let path = dir.join("config.toml");

        let loaded = Config::load_from(&path)?;

        assert_eq!(loaded, Config::default());
        assert!(!path.exists());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn corrupt_config_is_quarantined_and_reported() -> Result<()> {
        let dir = temp_dir("corrupt");
        let path = dir.join("config.toml");
        let garbage = b"this is not toml = = = [[[";
        std::fs::write(&path, garbage)?;

        let error = Config::load_from(&path).expect_err("corrupt config must not load");
        let message = format!("{error:#}");
        assert!(
            message.contains("preserved at"),
            "error should name the quarantine file, got: {message}"
        );

        // The original file is gone from its old name but its bytes survive.
        assert!(!path.exists());
        let quarantined: Vec<PathBuf> = std::fs::read_dir(&dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().contains(".corrupt-"))
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(quarantined.len(), 1, "expected exactly one quarantine file");
        assert_eq!(std::fs::read(&quarantined[0])?, garbage);

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
