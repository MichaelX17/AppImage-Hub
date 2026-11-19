use crate::appmeta;
use crate::config::{AppEntry, AppSource, Config};
use crate::desktop_entry;
use crate::github;
use crate::launcher;
use crate::permissions;
use crate::ui::breakpoints::SizeClass;
use crate::xdg;
use eframe::egui::{self, RichText};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

mod actions;
mod events;
mod first_run;
mod render;
mod snapshot;
#[cfg(test)]
mod tests;
mod toolbar;
mod update_check;
mod util;

#[cfg(test)]
pub(crate) use crate::backend::is_update_available;
#[cfg(test)]
pub(crate) use crate::scan::cheap_local_file_issue;
#[cfg(test)]
pub(crate) use crate::scan::verified_local_file_issue;
pub(crate) use crate::scan::LocalFileIssue;
#[cfg(test)]
pub(crate) use crate::scan::{baseline_fingerprint, cheap_issue_for_file};
#[cfg(test)]
pub(crate) use snapshot::apply_file_scan;
pub(crate) use snapshot::{AppSnapshotCache, CachedAppMeta, FileScanBatch, FileScanSpec};
pub use update_check::{UpdateCheckProgress, UpdateCheckTick};
pub(crate) use util::*;

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
        /// Published `<asset>.sha256` URL, when the release provides one.
        checksum_url: Option<String>,
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
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    RepoAssetsFetched {
        repo: String,
        assets: Vec<crate::github::GithubAssetInfo>,
    },
    RepoAssetsFailed {
        error: String,
    },
    UpdateAvailable {
        app_id: String,
        tag: String,
        url: String,
        commit_hash: String,
        checksum_url: Option<String>,
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
        fraction: f32,
        downloaded: u64,
        total: Option<u64>,
    },
    DownloadComplete {
        op_id: String,
        app_id: String,
        tag: String,
        checksum: ChecksumStatus,
    },
    AppInstalled {
        op_id: String,
        app_id: String,
        checksum: ChecksumStatus,
    },
    DownloadCancelled {
        op_id: String,
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

/// Whether the downloaded AppImage was checked against a published SHA-256.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumStatus {
    Verified,
    /// The release did not publish `<asset>.sha256`; the install still
    /// proceeds, but the status line says so.
    Unavailable,
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

/// Bound on establishing a connection to the GitHub API.
pub(crate) const API_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Hard bound on one GitHub API request (headers + body).
pub(crate) const API_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Bound on establishing a download connection.
pub(crate) const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Longest tolerated stall between download chunks. A black-holed server fails
/// the download instead of hanging the operation forever.
pub(crate) const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Concurrent download/install/self-update jobs. Keeps "Update All" from
/// saturating the network link while still overlapping independent apps.
pub(crate) const MAX_CONCURRENT_TRANSFERS: usize = 3;
/// Hard bound on one backend update check (network plus local hashing), so a
/// vanished app or a stalled request always produces a terminal event.
pub(crate) const UPDATE_CHECK_TASK_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) fn token_is_present(token: Option<&str>) -> bool {
    token.is_some_and(|token| !token.trim().is_empty())
}

pub(crate) fn update_check_cooldown_secs(has_github_token: bool) -> u64 {
    if has_github_token {
        AUTHENTICATED_UPDATE_CHECK_COOLDOWN_SECS
    } else {
        UPDATE_CHECK_COOLDOWN_SECS
    }
}

pub(crate) fn now_unix() -> i64 {
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
        checksum_url: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct AvailableUpdate {
    pub tag: String,
    pub url: String,
    pub commit_hash: String,
    pub checksum_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FailedOp {
    pub app_id: Option<String>,
    pub label: String,
    pub message: String,
    pub retry: Option<RetryAction>,
}

pub enum AppDownloadState<'a> {
    Downloading,
    Failed(&'a FailedOp),
    Idle,
}

#[derive(Debug, Clone)]
pub struct AddToState {
    pub app_id: String,
    pub app_name: String,
    pub desktop: bool,
    pub menu: bool,
    /// Resolved shortcut targets, computed once when the panel opens so the
    /// render path does no filesystem or process work.
    pub desktop_path: Option<PathBuf>,
    pub menu_path: Option<PathBuf>,
}

pub struct ManagerApp {
    pub(crate) config: Arc<Mutex<Config>>,
    pub(crate) cmd_tx: mpsc::Sender<BackendCommand>,
    pub(crate) evt_rx: mpsc::Receiver<BackendEvent>,
    pub(crate) ops: HashMap<String, ActiveOp>,
    pub(crate) failed_ops: Vec<FailedOp>,
    pub(crate) status: Option<String>,
    /// Load problem shown in the top bar (for example a corrupt `config.toml`
    /// that was quarantined). Automatic saves are paused until it is shown.
    pub(crate) config_warning: Option<String>,
    /// Non-fatal token-storage notices from startup (for example a legacy
    /// token file that was migrated, or a portable leftover token file).
    pub(crate) token_warning: Option<String>,
    pub(crate) pending_delete: Option<String>,
    /// Set when the user changes the storage folder and apps already exist;
    /// the confirmation modal decides whether to apply the change (D4: keep
    /// existing entries in place and warn).
    pub(crate) pending_storage_change: Option<PathBuf>,
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
    /// Cached app list and sort metadata, rebuilt only on config writes.
    snapshot: AppSnapshotCache,
    /// Background file scanner channels.
    scan_tx: std::sync::mpsc::Sender<Vec<FileScanSpec>>,
    scan_rx: std::sync::mpsc::Receiver<FileScanBatch>,
    scan_seq: u64,
    /// Startup icon repair / sidecar sync is deferred past the first frame.
    startup_scan_pending: bool,
    startup_scan_started: bool,
}

/// Result of reconciling per-AppImage sidecars with `config.toml`.
#[derive(Debug, Default, Clone)]
pub(crate) struct MetadataSyncReport {
    /// AppImages discovered on disk and adopted into the config.
    pub adopted: usize,
    /// Invalid sidecars that were quarantined.
    pub repaired: usize,
    /// Sidecars written by a newer AppImage Hub build; left untouched.
    pub skipped_newer: usize,
    /// Human-readable details for the files that needed attention.
    pub warnings: Vec<String>,
}

impl MetadataSyncReport {
    /// Short status-bar note, or `None` when nothing needed attention.
    fn note(&self) -> Option<String> {
        let mut parts = Vec::new();
        if self.repaired > 0 {
            parts.push(format!(
                "Quarantined {} corrupt metadata file(s)",
                self.repaired
            ));
        }
        if self.skipped_newer > 0 {
            parts.push(format!(
                "Skipped {} metadata file(s) written by a newer AppImage Hub",
                self.skipped_newer
            ));
        }
        if parts.is_empty() {
            return None;
        }
        if let Some(first) = self.warnings.first() {
            parts.push(first.clone());
        }
        Some(parts.join(". "))
    }
}

pub fn setup_custom_style(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    crate::ui::theme::apply(ctx);
}

impl ManagerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        github::ensure_crypto_provider();
        setup_custom_style(&cc.egui_ctx);

        let (loaded_config, config_warning) = match Config::load() {
            Ok(config) => (config, None),
            Err(error) => (
                Config::default(),
                Some(format!("Your configuration could not be loaded. {error:#}")),
            ),
        };
        let config = Arc::new(Mutex::new(loaded_config));

        // Do not act on default settings derived from an unreadable config:
        // wait until the user has seen the warning and chosen a storage folder.
        if config_warning.is_none() {
            if let Ok(cfg) = config.lock() {
                desktop_entry::sync_hub_shortcuts(
                    cfg.hub_start_menu_shortcut,
                    cfg.hub_desktop_shortcut,
                    &cfg.hub_start_menu_category,
                );
            }
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
        let token_report = crate::secrets::load_github_token_report();
        let token_warning = if token_report.warnings.is_empty() {
            None
        } else {
            Some(token_report.warnings.join(" "))
        };
        let github_token = token_report.token.or_else(|| {
            std::env::var("GITHUB_TOKEN")
                .ok()
                .or_else(|| std::env::var("GH_TOKEN").ok())
                .filter(|token| !token.trim().is_empty())
        });
        let has_github_token = token_is_present(github_token.as_deref());
        crate::backend::spawn_backend(config.clone(), cmd_rx, evt_tx, github_token);
        let (scan_tx, scan_rx) = snapshot::spawn_file_scanner();

        // Remove abandoned partial downloads from previous runs (older than an
        // hour; never an `.appimage` file).
        if let Ok(cfg) = config.lock() {
            let roots = crate::storage::roots(&cfg);
            drop(cfg);
            let _ =
                crate::downloader::cleanup_stale_temp_files(&roots, Duration::from_secs(60 * 60));
        }

        let mut app = Self {
            config,
            cmd_tx,
            evt_rx,
            ops: HashMap::new(),
            failed_ops: Vec::new(),
            status: None,
            config_warning,
            token_warning,
            pending_delete: None,
            pending_storage_change: None,
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
            snapshot: AppSnapshotCache::new(),
            scan_tx,
            scan_rx,
            scan_seq: 0,
            startup_scan_pending: false,
            startup_scan_started: false,
        };
        app.refresh_apps_snapshot();
        if !app.first_run_setup {
            // Paint the first frame before the (possibly slow) storage scan.
            app.startup_scan_pending = true;
            app.status = Some("Scanning storage…".into());
        }
        app.refresh_file_issues();
        app
    }

    /// Sets the status line from any string-like value.
    pub(crate) fn set_status(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    /// Sets the status line as `"<context>: <error>"`.
    pub(crate) fn report_error(&mut self, context: &str, error: impl std::fmt::Display) {
        self.status = Some(format!("{context}: {error}"));
    }

    pub(crate) fn send_cmd(&mut self, cmd: BackendCommand) -> bool {
        if self.cmd_tx.try_send(cmd).is_err() {
            self.status = Some("System busy, please try again.".into());
            return false;
        }
        true
    }

    pub(crate) fn finish_update_check_item(&mut self, app_id: &str) {
        if let Some(progress) = self.update_check.as_mut() {
            progress.mark_finished(app_id);
        }
    }

    pub(crate) fn tick_update_check(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|input| input.stable_dt).min(0.1);
        let now = Instant::now();
        let outcome = self
            .update_check
            .as_mut()
            .map(|progress| progress.tick(dt, now));
        match outcome {
            Some(UpdateCheckTick::Complete) => self.update_check = None,
            Some(UpdateCheckTick::TimedOut { remaining }) => {
                self.update_check = None;
                self.status = Some(format!(
                    "Update check timed out; {remaining} app(s) did not respond."
                ));
            }
            Some(UpdateCheckTick::Running) | None => {}
        }
    }

    pub(crate) fn app_exists(&self, app_id: &str) -> bool {
        self.config
            .lock()
            .ok()
            .is_some_and(|config| config.applications.iter().any(|app| app.id == app_id))
    }

    pub(crate) fn app_display_name(&self, app_id: &str) -> String {
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

    pub(crate) fn notify_completion(&self, app_id: &str, tag: &str) {
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

    /// Launches an app directly from the UI, independent of the backend queue.
    /// The child is reaped on a background thread, so a running AppImage never
    /// blocks a frame or waits behind an active download.
    pub(crate) fn launch_app(&mut self, app_id: &str) {
        let entry = self.config.lock().ok().and_then(|config| {
            config
                .applications
                .iter()
                .find(|app| app.id == app_id)
                .cloned()
        });
        let Some(entry) = entry else {
            self.status = Some("Application not found".into());
            return;
        };
        match launcher::spawn(&entry.local_path, &entry.args) {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => {
                self.status = Some(format!(
                    "Failed to launch {}: {error:#}",
                    entry.display_name
                ));
            }
        }
    }

    pub(crate) fn request_delete(&mut self, app_id: String) {
        self.pending_delete = Some(app_id);
    }

    pub(crate) fn is_right_panel_open(&self) -> bool {
        self.is_form_open || self.add_to.is_some() || self.show_settings
    }
}
