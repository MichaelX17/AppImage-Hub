//! Backend event handling and state transitions.
use super::*;
impl ManagerApp {
    pub(crate) fn apply_event(&mut self, evt: BackendEvent) {
        match evt {
            BackendEvent::RepoAssetsFetched { repo, assets } => {
                let current_url = self.side_panel_form.github_url.trim();
                if current_url.contains(&repo) || repo.contains(current_url) {
                    self.side_panel_form.is_fetching_assets = false;
                    self.side_panel_form.fetch_assets_error = None;
                    if self.side_panel_form.manual_asset.is_none() {
                        if let Some(first) = assets.first() {
                            self.side_panel_form.manual_asset = Some(first.name.clone());
                        }
                    }
                    self.side_panel_form.available_assets = assets;
                }
            }
            BackendEvent::RepoAssetsFailed { error } => {
                self.side_panel_form.is_fetching_assets = false;
                self.side_panel_form.fetch_assets_error = Some(error);
            }
            BackendEvent::UpdateAvailable {
                app_id,
                tag,
                url,
                commit_hash,
                checksum_url,
            } => {
                self.finish_update_check_item(&app_id);
                // The app may have been deleted while the check was in flight.
                if !self.app_exists(&app_id) {
                    return;
                }
                self.last_checked = Some(Instant::now());
                self.latest_versions.insert(app_id.clone(), tag.clone());
                self.update_available.insert(
                    app_id,
                    AvailableUpdate {
                        tag,
                        url,
                        commit_hash,
                        checksum_url,
                    },
                );
            }
            BackendEvent::UpToDate { app_id, tag } => {
                self.finish_update_check_item(&app_id);
                if !self.app_exists(&app_id) {
                    return;
                }
                self.last_checked = Some(Instant::now());
                self.latest_versions.insert(app_id.clone(), tag);
                self.update_available.remove(&app_id);
            }
            BackendEvent::HubUpdateAvailable { url } => {
                self.hub_update_url = Some(url);
            }
            BackendEvent::HubUpToDate => {
                self.hub_update_url = None;
            }
            BackendEvent::HubUpdateProgress { fraction } => {
                self.hub_update_progress = Some(fraction.clamp(0.0, 1.0));
            }
            BackendEvent::HubUpdateReady => {
                self.hub_update_progress = None;
                if let Some(target) = current_appimage_path() {
                    let _ = std::process::Command::new(target).spawn();
                }
                std::process::exit(0);
            }
            BackendEvent::HubUpdateFailed { message } => {
                self.hub_update_progress = None;
                self.status = Some(format!("AppImage Hub update failed: {message}"));
            }
            BackendEvent::DownloadStarted {
                op_id,
                app_id,
                label,
                kind,
            } => {
                // The UI inserts its operation before sending the command, with
                // a human-readable label, retry metadata and the same cancel
                // flag the backend received. Do not overwrite it: doing so
                // would replace the cancel flag (breaking Cancel) and show the
                // raw app id in the progress view.
                if self.ops.contains_key(&op_id) {
                    return;
                }
                let now = Instant::now();
                self.ops.insert(
                    op_id.clone(),
                    ActiveOp {
                        op_id,
                        app_id,
                        label,
                        kind,
                        fraction: 0.0,
                        downloaded: 0,
                        total: None,
                        started_at: now,
                        last_update: now,
                        cancel: Arc::new(AtomicBool::new(false)),
                        retry: None,
                    },
                );
            }
            BackendEvent::DownloadProgress {
                op_id,
                fraction,
                downloaded,
                total,
                ..
            } => {
                if let Some(op) = self.ops.get_mut(&op_id) {
                    op.fraction = fraction;
                    op.downloaded = downloaded;
                    op.total = total;
                    op.last_update = Instant::now();
                }
            }
            BackendEvent::DownloadComplete {
                op_id,
                app_id,
                tag,
                checksum,
            } => {
                self.ops.remove(&op_id);
                self.update_available.remove(&app_id);
                self.latest_versions.insert(app_id.clone(), tag.clone());
                if checksum == ChecksumStatus::Unavailable {
                    self.status = Some(format!(
                        "Updated {} to {tag}, but the release publishes no SHA-256 checksum, \
                         so integrity could not be verified.",
                        self.app_display_name(&app_id)
                    ));
                }
                self.notify_completion(&app_id, &tag);
            }
            BackendEvent::AppInstalled {
                op_id,
                app_id,
                checksum,
            } => {
                self.ops.remove(&op_id);
                let installed_tag = self
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| {
                        c.applications
                            .iter()
                            .find(|a| a.id == app_id)
                            .map(|a| a.current_version.clone())
                    })
                    .filter(|t| !t.trim().is_empty());
                if let Some(tag) = installed_tag {
                    self.latest_versions.insert(app_id.clone(), tag);
                }
                if checksum == ChecksumStatus::Unavailable {
                    self.status = Some(format!(
                        "Installed {}, but the release publishes no SHA-256 checksum, \
                         so integrity could not be verified.",
                        self.app_display_name(&app_id)
                    ));
                }
                self.notify_completion(&app_id, "");
            }
            BackendEvent::DownloadCancelled { op_id } => {
                if let Some(op) = self.ops.remove(&op_id) {
                    self.status = Some(format!("Cancelled: {}", op.label));
                }
            }
            BackendEvent::Error {
                op_id,
                app_id,
                message,
            } => {
                // Only a check-update failure (no operation id) completes a
                // pending update check; a failed download of the same app must
                // not be mistaken for its check result.
                if op_id.is_none() {
                    if let Some(app_id) = app_id.as_deref() {
                        self.finish_update_check_item(app_id);
                        if !self.app_exists(app_id) {
                            return;
                        }
                    }
                }
                self.status = Some(format!("Error: {message}"));
                let (label, retry) = if let Some(op_id) = op_id {
                    if let Some(op) = self.ops.remove(&op_id) {
                        (op.label.clone(), op.retry)
                    } else {
                        ("Operation".to_string(), None)
                    }
                } else {
                    ("Operation".to_string(), None)
                };
                self.failed_ops.push(FailedOp {
                    app_id,
                    label,
                    message,
                    retry,
                });
                if self.failed_ops.len() > 20 {
                    let drain = self.failed_ops.len() - 20;
                    self.failed_ops.drain(0..drain);
                }
            }
            BackendEvent::FileAccepted { app_id, error } => {
                if let Some(message) = error {
                    self.status = Some(format!("Could not update file metadata: {message}"));
                } else {
                    self.update_available.remove(&app_id);
                    self.status = Some(format!(
                        "Marked {} as up to date.",
                        self.app_display_name(&app_id)
                    ));
                }
                self.refresh_file_issues();
            }
        }
    }
}
