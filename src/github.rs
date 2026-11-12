use anyhow::{anyhow, Context, Result};
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
    pub asset_name: String,
    pub download_url: String,
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

pub async fn fetch_latest_release(client: &Octocrab, repo: &str) -> Result<ReleaseInfo> {
    let (owner, name) = normalize_repo(repo)?;

    let release = client
        .repos(&owner, &name)
        .releases()
        .get_latest()
        .await
        .with_context(|| format!("failed to fetch latest release for {owner}/{name}"))?;

    let arch = std::env::consts::ARCH;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.to_lowercase().contains("appimage") && a.name.contains(arch))
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|a| a.name.to_lowercase().contains("appimage"))
        })
        .ok_or_else(|| anyhow!("no AppImage asset found in release {}", release.tag_name))?;

    Ok(ReleaseInfo {
        tag: release.tag_name.clone(),
        asset_name: asset.name.clone(),
        download_url: asset.browser_download_url.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[tokio::test]
    async fn fetch_latest_release_real_repo() -> Result<()> {
        ensure_crypto_provider();
        let client = Octocrab::builder().build()?;
        let info = fetch_latest_release(&client, "obsidianmd/obsidian-releases").await?;
        assert!(!info.tag.is_empty(), "tag should not be empty");
        assert!(
            info.asset_name.to_lowercase().contains("appimage"),
            "asset should be an AppImage: {}",
            info.asset_name
        );
        assert!(
            info.download_url.starts_with("http"),
            "download URL should be http(s): {}",
            info.download_url
        );
        Ok(())
    }
}
