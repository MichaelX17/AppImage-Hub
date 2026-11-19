//! Integration coverage for the failure modes fixed in steps 1–8.
//!
//! These tests use only the public library modules and never touch the network.

use appimage_hub::config::{AppEntry, AppSource, Config, FileFingerprint};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aim-it-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sample_entry(path: PathBuf, categories: Option<String>) -> AppEntry {
    AppEntry {
        id: "app-id".into(),
        display_name: "Example".into(),
        source: AppSource::LocalOnly,
        local_path: path,
        current_version: "v1".into(),
        args: Vec::new(),
        icon_path: None,
        generic_name: None,
        comment: None,
        categories,
        startup_notify: true,
        asset_preference: None,
        added_at: None,
        commit_hash: None,
        file_fingerprint: Some(FileFingerprint {
            size: 1,
            mtime: 1,
            sha256: None,
        }),
        desktop_shortcut: true,
        menu_entry: true,
        mime_types: None,
        exec_arg: None,
    }
}

#[test]
fn corrupt_config_is_quarantined() {
    let dir = temp_dir("config");
    let path = dir.join("config.toml");
    std::fs::write(&path, b"not = valid = toml [[[").unwrap();

    let error = Config::load_from(&path).expect_err("corrupt config must fail");
    assert!(format!("{error:#}").contains("preserved at"));
    assert!(!path.exists());

    let quarantined = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .find(|name| name.contains(".corrupt-"));
    assert!(quarantined.is_some(), "quarantine file must exist");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn delete_from_custom_root_preserves_unrelated_files() {
    let root = temp_dir("custom-root");
    let folder = root.join("example");
    std::fs::create_dir_all(&folder).unwrap();
    let appimage = folder.join("example.appimage");
    std::fs::write(&appimage, b"binary").unwrap();
    std::fs::write(folder.join("example.json"), b"{}").unwrap();
    std::fs::write(folder.join("unrelated.txt"), b"keep").unwrap();
    let entry = sample_entry(appimage.clone(), None);
    let roots = vec![appimage_hub::storage::normalize_path(&root)];

    let outcome = appimage_hub::storage::delete_managed_entry(&entry, &roots);

    assert!(!outcome.skipped_outside_root);
    assert!(!appimage.exists());
    assert!(!folder.join("example.json").exists());
    assert!(
        folder.join("unrelated.txt").exists(),
        "unrelated file survives"
    );
    assert!(folder.exists(), "non-empty folder is kept");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn args_survive_edit_and_launch_round_trip() {
    let stored = vec![
        "--profile=My Profile".to_string(),
        "a b".to_string(),
        String::new(),
        "c:\\temp".to_string(),
        "say \"hi\"".to_string(),
    ];
    let displayed = appimage_hub::args::join_args(&stored);
    let parsed = appimage_hub::args::split_args(&displayed).unwrap();
    assert_eq!(parsed, stored);
    assert!(appimage_hub::args::split_args("a \"unterminated").is_err());
}

#[test]
fn desktop_categories_cannot_inject_keys() {
    let entry = sample_entry(
        PathBuf::from("/tmp/example.appimage"),
        Some("Utility;\nExec=sh -c evil".into()),
    );
    let content = appimage_hub::desktop_entry::desktop_file_content(&entry);
    assert_eq!(
        content
            .lines()
            .filter(|line| line.starts_with("Exec="))
            .count(),
        1
    );
    assert!(!content.lines().any(|line| line.starts_with("Exec=sh")));
    assert!(content.contains("Categories=Utility;\n"));
}

#[test]
fn update_check_timeout_expires() {
    let now = Instant::now();
    let mut progress =
        appimage_hub::app::UpdateCheckProgress::start(vec!["a".to_string()], |_| true, now)
            .expect("progress");
    let deadline =
        now + appimage_hub::app::UpdateCheckProgress::timeout_for(1) + Duration::from_secs(1);
    assert!(matches!(
        progress.tick(0.1, deadline),
        appimage_hub::app::UpdateCheckTick::TimedOut { remaining: 1 }
    ));
}

#[tokio::test]
async fn truncated_download_leaves_no_temp_file() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut request = [0_u8; 2048];
            let _ = socket.read(&mut request).await;
            let _ = socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: close\r\n\r\n\x7fELFxxxx",
                )
                .await;
            let _ = socket.shutdown().await;
        }
    });

    let dir = temp_dir("download");
    let target = dir.join("app.AppImage");
    std::fs::write(&target, b"old").unwrap();
    let temp = appimage_hub::downloader::temp_path_for(&target);

    let result = appimage_hub::downloader::download_to_path_with_progress(
        &reqwest::Client::new(),
        &format!("http://{addr}/app"),
        &temp,
        Arc::new(AtomicBool::new(false)),
        |_| {},
    )
    .await;

    assert!(result.is_err());
    assert!(!temp.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn storage_containment_rejects_root_and_siblings() {
    let root = temp_dir("containment");
    let roots = vec![appimage_hub::storage::normalize_path(&root)];
    assert!(!appimage_hub::storage::is_in_storage_root(&root, &roots));
    assert!(appimage_hub::storage::is_in_storage_root(
        &root.join("app").join("app.appimage"),
        &roots
    ));
    assert!(!appimage_hub::storage::is_in_storage_root(
        &Path::new(&format!("{}-evil", root.display())).join("app.appimage"),
        &roots
    ));
    let _ = std::fs::remove_dir_all(root);
}
