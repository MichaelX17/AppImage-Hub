//! Local-file state checks shared by the background scanner and update checks.
use crate::appmeta;
use crate::config::{AppEntry, Config};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Local file problem detected without contacting GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFileIssue {
    /// The AppImage exists but no longer matches the recorded fingerprint.
    Changed,
    /// The configured AppImage path does not exist.
    Missing,
}

pub(crate) fn sidecar_is_older_than_file(
    entry: &AppEntry,
    current: &crate::config::FileFingerprint,
) -> bool {
    sidecar_is_older_than_path(&entry.local_path, current)
}

pub(crate) fn sidecar_is_older_than_path(
    path: &Path,
    current: &crate::config::FileFingerprint,
) -> bool {
    let sidecar = appmeta::sidecar_path(path);
    sidecar
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|mtime| current.mtime > mtime.as_secs() as i64)
        .unwrap_or(false)
}

/// Cheap local check (metadata only, no hashing) used by the background scan.
pub(crate) fn cheap_issue_for_file(
    path: &Path,
    stored: Option<&crate::config::FileFingerprint>,
) -> Option<LocalFileIssue> {
    let Some(current) = crate::config::file_fingerprint(path) else {
        return Some(LocalFileIssue::Missing);
    };
    match stored {
        Some(stored) if stored.size == current.size && stored.mtime == current.mtime => None,
        Some(_) => Some(LocalFileIssue::Changed),
        None => sidecar_is_older_than_path(path, &current).then_some(LocalFileIssue::Changed),
    }
}

#[cfg(test)]
pub(crate) fn cheap_local_file_issue(entry: &AppEntry) -> Option<LocalFileIssue> {
    cheap_issue_for_file(&entry.local_path, entry.file_fingerprint.as_ref())
}

pub(crate) fn verified_local_file_issue(entry: &AppEntry) -> Option<LocalFileIssue> {
    let Some(current) = crate::config::file_fingerprint(&entry.local_path) else {
        return Some(LocalFileIssue::Missing);
    };
    let Some(stored) = entry.file_fingerprint.as_ref() else {
        return sidecar_is_older_than_file(entry, &current).then_some(LocalFileIssue::Changed);
    };
    if stored.size == current.size && stored.mtime == current.mtime {
        return None;
    }
    if let Some(stored_hash) = stored.sha256.as_deref().filter(|hash| !hash.is_empty()) {
        if crate::config::sha256_file(&entry.local_path)
            .is_some_and(|current_hash| stored_hash.eq_ignore_ascii_case(&current_hash))
        {
            return None;
        }
    }
    Some(LocalFileIssue::Changed)
}

/// Builds a fresh fingerprint for a known-good file. The content hash is
/// reused when it already exists; only the first baseline hashes the file.
pub(crate) fn baseline_fingerprint(entry: &AppEntry) -> Option<crate::config::FileFingerprint> {
    let current = crate::config::file_fingerprint(&entry.local_path)?;
    let existing_hash = entry
        .file_fingerprint
        .as_ref()
        .and_then(|fingerprint| fingerprint.sha256.clone())
        .filter(|hash| !hash.is_empty());
    let sha256 = existing_hash.or_else(|| crate::config::sha256_file(&entry.local_path));
    Some(crate::config::FileFingerprint {
        size: current.size,
        mtime: current.mtime,
        sha256,
    })
}

/// Persists a verified fingerprint, re-reading the live entry so a concurrent
/// edit is not overwritten in the sidecar. Must be called from a background
/// context (the sidecar write is small but the caller may be async).
pub(crate) fn persist_baseline_fingerprint(
    config: &Arc<Mutex<Config>>,
    app_id: &str,
    fingerprint: crate::config::FileFingerprint,
    clear_version: bool,
) -> bool {
    let new_hash = fingerprint.sha256.clone();
    let mut updated: Option<AppEntry> = None;
    let mut found = false;
    if let Ok(mut cfg) = config.lock() {
        if let Some(stored) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
            found = true;
            let mut changed = false;
            let old_hash = stored
                .file_fingerprint
                .as_ref()
                .and_then(|old| old.sha256.as_deref());
            let content_unchanged = old_hash
                .zip(new_hash.as_deref())
                .is_some_and(|(old, new)| old.eq_ignore_ascii_case(new));
            if stored.file_fingerprint.as_ref() != Some(&fingerprint) {
                stored.file_fingerprint = Some(fingerprint);
                changed = true;
            }
            if clear_version && !content_unchanged && !stored.current_version.is_empty() {
                // The version recorded for the previous managed build no
                // longer describes this manually accepted file.
                stored.current_version.clear();
                changed = true;
            }
            if changed {
                updated = Some(stored.clone());
            }
        }
        if updated.is_some() {
            let _ = cfg.save();
        }
    }
    if let Some(entry) = updated {
        if entry.local_path.exists() {
            let _ = appmeta::write_for_entry(&entry);
        }
    }
    found
}
