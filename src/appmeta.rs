//! Per-AppImage metadata sidecar.
//!
//! Every managed AppImage keeps its metadata in a JSON file next to the
//! binary (`<name>.json` beside `<name>.appimage`). A managed app folder is
//! therefore self-describing: it can be copied, moved or re-imported without
//! depending on the central `config.toml`.

use crate::config::{AppEntry, AppSource};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Current on-disk schema version for the sidecar file.
pub const SCHEMA_VERSION: u32 = 1;

/// Failure to load a sidecar, distinguishing "invalid and quarantined" from
/// "written by a newer build, leave it alone".
#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid metadata in {path} (preserved at {quarantine}): {source}")]
    Invalid {
        path: PathBuf,
        quarantine: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid metadata in {path} and it could not be preserved: {quarantine_error}")]
    InvalidNotPreserved {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
        quarantine_error: String,
    },
    #[error(
        "metadata file {path} was written by a newer AppImage Hub \
         (schema {found}, this build supports up to {supported})"
    )]
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
        supported: u32,
    },
}

fn default_schema() -> u32 {
    SCHEMA_VERSION
}

fn default_startup_notify() -> bool {
    true
}

/// Serialized form of the metadata stored beside an AppImage.
///
/// All fields are `#[serde(default)]` so older/newer files keep loading.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppMetadataFile {
    #[serde(default = "default_schema")]
    pub schema: u32,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub generic_name: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default)]
    pub categories: Option<String>,
    #[serde(default = "default_startup_notify")]
    pub startup_notify: bool,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub source: Option<AppSource>,
    #[serde(default)]
    pub current_version: String,
    #[serde(default)]
    pub asset_preference: Option<String>,
    /// AppImage file name inside the same folder (e.g. `example.appimage`).
    #[serde(default)]
    pub file: String,
    /// Icon file name inside the same folder, or an absolute path.
    #[serde(default)]
    pub icon: Option<String>,
    /// Unix timestamp for when the app was added.
    #[serde(default)]
    pub added_at: Option<i64>,
    /// Commit hash of the installed release used for update comparison.
    #[serde(default)]
    pub commit_hash: Option<String>,
    /// Fingerprint of the AppImage file when AppImage Hub last wrote it.
    #[serde(default)]
    pub file_fingerprint: Option<crate::config::FileFingerprint>,
    #[serde(default = "default_true")]
    pub desktop_shortcut: bool,
    #[serde(default = "default_true")]
    pub menu_entry: bool,
    #[serde(default)]
    pub mime_types: Option<String>,
    #[serde(default)]
    pub exec_arg: Option<String>,
}

fn default_true() -> bool {
    true
}

impl AppMetadataFile {
    /// Snapshot of a config entry, ready to be written beside its AppImage.
    pub fn from_entry(entry: &AppEntry) -> Self {
        let folder = entry.local_path.parent();
        let file = entry
            .local_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let icon = entry.icon_path.as_ref().map(|p| icon_reference(folder, p));

        Self {
            schema: SCHEMA_VERSION,
            id: entry.id.clone(),
            display_name: entry.display_name.clone(),
            generic_name: entry.generic_name.clone(),
            comment: entry.comment.clone(),
            categories: entry.categories.clone(),
            startup_notify: entry.startup_notify,
            args: entry.args.clone(),
            source: Some(entry.source.clone()),
            current_version: entry.current_version.clone(),
            asset_preference: entry.asset_preference.clone(),
            file,
            icon,
            added_at: entry.added_at,
            commit_hash: entry.commit_hash.clone(),
            file_fingerprint: entry.file_fingerprint.clone(),
            desktop_shortcut: entry.desktop_shortcut,
            menu_entry: entry.menu_entry,
            mime_types: entry.mime_types.clone(),
            exec_arg: entry.exec_arg.clone(),
        }
    }

    /// Rebuilds an `AppEntry` for `local_path`, resolving a relative icon
    /// reference against the AppImage's own folder.
    pub fn to_entry(&self, local_path: PathBuf) -> AppEntry {
        // Resolve the icon while `local_path` is still borrowed, before the
        // path is moved into the returned entry.
        let icon_path = resolve_icon(local_path.parent(), self.icon.as_deref());
        let display_name = if self.display_name.trim().is_empty() {
            local_path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "AppImage".to_string())
        } else {
            self.display_name.trim().to_string()
        };
        let id = if self.id.trim().is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            self.id.clone()
        };

        AppEntry {
            id,
            display_name,
            generic_name: self.generic_name.clone(),
            comment: self.comment.clone(),
            categories: self.categories.clone(),
            startup_notify: self.startup_notify,
            source: self.source.clone().unwrap_or(AppSource::LocalOnly),
            local_path,
            current_version: self.current_version.clone(),
            args: self.args.clone(),
            icon_path,
            asset_preference: self.asset_preference.clone(),
            added_at: self.added_at,
            commit_hash: self.commit_hash.clone(),
            file_fingerprint: self.file_fingerprint.clone(),
            desktop_shortcut: self.desktop_shortcut,
            menu_entry: self.menu_entry,
            mime_types: self.mime_types.clone(),
            exec_arg: self.exec_arg.clone(),
        }
    }
}

fn icon_reference(folder: Option<&Path>, icon: &Path) -> String {
    if let Some(folder) = folder {
        if icon.parent() == Some(folder) {
            if let Some(name) = icon.file_name() {
                return name.to_string_lossy().to_string();
            }
        }
    }
    icon.to_string_lossy().to_string()
}

fn resolve_icon(folder: Option<&Path>, icon: Option<&str>) -> Option<PathBuf> {
    let icon = icon?.trim();
    if icon.is_empty() {
        return None;
    }
    let path = Path::new(icon);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    folder.map(|f| f.join(path))
}

/// `<name>.appimage` → `<name>.json` (same folder, same stem).
pub fn sidecar_path(appimage: &Path) -> PathBuf {
    let mut path = appimage.to_path_buf();
    path.set_extension("json");
    path
}

/// Writes the metadata file next to the entry's AppImage.
pub fn write_for_entry(entry: &AppEntry) -> Result<PathBuf> {
    let path = sidecar_path(&entry.local_path);
    let meta = AppMetadataFile::from_entry(entry);
    let json = serde_json::to_string_pretty(&meta).context("failed to serialize app metadata")?;
    crate::fsutil::write_atomic(&path, json.as_bytes())
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// Reads a metadata file from an explicit path.
#[cfg(test)]
pub fn read(path: &Path) -> Result<AppMetadataFile> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let meta: AppMetadataFile = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    validate_schema(path, &meta)?;
    Ok(meta)
}

/// Rejects sidecars from a newer build instead of silently downgrading them.
/// Schema `0` is accepted for compatibility; the next write upgrades it to
/// `SCHEMA_VERSION`.
fn validate_schema(path: &Path, meta: &AppMetadataFile) -> Result<(), SidecarError> {
    if meta.schema > SCHEMA_VERSION {
        return Err(SidecarError::UnsupportedSchema {
            path: path.to_path_buf(),
            found: meta.schema,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

/// Reads the sidecar that belongs to `appimage`, distinguishing a missing file
/// (`Ok(None)`) from an unreadable or invalid one (`Err`).
///
/// Invalid JSON is quarantined as `<stem>.json.corrupt-<unix-seconds>` and the
/// error names the quarantine path, so the caller can tell the user where the
/// original data went instead of silently replacing it.
pub fn read_for_appimage_result(appimage: &Path) -> Result<Option<AppMetadataFile>, SidecarError> {
    let path = sidecar_path(appimage);
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(SidecarError::Read { path, source }),
    };

    match serde_json::from_str(&content) {
        Ok(meta) => {
            validate_schema(&path, &meta)?;
            Ok(Some(meta))
        }
        Err(source) => match crate::fsutil::quarantine(&path) {
            Ok(quarantine) => Err(SidecarError::Invalid {
                path,
                quarantine,
                source,
            }),
            Err(quarantine_error) => Err(SidecarError::InvalidNotPreserved {
                path,
                source,
                quarantine_error: format!("{quarantine_error:#}"),
            }),
        },
    }
}

/// Best-effort variant of [`read_for_appimage_result`] used where a missing or
/// invalid sidecar simply means "no cached metadata".
pub fn read_for_appimage(appimage: &Path) -> Option<AppMetadataFile> {
    read_for_appimage_result(appimage).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(local_path: PathBuf) -> AppEntry {
        AppEntry {
            id: "id-1234".into(),
            display_name: "Example".into(),
            generic_name: Some("Example Tool".into()),
            comment: Some("Does example things".into()),
            categories: Some("Graphics;".into()),
            startup_notify: true,
            source: AppSource::LocalWithRepo {
                github_repo: "owner/example".into(),
            },
            local_path: local_path.clone(),
            current_version: "v2.4.0".into(),
            args: vec!["--flag".into()],
            icon_path: local_path.parent().map(|p| p.join("icon.png")),
            asset_preference: Some("Example-x86_64.AppImage".into()),
            added_at: None,
            commit_hash: None,
            file_fingerprint: None,
            desktop_shortcut: true,
            menu_entry: true,
            mime_types: None,
            exec_arg: None,
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-meta-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sidecar_path_shares_stem() {
        assert_eq!(
            sidecar_path(Path::new("/apps/example/example.appimage")),
            PathBuf::from("/apps/example/example.json")
        );
        assert_eq!(
            sidecar_path(Path::new("/apps/Example_v2.4_x86_64.AppImage")),
            PathBuf::from("/apps/Example_v2.4_x86_64.json")
        );
    }

    #[test]
    fn write_and_read_round_trip() -> Result<()> {
        let dir = temp_dir("roundtrip");
        let appimage = dir.join("example.appimage");
        std::fs::write(&appimage, b"binary")?;
        let entry = sample_entry(appimage.clone());

        let written = write_for_entry(&entry)?;
        assert_eq!(written, dir.join("example.json"));
        assert!(written.exists());

        let meta = read(&written)?;
        assert_eq!(meta.display_name, "Example");
        assert_eq!(meta.categories.as_deref(), Some("Graphics;"));
        assert_eq!(meta.file, "example.appimage");
        assert_eq!(meta.icon.as_deref(), Some("icon.png"));
        assert_eq!(meta.current_version, "v2.4.0");

        let restored = meta.to_entry(appimage.clone());
        assert_eq!(restored.id, entry.id);
        assert_eq!(restored.display_name, entry.display_name);
        assert_eq!(restored.source, entry.source);
        assert_eq!(restored.icon_path, Some(dir.join("icon.png")));

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn missing_fields_are_defaulted() -> Result<()> {
        let dir = temp_dir("defaults");
        let path = dir.join("example.json");
        std::fs::write(&path, "{}")?;

        let meta = read(&path)?;
        assert_eq!(meta.schema, SCHEMA_VERSION);
        assert!(meta.startup_notify);
        assert_eq!(meta.args.len(), 0);

        let entry = meta.to_entry(dir.join("example.appimage"));
        assert_eq!(entry.display_name, "example");
        assert!(matches!(entry.source, AppSource::LocalOnly));
        assert!(!entry.id.is_empty());

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn absolute_icon_paths_are_preserved() -> Result<()> {
        let dir = temp_dir("absicon");
        let path = dir.join("example.json");
        std::fs::write(&path, r#"{"icon": "/usr/share/icons/foo.png"}"#)?;
        let meta = read(&path)?;
        let entry = meta.to_entry(dir.join("example.appimage"));
        assert_eq!(
            entry.icon_path,
            Some(PathBuf::from("/usr/share/icons/foo.png"))
        );
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn read_for_appimage_returns_none_without_sidecar() {
        let dir = temp_dir("nosidecar");
        assert!(read_for_appimage(&dir.join("nothing.appimage")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_creates_missing_folder() -> Result<()> {
        let dir = temp_dir("nested");
        let appimage = dir.join("sub").join("example.appimage");
        let entry = sample_entry(appimage);
        let written = write_for_entry(&entry)?;
        assert!(written.exists());
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn corrupt_sidecar_is_quarantined() -> Result<()> {
        let dir = temp_dir("corrupt");
        let appimage = dir.join("example.appimage");
        std::fs::write(&appimage, b"binary")?;
        let path = sidecar_path(&appimage);
        let garbage = b"{ this is not json";
        std::fs::write(&path, garbage)?;

        let error = read_for_appimage_result(&appimage).expect_err("invalid sidecar must fail");

        let SidecarError::Invalid { quarantine, .. } = &error else {
            panic!("expected Invalid error, got {error}");
        };
        assert!(error.to_string().contains("preserved at"));
        assert!(!path.exists());
        assert!(quarantine.exists());
        assert_eq!(std::fs::read(quarantine)?, garbage);

        // Best-effort readers see "no metadata" rather than crashing.
        assert!(read_for_appimage(&appimage).is_none());

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn newer_sidecar_schema_is_rejected() -> Result<()> {
        let dir = temp_dir("newer");
        let appimage = dir.join("example.appimage");
        std::fs::write(&appimage, b"binary")?;
        let path = sidecar_path(&appimage);
        let original = br#"{"schema": 99, "id": "future", "display_name": "Future"}"#;
        std::fs::write(&path, original)?;

        let error = read_for_appimage_result(&appimage).expect_err("newer schema must be rejected");

        assert!(
            matches!(
                error,
                SidecarError::UnsupportedSchema {
                    found: 99,
                    supported: SCHEMA_VERSION,
                    ..
                }
            ),
            "unexpected error: {error}"
        );
        // The newer file is neither quarantined nor overwritten.
        assert!(path.exists());
        assert_eq!(std::fs::read(&path)?, original);

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn schema_zero_is_upgraded_on_write() -> Result<()> {
        let dir = temp_dir("schemazero");
        let appimage = dir.join("example.appimage");
        std::fs::write(&appimage, b"binary")?;
        let path = sidecar_path(&appimage);
        std::fs::write(
            &path,
            br#"{"schema": 0, "id": "keep-me", "display_name": "Old"}"#,
        )?;

        let meta = read_for_appimage_result(&appimage)?.expect("sidecar should load");
        assert_eq!(meta.schema, 0);

        write_for_entry(&meta.to_entry(appimage.clone()))?;
        let rewritten: AppMetadataFile = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        assert_eq!(rewritten.schema, SCHEMA_VERSION);
        assert_eq!(rewritten.id, "keep-me");

        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
