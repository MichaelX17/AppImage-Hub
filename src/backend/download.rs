//! Managed-AppImage update transfer.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_download_and_replace(
    config: Arc<Mutex<Config>>,
    http: reqwest::Client,
    evt_tx: mpsc::Sender<BackendEvent>,
    cancel: Arc<AtomicBool>,
    op_id: String,
    app_id: String,
    url: String,
    tag: String,
    commit_hash: String,
    checksum_url: Option<String>,
) {
    let label = format!("Update {app_id}");
    let _ = evt_tx
        .send(BackendEvent::DownloadStarted {
            op_id: op_id.clone(),
            app_id: Some(app_id.clone()),
            label: label.clone(),
            kind: OpKind::Update,
        })
        .await;

    let target = config.lock().ok().and_then(|c| {
        c.applications
            .iter()
            .find(|a| a.id == app_id)
            .map(|a| a.local_path.clone())
    });
    if let Some(target) = target {
        // Keep the clean, stable file name: the new build is
        // swapped in place instead of adopting the upstream
        // asset name.
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = downloader::temp_path_for(&target);
        let res =
            downloader::download_to_path_with_progress(&http, &url, &tmp, cancel.clone(), |p| {
                let _ = evt_tx.try_send(BackendEvent::DownloadProgress {
                    op_id: op_id.clone(),
                    fraction: p.fraction,
                    downloaded: p.downloaded,
                    total: p.total,
                });
            })
            .await;
        match res {
            Ok(()) => {
                let finalized =
                    match finalize_transfer(&http, checksum_url.as_deref(), &tmp, &target).await {
                        Ok(finalized) => finalized,
                        Err(error) => {
                            let _ = evt_tx
                                .send(BackendEvent::Error {
                                    op_id: Some(op_id),
                                    app_id: Some(app_id),
                                    message: format!("{error:#}"),
                                })
                                .await;
                            return;
                        }
                    };
                let checksum = finalized.checksum;
                let downloaded_fingerprint = finalized.fingerprint;
                // Re-read the window class from the new build so a future
                // release that changes its WM_CLASS keeps grouping correctly.
                let refreshed_wm_class = crate::appimage::extract_metadata_with_policy(
                    &target,
                    None,
                    crate::appimage::ExtractionPolicy::NeverExecute,
                )
                .ok()
                .and_then(|meta| meta.startup_wm_class);
                let mut updated_entry: Option<AppEntry> = None;
                if let Ok(mut cfg) = config.lock() {
                    if let Some(entry) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
                        entry.current_version = tag.clone();
                        entry.commit_hash = Some(commit_hash.clone());
                        entry.file_fingerprint = downloaded_fingerprint;
                        if entry.startup_wm_class_auto != Some(false) {
                            if let Some(wm_class) = &refreshed_wm_class {
                                entry.startup_wm_class = Some(wm_class.clone());
                                entry.startup_wm_class_auto = Some(true);
                            }
                        }
                        if entry
                            .icon_path
                            .as_ref()
                            .map(|p| !p.exists())
                            .unwrap_or(true)
                        {
                            if let Some(folder) = target.parent() {
                                if let Ok(extracted) = crate::appimage::extract_icon_with_policy(
                                    &target,
                                    folder,
                                    crate::appimage::ExtractionPolicy::NeverExecute,
                                ) {
                                    entry.icon_path = Some(extracted);
                                }
                            }
                        }
                        updated_entry = Some(entry.clone());
                    }
                    let _ = cfg.save();
                }
                if let Some(entry) = updated_entry {
                    // Updates only refresh shortcuts that
                    // already exist; they never create new ones.
                    refresh_existing_shortcuts(&entry);
                    let _ = appmeta::write_for_entry(&entry);
                }
                let _ = evt_tx
                    .send(BackendEvent::DownloadComplete {
                        op_id,
                        app_id,
                        tag,
                        checksum,
                    })
                    .await;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                if cancel.load(Ordering::Relaxed) {
                    let _ = evt_tx.send(BackendEvent::DownloadCancelled { op_id }).await;
                } else {
                    let _ = evt_tx
                        .send(BackendEvent::Error {
                            op_id: Some(op_id),
                            app_id: Some(app_id),
                            message: format!("{e:#}"),
                        })
                        .await;
                }
            }
        }
    } else {
        let _ = evt_tx
            .send(BackendEvent::Error {
                op_id: Some(op_id),
                app_id: Some(app_id),
                message: "target app not found".into(),
            })
            .await;
    }
}

pub(super) async fn verify_download_checksum(
    http: &reqwest::Client,
    checksum_url: Option<&str>,
    downloaded: &Path,
) -> anyhow::Result<ChecksumStatus> {
    let Some(url) = checksum_url else {
        return Ok(ChecksumStatus::Unavailable);
    };
    let text = downloader::fetch_checksum(http, url).await?;
    let expected = downloader::parse_sha256(&text).ok_or_else(|| {
        anyhow::anyhow!("the checksum file at {url} does not contain a SHA-256 digest")
    })?;

    let path = downloaded.to_path_buf();
    let verification =
        tokio::task::spawn_blocking(move || downloader::verify_sha256(&path, &expected))
            .await
            .map_err(|error| anyhow::anyhow!("checksum task failed: {error}"))?;
    verification?;
    Ok(ChecksumStatus::Verified)
}

/// Shared download finalization: checksum, chmod, atomic replace, fingerprint.
/// The temp file is removed on any failure, so both update and install paths
/// share exactly one implementation.
pub(super) struct FinalizedTransfer {
    pub checksum: ChecksumStatus,
    pub fingerprint: Option<crate::config::FileFingerprint>,
}

pub(super) async fn finalize_transfer(
    http: &reqwest::Client,
    checksum_url: Option<&str>,
    temp: &Path,
    target: &Path,
) -> anyhow::Result<FinalizedTransfer> {
    let result = async {
        let checksum = verify_download_checksum(http, checksum_url, temp).await?;
        permissions::set_executable(temp)?;
        downloader::atomic_replace(temp, target)?;
        let hash_target = target.to_path_buf();
        let fingerprint = tokio::task::spawn_blocking(move || {
            crate::config::file_fingerprint_with_hash(&hash_target)
        })
        .await
        .ok()
        .flatten()
        .or_else(|| crate::config::file_fingerprint(target));
        Ok(FinalizedTransfer {
            checksum,
            fingerprint,
        })
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
