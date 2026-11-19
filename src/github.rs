use anyhow::{Context, Result};
use octocrab::Octocrab;
use std::sync::Once;

static CRYPTO_INIT: Once = Once::new();

pub fn ensure_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag: String,
    pub download_url: String,
    pub commit_hash: String,
    /// Size reported by the GitHub release, used as an early sanity bound.
    pub asset_size: u64,
    /// `<asset>.sha256` / `<asset>.sha256sum` published in the same release.
    pub checksum_url: Option<String>,
}

/// True when `checksum_name` is the published digest for `asset_name`.
fn is_checksum_for_asset(checksum_name: &str, asset_name: &str) -> bool {
    let lower = checksum_name.to_lowercase();
    lower == format!("{}.sha256", asset_name.to_lowercase())
        || lower == format!("{}.sha256sum", asset_name.to_lowercase())
}

fn normalize_repo(repo: &str) -> Result<(String, String)> {
    let mut s = repo.trim().trim_end_matches('/').to_string();
    if s.is_empty() {
        anyhow::bail!("repo must be in 'owner/repo' format");
    }
    for prefix in ["https://github.com/", "http://github.com/", "github.com/"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    let s = s.trim_end_matches('/');
    let s = s.strip_suffix(".git").unwrap_or(s);
    let s = s.split("/releases").next().unwrap_or(s);
    let mut parts = s.split('/').filter(|p| !p.is_empty());
    let owner = parts
        .next()
        .context("repo must be in 'owner/repo' format")?;
    let name = parts
        .next()
        .context("repo must be in 'owner/repo' format")?;
    Ok((owner.to_string(), name.to_string()))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GithubAssetInfo {
    pub name: String,
    pub size: u64,
    pub download_url: String,
}

/// Determines if an asset filename is a genuine AppImage executable binary,
/// excluding checksums, signatures, blockmaps, and update metadata files.
pub fn is_valid_appimage_asset(name: &str) -> bool {
    let lower = name.to_lowercase();

    // Must end with .appimage
    if !lower.ends_with(".appimage") {
        return false;
    }

    // Must not be a hash, checksum, or signature file
    let invalid_suffixes = [
        ".zsync",
        ".sha256",
        ".sha512",
        ".sha1",
        ".md5",
        ".asc",
        ".sig",
        ".signature",
        ".torrent",
        ".txt",
        ".json",
        ".yaml",
        ".yml",
        ".blockmap",
    ];
    for suffix in &invalid_suffixes {
        if lower.ends_with(suffix) {
            return false;
        }
    }

    true
}

/// Scores an AppImage asset filename based on how well it matches the target system architecture
/// and penalizes unwanted artifacts (e.g. debug builds, source tarballs).
pub fn score_asset(name: &str, target_arch: &str) -> i32 {
    let lower = name.to_lowercase();
    let mut score = 100;

    // Check architecture matching
    match target_arch {
        "x86_64" => {
            if lower.contains("x86_64") || lower.contains("amd64") || lower.contains("x64") {
                score += 60;
            } else if lower.contains("aarch64")
                || lower.contains("arm64")
                || lower.contains("armhf")
                || lower.contains("armv7")
                || lower.contains("i686")
                || lower.contains("i386")
            {
                score -= 100;
            }
        }
        "aarch64" => {
            if lower.contains("aarch64") || lower.contains("arm64") {
                score += 60;
            } else if lower.contains("x86_64")
                || lower.contains("amd64")
                || lower.contains("x64")
                || lower.contains("i686")
                || lower.contains("i386")
            {
                score -= 100;
            }
        }
        _ => {
            if lower.contains(target_arch) {
                score += 60;
            }
        }
    }

    // Penalties for debug or development artifacts
    if lower.contains("debug") || lower.contains("dbg") || lower.contains("symbols") {
        score -= 50;
    }
    if lower.contains("untested") || lower.contains("sources") || lower.contains("devel") {
        score -= 30;
    }

    // Slight preference for standard release/stable names
    if lower.contains("stable") || lower.contains("release") {
        score += 10;
    }

    score
}

pub async fn fetch_available_appimages(
    client: &Octocrab,
    repo: &str,
) -> Result<Vec<GithubAssetInfo>> {
    let (owner, name) = normalize_repo(repo)?;
    let release = client
        .repos(&owner, &name)
        .releases()
        .get_latest()
        .await
        .with_context(|| format!("failed to fetch latest release for {owner}/{name}"))?;

    let arch = std::env::consts::ARCH;
    let mut assets: Vec<GithubAssetInfo> = release
        .assets
        .into_iter()
        .filter(|a| is_valid_appimage_asset(&a.name))
        .map(|a| GithubAssetInfo {
            name: a.name,
            size: a.size as u64,
            download_url: a.browser_download_url.to_string(),
        })
        .collect();

    // Sort by smart score descending, so best match is first in the list
    assets.sort_by_key(|a| std::cmp::Reverse(score_asset(&a.name, arch)));
    Ok(assets)
}

#[cfg(test)]
pub async fn fetch_latest_release(client: &Octocrab, repo: &str) -> Result<ReleaseInfo> {
    fetch_latest_release_with_selector(client, repo, None).await
}

fn looks_like_commit_hash(value: &str) -> bool {
    let value = value.trim();
    value.len() >= 7 && value.chars().all(|c| c.is_ascii_hexdigit())
}

async fn resolve_release_commit(
    client: &Octocrab,
    owner: &str,
    repo: &str,
    tag: &str,
    target_commitish: &str,
) -> Result<String> {
    // Some repositories use a full commit SHA as target_commitish.
    if looks_like_commit_hash(target_commitish) {
        return Ok(target_commitish.trim().to_string());
    }

    // Resolve the release tag to its commit. This is the authoritative path
    // for static tags such as "latest".
    if let Ok(commit) = client.commits(owner, repo).get(tag).await {
        return Ok(commit.sha);
    }

    // Fall back to target_commitish (usually a branch name) if the tag cannot
    // be resolved directly.
    let target = target_commitish.trim();
    if !target.is_empty() {
        let commit = client
            .commits(owner, repo)
            .get(target)
            .await
            .with_context(|| {
                format!("failed to resolve release commit for {owner}/{repo} at {target}")
            })?;
        return Ok(commit.sha);
    }

    anyhow::bail!("release {tag} has no resolvable commit hash")
}

pub async fn fetch_latest_release_with_selector(
    client: &Octocrab,
    repo: &str,
    manual_asset: Option<&str>,
) -> Result<ReleaseInfo> {
    let (owner, name) = normalize_repo(repo)?;

    let release = client
        .repos(&owner, &name)
        .releases()
        .get_latest()
        .await
        .with_context(|| format!("failed to fetch latest release for {owner}/{name}"))?;

    let arch = std::env::consts::ARCH;

    let valid_assets: Vec<_> = release
        .assets
        .iter()
        .filter(|a| is_valid_appimage_asset(&a.name))
        .collect();

    if valid_assets.is_empty() {
        anyhow::bail!(
            "no valid AppImage asset found in release {}",
            release.tag_name
        );
    }

    let asset = if let Some(pref) = manual_asset {
        let pref_trimmed = pref.trim();
        // 1. Exact match
        valid_assets
            .iter()
            .find(|a| a.name == pref_trimmed)
            // 2. Case-insensitive exact match
            .or_else(|| {
                valid_assets
                    .iter()
                    .find(|a| a.name.eq_ignore_ascii_case(pref_trimmed))
            })
            // 3. Substring / pattern match
            .or_else(|| {
                let pref_lower = pref_trimmed.to_lowercase();
                valid_assets
                    .iter()
                    .find(|a| a.name.to_lowercase().contains(&pref_lower))
            })
            // Fallback to highest scored asset
            .unwrap_or_else(|| {
                valid_assets
                    .iter()
                    .max_by_key(|a| score_asset(&a.name, arch))
                    .unwrap()
            })
    } else {
        // Smart mode: pick highest score
        valid_assets
            .iter()
            .max_by_key(|a| score_asset(&a.name, arch))
            .unwrap()
    };

    if asset.browser_download_url.scheme() != "https" {
        anyhow::bail!(
            "refusing to download from a non-HTTPS URL: {}",
            asset.browser_download_url
        );
    }

    let checksum_url = release
        .assets
        .iter()
        .find(|candidate| {
            is_checksum_for_asset(&candidate.name, &asset.name)
                && candidate.browser_download_url.scheme() == "https"
        })
        .map(|candidate| candidate.browser_download_url.to_string());

    let commit_hash = resolve_release_commit(
        client,
        &owner,
        &name,
        &release.tag_name,
        &release.target_commitish,
    )
    .await?;

    Ok(ReleaseInfo {
        tag: release.tag_name.clone(),
        download_url: asset.browser_download_url.to_string(),
        commit_hash,
        asset_size: asset.size.max(0) as u64,
        checksum_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_asset_matches_its_appimage() {
        assert!(is_checksum_for_asset(
            "MyApp-1.0-x86_64.AppImage.sha256",
            "MyApp-1.0-x86_64.AppImage"
        ));
        assert!(is_checksum_for_asset(
            "myapp-1.0-x86_64.appimage.SHA256SUM",
            "MyApp-1.0-x86_64.AppImage"
        ));
        assert!(!is_checksum_for_asset(
            "Other.sha256",
            "MyApp-1.0-x86_64.AppImage"
        ));
        assert!(!is_checksum_for_asset(
            "MyApp-1.0-x86_64.AppImage",
            "MyApp-1.0-x86_64.AppImage"
        ));
    }

    #[test]
    fn normalize_repo_accepts_common_forms() -> Result<()> {
        let cases = [
            (
                "obsidianmd/obsidian-releases",
                ("obsidianmd", "obsidian-releases"),
            ),
            (
                "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher",
                ("Heroic-Games-Launcher", "HeroicGamesLauncher"),
            ),
            (
                "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/releases",
                ("Heroic-Games-Launcher", "HeroicGamesLauncher"),
            ),
            (
                "https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/",
                ("Heroic-Games-Launcher", "HeroicGamesLauncher"),
            ),
            (
                "github.com/linuxdeploy/linuxdeploy.git",
                ("linuxdeploy", "linuxdeploy"),
            ),
        ];
        for (input, (o, n)) in cases {
            let got = normalize_repo(input)?;
            assert_eq!(got, (o.to_string(), n.to_string()), "input={input}");
        }
        Ok(())
    }

    #[test]
    fn test_is_valid_appimage_asset() {
        assert!(is_valid_appimage_asset("MyApp-1.0.0-x86_64.AppImage"));
        assert!(is_valid_appimage_asset("myapp.appimage"));
        assert!(is_valid_appimage_asset("Krita-5.2.2-x86_64-qt6.AppImage"));

        // Hashes, signatures, and zsync must be rejected
        assert!(!is_valid_appimage_asset(
            "MyApp-1.0.0-x86_64.AppImage.zsync"
        ));
        assert!(!is_valid_appimage_asset(
            "MyApp-1.0.0-x86_64.AppImage.sha256"
        ));
        assert!(!is_valid_appimage_asset("MyApp-1.0.0-x86_64.AppImage.asc"));
        assert!(!is_valid_appimage_asset("MyApp-1.0.0-x86_64.AppImage.sig"));
        assert!(!is_valid_appimage_asset(
            "MyApp-1.0.0-x86_64.AppImage.blockmap"
        ));
        assert!(!is_valid_appimage_asset("MyApp-1.0.0.tar.gz"));
        assert!(!is_valid_appimage_asset("MyApp.deb"));
    }

    #[test]
    fn commit_hash_detection_accepts_full_and_short_shas() {
        assert!(looks_like_commit_hash(
            "0123456789abcdef0123456789abcdef01234567"
        ));
        assert!(looks_like_commit_hash("0123456"));
        assert!(!looks_like_commit_hash("main"));
        assert!(!looks_like_commit_hash("latest"));
    }

    #[test]
    fn test_score_asset_architectures() {
        // On x86_64: x86_64 and amd64 score highest
        let x86_score = score_asset("MyApp-x86_64.AppImage", "x86_64");
        let amd64_score = score_asset("MyApp-amd64.AppImage", "x86_64");
        let aarch64_score = score_asset("MyApp-aarch64.AppImage", "x86_64");
        let debug_score = score_asset("MyApp-x86_64-debug.AppImage", "x86_64");

        assert!(x86_score > aarch64_score);
        assert!(amd64_score > aarch64_score);
        assert!(x86_score > debug_score);

        // On aarch64: aarch64 and arm64 score highest
        let arm_score = score_asset("MyApp-aarch64.AppImage", "aarch64");
        let arm64_score = score_asset("MyApp-arm64.AppImage", "aarch64");
        let intel_score = score_asset("MyApp-x86_64.AppImage", "aarch64");

        assert!(arm_score > intel_score);
        assert!(arm64_score > intel_score);
    }

    #[tokio::test]
    #[ignore = "requires GitHub network access"]
    async fn fetch_latest_release_real_repo() -> Result<()> {
        ensure_crypto_provider();
        let client = Octocrab::builder().build()?;
        let info = fetch_latest_release(&client, "obsidianmd/obsidian-releases").await?;
        assert!(!info.tag.is_empty(), "tag should not be empty");
        assert!(
            info.download_url.starts_with("http"),
            "download URL should be http(s): {}",
            info.download_url
        );
        assert!(
            !info.commit_hash.is_empty(),
            "commit hash should not be empty"
        );
        Ok(())
    }
}
