use anyhow::{Context, Result};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;

/// The temporary path used while a download is in flight. Lives next to
/// the target so `rename()` stays on the same filesystem (atomic on Linux).
pub fn temp_path_for(target: &Path) -> PathBuf {
    let mut p = target.to_path_buf();
    let name = p
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    p.set_file_name(format!("{name}.new"));
    p
}

/// Atomically swap the freshly downloaded file over the managed AppImage.
/// Safe on Linux even if the AppImage is currently running — we swap the
/// directory entry, not the inode being executed.
pub fn atomic_replace(downloaded: &Path, target: &Path) -> Result<()> {
    std::fs::rename(downloaded, target)
        .with_context(|| format!("failed to replace {}", target.display()))?;
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
    mut on_progress: F,
) -> Result<()>
where
    F: FnMut(DownloadProgressInfo),
{
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

    let total = resp.content_length();
    let mut file = tokio::fs::File::create(dest)
        .await
        .with_context(|| format!("failed to create {}", dest.display()))?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    on_progress(DownloadProgressInfo {
        downloaded: 0,
        total,
        fraction: 0.0,
    });

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            anyhow::bail!("download cancelled");
        }
        let chunk = chunk.context("failed while reading download stream")?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed writing {}", dest.display()))?;
        downloaded += chunk.len() as u64;
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
        drop(file);
        let _ = tokio::fs::remove_file(dest).await;
        anyhow::bail!("download cancelled");
    }

    file.flush()
        .await
        .with_context(|| format!("failed flushing {}", dest.display()))?;
    on_progress(DownloadProgressInfo {
        downloaded,
        total,
        fraction: 1.0,
    });
    Ok(())
}

#[allow(dead_code)]
pub async fn download_to_path(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    let cancel = Arc::new(AtomicBool::new(false));
    download_to_path_with_progress(client, url, dest, cancel, |_| {}).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions;
    use std::fs;

    #[test]
    fn temp_path_is_sibling_new_file() {
        let target = PathBuf::from("/tmp/apps/tool.AppImage");
        let temp = temp_path_for(&target);
        assert_eq!(temp, PathBuf::from("/tmp/apps/tool.AppImage.new"));
        assert_eq!(temp.parent(), target.parent());
    }

    #[test]
    fn atomic_replace_swaps_file_contents() -> Result<()> {
        let dir = std::env::temp_dir().join(format!("aim-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let target = dir.join("app.AppImage");
        let temp = dir.join("app.AppImage.new");
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
        let dir = std::env::temp_dir().join(format!("aim-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&target)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }

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
        let dir = std::env::temp_dir().join(format!("aim-dl-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir)?;
        let target = dir.join("linuxdeploy.AppImage");
        fs::write(&target, b"old")?;
        let temp = temp_path_for(&target);

        download_to_path(&client, &release.download_url, &temp).await?;
        let meta = fs::metadata(&temp)?;
        assert!(meta.len() > 0, "downloaded file should not be empty");

        permissions::set_executable(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&temp)?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o755);
        }

        atomic_replace(&temp, &target)?;
        assert!(!temp.exists());
        assert_eq!(fs::metadata(&target)?.len(), meta.len());

        fs::remove_dir_all(&dir)?;
        Ok(())
    }
}
