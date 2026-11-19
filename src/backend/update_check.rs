//! Update checks, release cache, and commit comparison.
use super::*;

/// Outcome of one successful update check; the caller turns it into exactly one
/// terminal event.
enum CheckUpdateOutcome {
    Available {
        tag: String,
        url: String,
        commit_hash: String,
        checksum_url: Option<String>,
    },
    UpToDate {
        tag: String,
    },
}

pub(super) async fn run_check_update(
    config: Arc<Mutex<Config>>,
    octo: octocrab::Octocrab,
    release_cache: Arc<Mutex<HashMap<String, (Instant, github::ReleaseInfo)>>>,
    release_cache_ttl: Duration,
    evt_tx: mpsc::Sender<BackendEvent>,
    app_id: String,
) {
    let outcome = tokio::time::timeout(
        UPDATE_CHECK_TASK_TIMEOUT,
        perform_check_update(&config, &octo, &release_cache, release_cache_ttl, &app_id),
    )
    .await;

    let event = match outcome {
        Ok(Ok(CheckUpdateOutcome::Available {
            tag,
            url,
            commit_hash,
            checksum_url,
        })) => BackendEvent::UpdateAvailable {
            app_id: app_id.clone(),
            tag,
            url,
            commit_hash,
            checksum_url,
        },
        Ok(Ok(CheckUpdateOutcome::UpToDate { tag })) => BackendEvent::UpToDate {
            app_id: app_id.clone(),
            tag,
        },
        Ok(Err(error)) => BackendEvent::Error {
            op_id: None,
            app_id: Some(app_id.clone()),
            message: format!("{error:#}"),
        },
        Err(_) => BackendEvent::Error {
            op_id: None,
            app_id: Some(app_id.clone()),
            message: "update check timed out".into(),
        },
    };
    let _ = evt_tx.send(event).await;
}

async fn perform_check_update(
    config: &Arc<Mutex<Config>>,
    octo: &octocrab::Octocrab,
    release_cache: &Arc<Mutex<HashMap<String, (Instant, github::ReleaseInfo)>>>,
    release_cache_ttl: Duration,
    app_id: &str,
) -> anyhow::Result<CheckUpdateOutcome> {
    let entry = config
        .lock()
        .ok()
        .and_then(|cfg| {
            cfg.applications
                .iter()
                .find(|app| app.id == app_id)
                .cloned()
        })
        .ok_or_else(|| anyhow::anyhow!("app not found"))?;

    let repo = match &entry.source {
        AppSource::LocalWithRepo { github_repo } | AppSource::GithubOnly { github_repo } => {
            github_repo.clone()
        }
        AppSource::LocalOnly => {
            anyhow::bail!("app is not tracked by a GitHub repository")
        }
    };

    let cache_key = repo.trim().to_lowercase();
    let now = Instant::now();
    let cached = {
        let cache = release_cache.lock().ok();
        cache.and_then(|cache| {
            cache.get(&cache_key).and_then(|(fetched_at, info)| {
                (now.duration_since(*fetched_at) < release_cache_ttl).then(|| info.clone())
            })
        })
    };
    let info = match cached {
        Some(info) => info,
        None => {
            let info = github::fetch_latest_release_with_selector(
                octo,
                &repo,
                entry.asset_preference.as_deref(),
            )
            .await?;
            if let Ok(mut cache) = release_cache.lock() {
                cache.insert(cache_key, (now, info.clone()));
            }
            info
        }
    };

    // Confirm the file state off the async runtime: this may hash a large
    // AppImage when the cheap size/mtime metadata changed.
    let status_entry = entry.clone();
    let local_issue = tokio::task::spawn_blocking(move || verified_local_file_issue(&status_entry))
        .await
        .unwrap_or(None);

    if local_issue.is_none() {
        let baseline_entry = entry.clone();
        let baseline = tokio::task::spawn_blocking(move || baseline_fingerprint(&baseline_entry))
            .await
            .ok()
            .flatten();
        if let Some(fingerprint) = baseline {
            persist_baseline_fingerprint(config, &entry.id, fingerprint, false);
        }
    }

    let commit_changed = is_update_available(
        Some(entry.commit_hash.as_deref().unwrap_or_default()),
        &info.commit_hash,
    );

    if local_issue.is_none() && !commit_changed {
        Ok(CheckUpdateOutcome::UpToDate { tag: info.tag })
    } else {
        Ok(CheckUpdateOutcome::Available {
            tag: info.tag,
            url: info.download_url,
            commit_hash: info.commit_hash,
            checksum_url: info.checksum_url,
        })
    }
}

/// Commit-hash-only update comparison. A missing local hash means the release
/// must be installed once to establish a baseline.
pub(crate) fn is_update_available(installed_commit: Option<&str>, remote_commit: &str) -> bool {
    let Some(installed) = installed_commit.map(str::trim).filter(|c| !c.is_empty()) else {
        return true;
    };
    !installed.eq_ignore_ascii_case(remote_commit.trim())
}
