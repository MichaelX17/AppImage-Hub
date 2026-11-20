//! Managed storage roots and safe deletion.
//!
//! A managed AppImage may live under the default managed directory or under a
//! user-selected `custom_download_dir`. Every deletion decision is based on
//! these roots — never on string prefixes — and never follows symlinks. Only
//! the files AppImage Hub created are removed; the per-app folder is removed
//! with `remove_dir` (not recursively), so unrelated user files survive.

use crate::appmeta;
use crate::config::{AppEntry, Config};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Normalizes a directory/root path so containment checks are reliable in the
/// presence of `..` components and symlinked roots.
///
/// Existing paths are canonicalized. Paths that do not exist yet are resolved
/// through their nearest existing parent, then normalized lexically.
pub fn normalize_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    if let Some(parent) = path.parent() {
        if let Some(name) = path.file_name() {
            return normalize_path(parent).join(name);
        }
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    lexical_normalize(&absolute)
}

/// Normalizes a *file inside a root* without resolving a symlink at the final
/// component. Deleting a symlinked AppImage removes the link, never its target,
/// so containment must be based on the link's own path.
fn normalize_entry_path(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|dir| dir.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    match (absolute.parent(), absolute.file_name()) {
        (Some(parent), Some(name)) => normalize_path(parent).join(name),
        _ => normalize_path(&absolute),
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// All directories that may contain managed AppImages: the default managed
/// directory plus the configured custom root. Normalized and deduplicated.
pub fn roots(config: &Config) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut push = |path: &Path| {
        let normalized = normalize_path(path);
        if !roots.contains(&normalized) {
            roots.push(normalized);
        }
    };
    push(&crate::xdg::default_managed_apps_dir());
    if let Some(custom) = config.custom_download_dir.as_deref() {
        push(custom);
    }
    roots
}

/// True when `path` is strictly inside one of `roots`. A root itself is not an
/// app path and is therefore rejected.
pub fn is_in_storage_root(path: &Path, roots: &[PathBuf]) -> bool {
    let normalized = normalize_entry_path(path);
    roots
        .iter()
        .any(|root| normalized != *root && normalized.starts_with(root))
}

/// Returns the parent of the entry's app folder when that folder's name still
/// matches AppImage Hub's managed naming scheme for this entry.
///
/// After the storage folder changes (D4 keeps entries in place), the old root
/// is no longer configured. Recognizing the managed folder name lets the user
/// still delete those apps without ever trusting an arbitrary path.
pub fn legacy_root_for_entry(entry: &AppEntry, roots: &[PathBuf]) -> Option<PathBuf> {
    let folder = entry.local_path.parent()?;
    let name = folder.file_name()?.to_str()?;
    let expected = crate::xdg::managed_app_folder_name(&entry.display_name, &entry.id);
    let short_id: String = entry
        .id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(8)
        .collect();
    // The display name may have been edited since the folder was created, but
    // the id suffix is stable.
    let name_matches =
        name == expected || (!short_id.is_empty() && name.ends_with(&format!("-{short_id}")));
    if !name_matches {
        return None;
    }
    let root = normalize_path(folder.parent()?);
    (!roots.contains(&root)).then_some(root)
}

/// Drops scan candidates that are already known by path. Used by the folder
/// scan so changing the storage folder never duplicates existing entries.
pub fn unknown_candidates(
    candidates: Vec<PathBuf>,
    known_paths: &HashSet<PathBuf>,
) -> Vec<PathBuf> {
    candidates
        .into_iter()
        .filter(|path| !known_paths.contains(path))
        .collect()
}

#[derive(Debug, Default)]
pub struct DeleteOutcome {
    /// Files and folders that were actually removed.
    pub removed: Vec<PathBuf>,
    /// Non-fatal problems the user should see.
    pub warnings: Vec<String>,
    /// The AppImage lived outside every managed root, so nothing was deleted.
    pub skipped_outside_root: bool,
    /// The per-app folder was kept because it still contains other files.
    pub kept_non_empty_folder: Option<PathBuf>,
}

/// Deletes exactly the artifacts AppImage Hub created for `entry`.
///
/// Only paths proven to live inside `roots` are touched. The binary, its
/// sidecar, and its icon (when the icon lives in the app folder) are removed;
/// the folder itself is removed only when it is an empty direct child of a
/// root.
pub fn delete_managed_entry(entry: &AppEntry, roots: &[PathBuf]) -> DeleteOutcome {
    let mut outcome = DeleteOutcome::default();
    if !is_in_storage_root(&entry.local_path, roots) {
        outcome.skipped_outside_root = true;
        return outcome;
    }

    let folder = entry.local_path.parent().map(normalize_path);

    remove_file(&entry.local_path, roots, &mut outcome);
    let sidecar = appmeta::sidecar_path(&entry.local_path);
    remove_file(&sidecar, roots, &mut outcome);

    if let (Some(icon), Some(folder)) = (entry.icon_path.as_deref(), folder.as_deref()) {
        // Never delete icons that live outside the app folder (for example an
        // absolute icon path the user reused from another app).
        if normalize_path(icon).parent() == Some(folder) {
            remove_file(icon, roots, &mut outcome);
        }
    }

    if let Some(folder) = folder {
        if is_direct_child_of_root(&folder, roots) {
            match std::fs::remove_dir(&folder) {
                Ok(()) => outcome.removed.push(folder),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    let non_empty = std::fs::read_dir(&folder)
                        .map(|mut entries| entries.next().is_some())
                        .unwrap_or(false);
                    if non_empty {
                        outcome.kept_non_empty_folder = Some(folder);
                    } else {
                        outcome.warnings.push(format!(
                            "could not remove folder {}: {error}",
                            folder.display()
                        ));
                    }
                }
            }
        }
    }

    outcome
}

fn is_direct_child_of_root(path: &Path, roots: &[PathBuf]) -> bool {
    let normalized = normalize_path(path);
    roots
        .iter()
        .any(|root| normalized.parent() == Some(root.as_path()))
}

fn remove_file(path: &Path, roots: &[PathBuf], outcome: &mut DeleteOutcome) {
    if !is_in_storage_root(path, roots) {
        return;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_dir() {
                // A directory where a file was expected is never deleted
                // recursively; reject it instead.
                outcome.warnings.push(format!(
                    "refusing to delete directory {} where a file was expected",
                    path.display()
                ));
                return;
            }
            // Removes a symlink itself, never its target.
            match std::fs::remove_file(path) {
                Ok(()) => outcome.removed.push(path.to_path_buf()),
                Err(error) => outcome
                    .warnings
                    .push(format!("could not remove {}: {error}", path.display())),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => outcome
            .warnings
            .push(format!("could not inspect {}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppSource, FileFingerprint};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-storage-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_entry(local_path: PathBuf, icon_path: Option<PathBuf>) -> AppEntry {
        AppEntry {
            id: "app-id".into(),
            display_name: "Example".into(),
            source: AppSource::LocalOnly,
            local_path,
            current_version: "v1".into(),
            args: Vec::new(),
            icon_path,
            generic_name: None,
            comment: None,
            categories: None,
            startup_notify: true,
            startup_wm_class: None,
            startup_wm_class_auto: None,
            asset_preference: None,
            added_at: None,
            commit_hash: None,
            file_fingerprint: Some(FileFingerprint {
                size: 1,
                mtime: 1,
                sha256: None,
            }),
            desktop_shortcut: true,
            menu_entry: true,
            mime_types: None,
            exec_arg: None,
        }
    }

    /// Creates `<root>/example-1234/{example.appimage,example.json,icon.png,unrelated.txt}`
    /// and returns the entry plus paths.
    fn managed_fixture(root: &Path, tag: &str) -> (AppEntry, PathBuf, PathBuf, PathBuf) {
        let folder = root.join(format!("example-{tag}"));
        std::fs::create_dir_all(&folder).unwrap();
        let appimage = folder.join("example.appimage");
        std::fs::write(&appimage, b"binary").unwrap();
        let sidecar = folder.join("example.json");
        std::fs::write(&sidecar, b"{}").unwrap();
        let icon = folder.join("icon.png");
        std::fs::write(&icon, b"png").unwrap();
        std::fs::write(folder.join("unrelated.txt"), b"keep me").unwrap();
        (
            sample_entry(appimage, Some(icon.clone())),
            folder,
            sidecar,
            icon,
        )
    }

    fn delete_removes_files_from_root(tag: &str) {
        let root = temp_dir(&format!("root-{tag}"));
        let (entry, folder, sidecar, icon) = managed_fixture(&root, tag);
        let roots = vec![normalize_path(&root)];

        let outcome = delete_managed_entry(&entry, &roots);

        assert!(!entry.local_path.exists(), "binary must be removed");
        assert!(!sidecar.exists(), "sidecar must be removed");
        assert!(!icon.exists(), "icon must be removed");
        assert!(!outcome.skipped_outside_root);
        assert!(folder.exists(), "folder with unrelated files must be kept");
        assert!(folder.join("unrelated.txt").exists());
        assert_eq!(
            outcome.kept_non_empty_folder.as_deref(),
            Some(folder.as_path())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_removes_files_from_default_root() {
        delete_removes_files_from_root("default");
    }

    #[test]
    fn delete_removes_files_from_custom_root() {
        delete_removes_files_from_root("custom");
    }

    #[test]
    fn delete_preserves_unrelated_files_in_app_folder() {
        delete_removes_files_from_root("unrelated");
    }

    #[test]
    fn delete_never_touches_paths_outside_roots() {
        let root = temp_dir("inside-root");
        let outside = temp_dir("outside-root");
        let folder = outside.join("example");
        std::fs::create_dir_all(&folder).unwrap();
        let appimage = folder.join("example.appimage");
        std::fs::write(&appimage, b"binary").unwrap();
        let sidecar = folder.join("example.json");
        std::fs::write(&sidecar, b"{}").unwrap();
        let entry = sample_entry(appimage.clone(), None);

        let outcome = delete_managed_entry(&entry, &[normalize_path(&root)]);

        assert!(outcome.skipped_outside_root);
        assert!(outcome.removed.is_empty());
        assert!(appimage.exists());
        assert!(sidecar.exists());
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn delete_removes_folder_when_it_becomes_empty() {
        let root = temp_dir("empty-folder");
        let folder = root.join("example");
        std::fs::create_dir_all(&folder).unwrap();
        let appimage = folder.join("example.appimage");
        std::fs::write(&appimage, b"binary").unwrap();
        let entry = sample_entry(appimage, None);

        let outcome = delete_managed_entry(&entry, &[normalize_path(&root)]);

        assert!(!folder.exists(), "an emptied app folder must be removed");
        assert!(outcome.kept_non_empty_folder.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delete_does_not_follow_symlinks() {
        let root = temp_dir("symlink-root");
        let outside = temp_dir("symlink-outside");
        let victim = outside.join("victim.txt");
        std::fs::write(&victim, b"original").unwrap();
        let folder = root.join("example");
        std::fs::create_dir_all(&folder).unwrap();
        let link = folder.join("example.appimage");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let entry = sample_entry(link.clone(), None);

        let outcome = delete_managed_entry(&entry, &[normalize_path(&root)]);

        assert!(!link.exists(), "the symlink itself must be removed");
        assert_eq!(std::fs::read(&victim).unwrap(), b"original");
        assert!(!outcome.skipped_outside_root);
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_dir_all(outside);
    }

    #[test]
    fn delete_works_after_storage_root_changed() {
        let old_root = temp_dir("old-root");
        let new_root = temp_dir("new-root");
        let folder = old_root.join(crate::xdg::managed_app_folder_name("Example", "app-id"));
        std::fs::create_dir_all(&folder).unwrap();
        let appimage = folder.join("example.appimage");
        std::fs::write(&appimage, b"binary").unwrap();
        let entry = sample_entry(appimage.clone(), None);

        let mut roots = vec![normalize_path(&new_root)];
        // The old root is no longer configured: files must not be deleted yet.
        let skipped = delete_managed_entry(&entry, &roots);
        assert!(skipped.skipped_outside_root);
        assert!(appimage.exists());

        // The managed folder name identifies the previous root.
        let previous = legacy_root_for_entry(&entry, &roots).expect("managed folder recognized");
        roots.push(previous);
        let outcome = delete_managed_entry(&entry, &roots);

        assert!(!outcome.skipped_outside_root);
        assert!(!appimage.exists());
        let _ = std::fs::remove_dir_all(old_root);
        let _ = std::fs::remove_dir_all(new_root);
    }

    #[test]
    fn roots_deduplicates_default_and_custom() {
        let config = Config {
            custom_download_dir: Some(crate::xdg::default_managed_apps_dir()),
            ..Config::default()
        };

        let roots = roots(&config);

        assert_eq!(roots.len(), 1);
    }

    #[test]
    fn containment_rejects_root_itself() {
        let root = temp_dir("containment");
        let normalized = normalize_path(&root);

        assert!(!is_in_storage_root(
            &root,
            std::slice::from_ref(&normalized)
        ));
        assert!(is_in_storage_root(
            &root.join("app").join("app.appimage"),
            std::slice::from_ref(&normalized)
        ));
        assert!(!is_in_storage_root(
            &root.with_extension("evil").join("app.appimage"),
            &[normalized]
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn settings_change_does_not_duplicate_scan_results() {
        let old = PathBuf::from("/old/apps/example/example.appimage");
        let new = PathBuf::from("/new/apps/example/example.appimage");
        let known_paths: HashSet<PathBuf> = [old.clone()].into_iter().collect();

        let unknown = unknown_candidates(vec![old.clone(), new.clone()], &known_paths);

        assert_eq!(unknown, vec![new]);
    }

    #[test]
    fn normalize_removes_parent_components() {
        assert_eq!(
            normalize_path(Path::new("/apps/../apps/example/./app.appimage")),
            PathBuf::from("/apps/example/app.appimage")
        );
    }
}
