//! Cached app view, background file scanner, and async startup tasks.
//!
//! Every slow operation (OS keyring/Argon2id token load, shortcut sync, temp
//! cleanup, SquashFS icon/WMClass extraction, recursive storage scan, and 1 Hz
//! file issue polling) runs on background threads so the UI thread never blocks.

use super::*;
use crate::scan::cheap_issue_for_file;

pub(crate) struct StartupInitOutcome {
    pub github_token: Option<String>,
    pub token_warning: Option<String>,
}

pub(crate) struct StartupScanOutcome {
    pub status_message: Option<String>,
}

/// Runs all blocking startup I/O off the UI thread:
/// - Hub desktop/menu shortcut sync (idempotent)
/// - Stale partial download cleanup
/// - GitHub token load (OS keyring D-Bus + Argon2id fallback)
pub(crate) fn spawn_startup_init(
    config: Arc<Mutex<Config>>,
    sync_shortcuts: bool,
    ctx: egui::Context,
) -> std::sync::mpsc::Receiver<StartupInitOutcome> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("startup-init".into())
        .spawn(move || {
            let (roots, shortcut_spec) = config
                .lock()
                .ok()
                .map(|cfg| {
                    let spec = sync_shortcuts.then(|| {
                        (
                            cfg.hub_start_menu_shortcut,
                            cfg.hub_desktop_shortcut,
                            cfg.hub_start_menu_category.clone(),
                        )
                    });
                    (crate::storage::roots(&cfg), spec)
                })
                .unwrap_or_default();

            if let Some((menu, desktop, cat)) = shortcut_spec {
                desktop_entry::sync_hub_shortcuts(menu, desktop, &cat);
            }

            let _ =
                crate::downloader::cleanup_stale_temp_files(&roots, Duration::from_secs(60 * 60));

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

            let _ = tx.send(StartupInitOutcome {
                github_token,
                token_warning,
            });
            ctx.request_repaint();
        })
        .ok();
    rx
}

/// Runs the full storage scan & metadata repair off the UI thread so the
/// window stays 100% responsive immediately after launch.
pub(crate) fn spawn_startup_scan(
    config: Arc<Mutex<Config>>,
    announce_if_clean: bool,
    ctx: egui::Context,
) -> std::sync::mpsc::Receiver<StartupScanOutcome> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("startup-scan".into())
        .spawn(move || {
            actions::repair_missing_icons_in(&config);
            actions::normalize_managed_filenames_in(&config);
            let wm_err = actions::repair_startup_wm_classes_in(&config);
            let report = actions::sync_metadata_files_in(&config);

            let mut parts: Vec<String> = Vec::new();
            if report.adopted > 0 {
                parts.push(format!("Found {} existing app(s)", report.adopted));
            }
            if let Some(note) = report.note() {
                parts.push(note);
            }
            if let Some(err) = wm_err {
                parts.push(err);
            }

            let status_message = if !parts.is_empty() {
                Some(parts.join(". "))
            } else if announce_if_clean {
                Some("Storage scan complete.".to_string())
            } else {
                None
            };

            let _ = tx.send(StartupScanOutcome { status_message });
            ctx.request_repaint();
        })
        .ok();
    rx
}

impl ManagerApp {
    /// Number of apps in the cached snapshot (never locks the config).
    pub(crate) fn app_count(&self) -> usize {
        self.snapshot.apps.len()
    }

    /// Rebuilds the cached app list only when the config was written.
    pub(crate) fn refresh_apps_snapshot(&mut self) -> bool {
        let generation = crate::config::generation();
        if generation == self.snapshot.generation {
            return false;
        }
        let apps = self
            .config
            .lock()
            .map(|config| config.applications.clone())
            .unwrap_or_default();
        self.snapshot.rebuild(generation, apps)
    }

    /// Hands the current snapshot to the background scanner.
    pub(crate) fn request_file_scan(&mut self) {
        let specs: Vec<FileScanSpec> = self
            .snapshot
            .apps
            .iter()
            .map(|app| FileScanSpec {
                id: app.id.clone(),
                path: app.local_path.clone(),
                fingerprint: app.file_fingerprint.clone(),
            })
            .collect();
        self.scan_seq = self.scan_seq.wrapping_add(1);
        let _ = self.scan_tx.send(specs);
    }

    /// Requests an immediate background scan; results arrive on a later frame.
    pub(crate) fn refresh_file_issues(&mut self) {
        self.refresh_apps_snapshot();
        self.request_file_scan();
    }

    /// Applies the newest scan batch, ignoring stale sequences and deleted apps.
    pub(crate) fn poll_file_scan(&mut self) {
        while let Ok(batch) = self.scan_rx.try_recv() {
            if batch.seq != self.scan_seq {
                continue;
            }
            apply_file_scan(&mut self.file_issues, &self.snapshot.apps, batch.issues);
        }
    }

    /// Starts the background storage scan (non-blocking).
    pub(crate) fn start_background_storage_scan(
        &mut self,
        ctx: &egui::Context,
        announce_if_clean: bool,
    ) {
        self.startup_scan_rx = Some(spawn_startup_scan(
            self.config.clone(),
            announce_if_clean,
            ctx.clone(),
        ));
    }

    /// Polls completion of background startup init, storage scan, and async token fetches.
    pub(crate) fn poll_background_startup(&mut self) {
        if let Some(rx) = &self.startup_init_rx {
            if let Ok(outcome) = rx.try_recv() {
                self.startup_init_rx = None;
                self.startup_init_done = true;
                if self.token_warning.is_none() {
                    self.token_warning = outcome.token_warning;
                }
                self.has_github_token = token_is_present(outcome.github_token.as_deref());
                if outcome.github_token.is_some() {
                    let _ = self.send_cmd(BackendCommand::SetGithubToken(outcome.github_token));
                }
            }
        }

        if let Some(rx) = &self.startup_scan_rx {
            if let Ok(outcome) = rx.try_recv() {
                self.startup_scan_rx = None;
                if let Some(msg) = outcome.status_message {
                    self.status = Some(msg);
                }
                self.refresh_apps_snapshot();
                self.refresh_file_issues();
            }
        }

        if let Some(rx) = &self.settings_token_rx {
            if let Ok(token) = rx.try_recv() {
                self.settings_token_rx = None;
                // Only populate if Settings is still open and user hasn't started typing.
                if self.show_settings && self.settings_form.github_token.is_empty() {
                    self.settings_form.github_token = token;
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CachedAppMeta {
    pub size: u64,
    pub added_at: i64,
    /// Pre-resolved `file://...` icon URI so rendering never touches the filesystem.
    pub icon_uri: Option<Arc<str>>,
    /// Pre-lowercased display name for zero-allocation search and sorting.
    pub name_lower: String,
    /// Pre-lowercased GitHub repository string for zero-allocation search.
    pub repo_lower: Option<String>,
}

fn resolve_icon_uri(app: &AppEntry) -> Option<Arc<str>> {
    let icon_file = app
        .icon_path
        .as_ref()
        .filter(|path| path.exists())
        .cloned()
        .or_else(|| {
            app.local_path.parent().and_then(|folder| {
                let p1 = folder.join("icon.png");
                if p1.exists() {
                    return Some(p1);
                }
                let p2 = folder.join("icon.svg");
                if p2.exists() {
                    return Some(p2);
                }
                None
            })
        })?;
    Some(Arc::from(format!("file://{}", icon_file.display())))
}

/// Cached view of `Config::applications`, rebuilt only when the config
/// generation changes. Rendering clones the cheap `Arc`s, never the app list.
#[derive(Debug, Default)]
pub(crate) struct AppSnapshotCache {
    pub apps: Arc<[AppEntry]>,
    pub meta: Arc<HashMap<String, CachedAppMeta>>,
    generation: u64,
}

impl AppSnapshotCache {
    pub(crate) fn new() -> Self {
        Self {
            apps: Arc::from([]),
            meta: Arc::new(HashMap::new()),
            // Force the first refresh.
            generation: u64::MAX,
        }
    }

    /// Rebuilds the snapshot for `generation`. Returns `true` when it changed.
    pub(crate) fn rebuild(&mut self, generation: u64, apps: Vec<AppEntry>) -> bool {
        if generation == self.generation {
            return false;
        }
        let meta = apps
            .iter()
            .map(|app| {
                let file_meta = app.local_path.metadata().ok();
                let size = file_meta
                    .as_ref()
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                let added_at = app.added_at.unwrap_or_else(|| {
                    file_meta
                        .as_ref()
                        .and_then(|metadata| {
                            metadata.created().or_else(|_| metadata.modified()).ok()
                        })
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|duration| duration.as_secs() as i64)
                        .unwrap_or(0)
                });
                let icon_uri = resolve_icon_uri(app);
                let name_lower = app.display_name.to_lowercase();
                let repo_lower = match &app.source {
                    AppSource::LocalWithRepo { github_repo }
                    | AppSource::GithubOnly { github_repo } => Some(github_repo.to_lowercase()),
                    AppSource::LocalOnly => None,
                };
                (
                    app.id.clone(),
                    CachedAppMeta {
                        size,
                        added_at,
                        icon_uri,
                        name_lower,
                        repo_lower,
                    },
                )
            })
            .collect();
        self.apps = apps.into();
        self.meta = Arc::new(meta);
        self.generation = generation;
        true
    }
}

/// Input for the background file scanner.
#[derive(Debug, Clone)]
pub(crate) struct FileScanSpec {
    id: String,
    path: PathBuf,
    fingerprint: Option<crate::config::FileFingerprint>,
}

/// Output of one background scan pass.
#[derive(Debug)]
pub(crate) struct FileScanBatch {
    seq: u64,
    issues: Vec<(String, LocalFileIssue)>,
}

pub(crate) fn apply_file_scan(
    issues: &mut HashMap<String, LocalFileIssue>,
    apps: &[AppEntry],
    scanned: Vec<(String, LocalFileIssue)>,
) {
    let live: HashSet<&str> = apps.iter().map(|app| app.id.as_str()).collect();
    issues.clear();
    for (id, issue) in scanned {
        if live.contains(id.as_str()) {
            issues.insert(id, issue);
        }
    }
}

/// Spawns the 1 Hz metadata-only scanner. It receives a snapshot of the app
/// list and reports issues without ever touching the UI thread or executing an
/// AppImage.
pub(crate) fn spawn_file_scanner() -> (
    std::sync::mpsc::Sender<Vec<FileScanSpec>>,
    std::sync::mpsc::Receiver<FileScanBatch>,
) {
    let (request_tx, request_rx) = std::sync::mpsc::channel::<Vec<FileScanSpec>>();
    let (result_tx, result_rx) = std::sync::mpsc::channel::<FileScanBatch>();
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;

        let mut specs: Vec<FileScanSpec> = Vec::new();
        let mut seq: u64 = 0;
        loop {
            match request_rx.recv_timeout(Duration::from_secs(1)) {
                Ok(first) => {
                    let mut latest = first;
                    while let Ok(next) = request_rx.try_recv() {
                        latest = next;
                    }
                    specs = latest;
                    seq = seq.wrapping_add(1);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let issues = specs
                .iter()
                .filter_map(|spec| {
                    cheap_issue_for_file(&spec.path, spec.fingerprint.as_ref())
                        .map(|issue| (spec.id.clone(), issue))
                })
                .collect();
            if result_tx.send(FileScanBatch { seq, issues }).is_err() {
                return;
            }
        }
    });
    (request_tx, result_rx)
}
