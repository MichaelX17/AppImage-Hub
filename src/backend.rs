//! Background command loop, transfer pipeline, installs, update checks, and self-update.
use crate::app::{
    apply_app_shortcuts, hub_update_is_available, now_unix, refresh_existing_shortcuts,
    resolve_categories, token_is_present, update_check_cooldown_secs, BackendCommand, BackendEvent,
    ChecksumStatus, OpKind, API_CONNECT_TIMEOUT, API_REQUEST_TIMEOUT, DOWNLOAD_CONNECT_TIMEOUT,
    DOWNLOAD_IDLE_TIMEOUT, MAX_CONCURRENT_TRANSFERS, UPDATE_CHECK_TASK_TIMEOUT,
};
use crate::appmeta;
use crate::config::{AppEntry, AppSource, Config};
use crate::scan::{baseline_fingerprint, persist_baseline_fingerprint, verified_local_file_issue};
use crate::{downloader, github, permissions, xdg};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

fn build_github_client(token: Option<&str>) -> octocrab::Result<octocrab::Octocrab> {
    let builder = octocrab::Octocrab::builder()
        .set_connect_timeout(Some(API_CONNECT_TIMEOUT))
        .set_read_timeout(Some(API_REQUEST_TIMEOUT))
        .set_write_timeout(Some(API_REQUEST_TIMEOUT));
    match token.map(str::trim).filter(|token| !token.is_empty()) {
        Some(token) => builder.personal_token(token.to_string()).build(),
        None => builder.build(),
    }
}

/// A claimed job key. Dropping it releases the key for a future job.
struct JobClaim {
    key: String,
    active: Arc<Mutex<HashSet<String>>>,
}

impl Drop for JobClaim {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.key);
        }
    }
}

/// Claims `key` so two jobs with the same operation id or app id cannot run at
/// the same time. Returns `None` when the key is already held.
fn claim_job(active: &Arc<Mutex<HashSet<String>>>, key: String) -> Option<JobClaim> {
    let mut active_guard = active.lock().ok()?;
    if !active_guard.insert(key.clone()) {
        return None;
    }
    drop(active_guard);
    Some(JobClaim {
        key,
        active: active.clone(),
    })
}

pub(crate) fn spawn_backend(
    config: Arc<Mutex<Config>>,
    mut cmd_rx: mpsc::Receiver<BackendCommand>,
    evt_tx: mpsc::Sender<BackendEvent>,
    stored_github_token: Option<String>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build();
        let Ok(rt) = rt else {
            return;
        };
        rt.block_on(async move {
            let initial_token = stored_github_token.or_else(|| {
                std::env::var("GITHUB_TOKEN")
                    .ok()
                    .or_else(|| std::env::var("GH_TOKEN").ok())
                    .filter(|token| !token.trim().is_empty())
            });
            let mut octo = match build_github_client(initial_token.as_deref()) {
                Ok(c) => c,
                Err(e) => {
                    let _ = evt_tx
                        .send(BackendEvent::Error {
                            op_id: None,
                            app_id: None,
                            message: format!("failed to init GitHub client: {e}"),
                        })
                        .await;
                    return;
                }
            };
            // Downloads get their own client: no total timeout (large files are
            // legitimate) but bounded connect and idle times.
            let http = match reqwest::Client::builder()
                .user_agent("appimage-manager")
                .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
                .read_timeout(DOWNLOAD_IDLE_TIMEOUT)
                .redirect(reqwest::redirect::Policy::limited(5))
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = evt_tx
                        .send(BackendEvent::Error {
                            op_id: None,
                            app_id: None,
                            message: format!("failed to init HTTP client: {e}"),
                        })
                        .await;
                    return;
                }
            };

            // Bounds how many transfers run at once so "Update All" does not
            // saturate the link, while still overlapping independent apps.
            let transfers = Arc::new(Semaphore::new(MAX_CONCURRENT_TRANSFERS));
            // Defense in depth against duplicate operation/app ids.
            let active_jobs: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
            // Multiple apps can share one repository. Cache release lookups for
            // the same rate-limit window (2 min with a token, 5 min without) so
            // a batch check costs one request per unique repo instead of one per
            // app.
            let release_cache: Arc<Mutex<HashMap<String, (Instant, github::ReleaseInfo)>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let mut handles: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
            let mut release_cache_ttl = Duration::from_secs(update_check_cooldown_secs(
                token_is_present(initial_token.as_deref()),
            ));

            while let Some(cmd) = cmd_rx.recv().await {
                // Reap finished tasks so the handle map does not grow forever.
                handles.retain(|_, handle| !handle.is_finished());
                match cmd {
                    BackendCommand::SetGithubToken(token) => {
                        release_cache_ttl = Duration::from_secs(update_check_cooldown_secs(
                            token_is_present(token.as_deref()),
                        ));
                        match build_github_client(token.as_deref()) {
                            Ok(client) => octo = client,
                            Err(e) => {
                                let _ = evt_tx
                                    .send(BackendEvent::Error {
                                        op_id: None,
                                        app_id: None,
                                        message: format!("failed to update GitHub client: {e}"),
                                    })
                                    .await;
                            }
                        }
                    }
                    BackendCommand::CheckHubUpdate => {
                        let result = tokio::time::timeout(
                            API_REQUEST_TIMEOUT,
                            github::fetch_latest_release_with_selector(
                                &octo,
                                "MichaelX17/AppImage-Hub",
                                None,
                            ),
                        )
                        .await;
                        match result {
                            Ok(Ok(info)) => {
                                if hub_update_is_available(&info.commit_hash, &info.tag) {
                                    let _ = evt_tx
                                        .send(BackendEvent::HubUpdateAvailable {
                                            url: info.download_url,
                                        })
                                        .await;
                                } else {
                                    let _ = evt_tx.send(BackendEvent::HubUpToDate).await;
                                }
                            }
                            Ok(Err(e)) => {
                                let _ = evt_tx
                                    .send(BackendEvent::HubUpdateFailed {
                                        message: format!("{e:#}"),
                                    })
                                    .await;
                            }
                            Err(_) => {
                                let _ = evt_tx
                                    .send(BackendEvent::HubUpdateFailed {
                                        message: "GitHub API request timed out".into(),
                                    })
                                    .await;
                            }
                        }
                    }
                    BackendCommand::SelfUpdate {
                        url,
                        target,
                        cancel,
                    } => {
                        if handles.contains_key("self-update") {
                            let _ = evt_tx
                                .send(BackendEvent::HubUpdateFailed {
                                    message: "An AppImage Hub update is already running.".into(),
                                })
                                .await;
                            continue;
                        }
                        let evt_tx = evt_tx.clone();
                        let http = http.clone();
                        let transfers = transfers.clone();
                        handles.insert(
                            "self-update".to_string(),
                            tokio::spawn(async move {
                                let _permit = match transfers.acquire().await {
                                    Ok(permit) => permit,
                                    Err(_) => return,
                                };
                                run_self_update(http, url, target, cancel, evt_tx).await;
                            }),
                        );
                    }
                    BackendCommand::CheckUpdate { app_id } => {
                        let config = config.clone();
                        let octo = octo.clone();
                        let evt_tx = evt_tx.clone();
                        let release_cache = release_cache.clone();
                        let release_cache_ttl = release_cache_ttl;
                        tokio::spawn(async move {
                            run_check_update(
                                config,
                                octo,
                                release_cache,
                                release_cache_ttl,
                                evt_tx,
                                app_id,
                            )
                            .await;
                        });
                    }
                    BackendCommand::DownloadAndReplace {
                        op_id,
                        app_id,
                        url,
                        tag,
                        commit_hash,
                        checksum_url,
                        cancel,
                    } => {
                        let config = config.clone();
                        let http = http.clone();
                        let evt_tx = evt_tx.clone();
                        let transfers = transfers.clone();
                        let active_jobs = active_jobs.clone();
                        let key = op_id.clone();
                        handles.insert(
                            key,
                            tokio::spawn(async move {
                                let Some(_op_claim) =
                                    claim_job(&active_jobs, format!("op:{op_id}"))
                                else {
                                    let _ = evt_tx
                                        .send(BackendEvent::Error {
                                            op_id: Some(op_id),
                                            app_id: Some(app_id),
                                            message: "duplicate operation id; ignoring".into(),
                                        })
                                        .await;
                                    return;
                                };
                                let Some(_app_claim) =
                                    claim_job(&active_jobs, format!("app:{app_id}"))
                                else {
                                    let _ = evt_tx
                                        .send(BackendEvent::Error {
                                            op_id: Some(op_id),
                                            app_id: Some(app_id),
                                            message: "this app already has an update in progress"
                                                .into(),
                                        })
                                        .await;
                                    return;
                                };
                                let _permit = match transfers.acquire().await {
                                    Ok(permit) => permit,
                                    Err(_) => return,
                                };
                                run_download_and_replace(
                                    config,
                                    http,
                                    evt_tx,
                                    cancel,
                                    op_id,
                                    app_id,
                                    url,
                                    tag,
                                    commit_hash,
                                    checksum_url,
                                )
                                .await;
                            }),
                        );
                    }
                    BackendCommand::InstallFromRepo {
                        op_id,
                        github_repo,
                        display_name,
                        asset_name,
                        categories,
                        desktop_shortcut,
                        menu_entry,
                        mime_types,
                        exec_arg,
                        cancel,
                    } => {
                        let config = config.clone();
                        let http = http.clone();
                        let evt_tx = evt_tx.clone();
                        let octo = octo.clone();
                        let transfers = transfers.clone();
                        let active_jobs = active_jobs.clone();
                        let key = op_id.clone();
                        handles.insert(
                            key,
                            tokio::spawn(async move {
                                let Some(_op_claim) =
                                    claim_job(&active_jobs, format!("op:{op_id}"))
                                else {
                                    let _ = evt_tx
                                        .send(BackendEvent::Error {
                                            op_id: Some(op_id),
                                            app_id: None,
                                            message: "duplicate operation id; ignoring".into(),
                                        })
                                        .await;
                                    return;
                                };
                                let _permit = match transfers.acquire().await {
                                    Ok(permit) => permit,
                                    Err(_) => return,
                                };
                                run_install_from_repo(
                                    config,
                                    http,
                                    evt_tx,
                                    octo,
                                    op_id,
                                    github_repo,
                                    display_name,
                                    asset_name,
                                    categories,
                                    desktop_shortcut,
                                    menu_entry,
                                    mime_types,
                                    exec_arg,
                                    cancel,
                                )
                                .await;
                            }),
                        );
                    }
                    BackendCommand::FetchRepoAssets { repo } => {
                        let octo = octo.clone();
                        let evt_tx = evt_tx.clone();
                        tokio::spawn(async move {
                            let result = tokio::time::timeout(
                                API_REQUEST_TIMEOUT,
                                github::fetch_available_appimages(&octo, &repo),
                            )
                            .await;
                            match result {
                                Ok(Ok(assets)) => {
                                    let _ = evt_tx
                                        .send(BackendEvent::RepoAssetsFetched { repo, assets })
                                        .await;
                                }
                                Ok(Err(e)) => {
                                    let _ = evt_tx
                                        .send(BackendEvent::RepoAssetsFailed {
                                            error: format!("{e:#}"),
                                        })
                                        .await;
                                }
                                Err(_) => {
                                    let _ = evt_tx
                                        .send(BackendEvent::RepoAssetsFailed {
                                            error: "GitHub API request timed out".into(),
                                        })
                                        .await;
                                }
                            }
                        });
                    }
                    BackendCommand::AcceptCurrentFile { app_id } => {
                        let entry = config
                            .lock()
                            .ok()
                            .and_then(|c| c.applications.iter().find(|a| a.id == app_id).cloned());
                        let Some(entry) = entry else {
                            let _ = evt_tx
                                .send(BackendEvent::FileAccepted {
                                    app_id,
                                    error: Some("app not found".into()),
                                })
                                .await;
                            continue;
                        };
                        if !entry.local_path.exists() {
                            let _ = evt_tx
                                .send(BackendEvent::FileAccepted {
                                    app_id,
                                    error: Some("AppImage file is missing".into()),
                                })
                                .await;
                            continue;
                        }
                        let path = entry.local_path.clone();
                        let fingerprint = tokio::task::spawn_blocking(move || {
                            crate::config::file_fingerprint_with_hash(&path)
                        })
                        .await
                        .ok()
                        .flatten();
                        match fingerprint {
                            Some(fingerprint) => {
                                let stored = persist_baseline_fingerprint(
                                    &config,
                                    &app_id,
                                    fingerprint,
                                    true,
                                );
                                let _ = evt_tx
                                    .send(BackendEvent::FileAccepted {
                                        app_id,
                                        error: (!stored).then(|| "app not found".to_string()),
                                    })
                                    .await;
                            }
                            None => {
                                let _ = evt_tx
                                    .send(BackendEvent::FileAccepted {
                                        app_id,
                                        error: Some("failed to read the AppImage file".into()),
                                    })
                                    .await;
                            }
                        }
                    }
                }
            }
        });
    });
}

mod download;
mod install;
mod self_update;
mod update_check;

use download::run_download_and_replace;
use install::run_install_from_repo;
use self_update::run_self_update;
#[allow(unused_imports)]
pub(crate) use update_check::is_update_available;
use update_check::run_check_update;
