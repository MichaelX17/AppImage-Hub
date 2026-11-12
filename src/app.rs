use crate::config::{AppEntry, AppSource, Config};
use crate::desktop_entry;
use crate::downloader;
use crate::github;
use crate::launcher;
use crate::permissions;
use crate::xdg;
use eframe::egui::{self, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum BackendCommand {
    CheckUpdate {
        app_id: String,
    },
    DownloadAndReplace {
        op_id: String,
        app_id: String,
        url: String,
        tag: String,
        asset_name: String,
        cancel: Arc<AtomicBool>,
    },
    InstallFromRepo {
        op_id: String,
        github_repo: String,
        display_name: String,
        cancel: Arc<AtomicBool>,
    },
    Launch {
        app_id: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BackendEvent {
    UpdateAvailable {
        app_id: String,
        tag: String,
        url: String,
        asset_name: String,
    },
    UpToDate {
        app_id: String,
        tag: String,
    },
    DownloadStarted {
        op_id: String,
        app_id: Option<String>,
        label: String,
        kind: OpKind,
    },
    DownloadProgress {
        op_id: String,
        app_id: Option<String>,
        fraction: f32,
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadComplete {
        op_id: String,
        app_id: String,
        tag: String,
    },
    AppInstalled {
        op_id: String,
        app_id: String,
    },
    DownloadCancelled {
        op_id: String,
        app_id: Option<String>,
    },
    Error {
        op_id: Option<String>,
        app_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Install,
    Update,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ActiveOp {
    pub op_id: String,
    pub app_id: Option<String>,
    pub label: String,
    pub kind: OpKind,
    pub fraction: f32,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub started_at: Instant,
    pub last_update: Instant,
    pub cancel: Arc<AtomicBool>,
    pub retry: Option<RetryAction>,
}

#[derive(Debug, Clone)]
pub enum RetryAction {
    InstallFromRepo {
        github_repo: String,
        display_name: String,
    },
    DownloadAndReplace {
        app_id: String,
        url: String,
        tag: String,
        asset_name: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FailedOp {
    pub app_id: Option<String>,
    pub label: String,
    pub message: String,
    pub retry: Option<RetryAction>,
    pub at: Instant,
}

pub enum AppDownloadState<'a> {
    Downloading(&'a ActiveOp),
    Failed(&'a FailedOp),
    Idle,
}

#[derive(Debug, Clone)]
pub struct AddToState {
    pub app_id: String,
    pub app_name: String,
    pub desktop: bool,
    pub menu: bool,
}

pub struct ManagerApp {
    pub(crate) config: Arc<Mutex<Config>>,
    pub(crate) cmd_tx: mpsc::Sender<BackendCommand>,
    pub(crate) evt_rx: mpsc::Receiver<BackendEvent>,
    pub(crate) ops: HashMap<String, ActiveOp>,
    pub(crate) failed_ops: Vec<FailedOp>,
    pub(crate) status: Option<String>,
    pub(crate) pending_delete: Option<String>,
    pub(crate) update_available: HashMap<String, (String, String, String)>,
    pub(crate) latest_versions: HashMap<String, String>,
    pub(crate) is_form_open: bool,
    pub(crate) side_panel_form: crate::ui::side_panel::SidePanelFormState,
    pub(crate) add_to: Option<AddToState>,
}

pub fn setup_custom_style(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    crate::ui::theme::apply(ctx);
}

impl ManagerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        github::ensure_crypto_provider();
        setup_custom_style(&cc.egui_ctx);

        let config = Arc::new(Mutex::new(Config::load().unwrap_or_else(|e| {
            eprintln!("failed to load config: {e:#}");
            Config::default()
        })));

        let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCommand>(64);
        let (evt_tx, evt_rx) = mpsc::channel::<BackendEvent>(256);
        spawn_backend(config.clone(), cmd_rx, evt_tx);

        Self {
            config,
            cmd_tx,
            evt_rx,
            ops: HashMap::new(),
            failed_ops: Vec::new(),
            status: None,
            pending_delete: None,
            update_available: HashMap::new(),
            latest_versions: HashMap::new(),
            is_form_open: false,
            side_panel_form: crate::ui::side_panel::SidePanelFormState::default(),
            add_to: None,
        }
    }

    pub(crate) fn send_cmd(&mut self, cmd: BackendCommand) {
        if self.cmd_tx.try_send(cmd).is_err() {
            self.status = Some("System busy, please try again.".into());
        }
    }

    fn app_display_name(&self, app_id: &str) -> String {
        self.config
            .lock()
            .ok()
            .and_then(|c| {
                c.applications
                    .iter()
                    .find(|a| a.id == app_id)
                    .map(|a| a.display_name.clone())
            })
            .unwrap_or_else(|| app_id.to_string())
    }

    fn notify_completion(&self, app_id: &str, tag: &str) {
        let name = self.app_display_name(app_id);
        let message = if tag.is_empty() {
            format!("Successfully installed {name}")
        } else {
            format!("Successfully updated {name} to {tag}")
        };
        let _ = std::process::Command::new("notify-send")
            .args(["AppImage Manager", &message])
            .spawn();
    }

    fn apply_event(&mut self, evt: BackendEvent) {
        match evt {
            BackendEvent::UpdateAvailable {
                app_id,
                tag,
                url,
                asset_name,
            } => {
                self.latest_versions.insert(app_id.clone(), tag.clone());
                self.update_available.insert(app_id, (tag, url, asset_name));
            }
            BackendEvent::UpToDate { app_id, tag } => {
                self.latest_versions.insert(app_id.clone(), tag);
                self.update_available.remove(&app_id);
            }
            BackendEvent::DownloadStarted {
                op_id,
                app_id,
                label,
                kind,
            } => {
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
            BackendEvent::DownloadComplete { op_id, app_id, tag } => {
                self.ops.remove(&op_id);
                self.update_available.remove(&app_id);
                self.latest_versions.insert(app_id.clone(), tag.clone());
                self.notify_completion(&app_id, &tag);
            }
            BackendEvent::AppInstalled { op_id, app_id } => {
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
                self.notify_completion(&app_id, "");
            }
            BackendEvent::DownloadCancelled { op_id, .. } => {
                if let Some(op) = self.ops.remove(&op_id) {
                    self.status = Some(format!("Cancelled: {}", op.label));
                }
            }
            BackendEvent::Error {
                op_id,
                app_id,
                message,
            } => {
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
                    at: Instant::now(),
                });
                if self.failed_ops.len() > 20 {
                    let drain = self.failed_ops.len() - 20;
                    self.failed_ops.drain(0..drain);
                }
            }
        }
    }

    pub(crate) fn start_install_from_repo(&mut self, github_repo: String, display_name: String) {
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
                }),
            },
        );
        self.send_cmd(BackendCommand::InstallFromRepo {
            op_id,
            github_repo,
            display_name,
            cancel,
        });
    }

    pub(crate) fn start_update_download(
        &mut self,
        app_id: String,
        url: String,
        tag: String,
        asset_name: String,
    ) {
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
                    asset_name: asset_name.clone(),
                }),
            },
        );
        self.send_cmd(BackendCommand::DownloadAndReplace {
            op_id,
            app_id,
            url,
            tag,
            asset_name,
            cancel,
        });
    }

    pub(crate) fn request_delete(&mut self, app_id: String) {
        self.pending_delete = Some(app_id);
    }

    pub(crate) fn confirm_delete(&mut self, app_id: &str) {
        let mut removed: Option<AppEntry> = None;
        if let Ok(mut cfg) = self.config.lock() {
            if let Some(pos) = cfg.applications.iter().position(|a| a.id == app_id) {
                removed = Some(cfg.applications.remove(pos));
            }
            let _ = cfg.save();
        }
        if let Some(entry) = removed {
            if let (Ok(d), Ok(a)) = (xdg::desktop_dir(), xdg::applications_dir()) {
                let _ = desktop_entry::remove_entry(&entry, &d, &a);
            }
            if xdg::is_path_in_managed_apps(&entry.local_path) {
                if let Some(parent) = entry.local_path.parent() {
                    let _ = std::fs::remove_dir_all(parent);
                }
            } else if matches!(entry.source, AppSource::GithubOnly { .. }) {
                let _ = std::fs::remove_file(&entry.local_path);
            }
        }
        self.update_available.remove(app_id);
        self.latest_versions.remove(app_id);
    }

    pub(crate) fn open_add_form(&mut self) {
        self.add_to = None;
        self.side_panel_form.clear();
        self.status = None;
        self.is_form_open = true;
    }

    fn push_entry(&mut self, entry: AppEntry) {
        if let Ok(mut cfg) = self.config.lock() {
            cfg.applications.push(entry);
            let _ = cfg.save();
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

        self.add_to = None;
        self.side_panel_form = crate::ui::side_panel::SidePanelFormState {
            editing_app_id: Some(entry.id.clone()),
            display_name: entry.display_name.clone(),
            mode,
            local_path: entry.local_path.display().to_string(),
            icon_path: entry
                .icon_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            github_url,
            launch_args: entry.args.join(" "),
        };
        self.status = None;
        self.is_form_open = true;
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
                } => self.start_install_from_repo(github_repo, display_name),
                RetryAction::DownloadAndReplace {
                    app_id,
                    url,
                    tag,
                    asset_name,
                } => self.start_update_download(app_id, url, tag, asset_name),
            }
        }
    }

    pub(crate) fn app_download_state(&self, app_id: &str) -> AppDownloadState<'_> {
        if let Some(op) = self
            .ops
            .values()
            .find(|op| op.app_id.as_deref() == Some(app_id))
        {
            return AppDownloadState::Downloading(op);
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
        self.side_panel_form.clear();
        let (desktop, menu) = match (xdg::desktop_dir(), xdg::applications_dir()) {
            (Ok(d), Ok(a)) => (
                desktop_entry::desktop_shortcut_path(app, &d).exists(),
                desktop_entry::menu_entry_path(app, &a).exists(),
            ),
            _ => (false, false),
        };
        self.add_to = Some(AddToState {
            app_id: app.id.clone(),
            app_name: app.display_name.clone(),
            desktop,
            menu,
        });
        self.status = None;
    }

    pub(crate) fn close_add_to(&mut self) {
        self.add_to = None;
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
        let Some(entry) = entry else {
            self.status = Some("Application not found".into());
            self.add_to = None;
            return;
        };

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

    pub(crate) fn is_right_panel_open(&self) -> bool {
        self.is_form_open || self.add_to.is_some()
    }

    pub(crate) fn check_all_updates(&mut self) {
        let ids: Vec<String> = self
            .config
            .lock()
            .map(|c| {
                c.applications
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.source,
                            AppSource::LocalWithRepo { .. } | AppSource::GithubOnly { .. }
                        )
                    })
                    .map(|a| a.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        for app_id in ids {
            self.send_cmd(BackendCommand::CheckUpdate { app_id });
        }
    }

    pub(crate) fn update_all(&mut self) {
        if !self.ops.is_empty() {
            return;
        }
        let updates: Vec<(String, String, String, String)> = self
            .update_available
            .iter()
            .map(|(app_id, (tag, url, asset_name))| {
                (app_id.clone(), tag.clone(), url.clone(), asset_name.clone())
            })
            .collect();
        for (app_id, tag, url, asset_name) in updates {
            self.start_update_download(app_id, url, tag, asset_name);
        }
    }

    pub(crate) fn updates_available_count(&self) -> usize {
        self.update_available.len()
    }

    pub(crate) fn submit_side_panel_form(&mut self) {
        use crate::ui::side_panel::SidePanelMode;

        let form = self.side_panel_form.clone();
        let display_name = form.display_name.trim().to_string();
        if display_name.is_empty() {
            self.status = Some("Display Name is required".into());
            return;
        }
        let args = parse_args(&form.launch_args);
        let icon_path = {
            let p = form.icon_path.trim();
            if p.is_empty() {
                None
            } else {
                Some(PathBuf::from(p))
            }
        };

        if let Some(app_id) = form.editing_app_id.clone() {
            let mut old_entry: Option<AppEntry> = None;
            let mut updated: Option<AppEntry> = None;
            if let Ok(mut cfg) = self.config.lock() {
                if let Some(e) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
                    old_entry = Some(e.clone());
                    e.display_name = display_name;
                    e.args = args;
                    e.icon_path = icon_path;
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
                let _ = cfg.save();
            }
            if let (Some(old), Some(e)) = (old_entry, updated) {
                if let (Ok(d), Ok(a)) = (xdg::desktop_dir(), xdg::applications_dir()) {
                    let _ = desktop_entry::remove_entry(&old, &d, &a);
                }
                if let Err(e) = write_desktop(&e) {
                    self.status = Some(format!("Saved, but menu entry failed: {e:#}"));
                    self.is_form_open = false;
                    self.side_panel_form.clear();
                    return;
                }
            }
            self.is_form_open = false;
            self.side_panel_form.clear();
            self.status = None;
            return;
        }

        match form.mode {
            SidePanelMode::LocalOnly => {
                if form.local_path.trim().is_empty() {
                    self.status = Some("Select an AppImage file".into());
                    return;
                }
                let path = PathBuf::from(form.local_path.trim());
                let id = Uuid::new_v4().to_string();
                let custom_base = self
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| c.custom_download_dir.clone());
                let managed_path = match ensure_in_managed_folder(
                    &path,
                    &display_name,
                    &id,
                    custom_base.as_deref(),
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        self.status = Some(format!("failed to prepare app folder: {e:#}"));
                        return;
                    }
                };
                let entry = AppEntry {
                    id,
                    display_name,
                    source: AppSource::LocalOnly,
                    local_path: managed_path.clone(),
                    current_version: String::new(),
                    args,
                    icon_path,
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = write_desktop(&entry) {
                    self.status = Some(format!("desktop entry failed: {e:#}"));
                    return;
                }
                self.push_entry(entry);
            }
            SidePanelMode::LocalWithRepo => {
                if form.local_path.trim().is_empty() || form.github_url.trim().is_empty() {
                    self.status = Some("Provide AppImage path and GitHub URL".into());
                    return;
                }
                let path = PathBuf::from(form.local_path.trim());
                let repo = form.github_url.trim().to_string();
                let id = Uuid::new_v4().to_string();
                let custom_base = self
                    .config
                    .lock()
                    .ok()
                    .and_then(|c| c.custom_download_dir.clone());
                let managed_path = match ensure_in_managed_folder(
                    &path,
                    &display_name,
                    &id,
                    custom_base.as_deref(),
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        self.status = Some(format!("failed to prepare app folder: {e:#}"));
                        return;
                    }
                };
                let entry = AppEntry {
                    id: id.clone(),
                    display_name,
                    source: AppSource::LocalWithRepo { github_repo: repo },
                    local_path: managed_path.clone(),
                    current_version: String::new(),
                    args,
                    icon_path,
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = write_desktop(&entry) {
                    self.status = Some(format!("desktop entry failed: {e:#}"));
                    return;
                }
                self.push_entry(entry);
                self.send_cmd(BackendCommand::CheckUpdate { app_id: id });
            }
            SidePanelMode::GithubOnly => {
                if form.github_url.trim().is_empty() {
                    self.status = Some("Provide GitHub URL".into());
                    return;
                }
                self.start_install_from_repo(form.github_url.trim().to_string(), display_name);
            }
        }

        self.is_form_open = false;
        self.side_panel_form.clear();
    }
}

impl eframe::App for ManagerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(evt) = self.evt_rx.try_recv() {
            self.apply_event(evt);
        }

        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::NONE
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin {
                        left: crate::ui::theme::SPACE_LG as i8,
                        right: crate::ui::theme::SPACE_LG as i8,
                        top: crate::ui::theme::SPACE_MD as i8,
                        bottom: crate::ui::theme::SPACE_MD as i8,
                    }),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.horizontal(|ui| {
                        let avail = ui.available_size();
                        if avail.x.is_finite() && avail.y.is_finite() && avail.x >= 28.0 && avail.y >= 28.0 {
                            let (rect, _) =
                                ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                            if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
                                let r = rect.shrink(3.0);
                                ui.painter().rect_filled(
                                    r,
                                    egui::CornerRadius::same(6),
                                    crate::ui::theme::ACCENT,
                                );
                                ui.painter().rect_stroke(
                                    r.shrink(5.0),
                                    egui::CornerRadius::same(3),
                                    egui::Stroke::new(2.0_f32, eframe::egui::Color32::WHITE),
                                    egui::StrokeKind::Inside,
                                );
                            }
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("AppImage Hub").size(22.0).strong());
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(crate::ui::theme::primary_button("Add")).clicked() {
                            self.open_add_form();
                        }
                        let updates_count = self.updates_available_count();
                        let has_active_downloads = !self.ops.is_empty();
                        let update_all_label = if updates_count > 0 {
                            format!("Update All ({})", updates_count)
                        } else {
                            "Update All".to_string()
                        };
                        if ui
                            .add_enabled(
                                !has_active_downloads && updates_count > 0,
                                crate::ui::theme::ghost_button(update_all_label),
                            )
                            .clicked()
                        {
                            self.update_all();
                        }
                        if ui
                            .add(crate::ui::theme::secondary_button("Check All Updates"))
                            .clicked()
                        {
                            self.check_all_updates();
                        }

                        ui.add_space(4.0);
                        if ui
                            .add(crate::ui::theme::secondary_button("Browse"))
                            .on_hover_text("Choose custom download folder for GitHub installs")
                            .clicked()
                        {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                if let Ok(mut cfg) = self.config.lock() {
                                    cfg.custom_download_dir = Some(p);
                                    let _ = cfg.save();
                                }
                            }
                        }

                        let effective_download_dir = self
                            .config
                            .lock()
                            .ok()
                            .and_then(|c| c.custom_download_dir.clone())
                            .or_else(|| xdg::managed_apps_dir().ok());
                        let download_dir_text = effective_download_dir
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "unavailable".into());
                        ui.add(
                            egui::Label::new(crate::ui::theme::secondary_label(
                                download_dir_text.clone(),
                            ))
                            .truncate(),
                        )
                        .on_hover_text(download_dir_text);
                    });
                });
                let status_text = self.status.clone();
                if let Some(status) = status_text {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(status)
                                .small()
                                .color(crate::ui::theme::TEXT_SECONDARY),
                        );
                        if ui.small_button("Dismiss").clicked() {
                            self.status = None;
                        }
                    });
                }
            });

        if self.is_right_panel_open() {
            let available = ctx.screen_rect().width();
            let available = if available.is_finite() && available > 0.0 {
                available
            } else {
                800.0
            };
            let desired = (available * 0.37).clamp(360.0, 520.0);
            let min_list = 360.0;
            let panel_width = if available - desired < min_list {
                (available * 0.92).clamp(320.0, available.max(320.0))
            } else {
                desired
            };
            let panel_width = if panel_width.is_finite() && panel_width > 0.0 { panel_width } else { 360.0 };
            egui::SidePanel::right("editor_panel")
                .resizable(false)
                .exact_width(panel_width)
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    if self.add_to.is_some() {
                        crate::ui::side_panel::render_add_to(ui, self);
                    } else if self.is_form_open {
                        crate::ui::side_panel::render(ui, self);
                    }
                });
        }

        crate::ui::update_state::render(ctx, self);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin::same(crate::ui::theme::SPACE_LG as i8)),
            )
            .show(ctx, |ui| {
                let apps = self
                    .config
                    .lock()
                    .map(|c| c.applications.clone())
                    .unwrap_or_default();
                crate::ui::app_list::render(ui, &apps, self);
            });

        crate::ui::confirm_dialog::render(ctx, self);

        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
    }
}

fn parse_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

fn write_desktop(entry: &AppEntry) -> anyhow::Result<()> {
    let desktop = xdg::desktop_dir()?;
    let apps = xdg::applications_dir()?;
    desktop_entry::write_entry(entry, &desktop, &apps)
}

fn ensure_in_managed_folder(
    source_path: &std::path::Path,
    display_name: &str,
    id: &str,
    custom_base: Option<&std::path::Path>,
) -> anyhow::Result<PathBuf> {
    let folder = if let Some(base) = custom_base {
        xdg::managed_app_folder_in(base, display_name, id)?
    } else {
        xdg::managed_app_folder(display_name, id)?
    };
    let filename = source_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .filter(|s| s.to_lowercase().ends_with(".appimage"))
        .unwrap_or_else(|| format!("{}.AppImage", xdg::sanitize_name(display_name)));
    let dest = folder.join(filename);

    if source_path == dest {
        return Ok(dest);
    }

    if std::fs::rename(source_path, &dest).is_err() {
        std::fs::copy(source_path, &dest)?;
        let _ = std::fs::remove_file(source_path);
    }
    Ok(dest)
}

fn spawn_backend(
    config: Arc<Mutex<Config>>,
    mut cmd_rx: mpsc::Receiver<BackendCommand>,
    evt_tx: mpsc::Sender<BackendEvent>,
) {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build();
        let Ok(rt) = rt else {
            return;
        };
        rt.block_on(async move {
            let octo = match octocrab::Octocrab::builder().build() {
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
            let http = match reqwest::Client::builder()
                .user_agent("appimage-manager")
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

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    BackendCommand::CheckUpdate { app_id } => {
                        let cfg = config.lock().ok().map(|c| c.clone());
                        if let Some(cfg) = cfg {
                            if let Some(entry) =
                                cfg.applications.iter().find(|a| a.id == app_id).cloned()
                            {
                                let repo = match &entry.source {
                                    AppSource::LocalWithRepo { github_repo }
                                    | AppSource::GithubOnly { github_repo } => {
                                        Some(github_repo.clone())
                                    }
                                    AppSource::LocalOnly => None,
                                };
                                if let Some(repo) = repo {
                                    match github::fetch_latest_release(&octo, &repo).await {
                                        Ok(info) => {
                                            if !entry.current_version.is_empty()
                                                && info.tag == entry.current_version
                                            {
                                                let _ = evt_tx
                                                    .send(BackendEvent::UpToDate {
                                                        app_id,
                                                        tag: info.tag,
                                                    })
                                                    .await;
                                            } else {
                                                let _ = evt_tx
                                                    .send(BackendEvent::UpdateAvailable {
                                                        app_id,
                                                        tag: info.tag,
                                                        url: info.download_url,
                                                        asset_name: info.asset_name,
                                                    })
                                                    .await;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = evt_tx
                                                .send(BackendEvent::Error {
                                                    op_id: None,
                                                    app_id: Some(app_id),
                                                    message: format!("{e:#}"),
                                                })
                                                .await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    BackendCommand::DownloadAndReplace {
                        op_id,
                        app_id,
                        url,
                        tag,
                        asset_name,
                        cancel,
                    } => {
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
                            let filename = sanitize_asset_filename(&asset_name);
                            let new_target = target
                                .parent()
                                .map(|p| p.join(&filename))
                                .unwrap_or_else(|| PathBuf::from(&filename));
                            if let Some(parent) = new_target.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let tmp = downloader::temp_path_for(&new_target);
                            let res = downloader::download_to_path_with_progress(
                                &http,
                                &url,
                                &tmp,
                                cancel.clone(),
                                |p| {
                                    let _ = evt_tx.try_send(BackendEvent::DownloadProgress {
                                        op_id: op_id.clone(),
                                        app_id: Some(app_id.clone()),
                                        fraction: p.fraction,
                                        downloaded: p.downloaded,
                                        total: p.total,
                                    });
                                },
                            )
                            .await;
                            match res {
                                Ok(()) => {
                                    if let Err(e) = permissions::set_executable(&tmp) {
                                        let _ = std::fs::remove_file(&tmp);
                                        let _ = evt_tx
                                            .send(BackendEvent::Error {
                                                op_id: Some(op_id),
                                                app_id: Some(app_id),
                                                message: format!("{e:#}"),
                                            })
                                            .await;
                                        continue;
                                    }
                                    if let Err(e) = downloader::atomic_replace(&tmp, &new_target) {
                                        let _ = std::fs::remove_file(&tmp);
                                        let _ = evt_tx
                                            .send(BackendEvent::Error {
                                                op_id: Some(op_id),
                                                app_id: Some(app_id),
                                                message: format!("{e:#}"),
                                            })
                                            .await;
                                        continue;
                                    }
                                    if target != new_target {
                                        let _ = std::fs::remove_file(&target);
                                    }
                                    let mut updated_entry: Option<AppEntry> = None;
                                    if let Ok(mut cfg) = config.lock() {
                                        if let Some(entry) =
                                            cfg.applications.iter_mut().find(|a| a.id == app_id)
                                        {
                                            entry.local_path = new_target.clone();
                                            entry.current_version = tag.clone();
                                            updated_entry = Some(entry.clone());
                                        }
                                        let _ = cfg.save();
                                    }
                                    if let Some(entry) = updated_entry {
                                        if let Err(e) = write_desktop(&entry) {
                                            let _ = evt_tx
                                                .send(BackendEvent::Error {
                                                    op_id: Some(op_id.clone()),
                                                    app_id: Some(app_id.clone()),
                                                    message: format!("menu entry failed: {e:#}"),
                                                })
                                                .await;
                                        }
                                    }
                                    let _ = evt_tx
                                        .send(BackendEvent::DownloadComplete { op_id, app_id, tag })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = std::fs::remove_file(&tmp);
                                    if cancel.load(Ordering::Relaxed) {
                                        let _ = evt_tx
                                            .send(BackendEvent::DownloadCancelled {
                                                op_id,
                                                app_id: Some(app_id),
                                            })
                                            .await;
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
                    BackendCommand::InstallFromRepo {
                        op_id,
                        github_repo,
                        display_name,
                        cancel,
                    } => {
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
                        match github::fetch_latest_release(&octo, &github_repo).await {
                            Ok(info) => {
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
                                        let filename = sanitize_asset_filename(&info.asset_name);
                                        let target = folder.join(filename);
                                        let tmp = downloader::temp_path_for(&target);
                                        let res = downloader::download_to_path_with_progress(
                                            &http,
                                            &info.download_url,
                                            &tmp,
                                            cancel.clone(),
                                            |p| {
                                                let _ = evt_tx.try_send(
                                                    BackendEvent::DownloadProgress {
                                                        op_id: op_id.clone(),
                                                        app_id: Some(install_id.clone()),
                                                        fraction: p.fraction,
                                                        downloaded: p.downloaded,
                                                        total: p.total,
                                                    },
                                                );
                                            },
                                        )
                                        .await;
                                        match res {
                                            Ok(()) => {
                                                if let Err(e) = permissions::set_executable(&tmp) {
                                                    let _ = std::fs::remove_file(&tmp);
                                                    let _ = evt_tx
                                                        .send(BackendEvent::Error {
                                                            op_id: Some(op_id),
                                                            app_id: Some(install_id),
                                                            message: format!("{e:#}"),
                                                        })
                                                        .await;
                                                    continue;
                                                }
                                                if let Err(e) =
                                                    downloader::atomic_replace(&tmp, &target)
                                                {
                                                    let _ = evt_tx
                                                        .send(BackendEvent::Error {
                                                            op_id: Some(op_id),
                                                            app_id: Some(install_id),
                                                            message: format!("{e:#}"),
                                                        })
                                                        .await;
                                                    continue;
                                                }
                                                let entry = AppEntry {
                                                    id: install_id.clone(),
                                                    display_name: if display_name.trim().is_empty()
                                                    {
                                                        github_repo.clone()
                                                    } else {
                                                        display_name
                                                    },
                                                    source: AppSource::GithubOnly { github_repo },
                                                    local_path: target,
                                                    current_version: info.tag,
                                                    args: vec![],
                                                    icon_path: None,
                                                };
                                                let mut desktop_err: Option<String> = None;
                                                if let Err(e) = write_desktop(&entry) {
                                                    desktop_err = Some(format!("{e:#}"));
                                                }
                                                if let Ok(mut cfg) = config.lock() {
                                                    cfg.applications.push(entry.clone());
                                                    let _ = cfg.save();
                                                }
                                                if let Some(msg) = desktop_err {
                                                    let _ = evt_tx
                                                        .send(BackendEvent::Error {
                                                            op_id: Some(op_id.clone()),
                                                            app_id: Some(install_id.clone()),
                                                            message: format!(
                                                                "Installed, but menu entry failed: {msg}"
                                                            ),
                                                        })
                                                        .await;
                                                }
                                                let _ = evt_tx
                                                    .send(BackendEvent::AppInstalled {
                                                        op_id,
                                                        app_id: install_id,
                                                    })
                                                    .await;
                                            }
                                            Err(e) => {
                                                let _ = std::fs::remove_file(&tmp);
                                                let _ = std::fs::remove_dir_all(&folder);
                                                if cancel.load(Ordering::Relaxed) {
                                                    let _ = evt_tx
                                                        .send(BackendEvent::DownloadCancelled {
                                                            op_id,
                                                            app_id: Some(install_id),
                                                        })
                                                        .await;
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
                    BackendCommand::Launch { app_id } => {
                        let entry = config
                            .lock()
                            .ok()
                            .and_then(|c| c.applications.iter().find(|a| a.id == app_id).cloned());
                        if let Some(entry) = entry {
                            let _ = std::thread::spawn(move || {
                                let _ = launcher::launch(&entry.local_path, &entry.args);
                            });
                        }
                    }
                }
            }
        });
    });
}

fn sanitize_asset_filename(name: &str) -> String {
    let base = std::path::Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("app.AppImage");
    let safe: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    if safe.to_lowercase().ends_with(".appimage") {
        safe
    } else {
        format!("{safe}.AppImage")
    }
}
