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
    assert_eq!(update_check_cooldown_secs(true), 60);
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
fn hub_update_robust_digest_comparison() {
    // Hashes match (case insensitive, with or without sha256: prefix)
    assert!(!hub_update_is_available_robust(
        Some("d1dd5380924106a1f67f3e4390b24b6aa222506f118c939bdbd53355f884e139"),
        Some("sha256:D1DD5380924106A1F67F3E4390B24B6AA222506F118C939BDBD53355F884E139"),
        None,
        12345,
        10000,
        Some(10000),
    ));

    // Hashes differ
    assert!(hub_update_is_available_robust(
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Some("sha256:d1dd5380924106a1f67f3e4390b24b6aa222506f118c939bdbd53355f884e139"),
        None,
        12345,
        10000,
        Some(10000),
    ));
}

#[test]
fn hub_update_robust_fallback_comparison() {
    let saved_state = crate::config::HubUpdateState {
        asset_id: 12345,
        asset_size: 98765,
        updated_at: Some("2026-11-20T15:25:18Z".into()),
    };

    // Digest is None, but saved id and size match remote and local file size -> up to date
    assert!(!hub_update_is_available_robust(
        None,
        None,
        Some(&saved_state),
        12345,
        98765,
        Some(98765),
    ));

    // Digest is None, remote size changed -> update available
    assert!(hub_update_is_available_robust(
        None,
        None,
        Some(&saved_state),
        12345,
        99999,
        Some(98765),
    ));

    // Digest is None, remote asset_id changed -> update available
    assert!(hub_update_is_available_robust(
        None,
        None,
        Some(&saved_state),
        54321,
        98765,
        Some(98765),
    ));

    // Neither digest nor saved state -> update available
    assert!(hub_update_is_available_robust(
        None,
        None,
        None,
        12345,
        98765,
        Some(98765),
    ));
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
        startup_wm_class: None,
        startup_wm_class_auto: None,
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

fn started_update_check(ids: &[&str], now: Instant) -> UpdateCheckProgress {
    UpdateCheckProgress::start(
        ids.iter().map(|id| (*id).to_string()).collect(),
        |_| true,
        now,
    )
    .expect("at least one check should be queued")
}

#[test]
fn update_check_progress_animates_slowly_toward_target() {
    let now = Instant::now();
    let mut progress = started_update_check(&["a", "b"], now);

    progress.mark_finished("a");
    assert_eq!(progress.tick(1.0, now), UpdateCheckTick::Running);
    assert!((progress.fraction() - UpdateCheckProgress::FILL_PER_SECOND).abs() < f32::EPSILON);

    progress.mark_finished("b");
    assert_eq!(progress.tick(1.0, now), UpdateCheckTick::Running);
    assert!(
        (progress.fraction() - 2.0 * UpdateCheckProgress::FILL_PER_SECOND).abs() < f32::EPSILON
    );
    assert_eq!(progress.tick(0.5, now), UpdateCheckTick::Running);
    assert_eq!(progress.tick(0.5, now), UpdateCheckTick::Complete);
    assert!((progress.fraction() - 1.0).abs() < f32::EPSILON);
}

#[test]
fn update_check_completes_when_all_items_finish() {
    let now = Instant::now();
    let mut progress = started_update_check(&["a", "b"], now);
    progress.mark_finished("a");
    progress.mark_finished("b");

    let mut outcome = UpdateCheckTick::Running;
    for _ in 0..100 {
        outcome = progress.tick(0.1, now);
        if outcome == UpdateCheckTick::Complete {
            break;
        }
    }
    assert_eq!(outcome, UpdateCheckTick::Complete);
}

#[test]
fn update_check_expires_when_an_item_never_reports() {
    let now = Instant::now();
    let mut progress = started_update_check(&["a", "b"], now);
    progress.mark_finished("a");

    let past_deadline = now + UpdateCheckProgress::timeout_for(2) + Duration::from_secs(1);
    assert_eq!(
        progress.tick(0.1, past_deadline),
        UpdateCheckTick::TimedOut { remaining: 1 }
    );
}

#[test]
fn deleting_an_app_removes_it_from_pending() {
    let now = Instant::now();
    let mut progress = started_update_check(&["a", "b"], now);

    progress.mark_finished("a");

    assert!(!progress.pending.contains("a"));
    assert!(progress.pending.contains("b"));
    assert_eq!(progress.tick(0.1, now), UpdateCheckTick::Running);
}

#[test]
fn failed_send_does_not_add_pending_item() {
    let progress = UpdateCheckProgress::start(
        vec!["a".to_string(), "b".to_string()],
        |_| false,
        Instant::now(),
    );
    assert!(progress.is_none());
}

#[test]
fn open_edit_round_trip_preserves_args() {
    // What `open_edit` shows and what `submit_side_panel_form` parses must
    // reproduce the stored vector exactly.
    let stored = vec![
        "--profile=My Profile".to_string(),
        "a b".to_string(),
        String::new(),
        "c:\\temp".to_string(),
        "say \"hi\"".to_string(),
    ];
    let displayed = crate::args::join_args(&stored);
    assert_eq!(crate::args::split_args(&displayed).unwrap(), stored);
}

#[test]
fn snapshot_is_rebuilt_only_when_dirty() {
    let mut cache = AppSnapshotCache::new();
    let app = fingerprint_test_entry(PathBuf::from("/tmp/example.appimage"));

    assert!(cache.rebuild(1, vec![app.clone()]));
    assert_eq!(cache.apps.len(), 1);

    // Same generation: no rebuild, even if the provided list differs.
    assert!(!cache.rebuild(1, Vec::new()));
    assert_eq!(cache.apps.len(), 1);

    assert!(cache.rebuild(2, Vec::new()));
    assert!(cache.apps.is_empty());
}

#[test]
fn scanner_detects_missing_and_changed_files() {
    let dir = std::env::temp_dir().join(format!("aim-scan-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("app.appimage");

    assert_eq!(
        cheap_issue_for_file(&path, None),
        Some(LocalFileIssue::Missing)
    );

    std::fs::write(&path, b"binary").unwrap();
    let fingerprint = crate::config::file_fingerprint(&path);
    assert_eq!(cheap_issue_for_file(&path, fingerprint.as_ref()), None);

    std::fs::write(&path, b"a longer binary").unwrap();
    assert_eq!(
        cheap_issue_for_file(&path, fingerprint.as_ref()),
        Some(LocalFileIssue::Changed)
    );

    std::fs::remove_file(&path).unwrap();
    assert_eq!(
        cheap_issue_for_file(&path, fingerprint.as_ref()),
        Some(LocalFileIssue::Missing)
    );

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scanner_result_applies_without_cloning_apps() {
    let app = fingerprint_test_entry(PathBuf::from("/tmp/example.appimage"));
    let apps = [app.clone()];
    let mut issues = HashMap::new();
    issues.insert("stale-id".to_string(), LocalFileIssue::Missing);

    apply_file_scan(
        &mut issues,
        &apps,
        vec![(app.id.clone(), LocalFileIssue::Changed)],
    );

    assert_eq!(issues.get(&app.id), Some(&LocalFileIssue::Changed));
    assert!(!issues.contains_key("stale-id"));
}

#[test]
fn stale_scan_result_for_deleted_app_is_ignored() {
    let app = fingerprint_test_entry(PathBuf::from("/tmp/example.appimage"));
    let apps = [app.clone()];
    let mut issues = HashMap::new();

    apply_file_scan(
        &mut issues,
        &apps,
        vec![("deleted-app".to_string(), LocalFileIssue::Missing)],
    );

    assert!(issues.is_empty());
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
