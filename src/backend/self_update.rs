//! AppImage Hub self-update transfer.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_self_update(
    http: reqwest::Client,
    config: Arc<Mutex<Config>>,
    url: String,
    target: PathBuf,
    expected_digest: Option<String>,
    asset_id: u64,
    asset_size: u64,
    updated_at: Option<String>,
    cancel: Arc<AtomicBool>,
    evt_tx: mpsc::Sender<BackendEvent>,
) {
    let temp = downloader::temp_path_for(&target);
    let progress_tx = evt_tx.clone();
    let result =
        downloader::download_to_path_with_progress(&http, &url, &temp, cancel, move |progress| {
            let _ = progress_tx.try_send(BackendEvent::HubUpdateProgress {
                fraction: progress.fraction,
            });
        })
        .await;

    match result {
        Ok(()) => {
            if let Some(expected) = &expected_digest {
                let temp_copy = temp.clone();
                let expected_copy = expected.clone();
                let verify_res = tokio::task::spawn_blocking(move || {
                    let actual = crate::config::sha256_file(&temp_copy)
                        .ok_or_else(|| anyhow::anyhow!("failed to hash downloaded temporary file"))?;
                    eprintln!(
                        "[SelfUpdate] Downloaded binary hash: {}, expected hash: {}",
                        actual.to_ascii_lowercase(),
                        expected_copy.to_ascii_lowercase()
                    );
                    if !actual.eq_ignore_ascii_case(&expected_copy) {
                        anyhow::bail!(
                            "SHA-256 mismatch for downloaded update: expected {expected_copy}, got {actual}"
                        );
                    }
                    Ok(())
                })
                .await;

                match verify_res {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => {
                        let _ = std::fs::remove_file(&temp);
                        let _ = evt_tx
                            .send(BackendEvent::HubUpdateFailed {
                                message: format!("verification failed: {e:#}"),
                            })
                            .await;
                        return;
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&temp);
                        let _ = evt_tx
                            .send(BackendEvent::HubUpdateFailed {
                                message: format!("verification task panicked: {e:#}"),
                            })
                            .await;
                        return;
                    }
                }
            }

            if let Err(e) = permissions::set_executable(&temp) {
                let _ = std::fs::remove_file(&temp);
                let _ = evt_tx
                    .send(BackendEvent::HubUpdateFailed {
                        message: format!("chmod failed: {e:#}"),
                    })
                    .await;
                return;
            }
            if let Err(e) = downloader::atomic_replace(&temp, &target) {
                let _ = std::fs::remove_file(&temp);
                let _ = evt_tx
                    .send(BackendEvent::HubUpdateFailed {
                        message: format!("replace failed: {e:#}"),
                    })
                    .await;
                return;
            }

            // Persist the fallback state upon successful update.
            if let Ok(mut cfg) = config.lock() {
                cfg.last_hub_update = Some(crate::config::HubUpdateState {
                    asset_id,
                    asset_size,
                    updated_at,
                });
                let _ = cfg.save();
            }

            let _ = evt_tx.send(BackendEvent::HubUpdateReady).await;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            let _ = evt_tx
                .send(BackendEvent::HubUpdateFailed {
                    message: format!("{e:#}"),
                })
                .await;
        }
    }
}
