//! GitHub release install pipeline.
use super::download::finalize_transfer;
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_install_from_repo(
    config: Arc<Mutex<Config>>,
    http: reqwest::Client,
    evt_tx: mpsc::Sender<BackendEvent>,
    octo: octocrab::Octocrab,
    op_id: String,
    github_repo: String,
    display_name: String,
    asset_name: Option<String>,
    categories: Option<String>,
    desktop_shortcut: bool,
    menu_entry: bool,
    mime_types: Option<String>,
    exec_arg: Option<String>,
    cancel: Arc<AtomicBool>,
) {
    let label = if display_name.trim().is_empty() {
        format!("Install {github_repo}")
    } else {
        format!("Install {display_name}")
    };
    let _ = evt_tx
        .send(BackendEvent::DownloadStarted {
            op_id: op_id.clone(),
            app_id: None,
            label: label.clone(),
            kind: OpKind::Install,
        })
        .await;
    match github::fetch_latest_release_with_selector(&octo, &github_repo, asset_name.as_deref())
        .await
    {
        Ok(info) => {
            if info.asset_size > downloader::MAX_DOWNLOAD_BYTES {
                let _ = evt_tx
                    .send(BackendEvent::Error {
                        op_id: Some(op_id),
                        app_id: None,
                        message: format!(
                            "release asset is too large ({} bytes; limit {} bytes)",
                            info.asset_size,
                            downloader::MAX_DOWNLOAD_BYTES
                        ),
                    })
                    .await;
                return;
            }
            let install_id = Uuid::new_v4().to_string();
            let hint = if display_name.trim().is_empty() {
                github_repo
                    .split('/')
                    .next_back()
                    .unwrap_or(&github_repo)
                    .to_string()
            } else {
                display_name.clone()
            };
            let custom_base = config
                .lock()
                .ok()
                .and_then(|c| c.custom_download_dir.clone());
            let folder_result = if let Some(base) = custom_base.as_ref() {
                xdg::managed_app_folder_in(base, &hint, &install_id)
            } else {
                xdg::managed_app_folder(&hint, &install_id)
            };
            match folder_result {
                Ok(folder) => {
                    let filename = xdg::managed_appimage_filename(&hint, "app");
                    let target = folder.join(filename);
                    let tmp = downloader::temp_path_for(&target);
                    let res = downloader::download_to_path_with_progress(
                        &http,
                        &info.download_url,
                        &tmp,
                        cancel.clone(),
                        |p| {
                            let _ = evt_tx.try_send(BackendEvent::DownloadProgress {
                                op_id: op_id.clone(),
                                fraction: p.fraction,
                                downloaded: p.downloaded,
                                total: p.total,
                            });
                        },
                    )
                    .await;
                    match res {
                        Ok(()) => {
                            let finalized = match finalize_transfer(
                                &http,
                                info.checksum_url.as_deref(),
                                &tmp,
                                &target,
                            )
                            .await
                            {
                                Ok(finalized) => finalized,
                                Err(error) => {
                                    let _ = evt_tx
                                        .send(BackendEvent::Error {
                                            op_id: Some(op_id),
                                            app_id: Some(install_id),
                                            message: format!("{error:#}"),
                                        })
                                        .await;
                                    return;
                                }
                            };
                            let checksum = finalized.checksum;
                            let (extracted_icon, app_meta) = {
                                let policy = crate::appimage::ExtractionPolicy::NeverExecute;
                                let icon = crate::appimage::extract_icon_with_policy(
                                    &target, &folder, policy,
                                )
                                .ok();
                                let meta = crate::appimage::extract_metadata_with_policy(
                                    &target, None, policy,
                                )
                                .ok();
                                (icon, meta)
                            };
                            let installed_fingerprint = finalized.fingerprint;
                            let entry = AppEntry {
                                id: install_id.clone(),
                                display_name: if display_name.trim().is_empty() {
                                    app_meta
                                        .as_ref()
                                        .and_then(|m| m.name.clone())
                                        .unwrap_or_else(|| github_repo.clone())
                                } else {
                                    display_name
                                },
                                generic_name: app_meta
                                    .as_ref()
                                    .and_then(|m| m.generic_name.clone()),
                                comment: app_meta.as_ref().and_then(|m| m.comment.clone()),
                                categories: resolve_categories(
                                    categories.clone(),
                                    app_meta.as_ref(),
                                ),
                                startup_notify: app_meta
                                    .as_ref()
                                    .and_then(|m| m.startup_notify)
                                    .unwrap_or(true),
                                source: AppSource::GithubOnly { github_repo },
                                local_path: target,
                                current_version: info.tag,
                                args: vec![],
                                icon_path: extracted_icon,
                                asset_preference: asset_name,
                                added_at: Some(now_unix()),
                                commit_hash: Some(info.commit_hash),
                                file_fingerprint: installed_fingerprint,
                                desktop_shortcut,
                                menu_entry,
                                mime_types: mime_types.or_else(|| {
                                    app_meta.as_ref().and_then(|m| m.mime_types.clone())
                                }),
                                exec_arg: exec_arg
                                    .or_else(|| app_meta.as_ref().and_then(|m| m.exec_arg.clone())),
                            };
                            let mut desktop_err: Option<String> = None;
                            if let Err(e) = apply_app_shortcuts(&entry) {
                                desktop_err = Some(format!("{e:#}"));
                            }
                            // Persist the metadata beside the AppImage.
                            let _ = appmeta::write_for_entry(&entry);
                            if let Ok(mut cfg) = config.lock() {
                                cfg.applications.push(entry.clone());
                                let _ = cfg.save();
                            }
                            if let Some(msg) = desktop_err {
                                let _ = evt_tx
                                    .send(BackendEvent::Error {
                                        op_id: Some(op_id.clone()),
                                        app_id: Some(install_id.clone()),
                                        message: format!("Installed, but menu entry failed: {msg}"),
                                    })
                                    .await;
                            }
                            let _ = evt_tx
                                .send(BackendEvent::AppInstalled {
                                    op_id,
                                    app_id: install_id,
                                    checksum,
                                })
                                .await;
                        }
                        Err(e) => {
                            let _ = std::fs::remove_file(&tmp);
                            let _ = std::fs::remove_dir_all(&folder);
                            if cancel.load(Ordering::Relaxed) {
                                let _ =
                                    evt_tx.send(BackendEvent::DownloadCancelled { op_id }).await;
                            } else {
                                let _ = evt_tx
                                    .send(BackendEvent::Error {
                                        op_id: Some(op_id),
                                        app_id: Some(install_id),
                                        message: format!("{e:#}"),
                                    })
                                    .await;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = evt_tx
                        .send(BackendEvent::Error {
                            op_id: Some(op_id),
                            app_id: None,
                            message: format!("{e:#}"),
                        })
                        .await;
                }
            }
        }
        Err(e) => {
            let _ = evt_tx
                .send(BackendEvent::Error {
                    op_id: Some(op_id),
                    app_id: None,
                    message: format!("{e:#}"),
                })
                .await;
        }
    }
}
