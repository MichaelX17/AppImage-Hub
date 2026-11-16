use crate::appmeta;
use crate::config::{AppEntry, AppSource, Config};
use crate::desktop_entry;
use crate::downloader;
use crate::github;
use crate::launcher;
use crate::permissions;
use crate::ui::breakpoints::SizeClass;
use crate::xdg;
use eframe::egui::{self, RichText};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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
        cancel: Arc<AtomicBool>,
    },
    InstallFromRepo {
        op_id: String,
        github_repo: String,
        display_name: String,
        asset_name: Option<String>,
        /// Desktop category selected in the UI, already `;`-terminated.
        /// Takes precedence over anything detected inside the AppImage.
        categories: Option<String>,
        cancel: Arc<AtomicBool>,
    },
    FetchRepoAssets {
        repo: String,
    },
    Launch {
        app_id: String,
    },
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum BackendEvent {
    RepoAssetsFetched {
        repo: String,
        assets: Vec<crate::github::GithubAssetInfo>,
    },
    RepoAssetsFailed {
        repo: String,
        error: String,
    },
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppSortBy {
    #[default]
    Name,
    Size,
    DateAdded,
    Github,
    Local,
}

impl AppSortBy {
    pub const ALL: [Self; 5] = [
        Self::Name,
        Self::Size,
        Self::DateAdded,
        Self::Github,
        Self::Local,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Size => "Size",
            Self::DateAdded => "Date added",
            Self::Github => "GitHub",
            Self::Local => "Local",
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
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
        asset_name: Option<String>,
        categories: Option<String>,
    },
    DownloadAndReplace {
        app_id: String,
        url: String,
        tag: String,
    },
}

#[derive(Debug, Clone)]
pub struct UpdateCheckProgress {
    pending: HashSet<String>,
    total: usize,
    /// Smoothly animated value shown by the progress button.
    display_fraction: f32,
}

impl UpdateCheckProgress {
    /// Smooth, steady fill speed. The bar takes about three seconds to cross
    /// the button, even if all network checks finish much sooner.
    const FILL_PER_SECOND: f32 = 0.35;

    fn target_fraction(&self) -> f32 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total.saturating_sub(self.pending.len())) as f32 / self.total as f32
    }

    pub(crate) fn fraction(&self) -> f32 {
        self.display_fraction
    }

    fn tick(&mut self, dt: f32) -> bool {
        let target = self.target_fraction();
        if self.display_fraction < target {
            self.display_fraction =
                (self.display_fraction + dt * Self::FILL_PER_SECOND).min(target);
        }
        self.pending.is_empty() && self.display_fraction >= 1.0
    }
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
    pub(crate) side_panel_width: f32,
    pub(crate) show_side_panel: bool,
    /// True until the user picks where AppImages should be stored.
    first_run_setup: bool,
    /// When the last GitHub update check completed (drives the footer label).
    pub(crate) last_checked: Option<Instant>,
    /// Guards the one-shot auto-check performed on startup.
    pub(crate) auto_check_done: bool,
    pub(crate) sort_by: AppSortBy,
    pub(crate) update_check: Option<UpdateCheckProgress>,
}

pub fn setup_custom_style(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    crate::ui::theme::apply(ctx);
}

impl ManagerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        github::ensure_crypto_provider();
        setup_custom_style(&cc.egui_ctx);
        desktop_entry::ensure_hub_desktop_entry();

        let config = Arc::new(Mutex::new(Config::load().unwrap_or_else(|e| {
            eprintln!("failed to load config: {e:#}");
            Config::default()
        })));

        let side_panel_width = cc
            .storage
            .and_then(|s| s.get_string("side_panel_width"))
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(420.0)
            .clamp(360.0, 520.0);
        let show_side_panel = true;
        let is_form_open = true;
        let first_run_setup = config
            .lock()
            .map(|cfg| cfg.custom_download_dir.is_none() && cfg.applications.is_empty())
            .unwrap_or(true);
        let (cmd_tx, cmd_rx) = mpsc::channel::<BackendCommand>(64);
        let (evt_tx, evt_rx) = mpsc::channel::<BackendEvent>(256);
        spawn_backend(config.clone(), cmd_rx, evt_tx);

        let mut app = Self {
            config,
            cmd_tx,
            evt_rx,
            ops: HashMap::new(),
            failed_ops: Vec::new(),
            status: None,
            pending_delete: None,
            update_available: HashMap::new(),
            latest_versions: HashMap::new(),
            is_form_open,
            side_panel_form: crate::ui::side_panel::SidePanelFormState::default(),
            add_to: None,
            side_panel_width,
            show_side_panel,
            first_run_setup,
            last_checked: None,
            auto_check_done: false,
            sort_by: AppSortBy::Name,
            update_check: None,
        };
        if !app.first_run_setup {
            app.repair_missing_icons();
            app.normalize_managed_filenames();
            app.sync_metadata_files();
        }
        app
    }

    fn finish_first_run_setup(&mut self, apps_dir: PathBuf) {
        if let Err(e) = std::fs::create_dir_all(&apps_dir) {
            self.status = Some(format!("failed to create AppImage folder: {e:#}"));
            return;
        }
        let saved = self
            .config
            .lock()
            .map(|mut cfg| {
                cfg.custom_download_dir = Some(apps_dir.clone());
                cfg.save()
            })
            .unwrap_or_else(|_| Err(anyhow::anyhow!("config lock poisoned")));
        if let Err(e) = saved {
            self.status = Some(format!("failed to save AppImage folder: {e:#}"));
            return;
        }

        self.first_run_setup = false;
        self.repair_missing_icons();
        self.normalize_managed_filenames();
        let found = self.sync_metadata_files();
        self.status = Some(if found > 0 {
            format!(
                "AppImages will be stored in {}. Found {found} existing app(s).",
                apps_dir.display()
            )
        } else {
            format!("AppImages will be stored in {}", apps_dir.display())
        });
    }

    fn render_first_run_setup(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(crate::ui::theme::BG))
            .show(ctx, |ui| {
                ui.add_space((ui.available_height() * 0.16).clamp(32.0, 120.0));
                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../assets/logo.png"))
                            .fit_to_exact_size(egui::vec2(96.0, 96.0))
                            .corner_radius(egui::CornerRadius::same(20)),
                    );
                    ui.add_space(crate::ui::theme::SPACE_MD);
                    ui.label(
                        RichText::new("AppImage Hub")
                            .size(28.0)
                            .strong()
                            .color(crate::ui::theme::TEXT_PRIMARY),
                    );
                    ui.add_space(crate::ui::theme::SPACE_SM);
                    ui.label(
                        crate::ui::theme::secondary_label(
                            "Choose where your AppImages should be stored.",
                        )
                        .size(14.0),
                    );
                    ui.add_space(crate::ui::theme::SPACE_XL);

                    if ui
                        .add_sized([190.0, 44.0], crate::ui::theme::primary_button("Browse"))
                        .clicked()
                    {
                        if let Some(dir) = rfd::FileDialog::new()
                            .set_title("Choose AppImage storage folder")
                            .pick_folder()
                        {
                            self.finish_first_run_setup(dir);
                        }
                    }

                    ui.add_space(crate::ui::theme::SPACE_SM);
                    if ui
                        .add_sized([140.0, 38.0], crate::ui::theme::ghost_button("Default"))
                        .on_hover_text(format!("Use {}", xdg::default_managed_apps_dir().display()))
                        .clicked()
                    {
                        self.finish_first_run_setup(xdg::default_managed_apps_dir());
                    }
                });
            });
    }

    pub(crate) fn send_cmd(&mut self, cmd: BackendCommand) -> bool {
        if self.cmd_tx.try_send(cmd).is_err() {
            self.status = Some("System busy, please try again.".into());
            return false;
        }
        true
    }

    fn finish_update_check_item(&mut self, app_id: &str) {
        if let Some(progress) = self.update_check.as_mut() {
            progress.pending.remove(app_id);
        }
    }

    fn tick_update_check(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|input| input.stable_dt).min(0.1);
        let finished = self
            .update_check
            .as_mut()
            .is_some_and(|progress| progress.tick(dt));
        if finished {
            self.update_check = None;
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
            BackendEvent::RepoAssetsFailed { error, .. } => {
                self.side_panel_form.is_fetching_assets = false;
                self.side_panel_form.fetch_assets_error = Some(error);
            }
            BackendEvent::UpdateAvailable {
                app_id,
                tag,
                url,
                asset_name,
            } => {
                self.finish_update_check_item(&app_id);
                self.last_checked = Some(Instant::now());
                self.latest_versions.insert(app_id.clone(), tag.clone());
                self.update_available.insert(app_id, (tag, url, asset_name));
            }
            BackendEvent::UpToDate { app_id, tag } => {
                self.finish_update_check_item(&app_id);
                self.last_checked = Some(Instant::now());
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
                if let Some(app_id) = app_id.as_deref() {
                    self.finish_update_check_item(app_id);
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
                    at: Instant::now(),
                });
                if self.failed_ops.len() > 20 {
                    let drain = self.failed_ops.len() - 20;
                    self.failed_ops.drain(0..drain);
                }
            }
        }
    }

    pub(crate) fn start_install_from_repo(
        &mut self,
        github_repo: String,
        display_name: String,
        asset_name: Option<String>,
        categories: Option<String>,
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
                }),
            },
        );
        self.send_cmd(BackendCommand::InstallFromRepo {
            op_id,
            github_repo,
            display_name,
            asset_name,
            categories,
            cancel,
        });
    }

    pub(crate) fn start_update_download(&mut self, app_id: String, url: String, tag: String) {
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
                }),
            },
        );
        self.send_cmd(BackendCommand::DownloadAndReplace {
            op_id,
            app_id,
            url,
            tag,
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
            appmeta::remove_for_entry(&entry);
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
        self.show_side_panel = true;
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

        self.add_to = None;
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
            startup_notify: entry.startup_notify,
            mode,
            local_path: entry.local_path.display().to_string(),
            icon_path: entry
                .icon_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            github_url,
            launch_args: entry.args.join(" "),
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
                } => {
                    self.start_install_from_repo(github_repo, display_name, asset_name, categories)
                }
                RetryAction::DownloadAndReplace { app_id, url, tag } => {
                    self.start_update_download(app_id, url, tag)
                }
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
        self.show_side_panel = true;
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

    fn render_top_bar(&mut self, ui: &mut egui::Ui, size_class: SizeClass) {
        let full_brand = "AppImage Hub";
        let show_actions_inline = matches!(size_class, SizeClass::Large);
        let tight = matches!(size_class, SizeClass::Tight);

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

        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(egui::include_image!("../assets/logo.png"))
                    .fit_to_exact_size(egui::vec2(28.0, 28.0))
                    .corner_radius(egui::CornerRadius::same(6)),
            );
            ui.add_space(6.0);
            if tight {
                ui.label(
                    RichText::new("AIM")
                        .size(22.0)
                        .strong()
                        .color(crate::ui::theme::TEXT_PRIMARY),
                )
                .on_hover_text(full_brand);
            } else {
                ui.label(
                    RichText::new(full_brand)
                        .size(22.0)
                        .strong()
                        .color(crate::ui::theme::TEXT_PRIMARY),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.render_toolbar_actions(ui, show_actions_inline, &download_dir_text);
            });
        });

        // Breadcrumb row: painted folder icon + shortened download path.
        if !tight {
            ui.add_space(2.0);
            crate::ui::theme::breadcrumb(ui, &download_dir_text, crate::ui::theme::ACCENT_HOVER)
                .on_hover_text(format!("Download folder: {download_dir_text}"));
        }
    }

    fn render_toolbar_actions(&mut self, ui: &mut egui::Ui, inline: bool, download_dir_text: &str) {
        if inline {
            // Primary "Add" action: neon pill with a hover glow.
            let add = ui.add(crate::ui::theme::add_button("+ Add AppImage"));
            if add.hovered() {
                crate::ui::theme::draw_glow(
                    ui,
                    add.rect,
                    crate::ui::theme::RADIUS_PILL,
                    crate::ui::theme::ACCENT,
                );
            }
            if add.clicked() {
                self.open_add_form();
            }

            let updates_count = self.updates_available_count();
            let has_active_downloads = !self.ops.is_empty();
            let update_all_label = if updates_count > 0 {
                format!("Update All ({})", updates_count)
            } else {
                "Update All".to_string()
            };
            let update_all_clicked = ui
                .add_enabled(
                    !has_active_downloads && updates_count > 0,
                    crate::ui::theme::ghost_button(update_all_label),
                )
                .on_disabled_hover_text(if has_active_downloads {
                    "Wait for active downloads to finish"
                } else {
                    "No updates available"
                })
                .clicked();
            if update_all_clicked {
                self.update_all();
            }

            let check_progress = self
                .update_check
                .as_ref()
                .map(UpdateCheckProgress::fraction);
            let checking = check_progress.is_some();
            let check_clicked = check_updates_button(ui, check_progress, !checking).clicked();
            if check_clicked && !checking {
                self.check_all_updates();
            }

            ui.add_space(4.0);
            let browse_clicked = ui
                .add(crate::ui::theme::secondary_button("Browse"))
                .on_hover_text("Choose custom download folder for GitHub installs")
                .clicked();
            if browse_clicked {
                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                    if let Ok(mut cfg) = self.config.lock() {
                        cfg.custom_download_dir = Some(p);
                        let _ = cfg.save();
                    }
                    let found = self.sync_metadata_files();
                    if found > 0 {
                        self.status = Some(format!(
                            "Found {found} existing app(s) in the selected folder."
                        ));
                    }
                }
            }
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(crate::ui::theme::add_button("+ Add")).clicked() {
                    self.open_add_form();
                }
                ui.menu_button("Actions", |ui| {
                    ui.label(crate::ui::theme::secondary_label("Download folder:").small());
                    ui.add(
                        egui::Label::new(crate::ui::theme::muted_label(download_dir_text))
                            .truncate(),
                    )
                    .on_hover_text(download_dir_text);
                    ui.separator();
                    let browse_clicked = ui.button("Browse download folder").clicked();
                    if browse_clicked {
                        if let Some(p) = rfd::FileDialog::new().pick_folder() {
                            if let Ok(mut cfg) = self.config.lock() {
                                cfg.custom_download_dir = Some(p);
                                let _ = cfg.save();
                            }
                            let found = self.sync_metadata_files();
                            if found > 0 {
                                self.status = Some(format!(
                                    "Found {found} existing app(s) in the selected folder."
                                ));
                            }
                        }
                        ui.close_menu();
                    }

                    let checking = self.update_check.is_some();
                    let check_label = if checking {
                        "Checking updates…"
                    } else {
                        "Check All Updates"
                    };
                    let check_clicked = ui
                        .add_enabled(!checking, egui::Button::new(check_label))
                        .clicked();
                    if check_clicked {
                        self.check_all_updates();
                        ui.close_menu();
                    }

                    let updates_count = self.updates_available_count();
                    let has_active_downloads = !self.ops.is_empty();
                    let update_all_label = if updates_count > 0 {
                        format!("Update All ({})", updates_count)
                    } else {
                        "Update All".to_string()
                    };
                    let update_all_clicked = ui
                        .add_enabled(
                            !has_active_downloads && updates_count > 0,
                            egui::Button::new(update_all_label),
                        )
                        .clicked();
                    if update_all_clicked {
                        self.update_all();
                        ui.close_menu();
                    }
                });
            });
        }
    }

    pub(crate) fn check_all_updates(&mut self) {
        if self.update_check.is_some() {
            return;
        }

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

        if ids.is_empty() {
            self.status = Some("No GitHub-tracked apps to check.".into());
            return;
        }

        let mut pending = HashSet::new();
        for app_id in ids {
            if self.send_cmd(BackendCommand::CheckUpdate {
                app_id: app_id.clone(),
            }) {
                pending.insert(app_id);
            }
        }
        if !pending.is_empty() {
            self.update_check = Some(UpdateCheckProgress {
                total: pending.len(),
                pending,
                display_fraction: 0.0,
            });
        }
    }

    pub(crate) fn update_all(&mut self) {
        if !self.ops.is_empty() {
            return;
        }
        let updates: Vec<(String, String, String)> = self
            .update_available
            .iter()
            .map(|(app_id, (tag, url, _asset))| (app_id.clone(), tag.clone(), url.clone()))
            .collect();
        for (app_id, tag, url) in updates {
            self.start_update_download(app_id, url, tag);
        }
    }

    /// Whether automatic update checks on startup are enabled (persisted in config).
    pub(crate) fn auto_check_enabled(&self) -> bool {
        self.config
            .lock()
            .map(|c| c.auto_check_updates)
            .unwrap_or(true)
    }

    /// Persist the auto-check toggle (a failed config save must never break the UI).
    pub(crate) fn set_auto_check_enabled(&mut self, enabled: bool) {
        if let Ok(mut cfg) = self.config.lock() {
            if cfg.auto_check_updates != enabled {
                cfg.auto_check_updates = enabled;
                let _ = cfg.save();
            }
        }
    }

    pub(crate) fn auto_populate_from_appimage(&mut self, appimage_path: &Path) {
        // A sidecar written beside the AppImage is authoritative and avoids
        // re-executing the AppImage just to read its metadata again.
        if let Some(meta) = appmeta::read_for_appimage(appimage_path) {
            self.apply_sidecar_metadata(appimage_path, &meta);
            return;
        }
        if let Ok(meta) = crate::appimage::extract_metadata(appimage_path, None) {
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
                    if meta.is_custom_category {
                        self.side_panel_form.category = "Custom".to_string();
                        self.side_panel_form.custom_category = cat;
                    } else {
                        self.side_panel_form.category = cat;
                    }
                }
            }

            if let Some(notify) = meta.startup_notify {
                self.side_panel_form.startup_notify = notify;
            }

            if let Some(icon_p) = meta.icon_path {
                self.side_panel_form.icon_path = icon_p.display().to_string();
            }
        }
    }

    /// Fills the Add form from the JSON sidecar stored beside an AppImage.
    fn apply_sidecar_metadata(&mut self, appimage_path: &Path, meta: &appmeta::AppMetadataFile) {
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
                let cat = raw.trim().trim_end_matches(';').trim();
                if !cat.is_empty() {
                    if let Some((_, value)) = crate::ui::side_panel::KDE_CATEGORIES
                        .iter()
                        .find(|(_, value)| *value == cat)
                    {
                        self.side_panel_form.category = (*value).to_string();
                        self.side_panel_form.custom_category.clear();
                    } else {
                        self.side_panel_form.category = "Custom".to_string();
                        self.side_panel_form.custom_category = cat.to_string();
                    }
                }
            }
        }

        self.side_panel_form.startup_notify = meta.startup_notify;

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
                        } else if let Ok(extracted) =
                            crate::appimage::extract_icon(&app.local_path, folder)
                        {
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
            let _ = write_desktop(&app);
        }
    }

    /// Renames already-managed binaries to the clean `<name>.appimage` form,
    /// so entries added before this naming scheme also end up clean.
    /// Idempotent: skips missing files and existing target names.
    pub(crate) fn normalize_managed_filenames(&mut self) {
        let custom_base = self
            .config
            .lock()
            .ok()
            .and_then(|c| c.custom_download_dir.clone());
        let default_base = Some(xdg::default_managed_apps_dir());

        let mut moves: Vec<(String, PathBuf, PathBuf)> = Vec::new();
        if let Ok(cfg) = self.config.lock() {
            for app in &cfg.applications {
                if !app.local_path.exists() {
                    continue;
                }
                let inside = default_base
                    .as_ref()
                    .map(|base| app.local_path.starts_with(base))
                    .unwrap_or(false)
                    || custom_base
                        .as_ref()
                        .map(|base| app.local_path.starts_with(base))
                        .unwrap_or(false);
                if !inside {
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
                    rewritten.push(entry.clone());
                }
            }
            let _ = cfg.save();
        }
        for entry in &rewritten {
            let _ = write_desktop(entry);
            let _ = appmeta::write_for_entry(entry);
        }
    }

    fn app_storage_dir(&self) -> PathBuf {
        self.config
            .lock()
            .ok()
            .and_then(|cfg| cfg.custom_download_dir.clone())
            .unwrap_or_else(xdg::default_managed_apps_dir)
    }

    /// Keeps `<name>.json` sidecars in sync with the config, and scans the
    /// selected storage folder for existing AppImages that are missing from
    /// `config.toml` (for example after choosing a folder with previous apps).
    pub(crate) fn sync_metadata_files(&mut self) -> usize {
        let mut known_ids: HashSet<String> = HashSet::new();
        let mut known_paths: HashSet<PathBuf> = HashSet::new();
        let mut missing: Vec<AppEntry> = Vec::new();

        if let Ok(cfg) = self.config.lock() {
            for app in &cfg.applications {
                known_ids.insert(app.id.clone());
                known_paths.insert(app.local_path.clone());
                if app.local_path.exists() && appmeta::read_for_appimage(&app.local_path).is_none()
                {
                    missing.push(app.clone());
                }
            }
        }
        for app in missing {
            let _ = appmeta::write_for_entry(&app);
        }

        let base = self.app_storage_dir();
        let mut appimages = Vec::new();
        collect_appimages(&base, &mut appimages, 0);

        let mut adopted: Vec<AppEntry> = Vec::new();
        for appimage in appimages {
            if known_paths.contains(&appimage) {
                continue;
            }
            let entry = if let Some(meta) = appmeta::read_for_appimage(&appimage) {
                meta.to_entry(appimage.clone())
            } else {
                app_entry_from_appimage(&appimage)
            };
            if known_ids.contains(&entry.id) {
                continue;
            }
            known_ids.insert(entry.id.clone());
            known_paths.insert(appimage);
            adopted.push(entry);
        }

        if adopted.is_empty() {
            return 0;
        }
        if let Ok(mut cfg) = self.config.lock() {
            cfg.applications.extend(adopted.iter().cloned());
            let _ = cfg.save();
        }
        for entry in &adopted {
            let _ = write_desktop(entry);
            let _ = appmeta::write_for_entry(entry);
        }
        adopted.len()
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
        let categories = {
            let cat = form.effective_category();
            if cat.is_empty() {
                Some("Utility;".to_string())
            } else if cat.ends_with(';') {
                Some(cat)
            } else {
                Some(format!("{cat};"))
            }
        };
        let startup_notify = form.startup_notify;
        let asset_preference =
            if form.asset_selection_mode == crate::ui::side_panel::AssetSelectionMode::Manual {
                form.manual_asset.clone()
            } else {
                None
            };

        if let Some(app_id) = form.editing_app_id.clone() {
            let mut old_entry: Option<AppEntry> = None;
            let mut updated: Option<AppEntry> = None;
            if let Ok(mut cfg) = self.config.lock() {
                if let Some(e) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
                    old_entry = Some(e.clone());
                    e.display_name = display_name;
                    e.generic_name = generic_name;
                    e.comment = comment;
                    e.categories = categories;
                    e.startup_notify = startup_notify;
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
                let _ = appmeta::write_for_entry(&e);
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
                let folder = managed_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""));
                let final_icon_path = resolve_final_icon(icon_path, &managed_path, folder);

                let entry = AppEntry {
                    id,
                    display_name,
                    generic_name,
                    comment,
                    categories,
                    startup_notify,
                    source: AppSource::LocalOnly,
                    local_path: managed_path.clone(),
                    current_version: String::new(),
                    args,
                    icon_path: final_icon_path,
                    asset_preference: None,
                    added_at: Some(now_unix()),
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = write_desktop(&entry) {
                    self.status = Some(format!("desktop entry failed: {e:#}"));
                    return;
                }
                // Record the metadata next to the AppImage itself.
                let _ = appmeta::write_for_entry(&entry);
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
                let folder = managed_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""));
                let final_icon_path = resolve_final_icon(icon_path, &managed_path, folder);

                let entry = AppEntry {
                    id: id.clone(),
                    display_name,
                    generic_name,
                    comment,
                    categories,
                    startup_notify,
                    source: AppSource::LocalWithRepo { github_repo: repo },
                    local_path: managed_path.clone(),
                    current_version: String::new(),
                    args,
                    icon_path: final_icon_path,
                    asset_preference,
                    added_at: Some(now_unix()),
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = write_desktop(&entry) {
                    self.status = Some(format!("desktop entry failed: {e:#}"));
                    return;
                }
                let _ = appmeta::write_for_entry(&entry);
                self.push_entry(entry);
                self.send_cmd(BackendCommand::CheckUpdate { app_id: id });
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
                );
            }
        }

        self.is_form_open = false;
        self.side_panel_form.clear();
    }
}

impl eframe::App for ManagerApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("side_panel_width", self.side_panel_width.to_string());
        storage.set_string("show_side_panel", self.show_side_panel.to_string());
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.first_run_setup {
            self.render_first_run_setup(ctx);
            return;
        }
        let size_class = SizeClass::from_width(ctx.screen_rect().width());
        while let Ok(evt) = self.evt_rx.try_recv() {
            self.apply_event(evt);
        }
        self.tick_update_check(ctx);

        // One-shot auto-check on startup, honouring the persisted preference.
        if !self.auto_check_done {
            self.auto_check_done = true;
            if self.auto_check_enabled() {
                self.check_all_updates();
            }
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
                    })
                    .stroke(egui::Stroke::new(1.0_f32, crate::ui::theme::BORDER)),
            )
            .show(ctx, |ui| {
                self.render_top_bar(ui, size_class);
                let status_text = self.status.clone();
                if let Some(status) = status_text {
                    ui.add_space(crate::ui::theme::SPACE_XS);
                    ui.horizontal_wrapped(|ui| {
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

        let panel_open = self.is_right_panel_open() && self.show_side_panel;
        let use_side_panel =
            panel_open && matches!(size_class, SizeClass::Large | SizeClass::Medium);

        if use_side_panel {
            let total = ctx.screen_rect().width();
            let total = if total.is_finite() && total > 0.0 {
                total
            } else {
                800.0
            };
            let desired = (total * 0.37).clamp(360.0, 520.0);
            let max_allowed = (total - 420.0).max(320.0);
            let max_panel = desired.min(max_allowed).max(320.0).clamp(320.0, 520.0);
            let min_panel = 320.0_f32.min(max_panel);
            let default_width = self.side_panel_width.clamp(min_panel, max_panel);
            let panel = egui::SidePanel::right("editor_panel")
                .resizable(true)
                .default_width(default_width)
                .width_range(min_panel..=max_panel)
                .frame(egui::Frame::NONE.fill(crate::ui::theme::BG))
                .show(ctx, |ui| {
                    if self.add_to.is_some() {
                        crate::ui::side_panel::render_add_to(ui, self, size_class);
                    } else if self.is_form_open {
                        crate::ui::side_panel::render(ui, self, size_class);
                    }
                });
            let actual = panel.response.rect.width();
            if actual.is_finite() && actual >= min_panel {
                self.side_panel_width = actual.clamp(min_panel, 520.0);
            }
        }

        crate::ui::update_state::render(ctx, self, size_class);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(crate::ui::theme::BG)
                    .inner_margin(egui::Margin::same(crate::ui::theme::SPACE_LG as i8)),
            )
            .show(ctx, |ui| {
                if panel_open && !use_side_panel {
                    egui::ScrollArea::vertical()
                        .id_salt("overlay_panel_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let max_w = ui.available_width().min(640.0);
                            ui.set_max_width(max_w);
                            ui.horizontal(|ui| {
                                if ui
                                    .add(crate::ui::theme::secondary_button("← Back to list"))
                                    .clicked()
                                {
                                    self.show_side_panel = false;
                                }
                            });
                            ui.add_space(crate::ui::theme::SPACE_SM);
                            egui::Frame::NONE
                                .stroke(egui::Stroke::new(1.0_f32, crate::ui::theme::BORDER))
                                .corner_radius(egui::CornerRadius::same(
                                    crate::ui::theme::RADIUS_LG,
                                ))
                                .show(ui, |ui| {
                                    if self.add_to.is_some() {
                                        crate::ui::side_panel::render_add_to(ui, self, size_class);
                                    } else if self.is_form_open {
                                        crate::ui::side_panel::render(ui, self, size_class);
                                    }
                                });
                        });
                } else {
                    let apps = self
                        .config
                        .lock()
                        .map(|c| c.applications.clone())
                        .unwrap_or_default();
                    crate::ui::app_list::render(ui, &apps, self, size_class);
                }
            });

        crate::ui::confirm_dialog::render(ctx, self);

        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if self.update_check.is_some() {
            // Keep the progress animation at display refresh rate.
            ctx.request_repaint();
        }
    }
}

fn check_updates_button(ui: &mut egui::Ui, progress: Option<f32>, enabled: bool) -> egui::Response {
    let text = match progress {
        Some(fraction) => format!(
            "Checking updates… {}%",
            (fraction.clamp(0.0, 1.0) * 100.0).round() as i32
        ),
        None => "Check All Updates".to_string(),
    };
    let desired = egui::vec2(176.0, ui.spacing().interact_size.y.max(32.0));
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(desired, sense);

    if enabled && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let fill = if enabled && response.hovered() {
        crate::ui::theme::SURFACE_HOVER
    } else {
        crate::ui::theme::NEUTRAL
    };
    let stroke = if enabled && response.hovered() {
        crate::ui::theme::ACCENT_HOVER
    } else {
        crate::ui::theme::with_alpha(crate::ui::theme::BORDER, 180)
    };
    let radius = egui::CornerRadius::same(crate::ui::theme::RADIUS_PILL);

    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0_f32, stroke),
        egui::StrokeKind::Inside,
    );

    if let Some(fraction) = progress {
        let fraction = fraction.clamp(0.0, 1.0);
        if fraction > 0.0 {
            let progress_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.left() + rect.width() * fraction, rect.bottom()),
            );
            ui.painter().with_clip_rect(progress_rect).rect_filled(
                rect,
                radius,
                crate::ui::theme::ACCENT,
            );
        }
    }

    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(13.0),
        crate::ui::theme::TEXT_PRIMARY,
    );

    response
}

fn parse_args(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

fn resolve_final_icon(
    user_icon_path: Option<PathBuf>,
    managed_appimage: &std::path::Path,
    target_folder: &std::path::Path,
) -> Option<PathBuf> {
    if let Some(user_icon) = user_icon_path {
        if user_icon.exists() {
            if user_icon.parent() == Some(target_folder) {
                return Some(user_icon);
            }
            let ext = user_icon
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png");
            let dest = target_folder.join(format!("icon.{}", ext));
            if std::fs::copy(&user_icon, &dest).is_ok() {
                return Some(dest);
            }
            return Some(user_icon);
        }
    }

    let p_png = target_folder.join("icon.png");
    if p_png.exists() {
        return Some(p_png);
    }
    let p_svg = target_folder.join("icon.svg");
    if p_svg.exists() {
        return Some(p_svg);
    }

    crate::appimage::extract_icon(managed_appimage, target_folder).ok()
}

fn collect_appimages(dir: &Path, out: &mut Vec<PathBuf>, depth: u8) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_appimages(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("appimage"))
        {
            out.push(path);
        }
    }
}

fn app_entry_from_appimage(path: &Path) -> AppEntry {
    let folder = path.parent().unwrap_or_else(|| Path::new(""));
    let display_name = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "AppImage".to_string());
    let icon_path = [folder.join("icon.png"), folder.join("icon.svg")]
        .into_iter()
        .find(|icon| icon.exists());

    AppEntry {
        id: Uuid::new_v4().to_string(),
        display_name,
        generic_name: None,
        comment: None,
        categories: None,
        startup_notify: true,
        source: AppSource::LocalOnly,
        local_path: path.to_path_buf(),
        current_version: String::new(),
        args: Vec::new(),
        icon_path,
        asset_preference: None,
        added_at: Some(
            path.metadata()
                .ok()
                .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_secs() as i64)
                .unwrap_or_else(now_unix),
        ),
    }
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
    // Always store under the clean, user-typed name (`example.appimage`)
    // rather than the upstream asset name (`example_v2.4_x86_64.AppImage`).
    let fallback = source_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let filename = xdg::managed_appimage_filename(display_name, &fallback);
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

/// Clean target path for a binary that already lives in a managed app folder.
/// Returns `None` when the name is already correct.
fn clean_managed_path(local_path: &Path, display_name: &str) -> Option<PathBuf> {
    let parent = local_path.parent()?;
    let desired = parent.join(xdg::managed_appimage_filename(display_name, "app"));
    if desired == local_path {
        None
    } else {
        Some(desired)
    }
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
                                    match github::fetch_latest_release_with_selector(
                                        &octo,
                                        &repo,
                                        entry.asset_preference.as_deref(),
                                    )
                                    .await
                                    {
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
                        cancel,
                        ..
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
                            // Keep the clean, stable file name: the new build is
                            // swapped in place instead of adopting the upstream
                            // asset name.
                            if let Some(parent) = target.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let tmp = downloader::temp_path_for(&target);
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
                                    if let Err(e) = downloader::atomic_replace(&tmp, &target) {
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
                                    let mut updated_entry: Option<AppEntry> = None;
                                    if let Ok(mut cfg) = config.lock() {
                                        if let Some(entry) =
                                            cfg.applications.iter_mut().find(|a| a.id == app_id)
                                        {
                                            entry.current_version = tag.clone();
                                            if entry
                                                .icon_path
                                                .as_ref()
                                                .map(|p| !p.exists())
                                                .unwrap_or(true)
                                            {
                                                if let Some(folder) = target.parent() {
                                                    if let Ok(extracted) =
                                                        crate::appimage::extract_icon(&target, folder)
                                                    {
                                                        entry.icon_path = Some(extracted);
                                                    }
                                                }
                                            }
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
                                        let _ = appmeta::write_for_entry(&entry);
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
                        asset_name,
                        categories,
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
                        match github::fetch_latest_release_with_selector(
                            &octo,
                            &github_repo,
                            asset_name.as_deref(),
                        )
                        .await
                        {
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
                                        let filename = xdg::managed_appimage_filename(&hint, "app");
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
                                                let (extracted_icon, app_meta) = {
                                                    let icon = crate::appimage::extract_icon(&target, &folder).ok();
                                                    let meta = crate::appimage::extract_metadata(&target, None).ok();
                                                    (icon, meta)
                                                };

                                                let entry = AppEntry {
                                                    id: install_id.clone(),
                                                    display_name: if display_name.trim().is_empty()
                                                    {
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
                                                    comment: app_meta
                                                        .as_ref()
                                                        .and_then(|m| m.comment.clone()),
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
                                                };
                                                let mut desktop_err: Option<String> = None;
                                                if let Err(e) = write_desktop(&entry) {
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
                    BackendCommand::FetchRepoAssets { repo } => {
                        match github::fetch_available_appimages(&octo, &repo).await {
                            Ok(assets) => {
                                let _ = evt_tx
                                    .send(BackendEvent::RepoAssetsFetched { repo, assets })
                                    .await;
                            }
                            Err(e) => {
                                let _ = evt_tx
                                    .send(BackendEvent::RepoAssetsFailed {
                                        repo,
                                        error: format!("{e:#}"),
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

/// Guarantees a single trailing `;` on an XDG `Categories=` value.
fn ensure_trailing_semicolon(category: &str) -> String {
    let cat = category.trim();
    if cat.ends_with(';') {
        cat.to_string()
    } else {
        format!("{cat};")
    }
}

/// Resolves the `Categories=` value written to the menu entry.
///
/// The category the user picked in the Add/Edit form always wins; the
/// AppImage's own metadata is only a fallback, and `Utility;` is the last
/// resort so a valid `.desktop` file is always produced.
fn resolve_categories(
    user_selected: Option<String>,
    meta: Option<&crate::appimage::AppImageMetadata>,
) -> Option<String> {
    if let Some(cat) = user_selected
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        return Some(ensure_trailing_semicolon(cat));
    }

    if let Some(meta) = meta {
        // Prefer the KDE-mapped primary category (e.g. "AudioVideo") over the
        // raw XDG list, since Plasma places apps by the first known category.
        if let Some(mapped) = meta
            .matched_category
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            return Some(ensure_trailing_semicolon(mapped));
        }
        if let Some(raw) = meta
            .categories
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            return Some(ensure_trailing_semicolon(raw));
        }
    }

    Some("Utility;".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appimage::AppImageMetadata;

    fn meta_with(mapped: Option<&str>, raw: Option<&str>) -> AppImageMetadata {
        AppImageMetadata {
            matched_category: mapped.map(str::to_string),
            categories: raw.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn user_selected_category_wins_over_metadata() {
        let meta = meta_with(Some("Utility"), Some("Utility;"));
        let resolved = resolve_categories(Some("Graphics;".into()), Some(&meta));
        assert_eq!(resolved.as_deref(), Some("Graphics;"));
    }

    #[test]
    fn user_selected_category_gets_semicolon_appended() {
        let resolved = resolve_categories(Some("Development".into()), None);
        assert_eq!(resolved.as_deref(), Some("Development;"));
    }

    #[test]
    fn falls_back_to_mapped_metadata_category() {
        let meta = meta_with(Some("AudioVideo"), Some("AudioVideo;Player;"));
        let resolved = resolve_categories(None, Some(&meta));
        assert_eq!(resolved.as_deref(), Some("AudioVideo;"));
    }

    #[test]
    fn falls_back_to_raw_metadata_category() {
        let meta = meta_with(None, Some("Graphics;"));
        let resolved = resolve_categories(None, Some(&meta));
        assert_eq!(resolved.as_deref(), Some("Graphics;"));
    }

    #[test]
    fn blank_user_selection_falls_through_to_utility() {
        let resolved = resolve_categories(Some("   ".into()), None);
        assert_eq!(resolved.as_deref(), Some("Utility;"));
    }

    #[test]
    fn trailing_semicolon_is_not_duplicated() {
        assert_eq!(ensure_trailing_semicolon("Game;"), "Game;");
        assert_eq!(ensure_trailing_semicolon(" Game "), "Game;");
    }

    #[test]
    fn update_check_progress_animates_slowly_toward_target() {
        let mut pending = HashSet::new();
        pending.insert("a".to_string());
        pending.insert("b".to_string());
        let mut progress = UpdateCheckProgress {
            pending,
            total: 2,
            display_fraction: 0.0,
        };

        progress.pending.remove("a");
        progress.tick(1.0);
        assert!((progress.fraction() - UpdateCheckProgress::FILL_PER_SECOND).abs() < f32::EPSILON);

        progress.pending.remove("b");
        progress.tick(1.0);
        assert!(
            (progress.fraction() - 2.0 * UpdateCheckProgress::FILL_PER_SECOND).abs() < f32::EPSILON
        );
        assert!(!progress.tick(0.5));
        assert!(progress.tick(0.5));
        assert!((progress.fraction() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn clean_managed_path_rewrites_upstream_names() {
        let path = PathBuf::from("/apps/example-1234/Obsidian-1.8.7-x86_64.AppImage");
        assert_eq!(
            clean_managed_path(&path, "Obsidian"),
            Some(PathBuf::from("/apps/example-1234/obsidian.appimage"))
        );
    }

    #[test]
    fn clean_managed_path_is_noop_when_already_clean() {
        let path = PathBuf::from("/apps/example-1234/obsidian.appimage");
        assert_eq!(clean_managed_path(&path, "Obsidian"), None);
    }
}
