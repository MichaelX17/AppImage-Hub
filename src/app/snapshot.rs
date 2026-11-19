//! Cached app view and the background file scanner.
use super::*;
use crate::scan::cheap_issue_for_file;
impl ManagerApp {
    /// Recomputes offline file issues (missing/changed AppImages) for every
    /// app. Cheap: metadata only, no content hashing.
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

    /// Runs the deferred startup work after the first frame has been painted.
    pub(crate) fn run_startup_scan(&mut self) {
        self.repair_missing_icons();
        self.normalize_managed_filenames();
        let report = self.sync_metadata_files();
        let mut message = "Storage scan complete.".to_string();
        if report.adopted > 0 {
            message.push_str(&format!(". Found {} existing app(s)", report.adopted));
        }
        if let Some(note) = report.note() {
            message.push_str(&format!(". {note}"));
        }
        self.status = Some(message);
        self.refresh_apps_snapshot();
        self.refresh_file_issues();
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CachedAppMeta {
    pub size: u64,
    pub added_at: i64,
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
                (app.id.clone(), CachedAppMeta { size, added_at })
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
                    // Coalesce requests that arrived while scanning.
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
