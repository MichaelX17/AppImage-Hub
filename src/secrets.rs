//! Local secret storage for the optional GitHub token.
//!
//! The token is encrypted with AES-256-GCM. The encryption key is derived from
//! machine-specific local data, so the encrypted file is not portable between
//! machines and the token is never written to `config.toml` in plain text.

use anyhow::{anyhow, Context, Result};
use ring::aead;
use ring::digest;
use ring::rand::{SecureRandom, SystemRandom};
use std::path::PathBuf;

const MAGIC: &[u8; 5] = b"AIHT1";
const NONCE_LEN: usize = 12;

fn token_path() -> PathBuf {
    crate::config::config_dir().join("github_token.enc")
}

fn derive_key() -> Result<[u8; 32]> {
    let machine_id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
    let user = std::env::var("USER").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();

    let mut ctx = digest::Context::new(&digest::SHA256);
    ctx.update(b"appimage-hub-github-token-v1");
    ctx.update(machine_id.trim().as_bytes());
    ctx.update(user.as_bytes());
    ctx.update(home.as_bytes());
    let hash = ctx.finish();

    let mut key = [0_u8; 32];
    key.copy_from_slice(hash.as_ref());
    Ok(key)
}

pub fn save_github_token(token: &str) -> Result<()> {
    let path = token_path();
    if token.trim().is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let mut nonce = [0_u8; NONCE_LEN];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| anyhow!("failed to generate encryption nonce"))?;

    let key = derive_key()?;
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key)
        .map_err(|_| anyhow!("failed to initialize token encryption"))?;
    let key = aead::LessSafeKey::new(unbound);

    let mut in_out = token.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::empty(),
        &mut in_out,
    )
    .map_err(|_| anyhow!("failed to encrypt GitHub token"))?;

    let mut data = Vec::with_capacity(MAGIC.len() + NONCE_LEN + in_out.len());
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&in_out);
    std::fs::write(&path, data).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn load_github_token() -> Option<String> {
    let data = std::fs::read(token_path()).ok()?;
    if data.len() <= MAGIC.len() + NONCE_LEN || &data[..MAGIC.len()] != MAGIC {
        return None;
    }

    let (nonce_bytes, ciphertext) = data[MAGIC.len()..].split_at(NONCE_LEN);
    let mut nonce = [0_u8; NONCE_LEN];
    nonce.copy_from_slice(nonce_bytes);

    let key = derive_key().ok()?;
    let unbound = aead::UnboundKey::new(&aead::AES_256_GCM, &key).ok()?;
    let key = aead::LessSafeKey::new(unbound);
    let mut in_out = ciphertext.to_vec();
    let plaintext = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::empty(),
            &mut in_out,
        )
        .ok()?;
    String::from_utf8(plaintext.to_vec()).ok()
}
