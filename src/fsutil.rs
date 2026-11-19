//! Durable filesystem helpers shared by the persistence code.
//!
//! All user-visible state (config, per-AppImage sidecars, `mimeapps.list`) is
//! written with [`write_atomic`] so a crash, a full disk, or a killed process
//! can never leave a truncated file behind: the target is only ever replaced
//! by a completed, fsynced temp file via an atomic `rename` in the same
//! directory.

use anyhow::{Context, Result};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic suffix so two writers in the same process never pick the same
/// temp name.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to `path` atomically and durably.
///
/// The temp file is a sibling of the target so the final `rename` stays on the
/// same filesystem (a cross-device rename is not atomic and fails). It is
/// opened with `create_new(true)`, which refuses to follow a pre-existing
/// symlink at the temp path — an attacker cannot redirect the write.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_mode(path, bytes, 0o666)
}

/// Like [`write_atomic`], but creates the file with mode `0600` from the very
/// first byte (Unix), so a secret is never readable by other local users even
/// briefly.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with_mode(path, bytes, 0o600)
}

/// Creates `path` and any missing parents, then restricts it to `0700` on
/// Unix. Used for directories that hold secrets.
pub fn ensure_private_dir(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut permissions = std::fs::metadata(path)
            .with_context(|| format!("failed to inspect {}", path.display()))?
            .permissions();
        if permissions.mode() & 0o777 != 0o700 {
            permissions.set_mode(0o700);
            std::fs::set_permissions(path, permissions)
                .with_context(|| format!("failed to restrict {}", path.display()))?;
        }
    }
    Ok(())
}

fn write_atomic_with_mode(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());

    // Retry only for "a temp with that name already exists" (including a
    // planted symlink). Every other error is real and reported.
    let mut last_error: Option<std::io::Error> = None;
    for _ in 0..1024 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp = parent.join(format!("{file_name}.tmp-{}-{counter}", std::process::id()));

        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;

        let mut file = match options.open(&temp) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                last_error = Some(error);
                continue;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temp file in {}", parent.display())
                });
            }
        };

        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temp);
            return Err(error).with_context(|| format!("failed to write {}", temp.display()));
        }

        if let Err(error) = std::fs::rename(&temp, path) {
            let _ = std::fs::remove_file(&temp);
            return Err(error).with_context(|| format!("failed to replace {}", path.display()));
        }

        // Make the rename itself durable.
        sync_parent_dir(path);
        return Ok(());
    }

    Err(last_error.unwrap_or_else(|| std::io::Error::other("could not allocate a temp file")))
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Best-effort fsync of `path`'s parent directory, which persists a rename.
/// Filesystems that cannot open a directory for fsync are ignored.
pub fn sync_parent_dir(path: &Path) {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return;
    };
    if let Ok(directory) = std::fs::File::open(parent) {
        let _ = directory.sync_all();
    }
}

/// Moves a corrupt file aside instead of deleting it, so the original bytes
/// remain available for recovery or inspection.
///
/// The name is `<file>.corrupt-<unix-seconds>`; an existing quarantine file is
/// never overwritten (a numeric suffix is appended instead).
pub fn quarantine(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();

    for attempt in 0..1_000_u32 {
        let candidate = if attempt == 0 {
            parent.join(format!("{file_name}.corrupt-{timestamp}"))
        } else {
            parent.join(format!("{file_name}.corrupt-{timestamp}-{attempt}"))
        };
        if candidate.exists() {
            continue;
        }
        std::fs::rename(path, &candidate).with_context(|| {
            format!(
                "failed to quarantine {} as {}",
                path.display(),
                candidate.display()
            )
        })?;
        if let Ok(directory) = std::fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        return Ok(candidate);
    }

    anyhow::bail!(
        "could not find a free quarantine name for {}",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-fs-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let dir = temp_dir("replace");
        let target = dir.join("config.toml");
        std::fs::write(&target, b"old contents that are longer").unwrap();

        write_atomic(&target, b"new").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        // No temp files are left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_does_not_follow_symlink_at_temp_path() {
        use std::os::unix::fs::symlink;

        let dir = temp_dir("symlink");
        let victim = dir.join("victim.txt");
        std::fs::write(&victim, b"original victim").unwrap();
        let target = dir.join("target.txt");

        // Plant symlinks on the temp names the writer may pick. `create_new`
        // must refuse them instead of writing through to the victim.
        let pid = std::process::id();
        for counter in 0..512_u64 {
            let candidate = dir.join(format!("target.txt.tmp-{pid}-{counter}"));
            symlink(&victim, &candidate).unwrap();
        }

        write_atomic(&target, b"new contents").unwrap();

        assert_eq!(std::fs::read(&victim).unwrap(), b"original victim");
        assert_eq!(std::fs::read(&target).unwrap(), b"new contents");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quarantine_preserves_original_bytes() {
        let dir = temp_dir("quarantine");
        let path = dir.join("config.toml");
        std::fs::write(&path, b"not valid toml [[[").unwrap();

        let quarantined = quarantine(&path).unwrap();

        assert!(!path.exists());
        assert!(quarantined.exists());
        assert!(quarantined
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".corrupt-"));
        assert_eq!(std::fs::read(&quarantined).unwrap(), b"not valid toml [[[");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
