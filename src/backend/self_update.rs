//! AppImage Hub self-update transfer.
use super::*;

pub(super) async fn run_self_update(
    http: reqwest::Client,
    url: String,
    target: PathBuf,
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
