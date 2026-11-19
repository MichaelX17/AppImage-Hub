//! User-intent handlers: add/edit/delete, settings, shortcuts, update actions.
use super::*;
/// One local import; `repo` distinguishes Local Only from Local + Repo.
struct ImportRequest {
    path: PathBuf,
    display_name: String,
    generic_name: Option<String>,
    comment: Option<String>,
    categories: Option<String>,
    startup_notify: bool,
    desktop_shortcut: bool,
    menu_entry: bool,
    mime_types: Option<String>,
    exec_arg: Option<String>,
    args: Vec<String>,
    icon_path: Option<PathBuf>,
    asset_preference: Option<String>,
    repo: Option<String>,
}

impl ManagerApp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_install_from_repo(
        &mut self,
        github_repo: String,
        display_name: String,
        asset_name: Option<String>,
        categories: Option<String>,
        desktop_shortcut: bool,
        menu_entry: bool,
        mime_types: Option<String>,
        exec_arg: Option<String>,
    ) {
        let op_id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let label = if display_name.trim().is_empty() {
            format!("Install {github_repo}")
        } else {
            format!("Install {display_name}")
        };
        let now = Instant::now();
        self.ops.insert(
            op_id.clone(),
            ActiveOp {
                op_id: op_id.clone(),
                app_id: None,
                label,
                kind: OpKind::Install,
                fraction: 0.0,
                downloaded: 0,
                total: None,
                started_at: now,
                last_update: now,
                cancel: cancel.clone(),
                retry: Some(RetryAction::InstallFromRepo {
                    github_repo: github_repo.clone(),
                    display_name: display_name.clone(),
                    asset_name: asset_name.clone(),
                    categories: categories.clone(),
                    desktop_shortcut,
                    menu_entry,
                    mime_types: mime_types.clone(),
                    exec_arg: exec_arg.clone(),
                }),
            },
        );
        let op_key = op_id.clone();
        if !self.send_cmd(BackendCommand::InstallFromRepo {
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
        }) {
            // Never leave a progress row for a command that was not queued.
            self.ops.remove(&op_key);
        }
    }

    pub(crate) fn start_update_download(
        &mut self,
        app_id: String,
        url: String,
        tag: String,
        commit_hash: String,
        checksum_url: Option<String>,
    ) {
        // One operation per app at a time; the bottom progress view aggregates
        // every active update, so duplicate rows must never appear.
        if self
            .ops
            .values()
            .any(|op| op.app_id.as_deref() == Some(app_id.as_str()))
        {
            return;
        }
        let op_id = Uuid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        let label = format!("Update {}", self.app_display_name(&app_id));
        let now = Instant::now();
        self.ops.insert(
            op_id.clone(),
            ActiveOp {
                op_id: op_id.clone(),
                app_id: Some(app_id.clone()),
                label,
                kind: OpKind::Update,
                fraction: 0.0,
                downloaded: 0,
                total: None,
                started_at: now,
                last_update: now,
                cancel: cancel.clone(),
                retry: Some(RetryAction::DownloadAndReplace {
                    app_id: app_id.clone(),
                    url: url.clone(),
                    tag: tag.clone(),
                    commit_hash: commit_hash.clone(),
                    checksum_url: checksum_url.clone(),
                }),
            },
        );
        let op_key = op_id.clone();
        if !self.send_cmd(BackendCommand::DownloadAndReplace {
            op_id,
            app_id,
            url,
            tag,
            commit_hash,
            checksum_url,
            cancel,
        }) {
            self.ops.remove(&op_key);
        }
    }

    pub(crate) fn confirm_delete(&mut self, app_id: &str) {
        let mut removed: Option<(usize, AppEntry)> = None;
        let saved = if let Ok(mut cfg) = self.config.lock() {
            let position = cfg.applications.iter().position(|a| a.id == app_id);
            if let Some(position) = position {
                removed = Some((position, cfg.applications.remove(position)));
            }
            cfg.save()
        } else {
            Err(anyhow::anyhow!("configuration lock is poisoned"))
        };
        if let Err(error) = saved {
            // Put the entry back so memory still matches what is on disk, and
            // delete nothing.
            if let Some((position, entry)) = removed {
                if let Ok(mut cfg) = self.config.lock() {
                    let position = position.min(cfg.applications.len());
                    cfg.applications.insert(position, entry);
                }
            }
            self.status = Some(format!(
                "Could not save the app list; nothing was deleted: {error:#}"
            ));
            return;
        }
        self.finish_update_check_item(app_id);
        let mut message: Option<String> = None;
        if let Some((_, ref entry)) = removed {
            // Use the same roots the app was stored under (default + custom).
            let mut roots = self
                .config
                .lock()
                .ok()
                .map(|config| crate::storage::roots(&config))
                .unwrap_or_default();
            // After a storage-folder change the old root is no longer
            // configured; recognize the managed folder name so deleting the
            // entry still removes its files.
            if let Some(previous_root) = crate::storage::legacy_root_for_entry(entry, &roots) {
                roots.push(previous_root);
            }
            let outcome = crate::storage::delete_managed_entry(entry, &roots);

            if let (Ok(d), Ok(a)) = (xdg::desktop_dir(), xdg::applications_dir()) {
                let _ = desktop_entry::remove_entry(entry, &d, &a);
            }

            if outcome.skipped_outside_root {
                message = Some(format!(
                    "Removed {} from the list and its shortcuts, but left its files \
                     in place because they are outside the managed storage folders.",
                    entry.display_name
                ));
            } else if !outcome.warnings.is_empty() {
                message = Some(format!(
                    "Deleted {} with warnings: {}",
                    entry.display_name,
                    outcome.warnings.join("; ")
                ));
            } else if let Some(folder) = outcome.kept_non_empty_folder {
                message = Some(format!(
                    "Deleted {}. Its folder {} still contained other files, so it was kept.",
                    entry.display_name,
                    folder.display()
                ));
            }
        }
        self.update_available.remove(app_id);
        self.latest_versions.remove(app_id);
        self.file_issues.remove(app_id);
        self.refresh_apps_snapshot();
        self.request_file_scan();
        if message.is_some() {
            self.status = message;
        } else if let Some((_, entry)) = removed {
            self.status = Some(format!(
                "Deleted {} and removed its entry from config.toml.",
                entry.display_name
            ));
        }
    }

    pub(crate) fn open_add_form(&mut self) {
        self.add_to = None;
        self.close_settings();
        self.side_panel_form.clear();
        self.status = None;
        self.is_form_open = true;
        self.show_side_panel = true;
    }

    /// Appends a new app entry and persists it. Returns `false` (with a status
    /// message) when the save fails, so callers can avoid creating shortcuts
    /// for an app the next launch would not know about.
    pub(crate) fn push_entry(&mut self, entry: AppEntry) -> bool {
        let mut cfg = match self.config.lock() {
            Ok(cfg) => cfg,
            Err(_) => {
                self.status =
                    Some("Could not save the app list: configuration lock is poisoned".into());
                return false;
            }
        };
        cfg.applications.push(entry);
        match cfg.save() {
            Ok(()) => true,
            Err(error) => {
                cfg.applications.pop();
                self.status = Some(format!("Could not save the app list: {error:#}"));
                false
            }
        }
    }

    pub(crate) fn open_edit(&mut self, entry: &AppEntry) {
        use crate::ui::side_panel::SidePanelMode;

        let (mode, github_url) = match &entry.source {
            AppSource::LocalOnly => (SidePanelMode::LocalOnly, String::new()),
            AppSource::LocalWithRepo { github_repo } => {
                (SidePanelMode::LocalWithRepo, github_repo.clone())
            }
            AppSource::GithubOnly { github_repo } => {
                (SidePanelMode::GithubOnly, github_repo.clone())
            }
        };

        let cat_str = entry
            .categories
            .as_deref()
            .map(|c| c.trim_end_matches(';').trim())
            .filter(|c| !c.is_empty())
            .unwrap_or("Utility");
        let is_standard_cat = crate::ui::side_panel::KDE_CATEGORIES
            .iter()
            .any(|(_, val)| *val == cat_str);
        let (category, custom_category) = if is_standard_cat {
            (cat_str.to_string(), String::new())
        } else {
            ("Custom".to_string(), cat_str.to_string())
        };

        let mut mime_types = entry
            .mime_types
            .as_deref()
            .and_then(crate::desktop_entry::normalize_mime_types);
        let mut exec_arg = entry
            .exec_arg
            .as_deref()
            .map(str::trim)
            .filter(|arg| matches!(*arg, "%u" | "%U" | "%f" | "%F"))
            .map(str::to_string);

        if (mime_types.is_none() || exec_arg.is_none()) && entry.local_path.exists() {
            if let Some(meta) = appmeta::read_for_appimage(&entry.local_path) {
                if mime_types.is_none() {
                    mime_types = meta
                        .mime_types
                        .as_deref()
                        .and_then(crate::desktop_entry::normalize_mime_types);
                }
                if exec_arg.is_none() {
                    exec_arg = meta.exec_arg.clone();
                }
            }
            if mime_types.is_none() || exec_arg.is_none() {
                if let Ok(meta) = crate::appimage::extract_metadata_with_policy(
                    &entry.local_path,
                    None,
                    crate::appimage::ExtractionPolicy::NeverExecute,
                ) {
                    if mime_types.is_none() {
                        mime_types = meta.mime_types;
                    }
                    if exec_arg.is_none() {
                        exec_arg = meta.exec_arg;
                    }
                }
            }
        }

        self.add_to = None;
        self.close_settings();
        let asset_selection_mode = if entry.asset_preference.is_some() {
            crate::ui::side_panel::AssetSelectionMode::Manual
        } else {
            crate::ui::side_panel::AssetSelectionMode::Smart
        };
        self.side_panel_form = crate::ui::side_panel::SidePanelFormState {
            editing_app_id: Some(entry.id.clone()),
            display_name: entry.display_name.clone(),
            generic_name: entry.generic_name.clone().unwrap_or_default(),
            comment: entry.comment.clone().unwrap_or_default(),
            category,
            custom_category,
            category_user_selected: true,
            detected_category: None,
            startup_notify: entry.startup_notify,
            desktop_shortcut: entry.desktop_shortcut,
            menu_entry: entry.menu_entry,
            mime_types: mime_types.unwrap_or_default(),
            exec_arg: exec_arg.unwrap_or_default(),
            mode,
            local_path: entry.local_path.display().to_string(),
            icon_path: entry
                .icon_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            github_url,
            launch_args: crate::args::join_args(&entry.args),
            asset_selection_mode,
            manual_asset: entry.asset_preference.clone(),
            available_assets: Vec::new(),
            is_fetching_assets: false,
            fetch_assets_error: None,
        };
        self.status = None;
        self.is_form_open = true;
        self.show_side_panel = true;
    }

    pub(crate) fn retry_failed(&mut self, idx: usize) {
        if idx >= self.failed_ops.len() {
            return;
        }
        let failed = self.failed_ops.remove(idx);
        if let Some(retry) = failed.retry {
            match retry {
                RetryAction::InstallFromRepo {
                    github_repo,
                    display_name,
                    asset_name,
                    categories,
                    desktop_shortcut,
                    menu_entry,
                    mime_types,
                    exec_arg,
                } => self.start_install_from_repo(
                    github_repo,
                    display_name,
                    asset_name,
                    categories,
                    desktop_shortcut,
                    menu_entry,
                    mime_types,
                    exec_arg,
                ),
                RetryAction::DownloadAndReplace {
                    app_id,
                    url,
                    tag,
                    commit_hash,
                    checksum_url,
                } => self.start_update_download(app_id, url, tag, commit_hash, checksum_url),
            }
        }
    }

    pub(crate) fn app_download_state(&self, app_id: &str) -> AppDownloadState<'_> {
        if self
            .ops
            .values()
            .any(|op| op.app_id.as_deref() == Some(app_id))
        {
            return AppDownloadState::Downloading;
        }
        if let Some(failed) = self
            .failed_ops
            .iter()
            .rev()
            .find(|f| f.app_id.as_deref() == Some(app_id))
        {
            return AppDownloadState::Failed(failed);
        }
        AppDownloadState::Idle
    }

    pub(crate) fn cancel_op(&self, op_id: &str) {
        if let Some(op) = self.ops.get(op_id) {
            op.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub(crate) fn retry_failed_for_app(&mut self, app_id: &str) {
        if let Some(idx) = self
            .failed_ops
            .iter()
            .rposition(|f| f.app_id.as_deref() == Some(app_id))
        {
            self.retry_failed(idx);
        }
    }

    pub(crate) fn open_add_to(&mut self, app: &AppEntry) {
        self.is_form_open = false;
        self.close_settings();
        self.side_panel_form.clear();
        self.add_to = Some(AddToState {
            app_id: app.id.clone(),
            app_name: app.display_name.clone(),
            desktop: app.desktop_shortcut,
            menu: app.menu_entry,
            desktop_path: xdg::desktop_dir()
                .ok()
                .map(|dir| desktop_entry::desktop_shortcut_path(app, &dir)),
            menu_path: xdg::applications_dir()
                .ok()
                .map(|dir| desktop_entry::menu_entry_path(app, &dir)),
        });
        self.status = None;
        self.show_side_panel = true;
    }

    pub(crate) fn close_add_to(&mut self) {
        self.add_to = None;
    }

    /// Closes the settings panel and wipes the plaintext token from the form
    /// state. The token may still live in the backend copy used for API calls,
    /// but no UI copy survives after the panel closes.
    pub(crate) fn close_settings(&mut self) {
        use zeroize::Zeroize as _;
        self.settings_form.github_token.zeroize();
        self.settings_form.show_token = false;
        self.show_settings = false;
    }

    pub(crate) fn open_settings(&mut self) {
        let (apps_dir, auto_check_updates, hub_start_menu, hub_category, hub_desktop) = self
            .config
            .lock()
            .map(|cfg| {
                (
                    cfg.custom_download_dir
                        .clone()
                        .unwrap_or_else(xdg::default_managed_apps_dir)
                        .display()
                        .to_string(),
                    cfg.auto_check_updates,
                    cfg.hub_start_menu_shortcut,
                    cfg.hub_start_menu_category.clone(),
                    cfg.hub_desktop_shortcut,
                )
            })
            .unwrap_or_else(|_| {
                (
                    xdg::default_managed_apps_dir().display().to_string(),
                    true,
                    true,
                    "Utility".to_string(),
                    false,
                )
            });
        let github_token = crate::secrets::load_github_token()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .or_else(|| std::env::var("GH_TOKEN").ok())
            .unwrap_or_default();
        self.settings_form = crate::ui::settings_panel::SettingsFormState {
            apps_dir,
            auto_check_updates,
            hub_start_menu_shortcut: hub_start_menu,
            hub_start_menu_category: hub_category,
            hub_desktop_shortcut: hub_desktop,
            github_token,
            show_token: false,
        };
        self.is_form_open = false;
        self.add_to = None;
        self.show_settings = true;
        self.show_side_panel = true;
    }

    pub(crate) fn save_settings(&mut self) {
        self.save_settings_inner(false);
    }

    /// Applies a storage-folder change the user confirmed in the modal.
    pub(crate) fn confirm_storage_change(&mut self) {
        if let Some(path) = self.pending_storage_change.take() {
            self.settings_form.apps_dir = path.display().to_string();
            self.save_settings_inner(true);
        }
    }

    pub(crate) fn save_settings_inner(&mut self, storage_change_confirmed: bool) {
        let path = PathBuf::from(self.settings_form.apps_dir.trim());
        if path.as_os_str().is_empty() {
            self.status = Some("Choose an AppImage storage folder.".into());
            return;
        }
        // D4: keep existing entries where they are and warn instead of moving
        // files. Ask before applying a different root when apps already exist.
        if !storage_change_confirmed {
            let new_dir = crate::storage::normalize_path(&path);
            let current_dir = crate::storage::normalize_path(&self.app_storage_dir());
            let has_apps = self
                .config
                .lock()
                .ok()
                .is_some_and(|config| !config.applications.is_empty());
            if has_apps && new_dir != current_dir {
                self.pending_storage_change = Some(path);
                return;
            }
        }
        if let Err(e) = std::fs::create_dir_all(&path) {
            self.status = Some(format!("failed to create storage folder: {e:#}"));
            return;
        }

        let auto_check = self.settings_form.auto_check_updates;
        let hub_menu = self.settings_form.hub_start_menu_shortcut;
        let hub_category = self.settings_form.hub_start_menu_category.clone();
        let hub_desktop = self.settings_form.hub_desktop_shortcut;
        let saved = if let Ok(mut cfg) = self.config.lock() {
            cfg.custom_download_dir = Some(path.clone());
            cfg.auto_check_updates = auto_check;
            cfg.hub_start_menu_shortcut = hub_menu;
            cfg.hub_start_menu_category = hub_category.clone();
            cfg.hub_desktop_shortcut = hub_desktop;
            cfg.save()
        } else {
            Err(anyhow::anyhow!("configuration lock is poisoned"))
        };
        if let Err(e) = saved {
            self.status = Some(format!("failed to save settings: {e:#}"));
            return;
        }
        desktop_entry::sync_hub_shortcuts(hub_menu, hub_desktop, &hub_category);

        let token = zeroize::Zeroizing::new(self.settings_form.github_token.trim().to_string());
        if let Err(e) = crate::secrets::save_github_token(&token) {
            self.status = Some(format!("failed to save GitHub token: {e:#}"));
            return;
        }
        self.has_github_token = token_is_present(Some(&token));
        self.send_cmd(BackendCommand::SetGithubToken(if token.is_empty() {
            None
        } else {
            Some(token.to_string())
        }));
        let report = self.sync_metadata_files();
        self.close_settings();
        let mut message = format!("Settings saved. AppImages folder: {}", path.display());
        if report.adopted > 0 {
            message.push_str(&format!(". Found {} existing app(s)", report.adopted));
        }
        if let Some(note) = report.note() {
            message.push_str(&format!(". {note}"));
        }
        self.status = Some(message);
    }

    pub(crate) fn apply_add_to(&mut self) {
        let Some(state) = self.add_to.clone() else {
            return;
        };
        let entry = self.config.lock().ok().and_then(|c| {
            c.applications
                .iter()
                .find(|a| a.id == state.app_id)
                .cloned()
        });
        let Some(mut entry) = entry else {
            self.status = Some("Application not found".into());
            self.add_to = None;
            return;
        };
        let previous = (entry.desktop_shortcut, entry.menu_entry);
        entry.desktop_shortcut = state.desktop;
        entry.menu_entry = state.menu;
        let saved = if let Ok(mut cfg) = self.config.lock() {
            if let Some(stored) = cfg.applications.iter_mut().find(|a| a.id == state.app_id) {
                stored.desktop_shortcut = state.desktop;
                stored.menu_entry = state.menu;
            }
            cfg.save()
        } else {
            Err(anyhow::anyhow!("configuration lock is poisoned"))
        };
        if let Err(error) = saved {
            // Restore the in-memory flags so they match the file on disk, and
            // do not create or remove shortcuts that would not be persisted.
            if let Ok(mut cfg) = self.config.lock() {
                if let Some(stored) = cfg.applications.iter_mut().find(|a| a.id == state.app_id) {
                    stored.desktop_shortcut = previous.0;
                    stored.menu_entry = previous.1;
                }
            }
            self.status = Some(format!("Could not save shortcut settings: {error:#}"));
            return;
        }

        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();

        if state.desktop {
            match xdg::desktop_dir() {
                Ok(d) => match desktop_entry::write_desktop_shortcut(&entry, &d) {
                    Ok(()) => added.push(format!(
                        "desktop shortcut ({})",
                        desktop_entry::desktop_shortcut_path(&entry, &d).display()
                    )),
                    Err(e) => errors.push(format!("desktop shortcut: {e:#}")),
                },
                Err(e) => errors.push(format!("desktop directory: {e:#}")),
            }
        } else if let Ok(d) = xdg::desktop_dir() {
            let path = desktop_entry::desktop_shortcut_path(&entry, &d);
            if path.exists() {
                match desktop_entry::remove_desktop_shortcut(&entry, &d) {
                    Ok(()) => removed.push("desktop shortcut".into()),
                    Err(e) => errors.push(format!("remove desktop shortcut: {e:#}")),
                }
            }
        }

        if state.menu {
            match xdg::applications_dir() {
                Ok(a) => match desktop_entry::write_menu_entry(&entry, &a) {
                    Ok(()) => added.push(format!(
                        "start menu entry ({})",
                        desktop_entry::menu_entry_path(&entry, &a).display()
                    )),
                    Err(e) => errors.push(format!("start menu entry: {e:#}")),
                },
                Err(e) => errors.push(format!("applications directory: {e:#}")),
            }
        } else if let Ok(a) = xdg::applications_dir() {
            let path = desktop_entry::menu_entry_path(&entry, &a);
            if path.exists() {
                match desktop_entry::remove_menu_entry(&entry, &a) {
                    Ok(()) => removed.push("start menu entry".into()),
                    Err(e) => errors.push(format!("remove start menu entry: {e:#}")),
                }
            }
        }

        if errors.is_empty() {
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!("Added {}", added.join(" and ")));
            }
            if !removed.is_empty() {
                parts.push(format!("Removed {}", removed.join(" and ")));
            }
            if parts.is_empty() {
                self.status = Some("No changes (selected entries already match).".into());
            } else {
                self.status = Some(parts.join(". "));
            }
            self.add_to = None;
        } else {
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!("Added {}", added.join(" and ")));
            }
            if !removed.is_empty() {
                parts.push(format!("Removed {}", removed.join(" and ")));
            }
            parts.push(format!("Failed: {}", errors.join("; ")));
            self.status = Some(parts.join(". "));
        }
    }

    pub(crate) fn check_hub_update_if_allowed(&mut self) {
        let now = now_unix();
        let cooldown = self.update_check_cooldown();
        let allowed = if let Ok(mut cfg) = self.config.lock() {
            if update_check_cooldown_remaining(cfg.last_hub_update_check, now, cooldown).is_some() {
                false
            } else {
                cfg.last_hub_update_check = Some(now);
                let _ = cfg.save();
                true
            }
        } else {
            false
        };
        if allowed {
            self.send_cmd(BackendCommand::CheckHubUpdate);
        }
    }

    pub(crate) fn start_hub_update(&mut self) {
        let Some(url) = self.hub_update_url.clone() else {
            return;
        };
        let Some(target) = current_appimage_path() else {
            self.status = Some("Cannot determine the running AppImage path.".into());
            return;
        };
        self.hub_update_progress = Some(0.0);
        self.send_cmd(BackendCommand::SelfUpdate {
            url,
            target,
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }

    pub(crate) fn check_all_updates(&mut self) {
        if self.update_check.is_some() {
            return;
        }

        let now = now_unix();
        let cooldown = self.update_check_cooldown();
        let (ids, rate_limited) = {
            let Ok(mut cfg) = self.config.lock() else {
                self.status = Some("Could not access update settings.".into());
                return;
            };
            if let Some(remaining) =
                update_check_cooldown_remaining(cfg.last_update_check, now, cooldown)
            {
                (Vec::new(), Some(remaining))
            } else {
                let ids = cfg
                    .applications
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.source,
                            AppSource::LocalWithRepo { .. } | AppSource::GithubOnly { .. }
                        )
                    })
                    .map(|a| a.id.clone())
                    .collect::<Vec<_>>();
                if !ids.is_empty() {
                    cfg.last_update_check = Some(now);
                    let _ = cfg.save();
                }
                (ids, None)
            }
        };

        if let Some(remaining) = rate_limited {
            self.status = Some(format!(
                "Update checks are limited to once every {} minutes. Try again in {}m {}s.",
                cooldown / 60,
                remaining / 60,
                remaining % 60
            ));
            return;
        }

        if ids.is_empty() {
            self.status = Some("No GitHub-tracked apps to check.".into());
            return;
        }

        let progress = UpdateCheckProgress::start(
            ids,
            |app_id| {
                self.send_cmd(BackendCommand::CheckUpdate {
                    app_id: app_id.to_string(),
                })
            },
            Instant::now(),
        );
        if progress.is_some() {
            self.update_check = progress;
        } else {
            self.status = Some("System busy, please try again.".into());
        }
    }

    pub(crate) fn update_all(&mut self) {
        if !self.ops.is_empty() {
            return;
        }
        let updates: Vec<(String, AvailableUpdate)> = self
            .update_available
            .iter()
            .map(|(app_id, update)| (app_id.clone(), update.clone()))
            .collect();
        for (app_id, update) in updates {
            self.start_update_download(
                app_id,
                update.url,
                update.tag,
                update.commit_hash,
                update.checksum_url,
            );
        }
    }

    /// Rate limit between manual update checks: shorter when a GitHub token is
    /// configured (the authenticated API quota is much larger).
    pub(crate) fn update_check_cooldown(&self) -> u64 {
        update_check_cooldown_secs(self.has_github_token)
    }

    /// Whether automatic update checks on startup are enabled (persisted in config).
    pub(crate) fn auto_check_enabled(&self) -> bool {
        self.config
            .lock()
            .map(|c| c.auto_check_updates)
            .unwrap_or(true)
    }

    pub(crate) fn auto_populate_from_appimage(&mut self, appimage_path: &Path) {
        // A sidecar written beside the AppImage is authoritative and avoids
        // re-executing the AppImage just to read its metadata again.
        if let Some(meta) = appmeta::read_for_appimage(appimage_path) {
            self.apply_sidecar_metadata(appimage_path, &meta);
            return;
        }
        if let Ok(meta) = crate::appimage::extract_metadata_with_policy(
            appimage_path,
            None,
            crate::appimage::ExtractionPolicy::ExecuteAllowed,
        ) {
            let stem = appimage_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();

            if self.side_panel_form.display_name.trim().is_empty()
                || self.side_panel_form.display_name.trim() == stem
            {
                if let Some(name) = meta.name {
                    self.side_panel_form.display_name = name;
                } else if !stem.is_empty() {
                    self.side_panel_form.display_name = stem.to_string();
                }
            }

            if self.side_panel_form.generic_name.trim().is_empty() {
                if let Some(generic) = meta.generic_name {
                    self.side_panel_form.generic_name = generic;
                }
            }

            if self.side_panel_form.comment.trim().is_empty() {
                if let Some(comment) = meta.comment {
                    self.side_panel_form.comment = comment;
                }
            }

            // Only auto-fill the category when the user has not chosen one:
            // an explicit selection must always survive metadata detection.
            if !self.side_panel_form.category_user_selected {
                if let Some(cat) = meta.matched_category {
                    self.side_panel_form.detected_category = Some(cat);
                }
            }

            if let Some(notify) = meta.startup_notify {
                self.side_panel_form.startup_notify = notify;
            }

            if let Some(icon_p) = meta.icon_path {
                self.side_panel_form.icon_path = icon_p.display().to_string();
            }

            if self.side_panel_form.mime_types.trim().is_empty() {
                if let Some(mime_types) = meta.mime_types {
                    self.side_panel_form.mime_types = mime_types;
                }
            }
            if self.side_panel_form.exec_arg.trim().is_empty() {
                if let Some(exec_arg) = meta.exec_arg {
                    self.side_panel_form.exec_arg = exec_arg;
                }
            }
        }
    }

    /// Fills the Add form from the JSON sidecar stored beside an AppImage.
    pub(crate) fn apply_sidecar_metadata(
        &mut self,
        appimage_path: &Path,
        meta: &appmeta::AppMetadataFile,
    ) {
        let stem = appimage_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();

        if (self.side_panel_form.display_name.trim().is_empty()
            || self.side_panel_form.display_name.trim() == stem)
            && !meta.display_name.trim().is_empty()
        {
            self.side_panel_form.display_name = meta.display_name.trim().to_string();
        }

        if self.side_panel_form.generic_name.trim().is_empty() {
            if let Some(generic) = &meta.generic_name {
                self.side_panel_form.generic_name = generic.clone();
            }
        }

        if self.side_panel_form.comment.trim().is_empty() {
            if let Some(comment) = &meta.comment {
                self.side_panel_form.comment = comment.clone();
            }
        }

        if !self.side_panel_form.category_user_selected {
            if let Some(raw) = meta.categories.as_deref() {
                apply_detected_category(&mut self.side_panel_form, raw);
            }
        }

        self.side_panel_form.startup_notify = meta.startup_notify;

        if self.side_panel_form.mime_types.trim().is_empty() {
            if let Some(mime_types) = meta
                .mime_types
                .as_deref()
                .and_then(crate::desktop_entry::normalize_mime_types)
            {
                self.side_panel_form.mime_types = mime_types;
            }
        }
        if self.side_panel_form.exec_arg.trim().is_empty() {
            if let Some(exec_arg) = &meta.exec_arg {
                self.side_panel_form.exec_arg = exec_arg.clone();
            }
        }

        if self.side_panel_form.launch_args.trim().is_empty() && !meta.args.is_empty() {
            self.side_panel_form.launch_args = meta.args.join(" ");
        }

        if self.side_panel_form.icon_path.trim().is_empty() {
            if let Some(icon) = meta.icon.as_deref() {
                let icon = icon.trim();
                if !icon.is_empty() {
                    let path = std::path::Path::new(icon);
                    let resolved = if path.is_absolute() {
                        path.to_path_buf()
                    } else {
                        appimage_path
                            .parent()
                            .map(|folder| folder.join(path))
                            .unwrap_or_else(|| path.to_path_buf())
                    };
                    self.side_panel_form.icon_path = resolved.display().to_string();
                }
            }
        }

        if self.side_panel_form.github_url.trim().is_empty() {
            if let Some(source) = &meta.source {
                match source {
                    AppSource::LocalWithRepo { github_repo }
                    | AppSource::GithubOnly { github_repo } => {
                        self.side_panel_form.github_url = github_repo.clone();
                    }
                    AppSource::LocalOnly => {}
                }
            }
        }

        if self.side_panel_form.manual_asset.is_none() {
            self.side_panel_form.manual_asset = meta.asset_preference.clone();
        }
    }

    pub(crate) fn repair_missing_icons(&mut self) {
        let mut modified = false;
        let mut apps_to_rewrite: Vec<AppEntry> = Vec::new();

        if let Ok(mut cfg) = self.config.lock() {
            for app in &mut cfg.applications {
                let has_valid_icon = app.icon_path.as_ref().map(|p| p.exists()).unwrap_or(false);

                if !has_valid_icon && app.local_path.exists() {
                    if let Some(folder) = app.local_path.parent() {
                        let direct_png = folder.join("icon.png");
                        let direct_svg = folder.join("icon.svg");
                        if direct_png.exists() {
                            app.icon_path = Some(direct_png);
                            modified = true;
                            apps_to_rewrite.push(app.clone());
                        } else if direct_svg.exists() {
                            app.icon_path = Some(direct_svg);
                            modified = true;
                            apps_to_rewrite.push(app.clone());
                        } else if let Ok(extracted) = crate::appimage::extract_icon_with_policy(
                            &app.local_path,
                            folder,
                            crate::appimage::ExtractionPolicy::NeverExecute,
                        ) {
                            app.icon_path = Some(extracted);
                            modified = true;
                            apps_to_rewrite.push(app.clone());
                        }
                    }
                }
            }
            if modified {
                let _ = cfg.save();
            }
        }

        for app in apps_to_rewrite {
            refresh_existing_shortcuts(&app);
        }
    }

    /// Renames already-managed binaries to the clean `<name>.appimage` form,
    /// so entries added before this naming scheme also end up clean.
    /// Idempotent: skips missing files and existing target names.
    pub(crate) fn normalize_managed_filenames(&mut self) {
        let mut moves: Vec<(String, PathBuf, PathBuf)> = Vec::new();
        if let Ok(cfg) = self.config.lock() {
            let roots = crate::storage::roots(&cfg);
            for app in &cfg.applications {
                if !app.local_path.exists() {
                    continue;
                }
                if !crate::storage::is_in_storage_root(&app.local_path, &roots) {
                    continue;
                }
                if let Some(target) = clean_managed_path(&app.local_path, &app.display_name) {
                    if !target.exists() {
                        moves.push((app.id.clone(), app.local_path.clone(), target));
                    }
                }
            }
        }

        let mut moved: Vec<(String, PathBuf)> = Vec::new();
        for (id, old, new) in moves {
            if std::fs::rename(&old, &new).is_err() {
                continue;
            }
            // Drop the sidecar that belonged to the previous file name.
            let _ = std::fs::remove_file(appmeta::sidecar_path(&old));
            moved.push((id, new));
        }
        if moved.is_empty() {
            return;
        }

        let mut rewritten: Vec<AppEntry> = Vec::new();
        if let Ok(mut cfg) = self.config.lock() {
            for (id, new_path) in &moved {
                if let Some(entry) = cfg.applications.iter_mut().find(|a| &a.id == id) {
                    entry.local_path = new_path.clone();
                    entry.file_fingerprint = crate::config::file_fingerprint(new_path);
                    rewritten.push(entry.clone());
                }
            }
            let _ = cfg.save();
        }
        for entry in &rewritten {
            refresh_existing_shortcuts(entry);
            let _ = appmeta::write_for_entry(entry);
        }
    }

    pub(crate) fn app_storage_dir(&self) -> PathBuf {
        self.config
            .lock()
            .ok()
            .and_then(|cfg| cfg.custom_download_dir.clone())
            .unwrap_or_else(xdg::default_managed_apps_dir)
    }

    /// Keeps `<name>.json` sidecars in sync with the config, and scans the
    /// selected storage folder for existing AppImages that are missing from
    /// `config.toml` (for example after choosing a folder with previous apps).
    ///
    /// Invalid sidecars are quarantined and reported. Sidecars written by a
    /// newer AppImage Hub are never overwritten.
    pub(crate) fn sync_metadata_files(&mut self) -> MetadataSyncReport {
        let mut report = MetadataSyncReport::default();
        let mut known_ids: HashSet<String> = HashSet::new();
        let mut known_paths: HashSet<PathBuf> = HashSet::new();
        let mut missing: Vec<AppEntry> = Vec::new();
        // Paths whose sidecar must not be rewritten (newer schema).
        let mut protected: HashSet<PathBuf> = HashSet::new();

        if let Ok(cfg) = self.config.lock() {
            for app in &cfg.applications {
                known_ids.insert(app.id.clone());
                known_paths.insert(app.local_path.clone());
                if !app.local_path.exists() {
                    continue;
                }
                match appmeta::read_for_appimage_result(&app.local_path) {
                    Ok(Some(_)) => {}
                    Ok(None) => missing.push(app.clone()),
                    Err(error) => {
                        if matches!(error, appmeta::SidecarError::UnsupportedSchema { .. }) {
                            report.skipped_newer += 1;
                            protected.insert(app.local_path.clone());
                        } else {
                            // Invalid JSON is already quarantined by the read.
                            report.repaired += 1;
                            missing.push(app.clone());
                        }
                        report.warnings.push(error.to_string());
                    }
                }
            }
        }
        for app in missing {
            if protected.contains(&app.local_path) {
                continue;
            }
            if let Err(error) = appmeta::write_for_entry(&app) {
                report.warnings.push(format!(
                    "Could not restore metadata for {}: {error:#}",
                    app.display_name
                ));
            }
        }

        let base = self.app_storage_dir();
        let mut appimages = Vec::new();
        collect_appimages(&base, &mut appimages, 0);

        let mut adopted: Vec<AppEntry> = Vec::new();
        for appimage in crate::storage::unknown_candidates(appimages, &known_paths) {
            let discovered = match appmeta::read_for_appimage_result(&appimage) {
                Ok(meta) => meta,
                Err(error) => {
                    if matches!(error, appmeta::SidecarError::UnsupportedSchema { .. }) {
                        report.skipped_newer += 1;
                        protected.insert(appimage.clone());
                    } else {
                        report.repaired += 1;
                    }
                    report.warnings.push(error.to_string());
                    None
                }
            };
            let mut entry = match discovered {
                Some(meta) => meta.to_entry(appimage.clone()),
                None => app_entry_from_appimage(&appimage),
            };
            // Never trust a sidecar's fingerprint for a file we are adopting
            // now: baseline whatever is actually on disk.
            entry.file_fingerprint = crate::config::file_fingerprint(&appimage);
            if known_ids.contains(&entry.id) {
                continue;
            }
            known_ids.insert(entry.id.clone());
            known_paths.insert(appimage);
            adopted.push(entry);
        }

        if adopted.is_empty() {
            return report;
        }
        let saved = if let Ok(mut cfg) = self.config.lock() {
            cfg.applications.extend(adopted.iter().cloned());
            cfg.save()
        } else {
            Err(anyhow::anyhow!("configuration lock is poisoned"))
        };
        if let Err(error) = saved {
            // Roll the in-memory list back so it matches the file on disk.
            if let Ok(mut cfg) = self.config.lock() {
                let keep = cfg.applications.len().saturating_sub(adopted.len());
                cfg.applications.truncate(keep);
            }
            report
                .warnings
                .push(format!("Could not save adopted apps: {error:#}"));
            return report;
        }
        for entry in &adopted {
            // Scanning a folder must not create shortcuts by itself. Only
            // update entries the user has already chosen to create.
            refresh_existing_shortcuts(entry);
            if protected.contains(&entry.local_path) {
                continue;
            }
            if let Err(error) = appmeta::write_for_entry(entry) {
                report.warnings.push(format!(
                    "Could not write metadata for {}: {error:#}",
                    entry.display_name
                ));
            }
        }
        report.adopted = adopted.len();
        report
    }

    pub(crate) fn updates_available_count(&self) -> usize {
        self.update_available.len()
    }

    /// Clears a "changed on disk" warning by accepting the current file as the
    /// new baseline. Runs in the backend so hashing does not block the UI.
    pub(crate) fn accept_current_file(&mut self, app_id: &str) {
        self.send_cmd(BackendCommand::AcceptCurrentFile {
            app_id: app_id.to_string(),
        });
    }

    /// Local import request shared by Local Only and Local + Repo.
    fn import_local(&mut self, request: ImportRequest) {
        let id = Uuid::new_v4().to_string();
        let custom_base = self
            .config
            .lock()
            .ok()
            .and_then(|config| config.custom_download_dir.clone());
        let managed_path = match ensure_in_managed_folder(
            &request.path,
            &request.display_name,
            &id,
            custom_base.as_deref(),
        ) {
            Ok(path) => path,
            Err(error) => {
                self.report_error("failed to prepare app folder", error);
                return;
            }
        };
        let folder = managed_path
            .parent()
            .unwrap_or_else(|| std::path::Path::new(""));
        let final_icon_path = resolve_final_icon(request.icon_path, &managed_path, folder);
        let source = match &request.repo {
            Some(repo) => AppSource::LocalWithRepo {
                github_repo: repo.clone(),
            },
            None => AppSource::LocalOnly,
        };
        let entry = AppEntry {
            id: id.clone(),
            display_name: request.display_name,
            generic_name: request.generic_name,
            comment: request.comment,
            categories: request.categories,
            startup_notify: request.startup_notify,
            source,
            local_path: managed_path.clone(),
            current_version: String::new(),
            args: request.args,
            icon_path: final_icon_path,
            asset_preference: request.asset_preference,
            added_at: Some(now_unix()),
            commit_hash: None,
            file_fingerprint: crate::config::file_fingerprint(&managed_path),
            desktop_shortcut: request.desktop_shortcut,
            menu_entry: request.menu_entry,
            mime_types: request.mime_types,
            exec_arg: request.exec_arg,
        };
        if let Err(error) = permissions::set_executable(&managed_path) {
            self.report_error("chmod failed", error);
            return;
        }
        // Persist the list entry before creating shortcuts: a failed save must
        // not leave a shortcut for an app the next launch would not know about.
        if !self.push_entry(entry.clone()) {
            return;
        }
        let _ = appmeta::write_for_entry(&entry);
        if let Err(error) = apply_app_shortcuts(&entry) {
            self.report_error("App saved, but desktop entry failed", error);
            return;
        }
        if request.repo.is_some() {
            self.send_cmd(BackendCommand::CheckUpdate { app_id: id });
        }
    }

    pub(crate) fn submit_side_panel_form(&mut self) {
        use crate::ui::side_panel::SidePanelMode;

        let form = self.side_panel_form.clone();
        let display_name = form.display_name.trim().to_string();
        if display_name.is_empty() {
            self.status = Some("Display Name is required".into());
            return;
        }
        let args = match crate::args::split_args(&form.launch_args) {
            Ok(args) => args,
            Err(error) => {
                self.status = Some(format!("{error}"));
                return;
            }
        };
        let icon_path = {
            let p = form.icon_path.trim();
            if p.is_empty() {
                None
            } else {
                Some(PathBuf::from(p))
            }
        };

        let generic_name = {
            let g = form.generic_name.trim();
            if g.is_empty() {
                None
            } else {
                Some(g.to_string())
            }
        };
        let comment = {
            let c = form.comment.trim();
            if c.is_empty() {
                None
            } else {
                Some(c.to_string())
            }
        };
        let categories = submitted_categories(&form);
        let startup_notify = form.startup_notify;
        let desktop_shortcut = form.desktop_shortcut;
        let menu_entry = form.menu_entry;
        let mime_types = crate::desktop_entry::normalize_mime_types(form.mime_types.trim());
        let exec_arg = {
            let value = form.exec_arg.trim();
            (!value.is_empty()).then(|| value.to_string())
        };
        let asset_preference =
            if form.asset_selection_mode == crate::ui::side_panel::AssetSelectionMode::Manual {
                form.manual_asset.clone()
            } else {
                None
            };

        if let Some(app_id) = form.editing_app_id.clone() {
            let mut old_entry: Option<AppEntry> = None;
            let mut updated: Option<AppEntry> = None;
            let saved = if let Ok(mut cfg) = self.config.lock() {
                if let Some(e) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
                    old_entry = Some(e.clone());
                    e.display_name = display_name;
                    e.generic_name = generic_name;
                    e.comment = comment;
                    e.categories = categories;
                    e.startup_notify = startup_notify;
                    e.desktop_shortcut = desktop_shortcut;
                    e.menu_entry = menu_entry;
                    e.mime_types = mime_types.clone();
                    e.exec_arg = exec_arg.clone();
                    e.args = args;
                    e.asset_preference = asset_preference;
                    if let Some(folder) = e.local_path.parent() {
                        e.icon_path = resolve_final_icon(icon_path, &e.local_path, folder);
                    } else {
                        e.icon_path = icon_path;
                    }
                    match form.mode {
                        SidePanelMode::LocalOnly => {
                            e.source = AppSource::LocalOnly;
                        }
                        SidePanelMode::LocalWithRepo => {
                            let repo = form.github_url.trim().to_string();
                            if repo.is_empty() {
                                self.status = Some("GitHub URL is required".into());
                                return;
                            }
                            e.source = AppSource::LocalWithRepo { github_repo: repo };
                        }
                        SidePanelMode::GithubOnly => {
                            let repo = form.github_url.trim().to_string();
                            if repo.is_empty() {
                                self.status = Some("GitHub URL is required".into());
                                return;
                            }
                            e.source = AppSource::GithubOnly { github_repo: repo };
                        }
                    }
                    updated = Some(e.clone());
                }
                cfg.save()
            } else {
                Err(anyhow::anyhow!("configuration lock is poisoned"))
            };
            if let Err(error) = saved {
                // Revert the in-memory entry so it matches the untouched file.
                if let (Some(old), Some(changed)) = (old_entry.take(), updated.take()) {
                    if let Ok(mut cfg) = self.config.lock() {
                        if let Some(stored) =
                            cfg.applications.iter_mut().find(|a| a.id == changed.id)
                        {
                            *stored = old;
                        }
                    }
                }
                self.status = Some(format!("Could not save changes: {error:#}"));
                return;
            }
            if let (Some(old), Some(e)) = (old_entry, updated) {
                if let (Ok(d), Ok(a)) = (xdg::desktop_dir(), xdg::applications_dir()) {
                    let _ = desktop_entry::remove_entry(&old, &d, &a);
                }
                if let Err(e) = apply_app_shortcuts(&e) {
                    self.status = Some(format!("Saved, but menu entry failed: {e:#}"));
                    self.is_form_open = false;
                    self.side_panel_form.clear();
                    return;
                }
                let _ = appmeta::write_for_entry(&e);
            }
            self.is_form_open = false;
            self.side_panel_form.clear();
            self.status = None;
            return;
        }

        match form.mode {
            SidePanelMode::LocalOnly | SidePanelMode::LocalWithRepo => {
                if form.mode == SidePanelMode::LocalWithRepo
                    && (form.local_path.trim().is_empty() || form.github_url.trim().is_empty())
                {
                    self.set_status("Provide AppImage path and GitHub URL");
                    return;
                }
                if form.local_path.trim().is_empty() {
                    self.set_status("Select an AppImage file");
                    return;
                }
                let repo = if form.mode == SidePanelMode::LocalWithRepo {
                    Some(form.github_url.trim().to_string())
                } else {
                    None
                };
                let path = PathBuf::from(form.local_path.trim());
                if path.exists() && !crate::downloader::is_elf_file(&path) {
                    self.status =
                        Some("Selected file is not an AppImage (missing ELF header).".into());
                    return;
                }
                let asset_preference = repo.as_ref().and(asset_preference);
                self.import_local(ImportRequest {
                    path,
                    display_name,
                    generic_name,
                    comment,
                    categories,
                    startup_notify,
                    desktop_shortcut,
                    menu_entry,
                    mime_types,
                    exec_arg,
                    args,
                    icon_path,
                    asset_preference,
                    repo,
                });
            }
            SidePanelMode::GithubOnly => {
                if form.github_url.trim().is_empty() {
                    self.status = Some("Provide GitHub URL".into());
                    return;
                }
                self.start_install_from_repo(
                    form.github_url.trim().to_string(),
                    display_name,
                    asset_preference,
                    categories,
                    desktop_shortcut,
                    menu_entry,
                    mime_types,
                    exec_arg,
                );
            }
        }

        self.is_form_open = false;
        self.side_panel_form.clear();
    }
}
