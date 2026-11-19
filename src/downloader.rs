//! Bounded, verified downloads for managed AppImages and AppImage Hub updates.
//!
//! A download writes to a random sibling temp file opened with `create_new`
//! (and `O_NOFOLLOW` on Unix), is bounded by [`MAX_DOWNLOAD_BYTES`], must match
//! its `Content-Length`, is fsynced, and must start with an ELF header. Any
//! failure removes the temp file, so a partial download can never be renamed
//! over a working AppImage.

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncWriteExt, BufWriter};

/// Upper bound for a single download. AppImages are large but not multi-GiB;
/// this prevents a broken or malicious server from filling the disk.
pub const MAX_DOWNLOAD_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Bound for fetching a small checksum file.
const CHECKSUM_TIMEOUT: Duration = Duration::from_secs(30);

/// The temporary path used while a download is in flight.
///
/// It lives next to the target so `rename()` stays on the same filesystem, and
/// carries a random suffix so two concurrent transfers (or a retry racing a
/// stale file) can never write through the same path.
pub fn temp_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    target.with_file_name(format!("{name}.part-{}", uuid::Uuid::new_v4().simple()))
}

/// Atomically swap the freshly downloaded file over the managed AppImage.
/// Safe on Linux even if the AppImage is currently running — we swap the
/// directory entry, not the inode being executed.
pub fn atomic_replace(downloaded: &Path, target: &Path) -> Result<()> {
    std::fs::rename(downloaded, target)
        .with_context(|| format!("failed to replace {}", target.display()))?;
    crate::fsutil::sync_parent_dir(target);
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgressInfo {
    pub downloaded: u64,
    pub total: Option<u64>,
    pub fraction: f32,
}

pub async fn download_to_path_with_progress<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: Arc<AtomicBool>,
    on_progress: F,
) -> Result<()>
where
    F: FnMut(DownloadProgressInfo),
{
    download_to_path_with_progress_limited(
        client,
        url,
        dest,
        cancel,
        MAX_DOWNLOAD_BYTES,
        on_progress,
    )
    .await
}

#[cfg(test)]
pub async fn download_to_path(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    download_to_path_with_progress(client, url, dest, cancel, |_| {}).await
}

/// Same as [`download_to_path_with_progress`] with an explicit byte limit.
/// Exposed for tests; production callers use [`MAX_DOWNLOAD_BYTES`].
pub(crate) async fn download_to_path_with_progress_limited<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: Arc<AtomicBool>,
    max_bytes: u64,
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(DownloadProgressInfo),
{
    let result =
        download_to_path_inner(client, url, dest, &cancel, max_bytes, &mut on_progress).await;
    if result.is_err() {
        // A failed, timed-out, cancelled, oversized, or non-ELF download never
        // leaves a partial temp file behind for a later `rename` to pick up.
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

async fn download_to_path_inner<F>(
    client: &reqwest::Client,
    url: &str,
    dest: &Path,
    cancel: &AtomicBool,
    max_bytes: u64,
    on_progress: &mut F,
) -> Result<()>
where
    F: FnMut(DownloadProgressInfo),
{
    // Cancellation takes effect at chunk boundaries: the read timeout bounds
    // how long a stalled stream can keep the job alive between checks.
    if cancel.load(Ordering::Relaxed) {
        bail!("download cancelled");
    }

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to start download: {url}"))?
        .error_for_status()
        .with_context(|| format!("download request failed: {url}"))?;

    if cancel.load(Ordering::Relaxed) {
        bail!("download cancelled");
    }

    let total = resp.content_length();
    if let Some(total) = total {
        if total > max_bytes {
            bail!("download is too large: {total} bytes exceeds the {max_bytes} byte limit");
        }
    }

    let file = create_temp_file(dest).await?;
    let mut writer = BufWriter::new(file);
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    on_progress(DownloadProgressInfo {
        downloaded: 0,
        total,
        fraction: 0.0,
    });

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            bail!("download cancelled");
        }
        let chunk = chunk.context("failed while reading download stream")?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| anyhow!("download size overflow"))?;
        if downloaded > max_bytes {
            bail!("download is too large: exceeded the {max_bytes} byte limit while streaming");
        }
        writer
            .write_all(&chunk)
            .await
            .with_context(|| format!("failed writing {}", dest.display()))?;
        let fraction = total
            .map(|t| (downloaded as f32 / t.max(1) as f32).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        on_progress(DownloadProgressInfo {
            downloaded,
            total,
            fraction,
        });
    }

    if cancel.load(Ordering::Relaxed) {
        bail!("download cancelled");
    }

    if let Some(total) = total {
        if downloaded != total {
            bail!("download incomplete: expected {total} bytes, got {downloaded}");
        }
    }

    writer
        .flush()
        .await
        .with_context(|| format!("failed flushing {}", dest.display()))?;
    let file = writer.into_inner();
    file.sync_all()
        .await
        .with_context(|| format!("failed syncing {}", dest.display()))?;
    drop(file);

    ensure_elf_header(dest)?;

    on_progress(DownloadProgressInfo {
        downloaded,
        total,
        fraction: 1.0,
    });
    Ok(())
}

/// Opens the temp file without following a planted symlink.
async fn create_temp_file(path: &Path) -> Result<tokio::fs::File> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options
        .open(path)
        .await
        .with_context(|| format!("failed to create {}", path.display()))
}

/// True when `path` starts with the ELF magic. A sanity check, not a signature
/// or security verification.
pub fn is_elf_file(path: &Path) -> bool {
    use std::io::Read as _;

    let mut header = [0_u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok()
        && &header == b"\x7fELF"
}

/// A downloaded AppImage must at least look like an ELF executable.
fn ensure_elf_header(path: &Path) -> Result<()> {
    if !is_elf_file(path) {
        bail!("downloaded file is not an AppImage (missing ELF header)");
    }
    Ok(())
}

/// Downloads a checksum file (usually `<asset>.sha256`).
pub async fn fetch_checksum(client: &reqwest::Client, url: &str) -> Result<String> {
    client
        .get(url)
        .timeout(CHECKSUM_TIMEOUT)
        .send()
        .await
        .with_context(|| format!("failed to fetch checksum: {url}"))?
        .error_for_status()
        .with_context(|| format!("checksum request failed: {url}"))?
        .text()
        .await
        .with_context(|| format!("failed to read checksum: {url}"))
}

/// Extracts the first 64-hex-character token from a checksum file. Accepts
/// both `sha256sum` output (`<hex>  <file>`) and a bare digest.
pub fn parse_sha256(text: &str) -> Option<String> {
    let token = text.split_whitespace().next()?;
    if token.len() < 64 {
        return None;
    }
    let digest: String = token.chars().take(64).collect();
    digest
        .chars()
        .all(|character| character.is_ascii_hexdigit())
        .then(|| digest.to_ascii_lowercase())
}

/// Compares the file's SHA-256 with `expected` (hex, case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let actual = crate::config::sha256_file(path)
        .ok_or_else(|| anyhow!("failed to hash {}", path.display()))?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("SHA-256 mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

/// Removes abandoned `*.part-*` / legacy `*.new` files under `roots` that are
/// older than `max_age`. Never touches `.appimage` files or symlinks.
pub fn cleanup_stale_temp_files(roots: &[PathBuf], max_age: Duration) -> usize {
    let now = std::time::SystemTime::now();
    let mut removed = 0;
    for root in roots {
        removed += cleanup_dir(root, 0, now, max_age);
    }
    removed
}

fn cleanup_dir(dir: &Path, depth: u8, now: std::time::SystemTime, max_age: Duration) -> usize {
    if depth > 4 {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        // Never follow symlinks (a link is not a directory and is not deleted
        // unless it is itself a stale temp name).
        if file_type.is_dir() {
            removed += cleanup_dir(&entry.path(), depth + 1, now, max_age);
            continue;
        }
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".appimage") {
            continue;
        }
        let is_temp = name.contains(".part-") || name.ends_with(".new");
        if !is_temp {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= max_age);
        if stale && std::fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions;
    use std::fs;
    use std::net::SocketAddr;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aim-dl-{tag}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Serves one raw HTTP response and closes the connection.
    async fn serve_once(response: Vec<u8>) -> SocketAddr {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                // Drain the request first; answering before the client has
                // finished sending can reset the connection.
                let mut request = [0_u8; 4096];
                let _ = socket.read(&mut request).await;
                let _ = socket.write_all(&response).await;
                let _ = socket.shutdown().await;
            }
        });
        addr
    }

    fn http_ok_with_length(body: &[u8]) -> Vec<u8> {
        let mut response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        response.extend_from_slice(body);
        response
    }

    fn elf_payload() -> Vec<u8> {
        let mut body = b"\x7fELF".to_vec();
        body.extend_from_slice(&[0_u8; 12]);
        body
    }

    #[test]
    fn temp_path_is_sibling_part_file() {
        let target = PathBuf::from("/tmp/apps/tool.AppImage");
        let temp = temp_path_for(&target);
        assert_eq!(temp.parent(), target.parent());
        let name = temp.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("tool.AppImage.part-"), "got {name}");
        assert_ne!(temp, target);
    }

    #[test]
    fn atomic_replace_swaps_file_contents() -> Result<()> {
        let dir = temp_dir("replace");
        let target = dir.join("app.AppImage");
        let temp = dir.join("app.AppImage.part-test");
        fs::write(&target, b"old")?;
        fs::write(&temp, b"new")?;

        atomic_replace(&temp, &target)?;

        assert!(!temp.exists(), "temp file should be moved away");
        assert_eq!(fs::read(&target)?, b"new");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn chmod_and_replace_workflow() -> Result<()> {
        let dir = temp_dir("chmod");
        let target = dir.join("app.AppImage");
        let temp = temp_path_for(&target);
        fs::write(&target, b"old")?;
        fs::write(&temp, b"#!/bin/sh\necho hi\n")?;

        permissions::set_executable(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&temp)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }

        atomic_replace(&temp, &target)?;
        assert_eq!(fs::read(&target)?, b"#!/bin/sh\necho hi\n");
        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_oversized_content_length() -> Result<()> {
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 4000000000\r\nConnection: close\r\n\r\n".to_vec();
        let addr = serve_once(response).await;
        let dir = temp_dir("oversize-header");
        let dest = dir.join("app.part");

        let error = download_to_path_with_progress(
            &reqwest::Client::new(),
            &format!("http://{addr}/app"),
            &dest,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .expect_err("oversized Content-Length must be rejected");

        assert!(
            format!("{error:#}").contains("too large"),
            "unexpected error: {error:#}"
        );
        assert!(!dest.exists());

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn aborts_when_stream_exceeds_limit() -> Result<()> {
        // HTTP/1.0 without Content-Length: the body length is only known by
        // closing the connection, so the streaming limit has to catch it.
        let mut response = b"HTTP/1.0 200 OK\r\n\r\n".to_vec();
        response.extend_from_slice(&elf_payload());
        response.extend_from_slice(&[b'x'; 64]);
        let addr = serve_once(response).await;
        let dir = temp_dir("oversize-stream");
        let dest = dir.join("app.part");

        let error = download_to_path_with_progress_limited(
            &reqwest::Client::new(),
            &format!("http://{addr}/app"),
            &dest,
            Arc::new(AtomicBool::new(false)),
            8,
            |_| {},
        )
        .await
        .expect_err("streaming limit must abort the download");

        assert!(
            format!("{error:#}").contains("too large"),
            "unexpected error: {error:#}"
        );
        assert!(!dest.exists());

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn truncated_response_does_not_replace_target() -> Result<()> {
        // Promises 32 bytes, sends 8, then closes.
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: close\r\n\r\n".to_vec();
        response.extend_from_slice(b"\x7fELFxxxx");
        let addr = serve_once(response).await;
        let dir = temp_dir("truncated");
        let target = dir.join("app.AppImage");
        fs::write(&target, b"old working binary")?;
        let temp = temp_path_for(&target);

        let error = download_to_path_with_progress(
            &reqwest::Client::new(),
            &format!("http://{addr}/app"),
            &temp,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .expect_err("a truncated response must fail");

        assert!(!temp.exists(), "partial file must be removed");
        assert_eq!(fs::read(&target)?, b"old working binary");
        let _ = error;

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn non_elf_payload_is_rejected() -> Result<()> {
        let addr = serve_once(http_ok_with_length(b"notelf")).await;
        let dir = temp_dir("notelf");
        let dest = dir.join("app.part");

        let error = download_to_path_with_progress(
            &reqwest::Client::new(),
            &format!("http://{addr}/app"),
            &dest,
            Arc::new(AtomicBool::new(false)),
            |_| {},
        )
        .await
        .expect_err("non-ELF payloads must be rejected");

        assert!(
            format!("{error:#}").contains("missing ELF header"),
            "unexpected error: {error:#}"
        );
        assert!(!dest.exists());

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn temp_file_is_not_symlink_followed() -> Result<()> {
        let dir = temp_dir("symlink");
        let victim = dir.join("victim.txt");
        fs::write(&victim, b"original")?;
        let temp = dir.join("app.part");
        std::os::unix::fs::symlink(&victim, &temp)?;

        let result = create_temp_file(&temp).await;

        assert!(result.is_err(), "create_new must refuse a planted symlink");
        assert_eq!(fs::read(&victim)?, b"original");
        assert!(temp.exists(), "the symlink itself is left untouched");

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn stale_temp_files_are_cleaned() -> Result<()> {
        let root = temp_dir("cleanup");
        let app = root.join("app.appimage");
        let stale_part = root.join("app.appimage.part-old");
        let stale_new = root.join("app.appimage.new");
        let fresh_part = root.join("other.appimage.part-fresh");
        for path in [&app, &stale_part, &stale_new, &fresh_part] {
            fs::write(path, b"x")?;
        }
        let old = std::time::SystemTime::now() - Duration::from_secs(2 * 60 * 60);
        for path in [&stale_part, &stale_new] {
            let file = fs::OpenOptions::new().write(true).open(path)?;
            file.set_times(fs::FileTimes::new().set_modified(old))?;
        }

        let removed =
            cleanup_stale_temp_files(std::slice::from_ref(&root), Duration::from_secs(60 * 60));

        assert_eq!(removed, 2);
        assert!(app.exists(), "AppImages are never touched");
        assert!(fresh_part.exists(), "fresh temp files are kept");
        assert!(!stale_part.exists());
        assert!(!stale_new.exists());

        fs::remove_dir_all(&root)?;
        Ok(())
    }

    #[test]
    fn checksum_mismatch_aborts() -> Result<()> {
        let dir = temp_dir("checksum-mismatch");
        let path = dir.join("app.appimage");
        fs::write(&path, b"\x7fELF payload")?;
        let wrong = "0".repeat(64);

        let error = verify_sha256(&path, &wrong).expect_err("wrong digest must fail");
        assert!(format!("{error:#}").contains("SHA-256 mismatch"));

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[test]
    fn checksum_match_allows_install() -> Result<()> {
        let dir = temp_dir("checksum-match");
        let path = dir.join("app.appimage");
        fs::write(&path, b"\x7fELF payload")?;
        let expected = crate::config::sha256_file(&path).unwrap();

        verify_sha256(&path, &expected)?;
        // `sha256sum` output style also parses.
        assert_eq!(
            parse_sha256(&format!("{expected}  app.appimage\n")),
            Some(expected)
        );

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    async fn progress_reports_unknown_total() -> Result<()> {
        let body = elf_payload();
        let split = 5;
        let mut response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        for chunk in [&body[..split], &body[split..]] {
            response.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
            response.extend_from_slice(chunk);
            response.extend_from_slice(b"\r\n");
        }
        response.extend_from_slice(b"0\r\n\r\n");
        let addr = serve_once(response).await;
        let dir = temp_dir("unknown-total");
        let dest = dir.join("app.part");
        let totals: Arc<std::sync::Mutex<Vec<Option<u64>>>> = Arc::new(Default::default());
        let fractions: Arc<std::sync::Mutex<Vec<f32>>> = Arc::new(Default::default());
        let totals_cb = totals.clone();
        let fractions_cb = fractions.clone();

        download_to_path_with_progress(
            &reqwest::Client::new(),
            &format!("http://{addr}/app"),
            &dest,
            Arc::new(AtomicBool::new(false)),
            move |progress| {
                totals_cb.lock().unwrap().push(progress.total);
                fractions_cb.lock().unwrap().push(progress.fraction);
            },
        )
        .await?;

        assert!(dest.exists());
        assert!(totals.lock().unwrap().iter().all(|total| total.is_none()));
        assert_eq!(fractions.lock().unwrap().last().copied(), Some(1.0));

        fs::remove_dir_all(&dir)?;
        Ok(())
    }

    #[tokio::test]
    #[ignore = "requires GitHub network access"]
    async fn download_public_appimage_and_replace() -> Result<()> {
        crate::github::ensure_crypto_provider();
        let octo = octocrab::Octocrab::builder().build()?;
        // linuxdeploy ships relatively small AppImages and is stable/public.
        let release = crate::github::fetch_latest_release(&octo, "linuxdeploy/linuxdeploy").await?;

        let client = reqwest::Client::builder()
            .user_agent("appimage-manager-tests")
            .build()?;
        let dir = temp_dir("network");
        let target = dir.join("linuxdeploy.AppImage");
        fs::write(&target, b"old")?;
        let temp = temp_path_for(&target);

        download_to_path(&client, &release.download_url, &temp).await?;
        let meta = fs::metadata(&temp)?;
        assert!(meta.len() > 0, "downloaded file should not be empty");

        permissions::set_executable(&temp)?;
        atomic_replace(&temp, &target)?;
        assert!(!temp.exists());
        assert_eq!(fs::metadata(&target)?.len(), meta.len());

        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
