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
    CheckHubUpdate,
    SelfUpdate {
        url: String,
        target: PathBuf,
        cancel: Arc<AtomicBool>,
    },
    DownloadAndReplace {
        op_id: String,
        app_id: String,
        url: String,
        tag: String,
        commit_hash: String,
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
        desktop_shortcut: bool,
        menu_entry: bool,
        mime_types: Option<String>,
        exec_arg: Option<String>,
        cancel: Arc<AtomicBool>,
    },
    FetchRepoAssets {
        repo: String,
    },
    SetGithubToken(Option<String>),
    /// Re-baseline the installed file as accepted, clearing a "changed on
    /// disk" warning without downloading anything.
    AcceptCurrentFile {
        app_id: String,
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
        commit_hash: String,
    },
    UpToDate {
        app_id: String,
        tag: String,
    },
    HubUpdateAvailable {
        url: String,
    },
    HubUpToDate,
    HubUpdateProgress {
        fraction: f32,
        downloaded: u64,
        total: Option<u64>,
    },
    HubUpdateReady,
    HubUpdateFailed {
        message: String,
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
    /// Result of manually accepting the current AppImage file.
    FileAccepted {
        app_id: String,
        error: Option<String>,
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

/// Update-check cooldown without a GitHub token (60 requests/hour limit).
const UPDATE_CHECK_COOLDOWN_SECS: u64 = 5 * 60;
/// Update-check cooldown with a GitHub token (5,000 requests/hour limit).
const AUTHENTICATED_UPDATE_CHECK_COOLDOWN_SECS: u64 = 2 * 60;

fn token_is_present(token: Option<&str>) -> bool {
    token.is_some_and(|token| !token.trim().is_empty())
}

fn update_check_cooldown_secs(has_github_token: bool) -> u64 {
    if has_github_token {
        AUTHENTICATED_UPDATE_CHECK_COOLDOWN_SECS
    } else {
        UPDATE_CHECK_COOLDOWN_SECS
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn update_check_cooldown_remaining(
    last_check: Option<i64>,
    now: i64,
    cooldown_secs: u64,
) -> Option<u64> {
    let last = last_check?;
    let elapsed = now.saturating_sub(last).max(0) as u64;
    (elapsed < cooldown_secs).then(|| cooldown_secs - elapsed)
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
        desktop_shortcut: bool,
        menu_entry: bool,
        mime_types: Option<String>,
        exec_arg: Option<String>,
    },
    DownloadAndReplace {
        app_id: String,
        url: String,
        tag: String,
        commit_hash: String,
    },
}

#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub tag: String,
    pub url: String,
    pub commit_hash: String,
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
    Downloading,
    Failed(&'a FailedOp),
    Idle,
}

/// Local file problem detected without contacting GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalFileIssue {
    /// The AppImage exists but no longer matches the recorded fingerprint.
    Changed,
    /// The configured AppImage path does not exist.
    Missing,
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
    pub(crate) update_available: HashMap<String, AvailableUpdate>,
    pub(crate) latest_versions: HashMap<String, String>,
    pub(crate) is_form_open: bool,
    pub(crate) side_panel_form: crate::ui::side_panel::SidePanelFormState,
    pub(crate) add_to: Option<AddToState>,
    pub(crate) side_panel_width: f32,
    pub(crate) show_side_panel: bool,
    pub(crate) show_settings: bool,
    pub(crate) settings_form: crate::ui::settings_panel::SettingsFormState,
    /// True until the user picks where AppImages should be stored.
    first_run_setup: bool,
    /// When the last GitHub update check completed (drives the footer label).
    pub(crate) last_checked: Option<Instant>,
    /// Guards the one-shot auto-check performed on startup.
    pub(crate) auto_check_done: bool,
    pub(crate) sort_by: AppSortBy,
    pub(crate) update_check: Option<UpdateCheckProgress>,
    pub(crate) hub_update_url: Option<String>,
    pub(crate) hub_update_progress: Option<f32>,
    hub_update_check_started: bool,
    /// Whether a GitHub token is configured. Authenticated checks may run on a
    /// shorter cooldown than anonymous ones.
    has_github_token: bool,
    /// Number of footer activity items rendered on the previous frame.
    /// egui caches bottom-panel heights for one frame, so any change in the
    /// progress list needs a follow-up repaint to re-anchor the footer at the
    /// window bottom (otherwise a blank gap is left until the next input).
    activity_footer_items: usize,
    /// Local (offline) per-app file problems, refreshed periodically.
    pub(crate) file_issues: HashMap<String, LocalFileIssue>,
    last_file_scan: Option<Instant>,
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

        if let Ok(cfg) = config.lock() {
            desktop_entry::sync_hub_shortcuts(
                cfg.hub_start_menu_shortcut,
                cfg.hub_desktop_shortcut,
                &cfg.hub_start_menu_category,
            );
        }

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
        let github_token = crate::secrets::load_github_token().or_else(|| {
            std::env::var("GITHUB_TOKEN")
                .ok()
                .or_else(|| std::env::var("GH_TOKEN").ok())
                .filter(|token| !token.trim().is_empty())
        });
        let has_github_token = token_is_present(github_token.as_deref());
        spawn_backend(config.clone(), cmd_rx, evt_tx, github_token);

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
            show_settings: false,
            settings_form: crate::ui::settings_panel::SettingsFormState::default(),
            first_run_setup,
            last_checked: None,
            auto_check_done: false,
            sort_by: AppSortBy::Name,
            update_check: None,
            hub_update_url: None,
            hub_update_progress: None,
            hub_update_check_started: false,
            has_github_token,
            activity_footer_items: 0,
            file_issues: HashMap::new(),
            last_file_scan: None,
        };
        if !app.first_run_setup {
            app.repair_missing_icons();
            app.normalize_managed_filenames();
            app.sync_metadata_files();
        }
        app.refresh_file_issues();
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
                commit_hash,
            } => {
                self.finish_update_check_item(&app_id);
                self.last_checked = Some(Instant::now());
                self.latest_versions.insert(app_id.clone(), tag.clone());
                self.update_available.insert(
                    app_id,
                    AvailableUpdate {
                        tag,
                        url,
                        commit_hash,
                    },
                );
            }
            BackendEvent::UpToDate { app_id, tag } => {
                self.finish_update_check_item(&app_id);
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
            BackendEvent::HubUpdateProgress { fraction, .. } => {
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
        self.send_cmd(BackendCommand::InstallFromRepo {
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
        });
    }

    pub(crate) fn start_update_download(
        &mut self,
        app_id: String,
        url: String,
        tag: String,
        commit_hash: String,
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
                }),
            },
        );
        self.send_cmd(BackendCommand::DownloadAndReplace {
            op_id,
            app_id,
            url,
            tag,
            commit_hash,
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
        self.show_settings = false;
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

        let mut mime_types = entry.mime_types.clone();
        let mut exec_arg = entry.exec_arg.clone();
        if (mime_types.is_none() || exec_arg.is_none()) && entry.local_path.exists() {
            if let Some(meta) = appmeta::read_for_appimage(&entry.local_path) {
                if mime_types.is_none() {
                    mime_types = meta.mime_types.clone();
                }
                if exec_arg.is_none() {
                    exec_arg = meta.exec_arg.clone();
                }
            } else if let Ok(meta) = crate::appimage::extract_metadata(&entry.local_path, None) {
                if mime_types.is_none() {
                    mime_types = meta.mime_types;
                }
                if exec_arg.is_none() {
                    exec_arg = meta.exec_arg;
                }
            }
        }

        self.add_to = None;
        self.show_settings = false;
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
                } => self.start_update_download(app_id, url, tag, commit_hash),
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
        self.show_settings = false;
        self.side_panel_form.clear();
        self.add_to = Some(AddToState {
            app_id: app.id.clone(),
            app_name: app.display_name.clone(),
            desktop: app.desktop_shortcut,
            menu: app.menu_entry,
        });
        self.status = None;
        self.show_side_panel = true;
    }

    pub(crate) fn close_add_to(&mut self) {
        self.add_to = None;
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
        let path = PathBuf::from(self.settings_form.apps_dir.trim());
        if path.as_os_str().is_empty() {
            self.status = Some("Choose an AppImage storage folder.".into());
            return;
        }
        if let Err(e) = std::fs::create_dir_all(&path) {
            self.status = Some(format!("failed to create storage folder: {e:#}"));
            return;
        }

        let auto_check = self.settings_form.auto_check_updates;
        let hub_menu = self.settings_form.hub_start_menu_shortcut;
        let hub_category = self.settings_form.hub_start_menu_category.clone();
        let hub_desktop = self.settings_form.hub_desktop_shortcut;
        if let Ok(mut cfg) = self.config.lock() {
            cfg.custom_download_dir = Some(path.clone());
            cfg.auto_check_updates = auto_check;
            cfg.hub_start_menu_shortcut = hub_menu;
            cfg.hub_start_menu_category = hub_category.clone();
            cfg.hub_desktop_shortcut = hub_desktop;
            if let Err(e) = cfg.save() {
                self.status = Some(format!("failed to save settings: {e:#}"));
                return;
            }
        }
        desktop_entry::sync_hub_shortcuts(hub_menu, hub_desktop, &hub_category);

        let token = self.settings_form.github_token.trim().to_string();
        if let Err(e) = crate::secrets::save_github_token(&token) {
            self.status = Some(format!("failed to save GitHub token: {e:#}"));
            return;
        }
        self.has_github_token = token_is_present(Some(&token));
        self.send_cmd(BackendCommand::SetGithubToken(if token.is_empty() {
            None
        } else {
            Some(token)
        }));
        let found = self.sync_metadata_files();
        self.show_settings = false;
        self.status = Some(if found > 0 {
            format!(
                "Settings saved. AppImages folder: {}. Found {found} existing app(s).",
                path.display()
            )
        } else {
            format!("Settings saved. AppImages folder: {}", path.display())
        });
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
        entry.desktop_shortcut = state.desktop;
        entry.menu_entry = state.menu;
        if let Ok(mut cfg) = self.config.lock() {
            if let Some(stored) = cfg.applications.iter_mut().find(|a| a.id == state.app_id) {
                stored.desktop_shortcut = state.desktop;
                stored.menu_entry = state.menu;
            }
            let _ = cfg.save();
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

    pub(crate) fn is_right_panel_open(&self) -> bool {
        self.is_form_open || self.add_to.is_some() || self.show_settings
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui, size_class: SizeClass) {
        let full_brand = "AppImage Hub";
        let show_actions_inline = matches!(size_class, SizeClass::Large);
        let tight = matches!(size_class, SizeClass::Tight);

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
                self.render_toolbar_actions(ui, show_actions_inline);
            });
        });
    }

    fn render_toolbar_actions(&mut self, ui: &mut egui::Ui, inline: bool) {
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

            if self.hub_update_url.is_some() || self.hub_update_progress.is_some() {
                let hub_progress = self.hub_update_progress;
                let hub_enabled = hub_progress.is_none();
                let hub_clicked = progress_button(
                    ui,
                    "Update Hub",
                    "Updating Hub…",
                    hub_progress,
                    hub_enabled,
                    152.0,
                )
                .clicked();
                if hub_clicked && hub_enabled {
                    self.start_hub_update();
                }
            }

            ui.add_space(4.0);
            if ui
                .add(crate::ui::theme::secondary_button("Settings"))
                .on_hover_text("Storage, updates, and GitHub API settings")
                .clicked()
            {
                self.open_settings();
            }
        } else {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(crate::ui::theme::add_button("+ Add")).clicked() {
                    self.open_add_form();
                }
                if ui
                    .add(crate::ui::theme::secondary_button("Settings"))
                    .on_hover_text("Storage, updates, and GitHub API settings")
                    .clicked()
                {
                    self.open_settings();
                }
                ui.menu_button("Actions", |ui| {
                    if self.hub_update_url.is_some() {
                        if self.hub_update_progress.is_some() {
                            ui.add_enabled(false, egui::Button::new("Updating Hub…"));
                        } else if ui.button("Update Hub").clicked() {
                            self.start_hub_update();
                            ui.close_menu();
                        }
                        ui.separator();
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

    fn check_hub_update_if_allowed(&mut self) {
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

    fn start_hub_update(&mut self) {
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
        let updates: Vec<(String, AvailableUpdate)> = self
            .update_available
            .iter()
            .map(|(app_id, update)| (app_id.clone(), update.clone()))
            .collect();
        for (app_id, update) in updates {
            self.start_update_download(app_id, update.url, update.tag, update.commit_hash);
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
                apply_detected_category(&mut self.side_panel_form, raw);
            }
        }

        self.side_panel_form.startup_notify = meta.startup_notify;

        if self.side_panel_form.mime_types.trim().is_empty() {
            if let Some(mime_types) = &meta.mime_types {
                self.side_panel_form.mime_types = mime_types.clone();
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
            refresh_existing_shortcuts(&app);
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
            let mut entry = if let Some(meta) = appmeta::read_for_appimage(&appimage) {
                meta.to_entry(appimage.clone())
            } else {
                app_entry_from_appimage(&appimage)
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
            return 0;
        }
        if let Ok(mut cfg) = self.config.lock() {
            cfg.applications.extend(adopted.iter().cloned());
            let _ = cfg.save();
        }
        for entry in &adopted {
            // Scanning a folder must not create shortcuts by itself. Only
            // update entries the user has already chosen to create.
            refresh_existing_shortcuts(entry);
            let _ = appmeta::write_for_entry(entry);
        }
        adopted.len()
    }

    pub(crate) fn updates_available_count(&self) -> usize {
        self.update_available.len()
    }

    /// Recomputes offline file issues (missing/changed AppImages) for every
    /// app. Cheap: metadata only, no content hashing.
    pub(crate) fn refresh_file_issues(&mut self) {
        let apps = self
            .config
            .lock()
            .map(|cfg| cfg.applications.clone())
            .unwrap_or_default();
        let mut issues = HashMap::new();
        for app in &apps {
            if let Some(issue) = cheap_local_file_issue(app) {
                issues.insert(app.id.clone(), issue);
            }
        }
        self.file_issues = issues;
    }

    /// Clears a "changed on disk" warning by accepting the current file as the
    /// new baseline. Runs in the backend so hashing does not block the UI.
    pub(crate) fn accept_current_file(&mut self, app_id: &str) {
        self.send_cmd(BackendCommand::AcceptCurrentFile {
            app_id: app_id.to_string(),
        });
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
        let categories = submitted_categories(&form);
        let startup_notify = form.startup_notify;
        let desktop_shortcut = form.desktop_shortcut;
        let menu_entry = form.menu_entry;
        let mime_types = {
            let value = form.mime_types.trim();
            (!value.is_empty()).then(|| value.to_string())
        };
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
            if let Ok(mut cfg) = self.config.lock() {
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
                let _ = cfg.save();
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
                    commit_hash: None,
                    file_fingerprint: crate::config::file_fingerprint(&managed_path),
                    desktop_shortcut,
                    menu_entry,
                    mime_types: mime_types.clone(),
                    exec_arg: exec_arg.clone(),
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = apply_app_shortcuts(&entry) {
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
                    commit_hash: None,
                    file_fingerprint: crate::config::file_fingerprint(&managed_path),
                    desktop_shortcut,
                    menu_entry,
                    mime_types: mime_types.clone(),
                    exec_arg: exec_arg.clone(),
                };
                if let Err(e) = permissions::set_executable(&managed_path) {
                    self.status = Some(format!("chmod failed: {e:#}"));
                    return;
                }
                if let Err(e) = apply_app_shortcuts(&entry) {
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
        let mut had_events = false;
        while let Ok(evt) = self.evt_rx.try_recv() {
            had_events = true;
            self.apply_event(evt);
        }
        if had_events {
            self.refresh_file_issues();
        }
        // Poll local file state once a second so a replaced/deleted AppImage is
        // flagged even without a GitHub check.
        if self
            .last_file_scan
            .is_none_or(|scan| scan.elapsed() >= Duration::from_secs(1))
        {
            self.last_file_scan = Some(Instant::now());
            self.refresh_file_issues();
        }
        self.tick_update_check(ctx);

        // One-shot auto-check on startup, honouring the persisted preference.
        if !self.auto_check_done {
            self.auto_check_done = true;
            if self.auto_check_enabled() {
                self.check_all_updates();
            }
        }

        if !self.hub_update_check_started {
            self.hub_update_check_started = true;
            self.check_hub_update_if_allowed();
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
                    if self.show_settings {
                        crate::ui::settings_panel::render(ui, self);
                    } else if self.add_to.is_some() {
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
                                    if self.show_settings {
                                        crate::ui::settings_panel::render(ui, self);
                                    } else if self.add_to.is_some() {
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

        // Bottom-panel heights are cached by egui for one frame. Whenever the
        // progress list changes height (a download starts or finishes), the
        // frame that reflects the change lays the footer out against the old
        // height, leaving an unpainted gap. Requesting one extra frame
        // re-anchors the footer at the window bottom immediately instead of
        // leaving a blank block until the next input event.
        let activity_items = self.ops.len() + self.failed_ops.len();
        if activity_items != self.activity_footer_items {
            self.activity_footer_items = activity_items;
            ctx.request_repaint();
        }

        if !self.ops.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if self.update_check.is_some() || self.hub_update_progress.is_some() {
            // Keep the progress animation at display refresh rate.
            ctx.request_repaint();
        }
        // Wake up once a second so the offline file scan notices AppImages
        // replaced or deleted while the window is otherwise idle.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

fn current_appimage_path() -> Option<PathBuf> {
    std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .or_else(|| std::env::current_exe().ok())
}

fn hub_update_is_available(remote_commit: &str, remote_tag: &str) -> bool {
    hub_update_available_with(
        env!("APPIMAGE_HUB_COMMIT"),
        remote_commit,
        env!("CARGO_PKG_VERSION"),
        remote_tag,
    )
}

fn hub_update_available_with(
    local_commit: &str,
    remote_commit: &str,
    current_version: &str,
    remote_tag: &str,
) -> bool {
    let local_commit = local_commit.trim();
    if !local_commit.is_empty() {
        return !local_commit.eq_ignore_ascii_case(remote_commit.trim());
    }

    // Source builds without git metadata fall back to the package version.
    let remote = remote_tag.trim().trim_start_matches('v');
    remote != current_version.trim()
}

fn check_updates_button(ui: &mut egui::Ui, progress: Option<f32>, enabled: bool) -> egui::Response {
    progress_button(
        ui,
        "Check All Updates",
        "Checking updates…",
        progress,
        enabled,
        176.0,
    )
}

fn progress_button(
    ui: &mut egui::Ui,
    idle_label: &str,
    active_label: &str,
    progress: Option<f32>,
    enabled: bool,
    width: f32,
) -> egui::Response {
    let text = match progress {
        Some(fraction) => format!(
            "{active_label} {}%",
            (fraction.clamp(0.0, 1.0) * 100.0).round() as i32
        ),
        None => idle_label.to_string(),
    };
    let desired = egui::vec2(width, ui.spacing().interact_size.y.max(32.0));
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
        commit_hash: None,
        file_fingerprint: crate::config::file_fingerprint(path),
        desktop_shortcut: true,
        menu_entry: true,
        mime_types: None,
        exec_arg: None,
    }
}

fn apply_app_shortcuts(entry: &AppEntry) -> anyhow::Result<()> {
    let desktop = xdg::desktop_dir()?;
    let apps = xdg::applications_dir()?;
    if entry.desktop_shortcut {
        desktop_entry::write_desktop_shortcut(entry, &desktop)?;
    } else {
        let _ = desktop_entry::remove_desktop_shortcut(entry, &desktop);
    }
    if entry.menu_entry {
        desktop_entry::write_menu_entry(entry, &apps)?;
    } else {
        let _ = desktop_entry::remove_menu_entry(entry, &apps);
    }
    Ok(())
}

/// Refresh desktop/menu entries only when they already exist. Used by update,
/// migration, and folder-scan paths so they never create new shortcuts.
fn refresh_existing_shortcuts(entry: &AppEntry) {
    if let Ok(desktop_dir) = xdg::desktop_dir() {
        let path = desktop_entry::desktop_shortcut_path(entry, &desktop_dir);
        if path.exists() {
            let _ = desktop_entry::write_desktop_shortcut(entry, &desktop_dir);
        }
    }
    if let Ok(apps_dir) = xdg::applications_dir() {
        let path = desktop_entry::menu_entry_path(entry, &apps_dir);
        if path.exists() {
            let _ = desktop_entry::write_menu_entry(entry, &apps_dir);
        }
    }
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

fn build_github_client(token: Option<&str>) -> octocrab::Result<octocrab::Octocrab> {
    match token.map(str::trim).filter(|token| !token.is_empty()) {
        Some(token) => octocrab::Octocrab::builder()
            .personal_token(token.to_string())
            .build(),
        None => octocrab::Octocrab::builder().build(),
    }
}

fn spawn_backend(
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

            // Multiple apps can share one repository. Cache release lookups for
            // the same rate-limit window (2 min with a token, 5 min without) so
            // a batch check costs one request per unique repo instead of one per
            // app.
            let mut release_cache_ttl = Duration::from_secs(update_check_cooldown_secs(
                token_is_present(initial_token.as_deref()),
            ));
            let mut release_cache: HashMap<String, (Instant, github::ReleaseInfo)> = HashMap::new();

            while let Some(cmd) = cmd_rx.recv().await {
                match cmd {
                    BackendCommand::SetGithubToken(token) => {
                        release_cache_ttl =
                            Duration::from_secs(update_check_cooldown_secs(token_is_present(
                                token.as_deref(),
                            )));
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
                        match github::fetch_latest_release_with_selector(
                            &octo,
                            "MichaelX17/AppImage-Hub",
                            None,
                        )
                        .await
                        {
                            Ok(info) => {
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
                            Err(e) => {
                                let _ = evt_tx
                                    .send(BackendEvent::HubUpdateFailed {
                                        message: format!("{e:#}"),
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
                        let temp = downloader::temp_path_for(&target);
                        let progress_tx = evt_tx.clone();
                        let result = downloader::download_to_path_with_progress(
                            &http,
                            &url,
                            &temp,
                            cancel,
                            move |progress| {
                                let _ = progress_tx.try_send(
                                    BackendEvent::HubUpdateProgress {
                                        fraction: progress.fraction,
                                        downloaded: progress.downloaded,
                                        total: progress.total,
                                    },
                                );
                            },
                        )
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
                                    continue;
                                }
                                if let Err(e) = downloader::atomic_replace(&temp, &target) {
                                    let _ = std::fs::remove_file(&temp);
                                    let _ = evt_tx
                                        .send(BackendEvent::HubUpdateFailed {
                                            message: format!("replace failed: {e:#}"),
                                        })
                                        .await;
                                    continue;
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
                                    let cache_key = repo.trim().to_lowercase();
                                    let now = Instant::now();
                                    let cached = release_cache.get(&cache_key).and_then(
                                        |(fetched_at, info)| {
                                            (now.duration_since(*fetched_at) < release_cache_ttl)
                                                .then(|| info.clone())
                                        },
                                    );
                                    let release = match cached {
                                        Some(info) => Ok(info),
                                        None => github::fetch_latest_release_with_selector(
                                            &octo,
                                            &repo,
                                            entry.asset_preference.as_deref(),
                                        )
                                        .await
                                        .inspect(|info| {
                                            release_cache.insert(
                                                cache_key,
                                                (now, info.clone()),
                                            );
                                        }),
                                    };
                                    match release {
                                        Ok(info) => {
                                            // Confirm the file state off the
                                            // async runtime: this may hash a
                                            // large AppImage when the cheap
                                            // size/mtime metadata changed.
                                            let status_entry = entry.clone();
                                            let local_issue = tokio::task::spawn_blocking(
                                                move || verified_local_file_issue(&status_entry),
                                            )
                                            .await
                                            .unwrap_or(None);

                                            if local_issue.is_none() {
                                                let baseline_entry = entry.clone();
                                                let baseline = tokio::task::spawn_blocking(
                                                    move || baseline_fingerprint(&baseline_entry),
                                                )
                                                .await
                                                .ok()
                                                .flatten();
                                                if let Some(fingerprint) = baseline {
                                                    persist_baseline_fingerprint(
                                                        &config,
                                                        &entry.id,
                                                        fingerprint,
                                                        false,
                                                    );
                                                }
                                            }

                                            let commit_changed = is_update_available(
                                                Some(entry.commit_hash.as_deref().unwrap_or_default()),
                                                &info.commit_hash,
                                            );

                                            if local_issue.is_none() && !commit_changed {
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
                                                        commit_hash: info.commit_hash,
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
                        commit_hash,
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
                                    let hash_target = target.clone();
                                    let downloaded_fingerprint = tokio::task::spawn_blocking(
                                        move || {
                                            crate::config::file_fingerprint_with_hash(&hash_target)
                                        },
                                    )
                                    .await
                                    .ok()
                                    .flatten()
                                    .or_else(|| crate::config::file_fingerprint(&target));
                                    let mut updated_entry: Option<AppEntry> = None;
                                    if let Ok(mut cfg) = config.lock() {
                                        if let Some(entry) =
                                            cfg.applications.iter_mut().find(|a| a.id == app_id)
                                        {
                                            entry.current_version = tag.clone();
                                            entry.commit_hash = Some(commit_hash.clone());
                                            entry.file_fingerprint = downloaded_fingerprint;
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
                                        // Updates only refresh shortcuts that
                                        // already exist; they never create new ones.
                                        refresh_existing_shortcuts(&entry);
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
                        desktop_shortcut,
                        menu_entry,
                        mime_types,
                        exec_arg,
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

                                                let hash_target = target.clone();
                                                let installed_fingerprint = tokio::task::spawn_blocking(
                                                    move || {
                                                        crate::config::file_fingerprint_with_hash(
                                                            &hash_target,
                                                        )
                                                    },
                                                )
                                                .await
                                                .ok()
                                                .flatten()
                                                .or_else(|| {
                                                    crate::config::file_fingerprint(&target)
                                                });
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
                                                    commit_hash: Some(info.commit_hash),
                                                    file_fingerprint: installed_fingerprint,
                                                    desktop_shortcut,
                                                    menu_entry,
                                                    mime_types: mime_types.or_else(|| {
                                                        app_meta
                                                            .as_ref()
                                                            .and_then(|m| m.mime_types.clone())
                                                    }),
                                                    exec_arg: exec_arg.or_else(|| {
                                                        app_meta
                                                            .as_ref()
                                                            .and_then(|m| m.exec_arg.clone())
                                                    }),
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

/// Commit-hash-only update comparison. A missing local hash means the release
/// must be installed once to establish a baseline.
fn is_update_available(installed_commit: Option<&str>, remote_commit: &str) -> bool {
    let Some(installed) = installed_commit.map(str::trim).filter(|c| !c.is_empty()) else {
        return true;
    };
    !installed.eq_ignore_ascii_case(remote_commit.trim())
}

/// AppImage Hub always writes the sidecar after touching the binary, so an
/// AppImage strictly newer than its sidecar was written by someone else.
fn sidecar_is_older_than_file(entry: &AppEntry, current: &crate::config::FileFingerprint) -> bool {
    let sidecar = appmeta::sidecar_path(&entry.local_path);
    sidecar
        .metadata()
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|mtime| current.mtime > mtime.as_secs() as i64)
        .unwrap_or(false)
}

/// Cheap local check (metadata only, no hashing) used by the periodic UI scan.
fn cheap_local_file_issue(entry: &AppEntry) -> Option<LocalFileIssue> {
    let Some(current) = crate::config::file_fingerprint(&entry.local_path) else {
        return Some(LocalFileIssue::Missing);
    };
    match entry.file_fingerprint.as_ref() {
        Some(stored) if stored.size == current.size && stored.mtime == current.mtime => None,
        Some(_) => Some(LocalFileIssue::Changed),
        None => sidecar_is_older_than_file(entry, &current).then_some(LocalFileIssue::Changed),
    }
}

/// Verified local check used by update checks.
///
/// When the cheap metadata differs but a stored SHA-256 exists, the file is
/// hashed before flagging it: touch/copy/backup operations that preserve the
/// content are not reported as a different build.
fn verified_local_file_issue(entry: &AppEntry) -> Option<LocalFileIssue> {
    let Some(current) = crate::config::file_fingerprint(&entry.local_path) else {
        return Some(LocalFileIssue::Missing);
    };
    let Some(stored) = entry.file_fingerprint.as_ref() else {
        return sidecar_is_older_than_file(entry, &current).then_some(LocalFileIssue::Changed);
    };
    if stored.size == current.size && stored.mtime == current.mtime {
        return None;
    }
    if let Some(stored_hash) = stored.sha256.as_deref().filter(|hash| !hash.is_empty()) {
        if crate::config::sha256_file(&entry.local_path)
            .is_some_and(|current_hash| stored_hash.eq_ignore_ascii_case(&current_hash))
        {
            return None;
        }
    }
    Some(LocalFileIssue::Changed)
}

/// Builds a fresh fingerprint for a known-good file. The content hash is
/// reused when it already exists; only the first baseline hashes the file.
fn baseline_fingerprint(entry: &AppEntry) -> Option<crate::config::FileFingerprint> {
    let current = crate::config::file_fingerprint(&entry.local_path)?;
    let existing_hash = entry
        .file_fingerprint
        .as_ref()
        .and_then(|fingerprint| fingerprint.sha256.clone())
        .filter(|hash| !hash.is_empty());
    let sha256 = existing_hash.or_else(|| crate::config::sha256_file(&entry.local_path));
    Some(crate::config::FileFingerprint {
        size: current.size,
        mtime: current.mtime,
        sha256,
    })
}

/// Persists a verified fingerprint, re-reading the live entry so a concurrent
/// edit is not overwritten in the sidecar. Must be called from a background
/// context (the sidecar write is small but the caller may be async).
fn persist_baseline_fingerprint(
    config: &Arc<Mutex<Config>>,
    app_id: &str,
    fingerprint: crate::config::FileFingerprint,
    clear_version: bool,
) -> bool {
    let new_hash = fingerprint.sha256.clone();
    let mut updated: Option<AppEntry> = None;
    let mut found = false;
    if let Ok(mut cfg) = config.lock() {
        if let Some(stored) = cfg.applications.iter_mut().find(|a| a.id == app_id) {
            found = true;
            let mut changed = false;
            let old_hash = stored
                .file_fingerprint
                .as_ref()
                .and_then(|old| old.sha256.as_deref());
            let content_unchanged = old_hash
                .zip(new_hash.as_deref())
                .is_some_and(|(old, new)| old.eq_ignore_ascii_case(new));
            if stored.file_fingerprint.as_ref() != Some(&fingerprint) {
                stored.file_fingerprint = Some(fingerprint);
                changed = true;
            }
            if clear_version && !content_unchanged && !stored.current_version.is_empty() {
                // The version recorded for the previous managed build no
                // longer describes this manually accepted file.
                stored.current_version.clear();
                changed = true;
            }
            if changed {
                updated = Some(stored.clone());
            }
        }
        if updated.is_some() {
            let _ = cfg.save();
        }
    }
    if let Some(entry) = updated {
        if entry.local_path.exists() {
            let _ = appmeta::write_for_entry(&entry);
        }
    }
    found
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

/// Applies an XDG category list detected from AppImage/sidecar metadata,
/// using the same primary-category mapping as the embedded-desktop reader.
fn apply_detected_category(form: &mut crate::ui::side_panel::SidePanelFormState, raw: &str) {
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    let (category, _is_custom) = crate::appimage::map_kde_category(raw);
    if !form.category_user_selected {
        form.detected_category = Some(category);
    }
}

/// Category value submitted for the current form. GitHub-only installs return
/// `None` unless the user made an explicit category choice, so the downloaded
/// AppImage's internal metadata can supply the category.
fn submitted_categories(form: &crate::ui::side_panel::SidePanelFormState) -> Option<String> {
    if form.category_user_selected {
        return Some(ensure_trailing_semicolon(&form.effective_category()));
    }

    if matches!(
        form.mode,
        crate::ui::side_panel::SidePanelMode::LocalOnly
            | crate::ui::side_panel::SidePanelMode::LocalWithRepo
    ) {
        return Some(
            form.detected_category
                .as_deref()
                .map(ensure_trailing_semicolon)
                .unwrap_or_else(|| "Utility;".to_string()),
        );
    }

    // GitHub-only installs resolve the category from the downloaded AppImage.
    None
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
    fn detected_categories_use_primary_kde_mapping_without_changing_selector() {
        let mut form = crate::ui::side_panel::SidePanelFormState {
            category: "Utility".to_string(),
            ..Default::default()
        };
        apply_detected_category(&mut form, "Game;Emulator;");
        assert_eq!(form.category, "Utility");
        assert_eq!(form.detected_category.as_deref(), Some("Game"));

        apply_detected_category(&mut form, "Science;Math;");
        assert_eq!(form.category, "Utility");
        assert_eq!(form.detected_category.as_deref(), Some("Science"));

        apply_detected_category(&mut form, "UnknownCategory;");
        assert_eq!(form.category, "Utility");
        assert_eq!(form.detected_category.as_deref(), Some("UnknownCategory"));
    }

    #[test]
    fn github_install_uses_inner_category_when_user_has_not_selected() {
        let mut form = crate::ui::side_panel::SidePanelFormState {
            mode: crate::ui::side_panel::SidePanelMode::GithubOnly,
            ..Default::default()
        };
        assert_eq!(submitted_categories(&form), None);

        form.category_user_selected = true;
        form.category = "Graphics".to_string();
        assert_eq!(submitted_categories(&form).as_deref(), Some("Graphics;"));

        // Explicitly choosing the default Utility category must not fall back
        // to the AppImage's internal category.
        form.category = "Utility".to_string();
        assert_eq!(submitted_categories(&form).as_deref(), Some("Utility;"));
    }

    #[test]
    fn local_import_submits_detected_category() {
        let form = crate::ui::side_panel::SidePanelFormState {
            mode: crate::ui::side_panel::SidePanelMode::LocalOnly,
            category: "Utility".to_string(),
            detected_category: Some("AudioVideo".to_string()),
            ..Default::default()
        };
        assert_eq!(submitted_categories(&form).as_deref(), Some("AudioVideo;"));

        let default_form = crate::ui::side_panel::SidePanelFormState {
            mode: crate::ui::side_panel::SidePanelMode::LocalOnly,
            ..Default::default()
        };
        assert_eq!(
            submitted_categories(&default_form).as_deref(),
            Some("Utility;")
        );
    }

    #[test]
    fn update_checks_respect_cooldown() {
        assert_eq!(update_check_cooldown_remaining(None, 1_000, 300), None);
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 1_000, 300),
            Some(300)
        );
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 1_120, 300),
            Some(180)
        );
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 1_300, 300),
            None
        );
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 10_000, 300),
            None
        );

        // With a GitHub token the cooldown is two minutes instead of five.
        assert_eq!(update_check_cooldown_secs(false), 5 * 60);
        assert_eq!(update_check_cooldown_secs(true), 2 * 60);
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 1_060, 120),
            Some(60)
        );
        assert_eq!(
            update_check_cooldown_remaining(Some(1_000), 1_120, 120),
            None
        );
    }

    #[test]
    fn hub_update_uses_commit_hash_with_version_fallback() {
        assert!(!hub_update_available_with(
            "0123456789abcdef0123456789abcdef01234567",
            "0123456789ABCDEF0123456789ABCDEF01234567",
            "0.1.0",
            "v0.2.0"
        ));
        assert!(hub_update_available_with(
            "0123456789abcdef0123456789abcdef01234567",
            "fedcba9876543210fedcba9876543210fedcba98",
            "0.1.0",
            "v0.2.0"
        ));
        assert!(!hub_update_available_with("", "", "0.1.0", "v0.1.0"));
        assert!(hub_update_available_with("", "", "0.1.0", "v0.2.0"));
    }

    #[test]
    fn update_check_compares_commit_hashes_only() {
        assert!(!is_update_available(
            Some("0123456789abcdef0123456789abcdef01234567"),
            "0123456789ABCDEF0123456789ABCDEF01234567"
        ));
        assert!(is_update_available(
            Some("0123456789abcdef0123456789abcdef01234567"),
            "fedcba9876543210fedcba9876543210fedcba98"
        ));
        assert!(is_update_available(
            None,
            "fedcba9876543210fedcba9876543210fedcba98"
        ));
        assert!(is_update_available(
            Some("   "),
            "fedcba9876543210fedcba9876543210fedcba98"
        ));
    }

    fn fingerprint_test_entry(local_path: std::path::PathBuf) -> AppEntry {
        AppEntry {
            id: "fingerprint-test".into(),
            display_name: "Fingerprint Test".into(),
            source: AppSource::GithubOnly {
                github_repo: "owner/repo".into(),
            },
            local_path,
            current_version: "v1.0.0".into(),
            args: Vec::new(),
            icon_path: None,
            generic_name: None,
            comment: None,
            categories: None,
            startup_notify: false,
            asset_preference: None,
            added_at: None,
            commit_hash: Some("0123456789abcdef".into()),
            file_fingerprint: None,
            desktop_shortcut: false,
            menu_entry: false,
            mime_types: None,
            exec_arg: None,
        }
    }

    fn set_mtime(path: &std::path::Path, secs: u64) {
        let mtime = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    #[test]
    fn sha256_of_known_content() {
        let dir = std::env::temp_dir().join(format!("aim-sha-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("data.bin");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            crate::config::sha256_file(&file).as_deref(),
            Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn verified_status_ignores_touched_but_identical_files() {
        let dir = std::env::temp_dir().join(format!("aim-fp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("app.appimage");
        std::fs::write(&app, b"version one").unwrap();

        let mut entry = fingerprint_test_entry(app.clone());
        let original = crate::config::file_fingerprint_with_hash(&app).unwrap();
        entry.file_fingerprint = Some(original.clone());
        assert_eq!(verified_local_file_issue(&entry), None);

        // Touch: cheap metadata changes, but the content hash still matches.
        set_mtime(&app, original.mtime as u64 + 30);
        assert_eq!(
            cheap_local_file_issue(&entry),
            Some(LocalFileIssue::Changed)
        );
        assert_eq!(verified_local_file_issue(&entry), None);

        // Real replacement: content hash no longer matches.
        std::fs::write(&app, b"a much longer replacement build").unwrap();
        assert_eq!(
            verified_local_file_issue(&entry),
            Some(LocalFileIssue::Changed)
        );

        // Missing file has a distinct state.
        std::fs::remove_file(&app).unwrap();
        assert_eq!(
            verified_local_file_issue(&entry),
            Some(LocalFileIssue::Missing)
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn baseline_adds_hash_and_reuses_existing_hash() {
        let dir = std::env::temp_dir().join(format!("aim-fp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("app.appimage");
        std::fs::write(&app, b"payload").unwrap();

        let mut entry = fingerprint_test_entry(app.clone());
        let cheap = crate::config::file_fingerprint(&app).unwrap();
        entry.file_fingerprint = Some(cheap);
        let baseline = baseline_fingerprint(&entry).unwrap();
        assert!(baseline.sha256.is_some());

        set_mtime(&app, baseline.mtime as u64 + 5);
        entry.file_fingerprint = Some(baseline.clone());
        let rebaselined = baseline_fingerprint(&entry).unwrap();
        assert_eq!(rebaselined.sha256, baseline.sha256);
        assert_eq!(rebaselined.mtime, baseline.mtime + 5);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn legacy_entries_compare_appimage_mtime_with_sidecar() {
        let dir = std::env::temp_dir().join(format!("aim-fp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let app = dir.join("app.appimage");
        std::fs::write(&app, b"binary").unwrap();
        set_mtime(&app, 1_700_000_000);

        let sidecar = crate::appmeta::sidecar_path(&app);
        std::fs::write(&sidecar, b"{}").unwrap();

        // Sidecar written before the binary => file replaced outside the app.
        set_mtime(&sidecar, 1_699_999_000);
        let entry = fingerprint_test_entry(app.clone());
        assert_eq!(
            verified_local_file_issue(&entry),
            Some(LocalFileIssue::Changed)
        );

        // Sidecar written after the binary => normal managed state.
        set_mtime(&sidecar, 1_700_001_000);
        assert_eq!(verified_local_file_issue(&entry), None);

        std::fs::remove_dir_all(&dir).unwrap();
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
